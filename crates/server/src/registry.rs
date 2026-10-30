//! The registry in the app, on the server (REG-11 to REG-17): its catalog kept in memory and
//! fetched in the background, ⌘F's From the library, a library item's preview, and Get.
//!
//! The catalog is the one the cache keeps (`m2_editor::get::Remote`), read as the server starts
//! keeping the library, then fetched: at once, again once it is a day old, and ten minutes after a
//! fetch that failed, each in the background as a search asks, which never waits for it. One Get
//! runs at a time, on the request's own blocking thread: it says how far it is in `get` events
//! (at most ten a second) and stops when cancelled.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::get::{self, Missing, Offer, Remote};
use m2_song::Version;
use m2_song::pack::{Lock, PackName, PkgManifest, Store};
use m2_song::registry::Catalog;
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::AppState;

/// How long after a fetch that failed the next is tried, at a search (REG-12).
const RETRY: Duration = Duration::from_secs(10 * 60);
/// How often a Get says how far it is.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// The registry's catalog as the server has it, its fetches, and the Get running.
#[derive(Default)]
pub struct Registry {
    fetch: Mutex<Fetch>,
    getting: Mutex<Option<Getting>>,
}

/// The catalog kept, and how its fetches went.
#[derive(Default)]
struct Fetch {
    kept: Option<Arc<Catalog>>,
    running: bool,
    /// When the last fetch that worked ended.
    ok: Option<Instant>,
    /// When the last fetch ended, and why it failed, if it did.
    tried: Option<Instant>,
    error: Option<String>,
}

impl Fetch {
    /// Whether a fetch is due (REG-12): none ran yet; the last worked a day ago; or the last failed
    /// ten minutes ago.
    fn due(&self) -> bool {
        !self.running
            && match (&self.error, self.ok, self.tried) {
                (_, _, None) => true,
                (None, Some(ok), _) => ok.elapsed() >= get::FRESH,
                (_, _, Some(tried)) => tried.elapsed() >= RETRY,
            }
    }
}

/// The Get running: what it gets, and its cancel.
struct Getting {
    label: String,
    cancel: Arc<get::Cancel>,
}

/// What a Get is asked for: a library item (`id`), or a song's missing locked version (`pack` and
/// `version`); a sample goes to the open song (`session`) at `to`.
#[derive(serde::Deserialize)]
pub struct GetReq {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub pack: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub session: Option<u64>,
    #[serde(default)]
    pub to: Option<m2_editor::import::Target>,
}

impl AppState {
    /// The registry this server reads (REG-11): the one it was given; none in a server given none.
    pub fn remote(&self) -> Option<Remote> {
        self.library_folders().registry
    }

    /// The catalog kept, starting a fetch in the background when one is due (REG-12): never
    /// waiting for it. None with no registry, or none fetched yet.
    pub fn catalog(&self) -> Option<Arc<Catalog>> {
        let remote = self.remote()?;
        let mut f = self.registry.fetch.lock();
        if f.due() {
            f.running = true;
            let registry = self.registry.clone();
            let first = f.kept.is_none() && f.tried.is_none();
            let started = std::thread::Builder::new().name("music2-catalog".into()).spawn(move || {
                // As the app starts: the catalog kept, at once, while the fetch runs.
                if first && let Some(k) = remote.kept() {
                    registry.fetch.lock().kept.get_or_insert(k.catalog);
                }
                let got = remote.fetch(&AtomicBool::new(false));
                let mut f = registry.fetch.lock();
                (f.running, f.tried) = (false, Some(Instant::now()));
                match got {
                    Ok(k) => (f.kept, f.ok, f.error) = (Some(k.catalog), Some(Instant::now()), None),
                    Err(why) => {
                        eprintln!("music2: {why}");
                        f.error = Some(why);
                    }
                }
            });
            if started.is_err() {
                f.running = false;
            }
        }
        f.kept.clone()
    }

    /// What the search says of the registry (REG-12): null with none; else whether its catalog is
    /// being fetched, when the one kept was made, and why the last fetch failed.
    pub fn registry_said(&self) -> Value {
        if self.remote().is_none() {
            return Value::Null;
        }
        let f = self.registry.fetch.lock();
        json!({ "fetching": f.running, "made": f.kept.as_ref().map(|c| c.made.clone()), "error": f.error })
    }

    /// The newest version of each pack this computer has: in the store, or offered by the open
    /// song's build (`packages`, its own path packs among them).
    fn have(&self, packages: &[(String, PathBuf)]) -> impl Fn(&PackName) -> Option<Version> + use<> {
        let mut have: std::collections::HashMap<String, Version> = std::collections::HashMap::new();
        let mut put = |name: &str, v: Version| {
            let e = have.entry(name.to_string()).or_insert(v);
            *e = (*e).max(v);
        };
        if let Some(store) = self.library_folders().packs.map(Store::at) {
            for (name, versions) in store.list() {
                if let Some(v) = versions.last() {
                    put(name.as_str(), *v);
                }
            }
        }
        for (name, dir) in packages {
            if let Ok(m) = PkgManifest::at(dir) {
                put(name, m.package.version);
            }
        }
        move |name: &PackName| have.get(name.as_str()).copied()
    }

    /// From the library (REG-13): the catalog's items of packs this computer hasn't got, or has an
    /// older version of. None with no catalog. Reads files: call it off the async threads.
    pub fn offers(&self) -> Result<Vec<Offer>> {
        let Some(catalog) = self.catalog() else { return Ok(vec![]) };
        let have = self.have(&self.preview_packages()?);
        Ok(get::offers(&catalog, &have, &|_| false))
    }

    /// The library item with id `id` (`library:…`), as the catalog has it now, whether or not this
    /// computer has its pack. Why there is none, in words.
    fn offer(&self, id: &str) -> Result<Offer> {
        let catalog = self.catalog().ok_or_else(|| anyhow!("The library's catalog isn't here yet: try again in a moment."))?;
        get::offers(&catalog, &|_| None, &|_| false).into_iter().find(|o| o.item.id == id).ok_or_else(|| anyhow!("{id} isn't in the library."))
    }

    /// A library item's preview (REG-14): its blob, fetched by its hash and kept by it. Its file.
    pub fn library_preview(&self, id: &str) -> Result<PathBuf> {
        let remote = self.remote().context("No registry is set, so nothing is in the library.")?;
        let o = self.offer(id)?;
        let sha = o
            .facts
            .preview
            .ok_or_else(|| anyhow!("{}", o.item.no_preview.clone().unwrap_or_else(|| format!("{} has no preview.", o.item.label))))?;
        remote
            .blob(sha, m2_editor::library::samples::PREVIEW_BYTES, &AtomicBool::new(false))
            .map_err(|e| anyhow!("{}'s preview couldn't be fetched: {e}", o.item.label))
    }

    /// Cancel the Get running (REG-15): whether one was.
    pub fn cancel_get(&self) -> bool {
        self.registry.getting.lock().as_ref().is_some_and(|g| g.cancel.cancel())
    }

    /// The open song's locked versions that aren't installed (REG-17), each with whether the
    /// catalog has it with that very hash. None with no song open, or no lock.
    pub fn missing_packs(&self) -> Result<Vec<Missing>> {
        let Some(dir) = self.graph.dir() else { return Ok(vec![]) };
        let text = match std::fs::read_to_string(dir.join(m2_song::path::LOCK)) {
            Ok(t) => t,
            Err(_) => return Ok(vec![]),
        };
        let lock = Lock::read(&text).map_err(|d| anyhow!("{}", d.message))?;
        let store = Store::at(self.library_folders().packs.context("This app keeps no store of packs.")?);
        Ok(get::missing(&lock, &store, self.catalog().as_deref()))
    }

    /// Get what `r` asks for (REG-15, REG-16, REG-17), one Get at a time, saying how far it is in
    /// `get` events; the answer once it is done. A pack's whole version is installed and the open
    /// song told (its lists then offer it, and a song that waited for it builds); a sample is
    /// added to the open song as Add sample adds one. Blocks: call it off the async threads.
    pub fn get_from_library(&self, r: GetReq) -> Result<Value> {
        let remote =
            self.remote().context("No registry is set: set M2_REGISTRY, or `registry` in ~/.music2/settings.json, to get packs.")?;
        let (id, offer, version) = match (&r.id, &r.pack, &r.version) {
            (Some(id), None, None) => {
                let o = self.offer(id)?;
                (id.clone(), Some(o.clone()), o.version)
            }
            (None, Some(pack), Some(version)) => {
                let name = PackName::new(pack).map_err(|e| anyhow!(e))?;
                let version = Version::parse(version).ok_or_else(|| anyhow!("`{version}` isn't a version such as 1.2.0"))?;
                let m = self.missing_packs()?.into_iter().find(|m| m.name == name && m.version == version);
                let m = m.ok_or_else(|| anyhow!("The open song doesn't lock {name} {version}, or it is installed already."))?;
                (format!("{name}@{version}"), None, m.got.map_err(|e| anyhow!(e))?)
            }
            _ => bail!("Get asks for a library item (`id`), or a song's missing version (`pack` and `version`)."),
        };
        let sample = offer.as_ref().filter(|o| o.item.kind == m2_editor::library::Kind::Sample);
        if sample.is_some() && (r.session.is_none() || self.graph.session().is_none()) {
            bail!("Open a song to add a sample to it.");
        }
        let label = offer.as_ref().map_or_else(|| id.clone(), |o| o.item.label.clone());
        let cancel = Arc::new(get::Cancel::default());
        {
            let mut g = self.registry.getting.lock();
            if let Some(other) = g.as_ref() {
                bail!("Getting {} already: wait for it, or cancel it.", other.label);
            }
            *g = Some(Getting { label: label.clone(), cancel: cancel.clone() });
        }
        // However it ends, it is no longer running, and every window hears it ended.
        let _ends = Ends { state: self, id: id.clone() };
        let mut last = None::<Instant>;
        let mut progress = |p: get::Progress| {
            if last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) || p.done == p.total {
                last = Some(Instant::now());
                self.send(json!({"t": "get", "id": id, "label": label, "done": p.done, "total": p.total}));
            }
        };
        if let Some(o) = sample {
            let work = Scratch::new(&remote.cache)?;
            let s = get::sample(&remote, o, &work.0, &mut progress, &cancel).map_err(|e| anyhow!(e))?;
            let to = r.to.context("Say where the sample goes (`to`).")?;
            let mut done = self.graph.add_sample(r.session.expect("checked"), s, to)?;
            done["got"] = json!({"pack": o.pack.as_str(), "version": o.version.version.to_string()});
            return Ok(done);
        }
        let name =
            offer.as_ref().map(|o| o.pack.clone()).or_else(|| r.pack.as_deref().and_then(|p| PackName::new(p).ok())).expect("a pack");
        let store = Store::at(self.library_folders().packs.context("This app keeps no store of packs.")?);
        let std = crate::resources::std_dir();
        let installed = get::pack(&remote, &name, &version, &store, &std, &mut progress, &cancel).map_err(|e| anyhow!(e))?;
        self.graph.packs_installed()?;
        let already = matches!(installed, m2_editor::packs::Install::Already { .. });
        let item = offer.as_ref().map(|o| o.facts.id.clone());
        Ok(json!({"got": {"pack": name.as_str(), "version": version.version.to_string(), "already": already}, "id": item}))
    }
}

/// The end of a Get: it is no longer running, and every window hears so.
struct Ends<'a> {
    state: &'a AppState,
    id: String,
}

impl Drop for Ends<'_> {
    fn drop(&mut self) {
        *self.state.registry.getting.lock() = None;
        self.state.send(json!({"t": "get", "id": self.id, "end": true}));
    }
}

/// A folder of its own in the registry's cache for a sample being got, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(cache: &Path) -> Result<Scratch> {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = cache.join(format!("getting-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).with_context(|| format!("{} couldn't be made", dir.display()))?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
