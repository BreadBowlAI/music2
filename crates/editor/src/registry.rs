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
/// a `file://` URL. Over the network once music2 has its download module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bucket {
    Folder(PathBuf),
}

impl Bucket {
    /// A bucket's address: `file:///…` (a whole path) or a folder's path. Why not, in words.
    pub fn parse(url: &str) -> Result<Bucket, String> {
        if let Some(path) = url.strip_prefix("file://") {
            return match Path::new(path).is_absolute() {
                true => Ok(Bucket::Folder(path.into())),
                false => Err(format!("`{url}` must name a whole path, file:///…")),
            };
        }
        if url.starts_with("https://") || url.starts_with("http://") {
            return Err(format!(
                "`{url}`: reading a bucket over the network needs music2's download module, which this m2 doesn't have yet. Copy \
                 the bucket's versions/ into a folder and give its path"
            ));
        }
        match url.contains("://") {
            true => Err(format!("`{url}` isn't a bucket's address: give a folder's path or file:///…")),
            false => Ok(Bucket::Folder(url.into())),
        }
    }

    /// The file at `path` in the bucket (one of [`reg::blob_path`], [`reg::facts_path`] …); none
    /// when it isn't there.
    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        match self {
            Bucket::Folder(dir) => match std::fs::read(dir.join(path)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("{} can't be read: {e}", dir.join(path).display())),
            },
        }
    }

    /// Whether the bucket has the file at `path`.
    pub fn has(&self, path: &str) -> bool {
        match self {
            Bucket::Folder(dir) => dir.join(path).is_file(),
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
/// facts of, by name and version: what a build builds, and whose sources the CI fetches (REG-9).
/// Removed versions are never built.
pub fn pending(registry: &Registry, out: &Path, from: Option<&Bucket>) -> Vec<(PackName, ListedVersion)> {
    let out = Bucket::Folder(out.to_path_buf());
    let mut todo: Vec<(PackName, ListedVersion)> = vec![];
    for p in &registry.packs {
        for v in p.versions.iter().filter(|v| v.removed.is_none()) {
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
    let listed = ListedVersion { version, hash, source: a.source.to_string(), yanked: None, removed: None };
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

/// What [`build`] builds.
pub struct Build<'a> {
    pub registry: &'a Path,
    /// The folder the CI unpacked the pending versions' sources into, each somewhere under it.
    pub sources: &'a Path,
    /// The folder it writes the bucket's new files into.
    pub out: &'a Path,
    /// The bucket as it is: what is there isn't built again.
    pub from: Option<&'a Bucket>,
    /// When the catalog is made ([`reg::is_time`]): the CI gives the registry commit's time, so
    /// one commit always makes the same catalog.
    pub made: &'a str,
    /// The std this app has: what the packs build and their previews render with.
    pub std: &'a Path,
    /// A folder to render previews in, removed after.
    pub work: &'a Path,
}

/// What [`build`] made.
#[derive(Clone, Debug)]
pub struct Built {
    /// The facts of each version built now, by name and version.
    pub built: Vec<Facts>,
    pub catalog: Catalog,
    pub pointer: Pointer,
    /// The blobs to delete from the bucket: the removed versions' that no other uses (REG-7).
    pub to_delete: Vec<Sha>,
    /// Removed versions whose facts weren't found, so whose blobs aren't known.
    pub unknown: Vec<String>,
}

/// `m2 registry build` (REG-5): for each version [`pending`], its source in `sources` checked
/// ([`refusals`]) and refused unless its files are the listed ones; only when every one passes,
/// each is published ([`publish`]). Then the catalog from every listed version's facts (from
/// `out`, else `from`) and `catalog.json`. Every problem, in words.
pub fn build(b: &Build) -> Result<Built, Vec<String>> {
    if !reg::is_time(b.made) {
        return Err(vec![format!("`{}` isn't a time as the catalog writes one, such as 2026-10-30T12:00:00Z (UTC)", b.made)]);
    }
    let registry = Registry::read(b.registry)?;
    let todo = pending(&registry, b.out, b.from);
    let (found, unread) = sources_in(b.sources);
    let mut checked = vec![];
    let mut problems = vec![];
    for (name, v) in &todo {
        let at = format!("{name} {}", v.version);
        match found.get(&(name.clone(), v.version)).map(Vec::as_slice) {
            Some([dir]) => match refusals(dir, name, v, &registry.tags, b.std) {
                Ok((listing, manifest)) => checked.push((name, v, dir.clone(), listing, manifest)),
                Err(e) => problems.extend(e),
            },
            Some(dirs) => problems.push(format!(
                "{at} is in two folders of the sources, {}: unpack each version once",
                dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(" and ")
            )),
            None => {
                let mut said = format!("{at} isn't built yet, and its source isn't in {}: unpack {} there", b.sources.display(), v.source);
                if !unread.is_empty() {
                    said += &format!(" (these m2pkg.toml files there don't read: {})", unread.join("; "));
                }
                problems.push(said);
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let out = Bucket::Folder(b.out.to_path_buf());
    let mut built = vec![];
    for (name, v, dir, listing, manifest) in checked {
        built.push(publish(&Publish { dir: &dir, name, version: v, listing: &listing, manifest: &manifest, b }).map_err(|e| vec![e])?);
    }
    let mut facts: BTreeMap<Sha, Facts> = BTreeMap::new();
    for p in &registry.packs {
        for v in &p.versions {
            let at = reg::facts_path(&v.hash);
            let bytes = match out.read(&at).map_err(|e| vec![e])? {
                Some(bytes) => Some(bytes),
                None => b.from.map(|f| f.read(&at)).transpose().map_err(|e| vec![e])?.flatten(),
            };
            match bytes.map(|bytes| Facts::read_for(&bytes, &p.name, v)) {
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
    let catalog = Catalog::make(&registry, &lookup, b.made)?;
    let pointer = catalog.pointer();
    let io = |e: std::io::Error| vec![format!("{} couldn't be written: {e}", b.out.display())];
    put(b.out, &reg::catalog_path(&pointer.catalog), &catalog.bytes()).map_err(io)?;
    put(b.out, reg::POINTER, &pointer.bytes()).map_err(io)?;
    let (to_delete, unknown) = registry.removed_blobs(&lookup);
    Ok(Built { built, catalog, pointer, to_delete, unknown })
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
        if d.join(pack::PKG_MANIFEST).is_file() {
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

/// Why the source of version `v` of `name` in `dir` is refused (REG-5), in words: unless its
/// files are the ones the registry lists (their listing's hash is `v`'s, and its own
/// `m2pack.json`, if it has one, is that listing), nothing else is looked at; then a licence the
/// registry takes (D6), tags from its word list (P7), 10 GB at most (D9), and what `m2 pack
/// check` checks (PACK-12). Its listing and manifest when it passes.
fn refusals(
    dir: &Path,
    name: &PackName,
    v: &ListedVersion,
    tags: &BTreeSet<String>,
    std: &Path,
) -> Result<(Listing, PkgManifest), Vec<String>> {
    let at = format!("{name} {}", v.version);
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
    let size = listing.files.iter().map(|f| f.size).sum::<u64>() + listing.text().len() as u64;
    if size > LARGEST {
        out.push(format!("{at} is {:.1} GB: the registry takes packs up to 10 GB (D9)", size as f64 / 1e9));
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

/// Put the blob `sha` into `out`, its bytes those of `read()`: unless `out` has it already (its
/// bytes checked) or `from` has it. Bytes that aren't `sha`'s are refused.
fn blob(out: &Path, from: Option<&Bucket>, sha: Sha, read: impl FnOnce() -> std::io::Result<Vec<u8>>) -> Result<(), String> {
    let rel = reg::blob_path(&sha);
    if Sha::of_file(&out.join(&rel)).is_ok_and(|h| h == sha) || from.is_some_and(|f| f.has(&rel)) {
        return Ok(());
    }
    let bytes = read().map_err(|e| e.to_string())?;
    if Sha::of(&bytes) != sha {
        return Err(format!("a file changed while it was copied into the bucket (it should be {sha}): build again"));
    }
    put(out, &rel, &bytes).map_err(|e| format!("{} couldn't be written: {e}", out.join(&rel).display()))
}

/// One checked version to publish.
struct Publish<'a> {
    dir: &'a Path,
    name: &'a PackName,
    version: &'a ListedVersion,
    listing: &'a Listing,
    manifest: &'a PkgManifest,
    b: &'a Build<'a>,
}

/// Publish a checked version (REG-5): its `m2pack.json` and every file into the bucket by hash, a
/// preview of each item that has one, and its facts, written last.
fn publish(p: &Publish) -> Result<Facts, String> {
    let (b, listing) = (p.b, p.listing);
    let text = listing.text();
    let mut blobs = BTreeSet::from([p.version.hash]);
    blob(b.out, b.from, p.version.hash, || Ok(text.clone().into_bytes()))?;
    for f in &listing.files {
        blob(b.out, b.from, f.sha256, || std::fs::read(p.dir.join(&f.path)))?;
        blobs.insert(f.sha256);
    }
    let packages = vec![(pack::STD.to_string(), b.std.to_path_buf()), (p.name.as_str().to_string(), p.dir.to_path_buf())];
    let catalog = m2_session::describe(&packages);
    let indexed = samples::index(samples::Root::Package { name: p.name.as_str(), dir: p.dir }, None);
    let lib =
        library::Library::of(&Sources { catalog: &catalog, packages: &packages, yours: None, plugins: &[], samples: &[Arc::new(indexed)] });
    let mut items = vec![];
    let _ = std::fs::remove_dir_all(b.work);
    for (n, item) in lib.all().filter(|i| i.package == p.name.as_str()).enumerate() {
        let (preview, no_preview) = match item.kind {
            Kind::Sample => sample_preview(item, listing),
            _ if !item.preview => (None, item.no_preview.clone()),
            _ => match render(item, &catalog, &packages, b.work, n) {
                Ok(wav) => {
                    let sha = Sha::of(&wav);
                    blob(b.out, b.from, sha, || Ok(wav))?;
                    (Some(sha), None)
                }
                Err(why) => (None, Some(format!("{}'s preview didn't render: {why}.", item.label))),
            },
        };
        blobs.extend(preview);
        items.extend(item_facts(item, listing, preview, no_preview));
    }
    let _ = std::fs::remove_dir_all(b.work);
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
    put(b.out, &reg::facts_path(&p.version.hash), &facts.bytes()).map_err(|e| format!("{} couldn't be written: {e}", b.out.display()))?;
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
mod tests;
