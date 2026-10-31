//! Undo that survives closing (WRITE-23 to WRITE-26): the session's undo history, kept in
//! `.music2/undo/` as each change to it lands, and read back when the song next opens for writing.
//!
//! `index.json` holds the two stacks, undo's and redo's, oldest first. Each step names its record:
//! its inverse Tx's JSON, in a file named by the SHA-256 of its bytes, written once. A record is
//! written to a temporary name, pushed and renamed; then the index, the same way; then the records
//! no step names go. So a crash can lose the newest steps, never make one lie: what is read back
//! is checked against the files (WRITE-24) and each record against its name (WRITE-25).
//!
//! The history knows nothing of what a Tx means: a step is its inverse, what it is called, who made
//! it, and the hash each file it touches held before its change (what its undo puts back).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use m2_song::tx::{Op, Payload, Tx, TxPath};
use m2_song::{Sha, path::LOCAL};
use serde_json::{Map, Value, json};

use crate::session::Source;
use crate::tx::TxStep;

/// The index's format: one of another version is read as one that can't be read.
const VERSION: u64 = 1;

/// How much undo keeps (WRITE-26): the steps, the bytes of text and data their inverses hold, and
/// the bytes of the before-images they name in `.music2/tx/blobs/`. Past any of them the oldest
/// steps go first, never the newest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bound {
    pub steps: usize,
    pub text: usize,
    pub images: u64,
}

/// 500 steps, 64 MiB of text and data, 256 MiB of before-images.
pub const BOUND: Bound = Bound { steps: 500, text: 64 << 20, images: 256 << 20 };

/// Why undo, or redo, goes back no further than its oldest step (WRITE-24, WRITE-25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// A file a step read back touches doesn't hold what the step left: a change made outside
    /// music2, or one the history doesn't know.
    Outside,
    /// The history kept on disk, or a step's record, couldn't be read.
    Unread,
}

impl End {
    /// As the app's state says it: `outside` or `unread`.
    pub fn as_str(&self) -> &'static str {
        match self {
            End::Outside => "outside",
            End::Unread => "unread",
        }
    }

    fn parse(v: &Value) -> Option<End> {
        match v.as_str()? {
            "outside" => Some(End::Outside),
            "unread" => Some(End::Unread),
            _ => None,
        }
    }
}

/// `.music2/undo/` of the project at `dir`.
pub(crate) fn folder(dir: &Path) -> PathBuf {
    dir.join(LOCAL).join("undo")
}

fn index_file(dir: &Path) -> PathBuf {
    folder(dir).join("index.json")
}

fn record_file(dir: &Path, sha: &Sha) -> PathBuf {
    folder(dir).join(format!("{}.json", sha.hex()))
}

/// The records this session knows are in `.music2/undo/`, by name: written once, and gone once no
/// step names them.
#[derive(Debug, Default)]
pub(crate) struct Records(BTreeSet<Sha>);

impl Records {
    /// Forget them all: `.music2/` was taken away, so every record is written again.
    pub fn forget(&mut self) {
        self.0.clear();
    }
}

/// The history as read back, before it is checked against the files.
pub(crate) struct Index {
    /// The id the last step got.
    pub next: u64,
    /// Each step the index names, oldest first: None where its record is missing, or isn't its
    /// bytes, or doesn't read.
    pub undo: Vec<Option<TxStep>>,
    pub redo: Vec<Option<TxStep>>,
    /// Why each stack ended when the history was written.
    pub ends: (Option<End>, Option<End>),
    /// The records in the folder.
    pub records: Records,
}

/// Read the history of the project at `dir`: None when none was kept, Err with why when the index
/// can't be read (WRITE-25).
pub(crate) fn read(dir: &Path) -> Result<Option<Index>, String> {
    let bytes = match std::fs::read(index_file(dir)) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("its index isn't JSON ({e})"))?;
    if v.get("version").and_then(Value::as_u64) != Some(VERSION) {
        return Err("its index is of another version of music2".into());
    }
    let records = Records(listed(dir));
    let stack = |key: &str| -> Result<Vec<Option<TxStep>>, String> {
        let Some(steps) = v.get(key).and_then(Value::as_array) else { return Err(format!("its index has no `{key}`")) };
        steps.iter().map(|s| entry(dir, s, &records).ok_or_else(|| format!("a step of `{key}` doesn't read"))).collect()
    };
    let (undo, redo) = (stack("undo")?, stack("redo")?);
    let next = v.get("next").and_then(Value::as_u64).ok_or("its index has no `next`")?;
    let ends = v.get("ends").map_or((None, None), |e| (e.get("undo").and_then(End::parse), e.get("redo").and_then(End::parse)));
    Ok(Some(Index { next, undo, redo, ends, records }))
}

/// The records in `.music2/undo/`.
fn listed(dir: &Path) -> BTreeSet<Sha> {
    let Ok(entries) = std::fs::read_dir(folder(dir)) else { return BTreeSet::new() };
    entries.flatten().filter_map(|e| Sha::parse(e.file_name().to_str()?.strip_suffix(".json")?)).collect()
}

/// A step of the index, with its record: None when the entry itself doesn't read (the index is
/// then unread as a whole), `Some(None)` when its record is missing or isn't its bytes.
fn entry(dir: &Path, s: &Value, records: &Records) -> Option<Option<TxStep>> {
    let record = Sha::parse(s.get("record")?.as_str()?)?;
    let id = s.get("id")?.as_u64()?;
    let source = match (s.get("source")?.as_str()?, s.get("by").and_then(Value::as_str)) {
        ("page", _) => Source::Page,
        ("outside", _) => Source::Outside,
        ("agent", Some(by)) => Source::Agent(by.to_string()),
        _ => return None,
    };
    let name = match s.get("name") {
        Some(Value::String(n)) => Some((n.clone(), s.get("structural").and_then(Value::as_bool).unwrap_or(false))),
        _ => None,
    };
    let file = s.get("file")?.as_str()?.to_string();
    let (raw, data_only) = (s.get("raw")?.as_bool()?, s.get("data_only")?.as_bool()?);
    let mut before = BTreeMap::new();
    for (p, sha) in s.get("before")?.as_object()? {
        let sha = match sha {
            Value::Null => None,
            v => Some(Sha::parse_prefixed(v.as_str()?)?),
        };
        before.insert(TxPath::new(p).ok()?, sha);
    }
    if !records.0.contains(&record) {
        return Some(None);
    }
    let Ok(bytes) = std::fs::read(record_file(dir, &record)) else { return Some(None) };
    let inverse =
        (Sha::of(&bytes) == record).then(|| serde_json::from_slice::<Value>(&bytes).ok()).flatten().and_then(|v| Tx::from_json(&v).ok());
    let Some(inverse) = inverse else { return Some(None) };
    let mut step = TxStep::new(inverse, raw, data_only, file);
    (step.id, step.source, step.name, step.before, step.record) = (id, source, name, before, Some(record));
    Some(Some(step))
}

/// The bytes of the before-images `step` names, each read from `.music2/tx/blobs/`; None when one
/// isn't there.
pub(crate) fn images(dir: &Path, step: &TxStep) -> Option<u64> {
    let mut total = 0;
    for op in &step.inverse.ops {
        if let Op::Write { bytes: Payload::Local { path, .. }, .. } = op {
            total += std::fs::metadata(dir.join(path)).ok().filter(|m| m.is_file())?.len();
        }
    }
    Some(total)
}

/// Write the history of the project at `dir` (WRITE-23, WRITE-25): each step's record, unless
/// `records` says it is there; then the index; then the records no step names now.
pub(crate) fn write(
    dir: &Path,
    undo: &mut [TxStep],
    redo: &mut [TxStep],
    next: u64,
    ends: (Option<End>, Option<End>),
    records: &mut Records,
) -> io::Result<()> {
    let folder = folder(dir);
    std::fs::create_dir_all(&folder)?;
    let mut named = BTreeSet::new();
    for step in undo.iter_mut().chain(redo.iter_mut()) {
        let made = match step.record {
            Some(sha) if records.0.contains(&sha) => sha,
            _ => {
                let bytes = serde_json::to_vec(&step.inverse.to_json()).expect("JSON");
                let sha = Sha::of(&bytes);
                if !records.0.contains(&sha) {
                    put(&record_file(dir, &sha), &bytes)?;
                    records.0.insert(sha);
                }
                step.record = Some(sha);
                sha
            }
        };
        named.insert(made);
    }
    let stack = |steps: &[TxStep]| steps.iter().map(described).collect::<Vec<Value>>();
    let ends = json!({"undo": ends.0.map(|e| e.as_str()), "redo": ends.1.map(|e| e.as_str())});
    let index = json!({"version": VERSION, "next": next, "undo": stack(undo), "redo": stack(redo), "ends": ends});
    put(&index_file(dir), &serde_json::to_vec(&index).expect("JSON"))?;
    for sha in records.0.iter().filter(|s| !named.contains(s)).copied().collect::<Vec<_>>() {
        match std::fs::remove_file(record_file(dir, &sha)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {
                records.0.remove(&sha);
            }
        }
    }
    Ok(())
}

/// A step as the index says it.
fn described(step: &TxStep) -> Value {
    let before: Map<String, Value> =
        step.before.iter().map(|(p, sha)| (p.to_string(), sha.map_or(Value::Null, |s| json!(s.prefixed())))).collect();
    json!({
        "id": step.id,
        "source": step.source.as_str(),
        "by": step.source.by(),
        "name": step.name.as_ref().map(|n| &n.0),
        "structural": step.name.as_ref().is_some_and(|n| n.1),
        "file": step.file,
        "raw": step.raw,
        "data_only": step.data_only,
        "before": before,
        "record": step.record.expect("written first").hex(),
    })
}

/// `bytes` at `to`, whole: written to a temporary name beside it, pushed, and renamed over it.
fn put(to: &Path, bytes: &[u8]) -> io::Result<()> {
    static N: AtomicU64 = AtomicU64::new(0);
    let name = to.file_name().and_then(|n| n.to_str()).unwrap_or("undo");
    let tmp = to.with_file_name(format!(".{name}.{}.{}.part", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let done = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        m2_os::fs::push(&f)?;
        std::fs::rename(&tmp, to)
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// Remove what a write killed half way left in `.music2/undo/`: its temporary files.
pub(crate) fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(folder(dir)) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && name.ends_with(".part") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// The Notice `undo.unread` (WRITE-25), saying why.
fn unread(why: &str) -> m2_song::Diagnostic {
    m2_song::Diagnostic::notice("undo.unread", format!("The undo history kept in .music2/undo/ couldn't all be read: {why}"))
}

impl crate::Session {
    /// Read the history back as the last writer left it (WRITE-23), offering each step only while
    /// it can still apply exactly (WRITE-24), and write what is kept: the steps cut off go, and so
    /// do their records. An index that can't be read leaves no undo, and says so (WRITE-25).
    pub(crate) fn read_history(&mut self) {
        sweep(&self.dir);
        match read(&self.dir) {
            Ok(None) => {}
            Ok(Some(index)) => {
                self.steps = self.steps.max(index.next);
                self.records = index.records;
                let (undo, undo_end) = self.walk(index.undo);
                let (redo, redo_end) = self.walk(index.redo);
                if undo_end == Some(End::Unread) || redo_end == Some(End::Unread) {
                    self.own.push(unread("a step of it is missing or damaged, so Undo stops there"));
                }
                (self.history, self.redo) = (undo, redo);
                self.ends = (undo_end.or(index.ends.0), redo_end.or(index.ends.1));
                self.keep_history();
            }
            Err(why) => {
                (self.history, self.redo) = (vec![], vec![]);
                self.ends = (Some(End::Unread), None);
                self.own.push(unread(&format!("{why}, so Undo starts from here")));
                self.records = Records(listed(&self.dir));
                self.keep_history();
            }
        }
    }

    /// The steps of one stack read back, oldest first, that apply exactly, walking from its newest
    /// over the files as they are now (WRITE-24): a step whose files all hold what it left is kept,
    /// and the walk goes on from what it puts back; one whose files all hold what it puts back (its
    /// undo landed, its history's write didn't) is left out; anything else ends the walk there,
    /// with why.
    fn walk(&self, entries: Vec<Option<TxStep>>) -> (Vec<TxStep>, Option<End>) {
        // Each file's hash as the walk has it: None where nothing is; an error where it can't be read.
        let mut now: BTreeMap<TxPath, Result<Option<Sha>, ()>> = BTreeMap::new();
        let mut kept = vec![];
        let mut end = None;
        for entry in entries.into_iter().rev() {
            let Some(mut step) = entry else {
                end = Some(End::Unread);
                break;
            };
            let Some(images) = images(&self.dir, &step) else {
                end = Some(End::Unread);
                break;
            };
            step.images = images;
            let left: Vec<(TxPath, Option<Sha>)> = step.inverse.base.iter().map(|(p, b)| (p.clone(), b.sha())).collect();
            let mut at = |p: &TxPath| *now.entry(p.clone()).or_insert_with(|| self.read_current(p).map(|c| c.sha).map_err(drop));
            let put = |p: &TxPath, left: Option<Sha>| step.before.get(p).copied().unwrap_or(left);
            if left.iter().all(|(p, sha)| at(p) == Ok(*sha)) {
                for (p, sha) in &left {
                    now.insert(p.clone(), Ok(put(p, *sha)));
                }
                kept.push(step);
            } else if !left.iter().all(|(p, sha)| at(p) == Ok(put(p, *sha))) {
                end = Some(End::Outside);
                break;
            }
        }
        kept.reverse();
        (kept, end)
    }

    /// Write the history as it is now (WRITE-23): only a session that holds the lock, and never a
    /// read-only one's. A disk that refuses it costs the way back after closing, never undo now.
    pub(crate) fn keep_history(&mut self) {
        if !self.access.writer() || self.forward.is_some() {
            return;
        }
        let ends = self.ends;
        if let Err(e) = write(&self.dir, &mut self.history, &mut self.redo, self.steps, ends, &mut self.records) {
            eprintln!("music2: couldn't keep the undo history in .music2/undo/: {e}");
        }
    }

    /// Why undo goes back no further than its oldest step, when something cut it off (WRITE-24):
    /// a change made outside music2, or a history that couldn't be read.
    pub fn undo_end(&self) -> Option<End> {
        self.ends.0
    }

    /// The same for redo.
    pub fn redo_end(&self) -> Option<End> {
        self.ends.1
    }
}
