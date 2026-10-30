//! Packs (PACK-1 to PACK-9): packages other than std, from the store or a folder.
//!
//! ```text
//! PackName      std, or @owner/name (PACK-1): the one rule for a package's name
//! split_id      <package>:<id>, split at the first `:` (PACK-4)
//! AssetRef      a song's asset, or <package>:<path>, a package's own file (PACK-5)
//! PkgManifest   m2pkg.toml, read strictly (PACK-2)
//! Listing       m2pack.json: every file of an installed version, its size and hash (PACK-6)
//! Store         ~/Music/music2/Packs/<name>/<version>/ (PACK-6)
//! Lock          music2.lock: the exact version and hash of each pack a song uses (PACK-8)
//! resolve       the packs a song builds with, and what keeps one from it (PACK-9)
//! relock        the lock `m2 pack lock` and `m2 pack update` write (PACK-8)
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diag::Diagnostic;
use crate::hash::{HashCache, Sha};
use crate::manifest::{Dependency, Manifest, Requirement, SDK, Version};
use crate::path::{LOCK, MANIFEST};

/// The package the app bundles.
pub const STD: &str = "std";
/// A package's manifest, at its root.
pub const PKG_MANIFEST: &str = "m2pkg.toml";
/// An installed version's listing, at its root.
pub const LISTING: &str = "m2pack.json";

/// A package's name (PACK-1): `std`, or `@owner/name`, each part 1 to 39 characters of `a-z`,
/// `0-9` and `-`, neither starting nor ending with `-`. Never `@m2/core`, the SDK, and never a `:`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackName(String);

/// The longest owner or name.
const PART: usize = 39;

impl PackName {
    pub fn new(s: &str) -> Result<PackName, String> {
        if s == STD {
            return Ok(PackName(s.into()));
        }
        if s == SDK {
            return Err(format!("`{SDK}` is the SDK, which every song has, never a package"));
        }
        let part = |p: &str| {
            (1..=PART).contains(&p.len())
                && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !p.starts_with('-')
                && !p.ends_with('-')
        };
        match s.strip_prefix('@').and_then(|r| r.split_once('/')) {
            Some((owner, name)) if part(owner) && part(name) => Ok(PackName(s.into())),
            _ => Err(format!(
                "`{s}` isn't a package name: write `std`, or `@owner/name`, each part 1 to {PART} lowercase letters, digits and inner \
                 hyphens, such as `@alice/felt-piano`"
            )),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_std(&self) -> bool {
        self.0 == STD
    }

    /// Its folder in the store, relative: `@alice/felt-piano`.
    pub fn folder(&self) -> PathBuf {
        self.0.split('/').collect()
    }
}

impl std::fmt::Display for PackName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for PackName {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PackName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<PackName, D::Error> {
        PackName::new(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// A ComponentId, a kernel id or a plugin's id split into its package and the rest, at the first
/// `:` (PACK-4). None for a project's own component, which has no package.
pub fn split_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(':')
}

/// What an asset reference names (PACK-5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetRef<'a> {
    /// A file in the song's `assets/` (FMT-14).
    Song(&'a str),
    /// A file in a package's folder, by its path there.
    Package { package: PackName, path: &'a str },
}

impl<'a> AssetRef<'a> {
    /// A reference with a `:` is a package's file: its package must be a package's name, and its
    /// path relative and plain, as a package file's (FMT-6a): `/`-separated, with no empty, `.` or
    /// `..` part, none starting with `.`, and no `\` or control character. Whether the file is
    /// there is the resolver's to say.
    pub fn parse(s: &'a str) -> Result<AssetRef<'a>, String> {
        let Some((package, path)) = split_id(s) else { return Ok(AssetRef::Song(s)) };
        let package = PackName::new(package).map_err(|e| format!("`{s}` names a package's file, but {e}"))?;
        let plain = !path.is_empty()
            && path.split('/').all(|seg| !seg.is_empty() && !seg.starts_with('.'))
            && !path.chars().any(|c| c == '\\' || c.is_control());
        match plain {
            true => Ok(AssetRef::Package { package, path }),
            false => Err(format!(
                "`{s}` must name a file inside {package} by its path there, such as `{package}:samples/C4.wav`: no `..`, no `/` at \
                 the start, no empty or hidden folder"
            )),
        }
    }
}

/// `m2pkg.toml` (PACK-2), read strictly.
#[derive(Clone, Debug)]
pub struct PkgManifest {
    pub package: PkgInfo,
    /// `@m2/core` and `std` only.
    pub dependencies: BTreeMap<String, Requirement>,
    /// Its kernels, as written: the runtime reads them (PKG-2).
    pub kernel: Vec<toml::Value>,
}

#[derive(Clone, Debug)]
pub struct PkgInfo {
    pub name: PackName,
    pub version: Version,
    pub license: Option<String>,
    pub description: String,
    pub authors: Vec<String>,
    pub repository: Option<String>,
    pub tags: Vec<String>,
    /// The compiled kernels, relative to the package folder.
    pub wasm: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    package: RawInfo,
    #[serde(default)]
    dependencies: BTreeMap<String, Requirement>,
    #[serde(default)]
    kernel: Vec<toml::Value>,
    /// Kept only to refuse it: effects are declared in TypeScript now (PKG-2), and without the
    /// field an old manifest's `[[effect]]` would be refused as only an unknown key.
    #[serde(default)]
    effect: Vec<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInfo {
    name: PackName,
    version: String,
    license: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    authors: Vec<String>,
    repository: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    wasm: Option<String>,
}

impl PkgManifest {
    pub fn read(text: &str) -> Result<PkgManifest, String> {
        let m: RawManifest = toml::from_str(text).map_err(|e| e.message().to_string())?;
        let name = &m.package.name;
        if !m.effect.is_empty() {
            return Err(format!(
                "package `{name}`: `[[effect]]` is gone from m2pkg.toml (PKG-2). Effects are declared in TypeScript, \
                 `component({{ …, editor: {{ category: \"Filter\", insert: {{ … }} }} }}, …)`"
            ));
        }
        let version = Version::parse(&m.package.version)
            .ok_or_else(|| format!("package `{name}`: `version = \"{}\"` must be a version such as \"1.2.0\"", m.package.version))?;
        if let Some(other) = m.dependencies.keys().find(|d| !matches!(d.as_str(), SDK | STD)) {
            return Err(format!("package `{name}`: [dependencies] names `{other}`, but a pack may need only \"{SDK}\" and std"));
        }
        let RawInfo { name, license, description, authors, repository, tags, wasm, .. } = m.package;
        let package = PkgInfo { name, version, license, description, authors, repository, tags, wasm };
        Ok(PkgManifest { package, dependencies: m.dependencies, kernel: m.kernel })
    }

    /// Read `dir`'s `m2pkg.toml`.
    pub fn at(dir: &Path) -> Result<PkgManifest, String> {
        let path = dir.join(PKG_MANIFEST);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{} can't be read ({e})", path.display()))?;
        PkgManifest::read(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Whether this app's SDK and std meet what the package needs (PACK-2): the SDK by the caret
    /// rule, std as a lower bound, so a std that grew since still meets it.
    pub fn needs(&self, sdk: Version, std: Version) -> Result<(), String> {
        meets(&self.dependencies, Some(sdk), Some(std)).map_err(|why| format!("{} {} {why}", self.package.name, self.package.version))
    }
}

/// What this app gives a pack (PACK-2): its `@m2/core` and its std (`None`: none at all). Whether
/// it runs a pack is asked of it wherever that matters, the build, resolution, `m2 pack lock` and
/// `update`, and the lists, so they never disagree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct App {
    pub sdk: Option<Version>,
    pub std: Option<Version>,
}

impl App {
    /// Whether it meets a package's `[dependencies]`: [`meets`].
    pub fn meets(&self, dependencies: &BTreeMap<String, Requirement>) -> Result<(), String> {
        meets(dependencies, self.sdk, self.std)
    }

    /// Whether it runs the pack in `dir`: its `m2pkg.toml` reads, and it has what that needs. Why
    /// not, in words naming the pack: `@a/x 1.3.0 needs std "0.9" or later, but this app has 0.5.0`.
    pub fn runs(&self, dir: &Path) -> Result<(), String> {
        let m = PkgManifest::at(dir)?;
        self.meets(&m.dependencies).map_err(|why| format!("{} {} {why}", m.package.name, m.package.version))
    }
}

/// Whether the SDK and std this app has (`None`: none at all) meet a package's `[dependencies]`
/// (PACK-2): the SDK by the caret rule, std as a lower bound. What it needs that isn't met, in
/// words: `needs std "0.9" or later, but this app has 0.4.0`. The one rule; [`App`] asks it.
pub fn meets(dependencies: &BTreeMap<String, Requirement>, sdk: Option<Version>, std: Option<Version>) -> Result<(), String> {
    for (dep, req) in dependencies {
        let (have, met, at_least) = match dep.as_str() {
            SDK => (sdk, sdk.is_some_and(|v| req.matches(v)), ""),
            _ => (std, std.is_some_and(|v| req.at_least(v)), " or later"),
        };
        if !met {
            let have = have.map_or("none".to_string(), |v| v.to_string());
            return Err(format!("needs {dep} \"{req}\"{at_least}, but this app has {have}"));
        }
    }
    Ok(())
}

/// One file of an installed version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    pub path: String,
    pub size: u64,
    pub sha256: Sha,
}

/// `m2pack.json` (PACK-6): every file of a pack but itself, by sorted path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub files: Vec<Listed>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawListing {
    m2pack: u32,
    files: Vec<Listed>,
}

/// The listing's format.
const LISTING_FORMAT: u32 = 1;

impl Listing {
    /// Every file under `dir` but the listing itself, by its path there, `/`-separated, sorted
    /// bytewise. Dot files and folders (`.git`) are no part of a pack; a link is refused.
    pub fn of(dir: &Path) -> std::io::Result<Listing> {
        let mut files = vec![];
        for (rel, path) in pack_files(dir)? {
            if rel == LISTING {
                continue;
            }
            let size = std::fs::symlink_metadata(&path)?.len();
            files.push(Listed { path: rel, size, sha256: Sha::of_file(&path)? });
        }
        Ok(Listing { files })
    }

    pub fn read(text: &str) -> Result<Listing, String> {
        let raw: RawListing = serde_json::from_str(text).map_err(|e| format!("{LISTING} can't be read: {e}"))?;
        if raw.m2pack != LISTING_FORMAT {
            return Err(format!("{LISTING} is format {}, and this app reads {LISTING_FORMAT}", raw.m2pack));
        }
        Ok(Listing { files: raw.files })
    }

    /// The canonical form: one file a line.
    pub fn text(&self) -> String {
        let lines: Vec<String> =
            self.files.iter().map(|f| format!("  {}", serde_json::to_string(f).expect("a listing serialises"))).collect();
        format!("{{\"m2pack\": {LISTING_FORMAT}, \"files\": [\n{}\n]}}\n", lines.join(",\n"))
    }

    /// The version's hash.
    pub fn hash(&self) -> Sha {
        Sha::of(self.text().as_bytes())
    }
}

/// Every file under `dir`, by its path there, `/`-separated, sorted bytewise; dot files and
/// folders left out. A symbolic link, to a file or a folder, is an error naming it: a pack holds
/// none, and nothing of a pack is ever read through one (PACK-6), so the tools never leave it.
pub fn pack_files(dir: &Path) -> std::io::Result<Vec<(String, PathBuf)>> {
    let mut out = vec![];
    let mut stack = vec![(String::new(), dir.to_path_buf())];
    while let Some((rel, d)) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            let kind = std::fs::symlink_metadata(e.path())?.file_type();
            if kind.is_symlink() {
                return Err(std::io::Error::other(format!("{r} is a symbolic link: a pack holds none, so copy the file in instead")));
            }
            match kind.is_dir() {
                true => stack.push((r, e.path())),
                false => out.push((r, e.path())),
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The file at `path` inside a package's folder `dir`, each part spelled exactly as its folder
/// lists it, with no case folding and no part a symbolic link (PACK-5, FMT-6a): what every disk
/// finds the same, and never anything outside the package.
pub fn package_file(dir: &Path, path: &str) -> Result<PathBuf, String> {
    let mut at = dir.to_path_buf();
    for part in path.split('/') {
        let listed = std::fs::read_dir(&at).map_err(|_| "there is no such file".to_string())?;
        let names: Vec<std::ffi::OsString> = listed.flatten().map(|e| e.file_name()).collect();
        if !names.iter().any(|n| n == part) {
            return Err(match names.iter().find(|n| n.to_string_lossy().eq_ignore_ascii_case(part)) {
                Some(n) => format!("it must be spelled as the package names it, `{}`, with the same capitals", n.to_string_lossy()),
                None => "there is no such file".to_string(),
            });
        }
        at.push(part);
        if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("`{part}` is a symbolic link, and a package's file is never read through one"));
        }
    }
    match at.is_file() {
        true => Ok(at),
        false => Err("it is a folder, not a file".into()),
    }
}

/// The store (PACK-6): `<dir>/<name>/<version>/`, each version a pack with its listing.
#[derive(Clone, Debug)]
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn at(dir: PathBuf) -> Store {
        Store { dir }
    }

    /// The store of this computer, `m2_os::dirs::packs()`; none without a home folder.
    pub fn here() -> Option<Store> {
        m2_os::dirs::packs().map(Store::at)
    }

    pub fn version_dir(&self, name: &PackName, v: &Version) -> PathBuf {
        self.dir.join(name.folder()).join(v.to_string())
    }

    /// The installed versions of `name`, oldest first: each folder named as a version that holds
    /// its listing.
    pub fn versions(&self, name: &PackName) -> Vec<Version> {
        let mut out: Vec<Version> = std::fs::read_dir(self.dir.join(name.folder()))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| Version::parse(e.file_name().to_str()?).filter(|_| e.path().join(LISTING).is_file()))
            .collect();
        out.sort();
        out
    }

    /// The hash of an installed version: its listing's.
    pub fn hash(&self, name: &PackName, v: &Version) -> std::io::Result<Sha> {
        Sha::of_file(&self.version_dir(name, v).join(LISTING))
    }

    /// Check an installed version against its listing (PACK-6): every file in its folder is one the
    /// listing names, with that size and SHA-256, none is missing, and none is a link. Hashes come
    /// from the process's hash cache (FMT-16), so an unchanged file costs a stat. Its hash, or what
    /// differs, in words naming the file.
    pub fn check(&self, name: &PackName, v: &Version) -> Result<Sha, String> {
        let dir = self.version_dir(name, v);
        let text = std::fs::read(dir.join(LISTING)).map_err(|e| format!("its {LISTING} can't be read ({e})"))?;
        let listing = Listing::read(&String::from_utf8_lossy(&text))?;
        let found = pack_files(&dir).map_err(|e| e.to_string())?;
        let mut listed: BTreeMap<&str, &Listed> = listing.files.iter().map(|f| (f.path.as_str(), f)).collect();
        for (rel, path) in &found {
            if rel == LISTING {
                continue;
            }
            let Some(f) = listed.remove(rel.as_str()) else {
                return Err(format!("{rel} isn't one of its files"));
            };
            let size = std::fs::symlink_metadata(path).map(|m| m.len()).map_err(|e| format!("{rel} can't be read ({e})"))?;
            let sha = (size == f.size).then(|| HashCache::global().hash(path).ok()).flatten();
            if sha != Some(f.sha256) {
                return Err(format!("{rel} was changed"));
            }
        }
        if let Some(rel) = listed.keys().next() {
            return Err(format!("{rel} is missing"));
        }
        Ok(Sha::of(&text))
    }

    /// Every installed pack and its versions, oldest first, by name.
    pub fn list(&self) -> Vec<(PackName, Vec<Version>)> {
        let mut out = vec![];
        for owner in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let owner = owner.file_name().to_string_lossy().into_owned();
            for pack in std::fs::read_dir(self.dir.join(&owner)).into_iter().flatten().flatten() {
                let Ok(name) = PackName::new(&format!("{owner}/{}", pack.file_name().to_string_lossy())) else { continue };
                let versions = self.versions(&name);
                if !versions.is_empty() {
                    out.push((name, versions));
                }
            }
        }
        out.sort();
        out
    }
}

/// `music2.lock` (PACK-8): one entry per pack from the store, sorted by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lock {
    pub packs: Vec<Locked>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locked {
    pub name: PackName,
    pub version: Version,
    pub hash: Sha,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLock {
    #[serde(default)]
    pack: Vec<RawLocked>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocked {
    name: PackName,
    version: String,
    hash: Sha,
}

/// The lock's first lines.
const LOCK_HEAD: &str = "# The exact version of each pack this song uses, made by music2 (PACK-8). Don't edit it by hand.\n";

impl Lock {
    /// Read strictly; a problem is `manifest.invalid`, naming the lock.
    pub fn read(text: &str) -> Result<Lock, Diagnostic> {
        let invalid = |m: String| Diagnostic::error("manifest.invalid", format!("{LOCK} can't be read: {m}")).file(LOCK);
        let raw: RawLock = toml::from_str(text).map_err(|e| invalid(e.message().to_string()))?;
        let mut packs: Vec<Locked> = vec![];
        for p in raw.pack {
            let version =
                Version::parse(&p.version).ok_or_else(|| invalid(format!("`{}` locks `{}`, not a version", p.name, p.version)))?;
            if p.name.is_std() {
                return Err(invalid("std comes with the app and is never locked".into()));
            }
            if packs.iter().any(|l| l.name == p.name) {
                return Err(invalid(format!("`{}` is locked twice", p.name)));
            }
            packs.push(Locked { name: p.name, version, hash: p.hash });
        }
        packs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Lock { packs })
    }

    /// The canonical form (FMT-8a's rule for data files): the head, then each pack by name.
    pub fn canonical(&self) -> String {
        let mut out = LOCK_HEAD.to_string();
        for l in &self.packs {
            let q = |s: &str| serde_json::to_string(s).expect("a string serialises");
            out.push_str(&format!(
                "\n[[pack]]\nname = {}\nversion = \"{}\"\nhash = \"{}\"\n",
                q(l.name.as_str()),
                l.version,
                l.hash.prefixed()
            ));
        }
        out
    }

    pub fn get(&self, name: &PackName) -> Option<&Locked> {
        self.packs.iter().find(|l| l.name == *name)
    }

    /// This lock with `name` at `version`, in place of any entry it had.
    pub fn with(mut self, name: PackName, version: Version, hash: Sha) -> Lock {
        self.packs.retain(|l| l.name != name);
        self.packs.push(Locked { name, version, hash });
        self.packs.sort_by(|a, b| a.name.cmp(&b.name));
        self
    }
}

/// A pack a song builds with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub name: PackName,
    pub version: Version,
    pub dir: PathBuf,
    /// Its listing's hash; none for a pack in a folder.
    pub hash: Option<Sha>,
    /// A path dependency.
    pub path: bool,
}

/// What a song's packs resolve to (PACK-9).
#[derive(Clone, Debug, Default)]
pub struct Resolution {
    pub packs: Vec<Resolved>,
    pub problems: Vec<Diagnostic>,
}

/// The packs the song in `project` builds with, from its manifest, its lock and the store
/// (PACK-9). std and the SDK aren't here: the app has them, and FMT-3 checks them. With no store
/// (no home folder), a pack from the store is unsatisfied: nothing is looked for elsewhere. Of the
/// store's versions, only one `app` runs is chosen.
pub fn resolve(project: &Path, manifest: &Manifest, lock: &Lock, store: Option<&Store>, app: &App) -> Resolution {
    let mut out = Resolution::default();
    for (name, dep) in packs_of(manifest) {
        match resolve_one(project, &name, dep, lock, store, app) {
            Ok(r) => out.packs.push(r),
            Err(d) => out.problems.push(d),
        }
    }
    out
}

/// The manifest's packs: every `[dependencies]` entry but the SDK and std, by name.
fn packs_of(manifest: &Manifest) -> impl Iterator<Item = (PackName, &Dependency)> {
    // Manifest::read checked every name.
    manifest.dependencies.iter().filter(|(n, _)| !matches!(n.as_str(), SDK | STD)).filter_map(|(n, d)| Some((PackName::new(n).ok()?, d)))
}

/// What to say for a pack from the store when there is no store.
fn no_store(name: &PackName, req: &Requirement) -> Diagnostic {
    Diagnostic::error(
        "pkg.unsatisfied",
        format!("{MANIFEST} asks for {name} \"{req}\", but there is no store of packs here: music2 finds your home folder ($HOME) unset"),
    )
    .file(MANIFEST)
}

/// What says how to put back version `v` of `name` whose installed copy was changed (`why`).
fn changed(name: &PackName, v: &Version, why: &str) -> Diagnostic {
    Diagnostic::error(
        "pkg.changed",
        format!(
            "The installed {name} {v} was changed after it was installed: {why}. Install it again from its folder with `m2 pack \
             install <folder>`, which puts it back as it was"
        ),
    )
    .file(LOCK)
}

/// The store's version `v` of `name`, checked whole, as a song resolves it.
fn whole(store: &Store, name: &PackName, v: Version) -> Result<Resolved, Diagnostic> {
    let hash = store.check(name, &v).map_err(|why| changed(name, &v, &why))?;
    Ok(Resolved { name: name.clone(), version: v, dir: store.version_dir(name, &v), hash: Some(hash), path: false })
}

/// The highest installed version of `name` that `req` admits and `app` runs (PACK-2): another
/// app sharing the store may have installed a newer one this one can't run. One whose copy no
/// longer reads is taken all the same, so checking it whole says what changed. Its version, or
/// why this app runs none it admits.
fn runnable(store: &Store, name: &PackName, req: &Requirement, app: &App) -> Result<Option<Version>, String> {
    let mut cant = None;
    for v in store.versions(name).into_iter().rev().filter(|v| req.matches(*v)) {
        match app.runs(&store.version_dir(name, &v)) {
            Ok(()) => return Ok(Some(v)),
            Err(_) if store.check(name, &v).is_err() => return Ok(Some(v)),
            Err(why) => {
                cant.get_or_insert(why);
            }
        }
    }
    cant.map_or(Ok(None), Err)
}

/// The highest installed version of `name` that `req` admits and `app` runs, checked whole.
fn highest(store: &Store, name: &PackName, req: &Requirement, app: &App) -> Result<Resolved, Diagnostic> {
    let unsatisfied = |words: String| Diagnostic::error("pkg.unsatisfied", words).file(MANIFEST);
    let v = runnable(store, name, req, app).map_err(|why| {
        unsatisfied(format!(
            "{MANIFEST} asks for {name} \"{req}\", but the versions installed that it admits need a newer music2: {why}. Update \
             music2, or install a version made for this one with `m2 pack install <folder>`"
        ))
    })?;
    let installed = store.versions(name);
    let Some(v) = v else {
        let have = match installed.is_empty() {
            true => "none is installed".to_string(),
            false => format!("installed: {}", installed.iter().map(Version::to_string).collect::<Vec<_>>().join(", ")),
        };
        return Err(unsatisfied(format!(
            "{MANIFEST} asks for {name} \"{req}\", but {have}. Install it from its folder with `m2 pack install <folder>`"
        )));
    };
    whole(store, name, v)
}

fn resolve_one(
    project: &Path,
    name: &PackName,
    dep: &Dependency,
    lock: &Lock,
    store: Option<&Store>,
    app: &App,
) -> Result<Resolved, Diagnostic> {
    let req = match dep {
        Dependency::Path(p) => return in_folder(project, name, &p.path),
        Dependency::Version(req) => req,
    };
    let store = store.ok_or_else(|| no_store(name, req))?;
    let Some(l) = lock.get(name) else { return highest(store, name, req, app) };
    let installed = store.versions(name);
    if !req.matches(l.version) {
        return Err(Diagnostic::error(
            "pkg.unsatisfied",
            format!(
                "{LOCK} keeps {name} at {}, which {MANIFEST}'s \"{req}\" doesn't admit. Run `m2 pack lock <song>` to lock the \
                 highest installed version it admits",
                l.version
            ),
        )
        .file(LOCK));
    }
    if !installed.contains(&l.version) {
        let other = runnable(store, name, req, app).ok().flatten();
        let instead = match other {
            Some(v) => format!(", or run `m2 pack lock <song>` to use {v}, which is installed (the song may sound different)"),
            None => String::new(),
        };
        return Err(Diagnostic::error(
            "pkg.missing",
            format!(
                "This song uses {name} {}, which isn't installed. Install it from its folder with `m2 pack install <folder>`{instead}",
                l.version
            ),
        )
        .file(LOCK));
    }
    let r = whole(store, name, l.version)?;
    if r.hash != Some(l.hash) {
        return Err(not_locked(name, &l.version));
    }
    Ok(r)
}

/// What to say when the installed `v` of `name` isn't the copy the lock names.
fn not_locked(name: &PackName, v: &Version) -> Diagnostic {
    Diagnostic::error(
        "pkg.changed",
        format!(
            "The installed {name} {v} isn't the one this song was made with: its files differ from those {LOCK} names. To keep the \
             song's sound, put the one it was made with back: `m2 pack remove {name} {v}`, then `m2 pack install <its folder>`. To \
             use the installed one, run `m2 pack update <song> {name}` (the song may sound different)"
        ),
    )
    .file(LOCK)
}

/// A path dependency (PACK-7): the folder, which must hold that pack.
fn in_folder(project: &Path, name: &PackName, path: &str) -> Result<Resolved, Diagnostic> {
    let dir = project.join(path);
    let missing =
        |why: String| Diagnostic::error("pkg.missing", format!("{MANIFEST} takes {name} from the folder {path}, but {why}")).file(MANIFEST);
    let m = PkgManifest::at(&dir).map_err(|e| missing(format!("that has no pack ({e})")))?;
    if m.package.name != *name {
        return Err(missing(format!("that holds {}", m.package.name)));
    }
    let dir = dir.canonicalize().unwrap_or(dir);
    Ok(Resolved { name: name.clone(), version: m.package.version, dir, hash: None, path: true })
}

/// Which locked packs [`relock`] moves.
#[derive(Clone, Copy, Debug)]
pub enum Move<'a> {
    /// Only what must (`m2 pack lock`): an entry the requirement doesn't admit, or whose version
    /// isn't installed, and a pack with none.
    Needed,
    /// Every pack, to the highest installed version its requirement admits (`m2 pack update`).
    All,
    /// That pack (`m2 pack update <song> <name>`), and what must.
    Only(&'a PackName),
}

/// The lock `m2 pack lock` and `m2 pack update` write (PACK-8), from the manifest, the lock it
/// replaces and the store alone, never from a build: each pack the manifest takes from the store,
/// kept at its locked version when that is admitted, installed and the copy locked, else at the
/// highest installed version its requirement admits that `app` runs. An entry for a pack the manifest no longer
/// lists goes. What keeps one from it, as a build says it.
pub fn relock(manifest: &Manifest, lock: &Lock, store: Option<&Store>, app: &App, moving: Move) -> Result<Lock, Vec<Diagnostic>> {
    let mut out = Lock::default();
    let mut problems = vec![];
    for (name, dep) in packs_of(manifest) {
        let Dependency::Version(req) = dep else { continue };
        let Some(store) = store else {
            problems.push(no_store(&name, req));
            continue;
        };
        let moves = match moving {
            Move::Needed => false,
            Move::All => true,
            Move::Only(n) => *n == name,
        };
        let kept = lock.get(&name).filter(|l| !moves && req.matches(l.version) && store.versions(&name).contains(&l.version));
        let resolved = match kept {
            Some(l) => whole(store, &name, l.version).and_then(|r| match r.hash == Some(l.hash) {
                true => Ok(r),
                false => Err(not_locked(&name, &l.version)),
            }),
            None => highest(store, &name, req, app),
        };
        match resolved {
            Ok(r) => out = out.with(r.name, r.version, r.hash.expect("a store version has a hash")),
            Err(d) => problems.push(d),
        }
    }
    match problems.is_empty() {
        true => Ok(out),
        false => Err(problems),
    }
}

#[cfg(test)]
mod tests;
