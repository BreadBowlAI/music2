//! A music2 project folder, format 1.
//!
//! ```text
//! music2.toml                  [project] name, entry, format = 1, sample_rate = 48000
//!                              [dependencies] "@m2/core" = "1", std = "0.1"
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
pub mod key;
pub mod lane;
pub mod manifest;
pub mod path;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub use arrangement::{Arrangement, Meter, Tempo, TempoMap};
pub use data::{DataSet, Decls, Sidecar, StateDecl, Unused};
pub use diag::{Diagnostic, Edit, Fix, Severity};
pub use hash::{HashCache, Sha};
pub use key::{ClipId, Key, LaneAddr, ROOT};
pub use lane::{
    AbsNote, AudioClip, ControlClip, ControlLane, Holds, LaneData, LaneError, LaneKind, Note, NoteClip, canonical, parse_pitch,
};
pub use manifest::{FORMAT, Manifest, Requirement, SDK, Version};
pub use path::{AssetName, Class, DataPath};

/// The files one revision of a project is made of, read once (FMT-6): the text of the
/// manifest, code, arrangement, lane files and state sidecars, and of the assets and state
/// blobs only what they hash to. A revision is built from one snapshot, so its code, graph and
/// data always belong together.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub dir: PathBuf,
    /// Contents by path relative to `dir`.
    pub files: BTreeMap<PathBuf, String>,
    /// Assets and state blobs by path relative to `dir`: never their bytes (FMT-6). State blobs
    /// and the assets a lane names are hashed; other assets are listed by name.
    pub binaries: BTreeMap<PathBuf, Stored>,
    /// Files under `data/` that [`DataPath`] can't parse (FMT-7), and files in `assets/` whose
    /// name is an asset's in another Unicode form (FMT-15). They are Ignored files, so they never
    /// make two snapshots differ (PLANE-3).
    pub unknown: BTreeSet<PathBuf>,
    /// Files of the explicit set, and folders, that were listed but couldn't be read, with why:
    /// a lane file at mode 000, a dangling link, a folder that can't be listed. Only the
    /// manifest's, code's and the arrangement's block; a lane's or a sidecar's never stops
    /// anything else (FMT-9).
    pub unreadable: BTreeMap<PathBuf, String>,
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
        (&self.dir, &self.files, &self.binaries, &self.unreadable) == (&other.dir, &other.files, &other.binaries, &other.unreadable)
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
                match DataPath::parse(&rel) {
                    DataPath::Manifest
                    | DataPath::Code(_)
                    | DataPath::Arrangement
                    | DataPath::Lane { .. }
                    | DataPath::PluginState { sidecar: true, .. } => match std::fs::read(&p) {
                        Ok(bytes) => {
                            let text = String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                            snap.files.insert(rel, text);
                        }
                        // Removed between the listing and the read: as if listed a moment later.
                        // A dangling link is listed, and stays unreadable.
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !kind.as_ref().is_ok_and(|t| t.is_symlink()) => {}
                        Err(e) => {
                            snap.unreadable.insert(rel, e.to_string());
                        }
                    },
                    DataPath::PluginState { sidecar: false, .. } => {
                        snap.binaries.insert(rel, stored(&p));
                    }
                    DataPath::Asset(_) => {
                        snap.binaries.insert(rel, Stored::Listed);
                    }
                    // Dot files (`.DS_Store`) are the system's, not a typo.
                    d if d.unknown_data() && !name.starts_with('.') => {
                        snap.unknown.insert(rel);
                    }
                    // An asset's name in another Unicode form, so a reference to it can say so.
                    DataPath::Unknown(_) if path::denormal_asset(&rel).is_some() => {
                        snap.unknown.insert(rel);
                    }
                    _ => {}
                }
            }
        }
        snap.hash_assets();
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
            if let Some(s @ Stored::Listed) = self.binaries.get_mut(&p) {
                *s = stored(&self.dir.join(&p));
            }
        }
    }

    /// The TypeScript files, by absolute path.
    pub fn sources(&self) -> impl Iterator<Item = (PathBuf, &String)> {
        self.files.iter().filter(|(p, _)| matches!(DataPath::parse(p), DataPath::Code(_))).map(|(p, t)| (self.dir.join(p), t))
    }
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
        DataPath::Manifest | DataPath::Code(_) | DataPath::Arrangement => true,
        DataPath::Lane { .. } | DataPath::Module { .. } | DataPath::PluginState { .. } | DataPath::Asset(_) => false,
        _ => !(p.starts_with(path::DATA) || p.starts_with(path::ASSETS)) || p == Path::new(path::DATA),
    }
}

/// The blocking error for `p`, which couldn't be read (`why`).
fn unreadable_blocks(p: &Path, why: &str) -> Diagnostic {
    let file = path::slashed(p);
    match DataPath::parse(p) {
        DataPath::Manifest => Diagnostic::error("manifest.invalid", format!("{file} can't be read ({why})")),
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
    pub arrangement: Arrangement,
    /// Every Code file, by its path in the project, `/`-separated: all the build may import
    /// from the project (FMT-6a).
    pub code: BTreeMap<String, String>,
    /// Lane files by address: their path and text.
    lanes: BTreeMap<LaneAddr, (PathBuf, String)>,
    /// State sidecars' text, by path.
    sidecars: BTreeMap<PathBuf, String>,
    binaries: BTreeMap<PathBuf, Stored>,
    unknown: BTreeSet<PathBuf>,
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
        Ok(Project { dir: snap.dir.clone(), manifest, arrangement, code, lanes, sidecars, binaries, unknown, unreadable })
    }

    pub fn entry(&self) -> PathBuf {
        self.dir.join(&self.manifest.project.entry)
    }

    /// The rate the engine runs and exports at (RENDER-31).
    pub fn sample_rate(&self) -> u32 {
        self.manifest.project.sample_rate
    }
}
