//! Lane edits: a notes lane's clips, rewritten as a whole, as the piano roll and the
//! timeline change them.
//!
//! The lane's file is written in its canonical form, by `core/song`'s writer (FMT-5, FMT-8a),
//! so a change diffs as what it is. A clip given without an id gets one here, as a builder
//! outside a sandbox may draw it (FMT-11). Like every edit it is compiled as a candidate first:
//! the lane must be a notes lane the accepted revision declares, the candidate's graph must be
//! exactly the accepted one (lane data never changes it), and the file must read back as
//! exactly the clips written. Undo restores the file exactly, removing it if the edit created it.

use std::collections::BTreeSet;
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;

use m2_ir::Kind;
use m2_song::{Arrangement, ClipId, DataPath, LaneAddr, LaneData, Note, NoteClip, canonical, lane::number, lane::pitch_name};

use crate::edit::{EditError, Edited, Session};

impl Session {
    /// Replace the clips of `module`'s notes lane `lane`, as edited at revision `at`. If
    /// the lane's file changed since, the edit is refused: it would overwrite that change.
    /// Clips are written in order of start and id, and each clip's notes in order of start and pitch.
    pub fn write_notes(&mut self, at: u64, module: &str, lane: &str, clips: &[NoteClip]) -> Result<Edited, EditError> {
        self.refresh();
        if !self.draft_errors.is_empty() {
            return Err(EditError::DraftInvalid(self.draft_errors.clone()));
        }
        let rev = self.accepted();
        let m = rev.compiled.ir.modules.iter().find(|m| m.key == module).ok_or_else(|| EditError::NotFound(module.to_string()))?;
        let decl = m.lanes.iter().find(|l| l.key == lane).ok_or_else(|| EditError::NotFound(format!("lane `{lane}` of {module}")))?;
        match decl.kind {
            Kind::Events(_) => {}
            Kind::Control(_) => return Err(EditError::Rejected(format!("`{lane}` is an automation lane, not a notes lane"))),
            Kind::Audio(_) => return Err(EditError::Rejected(format!("`{lane}` is an audio lane, not a notes lane"))),
        }
        let addr = LaneAddr::new(module, lane).map_err(EditError::Rejected)?;
        let file = file(&addr);
        let now = rev.snapshot.files.get(&file);
        if at != rev.number && self.snapshot(at).map(|s| s.files.get(&file)) != Some(now) {
            return Err(EditError::Rejected(format!("{} changed since; edit the lane as it is now", file.display())));
        }
        // A file that doesn't read plays the data the lane had before (FMT-9), which is what the
        // edit was made on: writing it would replace the file's newer text with that old copy.
        let shown = DataPath::lane(&addr).to_string();
        if let Some(d) = rev.compiled.problems.iter().find(|d| d.src.as_deref() == Some(&shown) && d.code.starts_with("lane.")) {
            return Err(EditError::Rejected(format!(
                "{shown} doesn't read as the lane right now, so the notes shown are the ones it played before; fix the file first ({})",
                d.code
            )));
        }
        let clips = tidy(clips, &addr)?;
        let text = canonical(&LaneData::Notes(clips.clone()));
        if now == Some(&text) {
            let same = "no change".to_string();
            return Ok(Edited { revision: rev.number, file: file.display().to_string(), before: same.clone(), after: same });
        }
        let old = match rev.compiled.data.lane(module, lane).map(|d| &**d) {
            Some(LaneData::Notes(c)) => c.clone(),
            _ => vec![],
        };
        let (before, after) = describe(&old, &clips, &rev.project.arrangement);
        self.write_file(&file, Some(&text), (&before, &after), |accepted, _, compiled| {
            let read = match compiled.data.lane(module, lane).map(|d| &**d) {
                Some(LaneData::Notes(c)) => Some(c),
                _ => None,
            };
            if compiled.ir != accepted.compiled.ir || read != Some(&clips) {
                return Err(EditError::Rejected(format!("writing {} would change more than the lane's notes", file.display())));
            }
            Ok(())
        })
    }
}

/// Where a lane is written: `data/<module>/<lane>.json`, as `DataPath` spells it.
pub(crate) fn file(addr: &LaneAddr) -> PathBuf {
    DataPath::lane(addr).path()
}

/// The clips as they are written: checked, each with an id, in canonical order.
fn tidy(clips: &[NoteClip], addr: &LaneAddr) -> Result<Vec<NoteClip>, EditError> {
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
    // one of its own, drawn at random as a builder outside a sandbox may (FMT-11, FMT-12).
    let mut taken = BTreeSet::new();
    let fresh: Vec<bool> =
        clips.iter().map(|c| !c.id.as_deref().is_some_and(|i| ClipId::valid(i) && taken.insert(i.to_string()))).collect();
    let tx = format!("tx-{:016x}", std::collections::hash_map::RandomState::new().build_hasher().finish());
    let mut n = 0;
    for (c, _) in clips.iter_mut().zip(fresh).filter(|(_, fresh)| *fresh) {
        let id = loop {
            let id = ClipId::derive(&tx, addr, n);
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
fn place(arrangement: &Arrangement, beat: f64) -> String {
    let (bar, at) = arrangement.bar_at(beat);
    format!("bar {} beat {}", bar + 1, number(at + 1.0))
}

/// What an edit of a lane changed, to show it and its undo: the clips or notes only one
/// side has. `C4 at bar 1 beat 2` → `D4 at bar 1 beat 4`; `no clip` → `clip at bar 10 (1 bar)`.
fn describe(before: &[NoteClip], after: &[NoteClip], arrangement: &Arrangement) -> (String, String) {
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
    }
    (was, is)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use m2_runtime::PackageSet;
    use m2_song::{Note, Tempo};

    use super::*;

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
            copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-song"), &fixture.0);
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
        fixture.write("music2.toml", crate::tests::MANIFEST);
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

        // A change to the lane itself since revision 3 is never overwritten.
        let changed = fixture.read("data/bass/notes.json").unwrap().replacen("\"velocity\": 0.5", "\"velocity\": 0.25", 1);
        fixture.write("data/bass/notes.json", &changed);
        edited[0].notes[0].velocity = 1.0;
        assert!(matches!(s.write_notes(3, "bass", "notes", &edited), Err(EditError::Rejected(why)) if why.contains("changed since")));
        assert_eq!(fixture.read("data/bass/notes.json").unwrap(), changed);
        // Nor undone over.
        assert!(s.undo().is_err());
        assert_eq!(fixture.read("data/bass/notes.json").unwrap(), changed);
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

        // The code editor saves another file: an edit made here, like any other.
        let commented = song.replacen('\n', " // my first edit\n", 1);
        fixture.write("song.ts", &commented);
        assert!(s.saved(Path::new("song.ts"), Some(song.clone())));
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
        fixture.write("song.ts", "export default (");
        s.saved(Path::new("song.ts"), Some(commented.clone()));
        assert!(!s.can_undo(), "nothing can be undone while the code doesn't compile");
        fixture.write("song.ts", &commented);
        s.saved(Path::new("song.ts"), Some(commented.clone()));
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
        assert_eq!(d(&[], &one), "no clip → clip at bar 2 (2 bars)");
        assert_eq!(d(&one, &[clip(36.0, 8.0, vec![c4.clone()])]), "clip at bar 2 (2 bars) → clip at bar 10 (2 bars)");
        assert_eq!(d(&one, &[clip(4.0, 4.0, vec![c4])]), "clip at bar 2 (2 bars) → clip at bar 2 (1 bar)");
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
