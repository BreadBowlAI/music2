//! Checking and committing a Tx (WRITE-4, WRITE-5): the one write path every verified edit
//! takes, whoever builds it (the app's gestures, `m2 tx`, an agent, a fix).
//!
//! The pipeline (the step 6 design's 4.2): access; the Tx's shape (WRITE-3a); the draft rule; the
//! targets read from disk; the bases (WRITE-12: equal, or carried from the base text, else a
//! conflict naming the file); the baseline B (the accepted revision with each touched file as it
//! is on disk, so an outside change in it is never counted as the Tx's); the candidate, built by
//! the plane of the touched paths (no QuickJS for data alone); the delta from B to the candidate,
//! named in each file's base coordinates; the expectation; the diagnostics. `check` stops there
//! and writes nothing. `commit` runs it again (reusing what `check` built when nothing moved),
//! journals and stages the writes, renames them into place, accepts the candidate, and records
//! the inverse Tx for undo (WRITE-21).
//!
//! No step reads the folder beyond the Tx's own targets: outside changes reach the session
//! through the watcher, and through the targets' current bytes here.

pub mod delta;
pub mod expect;
pub mod journal;
pub mod stage;

#[cfg(test)]
mod delta_tests;
#[cfg(test)]
mod hook_tests;
#[cfg(test)]
mod kill_tests;
#[cfg(test)]
mod props;
#[cfg(test)]
mod share_tests;
#[cfg(test)]
mod stage_tests;
#[cfg(test)]
mod take_tests;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_song::tx::{Anchor, Base, Confined, Declared, Edit, Expect, ModuleChange, Op, Payload, Pos, Spot, Tx, TxPath};
use m2_song::{Class, DataPath, Diagnostic, LaneAddr, LaneData, Project, Sha, Snapshot, Stored};

use self::delta::{GraphDelta, Positions, Side};
use self::stage::{Before, Target};
use crate::session::{Revision, Session};
use crate::text::{Bias, Coords, Edits};
use crate::watch::Stamp;
use crate::{Code, Compiled};

/// What `check` found: the delta from the baseline to the candidate, its hash and the
/// diagnostics. Writes nothing (WRITE-4).
pub struct Checked {
    pub delta: GraphDelta,
    pub hash: Sha,
    pub diagnostics: Vec<Diagnostic>,
    candidate: Arc<Revision>,
    baseline: Arc<Revision>,
}

impl std::fmt::Debug for Checked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checked").field("delta", &self.delta).field("hash", &self.hash).field("diagnostics", &self.diagnostics).finish()
    }
}

impl Checked {
    fn of(p: &Prepared) -> Checked {
        let (candidate, baseline) = (p.candidate.clone(), p.baseline.clone());
        Checked { delta: p.delta.clone(), hash: p.hash, diagnostics: p.diagnostics.clone(), candidate, baseline }
    }

    /// The candidate, as a revision would be: what planners assert on.
    pub fn candidate(&self) -> &Revision {
        &self.candidate
    }

    /// The baseline the delta starts from: the accepted revision, with each touched file as it
    /// is on disk.
    pub fn baseline(&self) -> &Revision {
        &self.baseline
    }
}

/// A commit about to be written (PLANE-5): every check passed (its bases, its candidate read
/// against the accepted declarations, its delta, its expectation, its planner's assertion) and
/// every target still reads as its base (WRITE-15 step 1). Nothing is journaled or accepted yet:
/// when the write then fails, the accepted revision stays.
pub struct Writing {
    pub candidate: Arc<Revision>,
    pub accepted: Arc<Revision>,
    data_only: bool,
}

impl Writing {
    /// Only data files change, and the candidate plays the accepted code: no QuickJS ran
    /// (PLANE-1).
    pub fn data_only(&self) -> bool {
        self.data_only
    }
}

/// What the session's host does with a commit about to be written ([`Session::before_write`]).
pub type BeforeWrite = Box<dyn FnMut(&Writing) + Send>;

/// What a commit made.
#[derive(Clone, Debug)]
pub struct Committed {
    pub number: u64,
    pub code_rev: u64,
    pub data_rev: u64,
    /// The same as `check`'s, unless a target or the accepted revision moved between them.
    pub delta: GraphDelta,
    pub hash: Sha,
    pub diagnostics: Vec<Diagnostic>,
    /// What each text edit did, in the coordinates its file had just before the commit, so an
    /// open editor follows it without reloading.
    pub patches: Vec<Patch>,
    /// The paths written, each with the stamp of the session's own file there (None: gone, or not
    /// known to be ours), so the watcher's own writes make no batch (WRITE-19, R12).
    pub wrote: Vec<(PathBuf, Option<Stamp>)>,
}

/// One text edit as an editor applies it: bytes `start..end` of the file as it was, from line and
/// UTF-16 column `from` to `to`, became `text`.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub from: (u32, u32),
    pub to: (u32, u32),
}

/// A file a Tx's base doesn't apply to, and what it is now.
#[derive(Clone, Debug, PartialEq)]
pub struct ConflictFile {
    pub file: String,
    pub current: Option<Sha>,
    /// For a `Clips` op: the clip that changed (WRITE-21).
    pub clip: Option<String>,
    /// Why, when it isn't plainly that the file changed.
    pub why: Option<String>,
}

/// Why a Tx wasn't committed (WRITE-5).
#[derive(Clone, Debug)]
pub enum Refusal {
    /// Every base that no longer applies.
    Conflict { files: Vec<ConflictFile> },
    /// The Tx is malformed, doesn't build, or breaks its expectation.
    Rejected { diagnostics: Vec<Diagnostic>, unexpected: Option<Box<GraphDelta>> },
    /// The disk failed: nothing changed, or it was rolled back (6a.6).
    Io { message: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Conflict { files } => {
                let each: Vec<String> = files
                    .iter()
                    .map(|c| {
                        let clip = c.clip.as_ref().map(|k| format!(" (clip `{k}`)")).unwrap_or_default();
                        let why = c.why.clone().unwrap_or_else(|| "changed since the edit was planned".into());
                        format!("{}{clip} {why}", c.file)
                    })
                    .collect();
                write!(f, "{}", each.join("; "))
            }
            Refusal::Rejected { diagnostics, .. } => {
                write!(f, "{}", diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; "))
            }
            Refusal::Io { message } => write!(f, "{message}"),
        }
    }
}

/// Why [`Session::commit_with`] didn't commit.
#[derive(Debug)]
pub enum Uncommitted<E> {
    /// The session refused the Tx (WRITE-5).
    Refused(Refusal),
    /// The planner's assertion did (WRITE-9).
    Asserted(E),
}

fn rejected(d: Vec<Diagnostic>) -> Refusal {
    Refusal::Rejected { diagnostics: d, unexpected: None }
}

fn invalid(message: impl Into<String>) -> Refusal {
    rejected(vec![Diagnostic::error("tx.invalid", message)])
}

fn bad_path(message: impl Into<String>) -> Refusal {
    rejected(vec![Diagnostic::error("tx.path", message)])
}

/// A build's errors, as a refusal's diagnostics.
fn built(d: Vec<m2_ir::Diagnostic>) -> Vec<Diagnostic> {
    d.into_iter().map(crate::problem).collect()
}

/// A file a Tx touches, as it is on disk now.
#[derive(Clone, Debug, Default)]
pub(crate) struct Current {
    /// Its hash (a folder's directory hash); None when nothing is there.
    pub sha: Option<Sha>,
    pub bytes: Option<Arc<[u8]>>,
    /// Its text, when it is a text file in UTF-8.
    pub text: Option<String>,
    /// A folder's files (relative to the project) and bytes.
    pub folder: Vec<(PathBuf, Arc<[u8]>)>,
}

/// What a touched path becomes.
#[derive(Clone, Debug)]
enum New {
    /// These bytes, with their hash when the plan made it already.
    Bytes(Arc<[u8]>, Option<Sha>),
    Gone,
    /// Renamed: everything at `from` moves here.
    From(TxPath),
}

/// The ops applied to the current files (step 5), with what commit writes and undo undoes.
struct Planned {
    news: Vec<(TxPath, New)>,
    coords: BTreeMap<String, Coords>,
    /// Each touched text file's base text, where known.
    bases: BTreeMap<String, String>,
    /// Each TextEdit's ranges, in its file's base coordinates.
    edits: BTreeMap<String, Vec<Range<usize>>>,
    targets: Vec<Target>,
    inverse: Vec<Op>,
    inverse_base: BTreeMap<TxPath, Base>,
    patches: Vec<Patch>,
    /// The hash of each of `bases` that is known already: the file's on disk.
    base_shas: BTreeMap<String, Sha>,
    /// Lanes the plan read and wrote, as each new file reads (PLANE-1): the candidate's read takes
    /// them as made, instead of parsing the text the plan just wrote.
    seeds: Vec<Seed>,
}

/// A lane a `Clips` op made: its address and kind, its file's new text, and exactly what a read
/// of that text gives.
struct Seed {
    addr: LaneAddr,
    kind: m2_song::LaneKind,
    text: Arc<str>,
    data: Arc<LaneData>,
}

/// Everything `check` built, which `commit` reuses when nothing moved since.
pub(crate) struct Prepared {
    /// What `check` cached it under; a commit that plans for itself keys nothing.
    key: Option<(Sha, u64, Vec<Option<Sha>>)>,
    baseline: Arc<Revision>,
    candidate: Arc<Revision>,
    planned: Planned,
    delta: GraphDelta,
    hash: Sha,
    diagnostics: Vec<Diagnostic>,
    /// Both graphs by name, for judging the expectation.
    graphs: (delta::Graph, delta::Graph),
}

/// An undo step made of a Tx: its inverse, ready to commit (WRITE-21).
#[derive(Clone, Debug)]
pub(crate) struct TxStep {
    pub inverse: Tx,
    /// Committed without judging the graph, taken in like an outside write: the inverse of a
    /// save, of a recovery, or of a Tx committed on a seen delta.
    pub raw: bool,
    /// It touches only data files: judged `Confined { lanes it feeds }`, worked out when it is
    /// undone (R22).
    pub data_only: bool,
    /// The file it is shown as.
    pub file: String,
    /// The bytes of text and data it keeps alive, counted once, when it is made: undo's bound is
    /// checked at every step, over every step (WRITE-21).
    size: usize,
}

impl TxStep {
    pub fn new(inverse: Tx, raw: bool, data_only: bool, file: String) -> TxStep {
        let size = retained(&inverse);
        TxStep { inverse, raw, data_only, file, size }
    }

    /// The before-images in `blobs/` it needs.
    pub fn blobs(&self) -> Vec<Sha> {
        self.inverse
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Write { bytes: Payload::Local { sha, .. }, .. } => Some(*sha),
                _ => None,
            })
            .collect()
    }

    /// The bytes of text and data it keeps alive, against undo's 64 MiB (WRITE-21).
    pub fn retained(&self) -> usize {
        self.size
    }
}

/// The bytes of text and data `inverse` keeps alive.
fn retained(inverse: &Tx) -> usize {
    let bases: usize = inverse.base.values().map(|b| if let Base::WithText { text, .. } = b { text.len() } else { 0 }).sum();
    let ops: usize = (inverse.ops.iter())
        .map(|op| match op {
            Op::TextEdit { edits, .. } => edits.iter().map(|e| e.expected.len() + e.text.len()).sum(),
            Op::Write { bytes: Payload::Text(t), .. } => t.len(),
            Op::Write { bytes: Payload::Bytes(b), .. } => b.len(),
            Op::Clips { put, .. } => put.iter().map(|c| c.to_string().len()).sum(),
            _ => 0,
        })
        .sum();
    bases + ops
}

/// `p`'s state in `snap`: its hash, or for a folder its directory hash; `Some(None)` when it
/// exists but its hash isn't known (an asset only listed); None when it isn't there.
fn known(snap: &Snapshot, p: &TxPath) -> Option<Option<Sha>> {
    let path = p.path();
    match p.data() {
        DataPath::Module { .. } => {
            let mut files = vec![];
            for (k, _) in m2_song::under(&snap.files, &path) {
                files.push((k.clone(), snap.sha(k)));
            }
            for (k, s) in m2_song::under(&snap.binaries, &path) {
                files.push((k.clone(), journal::stored_sha(s)));
            }
            if files.is_empty() {
                return None;
            }
            let lanes = files
                .into_iter()
                .filter(|(k, _)| matches!(DataPath::parse(k), DataPath::Lane { .. } | DataPath::PluginState { .. }))
                .map(|(k, s)| Some((m2_song::path::slashed(k.strip_prefix(&path).ok()?), s?)))
                .collect::<Option<Vec<_>>>();
            Some(lanes.map(|l| m2_song::dir_hash(&l)))
        }
        _ => match snap.binaries.get(&path) {
            Some(s) => Some(journal::stored_sha(s)),
            None => snap.sha(&path).map(Some),
        },
    }
}

/// Put `bytes` at `p` in `snap` (None: nothing there), as a fresh read of the disk would have it.
pub(crate) fn put_file(snap: &mut Snapshot, p: &Path, bytes: Option<&[u8]>) {
    snap.files.remove(p);
    // Shared until changed: a project's thousands of assets are copied only when one changes.
    if snap.binaries.contains_key(p) {
        Arc::make_mut(&mut snap.binaries).remove(p);
    }
    snap.not_utf8.remove(p);
    snap.unreadable.remove(p);
    let Some(bytes) = bytes else { return };
    match DataPath::parse(p) {
        DataPath::Manifest
        | DataPath::Code(_)
        | DataPath::Arrangement
        | DataPath::Lane { .. }
        | DataPath::PluginState { sidecar: true, .. } => {
            let text: Arc<str> = match std::str::from_utf8(bytes) {
                Ok(t) => t.into(),
                Err(_) => {
                    snap.not_utf8.insert(p.to_path_buf(), Sha::of(bytes));
                    String::from_utf8_lossy(bytes).into()
                }
            };
            snap.files.insert(p.to_path_buf(), text);
        }
        DataPath::PluginState { sidecar: false, .. } | DataPath::Asset(_) => {
            Arc::make_mut(&mut snap.binaries).insert(p.to_path_buf(), Stored::Hash(Sha::of(bytes)));
        }
        // layout.json and anything else is no part of a snapshot (PLANE-3).
        _ => {}
    }
}

/// What the ops make of `snap`. A rename's source moves, so it is never removed first.
fn apply_news(snap: &mut Snapshot, news: &[(TxPath, New)]) {
    for (p, new) in news {
        match new {
            New::Bytes(b, _) => put_file(snap, &p.path(), Some(b)),
            New::Gone if news.iter().any(|(_, n)| matches!(n, New::From(f) if f == p)) => {}
            New::Gone => remove_under(snap, &p.path()),
            New::From(from) => move_under(snap, &from.path(), &p.path()),
        }
    }
}

/// Every entry of `snap` at `p` or inside it, gone.
fn remove_under(snap: &mut Snapshot, p: &Path) {
    snap.files.retain(|k, _| !k.starts_with(p));
    if m2_song::under(&snap.binaries, p).next().is_some() {
        Arc::make_mut(&mut snap.binaries).retain(|k, _| !k.starts_with(p));
    }
    snap.not_utf8.retain(|k, _| !k.starts_with(p));
    snap.unreadable.retain(|k, _| !k.starts_with(p));
}

/// Every entry of `snap` at `from` or inside it, moved to `to`.
fn move_under(snap: &mut Snapshot, from: &Path, to: &Path) {
    let moved = |k: &Path| k.strip_prefix(from).ok().map(|rest| if rest.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rest) });
    let files = std::mem::take(&mut snap.files);
    snap.files = files.into_iter().map(|(k, v)| (moved(&k).unwrap_or(k), v)).collect();
    if m2_song::under(&snap.binaries, from).next().is_some() {
        let bins = Arc::unwrap_or_clone(std::mem::take(&mut snap.binaries));
        snap.binaries = Arc::new(bins.into_iter().map(|(k, v)| (moved(&k).unwrap_or(k), v)).collect());
    }
    let bad = std::mem::take(&mut snap.not_utf8);
    snap.not_utf8 = bad.into_iter().map(|(k, v)| (moved(&k).unwrap_or(k), v)).collect();
    let unread = std::mem::take(&mut snap.unreadable);
    snap.unreadable = unread.into_iter().map(|(k, v)| (moved(&k).unwrap_or(k), v)).collect();
    if m2_song::under_set(&snap.unknown, from).next().is_some() {
        let unknown = Arc::unwrap_or_clone(std::mem::take(&mut snap.unknown));
        snap.unknown = Arc::new(unknown.into_iter().map(|k| moved(&k).unwrap_or(k)).collect());
    }
}

/// Assets as a fresh read lists them: hashed when a lane names them, else only listed. Only the
/// entries whose state changes are written, so the shared map is copied only then.
pub(crate) fn settle_assets(snap: &mut Snapshot) {
    // Only assets are settled: with none, there is nothing to read the lanes for. Assets are all
    // under `assets/`, one run of the map.
    let assets = Path::new(m2_song::path::ASSETS);
    if m2_song::under(&snap.binaries, assets).next().is_none() {
        return;
    }
    let mut named = BTreeSet::new();
    for (p, text) in &snap.files {
        if matches!(DataPath::parse(p), DataPath::Lane { .. }) {
            named.extend(named_by(text).iter().cloned());
        }
    }
    let changes: Vec<(PathBuf, Stored)> = m2_song::under(&snap.binaries, assets)
        .filter(|(p, s)| (**s != Stored::Listed || named.contains(*p)) && matches!(DataPath::parse(p), DataPath::Asset(_)))
        .filter_map(|(p, s)| match (named.contains(p), s) {
            (false, Stored::Listed) | (true, Stored::Hash(_) | Stored::Unreadable(_)) => None,
            (false, _) => Some((p.clone(), Stored::Listed)),
            (true, Stored::Listed) => Some((
                p.clone(),
                match m2_song::HashCache::global().hash(&snap.dir.join(p)) {
                    Ok(sha) => Stored::Hash(sha),
                    Err(e) => Stored::Unreadable(e.to_string()),
                },
            )),
        })
        .collect();
    if !changes.is_empty() {
        Arc::make_mut(&mut snap.binaries).extend(changes);
    }
}

/// The assets a lane file's text names, by the text itself: each text is read for them once,
/// however many snapshots share it. Keyed by the text's address and held weakly, so an entry is
/// only ever its own text's (an address is never reused while a `Weak` to it lives); entries
/// whose text is gone are dropped as others come.
static NAMED: std::sync::Mutex<BTreeMap<usize, Named>> = std::sync::Mutex::new(BTreeMap::new());

/// A lane text, held weakly, and the assets it names.
type Named = (std::sync::Weak<str>, Arc<[PathBuf]>);

fn named_by(text: &Arc<str>) -> Arc<[PathBuf]> {
    let key = Arc::as_ptr(text) as *const u8 as usize;
    let mine = |w: &std::sync::Weak<str>| w.upgrade().is_some_and(|t| Arc::ptr_eq(&t, text));
    if let Some((_, named)) = NAMED.lock().unwrap_or_else(|e| e.into_inner()).get(&key).filter(|(w, _)| mine(w)) {
        return named.clone();
    }
    // A lane file names an asset only in an `asset` key, spelled out or escaped.
    let marked = memchr::memmem::find(text.as_bytes(), b"\"asset\"").is_some() || memchr::memmem::find(text.as_bytes(), b"\\u").is_some();
    let mut named = vec![];
    if marked && let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        for c in v.get("clips").and_then(|c| c.as_array()).into_iter().flatten() {
            if let Some(a) = c.get("asset").and_then(|a| a.as_str()).and_then(|a| m2_song::AssetName::new(a).ok()) {
                named.push(DataPath::Asset(a).path());
            }
        }
    }
    let named: Arc<[PathBuf]> = named.into();
    let mut memo = NAMED.lock().unwrap_or_else(|e| e.into_inner());
    memo.retain(|_, (w, _)| w.strong_count() > 0);
    memo.insert(key, (Arc::downgrade(text), named.clone()));
    named
}

/// Whether `p` is spelled under `dir` as the disk spells it, through no link (WRITE-3a). On a
/// disk that ignores capitals or Unicode forms `Song.ts` reads and writes `song.ts`, and a link
/// to a folder inside the project gives its files a second name that no snapshot reads: a Tx
/// through either would change the real file while its delta saw another, whatever its base, so
/// both are refused. A name nothing answers to, in any spelling, is new from there on.
fn spelled(dir: &Path, p: &TxPath) -> Result<(), Refusal> {
    let path = p.path();
    let parts: Vec<&std::ffi::OsStr> = path.iter().collect();
    let mut at = dir.to_path_buf();
    for (i, name) in parts.iter().enumerate() {
        let next = at.join(name);
        let meta = match std::fs::symlink_metadata(&next) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(Refusal::Io { message: format!("{p} can't be read: {e}") }),
        };
        let listed: Vec<std::ffi::OsString> = std::fs::read_dir(&at)
            .map_err(|e| Refusal::Io { message: format!("{} can't be listed: {e}", at.display()) })?
            .flatten()
            .map(|e| e.file_name())
            .collect();
        if !listed.iter().any(|n| n == name) {
            // The entry it reaches, by its inode, to say how the disk spells it.
            #[cfg(unix)]
            let real = {
                use std::os::unix::fs::MetadataExt;
                let same = |n: &&std::ffi::OsString| {
                    std::fs::symlink_metadata(at.join(n)).is_ok_and(|m| (m.dev(), m.ino()) == (meta.dev(), meta.ino()))
                };
                listed.iter().find(same).map(|n| {
                    let mut real: PathBuf = parts[..i].iter().collect();
                    real.push(n);
                    m2_song::path::slashed(&real)
                })
            };
            #[cfg(not(unix))]
            let real: Option<String> = None;
            let real = real.map_or_else(|| "another way".to_string(), |r| format!("`{r}`"));
            return Err(bad_path(format!("{p} is spelled {real} on this disk: a Tx names a file exactly as the disk does")));
        }
        if meta.file_type().is_symlink() {
            let what = if i + 1 == parts.len() { "is a link" } else { "is in a folder that is a link" };
            return Err(bad_path(format!("{p} {what}, and music2 never writes through one")));
        }
        at = next;
    }
    Ok(())
}

/// `(line, UTF-16 column)` of byte `at` of `text`, from 0.
fn line_col(text: &str, at: usize) -> (u32, u32) {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = text[..line_start].matches('\n').count() as u32;
    (line, text[line_start..at].encode_utf16().count() as u32)
}

/// The paths of `snap` that differ from `other`'s, of every kind a revision holds: the keys
/// where any of the four maps differ. Each map is walked once in order; a text shared by both
/// (the same `Arc`) and a binaries map shared whole are equal without looking.
pub(crate) fn changed_paths(a: &Snapshot, b: &Snapshot) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    differ(&a.files, &b.files, |x, y| Arc::ptr_eq(x, y) || x == y, &mut out);
    if !Arc::ptr_eq(&a.binaries, &b.binaries) {
        differ(&a.binaries, &b.binaries, PartialEq::eq, &mut out);
    }
    differ(&a.unreadable, &b.unreadable, PartialEq::eq, &mut out);
    differ(&a.not_utf8, &b.not_utf8, PartialEq::eq, &mut out);
    out
}

/// The keys where `a` and `b` differ (one has it and the other doesn't, or `same` says they
/// aren't), into `out`: one merge of the two ordered maps.
fn differ<V>(a: &BTreeMap<PathBuf, V>, b: &BTreeMap<PathBuf, V>, same: impl Fn(&V, &V) -> bool, out: &mut BTreeSet<PathBuf>) {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.iter().peekable(), b.iter().peekable());
    loop {
        let order = match (x.peek(), y.peek()) {
            (None, None) => return,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some((ka, _)), Some((kb, _))) => ka.cmp(kb),
        };
        match order {
            Ordering::Less => {
                out.insert(x.next().expect("peeked").0.clone());
            }
            Ordering::Greater => {
                out.insert(y.next().expect("peeked").0.clone());
            }
            Ordering::Equal => {
                let ((k, va), (_, vb)) = (x.next().expect("peeked"), y.next().expect("peeked"));
                if !same(va, vb) {
                    out.insert(k.clone());
                }
            }
        }
    }
}

/// What kind of change a path is (PLANE-1): code (the manifest, code, and a folder that may hold
/// code), the arrangement, or data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Code,
    Arrangement,
    Data,
    None,
}

pub(crate) fn kind_of(p: &Path) -> Kind {
    match DataPath::parse(p).class() {
        Class::Code | Class::Manifest => Kind::Code,
        Class::Arrangement => Kind::Arrangement,
        Class::Data => Kind::Data,
        Class::Presentation => Kind::None,
        // An unreadable folder: under data/ or assets/ it is data; `data` itself holds the
        // arrangement; anywhere else it may hold code (FMT-9).
        Class::Ignored if p == Path::new(m2_song::path::DATA) => Kind::Arrangement,
        Class::Ignored if p.starts_with(m2_song::path::DATA) || p.starts_with(m2_song::path::ASSETS) => Kind::Data,
        Class::Ignored if p.starts_with(m2_song::path::LOCAL) => Kind::None,
        Class::Ignored => Kind::Code,
    }
}

/// Which modules claim which data files (DIAG-3): each declared lane's file and each declared
/// stateful plugin's blob and sidecar that exist.
pub(crate) fn claims(rev: &Revision) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    for (addr, _) in &rev.compiled.decls.lanes {
        if let Some(p) = rev.project.lane_file(addr) {
            out.insert(p.to_path_buf(), addr.module.to_string());
        }
    }
    for s in &rev.compiled.decls.states {
        for sidecar in [false, true] {
            let p = DataPath::PluginState { module: s.module.clone(), path: s.path.clone(), sidecar }.path();
            if rev.snapshot.files.contains_key(&p) || rev.snapshot.binaries.contains_key(&p) {
                out.insert(p, s.module.to_string());
            }
        }
    }
    out
}

/// `data.claimed` for each file that was there before, no module played, and some module plays
/// after (DIAG-3).
pub(crate) fn claimed(before: &Revision, after: &Revision) -> Vec<Diagnostic> {
    let (was, is) = (claims(before), claims(after));
    let existed = |p: &Path| before.snapshot.files.contains_key(p) || before.snapshot.binaries.contains_key(p);
    is.iter()
        .filter(|(p, _)| !was.contains_key(*p) && existed(p))
        .map(|(p, m)| {
            let label = after.compiled.ir.modules.iter().find(|x| x.key == *m).map_or(m.as_str(), |x| x.label());
            let file = m2_song::path::slashed(p);
            Diagnostic::notice("data.claimed", format!("{label} now plays {file}, which nothing played before")).file(&file).module(m)
        })
        .collect()
}

/// The delta from revision `a` to revision `b` (WRITE-8), each changed code file named in `a`'s
/// coordinates with its change as hunks: what the engine hears when what plays is `a` and the
/// accepted revision is `b` (the step 6 design's 5.5).
pub fn delta_between(a: &Revision, b: &Revision) -> GraphDelta {
    let mut coords = BTreeMap::new();
    for p in changed_paths(&a.snapshot, &b.snapshot) {
        if let (Class::Code, Some(x), Some(y)) = (DataPath::parse(&p).class(), a.snapshot.files.get(&p), b.snapshot.files.get(&p)) {
            coords.insert(m2_song::path::slashed(&p), Coords::revisions(Edits::between(x, y)));
        }
    }
    let old = Side { compiled: &a.compiled, snapshot: &a.snapshot, at: Positions { coords: &coords, new: false } };
    let new = Side { compiled: &b.compiled, snapshot: &b.snapshot, at: Positions { coords: &coords, new: true } };
    delta::compare(&old, &new).0
}

/// A problem's identity, to tell a candidate's new problems from the baseline's.
fn same(a: &Diagnostic, b: &Diagnostic) -> bool {
    (a.code, &a.src, &a.message) == (b.code, &b.src, &b.message)
}

/// A Tx id for an inverse: derived from the Tx it undoes.
fn inverse_id(id: &str, n: u64) -> String {
    format!("undo-{}", &Sha::of(format!("{id}\0{n}").as_bytes()).hex()[..12])
}

impl Session {
    /// Check `tx` (WRITE-4): its bases, its candidate, the delta from the baseline, the
    /// expectation and the diagnostics. Writes nothing. On `Expect::Seen` it never refuses for
    /// the expectation: that is how an agent learns the hash (WRITE-11), and how a planner probes
    /// a candidate.
    pub fn check(&mut self, tx: &Tx) -> Result<Checked, Refusal> {
        let currents = self.read_all(tx)?;
        let mut p = self.prepare(tx, &currents)?;
        let seen = matches!(tx.expect, Expect::Seen(_));
        if !seen {
            self.judge(tx, &mut p)?;
        }
        let checked = Checked::of(&p);
        p.key = Some(self.key(tx, &currents));
        self.cache = Some(p);
        Ok(checked)
    }

    /// Check `tx`, let `assert` judge the checked candidate (what a planner says its gesture means,
    /// WRITE-9), then commit it: what `check`, the assertion and `commit` do one after the other,
    /// with one read of the targets, one plan and one candidate (WRITE-4, WRITE-5). Nothing moves
    /// unseen meanwhile: the stage's own pre-check compares every target with what was read here
    /// (WRITE-15 step 1), and a lock taken again on the way has it judged from the disk anew.
    pub fn commit_with<E>(
        &mut self,
        tx: Tx,
        assert: impl FnOnce(&Session, &Checked) -> Result<(), E>,
    ) -> Result<Committed, Uncommitted<E>> {
        let refused = Uncommitted::Refused;
        // A recovery is checked against its journal, as `commit` does it.
        if matches!(tx.expect, Expect::Journal(_)) {
            let checked = self.check(&tx).map_err(refused)?;
            assert(self, &checked).map_err(Uncommitted::Asserted)?;
            return self.commit(tx).map_err(refused);
        }
        let currents = self.read_all(&tx).map_err(refused)?;
        let mut p = self.prepare(&tx, &currents).map_err(refused)?;
        // The expectation is judged before the assertion, as `check` does, except on `Seen`.
        let seen = matches!(tx.expect, Expect::Seen(_));
        if !seen {
            self.judge(&tx, &mut p).map_err(refused)?;
        }
        let checked = Checked::of(&p);
        assert(self, &checked).map_err(Uncommitted::Asserted)?;
        // The candidate is the commit's alone again, so accepting it copies nothing.
        drop(checked);
        let scans = self.scans;
        if let Some(r) = self.writable() {
            return Err(refused(rejected(vec![r])));
        }
        if self.scans != scans {
            // The lock was taken again and the journals read anew: one may hold a target now.
            return self.commit(tx).map_err(refused);
        }
        if seen {
            self.judge(&tx, &mut p).map_err(refused)?;
        }
        let (done, step) = self.write_prepared(&tx, p).map_err(refused)?;
        if let Some(step) = step {
            self.record_tx(step);
        }
        Ok(done)
    }

    /// Commit `tx` (WRITE-5): checked as `check` does, then journaled, staged and renamed into
    /// place under the lock, accepted, and recorded for undo as its inverse (WRITE-21). A new
    /// commit ends any redo.
    pub fn commit(&mut self, tx: Tx) -> Result<Committed, Refusal> {
        let (done, step) = self.commit_tx(&tx, false)?;
        if let Some(step) = step {
            self.record_tx(step);
        }
        Ok(done)
    }

    /// Commit `tx`; `raw` commits it unjudged and takes it in like an outside write. Returns what
    /// was committed and the step that undoes it (none when nothing changed).
    pub(crate) fn commit_tx(&mut self, tx: &Tx, raw: bool) -> Result<(Committed, Option<TxStep>), Refusal> {
        if let Some(r) = self.writable() {
            return Err(rejected(vec![r]));
        }
        let journal = matches!(tx.expect, Expect::Journal(_));
        if raw || journal {
            return self.commit_raw(tx);
        }
        let currents = self.read_all(tx)?;
        let key = self.key(tx, &currents);
        // What check built is reused when nothing moved since; the expectation is judged here
        // either way (a check on `Seen` never judged it).
        let mut prepared = match self.cache.take().filter(|c| c.key.as_ref() == Some(&key)) {
            Some(p) => p,
            None => self.prepare(tx, &currents)?,
        };
        self.judge(tx, &mut prepared)?;
        self.write_prepared(tx, prepared)
    }

    fn key(&self, tx: &Tx, currents: &BTreeMap<TxPath, Current>) -> (Sha, u64, Vec<Option<Sha>>) {
        (tx.hash(), self.accepted.number, currents.values().map(|c| c.sha).collect())
    }

    /// Steps 2 to 4 and the disk checks of WRITE-3a: the Tx's shape, the draft rule, and every
    /// target as it is on disk now.
    fn read_all(&mut self, tx: &Tx) -> Result<BTreeMap<TxPath, Current>, Refusal> {
        tx.validate().map_err(rejected)?;
        let touched = tx.touched();
        // A Tx touching an interrupted commit's targets waits until it is finished or discarded,
        // a folder holding one or a file inside one included (R15's overlap): moving it would
        // leave the journal nothing it can finish or undo. The fixes that do so are checked
        // against their journal instead.
        let blocked = journal::blocked(&self.interrupted);
        let own = match &tx.expect {
            Expect::Journal(id) => Some(id.as_str()),
            _ => None,
        };
        let interrupted = |p: &TxPath| {
            let p = p.path();
            blocked.iter().find(|(q, j)| (p.starts_with(q) || q.starts_with(&p)) && Some(j.as_str()) != own).map(|(_, j)| j)
        };
        if let Some((p, jid)) = touched.iter().find_map(|p| interrupted(p).map(|j| (p, j))) {
            return Err(rejected(vec![Diagnostic::error(
                "tx.interrupted",
                format!("a commit that touched {p} was interrupted (.music2/tx/{jid}.json): finish it or discard it first"),
            )]));
        }
        if let Expect::Journal(id) = &tx.expect {
            let Some(j) = self.interrupted.iter().find(|j| j.id == *id) else {
                return Err(invalid(format!("no interrupted commit is recorded as {id}")));
            };
            if !journal::restores(&self.dir, j, tx) {
                return Err(invalid(format!("this Tx doesn't restore what .music2/tx/{id}.json recorded")));
            }
        } else {
            // The draft rule (C7): a Tx touching code needs the draft's code to compile; one
            // touching only data needs an accepted code revision to read its lanes against.
            let code = touched.iter().any(|p| matches!(p.class(), Class::Code | Class::Manifest));
            if (code || self.accepted.code_rev == 0) && !self.draft_errors.is_empty() {
                let mut d = built(self.draft_errors.clone());
                if !code {
                    d.insert(0, Diagnostic::error("code.build", "the project's code has never compiled, so its lanes aren't known yet"));
                }
                return Err(rejected(d));
            }
        }
        let mut currents = BTreeMap::new();
        for (p, base) in &tx.base {
            // A name made where the project has it in other capitals would read as that one on a
            // disk that ignores capitals, or not at all.
            if matches!(base, Base::Absent)
                && let Some(by) = self.accepted.snapshot.taken(&p.path()).filter(|by| !by.starts_with(p.path()))
            {
                return Err(bad_path(format!(
                    "{p} can't be made: on a disk that ignores capitals it is {}, which is there already",
                    by.display()
                )));
            }
            currents.insert(p.clone(), self.read_current(p)?);
        }
        for op in &tx.ops {
            let file = match op {
                Op::TextEdit { file, .. } => file.clone(),
                Op::Clips { lane, .. } => TxPath::lane(lane),
                _ => continue,
            };
            let c = &currents[&file];
            if c.bytes.is_some() && c.text.is_none() {
                return Err(invalid(format!("{file} isn't UTF-8: re-save it as UTF-8 first")));
            }
        }
        Ok(currents)
    }

    /// One target as it is on disk now, refusing a link, a folder that is one, and a name the
    /// disk spells another way (WRITE-3a).
    pub(crate) fn read_current(&self, p: &TxPath) -> Result<Current, Refusal> {
        let file = self.dir.join(p.path());
        spelled(&self.dir, p)?;
        if let DataPath::Module { .. } = p.data() {
            let sha = stage::folder_sha(&mut stage::Real, &self.dir, p.as_str()).map_err(|e| invalid(e.to_string()))?;
            let mut folder = vec![];
            let mut stack = vec![file.clone()];
            while let Some(d) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&d) else { continue };
                for e in entries.flatten() {
                    let path = e.path();
                    if e.file_type().is_ok_and(|t| t.is_dir()) {
                        stack.push(path);
                    } else if !e.file_name().to_string_lossy().starts_with('.') {
                        let bytes = std::fs::read(&path).map_err(|e| Refusal::Io { message: e.to_string() })?;
                        folder.push((path.strip_prefix(&self.dir).expect("inside").to_path_buf(), bytes.into()));
                    }
                }
            }
            return Ok(Current { sha, bytes: None, text: None, folder });
        }
        match std::fs::read(&file) {
            Ok(bytes) => {
                let text = if p.is_text() { String::from_utf8(bytes.clone()).ok() } else { None };
                // A file that is the accepted text has that text's hash, made once.
                let (accepted, path) = (&self.accepted.snapshot, p.path());
                let sha = match accepted.files.get(&path) {
                    Some(t) if !accepted.not_utf8.contains_key(&path) && t.as_bytes() == bytes.as_slice() => Sha::of_shared(t),
                    _ => Sha::of(&bytes),
                };
                Ok(Current { sha: Some(sha), bytes: Some(bytes.into()), text, folder: vec![] })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Current::default()),
            Err(e) => Err(Refusal::Io { message: format!("{p} can't be read: {e}") }),
        }
    }

    /// Whether `p` waits (R3): it, or a file in it when it is a folder, changed on disk together
    /// with code that doesn't compile yet. A Tx taking it in would take in half of that batch.
    fn waits(&self, p: &TxPath) -> bool {
        let p = p.path();
        self.waiting.iter().any(|w| w.starts_with(&p) || p.starts_with(w))
    }

    /// The text whose hash is `sha` (WRITE-12): the Tx's own, then the store, then the texts the
    /// session's revisions had at `p`.
    fn base_text(&self, p: &TxPath, base: &Base) -> Result<Option<String>, Refusal> {
        let sha = match base {
            Base::WithText { sha, text } => {
                if Sha::of(text.as_bytes()) != *sha {
                    return Err(invalid(format!("the base text given for {p} isn't the one its hash says")));
                }
                return Ok(Some(text.to_string()));
            }
            Base::Sha(sha) => *sha,
            Base::Absent => return Ok(Some(String::new())),
        };
        if let Some(t) = self.texts.get(&sha) {
            return Ok(Some(t.to_string()));
        }
        let path = p.path();
        let revisions = std::iter::once(&self.accepted.snapshot).chain(self.past.iter().map(|x| &x.snapshot));
        Ok(revisions.filter_map(|s| s.files.get(&path)).find(|t| Sha::of(t.as_bytes()) == sha).map(|t| t.to_string()))
    }

    /// Step 5: the bases, and each op applied to the current content.
    fn plan(&self, tx: &Tx, currents: &BTreeMap<TxPath, Current>, decls: &m2_song::Decls) -> Result<Planned, Refusal> {
        let mut conflicts: Vec<ConflictFile> = vec![];
        let mut planned = Planned {
            news: vec![],
            coords: BTreeMap::new(),
            bases: BTreeMap::new(),
            edits: BTreeMap::new(),
            targets: vec![],
            inverse: vec![],
            inverse_base: BTreeMap::new(),
            patches: vec![],
            base_shas: BTreeMap::new(),
            seeds: vec![],
        };
        let changed = |p: &TxPath, c: &Current, why: Option<String>| ConflictFile { file: p.to_string(), current: c.sha, clip: None, why };
        // Recovery puts back recorded bytes whatever the draft says (6.5, P3): when the
        // interrupted commit itself left the code broken, everything waits, and Finish and
        // Discard must still apply.
        let recovery = matches!(tx.expect, Expect::Journal(_));
        for (p, b) in &tx.base {
            if !recovery && self.waits(p) {
                conflicts.push(changed(p, &currents[p], Some("changed on disk together with code that doesn't compile yet".into())));
            }
            if p.is_text()
                && let Some(c) = &currents[p].text
                && b.sha() == currents[p].sha
            {
                planned.bases.insert(p.to_string(), c.clone());
                planned.base_shas.extend(currents[p].sha.map(|sha| (p.to_string(), sha)));
            }
        }
        if !conflicts.is_empty() {
            return Err(Refusal::Conflict { files: conflicts });
        }
        let payload = |bytes: &Payload| -> Result<Arc<[u8]>, Refusal> {
            match bytes {
                Payload::Text(t) => Ok(Arc::from(t.as_bytes())),
                Payload::Bytes(b) => Ok(b.clone()),
                Payload::Local { path, sha } => journal::local(&self.dir, path, sha).map_err(invalid),
            }
        };
        for op in &tx.ops {
            match op {
                Op::TextEdit { file, edits } => {
                    let cur = &currents[file];
                    let base = &tx.base[file];
                    let Some(current) = cur.text.clone() else {
                        conflicts.push(changed(file, cur, Some("isn't there to edit".into())));
                        continue;
                    };
                    let text = if base.sha() == cur.sha { Some(current.clone()) } else { self.base_text(file, base)? };
                    let Some(text) = text else {
                        conflicts.push(changed(file, cur, None));
                        continue;
                    };
                    planned.bases.insert(file.to_string(), text.clone());
                    let mut resolved = vec![];
                    for e in edits {
                        let r = expect::bytes(&text, &m2_song::Range { start: e.start, end: e.end })
                            .map_err(|w| invalid(format!("{file}: {w}")))?;
                        if text[r.clone()] != e.expected {
                            return Err(rejected(vec![
                                Diagnostic::error(
                                    "tx.expected",
                                    format!(
                                        "{file} {}..{} says `{}`, not the `{}` the edit expects",
                                        r.start,
                                        r.end,
                                        &text[r.clone()],
                                        e.expected
                                    ),
                                )
                                .file(file),
                            ]));
                        }
                        resolved.push((r, e.text.clone()));
                    }
                    resolved.sort_by_key(|(r, _)| (r.start, r.end));
                    if resolved.windows(2).any(|w| w[0].0.end > w[1].0.start || (w[0].0 == w[1].0 && w[0].0.is_empty())) {
                        return Err(invalid(format!("the edits of {file} overlap, or two insert at one place")));
                    }
                    // Carried from the base text to the current one: strict (WRITE-12).
                    let outside = Edits::between(&text, &current);
                    let mut carried = vec![];
                    for (r, t) in &resolved {
                        match outside.carry(r.clone()) {
                            Some(to) => carried.push((to.start, to.end, t.clone())),
                            None => conflicts.push(changed(file, cur, Some("changed on disk where the edit is".into()))),
                        }
                    }
                    if carried.len() != resolved.len() {
                        continue;
                    }
                    let (own, new) = Edits::of(&current, &carried);
                    planned.edits.insert(file.to_string(), resolved.iter().map(|(r, _)| r.clone()).collect());
                    // The inverse puts back each edit's current text, at its new place.
                    let inverse: Vec<Edit> = own
                        .0
                        .iter()
                        .map(|h| Edit {
                            start: Pos::Byte(h.new.start as u32),
                            end: Pos::Byte(h.new.end as u32),
                            expected: new[h.new.clone()].to_string(),
                            text: current[h.old.clone()].to_string(),
                        })
                        .collect();
                    for h in &own.0 {
                        planned.patches.push(Patch {
                            file: file.to_string(),
                            start: h.old.start as u32,
                            end: h.old.end as u32,
                            text: new[h.new.clone()].to_string(),
                            from: line_col(&current, h.old.start),
                            to: line_col(&current, h.old.end),
                        });
                    }
                    planned.inverse.push(Op::TextEdit { file: file.clone(), edits: inverse });
                    planned.inverse_base.insert(file.clone(), Base::text(&new));
                    let own_base = resolved.iter().map(|(r, _)| Some(r.clone())).collect();
                    planned.coords.insert(file.to_string(), Coords::tx(outside, own, own_base));
                    planned.news.push((file.clone(), New::Bytes(Arc::from(new.into_bytes()), None)));
                }
                Op::Write { file, bytes } => {
                    let cur = &currents[file];
                    if tx.base[file].sha() != cur.sha {
                        conflicts.push(changed(file, cur, None));
                        continue;
                    }
                    let bytes = payload(bytes)?;
                    if let (Some(old), Ok(new)) = (cur.text.as_deref().or(cur.sha.is_none().then_some("")), std::str::from_utf8(&bytes))
                        && matches!(file.class(), Class::Code)
                    {
                        let own = Edits::between(old, new);
                        let own_base = own.0.iter().map(|h| Some(h.old.clone())).collect();
                        planned.coords.insert(file.to_string(), Coords::tx(Edits::default(), own, own_base));
                        if cur.sha.is_some() {
                            planned.patches.push(Patch {
                                file: file.to_string(),
                                start: 0,
                                end: old.len() as u32,
                                text: new.to_string(),
                                from: (0, 0),
                                to: line_col(old, old.len()),
                            });
                        }
                    }
                    let after = Sha::of(&bytes);
                    match &cur.bytes {
                        Some(old) => {
                            planned
                                .inverse
                                .push(Op::Write { file: file.clone(), bytes: before_payload(file, cur.sha.expect("there"), old) });
                        }
                        None => planned.inverse.push(Op::Remove { file: file.clone() }),
                    }
                    planned.inverse_base.insert(file.clone(), Base::Sha(after));
                    planned.news.push((file.clone(), New::Bytes(bytes, Some(after))));
                }
                Op::Remove { file } => {
                    let cur = &currents[file];
                    match (tx.base[file].sha(), &cur.bytes) {
                        (Some(b), Some(old)) if Some(b) == cur.sha => {
                            planned.inverse.push(Op::Write { file: file.clone(), bytes: before_payload(file, b, old) });
                            planned.inverse_base.insert(file.clone(), Base::Absent);
                            planned.news.push((file.clone(), New::Gone));
                        }
                        _ => conflicts.push(changed(file, cur, None)),
                    }
                }
                Op::Rename { from, to } => {
                    let (c, t) = (&currents[from], &currents[to]);
                    if tx.base[from].sha() != c.sha || c.sha.is_none() {
                        conflicts.push(changed(from, c, None));
                        continue;
                    }
                    if t.sha.is_some() || !t.folder.is_empty() {
                        conflicts.push(changed(to, t, Some("is there already, and a rename never replaces anything".into())));
                        continue;
                    }
                    planned.inverse.push(Op::Rename { from: to.clone(), to: from.clone() });
                    planned.inverse_base.insert(to.clone(), Base::Sha(c.sha.expect("there")));
                    planned.inverse_base.insert(from.clone(), Base::Absent);
                    planned.news.push((from.clone(), New::Gone));
                    planned.news.push((to.clone(), New::From(from.clone())));
                }
                Op::Clips { lane, put, delete } => {
                    let file = TxPath::lane(lane);
                    let cur = &currents[&file];
                    let Some(kind) = decls.lanes.iter().find(|(a, _)| a == lane).map(|(_, k)| k.clone()) else {
                        return Err(rejected(vec![Diagnostic::error(
                            "tx.invalid",
                            format!("{} › {} isn't a lane the accepted code declares, so its kind isn't known", lane.module, lane.lane),
                        )]));
                    };
                    // A text the accepted revision read as this lane is that read, shared: the file
                    // is almost always the accepted one (PLANE-1).
                    let read = |text: Option<&str>| -> Result<Arc<LaneData>, String> {
                        match text {
                            None => Ok(Arc::new(LaneData::empty(&kind))),
                            Some(t) => match self.accepted.compiled.data.parsed(lane, &kind, t) {
                                Some(d) => Ok(d.clone()),
                                None => LaneData::read(t, &kind).map(Arc::new).map_err(|e| match e {
                                    m2_song::LaneError::Invalid(w) => w,
                                    m2_song::LaneError::Unit { declared, .. } => format!("it isn't automation in `{declared}`"),
                                }),
                            },
                        }
                    };
                    let now = match read(cur.text.as_deref()) {
                        Ok(d) => d,
                        Err(why) => {
                            return Err(rejected(vec![
                                Diagnostic::error(
                                    "lane.invalid",
                                    format!("{file} doesn't read as its lane ({why}), so it isn't written over: fix the file first"),
                                )
                                .file(&file),
                            ]));
                        }
                    };
                    let base = &tx.base[&file];
                    let base_text = match base {
                        Base::Absent => Some(None),
                        // The file is its base: the base reads as the file does.
                        _ if base.sha() == cur.sha => None,
                        _ => match self.base_text(&file, base)? {
                            Some(t) => Some(Some(t)),
                            None => {
                                conflicts.push(changed(&file, cur, None));
                                continue;
                            }
                        },
                    };
                    let was = match base_text {
                        None => now.clone(),
                        Some(t) => match read(t.as_deref()) {
                            Ok(d) => d,
                            Err(_) => {
                                conflicts.push(changed(&file, cur, Some("its base doesn't read as the lane".into())));
                                continue;
                            }
                        },
                    };
                    // Every clip the op touches is the same in the base and the file now, byte for
                    // byte, or absent from both (WRITE-3); a clip whose id is there twice is a guess.
                    // A base that is the file now is the same read: only an id twice can be wrong.
                    let same = Arc::ptr_eq(&was, &now);
                    let ids: Vec<&str> =
                        put.iter().filter_map(|c| c.get("id").and_then(|i| i.as_str())).chain(delete.iter().map(String::as_str)).collect();
                    let mut bad = false;
                    for id in &ids {
                        let twice = Some("is in the lane twice; renumber its clips first");
                        let why = if same {
                            if now.clips_with(id) > 1 { twice } else { None }
                        } else {
                            let (a, b) = (was.lines_of(id), now.lines_of(id));
                            if a.len() > 1 || b.len() > 1 {
                                twice
                            } else if a != b {
                                Some("changed since the edit was planned")
                            } else {
                                None
                            }
                        };
                        if let Some(why) = why {
                            conflicts.push(ConflictFile {
                                file: file.to_string(),
                                current: cur.sha,
                                clip: Some(id.to_string()),
                                why: Some(why.into()),
                            });
                            bad = true;
                        }
                    }
                    if bad {
                        continue;
                    }
                    let mut next = (*now).clone();
                    next.put_clips(put, delete)
                        .map_err(|why| rejected(vec![Diagnostic::error("lane.invalid", format!("{file}: {why}")).file(&file)]))?;
                    let new: Arc<str> = m2_song::canonical(&next).into();
                    let after = Sha::of(new.as_bytes());
                    if let Some(text) = &cur.text {
                        planned.bases.insert(file.to_string(), text.clone());
                        planned.base_shas.extend(cur.sha.map(|sha| (file.to_string(), sha)));
                    }
                    match &cur.bytes {
                        Some(_) => {
                            // The inverse puts back each touched clip as it was, and deletes the
                            // ones the op added.
                            let mut back = vec![];
                            let mut gone = vec![];
                            for id in &ids {
                                match now.clip_value(id) {
                                    Some(v) => back.push(v),
                                    None if next.clips_with(id) == 1 => gone.push(id.to_string()),
                                    None => {}
                                }
                            }
                            planned.inverse.push(Op::Clips { lane: lane.clone(), put: back, delete: gone });
                            planned.inverse_base.insert(file.clone(), Base::WithText { sha: after, text: new.clone() });
                        }
                        None => {
                            planned.inverse.push(Op::Remove { file: file.clone() });
                            planned.inverse_base.insert(file.clone(), Base::Sha(after));
                        }
                    }
                    // What the candidate reads there is `next`, where its canonical text reads back
                    // as it bit for bit (FMT-8a): else the text is parsed, as any file is.
                    if next.reads_back(&kind) {
                        debug_assert_eq!(
                            format!("{:?}", LaneData::read(&new, &kind)),
                            format!("{:?}", Ok::<_, m2_song::LaneError>(&next)),
                            "the seed is what a read of its text gives"
                        );
                        planned.seeds.push(Seed { addr: lane.clone(), kind: kind.clone(), text: new.clone(), data: Arc::new(next) });
                    }
                    planned.news.push((file.clone(), New::Bytes(Arc::from(new.as_bytes()), Some(after))));
                }
            }
        }
        if !conflicts.is_empty() {
            return Err(Refusal::Conflict { files: conflicts });
        }
        // What commit writes, each target once, unchanged files left alone.
        for (p, new) in &planned.news {
            let cur = &currents[p];
            let before = |c: &Current| c.bytes.clone().map(|bytes| Before { sha: c.sha.expect("bytes have a hash"), bytes });
            match new {
                New::Bytes(b, sha) => {
                    if cur.bytes.as_deref() != Some(&**b) {
                        planned.targets.push(Target::Write {
                            path: p.to_string(),
                            before: before(cur),
                            bytes: b.clone(),
                            after: sha.unwrap_or_else(|| Sha::of(b)),
                        });
                    }
                }
                New::Gone if planned.news.iter().any(|(_, n)| matches!(n, New::From(f) if f == p)) => {}
                New::Gone => {
                    planned.targets.push(Target::Remove { path: p.to_string(), before: before(cur).expect("checked there") });
                }
                New::From(from) => {
                    let c = &currents[from];
                    let folder = matches!(from.data(), DataPath::Module { .. });
                    planned.targets.push(Target::Rename {
                        from: from.to_string(),
                        to: p.to_string(),
                        hash: c.sha.expect("checked"),
                        dir: folder,
                    });
                }
            }
        }
        planned.inverse.reverse();
        Ok(planned)
    }

    /// Steps 5 to 9 and 11: the bases, the baseline, the candidate, the delta and the diagnostics.
    fn prepare(&mut self, tx: &Tx, currents: &BTreeMap<TxPath, Current>) -> Result<Prepared, Refusal> {
        let accepted = self.accepted.clone();
        let planned = self.plan(tx, currents, &accepted.compiled.decls)?;

        // 6. The baseline: the accepted revision with each touched file as it is on disk.
        let differs: Vec<&TxPath> = currents
            .iter()
            .filter(|(p, c)| {
                if p.class() == Class::Presentation {
                    return false;
                }
                // A text file the accepted revision has: its bytes are compared, not hashed.
                let path = p.path();
                if let (Some(text), Some(bytes), false, false) = (
                    accepted.snapshot.files.get(&path),
                    &c.bytes,
                    accepted.snapshot.not_utf8.contains_key(&path),
                    accepted.snapshot.binaries.contains_key(&path) || matches!(p.data(), DataPath::Module { .. }),
                ) {
                    return text.as_bytes() != &**bytes;
                }
                match (known(&accepted.snapshot, p), c.sha) {
                    (None, None) => false,
                    // An asset only listed: its name is all a revision knows.
                    (Some(None), Some(_)) => false,
                    (Some(Some(a)), Some(b)) => a != b,
                    _ => true,
                }
            })
            .map(|(p, _)| p)
            .collect();
        let baseline = if differs.is_empty() {
            accepted.clone()
        } else {
            let mut snap = accepted.snapshot.clone();
            for p in &differs {
                let c = &currents[*p];
                if matches!(p.data(), DataPath::Module { .. }) {
                    remove_under(&mut snap, &p.path());
                    for (f, bytes) in &c.folder {
                        put_file(&mut snap, f, Some(bytes));
                    }
                } else {
                    put_file(&mut snap, &p.path(), c.bytes.as_deref());
                }
            }
            settle_assets(&mut snap);
            let code = differs.iter().any(|p| matches!(p.class(), Class::Code | Class::Manifest));
            match self.build(&snap, (!code).then(|| accepted.compiled.code().clone()), &accepted.compiled.data) {
                Ok((project, compiled)) => Arc::new(Revision {
                    number: accepted.number,
                    code_rev: accepted.code_rev,
                    data_rev: accepted.data_rev,
                    snapshot: snap,
                    project,
                    compiled,
                }),
                Err(_) => {
                    let files = differs
                        .iter()
                        .map(|p| ConflictFile {
                            file: p.to_string(),
                            current: currents[*p].sha,
                            clip: None,
                            why: Some("changed on disk, and doesn't compile yet".into()),
                        })
                        .collect();
                    return Err(Refusal::Conflict { files });
                }
            }
        };

        // 7. The candidate: the baseline with every op applied.
        let mut snap = baseline.snapshot.clone();
        apply_news(&mut snap, &planned.news);
        settle_assets(&mut snap);

        // 8. Built by the plane of the touched paths: no QuickJS for data alone (WRITE-4). A lane
        // the plan made is read as made, not parsed again (PLANE-1).
        let code = planned.news.iter().any(|(p, _)| matches!(p.class(), Class::Code | Class::Manifest));
        let seeded;
        let previous = match planned.seeds.as_slice() {
            [] => &baseline.compiled.data,
            seeds => {
                seeded = seeds.iter().fold(baseline.compiled.data.clone(), |d, s| d.seeded(&s.addr, &s.kind, s.text.clone(), s.data.clone()));
                &seeded
            }
        };
        let (project, compiled) =
            self.build(&snap, (!code).then(|| baseline.compiled.code().clone()), previous).map_err(|d| rejected(built(d)))?;
        // One that plays the baseline's code says so: a host that hears it before it is accepted
        // (PLANE-5) never takes it for a code change.
        let (number, code_rev) = if code { (0, 0) } else { (baseline.number, baseline.code_rev) };
        let candidate = Arc::new(Revision { number, code_rev, data_rev: 0, snapshot: snap, project, compiled });

        // 9. The delta, named in each touched file's base coordinates.
        let (delta, hash, graphs) = {
            let old =
                Side { compiled: &baseline.compiled, snapshot: &baseline.snapshot, at: Positions { coords: &planned.coords, new: false } };
            let new =
                Side { compiled: &candidate.compiled, snapshot: &candidate.snapshot, at: Positions { coords: &planned.coords, new: true } };
            let (delta, a, b) = delta::compare(&old, &new);
            let hash = delta.hash();
            (delta, hash, (a, b))
        };

        // 11. What the candidate needs attention for that the baseline doesn't, then claims.
        let mut diagnostics: Vec<Diagnostic> =
            candidate.compiled.problems.iter().filter(|d| !baseline.compiled.problems.iter().any(|b| same(b, d))).cloned().collect();
        diagnostics.extend(claimed(&baseline, &candidate));
        Ok(Prepared { key: None, baseline, candidate, planned, delta, hash, diagnostics, graphs })
    }

    /// Step 10: the expectation, on the delta (WRITE-6).
    fn judge(&self, tx: &Tx, p: &mut Prepared) -> Result<(), Refusal> {
        let (a, b) = &p.graphs;
        // The files the Tx's own ops write (C1): an edit writes its file as a whole write does.
        let mut written: BTreeSet<String> = BTreeSet::new();
        for op in &tx.ops {
            match op {
                Op::Write { file, .. } | Op::Remove { file } | Op::TextEdit { file, .. } => {
                    written.insert(file.to_string());
                }
                Op::Rename { from, to } => {
                    written.insert(from.to_string());
                    written.insert(to.to_string());
                }
                _ => {}
            }
        }
        let j = expect::Judge {
            delta: &p.delta,
            hash: &p.hash,
            old: a,
            new: b,
            coords: &p.planned.coords,
            bases: &p.planned.bases,
            edits: &p.planned.edits,
            written: &written,
        };
        expect::judge(&tx.expect, &j).map_err(|why| Refusal::Rejected {
            diagnostics: vec![Diagnostic::error("tx.unexpected", format!("the edit was refused: {why}"))],
            unexpected: Some(Box::new(p.delta.clone())),
        })
    }

    /// Build a snapshot: its data read against `code` when given (no QuickJS), else its code built
    /// too.
    pub(crate) fn build(
        &mut self,
        snap: &Snapshot,
        code: Option<Arc<Code>>,
        previous: &m2_song::DataSet,
    ) -> Result<(Project, Compiled), Vec<m2_ir::Diagnostic>> {
        let project = Project::from_snapshot(snap).map_err(|d| d.into_iter().map(crate::diagnostic).collect::<Vec<_>>())?;
        let code = match code {
            Some(c) => c,
            None => crate::build_code(&project, &mut self.packages)?,
        };
        let compiled = crate::read_data(&code, &project, &self.seen, Some(previous));
        Ok((project, compiled))
    }

    /// Steps 12 to 14: journal, stage and rename the prepared writes, then accept the candidate.
    fn write_prepared(&mut self, tx: &Tx, p: Prepared) -> Result<(Committed, Option<TxStep>), Refusal> {
        let Prepared { baseline, candidate, planned, delta, hash, diagnostics, .. } = p;
        if planned.targets.is_empty() {
            // Nothing to write: no revision and no undo step.
            let a = &self.accepted;
            return Ok((
                Committed {
                    number: a.number,
                    code_rev: a.code_rev,
                    data_rev: a.data_rev,
                    delta: GraphDelta::default(),
                    hash: GraphDelta::default().hash(),
                    diagnostics,
                    patches: vec![],
                    wrote: vec![],
                },
                None,
            ));
        }
        let wrote = {
            let writing = self.before_write.is_some().then(|| self.writing(&candidate));
            self.stage(tx, &planned.targets, vec![], writing.as_ref())?
        };
        // The draft follows the files this commit wrote; nothing else of it changes.
        let mut draft = self.draft.clone();
        for (path, _) in &planned.news {
            remove_under(&mut draft, &path.path());
        }
        for (path, _) in &planned.news {
            let at = path.path();
            for (k, v) in m2_song::under(&candidate.snapshot.files, &at) {
                draft.files.insert(k.clone(), v.clone());
            }
            let bins: Vec<(PathBuf, Stored)> = m2_song::under(&candidate.snapshot.binaries, &at).map(|(k, v)| (k.clone(), v.clone())).collect();
            if !bins.is_empty() {
                Arc::make_mut(&mut draft.binaries).extend(bins);
            }
            for (k, v) in m2_song::under(&candidate.snapshot.not_utf8, &at) {
                draft.not_utf8.insert(k.clone(), *v);
            }
        }
        settle_assets(&mut draft);
        for (file, text) in &planned.bases {
            match planned.base_shas.get(file) {
                Some(sha) => self.texts.put_hashed(*sha, text),
                None => self.texts.put(text),
            };
        }
        let step = self.inverse_step(tx, &planned, self.accepted.number + 1, Some(&baseline));
        // Only layout.json written: no revision (PLANE-3), though it is undone like any Tx.
        let number = if changed_paths(&self.accepted.snapshot, &candidate.snapshot).is_empty() {
            self.accepted.number
        } else {
            let Revision { snapshot, project, compiled, .. } = Arc::try_unwrap(candidate).unwrap_or_else(|c| c.duplicate());
            // The hashes of the texts written, which the store is keyed by: made already.
            let known = planned.targets.iter().filter_map(|t| match t {
                Target::Write { path, after, .. } => Some((PathBuf::from(path), *after)),
                _ => None,
            });
            self.accept_known(snapshot, project, compiled, &known.collect())
        };
        self.draft = draft;
        self.cache = None;
        let a = &self.accepted;
        Ok((
            Committed { number, code_rev: a.code_rev, data_rev: a.data_rev, delta, hash, diagnostics, patches: planned.patches, wrote },
            Some(step),
        ))
    }

    /// The undo step of a Tx just committed as revision `number`: its inverse, judged as R22 says.
    fn inverse_step(&self, tx: &Tx, planned: &Planned, number: u64, baseline: Option<&Revision>) -> TxStep {
        let data_only = tx.touched().iter().all(|p| p.class() == Class::Data);
        let raw = matches!(tx.expect, Expect::Seen(_) | Expect::Journal(_));
        let expect = match &tx.expect {
            Expect::Identical => Expect::Identical,
            Expect::Values(values) => Expect::Values(values.iter().filter_map(|(a, _)| inverse_anchor(a, planned, baseline?)).collect()),
            Expect::Confined(c) => Expect::Confined(Confined {
                ranges: c.ranges.iter().map(|(f, d)| (f.clone(), map_declared(d, f, planned))).collect(),
                lanes: c.lanes.clone(),
                modules: c.modules.iter().map(invert).collect(),
            }),
            Expect::Seen(_) | Expect::Journal(_) => Expect::Identical,
        };
        let mut inverse = Tx::new(&inverse_id(&tx.id, number), (tx.label.1.clone(), tx.label.0.clone()), planned.inverse.clone(), expect);
        inverse.base = planned.inverse_base.clone();
        // Shown where the change is now: a renamed file or folder by its new name.
        let file = inverse.touched().first().or(tx.touched().first()).map(|p| p.to_string()).unwrap_or_default();
        TxStep::new(inverse, raw, data_only, file)
    }

    /// What a commit to `candidate` tells the host before it is journaled (PLANE-5).
    fn writing(&self, candidate: &Arc<Revision>) -> Writing {
        let accepted = self.accepted.clone();
        let code = Arc::ptr_eq(candidate.compiled.code(), accepted.compiled.code()) && accepted.has_code();
        let data_only = code && changed_paths(&accepted.snapshot, &candidate.snapshot).iter().all(|p| kind_of(p) == Kind::Data);
        Writing { candidate: candidate.clone(), accepted, data_only }
    }

    /// What the host does with a commit once it passed every check and its targets still read as
    /// their bases, before it is journaled (PLANE-5): the app's engine hears a data commit there,
    /// while the session makes it durable. Called for each judged Tx (a builder's, an agent's, an
    /// undo's), never for a save, a recovery or a refused Tx; None takes it away.
    pub fn before_write(&mut self, hook: Option<BeforeWrite>) {
        self.before_write = hook;
    }

    /// Stage, journal and rename `targets` under the lock (WRITE-15). `tidy` names journals this
    /// commit finishes (a recovery's), removed once its renames are done; the host is told of
    /// `writing` between the pre-check and the journal (PLANE-5). Returns the paths written with
    /// the stamps of the session's own files, which [`Session::take_wrote`] also collects for the
    /// watcher.
    pub(crate) fn stage(
        &mut self,
        tx: &Tx,
        targets: &[Target],
        tidy: Vec<String>,
        writing: Option<&Writing>,
    ) -> Result<Vec<(PathBuf, Option<Stamp>)>, Refusal> {
        if let Some(r) = self.writable() {
            return Err(rejected(vec![r]));
        }
        let keep = self.blobs_in_use();
        let (host, host_id, program) = (self.host.clone(), self.host_id.clone(), self.program.clone());
        let writer = stage::Writer { host: &host, host_id: &host_id, program: &program };
        let dir = self.dir.clone();
        let Session { disk, pending, before_write, access, .. } = self;
        let mut checked = || {
            if let (Some(w), Some(hook)) = (writing, before_write.as_mut()) {
                hook(w);
                // The host took its time: the lock is checked again before anything is written
                // (WRITE-17), so a lock lost meanwhile is a commit that fails after the host heard it.
                if let crate::lock::Access::Writer(lock) = &*access
                    && !lock.holds(&dir)
                {
                    return Err(stage::Failed::Io("the project's lock was let go meanwhile (was .music2/ removed?)".into()));
                }
            }
            Ok(())
        };
        let done = stage::commit(&mut **disk, &dir, &tx.id, &tx.label, targets, &writer, pending, &tidy, &keep, &mut checked);
        match done {
            Ok(w) => {
                let wrote: Vec<(PathBuf, Option<Stamp>)> = w.stamps.into_iter().map(|(p, s)| (PathBuf::from(p), s)).collect();
                self.wrote.extend(wrote.iter().cloned());
                Ok(wrote)
            }
            Err(stage::Failed::Conflict(c)) => Err(Refusal::Conflict {
                files: c.into_iter().map(|c| ConflictFile { file: c.file, current: c.current, clip: None, why: None }).collect(),
            }),
            Err(stage::Failed::Io(why)) => {
                // Said with what wasn't saved: the disk's own words name only the call that failed.
                let files: Vec<&str> = targets.iter().flat_map(Target::paths).collect();
                Err(Refusal::Io { message: format!("{} couldn't be saved: {why}", files.join(", ")) })
            }
            Err(stage::Failed::Interrupted { journal, why }) => {
                // The rollback failed: the journal says what is half done, now (6a.6).
                self.rescan();
                Err(Refusal::Io {
                    message: format!("the commit failed and couldn't be undone ({why}); .music2/tx/{journal}.json records it"),
                })
            }
        }
    }

    /// Read the journals again: after a rollback that failed, its journal is reported at once
    /// (6a.6).
    pub(crate) fn rescan(&mut self) {
        self.scans += 1;
        // This session's own completed commits await its next barrier, not a scan's tidy.
        let scan = journal::scan(&self.dir, &self.host_id, self.access.writer(), &self.pending);
        self.own.retain(|d| !d.code.starts_with("tx."));
        self.own.extend(scan.problems);
        self.interrupted = scan.interrupted;
        self.kept = scan.keep;
        self.problems = self.gather_problems();
    }

    /// The before-images undo, interrupted journals and journals left alone still need.
    pub(crate) fn blobs_in_use(&self) -> BTreeSet<Sha> {
        let steps = self.history.iter().chain(&self.redo).flat_map(TxStep::blobs);
        steps.chain(self.interrupted.iter().flat_map(|j| j.blobs())).chain(self.kept.iter().copied()).collect()
    }

    /// Commit `tx` unjudged, and take what it wrote in like an outside write: recovery (whose
    /// result may not compile, P3), the undo of a save, and inverses of Tx committed on a seen
    /// delta.
    fn commit_raw(&mut self, tx: &Tx) -> Result<(Committed, Option<TxStep>), Refusal> {
        let currents = self.read_all(tx)?;
        let decls = self.accepted.compiled.decls.clone();
        let planned = self.plan(tx, &currents, &decls)?;
        let tidy = match &tx.expect {
            Expect::Journal(id) => vec![id.clone()],
            _ => vec![],
        };
        if planned.targets.is_empty() && tidy.is_empty() {
            let a = &self.accepted;
            let none = GraphDelta::default();
            return Ok((
                Committed {
                    number: a.number,
                    code_rev: a.code_rev,
                    data_rev: a.data_rev,
                    hash: none.hash(),
                    delta: none,
                    diagnostics: vec![],
                    patches: vec![],
                    wrote: vec![],
                },
                None,
            ));
        }
        let before = self.accepted.clone();
        let wrote = self.stage(tx, &planned.targets, tidy.clone(), None)?;
        if !tidy.is_empty() {
            // Every journal is judged again: one this recovery completed (an older commit whose
            // Finish this was) is tidied, and no fix stays offered that no longer applies.
            self.rescan();
        }
        let mut draft = self.draft.clone();
        apply_news(&mut draft, &planned.news);
        settle_assets(&mut draft);
        self.cache = None;
        self.take_in(draft);
        self.problems = self.gather_problems();
        let number = self.accepted.number;
        let step = self.inverse_step(tx, &planned, number, None);
        let step = TxStep { raw: true, ..step };
        let a = &self.accepted;
        let delta = if Arc::ptr_eq(&before, a) {
            GraphDelta::default()
        } else {
            let none = BTreeMap::new();
            let old = Side { compiled: &before.compiled, snapshot: &before.snapshot, at: Positions { coords: &none, new: false } };
            let new = Side { compiled: &a.compiled, snapshot: &a.snapshot, at: Positions { coords: &none, new: true } };
            delta::compare(&old, &new).0
        };
        Ok((
            Committed {
                number,
                code_rev: a.code_rev,
                data_rev: a.data_rev,
                hash: delta.hash(),
                delta,
                diagnostics: vec![],
                patches: planned.patches,
                wrote,
            },
            Some(step),
        ))
    }

    /// The expectation an undo step is judged by: a data-only inverse by the lanes its files
    /// feed in the accepted revision, worked out now (R22).
    pub(crate) fn step_tx(&self, step: &TxStep) -> Tx {
        let mut tx = step.inverse.clone();
        if step.data_only && !step.raw {
            let rev = &self.accepted;
            let touched = tx.touched();
            let lanes = rev
                .compiled
                .decls
                .lanes
                .iter()
                .filter(|(addr, _)| {
                    let file = DataPath::lane(addr).path();
                    touched.iter().any(|p| file.starts_with(p.path()))
                })
                .map(|(a, _)| a.clone())
                .collect();
            tx.expect = Expect::Confined(Confined { ranges: BTreeMap::new(), lanes, modules: vec![] });
        }
        tx
    }

    /// Whether undo step `step` would go ahead now, without compiling: writer access, the draft
    /// rule, no target waiting, and every base applies to the accepted revision (WRITE-21).
    pub(crate) fn can_tx(&self, step: &TxStep) -> bool {
        let tx = &step.inverse;
        if !self.access.writer() {
            return false;
        }
        let touched = tx.touched();
        if touched.iter().any(|p| matches!(p.class(), Class::Code | Class::Manifest)) && !self.draft_errors.is_empty() {
            return false;
        }
        if touched.iter().any(|p| self.waits(p)) {
            return false;
        }
        let snap = &self.accepted.snapshot;
        for op in &tx.ops {
            match op {
                Op::TextEdit { file, edits } => {
                    let (Some(Base::WithText { text, .. }), Some(now)) = (tx.base.get(file), snap.files.get(&file.path())) else {
                        return false;
                    };
                    let outside = Edits::between(text, now);
                    for e in edits {
                        let (Ok(s), Ok(end)) = (expect::resolve(text, &e.start), expect::resolve(text, &e.end)) else { return false };
                        match outside.carry(s..end) {
                            Some(r) if now.get(r.clone()) == Some(e.expected.as_str()) => {}
                            _ => return false,
                        }
                    }
                }
                Op::Clips { lane, put, delete } => {
                    let file = TxPath::lane(lane);
                    let Some(kind) = self.accepted.compiled.decls.lanes.iter().find(|(a, _)| a == lane).map(|(_, k)| k.clone()) else {
                        return false;
                    };
                    let (Some(Base::WithText { text, .. }), Some(now)) = (tx.base.get(&file), snap.files.get(&file.path())) else {
                        return false;
                    };
                    let ids = put.iter().filter_map(|c| c.get("id").and_then(|i| i.as_str())).chain(delete.iter().map(String::as_str));
                    // The base is the file now, which the accepted revision read as this lane: every
                    // clip is as it was, so only an id there twice stops it (no parse).
                    if (Arc::ptr_eq(text, now) || **text == **now)
                        && let Some(is) = self.accepted.compiled.data.parsed(lane, &kind, now)
                    {
                        if ids.into_iter().any(|id| is.clips_with(id) > 1) {
                            return false;
                        }
                        continue;
                    }
                    let (Ok(was), Ok(is)) = (LaneData::read(text, &kind), LaneData::read(now, &kind)) else { return false };
                    for id in ids {
                        let (a, b) = (was.lines_of(id), is.lines_of(id));
                        if a.len() > 1 || a != b {
                            return false;
                        }
                    }
                }
                _ => {
                    for p in op.paths() {
                        let want = tx.base.get(&p).and_then(Base::sha);
                        let have = match known(snap, &p) {
                            None => None,
                            Some(Some(s)) => Some(s),
                            Some(None) => return false,
                        };
                        if p.class() != Class::Presentation && want != have {
                            return false;
                        }
                    }
                }
            }
        }
        // A `Values` inverse is offered only while each number it writes back still sets its
        // value at the scale it had (`khz(0.9)` turned `hz(0.9)` would be undone to 2.5 Hz): the
        // design's 10.3 prediction, judged on the accepted revision's tables without compiling.
        if let Expect::Values(values) = &tx.expect {
            return values.iter().all(|(anchor, v)| self.predicts(tx, anchor, *v));
        }
        true
    }

    /// Whether the number `tx` writes at `anchor` (in its base text) gives `v` in the accepted
    /// revision: the constant written at the anchor's carried bytes, times its scale.
    fn predicts(&self, tx: &Tx, anchor: &Anchor, v: f64) -> bool {
        let Anchor::Written { file, range } = anchor else { return true };
        let (Some(Base::WithText { text, .. }), Some(now)) = (tx.base.get(file), self.accepted.snapshot.files.get(&file.path())) else {
            return false;
        };
        let Ok(r) = expect::bytes(text, range) else { return false };
        let written = tx.ops.iter().find_map(|op| match op {
            Op::TextEdit { file: f, edits } if f == file => edits.iter().find_map(|e| {
                let at = expect::resolve(text, &e.start).ok()?..expect::resolve(text, &e.end).ok()?;
                (at == r).then(|| js_number(&e.text)).flatten()
            }),
            _ => None,
        });
        let (Some(written), Some(to)) = (written, Edits::between(text, now).carry(r)) else { return false };
        let compiled = &self.accepted.compiled;
        let mut scale = None;
        for m in &compiled.ir.modules {
            each_const(&m.nodes, &mut |o| {
                let site = compiled.sites.iter().find(|x| x.id == o.site && x.file == file.as_str());
                if let Some(m2_script::sites::Arg::Number { start, end, .. }) = site.and_then(|x| x.arg(&o.arg))
                    && (*start as usize, *end as usize) == (to.start, to.end)
                {
                    scale = Some(o.scale.unwrap_or(1.0));
                }
            });
        }
        scale.is_some_and(|k| written * k == v)
    }
}

/// The value of a number as the code spells it (`0x320`, `8_00`, `0b11`, `.5`, `1e3`, `- 6`,
/// `(2)`): what an inverse writes back where a literal was. Rust's own parser knows only the
/// decimal spellings, so undo would not be offered for the rest.
fn js_number(text: &str) -> Option<f64> {
    let t = text.trim();
    if let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        return js_number(inner);
    }
    if let Some(rest) = t.strip_prefix('-') {
        return js_number(rest).map(|v| -v);
    }
    let radix = match t.get(..2) {
        Some("0x" | "0X") => 16,
        Some("0o" | "0O") => 8,
        Some("0b" | "0B") => 2,
        _ => 10,
    };
    // A separator only ever stands between two digits.
    let body = if radix == 10 { t } else { &t[2..] };
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    let digits = body.replace('_', "");
    if radix == 10 {
        let number = digits.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'));
        return number.then(|| digits.parse::<f64>().ok()).flatten();
    }
    match u128::from_str_radix(&digits, radix) {
        // Rounded to the nearest double, as JavaScript rounds a literal.
        Ok(v) => Some(v as f64),
        Err(_) => digits.chars().try_fold(0f64, |v, c| Some(v * radix as f64 + c.to_digit(radix)? as f64)),
    }
}

/// The origin of every constant of `nodes`, voices included.
fn each_const(nodes: &[m2_ir::Node], f: &mut impl FnMut(&m2_ir::Origin)) {
    for n in nodes {
        if let (m2_ir::Op::Const { .. }, Some(o)) = (&n.op, &n.origin) {
            f(o);
        }
        if let m2_ir::Op::Poly { body, .. } = &n.op {
            each_const(&body.nodes, f);
        }
    }
}

/// A text file's or a binary's before bytes, as an inverse writes them back: inline text, or the
/// before-image the commit keeps in `blobs/`.
fn before_payload(file: &TxPath, sha: Sha, bytes: &Arc<[u8]>) -> Payload {
    match std::str::from_utf8(bytes) {
        Ok(t) if file.is_text() => Payload::Text(t.into()),
        _ => Payload::Local { path: format!(".music2/tx/blobs/{}", sha.hex()), sha },
    }
}

/// A `Values` anchor of a committed Tx, where its value is now, with the value it had in the
/// baseline.
fn inverse_anchor(a: &Anchor, planned: &Planned, baseline: &Revision) -> Option<(Anchor, f64)> {
    let (file, range) = match a {
        Anchor::Written { file, range } | Anchor::Slot { site: Spot { file, range }, .. } => (file, range),
    };
    let text = planned.bases.get(file.as_str())?;
    let r = expect::bytes(text, range).ok()?;
    let c = planned.coords.get(file.as_str())?;
    // Where the anchor's bytes are now: an edit that replaced exactly them, else mapped.
    let now = match c.own_base.iter().position(|b| b.as_ref() == Some(&r)) {
        Some(i) if matches!(a, Anchor::Written { .. }) => c.own.0[i].new.clone(),
        _ => {
            let s = c.own.map(c.outside.map(r.start, Bias::Left), Bias::Left);
            let e = c.own.map(c.outside.map(r.end, Bias::Right), Bias::Right);
            s..e
        }
    };
    // Its value in the baseline: the constants it named there.
    let side = Side { compiled: &baseline.compiled, snapshot: &baseline.snapshot, at: Positions { coords: &planned.coords, new: false } };
    let g = side.graph();
    let exact = |at: &delta::At| match (at.start, at.end) {
        (crate::text::Point::Base(x), crate::text::Point::Base(y)) => Some(x..y + 1),
        _ => None,
    };
    let old = g.nodes.values().find_map(|f| {
        let here = match a {
            Anchor::Written { .. } => f.written.as_ref().is_some_and(|at| at.file == file.as_str() && exact(at) == Some(r.clone())),
            Anchor::Slot { slot, .. } => {
                f.slot.as_ref().is_some_and(|(at, s)| s == slot && at.file == file.as_str() && exact(at) == Some(r.clone()))
            }
        };
        if here { f.value } else { None }
    })?;
    let range = m2_song::Range::bytes(now.start as u32, now.end as u32);
    Some(match a {
        Anchor::Written { file, .. } => (Anchor::Written { file: file.clone(), range }, old),
        Anchor::Slot { slot, .. } => (Anchor::Slot { site: Spot { file: file.clone(), range }, slot: slot.clone() }, old),
    })
}

/// A declared range of a committed Tx, where it is in the text the Tx left: its ends taking in
/// the Tx's own insertions at them.
fn map_declared(d: &Declared, f: &TxPath, planned: &Planned) -> Declared {
    let Declared::Ranges(rs) = d else { return Declared::Whole };
    let (Some(text), Some(c)) = (planned.bases.get(f.as_str()), planned.coords.get(f.as_str())) else { return d.clone() };
    Declared::Ranges(
        rs.iter()
            .filter_map(|r| expect::bytes(text, r).ok())
            .map(|r| {
                let s = c.own.map(c.outside.map(r.start, Bias::Left), Bias::Left);
                let e = c.own.map(c.outside.map(r.end, Bias::Right), Bias::Right);
                m2_song::Range::bytes(s as u32, e as u32)
            })
            .collect(),
    )
}

fn invert(m: &ModuleChange) -> ModuleChange {
    match m {
        ModuleChange::Added(k) => ModuleChange::Removed(k.clone()),
        ModuleChange::Removed(k) => ModuleChange::Added(k.clone()),
        ModuleChange::Rekeyed { from, to } => ModuleChange::Rekeyed { from: to.clone(), to: from.clone() },
        ModuleChange::Muted { module, muted } => ModuleChange::Muted { module: module.clone(), muted: !muted },
    }
}
