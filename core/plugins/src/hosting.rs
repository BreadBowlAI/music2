//! The CLAP host itself (the `clap` feature, HOST-1): where plugins are installed, their
//! libraries, loaded at most once, and the process's host. Without it (the web), the crate is the
//! types the engine names, and nothing loads or runs plugin code.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use clap_sys::entry::clap_plugin_entry;
use clap_sys::factory::plugin_factory::*;
use clap_sys::version::clap_version_is_compatible;

use crate::{Found, InProcess, PluginHost, Remote, probe};

/// Where plugins are installed on this machine: `CLAP_PATH`, then the OS's CLAP folders.
pub fn search_paths() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(p) = std::env::var_os("CLAP_PATH") {
        v.extend(std::env::split_paths(&p));
    }
    v.extend(m2_os::dirs::clap());
    v
}

pub(crate) struct Library {
    /// None for the probe, which is linked in.
    pub(crate) _lib: Option<libloading::Library>,
    pub(crate) entry: *const clap_plugin_entry,
    pub(crate) factory: *const clap_plugin_factory,
    pub(crate) bundle: PathBuf,
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
    let mut files =
        std::fs::read_dir(&dir).with_context(|| format!("{} is not a CLAP bundle", bundle.display()))?.flatten().map(|e| e.path());
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
    let entry =
        unsafe { *lib.get::<*const clap_plugin_entry>(b"clap_entry\0").with_context(|| format!("{} has no clap_entry", bin.display()))? };
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
    Ok(Arc::new(Library { _lib: Some(lib), entry, factory, bundle: bundle.to_path_buf() }))
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

/// The program's host processes run at background priority from now on ([`Remote::background`]):
/// `m2 listen`'s (AGENT-7). Nothing in a program that installed none.
pub fn background_host() {
    if let Some(r) = REMOTE.get() {
        r.background();
    }
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
        let mut bundles: Vec<PathBuf> =
            entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "clap")).collect();
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
