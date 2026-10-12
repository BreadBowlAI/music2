//! Lane edits: a notes lane's clips, rewritten as a whole, as the piano roll and the
//! timeline change them.
//!
//! The lane's file is written the way the project's files are (note names, one-space
//! indents, a note's velocity only when it isn't the default), so a change diffs as what
//! it is. Like every edit it is compiled as a candidate first: the lane must be a notes
//! lane the accepted revision declares, the candidate's graph must be exactly the
//! accepted one (lane data never changes it), and the file must read back as exactly the
//! clips written. Undo restores the file exactly, removing it if the edit created it.

use std::path::PathBuf;

use m2_ir::Kind;
use m2_song::{Note, NoteClip, slug};

use crate::edit::{EditError, Edited, Session};

/// What a note written without a velocity plays at (`m2_song` reads it so).
const VELOCITY: f64 = 0.8;

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

impl Session {
    /// Replace the clips of `module`'s notes lane `lane`, as edited at revision `at`. If
    /// the lane's file changed since, the edit is refused: it would overwrite that change.
    /// Clips are written in order of start, and each clip's notes in order of start and pitch.
    pub fn write_notes(&mut self, at: u64, module: &str, lane: &str, clips: &[NoteClip]) -> Result<Edited, EditError> {
        self.refresh();
        if !self.draft_errors.is_empty() {
            return Err(EditError::DraftInvalid(self.draft_errors.clone()));
        }
        let rev = self.accepted();
        let m = rev.compiled.ir.modules.iter().find(|m| m.name == module).ok_or_else(|| EditError::NotFound(module.to_string()))?;
        let decl = m.lanes.iter().find(|l| l.id == lane).ok_or_else(|| EditError::NotFound(format!("lane `{lane}` of {module}")))?;
        match decl.kind {
            Kind::Events(_) => {}
            Kind::Control(_) => return Err(EditError::Rejected(format!("`{lane}` is an automation lane, not a notes lane"))),
            Kind::Audio(_) => return Err(EditError::Rejected(format!("`{lane}` is an audio lane, not a notes lane"))),
        }
        let file = file(module, lane)?;
        let now = rev.snapshot.files.get(&file);
        if at != rev.number && self.snapshot(at).map(|s| s.files.get(&file)) != Some(now) {
            return Err(EditError::Rejected(format!("{} changed since; edit the lane as it is now", file.display())));
        }
        let clips = tidy(clips)?;
        let text = json(&clips);
        if now == Some(&text) {
            let same = "no change".to_string();
            return Ok(Edited { revision: rev.number, file: file.display().to_string(), before: same.clone(), after: same });
        }
        let old = rev.project.timeline.note_clips(module, lane).unwrap_or_default();
        let (before, after) = describe(&old, &clips, rev.project.timeline.beats_per_bar());
        self.write_file(&file, Some(&text), (&before, &after), |accepted, project, compiled| {
            let read = project.timeline.note_clips(module, lane).map_err(|e| EditError::Rejected(format!("{e:#}")))?;
            if compiled.ir != accepted.compiled.ir || read != clips {
                return Err(EditError::Rejected(format!("writing {} would change more than the lane's notes", file.display())));
            }
            Ok(())
        })
    }
}

/// Where a module's lane is written: `timeline/<slug>.<lane>.json`.
fn file(module: &str, lane: &str) -> Result<PathBuf, EditError> {
    if lane.is_empty() || lane.contains(['/', '\\']) {
        return Err(EditError::Rejected(format!("the lane `{lane}` can't be a file name")));
    }
    Ok(PathBuf::from("timeline").join(format!("{}.{lane}.json", slug(module))))
}

/// The clips as they are written: checked, in order of start, their notes in order of
/// start and pitch.
fn tidy(clips: &[NoteClip]) -> Result<Vec<NoteClip>, EditError> {
    let mut clips = clips.to_vec();
    for c in &mut clips {
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
        c.notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
    }
    clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(clips)
}

/// A number as JSON, the way the project's files write it: `0`, `0.5`, `32`.
fn number(x: f64) -> String {
    // Rust's shortest round-trip form, without a sign on zero.
    if x == 0.0 { "0".into() } else { x.to_string() }
}

/// A pitch's name, `33` → `A1`, if it is a whole MIDI note.
fn name(p: f64) -> Option<String> {
    (p.fract() == 0.0 && (0.0..=127.0).contains(&p)).then(|| format!("{}{}", NAMES[p as usize % 12], p as i32 / 12 - 1))
}

/// A pitch as the project's files write it: `33` is `"A1"`. A pitch without a name (not a
/// whole MIDI note) stays a number.
fn pitch(p: f64) -> String {
    name(p).map_or_else(|| number(p), |n| format!("\"{n}\""))
}

/// A notes lane's file: `{"clips": [...]}` with one-space indents and a final newline.
fn json(clips: &[NoteClip]) -> String {
    let mut s = String::from("{\n \"clips\": [");
    for (i, c) in clips.iter().enumerate() {
        s += if i == 0 { "\n" } else { ",\n" };
        s += &format!("  {{\n   \"start\": {},\n   \"length\": {},\n   \"notes\": [", number(c.start), number(c.length));
        for (j, n) in c.notes.iter().enumerate() {
            s += if j == 0 { "\n" } else { ",\n" };
            s += &format!(
                "    {{\n     \"pitch\": {},\n     \"start\": {},\n     \"length\": {}",
                pitch(n.pitch),
                number(n.start),
                number(n.length)
            );
            if n.velocity != VELOCITY {
                s += &format!(",\n     \"velocity\": {}", number(n.velocity));
            }
            s += "\n    }";
        }
        s += if c.notes.is_empty() { "]\n  }" } else { "\n   ]\n  }" };
    }
    s += if clips.is_empty() { "]\n}\n" } else { "\n ]\n}\n" };
    s
}

/// What an edit of a lane changed, to show it and its undo: the clips or notes only one
/// side has. `C4 at bar 1 beat 2` → `D4 at bar 1 beat 4`; `no clip` → `clip at bar 10 (1 bar)`.
fn describe(before: &[NoteClip], after: &[NoteClip], beats_per_bar: u32) -> (String, String) {
    let bpb = f64::from(beats_per_bar.max(1));
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
                    let (bar, bars) = ((c.start / bpb).floor() + 1.0, c.length / bpb);
                    format!("clip at bar {} ({} {})", number(bar), number(bars), if bars == 1.0 { "bar" } else { "bars" })
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
        let (pos, pitch) = (start + n.start, name(n.pitch).unwrap_or_else(|| number(n.pitch)));
        let bar = (pos / bpb).floor();
        format!("{pitch} at bar {} beat {}", number(bar + 1.0), number(pos - bar * bpb + 1.0))
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
    use m2_song::{Note, parse_pitch};

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
            std::fs::write(self.0.join(name), text).unwrap();
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
        s.accepted().project.timeline.note_clips(module, lane).unwrap()
    }

    #[test]
    fn rewriting_a_lane_changes_only_what_changed_in_the_files_own_style() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        // Written back unchanged, every notes file is byte for byte what it was.
        for (module, lane) in [("Bass", "notes"), ("Hats", "hits"), ("Keys", "chords"), ("Kick", "hits")] {
            let original = fixture.read(&format!("timeline/{}.{lane}.json", slug(module))).unwrap();
            assert_eq!(json(&tidy(&clips(&s, module, lane)).unwrap()), original, "{module}.{lane}");
        }
        let original = fixture.read("timeline/bass.notes.json").unwrap();
        let same = s.write_notes(1, "Bass", "notes", &clips(&s, "Bass", "notes")).unwrap();
        assert_eq!(
            (same.revision, same.after.as_str(), s.can_undo()),
            (1, "no change", false),
            "an edit that changes nothing writes nothing"
        );

        let mut edited = clips(&s, "Bass", "notes");
        edited[0].notes[0].length = 0.75;
        let e = s.write_notes(1, "Bass", "notes", &edited).unwrap();
        assert_eq!(
            (e.revision, e.file.as_str(), e.before.as_str(), e.after.as_str()),
            (2, "timeline/bass.notes.json", "A1 at bar 1 beat 1, 0.5 beats long", "A1 at bar 1 beat 1, 0.75 beats long")
        );
        assert_eq!(fixture.read("timeline/bass.notes.json").unwrap(), original.replacen("\"length\": 0.5", "\"length\": 0.75", 1));
        assert_eq!(clips(&s, "Bass", "notes"), edited, "the accepted revision plays it");

        // Undo puts every byte back; redo writes the edit again.
        s.undo().unwrap();
        assert_eq!(fixture.read("timeline/bass.notes.json").unwrap(), original);
        s.redo().unwrap();
        assert_eq!(clips(&s, "Bass", "notes"), edited);
    }

    #[test]
    fn a_new_clip_creates_the_lane_file_and_undoing_it_removes_the_file() {
        let fixture = Fixture::empty();
        fixture.write(
            "song.ts",
            "import { song, track } from \"@m2/core\";\nimport { kick } from \"std/instruments\";\n\
             const k = track(\"Kick\", ({ lane }) => kick(lane.notes(\"hits\")));\nexport default song(() => k.out);\n",
        );
        let mut s = fixture.open();
        assert!(fixture.read("timeline/kick.hits.json").is_none());
        // Given out of order, clips and notes are written in order.
        let written = [
            NoteClip { start: 4.0, length: 4.0, notes: vec![note(24.0, 1.0, 0.25, 0.8), note(25.0, 0.0, 0.25, 1.0)] },
            NoteClip { start: 0.0, length: 2.0, notes: vec![] },
        ];
        let e = s.write_notes(1, "Kick", "hits", &written).unwrap();
        assert_eq!((e.before.as_str(), e.after.as_str()), ("no clip", "2 clips"));
        let expected = r#"{
 "clips": [
  {
   "start": 0,
   "length": 2,
   "notes": []
  },
  {
   "start": 4,
   "length": 4,
   "notes": [
    {
     "pitch": "C#1",
     "start": 0,
     "length": 0.25,
     "velocity": 1
    },
    {
     "pitch": "C1",
     "start": 1,
     "length": 0.25
    }
   ]
  }
 ]
}
"#;
        assert_eq!(fixture.read("timeline/kick.hits.json").unwrap(), expected);
        assert_eq!(clips(&s, "Kick", "hits").len(), 2);

        s.undo().unwrap();
        assert!(fixture.read("timeline/kick.hits.json").is_none(), "the file the edit created is gone");
        assert!(clips(&s, "Kick", "hits").is_empty() && !s.refresh());
        s.redo().unwrap();
        assert_eq!(fixture.read("timeline/kick.hits.json").unwrap(), expected);

        // Emptied, the lane keeps its file.
        s.write_notes(s.accepted().number, "Kick", "hits", &[]).unwrap();
        assert_eq!(fixture.read("timeline/kick.hits.json").unwrap(), "{\n \"clips\": []\n}\n");
    }

    #[test]
    fn only_a_declared_notes_lane_can_be_written() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let before = fixture.read("timeline/bass.cutoff.json").unwrap();
        let one = [NoteClip { start: 0.0, length: 4.0, notes: vec![note(45.0, 0.0, 1.0, 0.8)] }];
        assert!(matches!(s.write_notes(1, "Bass", "cutoff", &one), Err(EditError::Rejected(why)) if why.contains("automation lane")));
        assert!(matches!(s.write_notes(1, "Bass", "melody", &one), Err(EditError::NotFound(_))));
        assert!(matches!(s.write_notes(1, "Nobody", "notes", &one), Err(EditError::NotFound(_))));
        let silent = [NoteClip { start: 0.0, length: 0.0, notes: vec![] }];
        assert!(matches!(s.write_notes(1, "Bass", "notes", &silent), Err(EditError::Rejected(_))));
        let bad = [NoteClip { start: 0.0, length: 4.0, notes: vec![note(f64::NAN, 0.0, 1.0, 0.8)] }];
        assert!(matches!(s.write_notes(1, "Bass", "notes", &bad), Err(EditError::Rejected(_))));
        assert_eq!(fixture.read("timeline/bass.cutoff.json").unwrap(), before);
        assert!(fixture.read("timeline/bass.melody.json").is_none());
        assert_eq!((s.accepted().number, s.can_undo()), (1, false));
    }

    #[test]
    fn an_edit_planned_before_the_lane_changed_is_refused() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let mut edited = clips(&s, "Bass", "notes");
        edited[0].notes[0].velocity = 0.5;

        // A save elsewhere doesn't touch the lane: the edit goes ahead.
        let song = fixture.read("song.ts").unwrap();
        fixture.write("song.ts", &format!("// Edited elsewhere.\n{song}"));
        assert!(s.refresh() && s.accepted().number == 2);
        s.write_notes(1, "Bass", "notes", &edited).unwrap();
        assert!(fixture.read("timeline/bass.notes.json").unwrap().contains("\"velocity\": 0.5"));

        // A change to the lane itself since revision 3 is never overwritten.
        let changed = fixture.read("timeline/bass.notes.json").unwrap().replacen("\"velocity\": 0.5", "\"velocity\": 0.25", 1);
        fixture.write("timeline/bass.notes.json", &changed);
        edited[0].notes[0].velocity = 1.0;
        assert!(matches!(s.write_notes(3, "Bass", "notes", &edited), Err(EditError::Rejected(why)) if why.contains("changed since")));
        assert_eq!(fixture.read("timeline/bass.notes.json").unwrap(), changed);
        // Nor undone over.
        assert!(s.undo().is_err());
        assert_eq!(fixture.read("timeline/bass.notes.json").unwrap(), changed);
    }

    #[test]
    fn every_lane_edit_of_a_long_session_can_be_undone_and_redone() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let original = fixture.read("timeline/bass.notes.json").unwrap();
        // Far more edits than the session remembers revisions for.
        for i in 0..80 {
            let mut edited = clips(&s, "Bass", "notes");
            edited[0].notes[0].velocity = f64::from(i + 5) / 100.0;
            s.write_notes(s.accepted().number, "Bass", "notes", &edited).unwrap_or_else(|e| panic!("edit {i}: {e}"));
        }
        let last = fixture.read("timeline/bass.notes.json").unwrap();
        for i in 0..80 {
            assert!(s.can_undo(), "undo {i} is offered");
            s.undo().unwrap_or_else(|e| panic!("undo {i}: {e}"));
        }
        assert!(!s.can_undo());
        assert_eq!(fixture.read("timeline/bass.notes.json").unwrap(), original);
        for i in 0..80 {
            s.redo().unwrap_or_else(|e| panic!("redo {i}: {e}"));
        }
        assert_eq!(fixture.read("timeline/bass.notes.json").unwrap(), last);
    }

    #[test]
    fn a_save_from_the_code_editor_is_undone_in_turn_and_never_blocks_a_lane_undo() {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let (notes, song) = (fixture.read("timeline/keys.chords.json").unwrap(), fixture.read("song.ts").unwrap());
        let mut edited = clips(&s, "Keys", "chords");
        edited[0].notes.push(note(72.0, 0.0, 1.0, 0.8));
        s.write_notes(1, "Keys", "chords", &edited).unwrap();

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
        assert_eq!(fixture.read("timeline/keys.chords.json").unwrap(), notes);
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
        for name in ["bass.notes.m2tmp", &format!(".bass.notes.json.{}-0.m2tmp", std::process::id())] {
            std::os::unix::fs::symlink(outside.0.join("victim.txt"), fixture.0.join("timeline").join(name)).unwrap();
        }
        let mut edited = clips(&s, "Bass", "notes");
        edited[0].notes[0].velocity = 0.5;
        s.write_notes(1, "Bass", "notes", &edited).unwrap();
        assert_eq!(outside.read("victim.txt").unwrap(), "victim data\n");
        let file = fixture.0.join("timeline/bass.notes.json");
        assert!(!std::fs::symlink_metadata(&file).unwrap().file_type().is_symlink());
        assert!(fixture.read("timeline/bass.notes.json").unwrap().contains("\"velocity\": 0.5"));
    }

    #[test]
    fn an_edit_is_described_by_what_it_changed() {
        let clip = |start: f64, length: f64, notes: Vec<Note>| NoteClip { start, length, notes };
        let c4 = note(60.0, 1.0, 0.25, 0.8);
        let one = [clip(4.0, 8.0, vec![c4.clone()])];
        let d = |a: &[NoteClip], b: &[NoteClip]| {
            let (x, y) = describe(a, b, 4);
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

    #[test]
    fn pitches_are_written_as_names_that_read_back_as_the_same_pitch() {
        for p in 0..=127 {
            let name = pitch(p as f64);
            assert_eq!(parse_pitch(name.trim_matches('"')), Some(p as f64), "{name}");
        }
        assert_eq!([pitch(33.0), pitch(34.0), pitch(60.0), pitch(0.0)], ["\"A1\"", "\"A#1\"", "\"C4\"", "\"C-1\""]);
        // Pitches without a note name stay numbers.
        assert_eq!([pitch(60.5), pitch(128.0), pitch(-1.0)], ["60.5", "128", "-1"]);
        assert_eq!([number(-0.0), number(0.1 + 0.2), number(1e-7)], ["0", "0.30000000000000004", "0.0000001"]);
        // A note written without a velocity plays at the one left out.
        let n: Note = serde_json::from_str(r#"{"pitch": "C4", "start": 0, "length": 1}"#).unwrap();
        assert_eq!(n.velocity, VELOCITY);
    }
}
