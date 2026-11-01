//! Undo that survives closing (WRITE-23 to WRITE-26): the session's undo history, kept in
//! `.music2/undo/` and read back when the song next opens for writing.
//!
//! `index.json` holds the two stacks, undo's and redo's, oldest first, and the song as the history
//! last saw it, as one hash of its files. Each step names its record: its inverse Tx's JSON, in a
//! file named by the SHA-256 of its bytes, written once. Every write goes to a temporary name, is
//! pushed and renamed: the records first, then the index, then the records no step names go.
//!
//! A change to the stacks that writes the song (a commit, an undo, a redo, a change taken back) is
//! written ahead of its files: the index then says the stacks as they are, the step the change
//! pushes and how ([`Move`]), and the song as it will be. Read back, the files say whether it
//! landed. Any other change (a step from outside, a skip) is written after it. So a crash can lose
//! a step, never make one lie: what is read back is checked against the files (WRITE-24) and each
//! record against its name (WRITE-25).
//!
//! The history knows nothing of what a Tx means: a step is its inverse, what it is called, who made
//! it, and the hash each file it touches held before its change (what its undo puts back).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use m2_song::tx::{Op, Payload, Tx, TxPath};
use m2_song::{DataPath, Sha, Snapshot, Stored, path::LOCAL};
use serde_json::{Map, Value, json};

use crate::session::Source;
use crate::tx::TxStep;

/// The index's format: one of another version is read as one that can't be read.
const VERSION: u64 = 1;

/// How much undo keeps (WRITE-26), over both stacks: the steps, the bytes of text and data their
/// inverses hold, and the bytes of the before-images they name in `.music2/tx/blobs/`, each image
/// once. Past any of them the oldest steps go first: undo's oldest, then redo's farthest, never
/// either's top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bound {
    pub steps: usize,
    pub text: usize,
    pub images: u64,
}

/// 500 steps, 64 MiB of text and data, 256 MiB of before-images.
pub const BOUND: Bound = Bound { steps: 500, text: 64 << 20, images: 256 << 20 };

#[cfg(test)]
thread_local! {
    static TEST_BOUND: std::cell::Cell<Bound> = const { std::cell::Cell::new(BOUND) };
}

/// The bound a session opened on this thread keeps: [`BOUND`], but in a test that set another.
pub(crate) fn bound() -> Bound {
    #[cfg(test)]
    return TEST_BOUND.with(|b| b.get());
    #[cfg(not(test))]
    BOUND
}

/// Sessions opened on this thread from now on keep `b` (tests).
#[cfg(test)]
pub(crate) fn test_bound(b: Bound) {
    TEST_BOUND.with(|t| t.set(b));
}

/// Why undo, or redo, goes back no further than its oldest step, said by the session that met it
/// as it read the history back (WRITE-24, WRITE-25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// A file a step read back touches doesn't hold what the step left: a change made outside
    /// music2, or one the history doesn't know. For redo, any change to the song since.
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
}

/// What a change written ahead of its files does to the stacks (WRITE-25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Move {
    /// A new change: its step goes on undo, and redo ends.
    Commit,
    /// Undo's top goes; its step goes on redo.
    Undo,
    /// Redo's top goes; its step goes on undo.
    Redo,
    /// Undo's top goes, taken back as if it had never been (AGENT-3); redo ends.
    Retract,
}

impl Move {
    fn as_str(&self) -> &'static str {
        match self {
            Move::Commit => "commit",
            Move::Undo => "undo",
            Move::Redo => "redo",
            Move::Retract => "retract",
        }
    }

    fn parse(s: &str) -> Option<Move> {
        Some(match s {
            "commit" => Move::Commit,
            "undo" => Move::Undo,
            "redo" => Move::Redo,
            "retract" => Move::Retract,
            _ => return None,
        })
    }
}

/// The step an undo, a redo or a take-back moves, and how, while its change is written: kept out of
/// its stack meanwhile, and put back in the history written ahead of the change.
pub(crate) struct Moving {
    pub of: Move,
    pub from: Option<TxStep>,
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

/// The song's files as `snap` has them, as one hash (WRITE-24): each file's path with the hash of
/// its bytes, an asset by its name alone (never written over, FMT-14), a file that can't be read as
/// such. `layout.json` isn't among them: a step that touches it is checked by its own hash.
pub(crate) fn song(snap: &Snapshot) -> Sha {
    let mut lines = BTreeMap::new();
    for (p, text) in &snap.files {
        lines.insert(p, snap.not_utf8.get(p).copied().unwrap_or_else(|| Sha::of_shared(text)).hex());
    }
    for (p, stored) in snap.binaries.iter() {
        let said = match (DataPath::parse(p), stored) {
            (DataPath::Asset(_), _) => "asset".to_string(),
            (_, Stored::Hash(sha)) => sha.hex(),
            _ => "?".to_string(),
        };
        lines.insert(p, said);
    }
    for p in snap.unreadable.keys() {
        lines.insert(p, "?".to_string());
    }
    let all: Vec<String> = lines.iter().map(|(p, said)| format!("{}\0{said}", m2_song::path::slashed(p))).collect();
    Sha::of(all.join("\n").as_bytes())
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

/// A change written ahead of its files: how it moves the stacks, the step it pushes (None where its
/// record can't be read), and the song as it is once the change lands.
pub(crate) struct Ahead {
    pub of: Move,
    pub step: Option<TxStep>,
    pub song: Option<Sha>,
}

/// The history as read back, before it is checked against the files.
pub(crate) struct Index {
    /// The id the last step got.
    pub next: u64,
    /// Each step the index names, oldest first: None where its record is missing, or isn't its
    /// bytes, or doesn't read.
    pub undo: Vec<Option<TxStep>>,
    pub redo: Vec<Option<TxStep>>,
    /// The song as the history last saw it, before `ahead`.
    pub song: Option<Sha>,
    pub ahead: Option<Ahead>,
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
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| "it isn't whole")?;
    if v.get("version").and_then(Value::as_u64) != Some(VERSION) {
        return Err("another version of music2 wrote it".into());
    }
    let records = Records(listed(dir));
    let step = |s: &Value| entry(dir, s, &records).ok_or("a step in it doesn't read");
    let stack = |key: &str| -> Result<Vec<Option<TxStep>>, &str> {
        v.get(key).and_then(Value::as_array).ok_or("it is cut short")?.iter().map(step).collect()
    };
    let (undo, redo) = (stack("undo")?, stack("redo")?);
    let next = v.get("next").and_then(Value::as_u64).ok_or("it is cut short")?;
    let sha = |v: Option<&Value>| v.and_then(Value::as_str).and_then(Sha::parse_prefixed);
    let ahead = match v.get("ahead") {
        None | Some(Value::Null) => None,
        Some(a) => {
            let of = a.get("move").and_then(Value::as_str).and_then(Move::parse).ok_or("a step in it doesn't read")?;
            Some(Ahead { of, step: step(a.get("step").unwrap_or(&Value::Null))?, song: sha(a.get("song")) })
        }
    };
    Ok(Some(Index { next, undo, redo, song: sha(v.get("song")), ahead, records }))
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
    // Its name is the hash of its bytes: a record changed on disk, even into another Tx that reads,
    // is no step's.
    if Sha::of(&bytes) != record {
        return Some(None);
    }
    let Some(inverse) = serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| Tx::from_json(&v).ok()) else { return Some(None) };
    let mut step = TxStep::new(inverse, raw, data_only, file);
    (step.id, step.source, step.name, step.before, step.record) = (id, source, name, before, Some(record));
    // The files its change moved, as recorded; else every one its undo touches (AGENT-2).
    if let Some(moved) = s.get("moved").and_then(Value::as_array) {
        step.moved = moved.iter().filter_map(|m| m.as_str().map(str::to_string)).collect();
    }
    Some(Some(step))
}

/// The before-images `step` names, each with its size as `.music2/tx/blobs/` has it; None when
/// one isn't there.
pub(crate) fn images(dir: &Path, step: &TxStep) -> Option<Vec<(Sha, u64)>> {
    let mut out = vec![];
    for op in &step.inverse.ops {
        if let Op::Write { bytes: Payload::Local { path, sha }, .. } = op {
            out.push((*sha, std::fs::metadata(dir.join(path)).ok().filter(|m| m.is_file())?.len()));
        }
    }
    Some(out)
}

/// The history to write: both stacks, the id the last step got, the song as it is, and a change
/// written ahead of its files with the song as it will be.
pub(crate) struct Writing<'a> {
    pub undo: &'a mut [TxStep],
    pub redo: &'a mut [TxStep],
    pub next: u64,
    pub song: Sha,
    pub ahead: Option<(Move, &'a mut TxStep, Sha)>,
}

/// Write the history of the project at `dir` (WRITE-23, WRITE-25): each step's record, unless
/// `records` says it is there; then the index; then the records no step names now.
pub(crate) fn write(dir: &Path, w: Writing, records: &mut Records) -> io::Result<()> {
    let Writing { undo, redo, next, song, mut ahead } = w;
    std::fs::create_dir_all(folder(dir))?;
    let mut named = BTreeSet::new();
    for step in undo.iter_mut().chain(redo.iter_mut()).chain(ahead.as_mut().map(|a| &mut *a.1)) {
        let made = match step.record {
            Some(sha) if records.0.contains(&sha) => sha,
            _ => {
                let bytes = serde_json::to_vec(&step.inverse.to_json()).expect("JSON");
                let sha = Sha::of(&bytes);
                if !records.0.contains(&sha) {
                    m2_os::fs::replace(&record_file(dir, &sha), &bytes)?;
                    records.0.insert(sha);
                }
                step.record = Some(sha);
                sha
            }
        };
        named.insert(made);
    }
    let stack = |steps: &[TxStep]| steps.iter().map(described).collect::<Vec<Value>>();
    let mut index = json!({"version": VERSION, "next": next, "song": song.prefixed(), "undo": stack(undo), "redo": stack(redo)});
    if let Some((of, step, then)) = &ahead {
        index["ahead"] = json!({"move": of.as_str(), "step": described(step), "song": then.prefixed()});
    }
    m2_os::fs::replace(&index_file(dir), &serde_json::to_vec(&index).expect("JSON"))?;
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
        "moved": step.moved,
        "before": before,
        "record": step.record.expect("written first").hex(),
    })
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

/// The Notice `undo.unread` (WRITE-25), in plain words: why the saved history couldn't all be read.
fn unread(whole: bool) -> m2_song::Diagnostic {
    let said = if whole {
        "The saved undo history couldn't be read, so Undo starts from here."
    } else {
        "Part of the saved undo history couldn't be read, so Undo stops there."
    };
    m2_song::Diagnostic::notice("undo.unread", said)
}

/// The hash each file `step` touches holds before its change, and after it: what its undo puts
/// back, and what it left.
fn sides(step: &TxStep) -> Vec<(TxPath, Option<Sha>, Option<Sha>)> {
    step.inverse.base.iter().map(|(p, b)| (p.clone(), step.before.get(p).copied().unwrap_or(b.sha()), b.sha())).collect()
}

impl crate::Session {
    /// Read the history back as the last writer left it (WRITE-23): a change written ahead of its
    /// files is taken as landed unless every file it touches holds what it held before it; redo
    /// ends when the song isn't as the history last saw it; each stack then keeps only the steps
    /// that still apply exactly (WRITE-24), within the bound (WRITE-26). What is kept is written
    /// back. An index that can't be read leaves no undo, and says so (WRITE-25).
    pub(crate) fn read_history(&mut self) {
        sweep(&self.dir);
        self.ends = (None, None);
        match read(&self.dir) {
            Ok(None) => {}
            Ok(Some(index)) => {
                self.steps = self.steps.max(index.next);
                self.records = index.records;
                let (mut undo, mut redo, mut seen) = (index.undo, index.redo, index.song);
                if let Some(ahead) = index.ahead {
                    let undone = |s: &TxStep| sides(s).iter().all(|(p, before, _)| self.held(p) == Ok(*before));
                    match (ahead.of, ahead.step) {
                        (_, Some(step)) if undone(&step) => {}
                        (Move::Commit, step) => {
                            undo.push(step);
                            redo.clear();
                            seen = ahead.song;
                        }
                        (_, None) => {}
                        (Move::Undo, step) => {
                            undo.pop();
                            redo.push(step);
                            seen = ahead.song;
                        }
                        (Move::Redo, step) => {
                            redo.pop();
                            undo.push(step);
                            seen = ahead.song;
                        }
                        (Move::Retract, _) => {
                            undo.pop();
                            redo.clear();
                            seen = ahead.song;
                        }
                    }
                }
                let (undo, undo_end) = self.walk(undo);
                let (redo, mut redo_end) = self.walk(redo);
                (self.history, self.redo) = (undo, redo);
                // A new change ends redo (AGENT-2): one made while the song was closed too.
                if seen != Some(song(&self.draft)) && !self.redo.is_empty() {
                    self.redo.clear();
                    redo_end = Some(End::Outside);
                }
                if undo_end == Some(End::Unread) || redo_end == Some(End::Unread) {
                    self.own.push(unread(false));
                }
                self.ends = (undo_end, redo_end);
                self.bound_stacks();
                self.keep_history();
            }
            Err(_) => {
                (self.history, self.redo) = (vec![], vec![]);
                self.ends = (Some(End::Unread), None);
                self.own.push(unread(true));
                self.records = Records(listed(&self.dir));
                self.keep_history();
            }
        }
    }

    /// The hash the file at `p` holds now (a folder's directory hash), None where nothing is: an
    /// error where it can't be read.
    fn held(&self, p: &TxPath) -> Result<Option<Sha>, ()> {
        self.read_current(p).map(|c| c.sha).map_err(drop)
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
            let sides = sides(&step);
            let mut at = |p: &TxPath| *now.entry(p.clone()).or_insert_with(|| self.held(p));
            if sides.iter().all(|(p, _, left)| at(p) == Ok(*left)) {
                for (p, before, _) in &sides {
                    now.insert(p.clone(), Ok(*before));
                }
                kept.push(step);
            } else if !sides.iter().all(|(p, before, _)| at(p) == Ok(*before)) {
                end = Some(End::Outside);
                break;
            }
        }
        kept.reverse();
        (kept, end)
    }

    /// Write the history ahead of a change about to write the song (WRITE-25): the stacks as they
    /// are, the step undo or redo moves put back on top of its own, and `step`, which the change
    /// pushes, with the song as `after` will be. `step` gets the id, source and name it will have.
    pub(crate) fn history_ahead(&mut self, step: &mut TxStep, after: &Snapshot) {
        if !self.access.writer() || self.forward.is_some() {
            return;
        }
        let mut moving = self.moving.take();
        let of = moving.as_ref().map_or(Move::Commit, |m| m.of);
        let from = moving.as_mut().and_then(|m| m.from.take());
        let next = match &from {
            Some(from) => {
                (step.id, step.source, step.name) = (from.id, from.source.clone(), from.name.clone());
                self.steps
            }
            None => {
                self.stamp(step);
                self.steps + 1
            }
        };
        let on_redo = of == Move::Redo;
        let back = from.is_some();
        if let Some(from) = from {
            if on_redo { &mut self.redo } else { &mut self.history }.push(from);
        }
        let (now, then) = (song(&self.draft), song(after));
        let writing = Writing { undo: &mut self.history, redo: &mut self.redo, next, song: now, ahead: Some((of, step, then)) };
        if let Err(e) = write(&self.dir, writing, &mut self.records) {
            eprintln!("music2: couldn't keep the undo history in .music2/undo/: {e}");
        }
        self.unwritten = false;
        if back {
            let from = if on_redo { &mut self.redo } else { &mut self.history }.pop();
            moving.as_mut().expect("moving").from = from;
        }
        self.moving = moving;
    }

    /// Write the history as it is now (WRITE-23): only a session that holds the lock, and never a
    /// read-only one's. A disk that refuses it costs the way back after closing, never undo now.
    pub(crate) fn keep_history(&mut self) {
        if !self.access.writer() || self.forward.is_some() {
            return;
        }
        let writing = Writing { undo: &mut self.history, redo: &mut self.redo, next: self.steps, song: song(&self.draft), ahead: None };
        if let Err(e) = write(&self.dir, writing, &mut self.records) {
            eprintln!("music2: couldn't keep the undo history in .music2/undo/: {e}");
        }
        self.unwritten = false;
    }

    /// Why undo goes back no further than its oldest step, when something cut it off as this
    /// session read the history back (WRITE-24): a change made outside music2, or a history that
    /// couldn't be read.
    pub fn undo_end(&self) -> Option<End> {
        self.ends.0
    }

    /// The same for redo.
    pub fn redo_end(&self) -> Option<End> {
        self.ends.1
    }
}
