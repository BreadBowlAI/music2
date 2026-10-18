//! A plugin's own presets (HOST-8): CLAP's preset discovery, and `preset-load`. Both run where the
//! plugin runs: in a program, inside the host process (HOST-3). The search runs on the calling
//! thread, never the main thread (CLAP's discovery factory is thread-safe, and a long search must
//! not hold up the main thread's other work); `preset-load` runs on the main thread (HOST-2).
//!
//! Discovery: each provider of the plugin's preset-discovery factory is made with an indexer that
//! keeps the locations and file types it declares; then each location is read through the
//! provider's `get_metadata`: once for one inside the plugin, once per file for a file or a
//! folder (walked in name order for the file types it declared, every file when it declared
//! none; [`MAX_PRESET_FILES`] at most). A preset is kept when it names this plugin, or names none.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr::null;

use anyhow::{Result, bail};
use clap_sys::ext::preset_load::*;
use clap_sys::factory::preset_discovery::*;
use clap_sys::plugin::clap_plugin;
use clap_sys::timestamp::clap_timestamp;
use clap_sys::universal_plugin_id::clap_universal_plugin_id;
use clap_sys::version::CLAP_VERSION;

use crate::host::{MAX_PRESET_FILES, Offered, PresetAt};
use crate::inprocess::ext;
use crate::{Library, cstr};

/// What a provider's indexer heard while it was made: its locations (kind, path, as given) and the
/// extensions of its file types.
#[derive(Default)]
struct Declared {
    locations: Vec<(u32, Option<String>)>,
    extensions: Vec<String>,
}

/// What one `get_metadata` call said: each preset begun, with the plugins it names.
#[derive(Default)]
struct Heard {
    presets: Vec<(String, Option<String>, Vec<String>)>,
}

unsafe fn declared<'a>(i: *const clap_preset_discovery_indexer) -> &'a mut Declared {
    unsafe { &mut *((*i).indexer_data as *mut Declared) }
}

unsafe extern "C" fn declare_filetype(i: *const clap_preset_discovery_indexer, ft: *const clap_preset_discovery_filetype) -> bool {
    if !ft.is_null() {
        let ext = cstr(unsafe { (*ft).file_extension });
        unsafe { declared(i) }.extensions.push(ext.trim_start_matches('.').to_string());
    }
    true
}

unsafe extern "C" fn declare_location(i: *const clap_preset_discovery_indexer, l: *const clap_preset_discovery_location) -> bool {
    if !l.is_null() {
        let l = unsafe { &*l };
        let at = (!l.location.is_null()).then(|| cstr(l.location));
        unsafe { declared(i) }.locations.push((l.kind, at));
    }
    true
}

unsafe extern "C" fn declare_soundpack(_: *const clap_preset_discovery_indexer, _: *const clap_preset_discovery_soundpack) -> bool {
    true
}

unsafe extern "C" fn indexer_extension(_: *const clap_preset_discovery_indexer, _: *const c_char) -> *const c_void {
    null()
}

unsafe fn heard<'a>(r: *const clap_preset_discovery_metadata_receiver) -> &'a mut Heard {
    unsafe { &mut *((*r).receiver_data as *mut Heard) }
}

unsafe extern "C" fn on_error(_: *const clap_preset_discovery_metadata_receiver, _: i32, _: *const c_char) {}

unsafe extern "C" fn begin_preset(r: *const clap_preset_discovery_metadata_receiver, name: *const c_char, key: *const c_char) -> bool {
    let key = (!key.is_null()).then(|| cstr(key));
    unsafe { heard(r) }.presets.push((cstr(name), key, vec![]));
    true
}

unsafe extern "C" fn add_plugin_id(r: *const clap_preset_discovery_metadata_receiver, id: *const clap_universal_plugin_id) {
    if id.is_null() {
        return;
    }
    let id = unsafe { &*id };
    if cstr(id.abi) != "clap" {
        return;
    }
    if let Some(p) = unsafe { heard(r) }.presets.last_mut() {
        p.2.push(cstr(id.id));
    }
}

unsafe extern "C" fn set_text(_: *const clap_preset_discovery_metadata_receiver, _: *const c_char) {}
unsafe extern "C" fn set_flags(_: *const clap_preset_discovery_metadata_receiver, _: u32) {}
unsafe extern "C" fn set_timestamps(_: *const clap_preset_discovery_metadata_receiver, _: clap_timestamp, _: clap_timestamp) {}
unsafe extern "C" fn add_extra_info(_: *const clap_preset_discovery_metadata_receiver, _: *const c_char, _: *const c_char) {}

/// The plugin's preset-discovery factory, when it has one.
fn factory(lib: &Library) -> Option<&clap_preset_discovery_factory> {
    // SAFETY: the entry is a process-wide table of a library loaded for good.
    let get = unsafe { (*lib.entry).get_factory }?;
    [CLAP_PRESET_DISCOVERY_FACTORY_ID, CLAP_PRESET_DISCOVERY_FACTORY_ID_COMPAT].into_iter().find_map(|id| {
        let f = unsafe { get(id.as_ptr()) } as *const clap_preset_discovery_factory;
        (!f.is_null()).then(|| unsafe { &*f })
    })
}

/// The files of a file location: the file itself, or the folder's files with one of `extensions`
/// (any, when none), in name order, until `room` are found.
fn files(at: &Path, extensions: &[String], room: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= room {
        return;
    }
    if at.is_file() {
        out.push(at.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(at) else { return };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if out.len() >= room {
            return;
        }
        if p.is_dir() {
            files(&p, extensions, room, out);
        } else if extensions.is_empty() || p.extension().is_some_and(|x| extensions.iter().any(|e| x.eq_ignore_ascii_case(e.as_str()))) {
            out.push(p);
        }
    }
}

/// The presets plugin `id` of `lib` offers (on the host's main thread).
pub(crate) fn discover(lib: &Library, id: &str) -> Result<Vec<Offered>> {
    let Some(f) = factory(lib) else { return Ok(vec![]) };
    let (Some(count), Some(desc), Some(create)) = (f.count, f.get_descriptor, f.create) else { return Ok(vec![]) };
    let mut out = vec![];
    let mut read = 0;
    for i in 0..unsafe { count(f) } {
        let d = unsafe { desc(f, i) };
        if d.is_null() || unsafe { (*d).id }.is_null() {
            continue;
        }
        // Reached only through these pointers while the plugin holds them.
        let mut index = Declared::default();
        let index: *mut Declared = &mut index;
        let indexer = clap_preset_discovery_indexer {
            clap_version: CLAP_VERSION,
            name: c"music2".as_ptr(),
            vendor: c"music2".as_ptr(),
            url: c"".as_ptr(),
            version: c"0.1.0".as_ptr(),
            indexer_data: index as *mut c_void,
            declare_filetype: Some(declare_filetype),
            declare_location: Some(declare_location),
            declare_soundpack: Some(declare_soundpack),
            get_extension: Some(indexer_extension),
        };
        let p = unsafe { create(f, &indexer, (*d).id) };
        if p.is_null() {
            continue;
        }
        let pr = unsafe { &*p };
        if pr.init.is_some_and(|init| unsafe { init(p) }) {
            let mut got = Heard::default();
            let got: *mut Heard = &mut got;
            let receiver = clap_preset_discovery_metadata_receiver {
                receiver_data: got as *mut c_void,
                on_error: Some(on_error),
                begin_preset: Some(begin_preset),
                add_plugin_id: Some(add_plugin_id),
                set_soundpack_id: Some(set_text),
                set_flags: Some(set_flags),
                add_creator: Some(set_text),
                set_description: Some(set_text),
                set_timestamps: Some(set_timestamps),
                add_feature: Some(set_text),
                add_extra_info: Some(add_extra_info),
            };
            let locations = std::mem::take(unsafe { &mut (*index).locations });
            for (kind, at) in locations {
                let places: Vec<PresetAt> = match (kind, at) {
                    (CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN, _) => vec![PresetAt::Plugin],
                    (CLAP_PRESET_DISCOVERY_LOCATION_FILE, Some(at)) => {
                        let mut found = vec![];
                        files(Path::new(&at), unsafe { &(*index).extensions }, MAX_PRESET_FILES - read, &mut found);
                        read += found.len();
                        found.into_iter().map(PresetAt::File).collect()
                    }
                    _ => vec![],
                };
                for place in places {
                    let file = match &place {
                        PresetAt::File(p) => CString::new(p.to_string_lossy().as_bytes()).ok(),
                        PresetAt::Plugin => None,
                    };
                    let kind = if file.is_some() { CLAP_PRESET_DISCOVERY_LOCATION_FILE } else { CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN };
                    unsafe { (*got).presets.clear() };
                    let Some(get) = pr.get_metadata else { continue };
                    if !unsafe { get(p, kind, file.as_ref().map_or(null(), |c| c.as_ptr()), &receiver) } {
                        continue;
                    }
                    for (name, key, ids) in std::mem::take(unsafe { &mut (*got).presets }) {
                        if ids.is_empty() || ids.iter().any(|x| x == id) {
                            out.push(Offered { name, location: place.clone(), key });
                        }
                    }
                }
            }
        }
        if let Some(destroy) = pr.destroy {
            unsafe { destroy(p) };
        }
    }
    Ok(out)
}

/// Load one of the plugin's own presets into `plugin` (on the host's main thread).
pub(crate) fn load(plugin: *const clap_plugin, location: &PresetAt, key: Option<&str>) -> Result<()> {
    let ext = ext::<clap_plugin_preset_load>(plugin, CLAP_EXT_PRESET_LOAD).or_else(|| ext(plugin, CLAP_EXT_PRESET_LOAD_COMPAT));
    let Some(from) = ext.and_then(|e| e.from_location) else { bail!("this plugin can't load its own presets") };
    let (kind, at) = match location {
        PresetAt::Plugin => (CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN, None),
        PresetAt::File(p) => (CLAP_PRESET_DISCOVERY_LOCATION_FILE, Some(CString::new(p.to_string_lossy().as_bytes())?)),
    };
    let key = key.map(CString::new).transpose()?;
    let c = |s: &Option<CString>| s.as_deref().map_or(null(), CStr::as_ptr);
    if !unsafe { from(plugin, kind, c(&at), c(&key)) } {
        bail!("the plugin couldn't load that preset");
    }
    Ok(())
}
