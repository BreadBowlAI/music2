//! Tx through a session: bases, the baseline, conflicts, the expectations, the data path,
//! waiting batches, the lock, saves, a broken project's open, and undo as inverse Tx.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m2_runtime::PackageSet;
use m2_song::tx::{Anchor, Base, Confined, Declared, Edit, Expect, ModuleChange, Op, Payload, Pos, Range, Tx, TxPath};
use m2_song::{Key, LaneAddr, Sha};
use serde_json::{Value, json};

use super::Refusal;
use crate::{Session, StepError};

pub(crate) fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}

pub(crate) const MANIFEST: &str = crate::tests::MANIFEST;

/// A project folder, removed when dropped.
pub(crate) struct Dir(pub PathBuf);

impl Dir {
    pub fn new() -> Dir {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("m2-tx-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    /// A project of `files`, with the test manifest.
    pub fn with(files: &[(&str, &str)]) -> Dir {
        let d = Dir::new();
        d.write("music2.toml", MANIFEST);
        for (p, t) in files {
            d.write(p, t);
        }
        d
    }

    /// A copy of the session's first-song fixture.
    pub fn first_song() -> Dir {
        Dir::copy_of("core/session/tests/fixtures/first-song")
    }

    /// A copy of one of the repository's projects, `rel` from its root.
    pub fn copy_of(rel: &str) -> Dir {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let (f, t) = (e.path(), to.join(e.file_name()));
                if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
            }
        }
        let d = Dir::new();
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel), &d.0);
        d
    }

    pub fn write(&self, p: &str, text: &str) {
        self.write_bytes(p, text.as_bytes());
    }

    pub fn write_bytes(&self, p: &str, bytes: &[u8]) {
        let p = self.0.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    pub fn read(&self, p: &str) -> String {
        std::fs::read_to_string(self.0.join(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
    }

    pub fn open(&self) -> Session {
        Session::open(&self.0, std_packages()).unwrap_or_else(|d| panic!("{d:?}"))
    }

    /// Every file of the project but `.music2/`, by path, with its bytes.
    pub fn tree(&self) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![self.0.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                let rel = m2_song::path::slashed(p.strip_prefix(&self.0).unwrap());
                if rel.starts_with(".music2") {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.insert(rel, std::fs::read(&p).unwrap());
                }
            }
        }
        out
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn p(s: &str) -> TxPath {
    TxPath::new(s).unwrap_or_else(|e| panic!("{e}"))
}

/// A Tx of `ops`, each touched file based on what the project at `d` has now (a folder by its
/// directory hash), the text files' bases carrying their text.
pub(crate) fn based(d: &Dir, ops: Vec<Op>, expect: Expect) -> Tx {
    let mut tx = Tx::new("tx-test", ("before".into(), "after".into()), ops, expect);
    for path in tx.touched() {
        let file = d.0.join(path.path());
        let base = if file.is_dir() {
            Base::Sha(super::stage::folder_sha(&mut super::stage::Real, &d.0, path.as_str()).unwrap().unwrap())
        } else {
            match std::fs::read(&file) {
                Ok(bytes) => match String::from_utf8(bytes.clone()) {
                    Ok(t) if path.is_text() => Base::text(&t),
                    _ => Base::Sha(Sha::of(&bytes)),
                },
                Err(_) => Base::Absent,
            }
        };
        tx.base.insert(path, base);
    }
    tx
}

/// An edit of `file` replacing the first `old` after `after` with `new`, in the text `text`.
pub(crate) fn replace(file: &str, text: &str, after: &str, old: &str, new: &str) -> (Op, std::ops::Range<usize>) {
    let from = text.find(after).unwrap_or_else(|| panic!("no `{after}`"));
    let at = from + text[from..].find(old).unwrap_or_else(|| panic!("no `{old}`"));
    let r = at..at + old.len();
    let op = Op::TextEdit {
        file: p(file),
        edits: vec![Edit { start: Pos::Byte(r.start as u32), end: Pos::Byte(r.end as u32), expected: old.into(), text: new.into() }],
    };
    (op, r)
}

/// Setting the hats' pan in first-song's `song.ts`: a `Values` Tx.
pub(crate) fn pan(d: &Dir, value: f64) -> Tx {
    let song = d.read("song.ts");
    let rest = &song[song.find("pan: ").unwrap() + 5..];
    let now: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    let (op, r) = replace("song.ts", &song, "pan: ", &now, &value.to_string());
    based(
        d,
        vec![op],
        Expect::Values(vec![(Anchor::Written { file: p("song.ts"), range: Range::bytes(r.start as u32, r.end as u32) }, value)]),
    )
}

fn codes(r: &Refusal) -> Vec<&'static str> {
    match r {
        Refusal::Rejected { diagnostics, .. } => diagnostics.iter().map(|d| d.code).collect(),
        Refusal::Conflict { .. } => vec!["conflict"],
        Refusal::Io { .. } => vec!["io"],
    }
}

fn conflicts(r: Refusal) -> Vec<(String, Option<String>, Option<String>)> {
    match r {
        Refusal::Conflict { files } => files.into_iter().map(|c| (c.file, c.clip, c.why)).collect(),
        other => panic!("not a conflict: {other:?}"),
    }
}

// WRITE-3, WRITE-6, WRITE-13, WRITE-21: a Values Tx changes its literal's bytes alone; the delta
// is that one value; it moves code_rev; check then commit agree; undo puts every byte back and
// redo writes it again.
#[test]
fn a_values_tx_changes_its_literal_alone_and_undoes() {
    let d = Dir::first_song();
    let mut s = d.open();
    let song = d.read("song.ts");
    let tx = pan(&d, 0.45);
    let checked = s.check(&tx).unwrap();
    assert_eq!(checked.delta.values.len(), 1, "{:?}", checked.delta);
    assert!(!checked.delta.structural() && checked.delta.lanes.is_empty());
    assert_eq!((checked.delta.values[0].from, checked.delta.values[0].to), (0.3, 0.45));
    assert!(checked.candidate().snapshot.files[Path::new("song.ts")].contains("pan: 0.45"));
    assert_eq!(d.read("song.ts"), song, "check writes nothing");
    let done = s.commit(tx).unwrap();
    assert_eq!(done.hash, checked.hash, "check then commit: one delta");
    assert_eq!((done.number, done.code_rev, done.data_rev), (2, 2, 1));
    assert_eq!(d.read("song.ts"), song.replacen("pan: 0.3", "pan: 0.45", 1));
    assert_eq!(done.patches.len(), 1);
    assert_eq!((done.patches[0].text.as_str(), done.patches[0].from.0), ("0.45", 14), "lines from 0 (C5)");
    assert!(!s.refresh(), "the draft follows the commit");
    assert!(s.can_undo());
    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("after", "before"));
    assert_eq!(d.read("song.ts"), song);
    assert!(s.can_redo());
    s.redo().unwrap();
    assert_eq!(d.read("song.ts"), song.replacen("pan: 0.3", "pan: 0.45", 1));
    assert!(!d.0.join(".music2/tx").read_dir().unwrap().any(|e| e.unwrap().path().extension().is_some_and(|x| x == "part")));
}

// WRITE-12, R2: an outside change elsewhere in the touched file rides along: the commit carries
// past it and is judged against the baseline, so the delta is the knob's alone and both changes
// are on disk. A change on the literal itself is a conflict.
#[test]
fn an_outside_literal_edit_then_a_knob_commit_keeps_both() {
    let d = Dir::first_song();
    let mut s = d.open();
    let song = d.read("song.ts");
    let mut tx = pan(&d, 0.45);
    // Planned on the text by its hash alone: the session's store has it.
    tx.base.insert(p("song.ts"), Base::Sha(Sha::of(song.as_bytes())));
    d.write("song.ts", &song.replacen("db(-5)", "db(-4)", 1));
    let done = s.commit(tx).unwrap();
    assert_eq!(done.delta.values.len(), 1, "the knob's change alone: {:?}", done.delta);
    let now = d.read("song.ts");
    assert!(now.contains("db(-4)") && now.contains("pan: 0.45"), "{now}");
    assert_eq!(*s.accepted().snapshot.files[Path::new("song.ts")], *now, "the outside change is accepted with it");

    // The literal changed under it: a conflict naming the file.
    let tx = pan(&d, 0.5);
    d.write("song.ts", &now.replacen("pan: 0.45", "pan: 0.35", 1));
    let c = conflicts(s.commit(tx).unwrap_err());
    assert_eq!(c, [("song.ts".into(), None, Some("changed on disk where the edit is".into()))]);
    assert!(d.read("song.ts").contains("pan: 0.35"), "nothing written");
}

// WRITE-12: a base text that isn't its hash's is malformed; a base the session never saw, with
// no text, is a conflict.
#[test]
fn base_texts_are_checked() {
    let d = Dir::first_song();
    let mut s = d.open();
    let mut tx = pan(&d, 0.45);
    tx.base.insert(p("song.ts"), Base::WithText { sha: Sha::of(b"other"), text: d.read("song.ts").into() });
    assert_eq!(codes(&s.check(&tx).err().unwrap()), ["tx.invalid"]);
    let mut tx = pan(&d, 0.45);
    tx.base.insert(p("song.ts"), Base::Sha(Sha::of(b"a text no one has")));
    assert_eq!(conflicts(s.check(&tx).err().unwrap()).len(), 1);
    // `expected` guards each edit.
    let mut tx = pan(&d, 0.45);
    if let Op::TextEdit { edits, .. } = &mut tx.ops[0] {
        edits[0].expected = "0.9".into();
    }
    assert_eq!(codes(&s.check(&tx).err().unwrap()), ["tx.expected"]);
    // A line and a UTF-16 column name the same bytes as their offset.
    let mut tx = pan(&d, 0.45);
    let song = d.read("song.ts");
    let at = song.find("pan: 0.3").unwrap() + 5;
    let line = song[..at].matches('\n').count() as u32;
    let col = (at - song[..at].rfind('\n').unwrap() - 1) as u32;
    if let Op::TextEdit { edits, .. } = &mut tx.ops[0] {
        edits[0].start = Pos::LineCol { line, utf16_col: col };
        edits[0].end = Pos::LineCol { line, utf16_col: col + 3 };
    }
    assert_eq!(s.check(&tx).unwrap().delta.values.len(), 1);
}

// WRITE-6: Values names exactly its constants: an edit that changes something else is refused,
// with the delta, and nothing is written.
#[test]
fn values_refuses_more_than_its_values() {
    let d = Dir::first_song();
    let mut s = d.open();
    let song = d.read("song.ts");
    let (op, _) = replace("song.ts", &song, "keys:", "db(-5)", "db(-2)");
    let (_, r) = replace("song.ts", &song, "pan: ", "0.3", "0.45");
    let tx = based(
        &d,
        vec![op],
        Expect::Values(vec![(Anchor::Written { file: p("song.ts"), range: Range::bytes(r.start as u32, r.end as u32) }, 0.45)]),
    );
    match s.commit(tx).unwrap_err() {
        Refusal::Rejected { diagnostics, unexpected } => {
            assert_eq!(diagnostics[0].code, "tx.unexpected");
            assert_eq!(unexpected.unwrap().values.len(), 1);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(d.read("song.ts"), song);
    // Identical refuses any change that plays; a comment is none.
    let (op, _) = replace("song.ts", &song, "keys:", "db(-5)", "db(-2)");
    assert_eq!(codes(&s.check(&based(&d, vec![op], Expect::Identical)).err().unwrap()), ["tx.unexpected"]);
    let at = song.find("export default").unwrap() as u32;
    let comment = Op::TextEdit {
        file: p("song.ts"),
        edits: vec![Edit { start: Pos::Byte(at), end: Pos::Byte(at), expected: String::new(), text: "// a comment\n".into() }],
    };
    let done = s.commit(based(&d, vec![comment], Expect::Identical)).unwrap();
    assert!(done.delta.is_empty(), "{:?}", done.delta);
    assert_eq!((done.number, done.code_rev), (2, 2), "a code revision all the same");
}

/// A project of one sound, for wraps.
pub(crate) const SAW: &str = "import { hz, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { delay } from \"std/fx\";\n\
                   export default song(() => osc.saw(hz(110)));\n";

/// Wrapping `osc.saw(hz(110))` in `delay(`…`)` with two insertions, confined to the call.
pub(crate) fn wrap(d: &Dir) -> (Tx, std::ops::Range<usize>) {
    let song = d.read("song.ts");
    let s = song.find("osc.saw(hz(110))").unwrap();
    let e = s + "osc.saw(hz(110))".len();
    let ins =
        |at: usize, text: &str| Edit { start: Pos::Byte(at as u32), end: Pos::Byte(at as u32), expected: String::new(), text: text.into() };
    let op = Op::TextEdit { file: p("song.ts"), edits: vec![ins(s, "delay("), ins(e, ", { time: 0.25 })")] };
    let mut c = Confined::default();
    c.ranges.insert(p("song.ts"), Declared::Ranges(vec![Range::bytes(s as u32, e as u32)]));
    (based(d, vec![op], Expect::Confined(c)), s..e)
}

// WRITE-6, WRITE-8, R5: an effect added by two insertions around a call is confined to the call:
// the wrapped call keeps its nodes' names, so the delta is the effect's nodes added and its reader
// rewired, nothing removed. Undo takes it out again.
#[test]
fn an_added_effect_is_confined() {
    let d = Dir::with(&[("song.ts", SAW)]);
    let mut s = d.open();
    let (tx, _) = wrap(&d);
    let done = s.commit(tx).unwrap();
    assert!(d.read("song.ts").contains("delay(osc.saw(hz(110)), { time: 0.25 })"));
    let delta = &done.delta;
    assert!(delta.removed.is_empty(), "the wrapped call keeps its names: {:?}", delta.removed);
    assert!(!delta.added.is_empty() && delta.added.iter().any(|n| n.op["kernel"] == json!("std:delay")), "{:?}", delta.added);
    assert!(!delta.rewired.is_empty());
    assert!(delta.values.is_empty() && delta.modules.is_empty());
    s.undo().unwrap();
    assert_eq!(d.read("song.ts"), SAW);
    // Declared nowhere near the call: refused.
    let (tx, _) = wrap(&d);
    let mut tx = tx;
    tx.expect =
        Expect::Confined(Confined { ranges: [(p("song.ts"), Declared::Ranges(vec![Range::bytes(0, 1)]))].into(), ..Confined::default() });
    assert_eq!(codes(&s.check(&tx).err().unwrap()), ["tx.unexpected"]);
}

/// A notes lane of two clips, `a` and `b`.
const TWO: &str = "{\"clips\": [\n  \
                   {\"id\": \"a\", \"start\": 0, \"length\": 4, \"notes\": [\n    \
                   {\"pitch\": \"C1\", \"start\": 0, \"length\": 1, \"velocity\": 0.8}\n  ]},\n  \
                   {\"id\": \"b\", \"start\": 4, \"length\": 4, \"notes\": [\n    \
                   {\"pitch\": \"C1\", \"start\": 0, \"length\": 1, \"velocity\": 0.8}\n  ]}\n]}\n";

fn kick_hits() -> LaneAddr {
    LaneAddr::new("kick", "hits").unwrap()
}

fn clip(id: &str, start: f64, pitch: &str) -> Value {
    json!({"id": id, "start": start, "length": 4, "notes": [{"pitch": pitch, "start": 0, "length": 1, "velocity": 0.8}]})
}

// WRITE-3, WRITE-4, PLANE-1: a Clips Tx is a data Tx: no QuickJS, data_rev alone moves; an outside
// change to another clip merges; one to its own clip is a conflict naming the clip; an id twice is
// a conflict (R6).
#[test]
fn clips_merge_and_conflict_per_clip() {
    let d = Dir::first_song();
    d.write("data/kick/hits.json", TWO);
    let mut s = d.open();
    let ops = vec![Op::Clips { lane: kick_hits(), put: vec![clip("a", 0.0, "D1")], delete: vec![] }];
    let lanes = Expect::Confined(Confined { lanes: vec![kick_hits()], ..Confined::default() });
    let tx = based(&d, ops.clone(), lanes.clone());
    // Another clip changed meanwhile.
    d.write("data/kick/hits.json", &TWO.replacen("\"start\": 4", "\"start\": 8", 1));
    let builds = m2_script::builds();
    let done = s.commit(tx).unwrap();
    assert_eq!(m2_script::builds(), builds, "no QuickJS for data");
    assert_eq!((done.code_rev, done.data_rev), (1, 2));
    assert_eq!(done.delta.lanes, [kick_hits()]);
    let now = d.read("data/kick/hits.json");
    assert!(now.contains("\"pitch\": \"D1\"") && now.contains("\"start\": 8"), "both: {now}");

    // Its own clip changed meanwhile.
    let tx = based(&d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("a", 0.0, "E1")], delete: vec![] }], lanes.clone());
    d.write("data/kick/hits.json", &now.replacen("D1", "F1", 1));
    let c = conflicts(s.commit(tx).unwrap_err());
    assert_eq!(c, [("data/kick/hits.json".into(), Some("a".into()), Some("changed since the edit was planned".into()))]);

    // An id twice in the file: which one is meant is a guess.
    d.write("data/kick/hits.json", &TWO.replacen("\"id\": \"b\"", "\"id\": \"a\"", 1));
    let tx = based(&d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("a", 0.0, "E1")], delete: vec![] }], lanes);
    assert_eq!(conflicts(s.commit(tx).unwrap_err())[0].2.as_deref(), Some("is in the lane twice; renumber its clips first"));
}

// WRITE-21, the step 6 gate: undoing a clip edit after an outside change to that clip is a
// conflict naming the lane and the clip; after a change to another clip it goes ahead, and the
// other clip keeps the outside change.
#[test]
fn undoing_a_clip_after_its_outside_change_conflicts() {
    let d = Dir::first_song();
    d.write("data/kick/hits.json", TWO);
    let mut s = d.open();
    let lanes = Expect::Confined(Confined { lanes: vec![kick_hits()], ..Confined::default() });
    let tx = based(&d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("a", 1.0, "C1")], delete: vec![] }], lanes.clone());
    s.commit(tx).unwrap();
    let moved = d.read("data/kick/hits.json");
    d.write("data/kick/hits.json", &moved.replacen("\"pitch\": \"C1\"", "\"pitch\": \"G1\"", 1));
    assert!(s.refresh());
    assert!(!s.can_undo());
    match s.undo().unwrap_err() {
        StepError::Refused(Refusal::Conflict { files }) => {
            assert_eq!((files[0].file.as_str(), files[0].clip.as_deref()), ("data/kick/hits.json", Some("a")));
        }
        other => panic!("{other:?}"),
    }
    // Only clip b changed: undo goes ahead.
    let d = Dir::first_song();
    d.write("data/kick/hits.json", TWO);
    let mut s = d.open();
    let tx = based(&d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("a", 1.0, "C1")], delete: vec![] }], lanes);
    s.commit(tx).unwrap();
    let moved = d.read("data/kick/hits.json");
    let b_moved = moved.replacen("\"start\": 4", "\"start\": 12", 1);
    d.write("data/kick/hits.json", &b_moved);
    assert!(s.refresh());
    assert!(s.can_undo());
    s.undo().unwrap();
    let now = d.read("data/kick/hits.json");
    assert!(now.contains("\"id\": \"a\", \"start\": 0") && now.contains("\"start\": 12"), "{now}");
}

// WRITE-19, R12: what the session wrote comes with the stamps of its own files, so the watcher's
// settle rule makes no batch of them; an outside write right after is still a batch.
#[test]
fn the_sessions_own_writes_make_no_batch() {
    use crate::watch::{Git, Poll, Settle, listing, wall_ns};
    use std::time::{Duration, Instant};
    let d = Dir::first_song();
    let mut s = d.open();
    let none = std::collections::BTreeSet::new();
    let mut settle = Settle::new(&listing(&d.0, &none), wall_ns());
    s.commit(pan(&d, 0.45)).unwrap();
    let wrote = s.take_wrote();
    assert_eq!(wrote.len(), 1, "{wrote:?}");
    assert_eq!(wrote[0].0, Path::new("song.ts"));
    assert_eq!(wrote[0].1, crate::watch::Stamp::at(&d.0.join("song.ts")), "the stamp of its own file");
    assert!(s.take_wrote().is_empty(), "taken once");
    settle.wrote(&wrote, wall_ns());
    let t0 = Instant::now();
    let mut hash = |p: &Path| std::fs::read(d.0.join(p)).ok().map(|b| Sha::of(&b));
    for ms in [0, 50, 100, 200, 300] {
        let at = t0 + Duration::from_millis(ms);
        assert_eq!(settle.poll(at, wall_ns(), &listing(&d.0, &none), Git::default(), &mut hash), Poll::Idle, "{ms} ms");
    }
    // A save and an undo are told the same way.
    let song = d.read("song.ts");
    s.save(Path::new("song.ts"), Base::text(&song), &song.replacen("pan: 0.45", "pan: 0.5", 1)).unwrap();
    s.undo().unwrap();
    let wrote = s.take_wrote();
    assert!(wrote.len() == 2 && wrote.iter().all(|(p, st)| p == Path::new("song.ts") && st.is_some()), "{wrote:?}");
    settle.wrote(&wrote, wall_ns());
    d.write("data/kick/hits.json", &d.read("data/kick/hits.json").replacen("\"start\": 1,", "\"start\": 1.5,", 1));
    let mut ready = None;
    for ms in [400, 450, 500, 650, 700] {
        let at = t0 + Duration::from_millis(ms);
        if let Poll::Ready(p) = settle.poll(at, wall_ns(), &listing(&d.0, &none), Git::default(), &mut hash) {
            ready = Some(p);
        }
    }
    assert_eq!(ready, Some([PathBuf::from("data/kick/hits.json")].into()), "the outside write alone");
}

// WRITE-14, C7: a data Tx goes ahead while the draft's code is broken; a code Tx waits for it;
// a project that never compiled takes neither.
#[test]
fn the_draft_rule() {
    let d = Dir::first_song();
    let mut s = d.open();
    let song = d.read("song.ts");
    d.write("tracks/hats.ts", "export default (;\n");
    assert!(s.refresh() && !s.draft_errors.is_empty());
    assert_eq!(codes(&s.commit(pan(&d, 0.45)).unwrap_err())[0], "code.build");
    assert_eq!(d.read("song.ts"), song);
    let lanes = Expect::Confined(Confined { lanes: vec![kick_hits()], ..Confined::default() });
    let tx = based(&d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("x1", 64.0, "C1")], delete: vec![] }], lanes);
    let done = s.commit(tx).unwrap();
    assert_eq!((done.code_rev, done.data_rev), (1, 2), "a data revision while the code is broken");
    assert!(!s.draft_errors.is_empty(), "the code is still broken");
}

// WRITE-11, R17: check returns the delta's hash, and a commit on it goes ahead; another hash is
// refused. An unrelated edit elsewhere in the same file between them changes no name, so the
// commit still sees the same delta.
#[test]
fn seen_survives_an_unrelated_edit_in_the_same_file() {
    let d = Dir::first_song();
    let mut s = d.open();
    let mut tx = pan(&d, 0.45);
    tx.expect = Expect::Seen(Sha::of(b"anything"));
    let checked = s.check(&tx).unwrap();
    let mut wrong = tx.clone();
    wrong.expect = Expect::Seen(Sha::of(b"not it"));
    assert_eq!(codes(&s.commit(wrong).unwrap_err()), ["tx.unexpected"]);
    // A comment typed at the top, never taken in.
    let song = d.read("song.ts");
    d.write("song.ts", &format!("// typed meanwhile\n{song}"));
    tx.expect = Expect::Seen(checked.hash);
    let done = s.commit(tx).unwrap();
    assert_eq!(done.hash, checked.hash);
    assert!(d.read("song.ts").starts_with("// typed meanwhile\n") && d.read("song.ts").contains("pan: 0.45"));
}

// WRITE-19, R3: a broken code change and a lane move in one batch wait whole: the lane plays its
// accepted data, a Tx on a waiting path is a conflict, and fixing the code takes both in as one
// revision.
#[test]
fn a_broken_save_and_a_lane_move_in_one_batch_keep_the_lane() {
    let d = Dir::first_song();
    let mut s = d.open();
    let kick = d.read("tracks/kick.ts");
    let notes = s.accepted().compiled.data.notes("kick", "hits");
    let hits = d.read("data/kick/hits.json");
    d.write("tracks/kick.ts", &kick.replace("lane.notes(\"hits\")", "lane.notes(\"beats\") oops"));
    std::fs::rename(d.0.join("data/kick/hits.json"), d.0.join("data/kick/beats.json")).unwrap();
    assert!(s.refresh());
    assert_eq!(s.accepted().number, 1, "no revision");
    assert_eq!(s.accepted().compiled.data.notes("kick", "hits"), notes, "the lane plays its accepted data");
    assert!(s.waiting().contains(Path::new("data/kick/beats.json")));
    let lanes = Expect::Confined(Confined { lanes: vec![LaneAddr::new("kick", "beats").unwrap()], ..Confined::default() });
    let tx = based(&d, vec![Op::Write { file: p("data/kick/beats.json"), bytes: Payload::Text(hits.clone().into()) }], lanes);
    assert!(conflicts(s.check(&tx).unwrap_err())[0].2.as_deref().unwrap().contains("doesn't compile yet"));
    d.write("tracks/kick.ts", &kick.replace("lane.notes(\"hits\")", "lane.notes(\"beats\")"));
    assert!(s.refresh() && s.draft_errors.is_empty());
    assert_eq!(s.accepted().number, 2, "one revision for the whole batch");
    assert_eq!(s.accepted().compiled.data.notes("kick", "beats"), notes);
    assert!(s.waiting().is_empty());
}

// PLANE-1, WRITE-13, the step 6 gate: a lane file changed by an outside writer is taken in without
// running QuickJS; code_rev stays, data_rev moves.
#[test]
fn an_outside_lane_write_takes_the_data_path() {
    let d = Dir::first_song();
    let mut s = d.open();
    let builds = m2_script::builds();
    d.write("data/kick/hits.json", &d.read("data/kick/hits.json").replacen("\"start\": 1,", "\"start\": 1.5,", 1));
    assert!(s.refresh());
    assert_eq!(m2_script::builds(), builds, "no QuickJS run");
    let a = s.accepted();
    assert_eq!((a.number, a.code_rev, a.data_rev), (2, 1, 2));
    let taken = s.last_taken().unwrap();
    assert_eq!(taken.plane, crate::take::Plane::Data);
    // A full compile of the same files reads the same data and problems.
    let full = crate::compile(&a.project, &mut std_packages()).unwrap();
    assert_eq!(full.data.notes("kick", "hits"), a.compiled.data.notes("kick", "hits"));
    assert_eq!(full.problems, a.compiled.problems);
    let builds = m2_script::builds();
    // Code makes a code revision.
    d.write("song.ts", &format!("{}// end\n", d.read("song.ts")));
    assert!(s.refresh());
    assert_eq!(m2_script::builds(), builds + 1);
    let a = s.accepted();
    assert_eq!((a.number, a.code_rev, a.data_rev), (3, 3, 2));
}

// WRITE-20, the step 6 gate: a project whose code doesn't compile opens, with no accepted code and
// the build's errors; it plays nothing, refuses data and code Tx, saves, and takes its code in
// once it compiles.
#[test]
fn a_broken_project_opens_silent() {
    let d = Dir::first_song();
    let song = d.read("song.ts");
    d.write("song.ts", &song.replacen("master(", "master((", 1));
    let mut s = d.open();
    assert_eq!((s.accepted().number, s.accepted().code_rev, s.accepted().data_rev), (1, 0, 0));
    assert!(s.draft_errors.iter().any(|e| e.to_string().contains("song.ts")), "{:?}", s.draft_errors);
    // No graph and no data: nothing to show or play (the editor's view and timeline read these).
    assert!(s.accepted().compiled.ir.modules.is_empty() && s.accepted().compiled.sites.is_empty());
    assert!(s.accepted().compiled.data.lanes().next().is_none());
    let lanes = Expect::Confined(Confined { lanes: vec![kick_hits()], ..Confined::default() });
    let tx = based(&d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("x1", 64.0, "C1")], delete: vec![] }], lanes);
    assert_eq!(codes(&s.commit(tx).unwrap_err())[0], "code.build");
    let broken = d.read("song.ts");
    let saved = s.save(Path::new("song.ts"), Base::text(&broken), &song).unwrap();
    assert!(saved.taken.accepted.is_some());
    assert_eq!((s.accepted().number, s.accepted().code_rev, s.accepted().data_rev), (2, 2, 2));
    assert!(s.draft_errors.is_empty() && !s.accepted().compiled.data.notes("kick", "hits").is_empty());
    // Opened with a newer format, or no manifest, it doesn't open at all.
    d.write("music2.toml", &MANIFEST.replace("format = 1", "format = 2"));
    drop(s);
    assert_eq!(Session::open(&d.0, std_packages()).err().unwrap()[0].code, "format.newer");
    std::fs::remove_file(d.0.join("music2.toml")).unwrap();
    assert!(Session::open(&d.0, std_packages()).is_err());
}

// WRITE-2: a save is compared and swapped on its base, written whole, taken in, and undone while
// the file is as it left it; a stale base is a conflict and writes nothing.
#[test]
fn saves() {
    let d = Dir::first_song();
    let mut s = d.open();
    let song = d.read("song.ts");
    let edited = song.replacen("pan: 0.3", "pan: 0.4", 1);
    let saved = s.save(Path::new("song.ts"), Base::Sha(Sha::of(song.as_bytes())), &edited).unwrap();
    assert!(saved.recorded && d.read("song.ts") == edited);
    assert_eq!(s.accepted().code_rev, 2);
    let stale = s.save(Path::new("song.ts"), Base::Sha(Sha::of(song.as_bytes())), "export default 1;\n").unwrap_err();
    assert_eq!(conflicts(stale)[0].0, "song.ts");
    assert_eq!(d.read("song.ts"), edited);
    // A broken save is written and not recorded; the accepted revision stays.
    let broken = "export default (;\n";
    let saved = s.save(Path::new("song.ts"), Base::text(&edited), broken).unwrap();
    assert!(!saved.recorded && !s.draft_errors.is_empty() && s.accepted().number == 2);
    s.save(Path::new("song.ts"), Base::text(broken), &edited).unwrap();
    assert!(s.draft_errors.is_empty());
    // Undo of the first save puts its text back.
    s.undo().unwrap();
    assert_eq!(d.read("song.ts"), song);
    // A file that isn't UTF-8 is saved over: that is how it is fixed (R24).
    d.write_bytes("tracks/x.ts", b"export const x = 1;\xff\n");
    assert!(s.refresh());
    let raw = Sha::of(b"export const x = 1;\xff\n");
    s.save(Path::new("tracks/x.ts"), Base::Sha(raw), "export const x = 1;\n").unwrap();
    assert_eq!(d.read("tracks/x.ts"), "export const x = 1;\n");
}

// WRITE-17, C10: a second session of a project (here, in this process) is read-only: it reads
// and checks, refuses every write with WRITE-18's message, and can write once the first lets go.
#[test]
fn a_second_session_is_read_only() {
    let d = Dir::first_song();
    let mut first = d.open();
    let mut second = d.open();
    assert!(first.access().writer() && !second.access().writer());
    assert_eq!(second.problems()[0].code, "lock.held");
    assert!(second.check(&pan(&d, 0.45)).is_ok(), "a check writes nothing");
    let refused = second.commit(pan(&d, 0.45)).unwrap_err();
    assert_eq!(codes(&refused), ["lock.held"]);
    assert!(refused.to_string().starts_with("nothing was changed: this project is open for writing in music2"), "{refused}");
    assert!(matches!(
        second.save(Path::new("song.ts"), Base::text(&d.read("song.ts")), "export default 1;\n"),
        Err(Refusal::Rejected { .. })
    ));
    assert!(matches!(second.undo(), Err(StepError::Nothing(_))));
    assert!(!second.can_undo());
    first.commit(pan(&d, 0.45)).unwrap();
    drop(first);
    assert!(second.retry_lock());
    assert!(second.refresh());
    second.commit(pan(&d, 0.5)).unwrap();
    assert!(d.read("song.ts").contains("pan: 0.5"));
}

// WRITE-3a: a target that is a link, or in a folder that leads out of the project, or a name the
// project has in other capitals, is refused with `tx.path`; a text edit of a file that isn't UTF-8
// is refused (R24). Nothing is written.
#[test]
fn hostile_targets_on_disk_are_refused() {
    let d = Dir::first_song();
    let outside = Dir::new();
    outside.write("victim.json", "{\"clips\": []}\n");
    let mut s = d.open();
    let tree = d.tree();
    std::os::unix::fs::symlink(outside.0.join("victim.json"), d.0.join("data/kick/link.json")).unwrap();
    std::os::unix::fs::symlink(&outside.0, d.0.join("data/out")).unwrap();
    let write = |f: &str| {
        based(&d, vec![Op::Write { file: p(f), bytes: Payload::Text("{\"clips\": []}\n".into()) }], Expect::Confined(Confined::default()))
    };
    for f in ["data/kick/link.json", "data/out/x.json", "data/Kick/x.json", "data/kick/Hits.json"] {
        let Ok(path) = TxPath::new(f) else { continue };
        let mut tx = write(f);
        tx.base.insert(path, Base::Absent);
        let r = s.commit(tx).unwrap_err();
        assert_eq!(codes(&r), ["tx.path"], "{f}: {r}");
    }
    std::fs::remove_file(d.0.join("data/kick/link.json")).unwrap();
    std::fs::remove_file(d.0.join("data/out")).unwrap();
    assert_eq!(d.tree(), tree);
    assert_eq!(outside.read("victim.json"), "{\"clips\": []}\n");
    d.write_bytes("tracks/x.ts", b"export const x = 1;\xff\n");
    assert!(s.refresh());
    let edit = Op::TextEdit {
        file: p("tracks/x.ts"),
        edits: vec![Edit { start: Pos::Byte(0), end: Pos::Byte(0), expected: String::new(), text: "//\n".into() }],
    };
    let tx =
        based(&d, vec![edit], Expect::Confined(Confined { ranges: [(p("tracks/x.ts"), Declared::Whole)].into(), ..Confined::default() }));
    let r = s.commit(tx).unwrap_err();
    assert!(codes(&r) == ["tx.invalid"] && r.to_string().contains("isn't UTF-8"), "{r}");
}

// WRITE-21, R22: a Tx that writes a new lane file, removes one, and renames a folder is one undo
// step: undo puts every file back, redo makes it again; a data-only inverse is judged by the
// lanes its files feed, worked out when it is undone.
#[test]
fn a_data_tx_of_several_files_undoes_as_one_step() {
    let d = Dir::first_song();
    d.write("data/old/hits.json", "{\"clips\": []}\n");
    d.write("data/kick/stray.json", "{\"clips\": []}\n");
    let mut s = d.open();
    let before = d.tree();
    let ops = vec![
        Op::Write { file: p("data/hats/extra.json"), bytes: Payload::Text("{\"clips\": []}\n".into()) },
        Op::Remove { file: p("data/kick/stray.json") },
        Op::Rename { from: p("data/old"), to: p("data/older") },
    ];
    let done = s.commit(based(&d, ops, Expect::Confined(Confined::default()))).unwrap();
    assert!(done.delta.is_empty(), "{:?}", done.delta);
    let after = d.tree();
    assert!(
        after.contains_key("data/hats/extra.json")
            && !after.contains_key("data/kick/stray.json")
            && after.contains_key("data/older/hits.json")
    );
    assert!(!d.0.join("data/old").exists(), "an emptied folder goes");
    s.undo().unwrap();
    assert_eq!(d.tree(), before);
    s.redo().unwrap();
    assert_eq!(d.tree(), after);
}

// DIAG-3, C13: a Tx that makes a module play data nothing played before says so; a move to a new
// path doesn't.
#[test]
fn claiming_unclaimed_data_is_a_notice() {
    let src = "import { song, track } from \"@m2/core\";\nimport { kick } from \"std/instruments\";\n\
               const k = track(\"kick\", ({ lane }) => kick(lane.notes(\"hits\")));\nexport default song(() => k.out);\n";
    let d = Dir::with(&[("song.ts", src), ("data/snare/hits.json", "{\"clips\": []}\n")]);
    let mut s = d.open();
    assert_eq!(s.problems()[0].code, "data.unclaimed");
    let added = src
        .replace(
            "export default song(() => k.out);",
            "const sn = track(\"snare\", ({ lane }) => kick(lane.notes(\"hits\")));\nexport default song(() => sum(k.out, sn.out));",
        )
        .replace("import { kick }", "import { sum } from \"std/math\";\nimport { kick }");
    let tx = based(&d, vec![Op::Write { file: p("song.ts"), bytes: Payload::Text(added.into()) }], Expect::Seen(Sha::of(b"x")));
    let checked = s.check(&tx).unwrap();
    let claimed: Vec<_> = checked.diagnostics.iter().filter(|d| d.code == "data.claimed").map(|d| d.src.clone()).collect();
    assert_eq!(claimed, [Some("data/snare/hits.json".to_string())]);
    assert!(checked.delta.modules.contains(&ModuleChange::Added(Key::new("snare").unwrap())));
    // A relink moves the data to a path no one had: no claim.
    let moved = vec![Op::Rename { from: p("data/snare/hits.json"), to: p("data/kick/hits.json") }];
    let checked = s.check(&based(&d, moved, Expect::Seen(Sha::of(b"x")))).unwrap();
    assert!(checked.diagnostics.iter().all(|d| d.code != "data.claimed"), "{:?}", checked.diagnostics);
    assert_eq!(checked.delta.lanes, [kick_hits()]);
}

// PLANE-3, WRITE-3: a Tx may write layout.json: journaled and undone with it, but never a
// revision or part of its delta.
#[test]
fn layout_is_written_but_makes_no_revision() {
    let d = Dir::first_song();
    let mut s = d.open();
    let ops = vec![Op::Write { file: p("layout.json"), bytes: Payload::Text("{\"tracks\": {}}\n".into()) }];
    let done = s.commit(based(&d, ops, Expect::Identical)).unwrap();
    assert_eq!((done.number, d.read("layout.json").as_str()), (1, "{\"tracks\": {}}\n"));
    assert!(!s.refresh());
    s.undo().unwrap();
    assert!(!d.0.join("layout.json").exists());
    assert_eq!(s.accepted().number, 1);
}

// A Tx that changes nothing commits nothing: no revision, no undo step.
#[test]
fn a_tx_that_changes_nothing_commits_nothing() {
    let d = Dir::first_song();
    let mut s = d.open();
    let song = d.read("song.ts");
    let tx = based(&d, vec![Op::Write { file: p("song.ts"), bytes: Payload::Text(song.into()) }], Expect::Identical);
    let done = s.commit(tx).unwrap();
    assert_eq!((done.number, done.wrote.len()), (1, 0));
    assert!(!s.can_undo());
}

// WRITE-21: undo keeps 500 steps and 64 MiB of text at most, the oldest going first, but always
// the newest step, whatever it keeps.
#[test]
fn undo_memory_is_bounded() {
    let d = Dir::first_song();
    let mut s = d.open();
    let step = |n: usize| {
        let ops = vec![Op::Write { file: p("tracks/notes.ts"), bytes: Payload::Text("x".repeat(n).into()) }];
        let inverse = Tx::new("t", ("a".into(), "b".into()), ops, Expect::Identical);
        super::TxStep::new(inverse, true, false, "tracks/notes.ts".into())
    };
    for _ in 0..501 {
        s.record_tx(step(1));
    }
    assert_eq!(s.history.len(), 500);
    for _ in 0..3 {
        s.record_tx(step(30 << 20));
    }
    assert_eq!(s.history.len(), 2, "the oldest steps went first, until the rest fit");
    s.record_tx(step(65 << 20));
    assert_eq!(s.history.len(), 1, "alone over the bound, the newest step stays");
}

// WRITE-14, C8, DIAG-3: while the draft's code is broken, an outside batch that changes only data
// is taken in against the accepted code; an outside write that makes a module play data nothing
// played before is a session-only `data.claimed`, never one of the revision's problems.
#[test]
fn outside_data_is_taken_in_while_the_code_is_broken() {
    let d = Dir::first_song();
    let mut s = d.open();
    d.write("tracks/hats.ts", "export default (;\n");
    assert!(s.refresh() && !s.draft_errors.is_empty());
    let notes = s.accepted().compiled.data.notes("kick", "hits");
    d.write("data/kick/hits.json", &d.read("data/kick/hits.json").replacen("\"start\": 1,", "\"start\": 1.5,", 1));
    assert!(s.refresh());
    let a = s.accepted();
    assert_eq!((a.number, a.code_rev, a.data_rev), (2, 1, 2), "a data revision");
    assert!(!s.draft_errors.is_empty() && s.waiting().contains(Path::new("tracks/hats.ts")));
    assert_ne!(a.compiled.data.notes("kick", "hits"), notes, "the lane plays its new notes");

    // A track key renamed back by hand claims the folder it left: said for the outside write.
    let src = "import { song, track } from \"@m2/core\";\nimport { kick } from \"std/instruments\";\n\
               const k = track(\"kicks\", ({ lane }) => kick(lane.notes(\"hits\")));\nexport default song(() => k.out);\n";
    let d = Dir::with(&[("song.ts", src), ("data/kick/hits.json", "{\"clips\": []}\n")]);
    let mut s = d.open();
    d.write("song.ts", &src.replace("\"kicks\"", "\"kick\""));
    assert!(s.refresh());
    let notices = &s.last_taken().unwrap().notices;
    assert_eq!(notices.iter().map(|n| (n.code, n.src.as_deref())).collect::<Vec<_>>(), [("data.claimed", Some("data/kick/hits.json"))]);
    assert!(s.problems().iter().all(|p| p.code != "data.claimed"), "session-only");
}

// WRITE-3a, WRITE-6, WRITE-12: a Tx naming a file the disk spells another way (other capitals or
// another Unicode form, on a disk that ignores them), or one through a link to a folder inside the
// project, is refused with `tx.path` whatever its base: reading through the alias would match the
// real file's hash while the delta saw a file nobody plays, so every expectation, `Identical`
// included, would pass and the real file change. Nothing is written.
#[test]
fn a_file_spelled_another_way_is_refused() {
    let d = Dir::first_song();
    d.write("tracks/b\u{e9}ss.ts", "export const b = 1;\n");
    let mut s = d.open();
    let song = d.read("song.ts");
    let tree = d.tree();
    // Based on the real file's bytes, which a read through the alias finds.
    let aliased = |file: &str, real: &str, op: Op| {
        let mut tx = Tx::new("tx-alias", (String::new(), String::new()), vec![op], Expect::Identical);
        tx.base.insert(p(file), Base::Sha(Sha::of(&std::fs::read(d.0.join(real)).unwrap())));
        tx
    };
    std::os::unix::fs::symlink("tracks", d.0.join("alias")).unwrap();
    let nfd = "tracks/be\u{301}ss.ts";
    let one = || Payload::Text("export default 1;".into());
    let cases = [
        aliased("Song.ts", "song.ts", replace("Song.ts", &song, "pan: ", "0.3", "0.9").0),
        aliased("tracks/KICK.ts", "tracks/kick.ts", Op::Remove { file: p("tracks/KICK.ts") }),
        aliased(nfd, "tracks/b\u{e9}ss.ts", Op::Write { file: p(nfd), bytes: Payload::Text("export const b = 2;\n".into()) }),
        aliased("alias/kick.ts", "tracks/kick.ts", Op::Write { file: p("alias/kick.ts"), bytes: one() }),
    ];
    // Where the disk tells the spellings apart, nothing answers to them: a plain conflict.
    let folds = d.0.join("SONG.TS").exists();
    for (i, tx) in cases.into_iter().enumerate() {
        let r = s.commit(tx).unwrap_err();
        if folds || i == 3 {
            assert_eq!(codes(&r), ["tx.path"], "{i}: {r}");
        }
    }
    std::fs::remove_file(d.0.join("alias")).unwrap();
    assert_eq!(d.tree(), tree, "nothing written");
    assert_eq!(s.accepted().number, 1);
}

// WRITE-19, R3: a folder holding a file that waits (changed on disk together with code that
// doesn't compile yet) waits with it: moving it is a conflict, since its baseline would take in
// half of that batch.
#[test]
fn a_folder_holding_a_waiting_file_waits() {
    let d = Dir::first_song();
    let mut s = d.open();
    d.write("song.ts", "export default (;\n");
    d.write("data/kick/hits.json", &d.read("data/kick/hits.json").replacen("\"start\": 1,", "\"start\": 1.5,", 1));
    assert!(s.refresh() && s.waiting().contains(Path::new("data/kick/hits.json")));
    let lanes = Confined { lanes: vec![LaneAddr::new("kick", "hits").unwrap()], ..Confined::default() };
    let mv = based(&d, vec![Op::Rename { from: p("data/kick"), to: p("data/kicks") }], Expect::Confined(lanes));
    let c = conflicts(s.check(&mv).unwrap_err());
    assert!(c.len() == 1 && c[0].0 == "data/kick" && c[0].2.as_deref().unwrap().contains("doesn't compile yet"), "{c:?}");
    assert!(d.0.join("data/kick/hits.json").exists() && s.accepted().number == 1);
}

// WRITE-6 (C1, rule 5): a text edit writes its file as a whole write does, so a tempo changed by
// an edit of the arrangement declared whole is the Tx's own change, as the same change written
// whole is. Undeclared, the edit is refused for its bytes (rule 4).
#[test]
fn an_edit_of_the_arrangement_is_the_txs_own() {
    let d = Dir::first_song();
    let mut s = d.open();
    let text = d.read("data/arrangement.json");
    let (op, _) = replace("data/arrangement.json", &text, "bpm", "124", "100");
    let whole = Confined { ranges: [(p("data/arrangement.json"), Declared::Whole)].into(), ..Confined::default() };
    let checked = s.check(&based(&d, vec![op.clone()], Expect::Confined(whole.clone()))).unwrap();
    assert!(checked.delta.arrangement);
    let write = Op::Write { file: p("data/arrangement.json"), bytes: Payload::Text(text.replacen("124", "100", 1).into()) };
    assert_eq!(s.check(&based(&d, vec![write], Expect::Confined(whole))).unwrap().hash, checked.hash, "the same change written whole");
    let r = s.check(&based(&d, vec![op], Expect::Confined(Confined::default()))).unwrap_err();
    assert!(r.to_string().contains("outside the ranges it declares"), "{r}");
}

// WRITE-21, the step 6 design's 10.3: the number an inverse writes back is read as the code spells
// it, so undo is offered for every spelling it can undo.
#[test]
fn numbers_read_as_javascript_spells_them() {
    let read = [
        ("0x320", 800.0),
        ("0X6e", 110.0),
        ("8_00", 800.0),
        ("0o156", 110.0),
        ("0b1101110", 110.0),
        (".5", 0.5),
        ("5.", 5.0),
        ("1e3", 1000.0),
        ("1.5E-2", 0.015),
        ("- 6", -6.0),
        ("(2)", 2.0),
        ("0xFFFFFFFFFFFFFFFFF", 295147905179352830000.0),
    ];
    for (t, v) in read {
        assert_eq!(super::js_number(t), Some(v), "{t}");
    }
    for t in ["", "_1", "1_", "1__0", "0x", "0x_1", "abc", "inf", "NaN", "1n", "0x1g"] {
        assert_eq!(super::js_number(t), None, "{t}");
    }
}

// WRITE-17, FMT-20: a lock whose `.music2/` was removed while held (`git clean -fdx`) excludes
// nobody, so a second opener takes a new one; the first finds that at its next write and turns
// read-only naming the holder, never writing beside it. Alone, it takes the lock again, and
// `.music2/` with it.
#[test]
fn a_lock_whose_folder_was_removed_is_taken_again() {
    let d = Dir::first_song();
    let mut a = d.open();
    a.commit(pan(&d, 0.4)).unwrap();
    std::fs::remove_dir_all(d.0.join(".music2")).unwrap();
    let mut b = d.open();
    assert!(b.access().writer(), "the lock the first holds is no project's any more");
    b.commit(pan(&d, 0.5)).unwrap();
    let r = a.commit(pan(&d, 0.6)).unwrap_err();
    assert_eq!(codes(&r), ["lock.held"], "{r}");
    assert!(!a.access().writer() && a.problems().iter().any(|p| p.code == "lock.held"));
    assert!(d.read("song.ts").contains("pan: 0.5"));
    drop(b);
    assert!(a.retry_lock() && a.refresh());
    std::fs::remove_dir_all(d.0.join(".music2")).unwrap();
    a.commit(pan(&d, 0.6)).unwrap();
    assert!(d.0.join(".music2/lock").exists() && d.read(".music2/.gitignore") == "*\n", "made again");
    assert!(!d.open().access().writer(), "and it excludes others again");
}

// WRITE-6 (rule 5, C1), WRITE-21: a clip that starts playing an asset that is there, or the last
// clip playing one going, changes the lane, not the asset: an asset no lane names is only listed,
// its bytes known on the side that names it alone. Confined to the lane, both commit, and both
// undo.
#[test]
fn naming_an_asset_or_no_longer_is_the_lanes_change() {
    let d = Dir::copy_of("examples/resample");
    std::fs::copy(d.0.join("assets/keys-bounce.wav"), d.0.join("assets/other.wav")).unwrap();
    let mut s = d.open();
    let lane = LaneAddr::new("chops", "clips").unwrap();
    let file = "data/chops/clips.json";
    let text = d.read(file);
    let clips: Vec<Value> = serde_json::from_str::<Value>(&text).unwrap()["clips"].as_array().unwrap().clone();
    let confined = || Expect::Confined(Confined { lanes: vec![lane.clone()], ..Confined::default() });
    // A copy of the first clip playing other.wav, whose bytes (and so its pin) are the same.
    let mut put = clips[0].as_object().unwrap().clone();
    put.insert("id".into(), json!("zz9other"));
    put.insert("asset".into(), json!("other.wav"));
    let op = Op::Clips { lane: lane.clone(), put: vec![Value::Object(put)], delete: vec![] };
    let named = s.commit(based(&d, vec![op], confined())).unwrap();
    assert!(named.delta.lanes == [lane.clone()] && named.delta.assets.is_empty(), "{:?}", named.delta);
    s.undo().unwrap();
    assert_eq!(d.read(file), text);
    let ids: Vec<String> = clips.iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    let gone = s.commit(based(&d, vec![Op::Clips { lane: lane.clone(), put: vec![], delete: ids }], confined())).unwrap();
    assert!(gone.delta.lanes == [lane] && gone.delta.assets.is_empty(), "{:?}", gone.delta);
    s.undo().unwrap();
    assert_eq!(d.read(file), text);
}
