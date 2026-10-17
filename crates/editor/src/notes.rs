//! Lane edits: a notes lane's clips, as the piano roll and the timeline change them (WRITE-10:
//! `Confined { lanes }`).
//!
//! The client sends the whole lane, planned at a revision; the lane's file at that revision is
//! the Tx's base (the step 6 design's 12). A lane whose every clip has its own valid id is edited
//! by a `Clips` op putting each clip whose canonical line changed and deleting the ids gone, so
//! an outside change to another clip merges and one to the same clip is a conflict naming it
//! (WRITE-3). A lane with an anonymous, invalid-id or repeated clip has no address for its clips,
//! so it is written whole, and so is a lane file that isn't there yet. Clips given without an id
//! get one derived from the Tx's id (FMT-11). The file is in its canonical form, by `core/song`'s
//! writer (FMT-5, FMT-8a), so a change diffs as what it is. The planner asserts the candidate
//! reads every clip the edit put exactly as written, and none it deleted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use m2_ir::Kind;
use m2_session::Session;
use m2_song::tx::{Base, Confined, Expect, Op, Payload, TxPath};
use m2_song::{Arrangement, ClipId, DataPath, LaneAddr, LaneData, LaneKind, Note, NoteClip, canonical, lane::number, lane::pitch_name};

use crate::plan;
use crate::{EditError, Edited, ready};

/// Each clip by id, for the clips that have one (of an id written twice, the last, as a map
/// keeps it). Two clips are equal exactly when their canonical lines are: every number compares as
/// it is written, `-0` as `0`, and a pitch's name stands for one pitch (WRITE-3).
fn by_id(clips: &[NoteClip]) -> BTreeMap<&str, &NoteClip> {
    clips.iter().filter_map(|c| Some((c.id.as_deref()?, c))).collect()
}

/// A notes lane's clips; none for data of another kind.
fn notes_of(d: &LaneData) -> &[NoteClip] {
    match d {
        LaneData::Notes(c) => c,
        _ => &[],
    }
}

/// Replace the clips of `module`'s notes lane `lane`, as edited at revision `at`. Clips are
/// written in order of start and id, and each clip's notes in order of start and pitch.
pub fn write_notes(s: &mut Session, at: u64, module: &str, lane: &str, clips: &[NoteClip]) -> Result<Edited, EditError> {
    ready(s)?;
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == module).ok_or_else(|| EditError::NotFound(module.to_string()))?;
    let decl = m.lanes.iter().find(|l| l.key == lane).ok_or_else(|| EditError::NotFound(format!("lane `{lane}` of {module}")))?;
    match decl.kind {
        Kind::Events(_) => {}
        Kind::Control(_) => return Err(EditError::Rejected(format!("`{lane}` is an automation lane, not a notes lane"))),
        Kind::Audio(_) => return Err(EditError::Rejected(format!("`{lane}` is an audio lane, not a notes lane"))),
    }
    let addr = LaneAddr::new(module, lane).map_err(EditError::Rejected)?;
    let file = file(&addr);
    let shown = DataPath::lane(&addr).to_string();
    // A file that doesn't read plays the data the lane had before (FMT-9), which is what the
    // edit was made on: writing it would replace the file's newer text with that old copy.
    if let Some(d) = rev.compiled.problems.iter().find(|d| d.src.as_deref() == Some(&shown) && d.code.starts_with("lane.")) {
        return Err(EditError::Rejected(format!(
            "{shown} doesn't read as the lane right now, so the notes shown are the ones it played before; fix the file first ({})",
            d.code
        )));
    }
    // The lane's file as the edit was planned on it.
    let planned_on = s.snapshot(at).ok_or(EditError::Stale { current: rev.number })?;
    let base = planned_on.files.get(&file).cloned();
    let now = rev.snapshot.files.get(&file).cloned();
    if base.is_none()
        && now.is_none()
        && let Some(by) = rev.snapshot.taken(&file)
    {
        return Err(EditError::Rejected(crate::data::there_already(&file, &by)));
    }
    let id = m2_song::tx::random_id();
    let clips = tidy(clips, &addr, &id)?;
    let new = LaneData::Notes(clips.clone());
    let text = canonical(&new);
    let same = || {
        let same = "no change".to_string();
        Ok(Edited { revision: s.accepted().number, file: file.display().to_string(), before: same.clone(), after: same })
    };
    if now.as_deref() == Some(text.as_str()) {
        return same();
    }
    let old = match rev.compiled.data.lane(module, lane).map(|d| &**d) {
        Some(LaneData::Notes(c)) => c.clone(),
        _ => vec![],
    };
    let (before, after) = describe(&old, &clips, &rev.project.arrangement);
    let path = TxPath::lane(&addr);
    // The text the accepted revision read is that read, shared: a client planned on an older
    // revision whose file differs is the one parsed (PLANE-1).
    let was = match &base {
        Some(t) => Some(match rev.compiled.data.parsed(&addr, &LaneKind::Notes, t) {
            Some(d) => d.clone(),
            None => Arc::new(LaneData::read(t, &LaneKind::Notes).map_err(|_| EditError::Stale { current: rev.number })?),
        }),
        None => None,
    };
    let wrote = by_id(&clips);
    // Put and deleted by id where every clip has an address; else the whole lane.
    let (op, put, gone) = match &was {
        Some(was) if was.addressable() => {
            let old = by_id(notes_of(was));
            let put: Vec<String> = wrote.iter().filter(|(id, c)| old.get(*id) != Some(c)).map(|(id, _)| id.to_string()).collect();
            let gone: Vec<String> = old.keys().filter(|id| !wrote.contains_key(*id)).map(|id| id.to_string()).collect();
            if put.is_empty() && gone.is_empty() {
                return same();
            }
            let line = |id: &String| new.lines_of(id).pop().expect("a clip put is in the lane");
            let values = put.iter().map(|id| serde_json::from_str(&line(id)).expect("a canonical line is JSON")).collect();
            (Op::Clips { lane: addr.clone(), put: values, delete: gone.clone() }, put, gone)
        }
        _ => {
            let put = wrote.keys().map(|id| id.to_string()).collect();
            let gone = was.as_ref().map(|w| by_id(notes_of(w)).into_keys().map(str::to_string).collect()).unwrap_or_default();
            (Op::Write { file: path.clone(), bytes: Payload::Text(text.clone().into()) }, put, gone)
        }
    };
    let lanes = Expect::Confined(Confined { lanes: vec![addr.clone()], ..Confined::default() });
    let mut tx = plan::planned(s, (before.clone(), after.clone()), vec![op], lanes);
    tx.id = id;
    // Based on the text it was planned on: the accepted one's, hashed already, unless older (or
    // not UTF-8, which `planned` bases on its bytes).
    if base.as_deref() != now.as_deref() || rev.snapshot.not_utf8.contains_key(&file) {
        tx.base.insert(path, base.as_deref().map_or(Base::Absent, Base::text));
    }
    plan::run(s, tx, |_, checked| {
        // The planner's assertion: every clip it put reads as written, and none it deleted is
        // left; nothing but this lane's data changes (`Confined`).
        let lane = checked.candidate().compiled.data.lane(module, lane);
        let read = lane.map(|d| by_id(notes_of(d))).unwrap_or_default();
        let there = |id: &str| lane.is_some_and(|d| d.clips_with(id) > 0);
        let kept = put.iter().all(|id| read.get(id.as_str()) == wrote.get(id.as_str()))
            && gone.iter().all(|id| wrote.contains_key(id.as_str()) || !there(id));
        match kept {
            true => Ok(()),
            false => Err(EditError::Rejected(format!("writing {} would change more than the lane's notes", file.display()))),
        }
    })
    .map(|done| Edited { revision: done.number, file: file.display().to_string(), before, after })
}

/// Where a lane is written: `data/<module>/<lane>.json`, as `DataPath` spells it.
pub fn file(addr: &LaneAddr) -> PathBuf {
    DataPath::lane(addr).path()
}

/// The clips as they are written: checked, each with an id, in canonical order.
fn tidy(clips: &[NoteClip], addr: &LaneAddr, tx: &str) -> Result<Vec<NoteClip>, EditError> {
    let mut clips = clips.to_vec();
    for c in &clips {
        if !(c.start.is_finite() && c.start >= 0.0 && c.length.is_finite() && c.length > 0.0) {
            return Err(EditError::Rejected(format!("a clip starts at 0 or later and lasts a while, not at {} for {}", c.start, c.length)));
        }
        for n in &c.notes {
            if ![n.pitch, n.start, n.length, n.velocity].iter().all(|x| x.is_finite()) || n.length <= 0.0 || n.velocity < 0.0 {
                return Err(EditError::Rejected(format!(
                    "a note needs a pitch, a start, a length above 0 and a velocity of 0 or more, not {n:?}"
                )));
            }
        }
    }
    // A clip keeps its id. A new clip, or a copy that came with the id of a clip before it, gets
    // one of its own, derived from the Tx's id, which a builder outside a sandbox draws at random
    // (FMT-11, FMT-12).
    let mut taken = BTreeSet::new();
    let fresh: Vec<bool> =
        clips.iter().map(|c| !c.id.as_deref().is_some_and(|i| ClipId::valid(i) && taken.insert(i.to_string()))).collect();
    let mut n = 0;
    for (c, _) in clips.iter_mut().zip(fresh).filter(|(_, fresh)| *fresh) {
        let id = loop {
            let id = ClipId::derive(tx, addr, n);
            n += 1;
            if taken.insert(id.clone()) {
                break id;
            }
        };
        c.id = Some(id);
    }
    let mut data = LaneData::Notes(clips);
    data.sort();
    let LaneData::Notes(clips) = data else { unreachable!("sorted in place") };
    Ok(clips)
}

/// `bar 2 beat 3`, counted from 1, in the arrangement's meter.
pub fn place(arrangement: &Arrangement, beat: f64) -> String {
    let (bar, at) = arrangement.bar_at(beat);
    format!("bar {} beat {}", bar + 1, number(at + 1.0))
}

/// What an edit of a lane changed, to show it and its undo: the clips or notes only one
/// side has. `C4 at bar 1 beat 2` → `D4 at bar 1 beat 4`; `no clip` → `clip at bar 10 (1 bar)`.
pub fn describe(before: &[NoteClip], after: &[NoteClip], arrangement: &Arrangement) -> (String, String) {
    let sorted = |cs: &[NoteClip]| {
        let mut cs = cs.to_vec();
        cs.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.length.total_cmp(&b.length)));
        cs
    };
    let (before, after) = (sorted(before), sorted(after));
    let span = |c: &NoteClip| (c.start, c.length);
    if !before.iter().map(span).eq(after.iter().map(span)) {
        let clips = |a: &[NoteClip], b: &[NoteClip]| {
            let only: Vec<&NoteClip> = a.iter().filter(|c| !b.iter().any(|d| span(d) == span(c))).collect();
            match only.as_slice() {
                [] => "no clip".to_string(),
                [c] => {
                    let bars = c.length / arrangement.meter_at(c.start).bar();
                    format!(
                        "clip at bar {} ({} {})",
                        arrangement.bar_at(c.start).0 + 1,
                        number(bars),
                        if bars == 1.0 { "bar" } else { "bars" }
                    )
                }
                cs => format!("{} clips", cs.len()),
            }
        };
        return (clips(&before, &after), clips(&after, &before));
    }
    // The same clips: the notes only one side has, where they play.
    let only = |a: &[NoteClip], b: &[NoteClip]| -> Vec<(f64, Note)> {
        a.iter().zip(b).flat_map(|(x, y)| x.notes.iter().filter(|n| !y.notes.contains(n)).map(|n| (x.start, n.clone()))).collect()
    };
    let at = |(start, n): &(f64, Note)| {
        format!("{} at {}", pitch_name(n.pitch).unwrap_or_else(|| number(n.pitch)), place(arrangement, start + n.start))
    };
    let list = |ns: &[(f64, Note)]| match ns {
        [] => "no note".to_string(),
        [n] => at(n),
        ns => format!("{} notes", ns.len()),
    };
    let (gone, new) = (only(&before, &after), only(&after, &before));
    let (mut was, mut is) = (list(&gone), list(&new));
    if let ([(_, a)], [(_, b)]) = (gone.as_slice(), new.as_slice())
        && was == is
    {
        // The same note, longer or louder.
        if a.length != b.length {
            was += &format!(", {} beats long", number(a.length));
            is += &format!(", {} beats long", number(b.length));
        }
        if a.velocity != b.velocity {
            was += &format!(", velocity {}%", number((a.velocity * 100.0).round()));
            is += &format!(", velocity {}%", number((b.velocity * 100.0).round()));
        }
    } else if gone.len() > 1 && was == is {
        // As many notes either side ("4 notes"): what tells them apart, so the two sides never read
        // the same when something changed (the page names an undo step only then).
        let (x, y) = apart(&gone, &new, arrangement);
        was += &x;
        is += &y;
    }
    (was, is)
}

/// A note and where its clip starts.
type Placed = (f64, Note);
/// Something about some notes, in words.
type Summary<'a> = dyn Fn(&[Placed]) -> String + 'a;

/// What tells two sets of notes apart, beside their count: their pitches, where the first plays,
/// their lengths, their velocities; the first of these that differs, or `", changed"` after.
fn apart(a: &[Placed], b: &[Placed], arrangement: &Arrangement) -> (String, String) {
    let span = |ns: &[Placed], f: &dyn Fn(&Placed) -> f64| {
        let lo = ns.iter().map(f).fold(f64::INFINITY, f64::min);
        let hi = ns.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
        (lo, hi)
    };
    let range =
        |(lo, hi): (f64, f64), show: &dyn Fn(f64) -> String| if lo == hi { show(lo) } else { format!("{} to {}", show(lo), show(hi)) };
    let pitches = |ns: &[Placed]| format!(", {}", range(span(ns, &|(_, n)| n.pitch), &|p| pitch_name(p).unwrap_or_else(|| number(p))));
    let first = |ns: &[Placed]| format!(" from {}", place(arrangement, span(ns, &|(s, n)| s + n.start).0));
    let lengths = |ns: &[Placed]| format!(", {} beats long", range(span(ns, &|(_, n)| n.length), &number));
    let velocities = |ns: &[Placed]| format!(", velocity {}%", range(span(ns, &|(_, n)| n.velocity), &|v| number((v * 100.0).round())));
    let sides: [&Summary<'_>; 4] = [&pitches, &first, &lengths, &velocities];
    sides.iter().map(|f| (f(a), f(b))).find(|(x, y)| x != y).unwrap_or_else(|| (String::new(), ", changed".into()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use m2_runtime::PackageSet;
    use m2_session::Session;
    use m2_song::{Note, Tempo};

    use super::*;
    use crate::Gestures;

    struct Fixture(PathBuf);

    impl Fixture {
        fn empty() -> Self {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("m2-lanes-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// A copy of the stable first-song fixture.
        fn first_song() -> Self {
            fn copy(from: &Path, to: &Path) {
                std::fs::create_dir_all(to).unwrap();
                for e in std::fs::read_dir(from).unwrap().flatten() {
                    let (f, t) = (e.path(), to.join(e.file_name()));
                    if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
                }
            }
            let fixture = Self::empty();
            copy(&crate::testing::fixtures().join("first-song"), &fixture.0);
            fixture
        }

        fn write(&self, name: &str, text: &str) {
            let p = self.0.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }

        fn read(&self, name: &str) -> Option<String> {
            std::fs::read_to_string(self.0.join(name)).ok()
        }

        fn open(&self) -> Session {
            let mut packages = PackageSet::default();
            packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
            Session::open(&self.0, packages).unwrap_or_else(|d| panic!("{d:?}"))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn note(pitch: f64, start: f64, length: f64, velocity: f64) -> Note {
        Note { pitch, start, length, velocity }
    }

    fn clips(s: &Session, module: &str, lane: &str) -> Vec<NoteClip> {
        match s.accepted().compiled.data.lane(module, lane).map(|d| &**d) {
            Some(LaneData::Notes(c)) => c.clone(),
            _ => vec![],
        }
    }

    // FMT-5, FMT-8a: lanes are written by core/song's canonical writer, so a lane written back
    // unchanged is byte for byte what it was, and an edit changes only what changed.
    #[test]
    fn rewriting_a_lane_changes_only_what_changed() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        for (module, lane) in [("bass", "notes"), ("hats", "hits"), ("keys", "chords"), ("kick", "hits")] {
            let original = fixture.read(&format!("data/{module}/{lane}.json")).unwrap();
            assert_eq!(canonical(&LaneData::Notes(clips(&s, module, lane))), original, "{module}/{lane}");
        }
        let original = fixture.read("data/bass/notes.json").unwrap();
        let same = s.write_notes(1, "bass", "notes", &clips(&s, "bass", "notes")).unwrap();
        assert_eq!(
            (same.revision, same.after.as_str(), s.can_undo()),
            (1, "no change", false),
            "an edit that changes nothing writes nothing"
        );

        let mut edited = clips(&s, "bass", "notes");
        edited[0].notes[0].length = 0.75;
        let e = s.write_notes(1, "bass", "notes", &edited).unwrap();
        assert_eq!(
            (e.revision, e.file.as_str(), e.before.as_str(), e.after.as_str()),
            (2, "data/bass/notes.json", "A1 at bar 1 beat 1, 0.5 beats long", "A1 at bar 1 beat 1, 0.75 beats long")
        );
        let changed = fixture.read("data/bass/notes.json").unwrap();
        assert_eq!(changed.lines().filter(|l| !original.contains(*l)).count(), 1, "one note's line changed:\n{changed}");
        assert_eq!(clips(&s, "bass", "notes"), edited, "the accepted revision plays it");

        // Undo puts every byte back; redo writes the edit again.
        s.undo().unwrap();
        assert_eq!(fixture.read("data/bass/notes.json").unwrap(), original);
        s.redo().unwrap();
        assert_eq!(clips(&s, "bass", "notes"), edited);
    }

    #[test]
    fn a_new_clip_creates_the_lane_file_and_undoing_it_removes_the_file() {
        let fixture = Fixture::empty();
        fixture.write(
            "song.ts",
            "import { song, track } from \"@m2/core\";\nimport { kick } from \"std/instruments\";\n\
             const k = track(\"kick\", ({ lane }) => kick(lane.notes(\"hits\")));\nexport default song(() => k.out);\n",
        );
        fixture.write("music2.toml", crate::testing::MANIFEST);
        let mut s = fixture.open();
        assert!(fixture.read("data/kick/hits.json").is_none());
        // Given out of order, clips and notes are written in order; the new clip without an id
        // gets one (FMT-11), the other keeps its own.
        let written = [
            NoteClip {
                id: None,
                other_id: None,
                start: 4.0,
                length: 4.0,
                notes: vec![note(24.0, 1.0, 0.25, 0.8), note(25.0, 0.0, 0.25, 1.0)],
            },
            NoteClip { id: Some("a1".into()), other_id: None, start: 0.0, length: 2.0, notes: vec![] },
        ];
        let e = s.write_notes(1, "kick", "hits", &written).unwrap();
        assert_eq!((e.before.as_str(), e.after.as_str()), ("no clip", "2 clips"));
        let read = clips(&s, "kick", "hits");
        let id = read[1].id.clone().unwrap();
        assert!(ClipId::valid(&id) && id.len() == 8, "{id}");
        let expected = format!(
            "{{\"clips\": [\n  {{\"id\": \"a1\", \"start\": 0, \"length\": 2, \"notes\": []}},\n  {{\"id\": \"{id}\", \"start\": 4, \"length\": 4, \"notes\": [\n    \
             {{\"pitch\": \"C#1\", \"start\": 0, \"length\": 0.25, \"velocity\": 1}},\n    {{\"pitch\": \"C1\", \"start\": 1, \"length\": 0.25, \"velocity\": 0.8}}\n  ]}}\n]}}\n"
        );
        assert_eq!(fixture.read("data/kick/hits.json").unwrap(), expected);

        s.undo().unwrap();
        assert!(fixture.read("data/kick/hits.json").is_none(), "the file the edit created is gone");
        assert!(clips(&s, "kick", "hits").is_empty() && !s.refresh());
        s.redo().unwrap();
        assert_eq!(fixture.read("data/kick/hits.json").unwrap(), expected);

        // FMT-12: a copy that comes with another clip's id gets one of its own; the first keeps it.
        let mut copied = clips(&s, "kick", "hits");
        copied.push(NoteClip { start: 8.0, ..copied[0].clone() });
        s.write_notes(s.accepted().number, "kick", "hits", &copied).unwrap();
        let ids: Vec<String> = clips(&s, "kick", "hits").into_iter().map(|c| c.id.unwrap()).collect();
        assert_eq!((ids.len(), &ids[..2]), (3, &["a1".to_string(), id.clone()][..]));
        assert!(!ids[..2].contains(&ids[2]) && ClipId::valid(&ids[2]), "{ids:?}");

        // Emptied, the lane keeps its file.
        s.write_notes(s.accepted().number, "kick", "hits", &[]).unwrap();
        assert_eq!(fixture.read("data/kick/hits.json").unwrap(), "{\"clips\": []}\n");
    }

    #[test]
    fn only_a_declared_notes_lane_can_be_written() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let before = fixture.read("data/bass/cutoff.json").unwrap();
        let one = [NoteClip { id: None, other_id: None, start: 0.0, length: 4.0, notes: vec![note(45.0, 0.0, 1.0, 0.8)] }];
        assert!(matches!(s.write_notes(1, "bass", "cutoff", &one), Err(EditError::Rejected(why)) if why.contains("automation lane")));
        assert!(matches!(s.write_notes(1, "bass", "melody", &one), Err(EditError::NotFound(_))));
        assert!(matches!(s.write_notes(1, "nobody", "notes", &one), Err(EditError::NotFound(_))));
        let silent = [NoteClip { id: None, other_id: None, start: 0.0, length: 0.0, notes: vec![] }];
        assert!(matches!(s.write_notes(1, "bass", "notes", &silent), Err(EditError::Rejected(_))));
        let bad = [NoteClip { id: None, other_id: None, start: 0.0, length: 4.0, notes: vec![note(f64::NAN, 0.0, 1.0, 0.8)] }];
        assert!(matches!(s.write_notes(1, "bass", "notes", &bad), Err(EditError::Rejected(_))));
        assert_eq!(fixture.read("data/bass/cutoff.json").unwrap(), before);
        assert!(fixture.read("data/bass/melody.json").is_none());
        assert_eq!((s.accepted().number, s.can_undo()), (1, false));
    }

    // WRITE-3 (the step 6 design's 20.1): an edit planned on an older lane file is a `Clips` Tx
    // on that file: a change since to a clip it edits is never overwritten (a conflict naming the
    // clip, nor undone over); a change to another clip merges.
    #[test]
    fn an_edit_planned_before_the_lane_changed_is_refused() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let mut edited = clips(&s, "bass", "notes");
        edited[0].notes[0].velocity = 0.5;

        // A save elsewhere doesn't touch the lane: the edit goes ahead.
        let song = fixture.read("song.ts").unwrap();
        fixture.write("song.ts", &format!("// Edited elsewhere.\n{song}"));
        assert!(s.refresh() && s.accepted().number == 2);
        s.write_notes(1, "bass", "notes", &edited).unwrap();
        assert!(fixture.read("data/bass/notes.json").unwrap().contains("\"velocity\": 0.5"));

        // A change to the same clip since revision 3 is never overwritten.
        let changed = fixture.read("data/bass/notes.json").unwrap().replacen("\"velocity\": 0.5", "\"velocity\": 0.25", 1);
        fixture.write("data/bass/notes.json", &changed);
        edited[0].notes[0].velocity = 1.0;
        let refused = s.write_notes(3, "bass", "notes", &edited);
        let clip = edited[0].id.clone();
        assert!(matches!(&refused, Err(EditError::Changed(files)) if files[0].clip == clip), "{refused:?}");
        assert_eq!(fixture.read("data/bass/notes.json").unwrap(), changed);
        // Nor undone over.
        assert!(s.undo().is_err());
        assert_eq!(fixture.read("data/bass/notes.json").unwrap(), changed);

        // A change to another clip merges with the edit.
        assert!(s.refresh());
        let mut two = clips(&s, "bass", "notes");
        two.push(NoteClip { id: Some("later".into()), other_id: None, start: 64.0, length: 4.0, notes: vec![note(45.0, 0.0, 1.0, 0.8)] });
        s.write_notes(s.accepted().number, "bass", "notes", &two).unwrap();
        let at = s.accepted().number;
        let other = fixture.read("data/bass/notes.json").unwrap().replacen("\"start\": 64", "\"start\": 72", 1);
        fixture.write("data/bass/notes.json", &other);
        two[0].notes[0].velocity = 0.75;
        s.write_notes(at, "bass", "notes", &two).unwrap();
        let now = fixture.read("data/bass/notes.json").unwrap();
        assert!(now.contains("\"velocity\": 0.75") && now.contains("\"start\": 72"), "both changes: {now}");
    }

    // R6, FMT-12: a lane whose only clip has no id has no address for it, so an edit of one of
    // its notes writes the lane whole: exactly one clip, now with an id.
    #[test]
    fn a_note_edit_in_an_anonymous_clip_keeps_one_clip() {
        let fixture = Fixture::first_song();
        let text = fixture.read("data/bass/notes.json").unwrap();
        fixture.write("data/bass/notes.json", &text.replacen("\"id\": \"db9f9t5s\", ", "", 1));
        let mut s = fixture.open();
        let mut edited = clips(&s, "bass", "notes");
        assert_eq!((edited.len(), edited[0].id.as_deref()), (1, None));
        edited[0].notes[0].velocity = 0.5;
        s.write_notes(s.accepted().number, "bass", "notes", &edited).unwrap();
        let now = clips(&s, "bass", "notes");
        assert_eq!(now.len(), 1, "one clip, never a copy beside the old one");
        assert!(now[0].id.as_deref().is_some_and(ClipId::valid) && now[0].notes[0].velocity == 0.5, "{now:?}");
    }

    // WRITE-21, the step 6 gate: undoing a clip edit after an outside change to that clip is a
    // conflict naming the lane and the clip; after a change to another clip it goes ahead, and the
    // other clip keeps its change.
    #[test]
    fn undoing_a_clip_after_its_outside_change_conflicts() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let mut two = clips(&s, "bass", "notes");
        two.push(NoteClip { id: Some("later".into()), other_id: None, start: 64.0, length: 4.0, notes: vec![note(45.0, 0.0, 1.0, 0.8)] });
        s.write_notes(s.accepted().number, "bass", "notes", &two).unwrap();
        let mut moved = clips(&s, "bass", "notes");
        moved[0].notes[0].velocity = 0.5;
        s.write_notes(s.accepted().number, "bass", "notes", &moved).unwrap();
        // An outside writer changes the same clip.
        let file = fixture.read("data/bass/notes.json").unwrap();
        fixture.write("data/bass/notes.json", &file.replacen("\"velocity\": 0.5", "\"velocity\": 0.25", 1));
        assert!(s.refresh());
        assert!(!s.can_undo());
        let clip = moved[0].id.clone();
        match crate::undo(&mut s) {
            Err(EditError::Changed(files)) => {
                assert_eq!((files[0].file.as_str(), &files[0].clip), ("data/bass/notes.json", &clip));
                assert!(EditError::Changed(files).to_string().contains(&format!("clip `{}`", clip.unwrap())));
            }
            other => panic!("{other:?}"),
        }
        // Taken back, and another clip changed instead: undo goes ahead, and keeps that change.
        let other = file.replacen("\"start\": 64", "\"start\": 72", 1);
        fixture.write("data/bass/notes.json", &other);
        assert!(s.refresh());
        assert!(s.can_undo());
        crate::undo(&mut s).unwrap();
        let now = fixture.read("data/bass/notes.json").unwrap();
        assert!(!now.contains("\"velocity\": 0.5") && now.contains("\"start\": 72"), "{now}");
    }

    // FMT-9, WRITE-12: while a lane's file doesn't read, the lane plays the notes it had, and
    // those are what an edit in the app was made on. Writing them would replace someone's newer
    // text (an agent's new clip), so the edit is refused until the file reads.
    #[test]
    fn a_lane_whose_file_doesnt_read_is_never_written_over() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let edited = |s: &Session| {
            let mut c = clips(s, "kick", "hits");
            c[0].notes[0].velocity = 0.5;
            c
        };
        let agent = fixture.read("data/kick/hits.json").unwrap().replacen(
            "\n]}",
            ",\n  {\"id\": \"agent001\", \"start\": 64, \"length\": 4, \"notes\": [{\"pitch\": \"C1\", \"start\": 0, \"length\": 1, \"velocty\": 1}]}\n]}",
            1,
        );
        fixture.write("data/kick/hits.json", &agent);
        assert!(s.refresh() && s.draft_errors.is_empty());
        assert_eq!(s.problems()[0].code, "lane.invalid");
        let refused = s.write_notes(s.accepted().number, "kick", "hits", &edited(&s));
        assert!(matches!(&refused, Err(EditError::Rejected(why)) if why.contains("fix the file first")), "{refused:?}");
        assert_eq!(fixture.read("data/kick/hits.json").unwrap(), agent, "the agent's edit is still there");
        // Once it reads, the lane is edited as it is now.
        fixture.write("data/kick/hits.json", &agent.replace("velocty", "velocity"));
        assert!(s.refresh() && s.problems().is_empty(), "{:?}", s.problems());
        s.write_notes(s.accepted().number, "kick", "hits", &edited(&s)).unwrap();
        assert!(fixture.read("data/kick/hits.json").unwrap().contains("agent001"));
    }

    // FMT-7, ID-1: a lane's folder in other capitals (`data/Bass`) isn't read, and on a disk that
    // ignores capitals a new file would go into it: the edit says so, on every disk alike,
    // instead of failing as if the file had changed.
    #[test]
    fn a_folder_in_other_capitals_is_named_not_written_into() {
        let fixture = Fixture::first_song();
        std::fs::rename(fixture.0.join("data/bass"), fixture.0.join("data/Bass")).unwrap();
        let mut s = fixture.open();
        assert!(s.problems().iter().any(|d| d.code == "file.unknown" && d.src.as_deref() == Some("data/Bass/notes.json")));
        let one = [NoteClip { id: None, other_id: None, start: 0.0, length: 4.0, notes: vec![note(45.0, 0.0, 1.0, 0.8)] }];
        let refused = s.write_notes(1, "bass", "notes", &one);
        let why = match &refused {
            Err(EditError::Rejected(why)) => why,
            other => panic!("{other:?}"),
        };
        assert!(why.contains("data/Bass is there already") && why.contains("Rename it as its key is spelled"), "{why}");
        assert!(fixture.read("data/Bass/notes.json").is_some() && !s.can_undo());
    }

    #[test]
    fn every_lane_edit_of_a_long_session_can_be_undone_and_redone() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let original = fixture.read("data/bass/notes.json").unwrap();
        // Far more edits than the session remembers revisions for.
        for i in 0..80 {
            let mut edited = clips(&s, "bass", "notes");
            edited[0].notes[0].velocity = f64::from(i + 5) / 100.0;
            s.write_notes(s.accepted().number, "bass", "notes", &edited).unwrap_or_else(|e| panic!("edit {i}: {e}"));
        }
        let last = fixture.read("data/bass/notes.json").unwrap();
        for i in 0..80 {
            assert!(s.can_undo(), "undo {i} is offered");
            s.undo().unwrap_or_else(|e| panic!("undo {i}: {e}"));
        }
        assert!(!s.can_undo());
        assert_eq!(fixture.read("data/bass/notes.json").unwrap(), original);
        for i in 0..80 {
            s.redo().unwrap_or_else(|e| panic!("redo {i}: {e}"));
        }
        assert_eq!(fixture.read("data/bass/notes.json").unwrap(), last);
    }

    #[test]
    fn a_save_from_the_code_editor_is_undone_in_turn_and_never_blocks_a_lane_undo() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let (notes, song) = (fixture.read("data/keys/chords.json").unwrap(), fixture.read("song.ts").unwrap());
        let mut edited = clips(&s, "keys", "chords");
        edited[0].notes.push(note(72.0, 0.0, 1.0, 0.8));
        s.write_notes(1, "keys", "chords", &edited).unwrap();

        // The code editor saves another file: an edit made here, like any other (WRITE-2).
        let commented = song.replacen('\n', " // my first edit\n", 1);
        assert!(s.save(Path::new("song.ts"), Base::text(&song), &commented).unwrap().recorded);
        assert!(s.can_undo());
        let undone = s.undo().unwrap();
        assert_eq!(undone.file, "song.ts");
        assert!(undone.after.starts_with("// The root module") && undone.after.chars().count() <= 48, "{}", undone.after);
        assert_eq!(fixture.read("song.ts").unwrap(), song);
        // Then the notes, as the lane's own edit left them.
        assert!(s.can_undo());
        s.undo().unwrap();
        assert_eq!(fixture.read("data/keys/chords.json").unwrap(), notes);
        s.redo().unwrap();
        s.redo().unwrap();
        assert_eq!(fixture.read("song.ts").unwrap(), commented);

        // A save that doesn't compile isn't accepted, so there is nothing to record; neither
        // is a save that leaves the file as it was.
        s.save(Path::new("song.ts"), Base::text(&commented), "export default (").unwrap();
        assert!(!s.can_undo(), "nothing can be undone while the code doesn't compile");
        s.save(Path::new("song.ts"), Base::text("export default ("), &commented).unwrap();
        assert!(s.can_undo() && !s.can_redo());
        s.undo().unwrap();
        assert_eq!(fixture.read("song.ts").unwrap(), song, "the redone save is the latest change");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_planted_where_a_write_goes_first_is_never_written_through() {
        let fixture = Fixture::first_song();
        let outside = Fixture::empty();
        outside.write("victim.txt", "victim data\n");
        let mut s = fixture.open();
        // The old temporary name, and the first new one.
        for name in ["notes.m2tmp", &format!(".notes.json.{}-0.m2tmp", std::process::id())] {
            std::os::unix::fs::symlink(outside.0.join("victim.txt"), fixture.0.join("data/bass").join(name)).unwrap();
        }
        let mut edited = clips(&s, "bass", "notes");
        edited[0].notes[0].velocity = 0.5;
        s.write_notes(1, "bass", "notes", &edited).unwrap();
        assert_eq!(outside.read("victim.txt").unwrap(), "victim data\n");
        let file = fixture.0.join("data/bass/notes.json");
        assert!(!std::fs::symlink_metadata(&file).unwrap().file_type().is_symlink());
        assert!(fixture.read("data/bass/notes.json").unwrap().contains("\"velocity\": 0.5"));
    }

    #[test]
    fn an_edit_is_described_by_what_it_changed() {
        let clip = |start: f64, length: f64, notes: Vec<Note>| NoteClip { id: None, other_id: None, start, length, notes };
        let c4 = note(60.0, 1.0, 0.25, 0.8);
        let one = [clip(4.0, 8.0, vec![c4.clone()])];
        let d = |a: &[NoteClip], b: &[NoteClip]| {
            let (x, y) = describe(a, b, &Arrangement::default());
            format!("{x} → {y}")
        };
        assert_eq!(d(&one, &[clip(4.0, 8.0, vec![note(62.0, 3.0, 0.25, 0.8)])]), "C4 at bar 2 beat 2 → D4 at bar 2 beat 4");
        assert_eq!(
            d(&one, &[clip(4.0, 8.0, vec![note(60.0, 1.0, 0.75, 0.5)])]),
            "C4 at bar 2 beat 2, 0.25 beats long, velocity 80% → C4 at bar 2 beat 2, 0.75 beats long, velocity 50%"
        );
        assert_eq!(d(&one, &[clip(4.0, 8.0, vec![])]), "C4 at bar 2 beat 2 → no note");
        assert_eq!(
            d(&one, &[clip(4.0, 8.0, vec![c4.clone(), note(60.0, 1.5, 0.25, 0.8), note(64.0, 1.5, 0.25, 0.8)])]),
            "no note → 2 notes"
        );
        // As many notes either side: what tells them apart, never the same words (the page names
        // an undo step only when the two sides differ).
        let run = |f: &dyn Fn(usize) -> Note| [clip(4.0, 8.0, (0..4).map(f).collect())];
        let plain = run(&|k| note(33.0 + k as f64, k as f64 * 0.25, 0.25, 0.8));
        assert_eq!(d(&plain, &run(&|k| note(45.0 + k as f64, k as f64 * 0.25, 0.25, 0.8))), "4 notes, A1 to C2 → 4 notes, A2 to C3");
        assert_eq!(
            d(&plain, &run(&|k| note(33.0 + k as f64, k as f64 * 0.25 + 0.5, 0.25, 0.8))),
            "4 notes from bar 2 beat 1 → 4 notes from bar 2 beat 1.5"
        );
        assert_eq!(
            d(&plain, &run(&|k| note(33.0 + k as f64, k as f64 * 0.25, 1.0 - k as f64 * 0.25, 0.8))),
            "3 notes, 0.25 beats long → 3 notes, 0.5 to 1 beats long"
        );
        let louder = run(&|k| note(33.0 + k as f64, k as f64 * 0.25, 0.25, 0.9));
        assert_eq!(d(&plain, &louder), "4 notes, velocity 80% → 4 notes, velocity 90%");
        let swapped = |a: f64, b: f64| run(&|k| note(33.0 + k as f64, k as f64 * 0.25, 0.25, if k % 2 == 0 { a } else { b }));
        assert_eq!(d(&swapped(0.5, 0.8), &swapped(0.8, 0.5)), "4 notes → 4 notes, changed");
        assert_eq!(d(&[], &one), "no clip → clip at bar 2 (2 bars)");
        assert_eq!(d(&one, &[clip(36.0, 8.0, vec![c4.clone()])]), "clip at bar 2 (2 bars) → clip at bar 10 (2 bars)");
        assert_eq!(d(&one, &[clip(4.0, 4.0, vec![c4])]), "clip at bar 2 (2 bars) → clip at bar 2 (1 bar)");
    }

    /// Each clip's canonical line by id: what the diff and the assertion compared before they
    /// compared clips (WRITE-3).
    fn lines(d: &LaneData) -> BTreeMap<String, String> {
        d.clip_lines().into_iter().filter_map(|(id, line)| Some((id?.to_string(), line))).collect()
    }

    // WRITE-3 (the G11 fix's A.3): the clips put and deleted, found by comparing clips, are the
    // ones whose canonical lines changed, over generated lanes: numbers of every kind, `-0`, named
    // and fractional pitches, ids repeated and missing.
    #[test]
    fn a_note_write_puts_the_clips_whose_lines_changed() {
        let mut x: u64 = 0x5eed_a3a3;
        let mut below = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let numbers = [0.0, -0.0, 0.5, 1.0, 0.1 + 0.2, 0.3, 1e-9, 4.0, 60.0, 60.5, 127.0, 128.0];
        let lane = |below: &mut dyn FnMut(u64) -> u64| -> Vec<NoteClip> {
            (0..below(5))
                .map(|_| NoteClip {
                    id: [None, Some("a"), Some("b"), Some("c")][below(4) as usize].map(str::to_string),
                    other_id: None,
                    start: numbers[below(numbers.len() as u64) as usize],
                    length: numbers[below(numbers.len() as u64) as usize],
                    notes: (0..below(3))
                        .map(|_| {
                            let n = |below: &mut dyn FnMut(u64) -> u64| numbers[below(numbers.len() as u64) as usize];
                            Note { pitch: n(below), start: n(below), length: n(below), velocity: n(below) }
                        })
                        .collect(),
                })
                .collect()
        };
        for case in 0..20_000 {
            let was = LaneData::Notes(lane(&mut below));
            // Often the same lane with one thing changed, so most clips match.
            let mut new = match &was {
                LaneData::Notes(c) if below(2) == 0 => c.clone(),
                _ => lane(&mut below),
            };
            if let Some(c) = new.first_mut() {
                match below(4) {
                    0 => c.start = numbers[below(numbers.len() as u64) as usize],
                    1 => c.notes.iter_mut().for_each(|n| n.pitch = numbers[below(numbers.len() as u64) as usize]),
                    _ => {}
                }
            }
            let mut sorted = LaneData::Notes(new);
            sorted.sort();
            let new = notes_of(&sorted);
            let (old_lines, new_lines) = (lines(&was), lines(&sorted));
            let line_put: Vec<&String> = new_lines.iter().filter(|(id, l)| old_lines.get(*id) != Some(l)).map(|(id, _)| id).collect();
            let line_gone: Vec<&String> = old_lines.keys().filter(|id| !new_lines.contains_key(*id)).collect();
            let (old, wrote) = (by_id(notes_of(&was)), by_id(new));
            let put: Vec<&str> = wrote.iter().filter(|(id, c)| old.get(*id) != Some(c)).map(|(id, _)| *id).collect();
            let gone: Vec<&str> = old.keys().filter(|id| !wrote.contains_key(*id)).copied().collect();
            assert_eq!(put, line_put, "case {case}: {was:?} → {new:?}");
            assert_eq!(gone, line_gone, "case {case}");
        }
    }

    // PLANE-1, WRITE-12, WRITE-3: a lane read once is shared while its text is the file's: the
    // lanes a write doesn't touch are the very data played before, and the one it writes reads as
    // its file. An outside edit between two writes is planned on: one to another clip merges,
    // one to the clip written is a conflict naming it.
    #[test]
    fn a_lane_write_after_an_outside_edit_is_planned_on_the_file() {
        let fixture = Fixture::first_song();
        let file = "data/bass/notes.json";
        // A second clip, so one can change while the other is written.
        let mut two = LaneData::read(&fixture.read(file).unwrap(), &LaneKind::Notes).unwrap();
        let LaneData::Notes(c) = &mut two else { unreachable!() };
        c.push(NoteClip { id: Some("second".into()), start: 32.0, ..c[0].clone() });
        fixture.write(file, &canonical(&two));
        let mut s = fixture.open();
        let read = |s: &Session, m: &str, l: &str| s.accepted().compiled.data.lane(m, l).cloned().unwrap();
        assert!(read(&s, "bass", "notes").addressable());
        let mut edited = clips(&s, "bass", "notes");
        edited[0].notes[0].velocity = 0.5;
        let (keys, hats) = (read(&s, "keys", "chords"), read(&s, "hats", "hits"));
        let done = s.write_notes(s.accepted().number, "bass", "notes", &edited).unwrap();
        assert!(Arc::ptr_eq(&keys, &read(&s, "keys", "chords")) && Arc::ptr_eq(&hats, &read(&s, "hats", "hits")));
        let fresh = || LaneData::read(&fixture.read(file).unwrap(), &LaneKind::Notes).unwrap();
        assert_eq!(format!("{:?}", *read(&s, "bass", "notes")), format!("{:?}", fresh()), "the written lane reads as its file");

        // Another clip changed on disk, not taken in: the next write keeps it.
        let other = edited.len() - 1;
        assert!(other > 0, "two clips at least");
        let text = fixture.read(file).unwrap();
        let mut outside = fresh();
        let LaneData::Notes(c) = &mut outside else { unreachable!() };
        c[other].notes[0].velocity = 0.25;
        fixture.write(file, &canonical(&outside));
        let mut again = clips(&s, "bass", "notes");
        again[0].notes[0].velocity = 0.75;
        s.write_notes(done.revision, "bass", "notes", &again).unwrap();
        let LaneData::Notes(now) = fresh() else { unreachable!() };
        assert_eq!((now[0].notes[0].velocity, now[other].notes[0].velocity), (0.75, 0.25), "both kept");
        assert_eq!(format!("{:?}", *read(&s, "bass", "notes")), format!("{:?}", fresh()));
        assert_ne!(fixture.read(file).unwrap(), text);

        // The clip it writes changed on disk: a conflict, and the file is as the outside left it.
        let mut outside = fresh();
        let LaneData::Notes(c) = &mut outside else { unreachable!() };
        c[0].notes[0].velocity = 1.0;
        fixture.write(file, &canonical(&outside));
        let mut third = clips(&s, "bass", "notes");
        third[0].notes[0].velocity = 0.6;
        let e = s.write_notes(s.accepted().number, "bass", "notes", &third).unwrap_err();
        assert!(e.to_string().contains("changed since the edit was planned"), "{e}");
        assert_eq!(fixture.read(file).unwrap(), canonical(&outside));
    }

    // FMT-13: bars are counted in the arrangement's meter.
    #[test]
    fn places_follow_the_meter() {
        let three =
            Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 120.0 }], meter: vec![m2_song::Meter { beat: 0.0, beats: 3, unit: 4 }] };
        assert_eq!(place(&three, 0.0), "bar 1 beat 1");
        assert_eq!(place(&three, 4.5), "bar 2 beat 2.5");
        assert_eq!(place(&Arrangement::default(), 4.5), "bar 2 beat 1.5");
    }
}
