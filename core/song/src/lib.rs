//! A music2 project folder, format 1.
//!
//! ```text
//! music2.toml                  [project] name, entry, format = 1, sample_rate = 48000
//!                              [dependencies] "@m2/core" = "1", std = "0.7"
//! song.ts  tracks/*.ts  lib/   code
//! data/arrangement.json        the tempo and the meter
//! data/<module>/<lane>.json    one lane's clips
//! data/<module>/<frames>/<plugin-key>.state   a CLAP state blob, with its .state.json sidecar
//! assets/<path>                audio files, never overwritten
//! layout.json                  shared presentation; never part of a revision
//! .music2/                     local to this machine
//! ```
//!
//! Every path is spelled and classified by [`DataPath`] (FMT-5). Data files are plain JSON
//! with one canonical form, written here (FMT-8a), so they diff well and two writers agree
//! byte for byte. There is no reader for any earlier layout (FMT-1, FMT-21).

pub mod arrangement;
pub mod data;
pub mod diag;
pub mod hash;
pub mod instrument;
pub mod jcs;
pub mod key;
pub mod lane;
pub mod manifest;
pub mod pack;
pub mod path;
pub mod registry;
pub mod tx;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use arrangement::{Arrangement, Meter, Tempo, TempoMap};
pub use data::{AssetDecl, DataSet, Decls, Sidecar, StateDecl, Unused};
pub use diag::{Diagnostic, Fix, Severity};
pub use hash::{HashCache, Hashing, Sha};
pub use key::{ClipId, Key, LaneAddr, ROOT};
pub use lane::{
    AbsNote, AudioClip, ControlClip, ControlLane, Holds, LaneData, LaneError, LaneKind, Note, NoteClip, StepsOf, canonical, parse_pitch,
};
pub use manifest::{Dependency, FORMAT, Manifest, Requirement, SDK, Version};
pub use pack::{Lock, PackName};
pub use path::{AssetName, Class, DataPath, dir_hash};
pub use tx::{Anchor, Base, Confined, Declared, DeltaHash, Expect, ModuleChange, Op as TxOp, Payload, Pos, Range, Spot, Tx, TxPath};

/// The files one revision of a project is made of, read once (FMT-6): the text of the
/// manifest, code, arrangement, lane files and state sidecars, and of the assets and state
/// blobs only what they hash to. A revision is built from one snapshot, so its code, graph and
/// data always belong together.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub dir: PathBuf,
    /// Contents by path relative to `dir`. Shared: a snapshot made from another shares every
    /// text it didn't change, so a clone or a compare costs the files that changed.
    pub files: BTreeMap<PathBuf, Arc<str>>,
    /// Assets and state blobs by path relative to `dir`: never their bytes (FMT-6). State blobs
    /// and the assets a lane names are hashed; other assets are listed by name. Shared until
    /// changed (`Arc::make_mut`), so a project with thousands of assets copies none per edit.
    pub binaries: Arc<BTreeMap<PathBuf, Stored>>,
    /// Files under `data/` that [`DataPath`] can't parse (FMT-7), and files in `assets/` whose
    /// name is an asset's in another Unicode form (FMT-15). They are Ignored files, so they never
    /// make two snapshots differ (PLANE-3).
    pub unknown: Arc<BTreeSet<PathBuf>>,
    /// Files of the explicit set, and folders, that were listed but couldn't be read, with why:
    /// a lane file at mode 000, a dangling link, a folder that can't be listed. Only the
    /// manifest's, code's and the arrangement's block; a lane's or a sidecar's never stops
    /// anything else (FMT-9).
    pub unreadable: BTreeMap<PathBuf, String>,
    /// The hash of the bytes of each text file that isn't UTF-8: its entry in `files` is read
    /// with the bad bytes replaced, for the build and the code editor, so it isn't its bytes.
    pub not_utf8: BTreeMap<PathBuf, Sha>,
}

/// What a snapshot knows of a binary file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stored {
    Hash(Sha),
    /// An asset no lane names: its name is all that matters.
    Listed,
    Unreadable(String),
}

/// Equal when the song is: the read files, the binaries and what couldn't be read. Ignored
/// files don't count.
impl PartialEq for Snapshot {
    fn eq(&self, other: &Snapshot) -> bool {
        (&self.dir, &self.files, &self.binaries, &self.unreadable, &self.not_utf8)
            == (&other.dir, &other.files, &other.binaries, &other.unreadable, &other.not_utf8)
    }
}

impl Eq for Snapshot {}

impl Snapshot {
    /// Read the project at `dir`: the files [`DataPath`] classes as Manifest, Code,
    /// Arrangement, Lane and state sidecar, and nothing else (FMT-6). `.music2/`, dot folders and
    /// `node_modules/` are never entered. Fails only when the project folder itself can't be
    /// read; a file or a folder inside it that can't be is recorded in `unreadable`.
    pub fn read(dir: &Path) -> anyhow::Result<Snapshot> {
        use anyhow::Context;
        let dir = dir.canonicalize().with_context(|| format!("opening project {}", dir.display()))?;
        let mut snap = Snapshot { dir: dir.clone(), ..Snapshot::default() };
        let (mut binaries, mut unknown) = (BTreeMap::new(), BTreeSet::new());
        let mut stack = vec![dir.clone()];
        while let Some(d) = stack.pop() {
            let rel = |p: &Path| p.strip_prefix(&dir).expect("inside the project").to_path_buf();
            let entries = match std::fs::read_dir(&d) {
                Ok(entries) => entries,
                Err(e) if d == dir => return Err(e).with_context(|| format!("reading {}", d.display())),
                Err(e) => {
                    snap.unreadable.insert(rel(&d), e.to_string());
                    continue;
                }
            };
            for e in entries.flatten() {
                let p = e.path();
                let rel = rel(&p);
                let name = e.file_name();
                let name = name.to_string_lossy();
                // Symbolic links to folders aren't followed: a link to `..` would never end.
                let kind = e.file_type();
                if kind.as_ref().is_ok_and(|t| t.is_dir()) {
                    if !name.starts_with('.') && name != "node_modules" {
                        stack.push(p);
                    }
                    continue;
                }
                let link = kind.as_ref().is_ok_and(|t| t.is_symlink());
                snap.read_entry(&p, rel, link, &mut binaries, &mut unknown);
            }
        }
        (snap.binaries, snap.unknown) = (Arc::new(binaries), Arc::new(unknown));
        snap.hash_assets();
        HashCache::global().save();
        Ok(snap)
    }

    /// Read the file at `p` (project path `rel`; `link`: a symbolic link) into this snapshot, by what
    /// [`DataPath`] classes it as, as [`Snapshot::read`] reads each file it lists.
    fn read_entry(
        &mut self,
        p: &Path,
        rel: PathBuf,
        link: bool,
        binaries: &mut BTreeMap<PathBuf, Stored>,
        unknown: &mut BTreeSet<PathBuf>,
    ) {
        let d = DataPath::parse(&rel);
        match d {
            DataPath::Manifest
            | DataPath::Lock
            | DataPath::Code(_)
            | DataPath::Arrangement
            | DataPath::Lane { .. }
            | DataPath::PluginState { sidecar: true, .. } => match std::fs::read(p) {
                Ok(bytes) => {
                    let text = String::from_utf8(bytes).unwrap_or_else(|e| {
                        self.not_utf8.insert(rel.clone(), Sha::of(e.as_bytes()));
                        String::from_utf8_lossy(e.as_bytes()).into_owned()
                    });
                    self.files.insert(rel, text.into());
                }
                // Removed between the listing and the read: as if listed a moment later. A
                // dangling link is listed, and stays unreadable.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !link => {}
                Err(e) => {
                    self.unreadable.insert(rel, e.to_string());
                }
            },
            DataPath::PluginState { sidecar: false, .. } => {
                binaries.insert(rel, stored(p));
            }
            DataPath::Asset(_) => {
                binaries.insert(rel, Stored::Listed);
            }
            d if path::listed_unknown(&d, &rel) => {
                unknown.insert(rel);
            }
            _ => {}
        }
    }

    /// What [`Snapshot::read`] reads of `paths` alone (files of the project, by their paths in it),
    /// with `unknown` as the Ignored files a walk of the project listed: a batch the watcher settled
    /// is read without reading or listing the rest of the project (WRITE-19). A path with no file
    /// there is absent. Of the assets, those `hashed` names are hashed and the rest listed: which
    /// of them a lane names is settled where the paths are taken in.
    pub fn read_paths(
        dir: &Path,
        paths: &BTreeSet<PathBuf>,
        hashed: &dyn Fn(&Path) -> bool,
        unknown: BTreeSet<PathBuf>,
    ) -> anyhow::Result<Snapshot> {
        use anyhow::Context;
        let dir = dir.canonicalize().with_context(|| format!("opening project {}", dir.display()))?;
        let mut snap = Snapshot { dir: dir.clone(), ..Snapshot::default() };
        let (mut binaries, mut ignored) = (BTreeMap::new(), BTreeSet::new());
        let mut folders = BTreeMap::new();
        for rel in paths {
            // Only a name the disk lists, as a walk would: one a disk that ignores capitals or
            // Unicode forms still opens (a rename's old name) is not there.
            if !listed(&dir, rel, &mut folders) {
                continue;
            }
            let p = dir.join(rel);
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            if !meta.is_dir() {
                snap.read_entry(&p, rel.clone(), meta.file_type().is_symlink(), &mut binaries, &mut ignored);
            }
        }
        for (p, s) in binaries.iter_mut() {
            if *s == Stored::Listed && hashed(p) {
                *s = stored(&dir.join(p));
            }
        }
        (snap.binaries, snap.unknown) = (Arc::new(binaries), Arc::new(unknown));
        HashCache::global().save();
        Ok(snap)
    }

    /// Hash every listed asset a lane file names, so the revision records what it plays.
    pub fn hash_assets(&mut self) {
        let mut named = BTreeSet::new();
        for (p, text) in &self.files {
            if !matches!(DataPath::parse(p), DataPath::Lane { .. }) {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { continue };
            for c in v.get("clips").and_then(|c| c.as_array()).into_iter().flatten() {
                if let Some(a) = c.get("asset").and_then(|a| a.as_str()).and_then(|a| AssetName::new(a).ok()) {
                    named.insert(DataPath::Asset(a).path());
                }
            }
        }
        for p in named {
            if matches!(self.binaries.get(&p), Some(Stored::Listed)) {
                let s = stored(&self.dir.join(&p));
                Arc::make_mut(&mut self.binaries).insert(p, s);
            }
        }
    }

    /// The SHA-256 of the bytes at `path` as this snapshot read them: a text file's (its raw
    /// bytes, even when they aren't UTF-8), or a hashed binary's. What a Tx's base names.
    pub fn sha(&self, path: &Path) -> Option<Sha> {
        if let Some(sha) = self.not_utf8.get(path) {
            return Some(*sha);
        }
        match (self.files.get(path), self.binaries.get(path)) {
            (Some(text), _) => Some(Sha::of(text.as_bytes())),
            (None, Some(Stored::Hash(sha))) => Some(*sha),
            _ => None,
        }
    }

    /// The TypeScript files, by absolute path.
    pub fn sources(&self) -> impl Iterator<Item = (PathBuf, &str)> {
        self.files.iter().filter(|(p, _)| matches!(DataPath::parse(p), DataPath::Code(_))).map(|(p, t)| (self.dir.join(p), &**t))
    }

    /// Every file at `p` or inside it, with what it holds, in path order.
    pub fn under(&self, p: &Path) -> Vec<(PathBuf, Content)> {
        let text = under(&self.files, p).map(|(k, t)| (k.clone(), Content::Text(t.clone())));
        let bin = under(&*self.binaries, p).map(|(k, b)| (k.clone(), Content::Binary(b.clone())));
        let mut all: Vec<(PathBuf, Content)> = text.chain(bin).collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    /// A file or folder that has the name `p` on a disk that ignores capitals: `p` itself, `p`
    /// in other capitals, a file inside such a folder, or a folder on the way to `p` spelled in
    /// other capitals (`data/Bass` for `data/bass/notes.json`), which such a disk would write
    /// into. Every path the snapshot lists counts, files music2 doesn't read and ones it can't
    /// included. Judged by the names alone, so a project acts the same on every disk.
    pub fn taken(&self, p: &Path) -> Option<PathBuf> {
        let want: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let paths = self.files.keys().chain(self.binaries.keys()).chain(self.unknown.iter()).chain(self.unreadable.keys());
        paths.into_iter().find_map(|k| {
            let mut at = PathBuf::new();
            for (i, c) in k.components().enumerate() {
                let c = c.as_os_str().to_string_lossy();
                match want.get(i) {
                    // `k` is `p`, or inside it, in any capitals.
                    None => return Some(k.clone()),
                    Some(w) if *w == c => {}
                    Some(w) if w.eq_ignore_ascii_case(&c) => return Some(at.join(c.as_ref())),
                    Some(_) => return None,
                }
                at.push(c.as_ref());
            }
            // `k` is `p` exactly, or a folder on the way to it in the same capitals.
            (k.components().count() == want.len()).then(|| k.clone())
        })
    }

    /// Why `p`, something in it, or a folder it is in can't be read, if that is so (FMT-9): what
    /// can't be read can't be checked, so it is never moved or removed.
    pub fn unread(&self, p: &Path) -> Option<String> {
        let (at, why) = self.unreadable.iter().find(|(u, _)| u.starts_with(p) || p.starts_with(u))?;
        Some(format!("{} can't be read ({why}); fix that first, or move it by hand", at.display()))
    }
}

/// What a snapshot knows of a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Text(Arc<str>),
    Binary(Stored),
}

/// The entries of `map` at `p` or inside it, in path order. Paths order by component, so they are
/// one run, from `p` to [`past`] it: found without visiting the rest (thousands of assets beside a
/// lane), nor comparing each one's components.
pub fn under<'a, V>(map: &'a BTreeMap<PathBuf, V>, p: &Path) -> std::collections::btree_map::Range<'a, PathBuf, V> {
    match past(p) {
        Some(end) => map.range::<Path, _>((Bound::Included(p), Bound::Excluded(end.as_path()))),
        None => map.range::<Path, _>(..),
    }
}

/// The paths of `set` at `p` or inside it, as [`under`] finds them.
pub fn under_set<'a>(set: &'a BTreeSet<PathBuf>, p: &Path) -> std::collections::btree_set::Range<'a, PathBuf> {
    match past(p) {
        Some(end) => set.range::<Path, _>((Bound::Included(p), Bound::Excluded(end.as_path()))),
        None => set.range::<Path, _>(..),
    }
}

/// The first path after `p` and every path inside it: its last name with a NUL after it, which no
/// name on a disk has, so nothing sorts between. None for the empty path, which holds them all.
fn past(p: &Path) -> Option<PathBuf> {
    let mut name = p.file_name()?.to_os_string();
    name.push("\0");
    Some(p.with_file_name(name))
}

/// Whether each name of project path `rel` is one its folder lists, exactly, under `dir`, through
/// folders that aren't links: what a walk from the project reaches. `folders` holds the folders
/// listed so far, by their paths.
fn listed(dir: &Path, rel: &Path, folders: &mut BTreeMap<PathBuf, Option<BTreeMap<std::ffi::OsString, bool>>>) -> bool {
    let mut at = PathBuf::new();
    let parts: Vec<&std::ffi::OsStr> = rel.iter().collect();
    for (i, name) in parts.iter().enumerate() {
        // Each name with whether it is a folder (its entry's type, never followed).
        let names = folders.entry(at.clone()).or_insert_with(|| {
            std::fs::read_dir(dir.join(&at))
                .ok()
                .map(|r| r.flatten().map(|e| (e.file_name(), e.file_type().is_ok_and(|t| t.is_dir()))).collect())
        });
        match names.as_ref().and_then(|n| n.get(*name)) {
            Some(&folder) if folder || i + 1 == parts.len() => at.push(name),
            _ => return false,
        }
    }
    true
}

fn stored(path: &Path) -> Stored {
    match HashCache::global().hash(path) {
        Ok(sha) => Stored::Hash(sha),
        Err(e) => Stored::Unreadable(e.to_string()),
    }
}

/// Whether a path that couldn't be read stops the project from being accepted (FMT-9): the
/// manifest, code, the arrangement, and a folder that may hold code or the arrangement. Data and
/// assets never do.
fn blocks(p: &Path) -> bool {
    match DataPath::parse(p) {
        DataPath::Manifest | DataPath::Lock | DataPath::Code(_) | DataPath::Arrangement => true,
        DataPath::Lane { .. } | DataPath::Module { .. } | DataPath::PluginState { .. } | DataPath::Asset(_) => false,
        _ => !(p.starts_with(path::DATA) || p.starts_with(path::ASSETS)) || p == Path::new(path::DATA),
    }
}

/// The blocking error for `p`, which couldn't be read (`why`).
fn unreadable_blocks(p: &Path, why: &str) -> Diagnostic {
    let file = path::slashed(p);
    match DataPath::parse(p) {
        DataPath::Manifest | DataPath::Lock => Diagnostic::error("manifest.invalid", format!("{file} can't be read ({why})")),
        DataPath::Arrangement => Diagnostic::error("arrangement.invalid", format!("{file} can't be read ({why})")),
        _ if p == Path::new(path::DATA) => {
            Diagnostic::error("arrangement.invalid", format!("{file} can't be read ({why}), so neither can {}", DataPath::Arrangement))
        }
        DataPath::Code(_) => Diagnostic::error("code.build", format!("{file} can't be read ({why}), so the song can't be built")),
        _ => Diagnostic::error("code.build", format!("the folder {file} can't be read ({why}), so code in it can't be built")),
    }
    .file(file)
}

/// A project as one snapshot says it is: the manifest and arrangement read and checked, and
/// the data files found, by address. Lanes are read against their declarations by
/// [`Project::data`].
#[derive(Clone, Debug)]
pub struct Project {
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// `music2.lock`, or an empty lock when there is none (PACK-8).
    pub lock: Lock,
    pub arrangement: Arrangement,
    /// Every Code file, by its path in the project, `/`-separated: all the build may import
    /// from the project (FMT-6a).
    pub code: BTreeMap<String, Arc<str>>,
    /// Lane files by address: their path and text, shared with the snapshot.
    lanes: BTreeMap<LaneAddr, (PathBuf, Arc<str>)>,
    /// State sidecars' text, by path.
    sidecars: BTreeMap<PathBuf, Arc<str>>,
    binaries: Arc<BTreeMap<PathBuf, Stored>>,
    unknown: Arc<BTreeSet<PathBuf>>,
    /// Data files and folders that couldn't be read, with why (FMT-9).
    unreadable: BTreeMap<PathBuf, String>,
}

impl Project {
    pub fn open(dir: &Path) -> Result<Project, Vec<Diagnostic>> {
        let snap = Snapshot::read(dir).map_err(|e| vec![Diagnostic::error("manifest.invalid", format!("{e:#}"))])?;
        Project::from_snapshot(&snap)
    }

    /// Read a project from a snapshot, without reading the disk. The manifest's `format` is read
    /// first (FMT-3); an error whose code starts with `format.` means the project doesn't open.
    /// Manifest and arrangement errors block acceptance (FMT-9). Lane files never do.
    pub fn from_snapshot(snap: &Snapshot) -> Result<Project, Vec<Diagnostic>> {
        let Some(text) = snap.files.get(Path::new(path::MANIFEST)) else {
            let why = match snap.unreadable.get(Path::new(path::MANIFEST)) {
                Some(e) => format!("{} can't be read ({e})", path::MANIFEST),
                None => format!("{} has no {}, so it isn't a music2 project", snap.dir.display(), path::MANIFEST),
            };
            return Err(vec![Diagnostic::error("manifest.invalid", why)]);
        };
        let manifest = Manifest::read(text).map_err(|d| vec![d])?;
        let lock = match snap.files.get(Path::new(path::LOCK)) {
            Some(text) => Lock::read(text).map_err(|d| vec![d])?,
            None => Lock::default(),
        };
        // Code and the arrangement that can't be read block, as their errors do; data never does.
        let (blocking, unreadable): (BTreeMap<PathBuf, String>, BTreeMap<PathBuf, String>) =
            snap.unreadable.clone().into_iter().partition(|(p, _)| blocks(p));
        if !blocking.is_empty() {
            return Err(blocking.iter().map(|(p, e)| unreadable_blocks(p, e)).collect());
        }
        // A missing entry is the build's to report (`code.build`).
        let arrangement = match snap.files.get(&DataPath::Arrangement.path()) {
            Some(text) => Arrangement::read(text).map_err(|d| vec![d])?,
            None => Arrangement::default(),
        };
        let mut lanes = BTreeMap::new();
        let mut sidecars = BTreeMap::new();
        let mut code = BTreeMap::new();
        for (p, text) in &snap.files {
            match DataPath::parse(p) {
                DataPath::Code(c) => {
                    code.insert(path::slashed(&c), text.clone());
                }
                DataPath::Lane { module, lane } => {
                    lanes.insert(LaneAddr { module, lane }, (p.clone(), text.clone()));
                }
                DataPath::PluginState { sidecar: true, .. } => {
                    sidecars.insert(p.clone(), text.clone());
                }
                _ => {}
            }
        }
        let (binaries, unknown) = (snap.binaries.clone(), snap.unknown.clone());
        Ok(Project { dir: snap.dir.clone(), manifest, lock, arrangement, code, lanes, sidecars, binaries, unknown, unreadable })
    }

    /// The project a session opens with while its files don't build (WRITE-20): the manifest's
    /// name and rate read leniently, else the folder's name and 48 kHz; no code and no data.
    pub fn empty(snap: &Snapshot) -> Project {
        let table = snap.files.get(Path::new(path::MANIFEST)).and_then(|t| toml::from_str::<toml::Table>(t).ok());
        let project = table.as_ref().and_then(|t| t.get("project"));
        let folder = || snap.dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let name = project.and_then(|p| p.get("name")).and_then(|n| n.as_str()).map_or_else(folder, str::to_string);
        let rate = project.and_then(|p| p.get("sample_rate")).and_then(|r| r.as_integer()).and_then(|r| u32::try_from(r).ok());
        let sample_rate = rate.filter(|r| (8_000..=384_000).contains(r)).unwrap_or(48_000);
        let info = manifest::ProjectInfo { name, entry: "song.ts".into(), format: FORMAT, sample_rate };
        Project {
            dir: snap.dir.clone(),
            manifest: Manifest { project: info, dependencies: BTreeMap::new() },
            lock: Lock::default(),
            arrangement: Arrangement::default(),
            code: BTreeMap::new(),
            lanes: BTreeMap::new(),
            sidecars: BTreeMap::new(),
            binaries: Arc::default(),
            unknown: Arc::default(),
            unreadable: BTreeMap::new(),
        }
    }

    pub fn entry(&self) -> PathBuf {
        self.dir.join(&self.manifest.project.entry)
    }

    /// The rate the engine runs and exports at (RENDER-31).
    pub fn sample_rate(&self) -> u32 {
        self.manifest.project.sample_rate
    }
}
