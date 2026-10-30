//! The registry's robot (REG-2, REG-5 to REG-9): what `m2 registry check`, `add`, `pending` and
//! `build` do. The formats it reads and writes are `m2_song::registry`'s.
//!
//! [`check`] reads a registry strictly and says what a pull request broke: a listed version
//! changed (P5), a pack changed by someone not its scope's. [`add`] lists a version of a pack in
//! its file, for its author. [`pending`] says which listed versions a bucket doesn't have yet, so
//! the registry's CI knows which sources to fetch. [`build`] checks each of those sources, refuses
//! one whose files aren't the ones listed, copies its files into the bucket by hash, renders its
//! previews with the app's own code, writes its facts, and then the catalog from every version's
//! facts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use m2_song::pack::{self, Listing, PackName, PkgManifest};
use m2_song::registry::{
    self as reg, About, Catalog, Facts, ItemFacts, ItemKind, ListedVersion, PackFile, Pointer, Registry, SampleFacts, SampleKind,
};
use m2_song::{Sha, Version};

use crate::library::{self, Item, Kind, SampleType, Sources, preview, samples};

/// The licences the registry takes (design D6): anyone may share a pack under them and use it in
/// a song they sell. For sounds the Creative Commons ones, for code the rest.
pub const LICENCES: &[&str] = &["CC0-1.0", "CC-BY-3.0", "CC-BY-4.0", "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC"];
/// The largest version the registry takes, in bytes (design D9): 10 GB.
pub const LARGEST: u64 = 10_000_000_000;

/// A bucket the robot reads what is there already from (`--from`, REG-9): a folder, by its path or
/// a `file://` URL, or a web address read through music2's downloads (`crate::fetch`: `https://`,
/// or `http://` to this computer only, NET-3). It must reach the bucket itself, never a cache in
/// front of it (REG-7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bucket {
    Folder(PathBuf),
    /// The bucket's address, with no `/` at its end.
    Web(String),
}

/// The most a file read from a bucket over the network may be, and the longest it may take, by
/// its kind (NET-4): `catalog.json` is a line, a version's facts list its blobs and items, and a
/// catalog lists every pack's.
fn web_limits(path: &str) -> (u64, Duration) {
    match path {
        reg::POINTER => (64 << 10, Duration::from_secs(30)),
        p if p.starts_with("versions/") => (64 << 20, Duration::from_secs(120)),
        p if p.starts_with("catalog/") => (512 << 20, Duration::from_secs(600)),
        _ => (16 << 30, Duration::from_secs(3600)),
    }
}

impl Bucket {
    /// A bucket's address: `https://…` (`http://` only to this computer), `file:///…` (a whole
    /// path) or a folder's path. Why not, in words.
    pub fn parse(url: &str) -> Result<Bucket, String> {
        if let Some(path) = url.strip_prefix("file://") {
            return match Path::new(path).is_absolute() {
                true => Ok(Bucket::Folder(path.into())),
                false => Err(format!("`{url}` must name a whole path, file:///…")),
            };
        }
        if url.starts_with("https://") || url.starts_with("http://") {
            crate::fetch::check(url).map_err(|e| format!("`{url}`: {e}"))?;
            return Ok(Bucket::Web(url.trim_end_matches('/').to_string()));
        }
        match url.contains("://") {
            true => Err(format!("`{url}` isn't a bucket's address: give https://…, a folder's path or file:///…")),
            false => Ok(Bucket::Folder(url.into())),
        }
    }

    /// The file at `path` in the bucket (one of [`reg::blob_path`], [`reg::facts_path`] …); none
    /// when it isn't there (over the network: the server answers 404 or 410).
    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        match self {
            Bucket::Folder(dir) => match std::fs::read(dir.join(path)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("{} can't be read: {e}", dir.join(path).display())),
            },
            Bucket::Web(base) => {
                let (limit, within) = web_limits(path);
                let expect = crate::fetch::Expect { size: None, sha256: None, limit, within };
                match crate::fetch::get(&format!("{base}/{path}"), &expect, &std::sync::atomic::AtomicBool::new(false)) {
                    Ok(bytes) => Ok(Some(bytes)),
                    Err(crate::fetch::Error::Status { code: 404 | 410, .. }) => Ok(None),
                    Err(e) => Err(format!("{path} couldn't be read from the bucket: {e}")),
                }
            }
        }
    }

    /// Whether the bucket has the file at `path`. Over the network a blob is never asked for (it
    /// could be gigabytes): it is taken as not there, so it is put again, and the copy into the
    /// bucket skips one there already. A file that can't be read is taken as not there, so the
    /// most it costs is building a version again.
    pub fn has(&self, path: &str) -> bool {
        match self {
            Bucket::Folder(dir) => dir.join(path).is_file(),
            Bucket::Web(_) if path.starts_with("blobs/") => false,
            Bucket::Web(_) => self.read(path).is_ok_and(|b| b.is_some()),
        }
    }
}

/// What [`check`] checks.
pub struct Check<'a> {
    pub registry: &'a Path,
    /// The registry as it was (a pull request's base): what it may not break (P5).
    pub base: Option<&'a Path>,
    /// The GitHub account that made the change: it may change only its own scope's packs.
    pub author: Option<&'a str>,
}

/// `m2 registry check` (REG-2): the registry read strictly (REG-1); with a base, no listed version
/// changed or gone; with an author, every pack changed from the base theirs. The registry, or
/// every problem in words.
pub fn check(c: &Check) -> Result<Registry, Vec<String>> {
    let now = Registry::read(c.registry)?;
    let base = match c.base {
        Some(b) => Some(
            Registry::read(b).map_err(|p| p.into_iter().map(|e| format!("the registry as it was (--base): {e}")).collect::<Vec<_>>())?,
        ),
        None => None,
    };
    let mut problems = base.as_ref().map(|b| now.broken_promises(b)).unwrap_or_default();
    if let Some(author) = c.author {
        problems.extend(now.not_authors(base.as_ref(), author));
    }
    match problems.is_empty() {
        true => Ok(now),
        false => Err(problems),
    }
}

/// The listed versions `out` (the folder a build writes) and `from` (the bucket) haven't the
/// facts of, by name and version: what a build builds, and whose sources the CI fetches (REG-9),
/// each into `<sources>/<its hash's hex>/`. Taken-down versions are never built.
pub fn pending(registry: &Registry, out: &Path, from: Option<&Bucket>) -> Vec<(PackName, ListedVersion)> {
    let out = Bucket::Folder(out.to_path_buf());
    let mut todo: Vec<(PackName, ListedVersion)> = vec![];
    for p in &registry.packs {
        for v in p.versions.iter().filter(|v| registry.removal(&p.name, v.version).is_none()) {
            let at = reg::facts_path(&v.hash);
            if !out.has(&at) && !from.is_some_and(|f| f.has(&at)) {
                todo.push((p.name.clone(), v.clone()));
            }
        }
    }
    todo.sort_by(|a, b| (&a.0, a.1.version).cmp(&(&b.0, b.1.version)));
    todo
}

/// What [`add`] lists.
pub struct Add<'a> {
    pub registry: &'a Path,
    /// The pack's folder.
    pub pack: &'a Path,
    /// Where the robot will get its files: a `.tar.gz` of the folder, under the scope's GitHub.
    pub source: &'a str,
    /// The std this app has, which the pack builds against.
    pub std: &'a Path,
}

/// What [`add`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Added {
    /// The version is listed in `file`, a `new` one or the one there.
    Listed { name: PackName, version: Version, hash: Sha, file: PathBuf, new: bool },
    /// It was listed already, with these very files and this source.
    Already { name: PackName, version: Version },
}

/// `m2 registry add` (REG-8): check the pack as `m2 pack check` does, write its `m2pack.json`, and
/// list its version in its pack file: a new file with its name and its `m2pkg.toml`'s repository,
/// or at the end of the file there, as it is. The same version listed with other files or another
/// source is refused (P5); what is written must read, its addresses its scope's (REG-1).
pub fn add(a: &Add) -> Result<Added, Vec<String>> {
    let problems = crate::packs::check(a.pack, a.std);
    if !problems.is_empty() {
        return Err(problems);
    }
    let m = PkgManifest::at(a.pack).map_err(|e| vec![e])?;
    let (name, version) = (m.package.name, m.package.version);
    let listing = Listing::of(a.pack).map_err(|e| vec![format!("{} can't be listed: {e}", a.pack.display())])?;
    let hash = listing.hash();
    let rel = PackFile::path_of(&name);
    let file = a.registry.join(&rel);
    let listed = ListedVersion { version, hash, source: a.source.to_string(), yanked: None };
    let at = |e: String| vec![format!("{}: {e}", rel.display())];
    let (text, new) = match std::fs::read_to_string(&file) {
        Ok(text) => {
            let p = PackFile::read(&text).map_err(at)?;
            match p.get(version) {
                Some(v) if v.hash == hash && v.source == a.source => {
                    std::fs::write(a.pack.join(pack::LISTING), listing.text()).map_err(|e| vec![e.to_string()])?;
                    return Ok(Added::Already { name, version });
                }
                Some(_) => {
                    return Err(vec![format!(
                        "{name} {version} is listed already with other files or another source: a listed version is never changed \
                         (P5). Give these files a new version in m2pkg.toml"
                    )]);
                }
                None => {}
            }
            let gap = if text.ends_with("\n\n") || text.is_empty() {
                ""
            } else if text.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            (format!("{text}{gap}{}", PackFile::version_text(&listed)), false)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let repository = m.package.repository.ok_or_else(|| {
                vec![format!(
                    "m2pkg.toml names no repository: add `repository = \"https://github.com/{}/<repo>\"` under [package], where its \
                     code is",
                    reg::github_owner(&name)
                )]
            })?;
            (PackFile { name: name.clone(), repository, versions: vec![listed] }.text(), true)
        }
        Err(e) => return Err(at(format!("can't be read ({e})"))),
    };
    PackFile::read(&text).map_err(at)?;
    let written = (|| {
        std::fs::create_dir_all(file.parent().expect("a pack file is in its owner's folder"))?;
        std::fs::write(a.pack.join(pack::LISTING), listing.text())?;
        std::fs::write(&file, &text)
    })();
    written.map_err(|e| vec![format!("{} couldn't be written: {e}", file.display())])?;
    Ok(Added::Listed { name, version, hash, file, new })
}

/// How long the robot gives one version (REG-5): its check, its copy into the bucket and its
/// previews. A kernel's code first runs as a preview renders, and music2's engine sets a kernel no
/// fuel or epoch limit (a live kernel that never returns is a broken real-time rule, P4, which a
/// test of the kernel catches), so a version past it is refused and its thread, which can't be
/// stopped, is left to end with the robot. Its code in TypeScript has QuickJS's own limit.
pub const LONGEST: Duration = Duration::from_secs(10 * 60);

/// What [`build`] builds.
pub struct Build<'a> {
    pub registry: &'a Path,
    /// The folder the CI unpacked each pending version's source into: `<sources>/<its hash's hex>/`
    /// ([`pending`] prints the hex), so one source can never stand in for another's.
    pub sources: &'a Path,
    /// The folder it writes the bucket's new files into.
    pub out: &'a Path,
    /// The bucket as it is: what is there isn't built again, and what it publishes never changes.
    pub from: Option<&'a Bucket>,
    /// When the catalog is made ([`reg::is_time`]): the CI gives the registry commit's time, so
    /// one commit always makes the same catalog.
    pub made: &'a str,
    /// The std this app has: what the packs build and their previews render with.
    pub std: &'a Path,
    /// A folder for each version's work (its previews, and its files before they are kept), removed
    /// after.
    pub work: &'a Path,
    /// Where to write [`Built::to_delete`], one bucket path a line (REG-7), if anywhere.
    pub to_delete: Option<&'a Path>,
    /// How long one version may take: [`LONGEST`].
    pub limit: Duration,
}

/// A pending version that wasn't built this time, and why: it stays pending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failed {
    pub name: PackName,
    pub version: Version,
    pub problems: Vec<String>,
}

/// What [`build`] made.
#[derive(Clone, Debug)]
pub struct Built {
    /// The facts of each version built now, by name and version.
    pub built: Vec<Facts>,
    /// The pending versions that weren't built: each is left out of the catalog, and nothing of it
    /// is written. The others are built all the same.
    pub failed: Vec<Failed>,
    pub catalog: Catalog,
    pub pointer: Pointer,
    /// The blobs to delete from the bucket: the taken-down versions' that no other uses (REG-7).
    pub to_delete: Vec<Sha>,
    /// Taken-down versions whose facts weren't found, so whose blobs aren't known.
    pub unknown: Vec<String>,
}

/// The catalog `bucket` publishes now, if it has one, read through its `catalog.json` and
/// checked by hash (REG-3).
fn published(bucket: &Bucket) -> Result<Option<Catalog>, String> {
    let Some(bytes) = bucket.read(reg::POINTER)? else { return Ok(None) };
    let pointer = Pointer::read(&bytes)?;
    let at = reg::catalog_path(&pointer.catalog);
    let bytes = bucket.read(&at)?.ok_or_else(|| format!("{} names the catalog {at}, which isn't there", reg::POINTER))?;
    Catalog::read_checked(&bytes, &pointer).map(Some)
}

/// `m2 registry build` (REG-5). First what is published (the catalogs at `from` and `out`) is
/// checked against the registry ([`Registry::breaks`]): a published version changed, a take-down
/// taken back or a hash taken refuses the whole build, by anyone. Then each version [`pending`] is
/// built on its own ([`one`]): its source found in `sources/<hex>/`, checked ([`refusals`]),
/// published into a folder of its own, and kept in `out` only when all of it passed. A version
/// that fails is reported and stays pending; the others go on. Then the catalog from every listed
/// version's facts there (from `out`, else `from`) and `catalog.json`.
pub fn build(b: &Build) -> Result<Built, Vec<String>> {
    if !reg::is_time(b.made) {
        return Err(vec![format!("`{}` isn't a time as the catalog writes one, such as 2026-10-30T12:00:00Z (UTC)", b.made)]);
    }
    let registry = Registry::read(b.registry)?;
    let out = Bucket::Folder(b.out.to_path_buf());
    let mut problems = vec![];
    for (bucket, flag) in b.from.into_iter().map(|f| (f, "--from")).chain([(&out, "--out")]) {
        match published(bucket) {
            Ok(Some(c)) => problems.extend(registry.breaks(&c)),
            Ok(None) => {}
            Err(e) => problems.push(format!("what {flag} publishes can't be read, so nothing is built: {e}")),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let (mut built, mut failed) = (vec![], vec![]);
    for (name, v) in pending(&registry, b.out, b.from) {
        match one(b, &registry.tags, &name, &v) {
            Ok(f) => built.push(f),
            Err(problems) => failed.push(Failed { name, version: v.version, problems }),
        }
    }
    let _ = std::fs::remove_dir_all(b.work);
    let mut facts: BTreeMap<Sha, Facts> = BTreeMap::new();
    for p in &registry.packs {
        for v in &p.versions {
            let at = reg::facts_path(&v.hash);
            let bytes = match out.read(&at).map_err(|e| vec![e])? {
                Some(bytes) => Some(bytes),
                None => b.from.map(|f| f.read(&at)).transpose().map_err(|e| vec![e])?.flatten(),
            };
            match bytes.map(|bytes| Facts::read_for(&bytes, &p.name, v.version, v.hash)) {
                Some(Ok(f)) => drop(facts.insert(v.hash, f)),
                Some(Err(e)) => problems.push(e),
                None => {}
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let lookup = |h: &Sha| facts.get(h).cloned();
    let (catalog, _) = Catalog::make(&registry, &lookup, b.made);
    let pointer = catalog.pointer();
    let io = |e: std::io::Error| vec![format!("{} couldn't be written: {e}", b.out.display())];
    put(b.out, &reg::catalog_path(&pointer.catalog), &catalog.bytes()).map_err(io)?;
    put(b.out, reg::POINTER, &pointer.bytes()).map_err(io)?;
    let (to_delete, unknown) = registry.removed_blobs(&lookup);
    if let Some(file) = b.to_delete {
        let lines: String = to_delete.iter().map(|h| format!("{}\n", reg::blob_path(h))).collect();
        std::fs::write(file, lines).map_err(|e| vec![format!("{} couldn't be written: {e}", file.display())])?;
    }
    Ok(Built { built, failed, catalog, pointer, to_delete, unknown })
}

/// One pending version, built on a thread of its own in `work/<hex>/` and given `b.limit`
/// ([`LONGEST`]): its facts, its files moved into `out` only once all of it passed; or why not,
/// and nothing of it in `out`.
fn one(b: &Build, tags: &BTreeSet<String>, name: &PackName, v: &ListedVersion) -> Result<Facts, Vec<String>> {
    let at = format!("{name} {}", v.version);
    let stage = b.work.join(v.hash.hex());
    let _ = std::fs::remove_dir_all(&stage);
    let job = Job {
        name: name.clone(),
        version: v.clone(),
        src: b.sources.join(v.hash.hex()),
        stage: stage.clone(),
        tags: tags.clone(),
        std: b.std.to_path_buf(),
        out: b.out.to_path_buf(),
        from: b.from.cloned(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("music2-registry".into()).stack_size(16 << 20).spawn(move || {
        let _ = tx.send(job.run());
    });
    spawned.map_err(|e| vec![format!("{at}: no thread to build it on ({e})")])?;
    let facts = match rx.recv_timeout(b.limit) {
        Ok(done) => done,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            // Its thread may still be at work in `stage`: it is left there, and never kept.
            return Err(vec![format!(
                "{at} took longer than {} s to check and build, so it isn't built: its code or a kernel may never return (P4)",
                b.limit.as_secs()
            )]);
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(vec![format!("{at}: the robot stopped while building it, a fault in music2")])
        }
    };
    let kept = facts.and_then(|f| keep(&stage.join("bucket"), b.out).map(|()| f).map_err(|e| vec![format!("{at}: {e}")]));
    let _ = std::fs::remove_dir_all(&stage);
    kept
}

/// Move what a version's build put in `stage` into `out`: its blobs, then its facts, so a fact is
/// never there without its files. One `out` has already is left as it is.
fn keep(stage: &Path, out: &Path) -> Result<(), String> {
    for dir in ["blobs/sha256", "catalog", "versions"] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(stage.join(dir)).into_iter().flatten().flatten().map(|e| e.path()).collect();
        files.sort();
        for from in files {
            let to = out.join(dir).join(from.file_name().expect("a file"));
            if to.is_file() {
                continue;
            }
            std::fs::create_dir_all(to.parent().expect("a folder")).map_err(|e| format!("{} couldn't be made: {e}", out.display()))?;
            // A rename where it can; across disks, a copy through a part file.
            if std::fs::rename(&from, &to).is_err() {
                let part = to.with_extension(format!("{}.part", std::process::id()));
                std::fs::copy(&from, &part).and_then(|_| std::fs::rename(&part, &to)).map_err(|e| {
                    let _ = std::fs::remove_file(&part);
                    format!("{} couldn't be written: {e}", to.display())
                })?;
            }
        }
    }
    Ok(())
}

/// One version's build, on its own thread ([`one`]).
struct Job {
    name: PackName,
    version: ListedVersion,
    /// Where the CI unpacked its source.
    src: PathBuf,
    /// Its own folder: its files before they are kept (`bucket/`), and its previews' work.
    stage: PathBuf,
    tags: BTreeSet<String>,
    std: PathBuf,
    out: PathBuf,
    from: Option<Bucket>,
}

impl Job {
    /// Find its source in `src` (the one folder there whose `m2pkg.toml` names this pack and
    /// version), check it, and publish it into `stage`.
    fn run(self) -> Result<Facts, Vec<String>> {
        let (name, v) = (&self.name, &self.version);
        let at = format!("{name} {}", v.version);
        let (found, unread) = sources_in(&self.src);
        let unread = match unread.is_empty() {
            true => String::new(),
            false => format!(" (these m2pkg.toml files there don't read: {})", unread.join("; ")),
        };
        let dir = match found.get(&(name.clone(), v.version)).map(Vec::as_slice) {
            Some([dir]) => dir.clone(),
            Some(dirs) => {
                return Err(vec![format!(
                    "{at} is in two folders of its source, {}: its .tar.gz must hold it once",
                    dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(" and ")
                )]);
            }
            None if !self.src.is_dir() => {
                return Err(vec![format!(
                    "{at} isn't built yet, and its source isn't there: unpack {} into {}",
                    v.source,
                    self.src.display()
                )]);
            }
            None => {
                let held: Vec<String> = found.keys().map(|(n, v)| format!("{n} {v}")).collect();
                let held = if held.is_empty() { "no pack".to_string() } else { held.join(", ") };
                return Err(vec![format!("{at}: its source, {}, holds {held}, not {at}{unread}", v.source)]);
            }
        };
        let (listing, manifest) = refusals(&dir, name, v, &self.tags, &self.std)?;
        let p = Publish { dir: &dir, name, version: v, listing: &listing, manifest: &manifest, job: &self };
        publish(&p).map_err(|e| vec![format!("{at}: {e}")])
    }
}

/// The folders of each pack and version: one, unless a source was unpacked twice.
type Unpacked = BTreeMap<(PackName, Version), Vec<PathBuf>>;

/// The pack folders under `dir`: each folder holding an `m2pkg.toml`, by the name and version it
/// says (not looked inside further), never through a link or a hidden folder; and the
/// `m2pkg.toml` files that don't read, in words.
fn sources_in(dir: &Path) -> (Unpacked, Vec<String>) {
    let (mut found, mut unread) = (Unpacked::new(), vec![]);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if std::fs::symlink_metadata(d.join(pack::PKG_MANIFEST)).is_ok_and(|m| m.file_type().is_file()) {
            match PkgManifest::at(&d) {
                Ok(m) => found.entry((m.package.name, m.package.version)).or_default().push(d),
                Err(e) => unread.push(e),
            }
            continue;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let hidden = e.file_name().to_string_lossy().starts_with('.');
            if !hidden && e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    for dirs in found.values_mut() {
        dirs.sort();
    }
    (found, unread)
}

/// Whether `licence` (an SPDX expression: ids joined by `AND` and `OR`) names only licences the
/// registry takes (D6).
fn licence_ok(licence: &str) -> Result<(), String> {
    let ids: Vec<&str> = licence
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|w| !w.is_empty() && !matches!(*w, "AND" | "OR"))
        .collect();
    match ids.iter().find(|id| !LICENCES.contains(id)) {
        None if !ids.is_empty() => Ok(()),
        _ => Err(format!(
            "licence {licence} isn't one the registry takes: it takes {} (D6), so anyone may share the pack and use it in a song \
             they sell",
            LICENCES.join(", ")
        )),
    }
}

/// Why the source of version `v` of `name` in `dir` is refused (REG-5), in words. First what
/// needs no file opened: only folders and plain files (no link, pipe or device), and 10 GB at most
/// by their sizes (D9). Then unless its files are the ones the registry lists (their listing's hash
/// is `v`'s, and its own `m2pack.json`, if it has one, is that listing), nothing else is looked
/// at; then a licence the registry takes (D6), tags from its word list (P7), and what `m2 pack
/// check` checks (PACK-12). Its listing and manifest when it passes.
fn refusals(
    dir: &Path,
    name: &PackName,
    v: &ListedVersion,
    tags: &BTreeSet<String>,
    std: &Path,
) -> Result<(Listing, PkgManifest), Vec<String>> {
    let at = format!("{name} {}", v.version);
    let files = pack::pack_files(dir).map_err(|e| vec![format!("{at}: {e}")])?;
    let size: u64 = files.iter().map(|(_, p)| std::fs::symlink_metadata(p).map_or(0, |m| m.len())).sum();
    if size > LARGEST {
        return Err(vec![format!("{at} is {:.1} GB: the registry takes packs up to 10 GB (D9)", size as f64 / 1e9)]);
    }
    let m = PkgManifest::at(dir).map_err(|e| vec![format!("{at}: {e}")])?;
    let listing = Listing::of(dir).map_err(|e| vec![format!("{at}: its files at {} can't be listed: {e}", dir.display())])?;
    if listing.hash() != v.hash {
        return Err(vec![format!(
            "{at}: the files at {} aren't the files the registry lists: their m2pack.json's hash would be {}, and the registry lists \
             {}. Unpack exactly its source there",
            dir.display(),
            listing.hash(),
            v.hash
        )]);
    }
    if std::fs::read(dir.join(pack::LISTING)).is_ok_and(|own| own != listing.text().as_bytes()) {
        return Err(vec![format!(
            "{at}: its own m2pack.json isn't the listing of its files: make it again with `m2 pack build`, or leave it out"
        )]);
    }
    let mut out = vec![];
    match m.package.license.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => out.extend(licence_ok(l).err().map(|e| format!("{at}: {e}"))),
        None => out.push(format!("{at}: m2pkg.toml names no licence")),
    }
    for t in m.package.tags.iter().filter(|t| !tags.contains(*t)) {
        out.push(format!("{at}: tag `{t}` isn't in the registry's word list ({}): use its words, or ask for a new one", reg::TAGS));
    }
    out.extend(crate::packs::check(dir, std).into_iter().map(|p| format!("{at}: {p}")));
    match out.is_empty() {
        true => Ok((listing, m)),
        false => Err(out),
    }
}

/// Write `bytes` to `rel` in `dir` through a part file renamed into place, so a write that stops
/// leaves nothing half there.
fn put(dir: &Path, rel: &str, bytes: &[u8]) -> std::io::Result<()> {
    let to = dir.join(rel);
    std::fs::create_dir_all(to.parent().expect("a file in the bucket"))?;
    let part = to.with_extension(format!("{}.part", std::process::id()));
    let written = std::fs::write(&part, bytes).and_then(|()| std::fs::rename(&part, &to));
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    written
}

/// Whether the blob `sha` of `size` bytes needs writing: not when `out` has it (written only
/// through a part file, its bytes checked) or `from` has it.
fn needed(job: &Job, sha: &Sha, size: u64) -> bool {
    let rel = reg::blob_path(sha);
    !(std::fs::metadata(job.out.join(&rel)).is_ok_and(|m| m.len() == size) || job.from.as_ref().is_some_and(|f| f.has(&rel)))
}

/// Put the blob `sha`, these `bytes`, into the job's own bucket folder, unless it isn't needed.
fn blob_bytes(job: &Job, sha: Sha, bytes: &[u8]) -> Result<(), String> {
    if !needed(job, &sha, bytes.len() as u64) {
        return Ok(());
    }
    let dir = job.stage.join("bucket");
    put(&dir, &reg::blob_path(&sha), bytes).map_err(|e| format!("{} couldn't be written: {e}", dir.display()))
}

/// Put the blob `sha`, the file at `from` of `size` bytes, into the job's own bucket folder,
/// unless it isn't needed: copied as it streams, never held whole, and its copy's hash checked.
fn blob_file(job: &Job, sha: Sha, size: u64, from: &Path) -> Result<(), String> {
    if !needed(job, &sha, size) {
        return Ok(());
    }
    let to = job.stage.join("bucket").join(reg::blob_path(&sha));
    let part = to.with_extension("part");
    let copied = (|| {
        std::fs::create_dir_all(to.parent().expect("a file in the bucket"))?;
        std::fs::copy(from, &part)?;
        match Sha::of_file(&part)? == sha {
            true => std::fs::rename(&part, &to),
            false => Err(std::io::Error::other(format!("{} changed while it was copied: build again", from.display()))),
        }
    })();
    if copied.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    copied.map_err(|e| e.to_string())
}

/// One checked version to publish.
struct Publish<'a> {
    dir: &'a Path,
    name: &'a PackName,
    version: &'a ListedVersion,
    listing: &'a Listing,
    manifest: &'a PkgManifest,
    job: &'a Job,
}

/// Publish a checked version (REG-5) into its job's own bucket folder: its `m2pack.json` and
/// every file by hash, a preview of each item that has one, and its facts, written last.
fn publish(p: &Publish) -> Result<Facts, String> {
    let (job, listing) = (p.job, p.listing);
    let text = listing.text();
    let mut blobs = BTreeSet::from([p.version.hash]);
    blob_bytes(job, p.version.hash, text.as_bytes())?;
    for f in &listing.files {
        blob_file(job, f.sha256, f.size, &p.dir.join(&f.path))?;
        blobs.insert(f.sha256);
    }
    let packages = vec![(pack::STD.to_string(), job.std.clone()), (p.name.as_str().to_string(), p.dir.to_path_buf())];
    let catalog = m2_session::describe(&packages);
    let indexed = samples::index(samples::Root::Package { name: p.name.as_str(), dir: p.dir }, None);
    let lib =
        library::Library::of(&Sources { catalog: &catalog, packages: &packages, yours: None, plugins: &[], samples: &[Arc::new(indexed)] });
    let work = job.stage.join("work");
    let mut items = vec![];
    for (n, item) in lib.all().filter(|i| i.package == p.name.as_str()).enumerate() {
        let (preview, no_preview) = match item.kind {
            Kind::Sample => sample_preview(item, listing),
            _ if !item.preview => (None, item.no_preview.clone()),
            _ => match render(item, &catalog, &packages, &work, n) {
                Ok(wav) => {
                    let sha = Sha::of(&wav);
                    blob_bytes(job, sha, &wav)?;
                    (Some(sha), None)
                }
                Err(why) => (None, Some(format!("{}'s preview didn't render: {why}.", item.label))),
            },
        };
        blobs.extend(preview);
        items.extend(item_facts(item, listing, preview, no_preview));
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    let m = &p.manifest.package;
    let facts = Facts {
        format: reg::FORMAT,
        name: p.name.clone(),
        version: p.version.version,
        hash: p.version.hash,
        about: About {
            description: m.description.clone(),
            license: m.license.clone().unwrap_or_default(),
            authors: m.authors.clone(),
            tags: m.tags.clone(),
        },
        size: listing.files.iter().map(|f| f.size).sum::<u64>() + text.len() as u64,
        files: listing.files.len() as u64 + 1,
        blobs: blobs.into_iter().collect(),
        items,
        made_by: preview::made_by(),
    };
    let dir = job.stage.join("bucket");
    put(&dir, &reg::facts_path(&p.version.hash), &facts.bytes()).map_err(|e| format!("{} couldn't be written: {e}", dir.display()))?;
    Ok(facts)
}

/// An item's preview rendered (REG-5): the song the app's ⌘F writes for it ([`preview::of`]),
/// rendered by its code ([`preview::render`]) in `work`. The WAV's bytes, or why not.
fn render(item: &Item, catalog: &m2_script::Catalog, packages: &[(String, PathBuf)], work: &Path, n: usize) -> Result<Vec<u8>, String> {
    let (song, loads) = preview::of(item, catalog, packages, None)?;
    let wav = work.join(format!("preview-{n}.wav"));
    preview::render(&song, &loads, &work.join(format!("song-{n}")), &wav).map_err(|e| format!("{e:#}"))?;
    let bytes = std::fs::read(&wav).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&wav);
    bytes
}

/// A sample's preview (LIB-6): its own file, unless it is longer than the app previews.
fn sample_preview(item: &Item, listing: &Listing) -> (Option<Sha>, Option<String>) {
    match own_file(item, listing) {
        Some(f) if f.size <= samples::PREVIEW_BYTES => (Some(f.sha256), None),
        Some(f) => (
            None,
            Some(format!(
                "{} is {} MB, too long to preview here (up to {} MB): add it to the song to hear it.",
                item.label,
                f.size / 1_000_000,
                samples::PREVIEW_BYTES / 1_000_000
            )),
        ),
        None => (None, Some(format!("{} isn't one of its version's files.", item.label))),
    }
}

/// A sample's own file, as its version lists it: the path its component names.
fn own_file<'a>(item: &Item, listing: &'a Listing) -> Option<&'a m2_song::pack::Listed> {
    let (_, path) = pack::split_id(&item.component)?;
    listing.files.iter().find(|f| f.path == path)
}

/// The facts of a library item (REG-4): none for a plugin, which no pack holds.
fn item_facts(item: &Item, listing: &Listing, preview: Option<Sha>, no_preview: Option<String>) -> Option<ItemFacts> {
    let kind = match item.kind {
        Kind::Instrument => ItemKind::Instrument,
        Kind::Sound => ItemKind::Sound,
        Kind::Effect => ItemKind::Effect,
        Kind::Sample => ItemKind::Sample,
        Kind::Plugin => return None,
    };
    let sample = item.sample.as_ref().map(|s| SampleFacts {
        kind: match s.kind {
            SampleType::Loop => SampleKind::Loop,
            SampleType::OneShot => SampleKind::OneShot,
        },
        instrument: s.instrument.clone(),
        bpm: s.bpm,
        // As the library writes it: `Am`, `F#`.
        key: s.key.and_then(|k| serde_json::to_value(k).ok()?.as_str().map(String::from)),
        seconds: s.seconds.unwrap_or(0.0),
        channels: s.channels,
        rate: s.rate,
        peak: s.peak,
    });
    Some(ItemFacts {
        kind,
        id: item.id.clone(),
        label: item.label.clone(),
        category: item.category.clone(),
        description: item.description.clone(),
        tags: item.tags.clone(),
        component: item.component.clone(),
        instrument: item.instrument,
        effect: item.effect,
        preset: item.preset.as_ref().map(|p| p.name.clone()),
        file: (kind == ItemKind::Sample).then(|| own_file(item, listing).cloned()).flatten(),
        sample,
        preview,
        no_preview,
    })
}

#[cfg(test)]
#[path = "registry_tests.rs"]
pub(crate) mod tests;
