//! The registry in the app (REG-11 to REG-17): where it is, its catalog kept on this computer,
//! ⌘F's From the library, previews by hash, and Get.
//!
//! [`address`] says which registry this program reads. A [`Remote`] is that registry and the
//! folder of the cache folder where what is fetched from it is kept: [`Remote::fetch`] gets
//! `catalog.json` and, when it isn't kept, the catalog it names, checks it and keeps both;
//! [`Remote::kept`] reads back the one kept, offline too; [`Remote::blob`] gets a preview by its
//! hash and keeps it. [`offers`] makes the catalog's items of packs this computer hasn't got into
//! library items, which LIB-1's one search ranks ([`find`]). [`pack`] gets a pack's whole version
//! into a folder beside the store and installs it from there, all or nothing; [`sample`] gets one
//! sample's file; [`missing`] says which of a song's locked versions aren't installed and whether
//! the catalog has them.
//!
//! Every byte comes through [`crate::fetch`] (NET-1), checked by its size and hash, and every call
//! here blocks: the server calls them off its async threads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use m2_song::pack::{self, Listing, Lock, PackName, PkgManifest, Store};
use m2_song::registry::{self as reg, Catalog, CatalogPack, CatalogVersion, ItemFacts, ItemKind, Pointer, SampleKind};
use m2_song::{Sha, Version};
use serde_json::{Value, json};

use crate::fetch::{self, Expect};
use crate::library::{Item, Kind, PresetOf, Query, Sample, SampleType, samples};
use crate::packs::Install;

/// The registry music2 reads when neither `M2_REGISTRY` nor the settings name one (REG-11):
/// empty until the owner picks the bucket's address (design D10).
pub const DEFAULT_REGISTRY: &str = "";

/// The environment variable that names the registry (REG-11).
pub const ENV: &str = "M2_REGISTRY";

/// What a library item's id starts with (REG-13): `library:@alice/drums:…`.
pub const PREFIX: &str = "library:";

/// How old a catalog may be before it is fetched again (REG-12): a day.
pub const FRESH: Duration = Duration::from_secs(24 * 60 * 60);

/// The most `catalog.json` may be, and the longest it may take (REG-12, as REG-9's).
const POINTER_LIMIT: (u64, Duration) = (64 << 10, Duration::from_secs(30));
/// The most a catalog may be, and the longest it may take.
const CATALOG_LIMIT: (u64, Duration) = (512 << 20, Duration::from_secs(600));
/// The most a version's `m2pack.json` may be: its paths, sizes and hashes.
const LISTING_LIMIT: u64 = 64 << 20;

/// How long a download of `size` bytes may take (NET-6's deadline): two minutes, and a second for
/// every 256 KiB, so a server that drips can't hold a Get, and a slow connection still gets it.
fn deadline(size: u64) -> Duration {
    Duration::from_secs(120 + size / (256 << 10))
}

/// Where the registry is (REG-11): `env` (`M2_REGISTRY`) when it says one, else `setting` (the
/// settings' `registry`), else [`DEFAULT_REGISTRY`]; none when none says one.
pub fn address(env: Option<&str>, setting: Option<&str>) -> Option<String> {
    [env, setting, Some(DEFAULT_REGISTRY)].into_iter().flatten().map(str::trim).find(|a| !a.is_empty()).map(String::from)
}

/// The registry this program reads (REG-11), and the folder of its cache where what is fetched
/// from it is kept (`registry` in music2's cache folder).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    /// The bucket's address, with no `/` at its end: `https://…`, `http://127.0.0.1:…`, `file:///…`.
    pub at: String,
    pub cache: PathBuf,
}

/// The catalog kept (REG-12), and when it was last fetched.
#[derive(Clone, Debug)]
pub struct Kept {
    pub catalog: Arc<Catalog>,
    pub fetched: SystemTime,
}

/// Write `bytes` at `path` through a part file of its own renamed into place, its folder made:
/// what reads it finds the old file or the new one, never half of one.
fn put(path: &Path, bytes: &[u8]) -> Result<(), String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let why = |e: std::io::Error| format!("{} couldn't be written: {e}", path.display());
    std::fs::create_dir_all(path.parent().expect("a file in a folder")).map_err(why)?;
    let part = path.with_extension(format!("{}-{}.part", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let written = (|| {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&part)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&part, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    written.map_err(why)
}

impl Remote {
    /// The registry at `at` (an address, or a folder's whole path), kept in `cache`. Why not, in
    /// words, when `at` isn't one music2 downloads from (NET-3).
    pub fn new(at: &str, cache: &Path) -> Result<Remote, String> {
        let at = at.trim();
        let url = if at.contains("://") {
            at.to_string()
        } else if Path::new(at).is_absolute() {
            fetch::file_address(Path::new(at)).ok_or_else(|| format!("`{at}` isn't a folder's whole path"))?
        } else {
            return Err(format!("`{at}` isn't a registry's address: give https://…, file:///… or a folder's whole path"));
        };
        let url = url.trim_end_matches('/').to_string();
        fetch::check(&url).map_err(|e| format!("The registry `{at}` can't be used: {e}"))?;
        Ok(Remote { at: url, cache: cache.to_path_buf() })
    }

    /// The address of `path` in the bucket.
    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.at)
    }

    /// Where this registry's catalog is kept: by a short hash of its address, so two registries
    /// never mix.
    fn dir(&self) -> PathBuf {
        self.cache.join(&Sha::of(self.at.as_bytes()).hex()[..16])
    }

    /// Where blobs are kept, by their hashes: every registry's, since a hash names the same bytes
    /// wherever they come from.
    fn blobs(&self) -> PathBuf {
        self.cache.join("blobs")
    }

    /// The `catalog.json` kept, when the catalog it names is kept too.
    fn kept_pointer(&self) -> Option<Pointer> {
        let dir = self.dir();
        let pointer = Pointer::read(&std::fs::read(dir.join(reg::POINTER)).ok()?).ok()?;
        dir.join(reg::catalog_path(&pointer.catalog)).is_file().then_some(pointer)
    }

    /// The catalog kept in the cache, read back and checked against the `catalog.json` kept with it
    /// (REG-12); none when there is none, or it doesn't read.
    pub fn kept(&self) -> Option<Kept> {
        let dir = self.dir();
        let file = dir.join(reg::POINTER);
        let pointer = Pointer::read(&std::fs::read(&file).ok()?).ok()?;
        let catalog = Catalog::read_checked(&std::fs::read(dir.join(reg::catalog_path(&pointer.catalog))).ok()?, &pointer).ok()?;
        let fetched = std::fs::metadata(&file).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        Some(Kept { catalog: Arc::new(catalog), fetched })
    }

    /// Fetch `catalog.json`, and the catalog it names when it isn't kept, check it, and keep both
    /// (REG-12): the catalog first, then `catalog.json`, then the catalogs it replaced go. Why not,
    /// in words: the one kept stays as it was.
    pub fn fetch(&self, cancel: &AtomicBool) -> Result<Kept, String> {
        let said = |why: String| format!("The library's catalog couldn't be fetched: {why}");
        let (limit, within) = POINTER_LIMIT;
        let bytes = fetch::get(&self.url(reg::POINTER), &Expect { size: None, sha256: None, limit, within }, cancel)
            .map_err(|e| said(e.to_string()))?;
        let pointer = Pointer::read(&bytes).map_err(said)?;
        // Never an older one than the one kept, nor another as old (REG-12): a cache in front of
        // the bucket, or a host, serving an old catalog would undo a yank or a take-down.
        if let Some(kept) = self.kept_pointer() {
            if pointer.made < kept.made {
                return Err(said(format!(
                    "{} names a catalog made at {}, older than the one kept (made at {}), so the one kept stays",
                    reg::POINTER,
                    pointer.made,
                    kept.made
                )));
            }
            if pointer.made == kept.made && pointer.catalog != kept.catalog {
                return Err(said(format!(
                    "{} names another catalog as old as the one kept (made at {}), so the one kept stays",
                    reg::POINTER,
                    kept.made
                )));
            }
        }
        let dir = self.dir();
        let named = reg::catalog_path(&pointer.catalog);
        let at = dir.join(&named);
        let catalog = match std::fs::read(&at).ok().and_then(|b| Catalog::read_checked(&b, &pointer).ok()) {
            Some(c) => c,
            None => {
                let (limit, within) = CATALOG_LIMIT;
                let expect = Expect { size: Some(pointer.size), sha256: Some(pointer.catalog), limit, within };
                let got = fetch::get(&self.url(&named), &expect, cancel).map_err(|e| said(e.to_string()))?;
                let c = Catalog::read_checked(&got, &pointer).map_err(said)?;
                put(&at, &got)?;
                c
            }
        };
        put(&dir.join(reg::POINTER), &bytes)?;
        for old in std::fs::read_dir(dir.join("catalog")).into_iter().flatten().flatten().map(|e| e.path()) {
            if old != at {
                let _ = std::fs::remove_file(old);
            }
        }
        Ok(Kept { catalog: Arc::new(catalog), fetched: SystemTime::now() })
    }

    /// The catalog kept when it was fetched within `fresh`, else fetched now; offline, the one
    /// kept, with why it couldn't be fetched (what `m2` does with the app closed).
    pub fn current(&self, fresh: Duration) -> (Option<Kept>, Option<String>) {
        let kept = self.kept();
        if kept.as_ref().is_some_and(|k| k.fetched.elapsed().is_ok_and(|age| age < fresh)) {
            return (kept, None);
        }
        match self.fetch(&AtomicBool::new(false)) {
            Ok(k) => (Some(k), None),
            Err(why) => (kept, Some(why)),
        }
    }

    /// The blob `sha` (REG-14), kept in the cache by its hash: fetched only when it isn't kept, or
    /// the one kept doesn't match it; at most `limit` bytes. Its file.
    pub fn blob(&self, sha: Sha, limit: u64, cancel: &AtomicBool) -> Result<PathBuf, String> {
        let file = self.blobs().join(sha.hex());
        if Sha::of_file(&file).is_ok_and(|h| h == sha) {
            return Ok(file);
        }
        std::fs::create_dir_all(self.blobs()).map_err(|e| format!("{} couldn't be made: {e}", self.blobs().display()))?;
        let expect = Expect { size: None, sha256: Some(sha), limit, within: deadline(limit) };
        fetch::get_to_file(&self.url(&reg::blob_path(&sha)), &file, &expect, |_| {}, cancel).map_err(|e| e.to_string())?;
        Ok(file)
    }
}

/// An item From the library (REG-13): the library item it would be, and what getting it fetches.
#[derive(Clone, Debug)]
pub struct Offer {
    /// As the library lists items: its id is `library:<its id>`.
    pub item: Item,
    pub pack: PackName,
    /// The version a Get gets: the pack's latest.
    pub version: CatalogVersion,
    pub license: String,
    pub authors: Vec<String>,
    /// The newest version of the pack this computer has, if any: an older one.
    pub have: Option<Version>,
    /// What the catalog says of the item.
    pub facts: ItemFacts,
}

impl Offer {
    /// The bytes a Get of it fetches: its one file for a sample, else its whole version.
    pub fn size(&self) -> u64 {
        match &self.facts.file {
            Some(f) if self.facts.kind == ItemKind::Sample => f.size,
            _ => self.version.size.unwrap_or(0),
        }
    }

    /// As `GET /api/library` answers it: the item, and `library`: its pack, version, licence,
    /// authors, the bytes a Get fetches, and the version this computer has.
    pub fn json(&self) -> Value {
        let mut v = serde_json::to_value(&self.item).unwrap_or(Value::Null);
        v["library"] = json!({
            "pack": self.pack.as_str(),
            "version": self.version.version.to_string(),
            "license": self.license,
            "authors": self.authors,
            "size": self.size(),
            "have": self.have.map(|h| h.to_string()),
        });
        v
    }
}

/// A sample's note (LIB-7) for an item of `p` at `version`: its pack, version, file, author and
/// licence, as the note of an installed pack's sample says them.
fn note(p: &CatalogPack, version: Version, f: &ItemFacts) -> Value {
    let mut from = json!({ "pack": p.name.as_str(), "version": version.to_string() });
    if let Some(file) = &f.file {
        from["file"] = json!(file.path);
    }
    if !p.authors.is_empty() {
        from["author"] = json!(p.authors.join(", "));
    }
    if !p.license.is_empty() {
        from["license"] = json!(p.license);
    }
    from
}

/// The library item `f` of pack `p` at `version` would be, under [`PREFIX`].
fn item_of(p: &CatalogPack, v: &CatalogVersion, f: &ItemFacts) -> Item {
    let version = v.version;
    let kind = match f.kind {
        ItemKind::Instrument => Kind::Instrument,
        ItemKind::Sound => Kind::Sound,
        ItemKind::Effect => Kind::Effect,
        ItemKind::Sample => Kind::Sample,
    };
    let sample = f.sample.as_ref().map(|s| Sample {
        kind: match s.kind {
            SampleKind::Loop => SampleType::Loop,
            SampleKind::OneShot => SampleType::OneShot,
        },
        bpm: s.bpm,
        key: s.key.as_deref().and_then(samples::key_said),
        seconds: Some(s.seconds),
        instrument: s.instrument.clone(),
        channels: s.channels,
        rate: s.rate,
        peak: s.peak,
        hash: f.file.as_ref().map(|x| x.sha256.hex()).unwrap_or_default(),
        from: note(p, version, f),
        file: PathBuf::new(),
        stamp: (0, 0),
    });
    Item {
        kind,
        id: format!("{PREFIX}{}", f.id),
        label: f.label.clone(),
        category: f.category.clone(),
        description: f.description.clone(),
        tags: f.tags.clone(),
        package: p.name.as_str().to_string(),
        // Music2's own only as the version music2 comes with (PACK-19), never by its name.
        own: m2_song::pack::own(&p.name, &version, &v.hash),
        component: f.component.clone(),
        instrument: f.instrument,
        effect: f.effect,
        preview: f.preview.is_some(),
        no_preview: match f.preview {
            Some(_) => None,
            None => Some(f.no_preview.clone().unwrap_or_else(|| format!("{} has no preview.", f.label))),
        },
        preset: f.preset.as_ref().map(|name| PresetOf { from: "std".into(), name: name.clone() }),
        sample,
    }
}

/// The items From the library (REG-13): of each pack of `catalog` with a latest version that the
/// open song doesn't list or lock (`uses`), its latest's items, when `have` says this computer has
/// none of it, or an older one.
pub fn offers(catalog: &Catalog, have: &dyn Fn(&PackName) -> Option<Version>, uses: &dyn Fn(&PackName) -> bool) -> Vec<Offer> {
    let mut out = vec![];
    for p in &catalog.packs {
        // The open song's own packs: moving it to another version is `m2 pack update`, never a Get
        // (design §11); a version it locks that isn't here is a missing pack's Get (REG-17).
        if uses(&p.name) {
            continue;
        }
        let Some(version) = p.latest.and_then(|l| p.get(l)) else { continue };
        let had = have(&p.name);
        if had.is_some_and(|h| h >= version.version) {
            continue;
        }
        for f in &p.items {
            out.push(Offer {
                item: item_of(p, version, f),
                pack: p.name.clone(),
                version: version.clone(),
                license: p.license.clone(),
                authors: p.authors.clone(),
                have: had,
                facts: f.clone(),
            });
        }
    }
    out
}

/// The offers `q` finds, best first, by LIB-1's one search.
pub fn find<'a>(offers: &'a [Offer], q: &Query) -> Vec<&'a Offer> {
    let by_item: HashMap<*const Item, &Offer> = offers.iter().map(|o| (&o.item as *const Item, o)).collect();
    crate::library::search(offers.iter().map(|o| &o.item), q)
        .into_iter()
        .filter_map(|i| by_item.get(&(i as *const Item)).copied())
        .collect()
}

/// How far a Get is: bytes; and once every byte is in, whether it is being installed, when it
/// can no longer be cancelled (REG-15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub installing: bool,
}

/// A Get's cancel (REG-15): asked for while its files come, and refused once it is installed (or
/// added): from then on it ends as it began.
#[derive(Debug, Default)]
pub struct Cancel {
    /// What `fetch` looks at: set once a cancel is taken.
    flag: AtomicBool,
    /// 0 while it may be cancelled, 1 cancelled, 2 past the point of no return.
    state: std::sync::atomic::AtomicU8,
}

impl Cancel {
    /// Cancel it: whether it will stop (it was cancelled already, or isn't past its point of no
    /// return yet).
    pub fn cancel(&self) -> bool {
        match self.state.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) | Err(1) => {
                self.flag.store(true, Ordering::SeqCst);
                true
            }
            Err(_) => false,
        }
    }

    /// Whether it was cancelled.
    pub fn cancelled(&self) -> bool {
        self.state.load(Ordering::SeqCst) == 1
    }

    /// Whether it is past its point of no return: being installed (or added).
    pub fn installing(&self) -> bool {
        self.state.load(Ordering::SeqCst) == 2
    }

    /// Pass the point of no return: whether it wasn't cancelled before.
    pub fn commit(&self) -> bool {
        self.state.compare_exchange(0, 2, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

    /// What a download looks at to stop.
    pub fn flag(&self) -> &AtomicBool {
        &self.flag
    }
}

/// What a Get's own folder beside the store is called: `.getting-<process>-<n>`.
pub const GETTING: &str = ".getting-";
/// What a sample's Get's own folder in the registry's cache is called: `getting-<process>-<n>`.
pub const GETTING_SAMPLE: &str = "getting-";

/// The folders this process's Gets are using now: a folder with its own id that isn't one of
/// these was left by a process that crashed with that id before (ids are reused).
static IN_USE: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// A Get's own folder (REG-15), `<parent>/<prefix><process>-<n>`: beside the store for a pack's
/// files, in the registry's cache for a sample's. Removed with everything in it when dropped:
/// however the Get ends, nothing is left.
pub struct Work(pub PathBuf);

impl Work {
    /// A new folder of this Get's own in `parent`, named by `prefix`. The folders of Gets that
    /// crashed go first: another process's that no longer runs, and one with this process's id
    /// that none of its Gets is using.
    pub fn new(parent: &Path, prefix: &str) -> Result<Work, String> {
        static N: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(parent).map_err(|e| format!("{} couldn't be made: {e}", parent.display()))?;
        let mut in_use = IN_USE.lock().unwrap_or_else(|e| e.into_inner());
        let me = std::process::id();
        for e in std::fs::read_dir(parent).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let pid = name.strip_prefix(prefix).and_then(|r| r.split_once('-')).and_then(|(pid, _)| pid.parse::<u32>().ok());
            let left = pid.is_some_and(|pid| match pid == me {
                true => !in_use.contains(&e.path()),
                false => !m2_os::process::alive(pid),
            });
            if left {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        let dir = parent.join(format!("{prefix}{me}-{}", N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&dir).map_err(|e| format!("{} couldn't be made: {e}", dir.display()))?;
        in_use.push(dir.clone());
        Ok(Work(dir))
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        IN_USE.lock().unwrap_or_else(|e| e.into_inner()).retain(|d| *d != self.0);
    }
}

/// Why `path` isn't a plain path in a pack (PACK-5's rules for a package file), if it isn't:
/// relative, `/`-separated, no empty, `.` or `..` part, none starting with a dot (no part of a
/// pack, PACK-6), and no `\`, `:` or NUL.
fn not_plain(path: &str) -> Option<&'static str> {
    if path.is_empty() || path.starts_with('/') {
        return Some("it isn't a path inside the pack");
    }
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Some("it has an empty, `.` or `..` part");
        }
        if part.starts_with('.') {
            return Some("a part of it starts with a dot");
        }
        if part.contains(['\\', ':', '\0']) {
            return Some("it holds a `\\`, a `:` or a NUL");
        }
    }
    None
}

/// A version's `m2pack.json` as fetched (REG-15), read strictly: in its one form, every path
/// plain, by sorted path, each once, and adding up to the size and count the catalog says.
fn listing_of(bytes: &[u8], v: &CatalogVersion) -> Result<Listing, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "its m2pack.json isn't text".to_string())?;
    let listing = Listing::read(text)?;
    if listing.text() != text {
        return Err("its m2pack.json isn't written in its one form (PACK-6)".into());
    }
    for f in &listing.files {
        if let Some(why) = not_plain(&f.path) {
            return Err(format!("its m2pack.json lists `{}`, which isn't a plain path in a pack: {why}", f.path.escape_debug()));
        }
        if f.path == pack::LISTING {
            return Err("its m2pack.json lists itself".into());
        }
    }
    if !listing.files.windows(2).all(|w| w[0].path < w[1].path) {
        return Err("its m2pack.json lists its files out of order, or one twice".into());
    }
    // Sizes from the network: added up with a check, so none can wrap round to a small one.
    let size =
        listing.files.iter().try_fold(bytes.len() as u64, |sum, f| sum.checked_add(f.size).filter(|s| *s <= crate::registry::LARGEST));
    let Some(size) = size else {
        return Err("its m2pack.json lists files of more than the 10 GB a pack may be".into());
    };
    let files = listing.files.len() as u64 + 1;
    if v.size != Some(size) || v.files != Some(files) {
        return Err(format!(
            "its m2pack.json lists {files} files of {size} bytes, where the library says {} of {}",
            v.files.unwrap_or(0),
            v.size.unwrap_or(0)
        ));
    }
    Ok(listing)
}

/// Get version `version` of `name` (REG-15) from `remote` into `store`, whole and checked, as
/// `m2 pack install` installs (std in `std`). `progress` is told how far, `cancel` stops it. A
/// failure or a cancel leaves nothing installed and no folder.
pub fn pack(
    remote: &Remote,
    name: &PackName,
    version: &CatalogVersion,
    store: &Store,
    std: &Path,
    progress: &mut dyn FnMut(Progress),
    stop: &Cancel,
) -> Result<Install, String> {
    let cancel = stop.flag();
    let at = format!("{name} {}", version.version);
    let said = |why: String| format!("Couldn't get {at}: {why}");
    if let Some(why) = &version.removed {
        return Err(said(format!("it was taken down from the library ({why}), so its files are gone")));
    }
    let total = version.size.ok_or_else(|| said("the library lists no files for it".into()))?;
    let work = Work::new(&store.dir, GETTING).map_err(said)?;
    // Its listing, by the version's hash, read before any other file is asked for.
    let expect = Expect { size: None, sha256: Some(version.hash), limit: LISTING_LIMIT, within: deadline(LISTING_LIMIT) };
    let bytes = fetch::get(&remote.url(&reg::blob_path(&version.hash)), &expect, cancel).map_err(|e| said(e.to_string()))?;
    let listing = listing_of(&bytes, version).map_err(said)?;
    let mut done = bytes.len() as u64;
    progress(Progress { done, total, installing: false });
    let manifest = listing.files.iter().find(|f| f.path == pack::PKG_MANIFEST).ok_or_else(|| said("it has no m2pkg.toml".into()))?;
    // Its m2pkg.toml first: a version this app can't run stops here.
    let rest = listing.files.iter().filter(|f| f.path != pack::PKG_MANIFEST);
    for (n, f) in std::iter::once(manifest).chain(rest).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(said(fetch::Error::Cancelled.to_string()));
        }
        let dest = work.0.join(&f.path);
        std::fs::create_dir_all(dest.parent().expect("inside the folder")).map_err(|e| said(format!("{}: {e}", f.path)))?;
        let expect = Expect { size: Some(f.size), sha256: Some(f.sha256), limit: f.size, within: deadline(f.size) };
        let base = done;
        let url = remote.url(&reg::blob_path(&f.sha256));
        fetch::get_to_file(&url, &dest, &expect, |p| progress(Progress { done: base + p.done, total, installing: false }), cancel)
            .map_err(|e| match e {
                fetch::Error::Cancelled => said(e.to_string()),
                e => said(format!("{}: {e}", f.path)),
            })?;
        done += f.size;
        progress(Progress { done, total, installing: false });
        if n == 0 {
            runs_here(&work.0, name, version.version, std).map_err(said)?;
        }
    }
    // What came is what is listed, and nothing else.
    if Listing::of(&work.0).ok().as_ref() != Some(&listing) {
        return Err(said("the files got aren't the ones its m2pack.json lists".into()));
    }
    // The point of no return: a cancel taken before it stops the Get here; one asked for after it
    // is refused, and the version is installed (or the install refused, leaving nothing).
    if !stop.commit() {
        return Err(said(fetch::Error::Cancelled.to_string()));
    }
    progress(Progress { done: total, total, installing: true });
    crate::packs::install(&work.0, store, std).map_err(|problems| said(problems.join("; ")))
}

/// Whether the version whose `m2pkg.toml` is in `dir` is `name`'s `version`, and this app runs it
/// with std in `std` (PACK-2). Why not, in words.
fn runs_here(dir: &Path, name: &PackName, version: Version, std: &Path) -> Result<(), String> {
    let m = PkgManifest::at(dir)?;
    if (&m.package.name, m.package.version) != (name, version) {
        return Err(format!("its m2pkg.toml is {} {}, not {name} {version}", m.package.name, m.package.version));
    }
    let sdk = Version::parse(m2_script::SDK_VERSION);
    let std = PkgManifest::at(std).ok().map(|s| s.package.version);
    pack::meets(&m.dependencies, sdk, std).map_err(|why| format!("it needs a newer music2: it {why}. Update music2 to get it"))
}

/// Get the sample `offer` (REG-16): its one file, into `into` under its own name. The sample, as
/// Add sample takes it (LIB-7): that file, and its note naming the pack.
pub fn sample(remote: &Remote, offer: &Offer, into: &Path, progress: &mut dyn FnMut(Progress), stop: &Cancel) -> Result<Sample, String> {
    let cancel = stop.flag();
    let said = |why: String| format!("Couldn't get {}: {why}", offer.item.label);
    let (Some(file), Some(mut sample)) = (&offer.facts.file, offer.item.sample.clone()) else {
        return Err(said("it isn't a sample".into()));
    };
    if file.size > crate::registry::LARGEST {
        return Err(said("the library says its file is more than the 10 GB a pack may be".into()));
    }
    let own = file.path.rsplit('/').next().filter(|n| not_plain(n).is_none()).ok_or_else(|| said("its file has no plain name".into()))?;
    let dest = into.join(own);
    let expect = Expect { size: Some(file.size), sha256: Some(file.sha256), limit: file.size, within: deadline(file.size) };
    let total = file.size;
    // One played already is kept: copied from there, checked as any download is.
    let kept = remote.blobs().join(file.sha256.hex());
    let copied = fetch::file_address(&kept).filter(|_| kept.is_file()).is_some_and(|url| {
        fetch::get_to_file(&url, &dest, &expect, |p| progress(Progress { done: p.done, total, installing: false }), cancel).is_ok()
    });
    if !copied {
        let url = remote.url(&reg::blob_path(&file.sha256));
        fetch::get_to_file(&url, &dest, &expect, |p| progress(Progress { done: p.done, total, installing: false }), cancel)
            .map_err(|e| said(e.to_string()))?;
    }
    progress(Progress { done: total, total, installing: false });
    sample.file = dest;
    Ok(sample)
}

/// One of a song's locked versions that isn't installed (REG-17).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    pub name: PackName,
    pub version: Version,
    pub hash: Sha,
    /// The catalog's version a Get gets; or why there is none, in words.
    pub got: Result<CatalogVersion, String>,
}

/// The versions `lock` names that `store` hasn't got (REG-17), each with whether `catalog` lists
/// that version with that very hash.
pub fn missing(lock: &Lock, store: &Store, catalog: Option<&Catalog>) -> Vec<Missing> {
    lock.packs
        .iter()
        .filter(|l| !store.versions(&l.name).contains(&l.version))
        .map(|l| {
            let at = format!("{} {}", l.name, l.version);
            let got = match catalog.map(|c| c.get(&l.name).map(|p| p.get(l.version))) {
                None => Err(format!("{at} isn't installed, and there is no catalog of the library here to get it from")),
                Some(None) => Err(format!("{at} isn't in the library: install it from its folder with `m2 pack install <folder>`")),
                Some(Some(None)) => Err(format!("{at} isn't in the library, only other versions of it")),
                Some(Some(Some(v))) if v.hash != l.hash => Err(format!(
                    "the library's {at} has other files than the ones this song was made with ({} names {}), so it isn't got",
                    m2_song::path::LOCK,
                    l.hash
                )),
                Some(Some(Some(v))) => match &v.removed {
                    Some(why) => Err(format!("{at} was taken down from the library ({why}), so its files are gone")),
                    None => Ok(v.clone()),
                },
            };
            Missing { name: l.name.clone(), version: l.version, hash: l.hash, got }
        })
        .collect()
}

/// The version of `name` `m2 pack get` gets (REG-17): `version`, else the latest. Why not, in
/// words: not listed, taken down, or every version yanked.
pub fn chosen<'a>(catalog: &'a Catalog, name: &PackName, version: Option<Version>) -> Result<&'a CatalogVersion, String> {
    let p = catalog.get(name).ok_or_else(|| format!("{name} isn't in the library: `m2 search` lists what is"))?;
    let listed = || p.versions.iter().map(|v| v.version.to_string()).collect::<Vec<_>>().join(", ");
    let v = match version {
        Some(want) => p.get(want).ok_or_else(|| format!("{name} {want} isn't in the library: it has {}", listed()))?,
        None => match p.latest.and_then(|l| p.get(l)) {
            Some(v) => v,
            None => return Err(format!("every version of {name} was yanked or taken down ({}): name one to get it", listed())),
        },
    };
    match &v.removed {
        Some(why) => Err(format!("{name} {} was taken down from the library ({why}), so its files are gone", v.version)),
        None => Ok(v),
    }
}

#[cfg(test)]
#[path = "get_tests.rs"]
mod tests;
