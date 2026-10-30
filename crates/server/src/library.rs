//! The library on the server (LIB-1 to LIB-6): its items for the open song, and previews, kept in
//! the cache folder.
//!
//! The items come from the open song's build (its packages, the store's other packs and its own
//! components), or std's and the store's packs with no song open; each package's presets and yours;
//! the samples of each package and of your sample folders ([`SampleIndex`]: a package's indexed at
//! once, a folder of yours in the background); and the plugins the app has listed, never a scan
//! (AGENT-11), nor waiting for one. A sample's preview is its own file. [`preview`] makes an item's preview with
//! `m2_editor::library::preview`, which renders on a thread of its own at background priority
//! (never the audio thread or the session's), with std and the item's own package loaded: kept in
//! the cache as `<item>-<what it renders>.wav`, each a hash, so a second ask renders nothing and a
//! changed preset, package or music2 renders anew, and the item's older preview goes. One render of
//! an item at a time: a second ask waits for the first, then finds its file; an answer is read
//! before the next render of the item can replace it. Your presets and the cache are the folders
//! the server keeps ([`Folders`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use m2_editor::library::samples::{self, Indexed, Root};
use m2_editor::library::{self, Item, Kind, Library, Plugin, Sources, preview as made};
use parking_lot::Mutex;

use crate::AppState;

/// The previews being made: one lock per item, so a second ask waits for the first's render.
#[derive(Default)]
pub struct Previews {
    making: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// Where the library reads your presets and the store, and keeps its previews and what it measured
/// of samples (LIB-1, LIB-3, LIB-5), as the server keeps them ([`AppState::keep_library`]). None of
/// one: no presets of yours, no packs with no song open, no previews, no samples' cache.
#[derive(Clone, Debug, Default)]
pub struct Folders {
    /// Your Presets folder.
    pub presets: Option<PathBuf>,
    /// Where previews are kept: `previews` in music2's cache folder.
    pub previews: Option<PathBuf>,
    /// The store of installed packs (PACK-6), whose packs the library offers with no song open.
    pub packs: Option<PathBuf>,
    /// Where what was measured of samples is kept: `samples` in music2's cache folder.
    pub samples: Option<PathBuf>,
}

impl Folders {
    /// This user's: the Presets folder (`m2_os::dirs::presets`), the store (`m2_os::dirs::packs`),
    /// and `previews` and `samples` in music2's cache folder (`m2_os::dirs::cache`).
    pub fn of_this_user() -> Folders {
        let cache = m2_os::dirs::cache();
        Folders {
            presets: m2_os::dirs::presets(),
            previews: cache.as_ref().map(|c| c.join("previews")),
            packs: m2_os::dirs::packs(),
            samples: cache.map(|c| c.join("samples")),
        }
    }
}

/// How long a folder's index is used before it is made again in the background, while the old one
/// is used: a folder of yours.
const FRESH: Duration = Duration::from_secs(60);
/// A package's, which costs a walk and a look in the cache.
const FRESH_PACKAGE: Duration = Duration::from_secs(10);
/// How long asking for one sample (its preview, adding it) waits for an index running.
const WAIT_FOR_INDEX: Duration = Duration::from_secs(30);

/// The samples indexed so far (LIB-4, LIB-5), by folder: a package's `samples/`, or a folder of
/// yours. Every index runs in the background, one at a time for each folder; a search uses the
/// last one meanwhile, and never waits.
#[derive(Default)]
pub struct SampleIndex {
    kept: Mutex<HashMap<Of, Kept>>,
    /// Counts the indexes running, for [`SampleIndex::settled`].
    running: Mutex<usize>,
    done: parking_lot::Condvar,
}

/// What an index is of: a package's folder, or a folder of yours (the same folder may be both).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Of {
    Package(PathBuf),
    Folder(PathBuf),
}

struct Kept {
    indexed: Option<Arc<Indexed>>,
    at: Instant,
    indexing: bool,
    /// Set to stop the index running (the folder was removed): each index has its own.
    stop: Arc<AtomicBool>,
}

/// An index running, in its thread. However it ends, a panic too, its folder is no longer being
/// indexed and the count of those running drops when it goes, so no folder stays "looking…" for
/// good (D11); what it made is kept by [`Running::finish`] only for the folder it was started for.
struct Running {
    index: Arc<SampleIndex>,
    of: Of,
    stop: Arc<AtomicBool>,
}

impl Running {
    /// Keep what the index made: not when it was stopped; and for a folder that isn't there, only
    /// when nothing was listed before (LIB-5: its last index stays).
    fn finish(&self, made: Indexed) {
        if made.stopped {
            return;
        }
        let mut kept = self.index.kept.lock();
        if let Some(k) = kept.get_mut(&self.of).filter(|k| Arc::ptr_eq(&k.stop, &self.stop)) {
            k.at = Instant::now();
            if !made.missing || k.indexed.is_none() {
                k.indexed = Some(Arc::new(made));
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(k) = self.index.kept.lock().get_mut(&self.of).filter(|k| Arc::ptr_eq(&k.stop, &self.stop)) {
            k.indexing = false;
        }
        *self.index.running.lock() -= 1;
        self.index.done.notify_all();
    }
}

impl SampleIndex {
    /// The samples of `of` as indexed so far, and whether an index of it is running, never
    /// waiting: `job` is started in the background, at background priority, when there is none or
    /// the last is `fresh` or older, and no index of it runs.
    pub(crate) fn get(
        self: &Arc<Self>,
        of: Of,
        fresh: Duration,
        job: impl FnOnce(&AtomicBool) -> Indexed + Send + 'static,
    ) -> (Option<Arc<Indexed>>, bool) {
        let (indexed, start) = {
            let mut kept = self.kept.lock();
            let k =
                kept.entry(of.clone()).or_insert_with(|| Kept { indexed: None, at: Instant::now(), indexing: false, stop: Arc::default() });
            let start = !k.indexing && (k.indexed.is_none() || k.at.elapsed() >= fresh);
            if start {
                k.indexing = true;
                k.stop = Arc::default();
                *self.running.lock() += 1;
            }
            (k.indexed.clone(), start.then(|| k.stop.clone()))
        };
        let Some(stop) = start else { return (indexed, self.indexing_of(&of)) };
        // Started with no lock held: a thread that can't start drops its guard here, which takes it.
        let running = Running { index: self.clone(), of: of.clone(), stop };
        let started = std::thread::Builder::new().name("music2-samples".into()).spawn(move || {
            let _ = m2_os::thread::background();
            let made = job(&running.stop);
            running.finish(made);
        });
        (indexed, started.is_ok() && self.indexing_of(&of))
    }

    pub(crate) fn indexing_of(&self, of: &Of) -> bool {
        self.kept.lock().get(of).is_some_and(|k| k.indexing)
    }

    /// Package `name`'s samples, in `dir`, as indexed so far, with the cache in `cache`, and
    /// whether an index of them is running ([`SampleIndex::get`]).
    pub fn package(self: &Arc<Self>, name: &str, dir: &Path, cache: Option<&Path>) -> (Option<Arc<Indexed>>, bool) {
        let (name, d, cache) = (name.to_string(), dir.to_path_buf(), cache.map(Path::to_path_buf));
        self.get(Of::Package(dir.to_path_buf()), FRESH_PACKAGE, move |stop| {
            samples::index_until(Root::Package { name: &name, dir: &d }, cache.as_deref(), stop)
        })
    }

    /// Folder `dir`'s samples as indexed so far, and whether an index of it is running (LIB-5,
    /// [`SampleIndex::get`]).
    pub fn folder(self: &Arc<Self>, dir: &Path, cache: Option<&Path>) -> (Option<Arc<Indexed>>, bool) {
        let (d, cache) = (dir.to_path_buf(), cache.map(Path::to_path_buf));
        self.get(Of::Folder(dir.to_path_buf()), FRESH, move |stop| samples::index_until(Root::Folder(&d), cache.as_deref(), stop))
    }

    /// How many indexes are running.
    pub fn indexing(&self) -> usize {
        *self.running.lock()
    }

    /// Wait until no folder is being indexed, up to `limit`: whether none is.
    pub fn settled(&self, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        let mut running = self.running.lock();
        while *running > 0 {
            if self.done.wait_until(&mut running, until).timed_out() {
                return *running == 0;
            }
        }
        true
    }

    /// Forget folder `dir` of yours: it was removed from the library. Its index running stops
    /// between two files, and what it made is never kept.
    pub fn forget(&self, dir: &Path) {
        if let Some(k) = self.kept.lock().remove(&Of::Folder(dir.to_path_buf())) {
            k.stop.store(true, Ordering::Relaxed);
        }
    }
}

/// One of your folders as the library shows it: how many samples it has, how many files were
/// skipped (not WAV) and don't read, and whether it is being indexed now, or isn't there.
#[derive(Clone, Debug, serde::Serialize)]
pub struct FolderShown {
    pub path: PathBuf,
    pub samples: usize,
    pub not_wav: usize,
    pub unreadable: usize,
    pub indexing: bool,
    pub missing: bool,
}

/// A preview, as [`preview`] answers it: its WAV, read while the item's lock was held, its file,
/// and whether this ask rendered it.
#[derive(Debug)]
pub struct Preview {
    pub wav: Vec<u8>,
    pub file: PathBuf,
    pub rendered: bool,
}

/// std, found as the app finds it: what a build loads with no song open.
fn std_only() -> Vec<(String, PathBuf)> {
    vec![("std".to_string(), crate::resources::std_dir())]
}

/// std and the packs of the store in `store` the lists offer (PACK-11): the packages with no song
/// open.
fn without_song(store: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut packages = std_only();
    if let Some(store) = store {
        packages.extend(library::store_packs(store, &packages[0].1));
    }
    packages
}

/// How old a part file of another render must be to be taken for one that failed: a render writes
/// its part file in well under a second.
const STALE_PART: Duration = Duration::from_secs(60);

/// The plugins in the app's kept list (`GET /api/plugins`), as the library takes them.
fn listed(kept: Option<&serde_json::Value>) -> Vec<Plugin> {
    let list = kept.and_then(|k| k["plugins"].as_array()).cloned().unwrap_or_default();
    list.iter()
        .filter_map(|p| {
            Some(Plugin {
                id: p["id"].as_str()?.to_string(),
                name: p["name"].as_str()?.to_string(),
                vendor: p["vendor"].as_str().unwrap_or_default().to_string(),
                instrument: p["instrument"].as_bool().unwrap_or(false),
                effect: p["effect"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

impl AppState {
    /// The library (LIB-1): the open song's build's components, or std's and the store's packs'
    /// with no song open; their presets and yours; their samples and your folders' as indexed so
    /// far, borrowed from their indexes; the plugins the app has listed. Reads files: call it off
    /// the async threads.
    pub fn library(&self) -> Result<Library> {
        let folders = self.library_folders();
        let (catalog, packages) = match self.graph.library()? {
            Some(both) => both,
            None => {
                let packages = without_song(folders.packs.as_deref());
                (m2_script::describe(&packages), packages)
            }
        };
        // The list kept from before any scan that runs now: the scan holds another lock.
        let plugins = listed(self.plugins.lock().as_ref());
        let samples = self.samples(&packages, &folders);
        let yours = folders.presets.as_deref();
        Ok(Library::of(&Sources { catalog: &catalog, packages: &packages, yours, plugins: &plugins, samples: &samples }))
    }

    /// The samples of `packages` and of your sample folders, as indexed so far (LIB-4, LIB-5): an
    /// index is started where there is none, or an old one. A folder inside another of yours (kept
    /// from before they were refused) is that one's.
    fn samples(&self, packages: &[(String, PathBuf)], folders: &Folders) -> Vec<Arc<Indexed>> {
        let cache = folders.samples.as_deref();
        let mut out = vec![];
        for (name, dir) in packages {
            out.extend(self.sample_index.package(name, dir, cache).0);
        }
        let yours = self.sample_folders();
        for dir in yours.iter().filter(|d| !yours.iter().any(|o| o != *d && d.starts_with(o))) {
            out.extend(self.sample_index.folder(dir, cache).0);
        }
        out
    }

    /// Your sample folders, as this computer's settings keep them (LIB-5): none when this server
    /// keeps no settings.
    pub fn sample_folders(&self) -> Vec<PathBuf> {
        self.settings().map(|s| s.read().samples).unwrap_or_default()
    }

    /// Your sample folders as the library shows them, each indexed so far (LIB-5).
    pub fn sample_folders_shown(&self) -> Vec<FolderShown> {
        let cache = self.library_folders().samples;
        self.sample_folders()
            .into_iter()
            .map(|dir| {
                let (i, indexing) = self.sample_index.folder(&dir, cache.as_deref());
                let i = i.unwrap_or_default();
                FolderShown {
                    missing: !dir.is_dir(),
                    path: dir,
                    samples: i.items.len(),
                    not_wav: i.not_wav,
                    unreadable: i.unreadable.len(),
                    indexing,
                }
            })
            .collect()
    }

    /// The packages a preview may load (LIB-3): the open song's build's and the store's other
    /// packs, or std and the store's packs with no song open.
    pub fn preview_packages(&self) -> Result<Vec<(String, PathBuf)>> {
        Ok(match self.graph.library()? {
            Some((_, packages)) => packages,
            None => without_song(self.library_folders().packs.as_deref()),
        })
    }

    /// The sample with library id `id`, as the library lists it now (LIB-6, LIB-7): never a
    /// path asked for. An index running (the app just started) is waited for, up to 30 seconds.
    /// Why there is none, in words.
    pub fn listed_sample(&self, id: &str) -> Result<Item> {
        let find = |lib: &Library| lib.get(id).filter(|i| i.kind == Kind::Sample && i.sample.is_some()).cloned();
        if let Some(found) = find(&self.library()?) {
            return Ok(found);
        }
        if self.sample_index.indexing() > 0
            && self.sample_index.settled(WAIT_FOR_INDEX)
            && let Some(found) = find(&self.library()?)
        {
            return Ok(found);
        }
        Err(anyhow!("There is no sample {id} in the library."))
    }

    /// Start indexing the samples a search will want first, std's and the store's packs' and
    /// yours, and delete the cache files of folders that are gone, in the background (LIB-5): as
    /// the app starts.
    pub(crate) fn warm_samples(&self) {
        let folders = self.library_folders();
        let packages = without_song(folders.packs.as_deref());
        let yours = self.sample_folders();
        if let Some(cache) = folders.samples.clone() {
            let kept = yours.clone();
            let _ = std::thread::Builder::new().name("music2-samples-prune".into()).spawn(move || {
                let _ = m2_os::thread::background();
                samples::prune(&cache, &kept);
            });
        }
        let cache = folders.samples.as_deref();
        for (name, dir) in &packages {
            self.sample_index.package(name, dir, cache);
        }
        for dir in &yours {
            self.sample_index.folder(dir, cache);
        }
    }
}

/// A short hash of `text`, for a file name.
fn short(text: &[u8]) -> String {
    m2_song::Sha::of(text).hex()[..16].to_string()
}

/// The preview of item `id` (LIB-3), in the library `packages` (the open song's, or std alone; std
/// among them) and `yours` (your Presets folder) make: its WAV in `cache`, rendered now with std and
/// the item's own package loaded, unless kept there. Why there is none, in words.
pub fn preview(previews: &Previews, id: &str, packages: &[(String, PathBuf)], yours: Option<&Path>, cache: &Path) -> Result<Preview> {
    let catalog = m2_script::describe(packages);
    packages.iter().find(|(n, _)| n == "std").context("no std package is loaded")?;
    let items = library::items(&Sources { catalog: &catalog, packages, yours, plugins: &[], samples: &[] });
    let item = items.iter().find(|i| i.id == id).ok_or_else(|| anyhow!("There is no {id} in the library."))?;
    let (song, packages) = made::of(item, &catalog, packages, yours).map_err(|e| anyhow!(e))?;
    let named = short(id.as_bytes());
    let file = cache.join(format!("{named}-{}.wav", &made::fingerprint(&song, &packages).hex()[..16]));
    let lock = previews.making.lock().entry(named.clone()).or_default().clone();
    let _making = lock.lock();
    if let Ok(wav) = std::fs::read(&file) {
        return Ok(Preview { wav, file, rendered: false });
    }
    made::render(&song, &packages, &cache.join(format!("making-{named}")), &file)?;
    // The item's older previews, of what it rendered before, and the part file of a render that
    // stopped half way (a crash): not one that may be another app's at work.
    for old in std::fs::read_dir(cache).into_iter().flatten().flatten().map(|e| e.path()) {
        let name = old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stale = || old.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|age| age > STALE_PART));
        if old != file && name.starts_with(&format!("{named}-")) && (name.ends_with(".wav") || name.ends_with(".part") && stale()) {
            let _ = std::fs::remove_file(&old);
        }
    }
    Ok(Preview { wav: std::fs::read(&file)?, file, rendered: true })
}
