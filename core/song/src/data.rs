//! A revision's data, read against what the code declares: every lane's clips, the verified
//! plugin states, and the problems found on the way (FMT-9, FMT-12, FMT-15, FMT-17, FMT-7).
//!
//! Nothing here blocks the song. A lane whose file fails keeps the data the session last
//! accepted for it, or plays empty; a clip whose asset is missing is silent; a state blob
//! that can't be verified isn't loaded.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::arrangement::{Arrangement, TempoMap};
use crate::diag::{Diagnostic, Fix};
use crate::hash::{HashCache, Sha};
use crate::key::{ClipId, Key, LaneAddr};
use crate::lane::{self, AbsNote, AudioClip, ControlLane, Holds, LaneData, LaneError, LaneKind, canonical, number};
use crate::path::{AssetName, DataPath, slashed};
use crate::tx::{Base, Confined, Expect, Op, Payload, TxPath};
use crate::{Project, Stored};

/// What the code declares that reads data: lanes, and plugins with saved state.
#[derive(Clone, Debug, Default)]
pub struct Decls {
    pub lanes: Vec<(LaneAddr, LaneKind)>,
    pub states: Vec<StateDecl>,
}

/// A plugin whose state is saved, at `data/<module>/<path…>.state`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDecl {
    pub module: Key,
    /// The `key()` frames, then the plugin key.
    pub path: Vec<Key>,
    /// The plugin as the code names it: `clap:org.surge-synth-team.surge-xt`.
    pub plugin: String,
}

/// A state blob's required sidecar, `<plugin-key>.state.json`: exactly which plugin wrote it
/// (FMT-17). No parameter values, and no hash.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sidecar {
    pub plugin: String,
    pub plugin_version: String,
}

impl Sidecar {
    pub fn read(text: &str) -> Result<Sidecar, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    /// The canonical form (FMT-8a).
    pub fn canonical(&self) -> String {
        let s = |x: &str| serde_json::to_string(x).expect("a string serialises");
        format!("{{\"plugin\": {}, \"plugin_version\": {}}}\n", s(&self.plugin), s(&self.plugin_version))
    }
}

/// The data a renderer plays: the arrangement, each declared lane's clips, and the state blobs
/// that may be loaded. The engine's `DataSet`.
#[derive(Clone, Debug, Default)]
pub struct DataSet {
    pub arrangement: Arc<Arrangement>,
    tempo: Arc<TempoMap>,
    lanes: BTreeMap<LaneAddr, Arc<LaneData>>,
    /// Verified state blobs: `(module, path)` → the blob, relative to the project, and its hash
    /// as the snapshot read it.
    states: BTreeMap<(Key, Vec<Key>), (PathBuf, Option<Sha>)>,
    /// Assets clips may play, by exact name: shared by every revision with the same binaries.
    assets: Arc<BTreeSet<String>>,
    /// The binaries `assets` was listed from, so a revision whose binaries are these lists none
    /// again (thousands of assets beside a lane edit). Not data.
    assets_of: Option<Arc<BTreeMap<PathBuf, Stored>>>,
    /// The project folder, where assets and state blobs are read from. None: no files.
    dir: Option<PathBuf>,
    /// Where the last clip of any lane file ends, read by a lane or not.
    end: f64,
    /// The text each lane read here was read from, as its kind: a later read of the same text as
    /// the same kind keeps the same data instead of parsing it again (PLANE-1's data path costs
    /// the files a change touches, not the project's). Not data: what plays is `lanes`.
    read: BTreeMap<LaneAddr, (LaneKind, Arc<str>)>,
}

impl Default for TempoMap {
    fn default() -> TempoMap {
        Arrangement::default().tempo_map()
    }
}

impl DataSet {
    /// No project: 120 bpm, no lanes, no files.
    pub fn empty() -> DataSet {
        DataSet::default()
    }

    /// Only an arrangement and lanes: for tests and tools that render data built in memory.
    pub fn with_lanes(arrangement: Arrangement, lanes: impl IntoIterator<Item = (LaneAddr, LaneData)>) -> DataSet {
        let tempo = Arc::new(arrangement.tempo_map());
        let lanes: BTreeMap<LaneAddr, Arc<LaneData>> = lanes.into_iter().map(|(a, d)| (a, Arc::new(d))).collect();
        let end = lanes.values().map(|d| d.end()).fold(0.0, f64::max);
        DataSet { arrangement: Arc::new(arrangement), tempo, lanes, end, ..DataSet::default() }
    }

    pub fn tempo(&self) -> &TempoMap {
        &self.tempo
    }

    pub fn lane(&self, module: &str, lane: &str) -> Option<&Arc<LaneData>> {
        self.lanes.get(&LaneAddr::new(module, lane).ok()?)
    }

    pub fn lanes(&self) -> impl Iterator<Item = (&LaneAddr, &Arc<LaneData>)> {
        self.lanes.iter()
    }

    /// A notes lane's notes in song time, in a total order; none when it has no data.
    pub fn notes(&self, module: &str, lane: &str) -> Vec<AbsNote> {
        match self.lane(module, lane).map(|d| &**d) {
            Some(LaneData::Notes(clips)) => lane::notes(clips),
            _ => vec![],
        }
    }

    pub fn control(&self, module: &str, lane: &str) -> ControlLane {
        match self.lane(module, lane).map(|d| &**d) {
            Some(LaneData::Control { clips, .. }) => ControlLane::new(clips.clone()),
            _ => ControlLane::default(),
        }
    }

    /// An audio lane's clips, in canonical order.
    pub fn audio(&self, module: &str, lane: &str) -> &[AudioClip] {
        match self.lane(module, lane).map(|d| &**d) {
            Some(LaneData::Audio(clips)) => clips,
            _ => &[],
        }
    }

    /// Where to read the asset a clip names, if it may be played: an exact entry of `assets/`
    /// that could be read when the revision was made (FMT-15).
    pub fn asset_path(&self, name: &str) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let name = AssetName::new(name).ok().filter(|n| self.assets.contains(n.as_str()))?;
        Some(dir.join(DataPath::Asset(name).path()))
    }

    /// The verified state blob of the plugin at `path` in `module`, if there is one: a blob
    /// whose sidecar names the plugin the code does (FMT-17).
    pub fn state(&self, module: &str, path: &[&str]) -> Option<PathBuf> {
        let module = Key::new(module).ok()?;
        let path = path.iter().map(|k| Key::new(k)).collect::<Result<Vec<_>, _>>().ok()?;
        Some(self.dir.as_ref()?.join(&self.states.get(&(module, path))?.0))
    }

    /// The hash of the verified state blob [`Self::state`] names, as the snapshot read it: what
    /// carrying a plugin instance across a rebuild compares (HOST-5).
    pub fn state_hash(&self, module: &str, path: &[&str]) -> Option<Sha> {
        let module = Key::new(module).ok()?;
        let path = path.iter().map(|k| Key::new(k)).collect::<Result<Vec<_>, _>>().ok()?;
        self.states.get(&(module, path))?.1
    }

    /// The beat where the song's last clip ends: of any lane file, whether a lane reads it or
    /// not, so data a rename left behind still counts until it is moved or deleted.
    pub fn end(&self) -> f64 {
        self.end
    }

    /// Lane `addr`'s data, when this set read it from exactly `text` as `kind`: what a parse of
    /// `text` gives, already made (PLANE-1's data path parses only what a change touched).
    pub fn parsed(&self, addr: &LaneAddr, kind: &LaneKind, text: &str) -> Option<&Arc<LaneData>> {
        let (k, t) = self.read.get(addr)?;
        let same = (t.as_ptr() == text.as_ptr() && t.len() == text.len()) || **t == *text;
        if k == kind && same { self.lanes.get(addr) } else { None }
    }

    /// This set as the `previous` of a read in which lane `addr`'s file is `text`, which reads as
    /// `kind` to exactly `data`: a Tx's own lane, made by its plan, is then not parsed again. The
    /// caller vouches that `data` is what [`LaneData::read`] gives for `text`.
    pub fn seeded(&self, addr: &LaneAddr, kind: &LaneKind, text: Arc<str>, data: Arc<LaneData>) -> DataSet {
        let mut d = self.clone();
        d.lanes.insert(addr.clone(), data);
        d.read.insert(addr.clone(), (kind.clone(), text));
        d
    }
}

/// A lane file no declared lane reads (DIAG-6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unused<'a> {
    pub path: &'a Path,
    pub addr: &'a LaneAddr,
    pub text: &'a str,
}

impl Project {
    /// Read every declared lane and state against its declaration. `previous` is the data the
    /// session last accepted: a lane whose file fails keeps playing it (FMT-9), and one whose
    /// file is the text it was read from there, as the same kind, keeps that read (the same
    /// data, as a parse of that text gives) instead of parsing it again.
    pub fn data(&self, decls: &Decls, previous: Option<&DataSet>) -> (DataSet, Vec<Diagnostic>) {
        let mut diags = vec![];
        let mut lanes = BTreeMap::new();
        let mut read = BTreeMap::new();
        for (addr, kind) in &decls.lanes {
            if lanes.contains_key(addr) {
                continue;
            }
            let kept = previous.and_then(|p| p.lanes.get(addr)).filter(|d| d.fits(kind));
            let then = match kept {
                Some(_) => "Until then it plays the data it had before this change.",
                None => "Until then it plays nothing.",
            };
            let error = match self.lanes.get(addr) {
                Some((path, text)) => {
                    let file = slashed(path);
                    // Read from this very text, as this kind: the same data a parse would give.
                    let same = previous.and_then(|p| Some((p.read.get(addr)?, p.lanes.get(addr)?)));
                    let parsed = match same {
                        Some(((k, t), d)) if k == kind && (Arc::ptr_eq(t, text) || **t == **text) => Ok((d.clone(), t.clone())),
                        _ => LaneData::read(text, kind).map(|d| (Arc::new(d), text.clone())),
                    };
                    match parsed {
                        Ok((data, source)) => {
                            diags.extend(self.clip_ids(addr, &file, text, &data));
                            if let LaneData::Audio(clips) = &*data {
                                diags.extend(self.assets(addr, &file, text, clips, &data));
                            }
                            lanes.insert(addr.clone(), data);
                            read.insert(addr.clone(), (kind.clone(), source));
                            continue;
                        }
                        Err(e) => lane_error(addr, &file, text, e, then),
                    }
                }
                // A file that can't be read at all fails as one that doesn't parse (FMT-9).
                None => match self.unreadable_at(&DataPath::lane(addr).path()) {
                    Some((at, why)) => {
                        let file = DataPath::lane(addr).to_string();
                        let what =
                            if at == file { format!("{file} can't be read") } else { format!("{at} can't be read, nor {file} in it") };
                        let fix = format!("Fix it, or its permissions, so lane `{}` of {} can read it", addr.lane, addr.module);
                        Diagnostic::error("lane.invalid", format!("{what} ({why}). {fix}. {then}")).file(&file).module(&addr.module)
                    }
                    None => continue,
                },
            };
            diags.push(error);
            if let Some(k) = kept {
                lanes.insert(addr.clone(), k.clone());
            }
        }
        let states = self.states(decls, &mut diags);
        diags.extend(self.unread(decls));
        for p in self.unknown.iter().filter(|p| DataPath::parse(p).unknown_data()) {
            let file = slashed(p);
            diags.push(Diagnostic::notice("file.unknown", unknown_file(&file)).file(file));
        }
        // Listed again only when the binaries changed.
        let listed = previous.filter(|p| p.assets_of.as_ref().is_some_and(|b| Arc::ptr_eq(b, &self.binaries))).map(|p| p.assets.clone());
        let assets = listed.unwrap_or_else(|| {
            let assets = crate::under(&self.binaries, Path::new(crate::path::ASSETS)).filter_map(|(p, s)| match (DataPath::parse(p), s) {
                (DataPath::Asset(a), Stored::Hash(_) | Stored::Listed) => Some(a.as_str().to_string()),
                _ => None,
            });
            Arc::new(assets.collect())
        });
        // A file a lane read ends where its data does; only the others are parsed for it here.
        let files = self.lanes.iter().filter(|(a, _)| !read.contains_key(*a)).filter_map(|(_, (_, text))| {
            let kind = match Holds::of(text) {
                Holds::Notes => LaneKind::Notes,
                Holds::Control => LaneKind::Control { unit: None },
                Holds::Audio => LaneKind::Audio,
                Holds::Empty | Holds::Unknown => return None,
            };
            let without_unit = LaneData::read(text, &kind).or_else(|_| LaneData::read(text, &LaneKind::Control { unit: None }));
            without_unit.ok().map(|d| d.end())
        });
        let end = files.chain(lanes.values().map(|d| d.end())).fold(0.0, f64::max);
        let data = DataSet {
            arrangement: Arc::new(self.arrangement.clone()),
            tempo: Arc::new(self.arrangement.tempo_map()),
            lanes,
            states,
            assets,
            assets_of: Some(self.binaries.clone()),
            dir: Some(self.dir.clone()),
            end,
            read,
        };
        (data, diags)
    }

    /// Lane files no declared lane reads.
    pub fn unused(&self, declared: &BTreeSet<LaneAddr>) -> Vec<Unused<'_>> {
        self.lanes.iter().filter(|(a, _)| !declared.contains(*a)).map(|(addr, (path, text))| Unused { path, addr, text }).collect()
    }

    /// The file holding a lane's clips, if there is one, whether or not it can be read.
    pub fn lane_file(&self, addr: &LaneAddr) -> Option<&Path> {
        let unreadable = || self.unreadable.get_key_value(&DataPath::lane(addr).path()).map(|(p, _)| p.as_path());
        self.lanes.get(addr).map(|(p, _)| p.as_path()).or_else(unreadable)
    }

    /// Data files and folders that couldn't be read, with why (FMT-9).
    pub fn unreadable(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.unreadable.iter().map(|(p, e)| (p.as_path(), e.as_str()))
    }

    /// Why `path` can't be read, if it can't, or a folder it is in can't be: that path,
    /// `/`-separated, and why.
    fn unreadable_at(&self, path: &Path) -> Option<(String, &str)> {
        self.unreadable.iter().find(|(p, _)| path.starts_with(p)).map(|(p, e)| (slashed(p), e.as_str()))
    }

    /// A problem for each data file or folder that can't be read and that no declared lane or
    /// plugin reads, so none goes unseen (FMT-9, DIAG-4). Ones a lane or a plugin reads are
    /// that lane's or that plugin's problem.
    fn unread(&self, decls: &Decls) -> Vec<Diagnostic> {
        let lanes = decls.lanes.iter().map(|(a, _)| DataPath::lane(a).path());
        let states = decls.states.iter().flat_map(|s| {
            [false, true].map(|sidecar| DataPath::PluginState { module: s.module.clone(), path: s.path.clone(), sidecar }.path())
        });
        let read: Vec<PathBuf> = lanes.chain(states).collect();
        let mut out = vec![];
        for (p, why) in &self.unreadable {
            if read.iter().any(|r| r.starts_with(p)) || !p.starts_with(crate::path::DATA) {
                continue;
            }
            let file = slashed(p);
            let d = match DataPath::parse(p) {
                DataPath::PluginState { .. } => Diagnostic::error(
                    "state.unverified",
                    format!("{file} can't be read ({why}), so the state beside it can't be verified. Fix it, or its permissions"),
                ),
                _ => Diagnostic::error(
                    "lane.invalid",
                    format!("{file} can't be read ({why}). No lane reads it now, so nothing else changes; fix it, or its permissions"),
                ),
            };
            out.push(match folder_module(p) {
                Some(m) => d.file(file).module(m),
                None => d.file(file),
            });
        }
        out
    }

    /// Every lane file, by address.
    pub fn lane_files(&self) -> impl Iterator<Item = (&LaneAddr, &Path, &str)> {
        self.lanes.iter().map(|(a, (p, t))| (a, p.as_path(), &**t))
    }

    /// Files under `data/` that aren't data music2 reads (FMT-7), by path.
    pub fn unknown_files(&self) -> impl Iterator<Item = &Path> {
        self.unknown.iter().map(PathBuf::as_path).filter(|p| DataPath::parse(p).unknown_data())
    }

    /// Every plugin state file, blobs and sidecars, by path.
    pub fn state_files(&self) -> impl Iterator<Item = &Path> {
        // Plugin states are all under `data/`: the assets beside them aren't looked at.
        let blobs = crate::under(&self.binaries, Path::new(crate::path::DATA))
            .map(|(p, _)| p)
            .filter(|p| matches!(DataPath::parse(p), DataPath::PluginState { .. }));
        blobs.chain(self.sidecars.keys()).map(PathBuf::as_path)
    }

    /// What the snapshot knows of an asset or a state blob.
    pub fn binary(&self, path: &Path) -> Option<&Stored> {
        self.binaries.get(path)
    }

    /// A state sidecar's text.
    pub fn sidecar(&self, path: &Path) -> Option<&str> {
        self.sidecars.get(path).map(|t| &**t)
    }

    /// Every module that has a folder under `data/`: lanes or plugin state, read or not.
    pub fn data_modules(&self) -> BTreeSet<Key> {
        let blobs = crate::under(&self.binaries, Path::new(crate::path::DATA)).map(|(p, _)| p);
        let states = blobs.chain(self.sidecars.keys()).filter_map(|p| DataPath::parse(p).module().cloned());
        let unread = self.unreadable.keys().filter_map(|p| folder_module(p));
        self.lanes.keys().map(|a| a.module.clone()).chain(states).chain(unread).collect()
    }

    /// Clip ids (FMT-12): anonymous and invalid ids are Notices, duplicates a Warning, each
    /// with a fix that rewrites the lane. Ids are never assigned on read.
    fn clip_ids(&self, addr: &LaneAddr, file: &str, text: &str, data: &LaneData) -> Vec<Diagnostic> {
        let (ids, others) = (data.ids(), data.other_ids());
        let anonymous = ids.iter().zip(&others).filter(|(i, o)| i.is_none() && o.is_none()).count();
        // An id written as a number or any other non-string breaks the rule too (FMT-12).
        let invalid: Vec<String> = ids
            .iter()
            .flatten()
            .filter(|i| !ClipId::valid(i))
            .map(|i| format!("`{i}`"))
            .chain(others.iter().flatten().map(|v| format!("`{v}`")))
            .collect();
        let mut seen = BTreeSet::new();
        let dups: BTreeSet<&str> = ids.iter().flatten().copied().filter(|i| ClipId::valid(i) && !seen.insert(*i)).collect();
        let mut out = vec![];
        let assign = || {
            let mut d = data.clone();
            mint(&mut d, addr, text, |id, _| id.is_some_and(ClipId::valid));
            rewrite("assign-clip-ids", "Give every clip an id".into(), true, file, text, &d)
        };
        if anonymous > 0 {
            let n = if anonymous == 1 { "A clip has no id".to_string() } else { format!("{anonymous} clips have no id") };
            out.push(
                Diagnostic::notice(
                    "clip.anonymous",
                    format!("{file}: {n}, so it can be addressed only within one revision. Assign ids to fix it"),
                )
                .file(file)
                .module(&addr.module)
                .fix(assign()),
            );
        }
        if !invalid.is_empty() {
            let list = invalid.join(", ");
            out.push(
                Diagnostic::notice(
                    "clip.invalid-id",
                    format!(
                        "{file}: the clip id {list} isn't a string of 1 to 16 lowercase letters and digits, so the clip is read as \
                         anonymous. Assign ids to fix it"
                    ),
                )
                .file(file)
                .module(&addr.module)
                .fix(assign()),
            );
        }
        if !dups.is_empty() {
            let mut d = data.clone();
            let mut kept = BTreeSet::new();
            // The first of each shared id keeps it; clips without a valid id are another fix's.
            mint(&mut d, addr, text, |id, _| id.is_none_or(|i| !dups.contains(i) || kept.insert(i.to_string())));
            let list = dups.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", ");
            out.push(
                Diagnostic::warning(
                    "clip.duplicate-id",
                    format!("{file}: two clips share the id {list}. Renumber them so each has its own"),
                )
                .file(file)
                .module(&addr.module)
                .fix(rewrite("renumber-clips", "Give each clip its own id".into(), true, file, text, &d)),
            );
        }
        out
    }

    /// Asset references (FMT-15): found by listing `assets/`, never by opening a path.
    fn assets(&self, addr: &LaneAddr, file: &str, text: &str, clips: &[AudioClip], data: &LaneData) -> Vec<Diagnostic> {
        let mut out = vec![];
        let mut missing: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        let mut changed: BTreeMap<&str, Sha> = BTreeMap::new();
        let mut unpinned: BTreeMap<&str, Sha> = BTreeMap::new();
        for c in clips {
            let stored = AssetName::new(&c.asset).ok().and_then(|a| self.binaries.get(&DataPath::Asset(a.clone()).path()).map(|s| (s, a)));
            let sha = match stored {
                Some((Stored::Hash(sha), _)) => Some(*sha),
                // Listed only: a snapshot built in memory over an asset no lane used before.
                Some((Stored::Listed, a)) => HashCache::global().hash(&self.dir.join(DataPath::Asset(a).path())).ok(),
                Some((Stored::Unreadable(_), _)) | None => {
                    missing.entry(&c.asset).or_default().push(c.start);
                    continue;
                }
            };
            let Some(sha) = sha else { continue };
            match &c.hash {
                None => {
                    unpinned.insert(&c.asset, sha);
                }
                Some(pin) if *pin != sha.pin() => {
                    changed.insert(&c.asset, sha);
                }
                Some(_) => {}
            }
        }
        for (name, starts) in missing {
            let at = starts.iter().map(|s| number(*s)).collect::<Vec<_>>().join(", ");
            let beats = if starts.len() == 1 { format!("the clip at beat {at}") } else { format!("the clips at beats {at}") };
            // A file whose name is this one in another Unicode form isn't an asset (FMT-14), so no
            // reference can name it; it is renamed instead.
            let denormal = self.unknown.iter().any(|p| crate::path::denormal_asset(p).is_some_and(|a| a.as_str() == name));
            let named = AssetName::new(name).ok().map(|a| DataPath::Asset(a).path());
            let why = match named.as_ref().map(|p| (self.binaries.get(p), self.unreadable_at(p))) {
                Some((Some(Stored::Unreadable(e)), _)) => format!("can't be read ({e})"),
                Some((None, Some((at, why)))) => format!("can't be read: {at} can't be ({why})"),
                _ if named.is_none() => "isn't a file name inside assets/".into(),
                _ if denormal => "is in assets/ only with its name in another Unicode form (decomposed, as some disks and copies \
                                  write it), which isn't an asset's name"
                    .into(),
                _ => "isn't in assets/".into(),
            };
            // Only the case or the Unicode normalisation differs: offer the exact name.
            let near: Vec<&str> = self.asset_names().filter(|n| *n != name && AssetName::folded(n) == AssetName::folded(name)).collect();
            let mut d = Diagnostic::error(
                "asset.missing",
                format!("{file}: `{name}` {why}, so {beats} {} silent", if starts.len() == 1 { "is" } else { "are" }),
            )
            .file(file)
            .module(&addr.module);
            if denormal && near.is_empty() {
                d.message += ". Rename the file so its name is in composed Unicode (NFC), and they play";
            }
            if let [exact] = near[..] {
                d.message += &format!(". assets/ has `{exact}`: use that name");
                let mut fixed = data.clone();
                if let LaneData::Audio(cs) = &mut fixed {
                    cs.iter_mut().filter(|c| c.asset == name).for_each(|c| c.asset = exact.to_string());
                }
                d = d.fix(rewrite("fix-asset-name", format!("Use `{exact}`"), true, file, text, &fixed));
            }
            out.push(d);
        }
        let repin = |pins: &BTreeMap<&str, Sha>, only: Option<&str>| {
            let mut fixed = data.clone();
            if let LaneData::Audio(cs) = &mut fixed {
                for c in cs.iter_mut().filter(|c| only.is_none_or(|o| c.asset == o)) {
                    if let Some(sha) = pins.get(c.asset.as_str()) {
                        c.hash = Some(sha.pin());
                    }
                }
            }
            fixed
        };
        for (name, sha) in &changed {
            let fixed = repin(&changed, Some(name));
            out.push(
                Diagnostic::warning(
                    "asset.changed",
                    format!(
                        "{file}: assets/{name} has changed since its clips were placed (its hash is now {}). They play the file as it \
                         is; repin it to keep this version",
                        sha.pin()
                    ),
                )
                .file(file)
                .module(&addr.module)
                .fix(rewrite("repin-asset", format!("Pin `{name}` as it is now"), true, file, text, &fixed)),
            );
        }
        if !unpinned.is_empty() {
            let list = unpinned.keys().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
            out.push(
                Diagnostic::notice(
                    "asset.unpinned",
                    format!("{file}: clips of {list} have no `hash`, so a change to the file would go unnoticed. Pin them"),
                )
                .file(file)
                .module(&addr.module)
                .fix(rewrite(
                    "repin-asset",
                    "Pin the assets as they are now".into(),
                    true,
                    file,
                    text,
                    &repin(&unpinned, None),
                )),
            );
        }
        out
    }

    /// Every file in `assets/`, by name.
    fn asset_names(&self) -> impl Iterator<Item = &str> {
        self.binaries.keys().filter_map(|p| p.strip_prefix(crate::path::ASSETS).ok()?.to_str())
    }

    /// Plugin states (FMT-17): a blob is loaded only with a sidecar naming the plugin the code
    /// does; otherwise the plugin plays its defaults, with an Error.
    fn states(&self, decls: &Decls, diags: &mut Vec<Diagnostic>) -> BTreeMap<(Key, Vec<Key>), (PathBuf, Option<Sha>)> {
        let mut out = BTreeMap::new();
        let at =
            |module: &Key, path: &[Key], sidecar| DataPath::PluginState { module: module.clone(), path: path.to_vec(), sidecar }.path();
        for s in &decls.states {
            let (blob, side) = (at(&s.module, &s.path, false), at(&s.module, &s.path, true));
            let (file, sidefile) = (slashed(&blob), slashed(&side));
            let Some(stored) = self.binaries.get(&blob) else {
                // The folder it would be in can't be listed, so whether it has state is unknown (FMT-9).
                if let Some((at, why)) = self.unreadable_at(&blob) {
                    let why =
                        format!("{at} can't be read ({why}), nor {file} in it. The state isn't loaded, so the plugin plays its defaults");
                    diags.push(Diagnostic::error("state.unverified", why).file(&file).module(&s.module));
                }
                continue;
            };
            let sidecar = self.sidecars.get(&side);
            let unread_side = self.unreadable_at(&side);
            let problem = match (stored, sidecar.map(|t| Sidecar::read(t))) {
                (Stored::Unreadable(e), _) => Some(("state.unverified", format!("{file} can't be read ({e})"))),
                (_, None) => Some((
                    "state.unverified",
                    match unread_side {
                        Some((at, why)) => format!("{at} can't be read ({why}), so {file} can't be verified"),
                        None => format!("{file} has no {sidefile} saying which plugin wrote it"),
                    },
                )),
                (_, Some(Err(e))) => Some(("state.unverified", format!("{sidefile} can't be read: {e}"))),
                (_, Some(Ok(c))) if c.plugin != s.plugin => {
                    Some(("state.plugin-mismatch", format!("{file} was saved by {}, but the code plays {} here", c.plugin, s.plugin)))
                }
                _ => None,
            };
            let Some((code, why)) = problem else {
                let sha = match stored {
                    Stored::Hash(sha) => Some(*sha),
                    _ => None,
                };
                out.insert((s.module.clone(), s.path.clone()), (blob, sha));
                continue;
            };
            let mut d = Diagnostic::error(code, format!("{why}. The state isn't loaded, so the plugin plays its defaults"))
                .file(&file)
                .module(&s.module);
            // Another plugin of the kind that saved it, with no state of its own, can take it.
            let saved_by = sidecar.and_then(|t| Sidecar::read(t).ok()).map(|c| c.plugin);
            if code == "state.plugin-mismatch"
                && let Some(to) = decls
                    .states
                    .iter()
                    .find(|o| Some(&o.plugin) == saved_by.as_ref() && !self.binaries.contains_key(&at(&o.module, &o.path, false)))
            {
                let (to_blob, to_side) = (at(&to.module, &to.path, false), at(&to.module, &to.path, true));
                let at = to.path.iter().map(Key::as_str).collect::<Vec<_>>().join("/");
                let mut moves = vec![(blob.clone(), stored.sha(), to_blob)];
                moves.extend(sidecar.map(|t| (side.clone(), Some(Sha::of(t.as_bytes())), to_side)));
                // A plugin state moves with its sidecar; the state it then loads is the Tx's own
                // write (C1).
                let label = ("unused plugin state".to_string(), format!("{} › {at}", to.module));
                if let Some(f) = relink_fix(&moves, label, format!("Move it to {at}"), false, Expect::Confined(Confined::default())) {
                    d = d.fix(f);
                }
            }
            let mut files = vec![(blob.clone(), stored.sha())];
            files.extend(sidecar.map(|t| (side.clone(), Some(Sha::of(t.as_bytes())))));
            let label = ("unused plugin state".to_string(), "deleted".to_string());
            let delete = delete_fix(&files, label, "Delete the saved state");
            diags.push(match delete {
                Some(f) => d.fix(f),
                None => d,
            });
        }
        out
    }
}

/// The module whose folder `p` is, or is in, for a path under `data/`.
fn folder_module(p: &Path) -> Option<Key> {
    let seg = p.strip_prefix(crate::path::DATA).ok()?.components().next()?.as_os_str().to_str()?;
    Key::from_segment(seg)
}

/// What `file.unknown` says about `file` (FMT-7): where lanes and states go, and for a path that
/// spells a Windows device name bare, where it belongs (ID-9).
fn unknown_file(file: &str) -> String {
    let mut m = format!(
        "{file} isn't a file music2 reads, so nothing plays it. Lanes are data/<track>/<lane>.json and plugin states \
         data/<track>/…/<plugin>.state, with keys in lowercase letters, digits and hyphens, and a hyphen after a key that is a \
         Windows device name (aux, con, nul, prn, com1 to com9, lpt1 to lpt9): data/aux-/"
    );
    let segs: Vec<String> = file
        .split('/')
        .enumerate()
        .map(|(i, seg)| {
            let suffix = [".state.json", ".state", ".json"].into_iter().find(|x| seg.ends_with(x)).filter(|_| i > 1).unwrap_or("");
            let stem = &seg[..seg.len() - suffix.len()];
            if i > 0 && Key::device(stem) { format!("{stem}-{suffix}") } else { seg.to_string() }
        })
        .collect();
    let spelled = segs.join("/");
    if spelled != file && !matches!(DataPath::parse(Path::new(&spelled)), DataPath::Unknown(_)) {
        m += &format!(". This one belongs at {spelled}");
    }
    m
}

/// `lane.invalid` or `lane.unit-mismatch` (FMT-9, FMT-10), naming the file, the field and the fix.
fn lane_error(addr: &LaneAddr, file: &str, text: &str, e: LaneError, then: &str) -> Diagnostic {
    match e {
        LaneError::Invalid(why) => Diagnostic::error(
            "lane.invalid",
            format!("{file}: {why}. Fix the file so lane `{}` of {} can read it. {then}", addr.lane, addr.module),
        )
        .file(file)
        .module(&addr.module),
        LaneError::Unit { found, declared } => {
            let was = found.as_ref().map_or_else(|| "records no unit".to_string(), |u| format!("is automation in `{u}`"));
            let mut d = Diagnostic::error(
                "lane.unit-mismatch",
                format!(
                    "{file} {was}, but lane `{}` of {} is declared in `{declared}`. Retag the file if its values are in `{declared}`. \
                     {then}",
                    addr.lane, addr.module
                ),
            )
            .file(file)
            .module(&addr.module);
            // Retag: the same points, recorded in the declared unit.
            if let Ok(LaneData::Control { clips, .. }) = LaneData::read(text, &LaneKind::Control { unit: None }) {
                let fixed = LaneData::Control { unit: Some(declared.clone()), clips };
                d = d.fix(rewrite("retag", format!("Record it in `{declared}`"), found.is_none(), file, text, &fixed));
            }
            d
        }
    }
}

/// A fix that writes the lane file as `data`, in canonical form: a `Write` on the file's hash,
/// confined to its lane (the step 6 design's 11).
fn rewrite(code: &'static str, title: String, likely: bool, file: &str, text: &str, data: &LaneData) -> Fix {
    let path = TxPath::new(file).expect("a lane file's path");
    let lanes = match path.data() {
        DataPath::Lane { module, lane } => vec![LaneAddr { module: module.clone(), lane: lane.clone() }],
        _ => vec![],
    };
    let label = (lanes.first().map_or(file.to_string(), |a| format!("{} › {}", a.module, a.lane)), title.to_lowercase());
    let base = [(path.clone(), Base::Sha(Sha::of(text.as_bytes())))].into();
    let ops = vec![Op::Write { file: path, bytes: Payload::Text(canonical(data).into()) }];
    Fix::new(code, title, likely, label, base, ops, Expect::Confined(Confined { lanes, ..Confined::default() }))
}

/// A fix that renames each `(from, its hash, to)`, none of whose targets may exist; None when a
/// hash isn't known (a file that can't be read is never moved).
pub fn relink_fix(
    moves: &[(PathBuf, Option<Sha>, PathBuf)],
    label: (String, String),
    title: String,
    likely: bool,
    expect: Expect,
) -> Option<Fix> {
    let mut base = BTreeMap::new();
    let mut ops = vec![];
    for (from, sha, to) in moves {
        let (from, to) = (TxPath::new(&slashed(from)).ok()?, TxPath::new(&slashed(to)).ok()?);
        base.insert(from.clone(), Base::Sha((*sha)?));
        base.insert(to.clone(), Base::Absent);
        ops.push(Op::Rename { from, to });
    }
    Some(Fix::new("relink", title, likely, label, base, ops, expect))
}

/// A fix that removes each of `files` while it has its hash; it changes nothing that plays. None
/// when a hash isn't known.
pub fn delete_fix(files: &[(PathBuf, Option<Sha>)], label: (String, String), title: &str) -> Option<Fix> {
    let mut base = BTreeMap::new();
    let mut ops = vec![];
    for (file, sha) in files {
        let file = TxPath::new(&slashed(file)).ok()?;
        base.insert(file.clone(), Base::Sha((*sha)?));
        ops.push(Op::Remove { file });
    }
    Some(Fix::new("delete-data", title, false, label, base, ops, Expect::Identical))
}

/// Give a new id to every clip `keep` refuses (by its id and index), derived as a fix derives
/// them (FMT-11): from a Tx id of its own, made from the file's content, so the same file always
/// gets the same fix. An id already in the lane is skipped.
fn mint(data: &mut LaneData, addr: &LaneAddr, text: &str, mut keep: impl FnMut(Option<&str>, usize) -> bool) {
    let tx = format!("fix-{}", Sha::of(text.as_bytes()).pin());
    let mut taken: BTreeSet<String> = data.ids().into_iter().flatten().map(str::to_string).collect();
    let mut n = 0;
    let ids: Vec<Option<String>> = data.ids().into_iter().map(|i| i.map(str::to_string)).collect();
    for (i, id) in ids.iter().enumerate() {
        if keep(id.as_deref(), i) {
            continue;
        }
        let new = loop {
            let id = ClipId::derive(&tx, addr, n);
            n += 1;
            if !taken.contains(&id) {
                break id;
            }
        };
        taken.insert(new.clone());
        data.set_id(i, new);
    }
}

impl Stored {
    fn sha(&self) -> Option<Sha> {
        match self {
            Stored::Hash(s) => Some(*s),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
