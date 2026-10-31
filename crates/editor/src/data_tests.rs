//! Data a rename left behind: the problem, moving it where it belongs, deleting it, the data
//! fixes, and undo.

use std::path::{Path, PathBuf};

use m2_runtime::PackageSet;
use m2_song::Diagnostic;

use m2_session::{Refusal, Session, StepError};

use super::*;
use crate::{Gestures, TimelineLane};

struct Fixture(PathBuf);

impl Fixture {
    /// A copy of the stable first-song fixture.
    fn first_song() -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-relink-test-{}-{n}", std::process::id()));
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let (f, t) = (e.path(), to.join(e.file_name()));
                if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
            }
        }
        copy(&crate::testing::fixtures().join("first-song"), &dir);
        Self(dir)
    }

    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(name)).ok()
    }

    fn write(&self, name: &str, text: &str) {
        let p = self.0.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn open(&self) -> Session {
        let mut packages = PackageSet::default().with_store(m2_song::pack::test_store());
        packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        Session::open(&self.0, packages).unwrap_or_else(|d| panic!("{d:?}"))
    }

    /// Change the kick track's key in its code, from outside the session, and pick it up. Its
    /// label stays "Kick" (ID-17).
    fn rename_kick(&self, s: &mut Session, to: &str) {
        let kick = self.read("tracks/kick.ts").unwrap();
        self.write("tracks/kick.ts", &kick.replacen("track(\"kick\"", &format!("track(\"{to}\""), 1));
        assert!(s.refresh() && s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How many notes `module`'s lane `lane` plays.
fn notes(s: &Session, module: &str, lane: &str) -> usize {
    let t = s.timeline();
    let found = t.tracks.iter().find(|t| t.module == module).and_then(|t| t.lanes.iter().find(|l| l.key() == lane));
    match found {
        Some(TimelineLane::Notes { clips, .. }) => clips.iter().map(|c| c.notes.len()).sum(),
        _ => panic!("no notes lane {module} › {lane}"),
    }
}

/// The accepted revision's `data.unclaimed` problems.
fn unclaimed(s: &Session) -> Vec<&Diagnostic> {
    s.problems().iter().filter(|d| d.code == "data.unclaimed").collect()
}

/// The problem about `src`.
fn about<'a>(s: &'a Session, src: &str) -> &'a Diagnostic {
    s.problems().iter().find(|d| d.src.as_deref() == Some(src)).unwrap_or_else(|| panic!("nothing about {src}: {:?}", s.problems()))
}

/// Apply fix `n` of the problem about `src`, sent back as a client sends it: its Tx (DIAG-1).
fn fix(s: &mut Session, src: &str, n: usize) -> Result<Edited, EditError> {
    let f = about(s, src).fixes[n].clone();
    s.apply_fix(&f)
}

const OLD: &str = "data/kick/hits.json";
const NEW: &str = "data/kicks/hits.json";

// The step 4 gate, DIAG-6, ID-17: renaming a track key by hand raises `data.unclaimed` on its
// folder, and its Relink restores the sound; undo moves it back.
#[test]
fn a_renamed_tracks_folder_is_moved_where_it_belongs_and_undo_moves_it_back() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    assert!(s.problems().is_empty(), "{:?}", s.problems());
    let bytes = fixture.read(OLD).unwrap();
    let playing = notes(&s, "kick", "hits");
    assert!(playing > 0);

    fixture.rename_kick(&mut s, "kicks");
    assert_eq!(notes(&s, "kicks", "hits"), 0, "the renamed track plays nothing");
    let w = unclaimed(&s);
    assert_eq!(w.len(), 1);
    assert_eq!((w[0].src.as_deref(), w[0].module.as_deref()), (Some("data/kick"), Some("kick")));
    assert_eq!(
        w[0].message,
        "Data filed under Kick (kick) isn't playing. There's no track called Kick (kick) any more, so nothing plays what data/kick \
         holds: notes in hits. Did you rename it? Move its data to Kick (kicks) to hear it again, or delete it."
    );
    let first = &w[0].fixes[0];
    assert_eq!((first.code, first.title.as_str(), first.likely), ("relink", "Move all to Kick (kicks)", true));

    let at = s.accepted().number;
    let moved = fix(&mut s, "data/kick", 0).unwrap();
    assert_eq!(
        (moved.revision, moved.file.as_str(), moved.before.as_str(), moved.after.as_str()),
        (at + 1, "data/kick", "unused data", "Kick (kicks)")
    );
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (None, Some(bytes.clone())), "renamed, byte for byte");
    assert!(!fixture.0.join("data/kick").exists(), "the folder itself moved");
    assert_eq!(notes(&s, "kicks", "hits"), playing, "the track plays its notes again");
    assert!(s.problems().is_empty() && !s.refresh(), "the session took in the move itself");

    assert!(s.can_undo());
    let undone = s.undo().unwrap();
    assert_eq!((undone.file.as_str(), undone.before.as_str(), undone.after.as_str()), ("data/kicks", "Kick (kicks)", "unused data"));
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (Some(bytes.clone()), None));
    assert_eq!(unclaimed(&s).len(), 1);
    assert_eq!(notes(&s, "kicks", "hits"), 0);
    s.redo().unwrap();
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (None, Some(bytes)));
    assert_eq!(notes(&s, "kicks", "hits"), playing);
}

// DIAG-6: the same for a lane key renamed by hand, on its own lane file: the message and the
// fix. The step 4 gate itself, that the Relink restores the sound bit for bit, is
// `gate_tests::relinking_a_renamed_lanes_file_restores_its_sound`.
#[test]
fn a_renamed_lanes_file_is_moved_where_it_belongs() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let playing = notes(&s, "kick", "hits");
    let kick = fixture.read("tracks/kick.ts").unwrap();
    fixture.write("tracks/kick.ts", &kick.replacen("lane.notes(\"hits\")", "lane.notes(\"beats\")", 1));
    assert!(s.refresh() && s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    assert_eq!(notes(&s, "kick", "beats"), 0);
    let w = about(&s, OLD);
    assert_eq!(
        w.message,
        "Notes on Kick (kick) › hits aren't playing. Kick (kick) has no lane called hits any more. Did you rename it? Move them to \
         Kick (kick) › beats to hear them again, or delete them."
    );
    assert_eq!((w.fixes[0].code, w.fixes[0].likely), ("relink", true));
    let moved = fix(&mut s, OLD, 0).unwrap();
    assert_eq!((moved.before.as_str(), moved.after.as_str()), ("unused notes", "kick › beats"));
    assert_eq!(notes(&s, "kick", "beats"), playing);
    assert!(s.problems().is_empty());
}

/// A track that was there beside Kick isn't Kick renamed: with Clap's hits lane as free as
/// Kicks', Kick's notes still plainly belong to Kicks. Just opened, either could be it.
#[test]
fn a_track_that_was_there_beside_is_not_the_renamed_one() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let clap = "import { track } from \"@m2/core\";\nimport { kick } from \"@m2/synths\";\n\n\
                export default track(\"clap\", ({ lane }) => kick(lane.notes(\"hits\")));\n";
    fixture.write("tracks/clap.ts", clap);
    let song = fixture.read("song.ts").unwrap();
    let song =
        song.replacen("import kick from \"./tracks/kick\";", "import kick from \"./tracks/kick\";\nimport clap from \"./tracks/clap\";", 1);
    fixture.write("song.ts", &song.replacen("      kick: kick.out,", "      kick: kick.out,\n      clap: clap.out,", 1));
    assert!(s.refresh() && s.draft_errors.is_empty() && s.problems().is_empty(), "{:?} {:?}", s.draft_errors, s.problems());
    fixture.rename_kick(&mut s, "kicks");
    let offered = |s: &Session| -> Vec<(String, bool)> {
        about(s, OLD).fixes.iter().filter(|f| f.code == "relink").map(|f| (f.title.clone(), f.likely)).collect()
    };
    assert_eq!(offered(&s), [("Move to Kick (kicks) › hits".to_string(), true), ("Move to clap › hits".to_string(), false)]);
    let mut both = offered(&fixture.open());
    both.sort();
    assert_eq!(both, [("Move to Kick (kicks) › hits".to_string(), false), ("Move to clap › hits".to_string(), false)]);
}

#[test]
fn undo_moves_data_back_only_while_it_is_unchanged_and_its_old_name_is_free() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    fixture.rename_kick(&mut s, "kicks");
    s.relink(s.accepted().number, Path::new(OLD), Path::new(NEW)).unwrap();
    let moved = fixture.read(NEW).unwrap();

    // Something takes the old name: the move isn't undone over it, nor offered.
    fixture.write(OLD, r#"{"clips": []}"#);
    assert!(s.refresh());
    assert!(!s.can_undo());
    let taken = |f: &m2_session::ConflictFile| f.file == OLD && f.why.as_deref().is_some_and(|w| w.contains("is there already"));
    assert!(matches!(s.undo(), Err(StepError::Refused(Refusal::Conflict { files })) if files.iter().any(taken)));
    assert_eq!((fixture.read(OLD).unwrap().as_str(), fixture.read(NEW).unwrap()), (r#"{"clips": []}"#, moved.clone()));
    std::fs::remove_file(fixture.0.join(OLD)).unwrap();
    assert!(s.refresh() && s.can_undo(), "free again, it can be undone");

    // The moved notes are edited since: undo won't move the edit away from its lane.
    let edited = moved.replacen("\"C1\"", "\"D1\"", 1);
    assert_ne!(edited, moved);
    fixture.write(NEW, &edited);
    assert!(s.refresh());
    assert!(!s.can_undo() && s.undo().is_err());
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (None, Some(edited)));
}

#[test]
fn only_data_no_lane_reads_is_moved_and_only_to_a_lane_that_reads_it_and_has_none() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let at = s.accepted().number;
    // Claimed data stays where it is, and what isn't data is never moved this way.
    let refused = |r: Result<Edited, EditError>, why: &str| {
        r == Err(EditError::Rejected(format!("only data no lane or plugin reads is moved or deleted this way, and {why}")))
    };
    let to = Path::new("data/kick/other.json");
    let notes = "bass › notes reads data/bass/notes.json";
    assert!(refused(s.relink(at, Path::new("data/bass/notes.json"), to), notes));
    assert!(refused(s.delete_data(at, Path::new("data/bass/notes.json")), notes));
    assert!(refused(s.delete_data(at, Path::new("data/bass")), "Bass (bass) reads data/bass"));
    for file in ["song.ts", "tracks/kick.ts", "music2.toml", "data/arrangement.json"] {
        assert!(refused(s.delete_data(at, Path::new(file)), &format!("{file} isn't lane data")), "{file}");
        assert!(refused(s.relink(at, Path::new(file), to), &format!("{file} isn't lane data")), "{file}");
    }
    let missing = |r: Result<Edited, EditError>| matches!(r, Err(EditError::Rejected(why)) if why.starts_with("the project has no"));
    assert!(missing(s.relink(at, Path::new("data/nothing/here.json"), to)));
    assert!(missing(s.delete_data(at, Path::new("data/../data/kick/hits.json"))));
    assert!(missing(s.delete_data(at, &fixture.0.canonicalize().unwrap().join(OLD))));

    fixture.rename_kick(&mut s, "kicks");
    let at = s.accepted().number;
    let old = Path::new(OLD);
    // A lane with data of its own isn't overwritten; one of another kind can't read notes.
    let rejected = |r: Result<Edited, EditError>, part: &str| matches!(r, Err(EditError::Rejected(why)) if why.contains(part));
    assert!(rejected(s.relink(at, old, Path::new("data/hats/hits.json")), "data of its own"));
    assert!(rejected(s.relink(at, old, Path::new("data/bass/cutoff.json")), "can't read notes"));
    assert!(rejected(s.relink(at, old, Path::new("data/kicks")), "another kind of data"));
    let nowhere = |r: Result<Edited, EditError>, to: &str| r == Err(EditError::Rejected(format!("there is no {to} to move it to")));
    assert!(nowhere(s.relink(at, old, Path::new("data/kicks/nope.json")), "lane nope on kicks"));
    assert!(nowhere(s.relink(at, old, Path::new("data/nobody/hits.json")), "track nobody"));
    // A folder goes only to a module with no data of its own.
    assert!(rejected(s.relink(at, Path::new("data/kick"), Path::new("data/hats")), "data of its own"));
    // Planned before the data changed: refused.
    let changed = fixture.read(OLD).unwrap().replacen("\"C1\"", "\"D1\"", 1);
    fixture.write(OLD, &changed);
    // Taken in (a gesture never reads the folder itself, R2).
    assert!(s.refresh());
    assert!(rejected(s.relink(at, old, Path::new(NEW)), "changed since"));
    assert!(rejected(s.delete_data(at, old), "changed since"));
    assert!(rejected(s.relink(at, Path::new("data/kick"), Path::new("data/kicks")), "changed since"));
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (Some(changed), None));
    assert!(!s.can_undo(), "nothing was done");
}

/// Automation goes only to a lane whose unit can hold it: hertz never into a 0..1 mask. Lane
/// files record their unit (FMT-10), so even a project just opened knows it.
#[test]
fn automation_is_moved_only_to_a_lane_of_its_unit() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let bass = fixture.read("tracks/bass.ts").unwrap();
    fixture.write("tracks/bass.ts", &bass.replacen("track(\"bass\"", "track(\"sub-bass\"", 1));
    assert!(s.refresh() && s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    // The folder whole, then each file to the one lane of its own kind and unit.
    let offered = |s: &Session| -> Vec<(String, bool)> {
        about(s, "data/bass").fixes.iter().filter(|f| f.code == "relink").map(|f| (f.title.clone(), f.likely)).collect()
    };
    let lanes = [
        "Move all to Bass (sub-bass)",
        "Move to Bass (sub-bass) › cutoff",
        "Move to Bass (sub-bass) › drop",
        "Move to Bass (sub-bass) › notes",
    ];
    let expect: Vec<(String, bool)> = lanes.iter().enumerate().map(|(i, t)| (t.to_string(), i == 0)).collect();
    assert_eq!(offered(&s), expect);
    let at = s.accepted().number;
    let (cutoff, drop) = (Path::new("data/bass/cutoff.json"), Path::new("data/bass/drop.json"));
    let hz = "sub-bass › drop is a control<unit01> lane, which can't read automation in hz";
    assert_eq!(s.relink(at, cutoff, Path::new("data/sub-bass/drop.json")), Err(EditError::Rejected(hz.into())));
    let unit01 = "sub-bass › cutoff is a control<hz> lane, which can't read automation in unit01";
    assert_eq!(s.relink(at, drop, Path::new("data/sub-bass/cutoff.json")), Err(EditError::Rejected(unit01.into())));

    // Just opened, the same: the files say their units.
    let s = fixture.open();
    assert_eq!(offered(&s), expect);
    assert!(fixture.read("data/sub-bass/drop.json").is_none() && fixture.read("data/bass/cutoff.json").is_some(), "nothing moved");
}

#[test]
fn deleted_data_comes_back_byte_for_byte_with_undo() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let bytes = fixture.read(OLD).unwrap();
    fixture.rename_kick(&mut s, "kicks");
    let deleted = s.delete_data(s.accepted().number, Path::new(OLD)).unwrap();
    assert_eq!((deleted.file.as_str(), deleted.before.as_str(), deleted.after.as_str()), (OLD, "unused notes", "deleted"));
    assert_eq!(fixture.read(OLD), None);
    assert!(s.problems().is_empty());

    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("deleted", "unused notes"));
    assert_eq!(fixture.read(OLD), Some(bytes.clone()));
    assert_eq!(unclaimed(&s).len(), 1);

    // Deleted again, then something else takes the name: undo leaves it be.
    s.redo().unwrap();
    fixture.write(OLD, r#"{"clips": []}"#);
    assert!(s.refresh());
    assert!(!s.can_undo() && s.undo().is_err());
    assert_eq!(fixture.read(OLD).unwrap(), r#"{"clips": []}"#);

    // A whole folder, deleted and brought back, by a session of its own: the lock is one
    // session's at a time (WRITE-17).
    drop(s);
    let mut s = fixture.open();
    let last = about(&s, "data/kick").fixes.len() - 1;
    let deleted = fix(&mut s, "data/kick", last).unwrap();
    assert_eq!((deleted.file.as_str(), deleted.after.as_str()), ("data/kick", "deleted"));
    assert!(!fixture.0.join("data/kick").exists() && s.problems().is_empty());
    s.undo().unwrap();
    assert_eq!(fixture.read(OLD).unwrap(), r#"{"clips": []}"#);
}

// DIAG-1, FMT-12: a data problem's fix is applied when asked, as a checked edit with one undo
// step; a fix no problem offers any more (its base changed) is refused.
#[test]
fn a_data_problems_fix_is_applied_when_asked_and_undone() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let original = fixture.read(OLD).unwrap();
    let anonymous = original.replacen("\"id\": \"my53vvkc\", ", "", 1);
    assert_ne!(anonymous, original);
    fixture.write(OLD, &anonymous);
    assert!(s.refresh());
    let d = about(&s, OLD);
    assert_eq!((d.code, d.fixes[0].code), ("clip.anonymous", "assign-clip-ids"));
    let stale = d.fixes[0].clone();
    let done = fix(&mut s, OLD, 0).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("kick › hits", "give every clip an id"));
    assert!(s.problems().is_empty(), "{:?}", s.problems());
    let fixed = fixture.read(OLD).unwrap();
    assert!(fixed.contains("\"id\": \""), "{fixed}");
    // A fix whose base is stale is a conflict, as any Tx's (DIAG-1).
    assert!(matches!(s.apply_fix(&stale), Err(EditError::Changed(files)) if files[0].file == OLD));
    s.undo().unwrap();
    assert_eq!(fixture.read(OLD).unwrap(), anonymous);
}

#[test]
fn a_value_whose_units_changed_around_it_is_not_offered_for_undo() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let hi = s.controls().into_iter().find(|c| c.file == "tracks/bass.ts" && c.literal == "2.5").unwrap();
    s.set(hi.revision, &hi.id, 900.0, Some(2500.0)).unwrap();
    // `khz(0.9)` becomes `hz(0.9)`: its bytes are untouched, but undo would write 2.5 Hz, not 2.5 kHz.
    let bass = fixture.read("tracks/bass.ts").unwrap();
    fixture.write("tracks/bass.ts", &bass.replacen("khz(0.9)", "hz(0.9)", 1));
    assert!(s.refresh() && s.draft_errors.is_empty());
    assert!(!s.can_undo());
    // It would write 2.5 Hz, not the 2.5 kHz it was: its `Values` inverse is refused.
    assert!(matches!(s.undo(), Err(StepError::Refused(Refusal::Rejected { .. }))));
    // Back as it was, it can be undone again.
    fixture.write("tracks/bass.ts", &bass);
    assert!(s.refresh() && s.can_undo());
    s.undo().unwrap();
    assert!(fixture.read("tracks/bass.ts").unwrap().contains("khz(2.5)"));
}

// FMT-17, DIAG-6: a plugin state no stateful plugin reads moves with its sidecar to the plugin
// it belongs to now, or is deleted; undo brings the blob back byte for byte.
#[test]
fn plugin_state_is_moved_or_deleted_with_its_sidecar_and_undone() {
    let fixture = Fixture(std::env::temp_dir().join(format!("m2-relink-state-{}", std::process::id())));
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\n";
    // Not installed, so it plays nothing, but its state is still filed under its keys (FMT-18a).
    let song = "import { plugin, song, track } from \"@m2/core\";\n\
                const l = track(\"lead\", ({ lane }) =>\n\
                  plugin(\"clap:org.example.synth\", { key: \"pad\", notes: lane.notes(\"notes\") }));\n\
                export default song(() => l.out);\n";
    let blob: Vec<u8> = (0..=255u8).cycle().take(4000).collect();
    let side = "{\"plugin\": \"clap:org.example.synth\", \"plugin_version\": \"1.0.0\"}\n";
    fixture.write("music2.toml", manifest);
    fixture.write("song.ts", song);
    std::fs::create_dir_all(fixture.0.join("data/lead")).unwrap();
    std::fs::write(fixture.0.join("data/lead/lead.state"), &blob).unwrap();
    fixture.write("data/lead/lead.state.json", side);
    let mut s = fixture.open();
    let d = about(&s, "data/lead/lead.state");
    assert_eq!((d.code, d.fixes[0].code, d.fixes[0].likely), ("data.unclaimed", "relink", true));
    let moved = fix(&mut s, "data/lead/lead.state", 0).unwrap();
    assert_eq!((moved.before.as_str(), moved.after.as_str()), ("unused plugin state", "lead › pad"));
    assert_eq!(std::fs::read(fixture.0.join("data/lead/pad.state")).unwrap(), blob);
    assert_eq!(fixture.read("data/lead/pad.state.json").unwrap(), side);
    assert!(fixture.read("data/lead/lead.state.json").is_none() && unclaimed(&s).is_empty());
    s.undo().unwrap();
    assert_eq!(std::fs::read(fixture.0.join("data/lead/lead.state")).unwrap(), blob);

    let last = about(&s, "data/lead/lead.state").fixes.len() - 1;
    let deleted = fix(&mut s, "data/lead/lead.state", last).unwrap();
    assert_eq!(deleted.after, "deleted");
    assert!(!fixture.0.join("data/lead").exists());
    s.undo().unwrap();
    assert_eq!(std::fs::read(fixture.0.join("data/lead/lead.state")).unwrap(), blob, "the blob comes back byte for byte");
    assert_eq!(fixture.read("data/lead/lead.state.json").unwrap(), side);
}

// FMT-7, ID-1, DIAG-6: a Relink onto a key whose folder is there in other capitals (`data/Bass`)
// is refused, naming that folder, on every disk alike; nothing moves.
#[test]
fn a_relink_never_moves_into_a_folder_in_other_capitals() {
    let fixture = Fixture::first_song();
    std::fs::rename(fixture.0.join("data/bass"), fixture.0.join("data/Bass")).unwrap();
    fixture.write("data/old-bass/notes.json", &fixture.read("data/Bass/notes.json").unwrap());
    let mut s = fixture.open();
    let to = Path::new("data/bass");
    let refused = s.relink(s.accepted().number, Path::new("data/old-bass"), to);
    assert!(
        matches!(&refused, Err(EditError::Rejected(why)) if why.contains("data/Bass is there already") && why.contains("it is data/bass")),
        "{refused:?}"
    );
    assert!(fixture.read("data/old-bass/notes.json").is_some() && fixture.read("data/Bass/notes.json").is_some());
    let refused = s.relink(s.accepted().number, Path::new("data/old-bass/notes.json"), Path::new("data/bass/notes.json"));
    assert!(matches!(&refused, Err(EditError::Rejected(why)) if why.contains("data/Bass is there already")), "{refused:?}");
}

// FMT-9: data that can't be read can't be checked, so it is never moved or deleted.
#[cfg(unix)]
#[test]
fn data_that_cant_be_read_is_never_moved() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    fixture.rename_kick(&mut s, "kicks");
    fixture.write("data/kick/other.json", "{\"clips\": []}\n");
    std::fs::set_permissions(fixture.0.join("data/kick/other.json"), std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(s.refresh());
    let refused = s.relink(s.accepted().number, Path::new("data/kick"), Path::new("data/kicks"));
    std::fs::set_permissions(fixture.0.join("data/kick/other.json"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(&refused, Err(EditError::Rejected(why)) if why.contains("data/kick/other.json can't be read")), "{refused:?}");
    assert!(fixture.read(OLD).is_some() && fixture.read(NEW).is_none());
    // Nor deleted in part.
    std::fs::set_permissions(fixture.0.join("data/kick/other.json"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let refused = s.delete_data(s.accepted().number, Path::new("data/kick"));
    std::fs::set_permissions(fixture.0.join("data/kick/other.json"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(&refused, Err(EditError::Rejected(why)) if why.contains("can't be read")), "{refused:?}");
    assert!(fixture.read(OLD).is_some());
}

// R22, ID-17: a data-only inverse is confined, when it is undone, to the lanes its files feed: a
// folder's relink is undone after its old key is declared again, moving the notes back to it.
#[test]
fn a_folder_relink_is_undone_after_its_old_key_is_declared_again() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let (bytes, kick) = (fixture.read(OLD).unwrap(), fixture.read("tracks/kick.ts").unwrap());
    let playing = notes(&s, "kick", "hits");
    fixture.rename_kick(&mut s, "kicks");
    fix(&mut s, "data/kick", 0).unwrap();
    assert_eq!(notes(&s, "kicks", "hits"), playing);
    // The key goes back by hand: the moved folder is what nothing plays now.
    fixture.write("tracks/kick.ts", &kick);
    assert!(s.refresh() && s.draft_errors.is_empty());
    assert_eq!(notes(&s, "kick", "hits"), 0);
    assert!(s.can_undo());
    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("Kick (kicks)", "unused data"));
    assert_eq!(fixture.read(OLD), Some(bytes));
    assert_eq!(notes(&s, "kick", "hits"), playing, "the track plays its notes again");
}

// R22: unread data deleted comes back with undo even after a lane reads its path again.
#[test]
fn deleted_data_comes_back_after_its_lane_is_declared_again() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let (bytes, kick) = (fixture.read(OLD).unwrap(), fixture.read("tracks/kick.ts").unwrap());
    fixture.rename_kick(&mut s, "kicks");
    s.delete_data(s.accepted().number, Path::new(OLD)).unwrap();
    fixture.write("tracks/kick.ts", &kick);
    assert!(s.refresh() && s.draft_errors.is_empty());
    assert_eq!(notes(&s, "kick", "hits"), 0);
    s.undo().unwrap();
    assert_eq!(fixture.read(OLD), Some(bytes));
    assert!(notes(&s, "kick", "hits") > 0);
}

// WRITE-1, FMT-17: a plugin's state is saved as one Tx, blob and sidecar together, replacing a
// state that is there; undo puts back what was there, byte for byte.
#[test]
fn a_plugin_state_saved_as_one_tx_is_undone_whole() {
    let fixture = Fixture(std::env::temp_dir().join(format!("m2-plugin-state-{}", std::process::id())));
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\n";
    // Not installed, so it plays nothing, but its state is still filed under its keys (FMT-18a).
    let song = "import { plugin, song, track } from \"@m2/core\";\n\
                const l = track(\"lead\", ({ lane }) =>\n\
                  plugin(\"clap:org.example.synth\", { key: \"pad\", notes: lane.notes(\"notes\") }));\n\
                export default song(() => l.out);\n";
    fixture.write("music2.toml", manifest);
    fixture.write("song.ts", song);
    let mut s = fixture.open();
    let side = m2_song::Sidecar { plugin: "clap:org.example.synth".into(), plugin_version: "1.0.0".into() };
    let (lead, pad) = (m2_song::Key::new("lead").unwrap(), m2_song::Key::new("pad").unwrap());
    let first: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    crate::data::plugin_state(&mut s, &lead, std::slice::from_ref(&pad), first.clone(), &side).unwrap();
    assert_eq!(std::fs::read(fixture.0.join("data/lead/pad.state")).unwrap(), first);
    assert!(fixture.read("data/lead/pad.state.json").unwrap().contains("clap:org.example.synth"));
    // Saved again: it replaces the state, as one step.
    crate::data::plugin_state(&mut s, &lead, std::slice::from_ref(&pad), vec![7; 100], &side).unwrap();
    s.undo().unwrap();
    assert_eq!(std::fs::read(fixture.0.join("data/lead/pad.state")).unwrap(), first, "the state before, byte for byte");
    s.undo().unwrap();
    assert!(!fixture.0.join("data/lead/pad.state").exists() && fixture.read("data/lead/pad.state.json").is_none());
}

// WRITE-12, ID-17, DIAG-1: a folder holding a file that is neither a lane nor a state (a dot file
// aside) can't move whole, so the whole folder isn't offered, since a fix offered is one that
// applies: its files are, one at a time, and the problem names the file. Moved whole by name (the
// CLI's `m2 relink`), it is refused naming the file, and nothing moves. Without the file, the
// folder moves, its dot file with it.
#[test]
fn a_folder_holding_a_file_that_isnt_data_is_not_moved() {
    let fixture = Fixture::first_song();
    fixture.write("data/kick/notes.txt", "hits from the old take\n");
    fixture.write("data/kick/.DS_Store", "");
    let mut s = fixture.open();
    fixture.rename_kick(&mut s, "kicks");
    let problem = about(&s, "data/kick").clone();
    assert!(problem.fixes.iter().all(|f| !f.title.starts_with("Move all") && !f.likely), "{problem:?}");
    assert!(problem.message.contains("data/kick/notes.txt isn't data"), "{}", problem.message);
    fix(&mut s, "data/kick", 0).unwrap();
    assert!(fixture.read(OLD).is_none() && fixture.read(NEW).is_some(), "its first fix moves the lane file alone");
    s.undo().unwrap();
    let at = s.accepted().number;
    let refused = s.relink(at, Path::new("data/kick"), Path::new("data/kicks"));
    assert!(matches!(&refused, Err(e) if e.to_string().contains("data/kick/notes.txt")), "{refused:?}");
    assert!(fixture.read(OLD).is_some() && fixture.read(NEW).is_none(), "nothing moved");
    std::fs::remove_file(fixture.0.join("data/kick/notes.txt")).unwrap();
    s.relink(at, Path::new("data/kick"), Path::new("data/kicks")).unwrap();
    assert!(fixture.read(NEW).is_some() && fixture.0.join("data/kicks/.DS_Store").exists());
}
