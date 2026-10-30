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

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use m2_editor::library::samples::{self, Indexed, Root};
use m2_editor::library::{self, Item, Kind, Plugin, Sources, preview as made};
use m2_editor::presets;
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

/// How long a folder's index is used before it is made again: a package's at the next ask, a
/// folder of yours in the background while the old one is used.
const FRESH: Duration = Duration::from_secs(60);
/// A package's, which costs a walk and a look in the cache.
const FRESH_PACKAGE: Duration = Duration::from_secs(10);

/// The samples indexed so far (LIB-4, LIB-5), by folder: a package's `samples/`, or a folder of
/// yours.
#[derive(Default)]
pub struct SampleIndex {
    kept: Mutex<HashMap<PathBuf, Kept>>,
    /// Counts the background indexes running, for [`SampleIndex::settled`].
    running: Mutex<usize>,
    done: parking_lot::Condvar,
}

struct Kept {
    indexed: Option<Arc<Indexed>>,
    at: Instant,
    indexing: bool,
}

/// One of your folders as the library shows it: how many samples it has, how many files were
/// skipped (not WAV), and whether it is being indexed now.
#[derive(Clone, Debug, serde::Serialize)]
pub struct FolderShown {
    pub path: PathBuf,
    pub samples: usize,
    pub not_wav: usize,
    pub unreadable: usize,
    pub indexing: bool,
    pub missing: bool,
}

impl SampleIndex {
    /// Package `name`'s samples, in `dir`: indexed now, with the cache in `cache`, the first time
    /// and when the last is older than [`FRESH_PACKAGE`].
    pub fn package(&self, name: &str, dir: &Path, cache: Option<&Path>) -> Arc<Indexed> {
        if let Some(k) = self.kept.lock().get(dir)
            && let Some(i) = &k.indexed
            && k.at.elapsed() < FRESH_PACKAGE
        {
            return i.clone();
        }
        let made = Arc::new(samples::index(Root::Package { name, dir }, cache));
        self.kept.lock().insert(dir.to_path_buf(), Kept { indexed: Some(made.clone()), at: Instant::now(), indexing: false });
        made
    }

    /// Folder `dir`'s samples as indexed so far, never waiting (LIB-5): an index is started in
    /// the background, at background priority, when there is none, or it is older than [`FRESH`].
    pub fn folder(self: &Arc<Self>, dir: &Path, cache: Option<&Path>) -> (Option<Arc<Indexed>>, bool) {
        let mut kept = self.kept.lock();
        let k = kept.entry(dir.to_path_buf()).or_insert(Kept { indexed: None, at: Instant::now(), indexing: false });
        if !k.indexing && (k.indexed.is_none() || k.at.elapsed() >= FRESH) {
            k.indexing = true;
            *self.running.lock() += 1;
            let (me, dir, cache) = (self.clone(), dir.to_path_buf(), cache.map(Path::to_path_buf));
            let started = std::thread::Builder::new().name("music2-samples".into()).spawn(move || {
                let _ = m2_os::thread::background();
                let made = Arc::new(samples::index(Root::Folder(&dir), cache.as_deref()));
                me.kept.lock().insert(dir, Kept { indexed: Some(made), at: Instant::now(), indexing: false });
                *me.running.lock() -= 1;
                me.done.notify_all();
            });
            if started.is_err() {
                k.indexing = false;
                *self.running.lock() -= 1;
            }
        }
        (k.indexed.clone(), k.indexing)
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

    /// Forget folder `dir`'s index: it was removed from the library.
    pub fn forget(&self, dir: &Path) {
        self.kept.lock().remove(dir);
    }
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
    /// Every item of the library (LIB-1): the open song's build's components, or std's and the
    /// store's packs' with no song open; their presets and yours; their samples and your folders'
    /// as indexed so far; the plugins the app has listed. Reads files: call it off the async
    /// threads.
    pub fn library(&self) -> Result<Vec<Item>> {
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
        Ok(library::items(&Sources { catalog: &catalog, packages: &packages, yours, plugins: &plugins, samples: &samples }))
    }

    /// The samples of `packages` and of your sample folders, as indexed so far (LIB-4, LIB-5).
    fn samples(&self, packages: &[(String, PathBuf)], folders: &Folders) -> Vec<Item> {
        let cache = folders.samples.as_deref();
        let mut out = vec![];
        for (name, dir) in packages {
            out.extend(self.sample_index.package(name, dir, cache).items.iter().cloned());
        }
        for dir in self.sample_folders() {
            if let (Some(i), _) = self.sample_index.folder(&dir, cache) {
                out.extend(i.items.iter().cloned());
            }
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
    /// path asked for. Why there is none, in words.
    pub fn listed_sample(&self, id: &str) -> Result<Item> {
        let items = self.library()?;
        items
            .into_iter()
            .find(|i| i.id == id && i.kind == Kind::Sample && i.sample.is_some())
            .ok_or_else(|| anyhow!("There is no sample {id} in the library."))
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
    // std (the song's mixer, an effect's loop) and the item's own package.
    let own = item.component.split_once(':').map_or("", |(p, _)| p);
    let packages: Vec<(String, PathBuf)> = packages.iter().filter(|(n, _)| n == "std" || n == own).cloned().collect();
    let values = match &item.preset {
        Some(p) => {
            // A package's own presets are in its folder; yours in yours.
            let dir = match p.from.as_str() {
                "std" => packages.iter().find(|(n, _)| *n == item.package).and_then(|(_, d)| presets::factory(d, &item.component)),
                _ => yours.map(|y| y.join(presets::folder(&item.component))),
            };
            let dir = dir.ok_or_else(|| anyhow!("{} can't be found.", p.name))?;
            presets::read(&dir, &item.component, &p.name).map_err(|e| anyhow!(e))?.values
        }
        None => BTreeMap::new(),
    };
    let song = made::song(item, &catalog, &values).map_err(|e| anyhow!(e))?;
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
