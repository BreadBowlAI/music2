//! Hosts CLAP plugins.
//!
//! The plugins producers already own become graph nodes: audio in and out, notes in for
//! instruments, and every automatable parameter as a control input. A plugin's state is saved and
//! loaded as bytes, which the project stores as data.
//!
//! The engine reaches plugins only through [`PluginHost`] (P11, HOST-1). [`InProcess`], today's
//! host, runs them in this process, which trusts their native code like any DAW (the host process
//! of HOST-3 comes later). Its threads follow CLAP's model, and its thread checks answer truthfully
//! (HOST-2):
//! - every main-thread call (create, activate, state, parameters, `on_main_thread`, deactivate,
//!   destroy) runs on the plugin main thread, which the program installs: see [`main_thread`];
//! - `process` and `reset` run on whichever thread renders (the live engine's audio thread, or an
//!   offline render's), which holds the instance's audio role while it does; they never allocate,
//!   lock or wait (`src/audio.rs`, RENDER-28);
//! - a plugin's requests from any thread (`request_callback`, `request_restart`, `latency.changed`
//!   and others) only raise flags, which the main thread sweeps every 5 ms: `on_main_thread` never
//!   runs inside `process`;
//! - libraries load on the thread that asks for them, never the main thread, once per bundle.
//!
//! Plugins are impure: the runtime never caches their output.

mod audio;
mod host;
mod inprocess;
mod main_thread;
pub mod probe;
mod remote;

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use clap_sys::entry::clap_plugin_entry;
use clap_sys::factory::plugin_factory::*;
use clap_sys::version::clap_version_is_compatible;

pub use host::{
    Blob, Events, Handle, HostEvent, HostEvents, MAX_CALL_EVENTS, MainCall, Owner, Pending, PluginHost, RenderMode, Reply, SharedBuffers, Status,
};
pub use inprocess::{InProcess, MAX_INSTANCES};
pub use remote::child::serve as serve_host;
pub use remote::{FLAG as HOST_FLAG, HANG, Remote};
pub use main_thread::{DRAIN, MainThread, main_thread, on_main_thread, run_main, test_main_thread, use_app_main_thread};

/// Where plugins are installed on this machine: `CLAP_PATH`, then the OS's CLAP folders.
pub fn search_paths() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(p) = std::env::var_os("CLAP_PATH") {
        v.extend(std::env::split_paths(&p));
    }
    v.extend(m2_os::dirs::clap());
    v
}

/// A plugin found on disk.
#[derive(Clone, Debug)]
pub struct Found {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub features: Vec<String>,
    pub bundle: PathBuf,
}

/// What a plugin instance offers, read after `init`.
#[derive(Clone, Debug)]
pub struct Info {
    pub found: Found,
    /// Channel count of the main audio input, if any.
    pub audio_in: Option<u32>,
    /// Channel count of the main audio output, if any.
    pub audio_out: Option<u32>,
    pub note_in: bool,
    pub params: Vec<Param>,
    /// Measured when it was last activated (0 before).
    pub latency: u32,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub id: u32,
    pub name: String,
    pub module: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub stepped: bool,
}

// ---------------------------------------------------------------- libraries

pub(crate) struct Library {
    /// None for the probe, which is linked in.
    _lib: Option<libloading::Library>,
    _entry: *const clap_plugin_entry,
    factory: *const clap_plugin_factory,
    bundle: PathBuf,
}

// SAFETY: the entry and factory are process-wide tables that CLAP requires to be thread-safe; the
// library stays loaded for the life of the process.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

pub(crate) fn cstr(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

pub(crate) fn fixed(s: &[c_char]) -> String {
    let bytes: Vec<u8> = s.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// One bundle's library, loaded at most once.
type Cell = Arc<OnceLock<Result<Arc<Library>, String>>>;

/// Loaded libraries, kept for the life of the process (CLAP's `deinit` is never called, which is
/// allowed when the process exits). The lock is held only to find a bundle's cell: loading runs in
/// the cell, so a slow bundle holds up only the threads that want that bundle.
fn libraries() -> &'static Mutex<HashMap<PathBuf, Cell>> {
    static L: OnceLock<Mutex<HashMap<PathBuf, Cell>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

fn binary_in(bundle: &Path) -> Result<PathBuf> {
    if bundle.is_file() {
        return Ok(bundle.to_path_buf());
    }
    let dir = bundle.join("Contents/MacOS");
    let mut files = std::fs::read_dir(&dir).with_context(|| format!("{} is not a CLAP bundle", bundle.display()))?.flatten().map(|e| e.path());
    files.next().ok_or_else(|| anyhow!("{} has no binary", dir.display()))
}

/// The library of `bundle`, loaded on the calling thread the first time (CLAP: `entry.init` from
/// any thread, once, never concurrently). A load that failed is tried again next time.
pub(crate) fn library(bundle: &Path) -> Result<Arc<Library>> {
    if probe::is_bundle(bundle) {
        return Ok(probe::library());
    }
    let cell = libraries().lock().unwrap_or_else(|e| e.into_inner()).entry(bundle.to_path_buf()).or_default().clone();
    match cell.get_or_init(|| load(bundle).map_err(|e| format!("{e:#}"))) {
        Ok(l) => Ok(l.clone()),
        Err(e) => {
            let mut libs = libraries().lock().unwrap_or_else(|e| e.into_inner());
            if libs.get(bundle).is_some_and(|c| Arc::ptr_eq(c, &cell)) {
                libs.remove(bundle);
            }
            Err(anyhow!("{e}"))
        }
    }
}

fn load(bundle: &Path) -> Result<Arc<Library>> {
    let bin = binary_in(bundle)?;
    // SAFETY: loading a plugin runs its native code; plugins are trusted like in any DAW.
    let lib = unsafe { libloading::Library::new(&bin) }.with_context(|| format!("loading {}", bin.display()))?;
    let entry = unsafe { *lib.get::<*const clap_plugin_entry>(b"clap_entry\0").with_context(|| format!("{} has no clap_entry", bin.display()))? };
    let e = unsafe { &*entry };
    if !clap_version_is_compatible(e.clap_version) {
        bail!("{} uses an incompatible CLAP version", bundle.display());
    }
    let path = CString::new(bundle.to_string_lossy().as_bytes())?;
    if !unsafe { (e.init.context("no init")?)(path.as_ptr()) } {
        bail!("{} failed to initialise", bundle.display());
    }
    let factory = unsafe { (e.get_factory.context("no get_factory")?)(CLAP_PLUGIN_FACTORY_ID.as_ptr()) } as *const clap_plugin_factory;
    if factory.is_null() {
        bail!("{} has no plugin factory", bundle.display());
    }
    Ok(Arc::new(Library { _lib: Some(lib), _entry: entry, factory, bundle: bundle.to_path_buf() }))
}

impl Library {
    fn plugins(&self) -> Vec<Found> {
        let f = unsafe { &*self.factory };
        let Some(count) = f.get_plugin_count else { return vec![] };
        let Some(desc) = f.get_plugin_descriptor else { return vec![] };
        (0..unsafe { count(self.factory) })
            .filter_map(|i| {
                let d = unsafe { desc(self.factory, i) };
                if d.is_null() {
                    return None;
                }
                let d = unsafe { &*d };
                let mut features = vec![];
                if !d.features.is_null() {
                    let mut p = d.features;
                    while !unsafe { *p }.is_null() {
                        features.push(cstr(unsafe { *p }));
                        p = unsafe { p.add(1) };
                    }
                }
                Some(Found {
                    id: cstr(d.id),
                    name: cstr(d.name),
                    vendor: cstr(d.vendor),
                    version: cstr(d.version),
                    features,
                    bundle: self.bundle.clone(),
                })
            })
            .collect()
    }
}

/// The process's plugin host: the host process ([`Remote`]) in every program, which `run_main` and
/// `use_app_main_thread` install; [`InProcess`] where none was installed (tests, and inside the
/// host process itself).
pub fn host() -> Arc<dyn PluginHost> {
    HOST.get_or_init(|| Arc::new(InProcess)).clone()
}

static HOST: OnceLock<Arc<dyn PluginHost>> = OnceLock::new();

/// Make the host process the process's host, before anything asked for one.
pub(crate) fn use_remote(r: Remote) {
    if REMOTE.set(r.clone()).is_ok() {
        let _ = HOST.set(Arc::new(r));
    }
}

static REMOTE: OnceLock<Remote> = OnceLock::new();

/// The host process's id while it runs (a program's, through [`run_main`] or
/// [`use_app_main_thread`]).
pub fn host_process() -> Option<u32> {
    REMOTE.get().and_then(Remote::process_id)
}

/// The host process's instances alive (0 in a program that installed none).
pub(crate) fn remote_live() -> u64 {
    REMOTE.get().map_or(0, |r| r.live())
}

/// Quit: end the host process, once the program's instances are destroyed. Nothing when the
/// program hosts none.
pub fn end_host_process() {
    if let Some(r) = REMOTE.get() {
        r.shutdown();
    }
}

/// Every plugin installed, through the process's host: in a program, the host process loads them,
/// never this one (HOST-3). Bundles that fail to load are reported, not fatal.
pub fn scan() -> (Vec<Found>, Vec<String>) {
    host().scan()
}

/// Every plugin in the search paths (and the probe, once installed), loaded in this process.
/// Bundles that fail to load are reported, not fatal.
pub(crate) fn scan_here() -> (Vec<Found>, Vec<String>) {
    let (mut found, mut errors) = (vec![], vec![]);
    for dir in search_paths() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut bundles: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "clap")).collect();
        bundles.sort();
        for b in bundles {
            match library(&b) {
                Ok(l) => found.extend(l.plugins()),
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
    }
    if probe::installed() {
        found.extend(probe::library().plugins());
    }
    (found, errors)
}

/// Find a plugin by id, through the process's host ([`scan`]).
pub fn find(id: &str) -> Result<Found> {
    let (found, _) = scan();
    found.into_iter().find(|f| f.id == id).ok_or_else(|| anyhow!("no CLAP plugin `{id}` is installed (looked in {:?})", search_paths()))
}

// ---------------------------------------------------------------- events

/// Something that happens at a sample offset within a `process` call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    NoteOn { time: u32, id: i32, key: i16, velocity: f64 },
    NoteOff { time: u32, id: i32, key: i16 },
    /// End a note at once, with no release (a panic).
    NoteChoke { time: u32, id: i32, key: i16 },
    /// Tuning in semitones for one note (for pitches between keys).
    Tuning { time: u32, id: i32, key: i16, semitones: f64 },
    Param { time: u32, id: u32, value: f64 },
}

impl Event {
    pub fn time(&self) -> u32 {
        match *self {
            Event::NoteOn { time, .. }
            | Event::NoteOff { time, .. }
            | Event::NoteChoke { time, .. }
            | Event::Tuning { time, .. }
            | Event::Param { time, .. } => time,
        }
    }
}

/// The transport plugins get from the engine (HOST-6), for each `process` call: the song
/// position at its first frame, whether the song plays and loops, the meter, and the engine
/// clock as `steady_time`. A call never spans a jump: the engine splits the block there.
#[derive(Clone, Copy, Debug, Default)]
pub struct Transport {
    pub beats: f64,
    pub seconds: f64,
    pub bpm: f64,
    pub playing: bool,
    /// The loop, when one is on: in beats and in seconds.
    pub looping: Option<((f64, f64), (f64, f64))>,
    /// Time signature: beats per bar and the note value of a beat.
    pub tsig: (u16, u16),
    /// The beat the current bar started on, and its number (0 is the first).
    pub bar_start: f64,
    pub bar: i32,
    /// Samples since the engine started: never jumps, never resets (RENDER-6).
    pub steady: i64,
}

/// The address of each of a plugin's parameters in code (FMT-18): `<name>_<id>`, its display
/// name in lowercase words joined by `_`, then its CLAP parameter id (`dry_wet_12`). The id
/// makes it unique and is all that binds it; the name part is a label that a plugin update may
/// change. A parameter without a name is `param_<id>`.
pub fn param_port_names(params: &[Param]) -> Vec<String> {
    params.iter().map(|p| format!("{}_{}", param_label(&p.name), p.id)).collect()
}

/// The name part of a parameter address: lowercase ASCII letters and digits, every run of
/// anything else one `_`, trimmed; `param` if nothing is left.
pub fn param_label(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('_') {
            s.push('_');
        }
    }
    match s.trim_matches('_') {
        "" => "param".into(),
        t => t.into(),
    }
}

/// A parameter address split into its name part and its CLAP id: `dry_wet_12` → (`dry_wet`, 12).
/// `None` for a port written without an id (`dry_wet`), or one that isn't a parameter (`in`).
pub fn param_address(port: &str) -> Option<(&str, u32)> {
    let (name, id) = port.rsplit_once('_')?;
    // `u32::from_str` takes a leading `+`; an address is digits only.
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(())?;
    Some((name, id.parse().ok()?))
}

#[cfg(test)]
mod tests;
