//! Data a rename left behind: the warning, moving it where it belongs, deleting it, and undo.

use std::path::{Path, PathBuf};

use m2_runtime::PackageSet;

use super::*;
use crate::{Fix, TimelineLane};

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
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-song"), &dir);
        Self(dir)
    }

    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(name)).ok()
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.0.join(name), text).unwrap();
    }

    fn open(&self) -> Session {
        let mut packages = PackageSet::default();
        packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        Session::open(&self.0, packages).unwrap_or_else(|d| panic!("{d:?}"))
    }

    /// Rename the Kick track in its code, from outside the session, and pick it up.
    fn rename_kick(&self, s: &mut Session, to: &str) {
        let kick = self.read("tracks/kick.ts").unwrap();
        self.write("tracks/kick.ts", &kick.replacen("track(\"Kick\"", &format!("track(\"{to}\""), 1));
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
    let found = t
        .tracks
        .iter()
        .find(|t| t.module == module)
        .and_then(|t| t.lanes.iter().find(|l| matches!(l, TimelineLane::Notes { id, .. } if id == lane)));
    match found {
        Some(TimelineLane::Notes { clips, .. }) => clips.iter().map(|c| c.notes.len()).sum(),
        _ => panic!("no notes lane {module} › {lane}"),
    }
}

const OLD: &str = "timeline/kick.hits.json";
const NEW: &str = "timeline/kicks.hits.json";

#[test]
fn a_renamed_tracks_notes_are_moved_where_they_belong_and_undo_moves_them_back() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    assert!(s.warnings().is_empty());
    let bytes = fixture.read(OLD).unwrap();
    let playing = notes(&s, "Kick", "hits");
    assert!(playing > 0);

    fixture.rename_kick(&mut s, "Kicks");
    assert_eq!(notes(&s, "Kicks", "hits"), 0, "the renamed track plays nothing");
    let w = s.warnings();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].file, OLD);
    assert_eq!(
        w[0].to_string(),
        "Notes on Kick › hits aren't playing. There's no track called Kick any more. Did you rename it? Move them to Kicks › hits to \
         hear them again, or delete them."
    );
    assert_eq!(w[0].fixes[0], Fix::Relink { from: OLD.into(), to: NEW.into(), module: "Kicks".into(), lane: "hits".into(), likely: true });

    let at = s.accepted().number;
    let moved = s.relink(at, Path::new(OLD), "Kicks", "hits").unwrap();
    assert_eq!(
        (moved.revision, moved.file.as_str(), moved.before.as_str(), moved.after.as_str()),
        (at + 1, OLD, "unused notes", "Kicks › hits")
    );
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (None, Some(bytes.clone())), "renamed, byte for byte");
    assert_eq!(notes(&s, "Kicks", "hits"), playing, "the track plays its notes again");
    assert!(s.warnings().is_empty() && !s.refresh(), "the session took in the move itself");

    assert!(s.can_undo());
    let undone = s.undo().unwrap();
    assert_eq!((undone.file.as_str(), undone.before.as_str(), undone.after.as_str()), (NEW, "Kicks › hits", "unused notes"));
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (Some(bytes.clone()), None));
    assert_eq!(s.warnings().len(), 1);
    assert_eq!(notes(&s, "Kicks", "hits"), 0);
    s.redo().unwrap();
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (None, Some(bytes)));
    assert_eq!(notes(&s, "Kicks", "hits"), playing);
}

/// On a disk that ignores capitals (macOS by default), `kick.hits.json` already "exists" while
/// the file is named `Kick.hits.json`: the move is a rename of the capitals, not a conflict.
#[test]
fn a_file_named_with_other_capitals_moves_to_its_own_lane_and_back() {
    let fixture = Fixture::first_song();
    let bytes = fixture.read(OLD).unwrap();
    let upper = "timeline/Kick.hits.json";
    std::fs::rename(fixture.0.join(OLD), fixture.0.join("timeline/moving.json")).unwrap();
    std::fs::rename(fixture.0.join("timeline/moving.json"), fixture.0.join(upper)).unwrap();
    let names = || -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(fixture.0.join("timeline")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.retain(|n| n.eq_ignore_ascii_case("kick.hits.json"));
        v
    };
    let mut s = fixture.open();
    assert_eq!(notes(&s, "Kick", "hits"), 0);
    assert!(matches!(&s.warnings()[0].fixes[0], Fix::Relink { to, likely: true, .. } if to == OLD), "{:?}", s.warnings());

    s.relink(s.accepted().number, Path::new(upper), "Kick", "hits").unwrap();
    assert_eq!((names(), fixture.read(OLD)), (vec!["kick.hits.json".to_string()], Some(bytes.clone())));
    assert!(notes(&s, "Kick", "hits") > 0 && s.warnings().is_empty() && !s.refresh());
    assert!(s.can_undo());
    s.undo().unwrap();
    assert_eq!(names(), ["Kick.hits.json"]);
    assert_eq!((notes(&s, "Kick", "hits"), s.warnings().len()), (0, 1));
    s.redo().unwrap();
    assert_eq!((names(), fixture.read(OLD)), (vec!["kick.hits.json".to_string()], Some(bytes)));
}

/// A track that was there beside Kick isn't Kick renamed: with Clap's hits lane as free as
/// Kicks', Kick's notes still plainly belong to Kicks. Just opened, either could be it.
#[test]
fn a_track_that_was_there_beside_is_not_the_renamed_one() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let clap = "import { track } from \"@m2/core\";\nimport { kick } from \"std/instruments\";\n\n\
                export default track(\"Clap\", ({ lane }) => kick(lane.notes(\"hits\")));\n";
    fixture.write("tracks/clap.ts", clap);
    let song = fixture.read("song.ts").unwrap();
    let song =
        song.replacen("import kick from \"./tracks/kick\";", "import kick from \"./tracks/kick\";\nimport clap from \"./tracks/clap\";", 1);
    fixture.write("song.ts", &song.replacen("      kick: kick.out,", "      kick: kick.out,\n      clap: clap.out,", 1));
    assert!(s.refresh() && s.draft_errors.is_empty() && s.warnings().is_empty(), "{:?}", s.draft_errors);
    fixture.rename_kick(&mut s, "Kicks");
    let offered = |s: &Session| -> Vec<(String, bool)> {
        let w = s.warnings().iter().find(|w| w.file == OLD).unwrap();
        w.fixes
            .iter()
            .filter_map(|f| if let Fix::Relink { module, likely, .. } = f { Some((module.clone(), *likely)) } else { None })
            .collect()
    };
    assert_eq!(offered(&s), [("Kicks".to_string(), true), ("Clap".to_string(), false)]);
    let mut both = offered(&fixture.open());
    both.sort();
    assert_eq!(both, [("Clap".to_string(), false), ("Kicks".to_string(), false)]);
}

/// On a disk that ignores capitals, `Kick.hits.json` takes the name `kick.hits.json`: undo
/// doesn't offer to put the data back there, on any disk, and says why when asked.
#[test]
fn a_name_another_file_has_in_other_capitals_is_not_free_for_undo() {
    let upper = "timeline/Kick.hits.json";
    for delete in [false, true] {
        let fixture = Fixture::first_song();
        let mut s = fixture.open();
        let bytes = fixture.read(OLD).unwrap();
        fixture.rename_kick(&mut s, "Kicks");
        let at = s.accepted().number;
        if delete { s.delete_data(at, Path::new(OLD)).map(drop) } else { s.relink(at, Path::new(OLD), "Kicks", "hits").map(drop) }.unwrap();
        fixture.write(upper, r#"{"clips": []}"#);
        assert!(s.refresh() && s.draft_errors.is_empty());
        assert!(!s.can_undo(), "delete: {delete}");
        let refused = s.undo();
        let why = if delete {
            format!("{upper} is there already, and on a disk that ignores capitals it is {OLD}; it isn't replaced")
        } else {
            format!("{NEW} changed since this edit, or {OLD} is taken; move it by hand")
        };
        assert_eq!(refused, Err(EditError::Rejected(why)), "delete: {delete}");
        assert_eq!(fixture.read(upper).as_deref(), Some(r#"{"clips": []}"#));
        // Gone again, it can be undone.
        std::fs::remove_file(fixture.0.join(upper)).unwrap();
        assert!(s.refresh() && s.can_undo(), "delete: {delete}");
        s.undo().unwrap();
        assert_eq!((fixture.read(OLD), fixture.read(NEW)), (Some(bytes), None));
    }
}

/// On a disk that ignores capitals, `Kicks.hits.json` is `kicks.hits.json`: other data isn't
/// moved onto it, and no new file is written over it.
#[test]
fn a_name_another_file_has_in_other_capitals_is_not_free_to_move_or_write_to() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let bytes = fixture.read(OLD).unwrap();
    let upper = "timeline/Kicks.hits.json";
    fixture.write(upper, &bytes);
    fixture.rename_kick(&mut s, "Kicks");
    let w = s.warnings();
    let kick = w.iter().find(|w| w.file == OLD).unwrap();
    assert_eq!(kick.fixes, [Fix::DeleteData { file: OLD.into() }], "Kicks › hits would read {upper}, not this");
    let taken = format!("{upper} is there already, and on a disk that ignores capitals it is {NEW}; it isn't replaced");
    assert_eq!(s.relink(s.accepted().number, Path::new(OLD), "Kicks", "hits"), Err(EditError::Rejected(taken.clone())));
    let clip = m2_song::NoteClip { start: 0.0, length: 4.0, notes: vec![] };
    assert_eq!(s.write_notes(s.accepted().number, "Kicks", "hits", &[clip]), Err(EditError::Rejected(taken)));
    assert_eq!((fixture.read(OLD), fixture.read(upper)), (Some(bytes.clone()), Some(bytes)));
    assert!(!s.can_undo(), "nothing was done");
    // The file in other capitals moves to its own lane.
    s.relink(s.accepted().number, Path::new(upper), "Kicks", "hits").unwrap();
    assert!(fixture.read(NEW).is_some() && notes(&s, "Kicks", "hits") > 0);
}

#[test]
fn undo_moves_data_back_only_while_it_is_unchanged_and_its_old_name_is_free() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    fixture.rename_kick(&mut s, "Kicks");
    s.relink(s.accepted().number, Path::new(OLD), "Kicks", "hits").unwrap();
    let moved = fixture.read(NEW).unwrap();

    // Something takes the old name: the move isn't undone over it, nor offered.
    fixture.write(OLD, r#"{"clips": []}"#);
    assert!(s.refresh());
    assert!(!s.can_undo());
    assert!(matches!(s.undo(), Err(EditError::Rejected(why)) if why.contains("is taken")));
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
    // Claimed data stays where it is, and what isn't lane data is never moved this way.
    let refused = |r: Result<Edited, EditError>, why: &str| {
        r == Err(EditError::Rejected(format!("only lane data no lane reads is moved or deleted this way, and {why}")))
    };
    let notes = "Bass › notes reads timeline/bass.notes.json";
    assert!(refused(s.relink(at, Path::new("timeline/bass.notes.json"), "Kick", "hits"), notes));
    assert!(refused(s.delete_data(at, Path::new("timeline/bass.notes.json")), notes));
    for file in ["song.ts", "tracks/kick.ts", "music2.toml", "timeline/song.json"] {
        assert!(refused(s.delete_data(at, Path::new(file)), &format!("{file} isn't lane data")), "{file}");
        assert!(refused(s.relink(at, Path::new(file), "Kick", "hits"), &format!("{file} isn't lane data")), "{file}");
    }
    let missing = |r: Result<Edited, EditError>| matches!(r, Err(EditError::Rejected(why)) if why.starts_with("the project has no"));
    assert!(missing(s.relink(at, Path::new("timeline/nothing.here.json"), "Kick", "hits")));
    assert!(missing(s.delete_data(at, Path::new("timeline/../timeline/kick.hits.json"))));
    assert!(missing(s.delete_data(at, &fixture.0.canonicalize().unwrap().join(OLD))));

    fixture.rename_kick(&mut s, "Kicks");
    let at = s.accepted().number;
    let old = Path::new(OLD);
    // A lane with data of its own isn't overwritten; one of another kind can't read notes.
    assert!(matches!(s.relink(at, old, "Hats", "hits"), Err(EditError::Rejected(why)) if why.contains("data of its own")));
    assert!(matches!(s.relink(at, old, "Bass", "cutoff"), Err(EditError::Rejected(why)) if why.contains("can't read notes")));
    let nowhere = |r: Result<Edited, EditError>, to: &str| {
        matches!(r, Err(EditError::Rejected(why)) if why == format!("there is no {to} to move it to"))
    };
    assert!(nowhere(s.relink(at, old, "Kicks", "nope"), "lane nope on Kicks"));
    assert!(nowhere(s.relink(at, old, "Nobody", "hits"), "track Nobody"));
    // Planned before the data changed: refused.
    let changed = fixture.read(OLD).unwrap().replacen("\"C1\"", "\"D1\"", 1);
    fixture.write(OLD, &changed);
    assert!(matches!(s.relink(at, old, "Kicks", "hits"), Err(EditError::Rejected(why)) if why.contains("changed since")));
    assert!(matches!(s.delete_data(at, old), Err(EditError::Rejected(why)) if why.contains("changed since")));
    assert_eq!((fixture.read(OLD), fixture.read(NEW)), (Some(changed), None));
    assert!(!s.can_undo(), "nothing was done");
}

/// Automation goes only to a lane whose unit can hold it: hertz never into a 0..1 mask. A
/// session knows the unit each lane had; a project just opened knows only the values.
#[test]
fn automation_is_moved_only_to_a_lane_of_its_unit() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let bass = fixture.read("tracks/bass.ts").unwrap();
    fixture.write("tracks/bass.ts", &bass.replacen("track(\"Bass\"", "track(\"Sub Bass\"", 1));
    assert!(s.refresh() && s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let offered = |s: &Session, file: &str| -> Vec<(String, bool)> {
        let w = s.warnings().iter().find(|w| w.file == file).unwrap();
        w.fixes.iter().filter_map(|f| if let Fix::Relink { lane, likely, .. } = f { Some((lane.clone(), *likely)) } else { None }).collect()
    };
    let (cutoff, drop) = ("timeline/bass.cutoff.json", "timeline/bass.drop.json");
    let one = |lane: &str| vec![(lane.to_string(), true)];
    assert_eq!((offered(&s, cutoff), offered(&s, drop)), (one("cutoff"), one("drop")), "each to its own unit's lane only");
    let at = s.accepted().number;
    let hz = "Sub Bass › drop is a control<unit01> lane, which can't read automation in hz";
    assert_eq!(s.relink(at, Path::new(cutoff), "Sub Bass", "drop"), Err(EditError::Rejected(hz.into())));
    let unit01 = "Sub Bass › cutoff is a control<hz> lane, which can't read automation in unit01";
    assert_eq!(s.relink(at, Path::new(drop), "Sub Bass", "cutoff"), Err(EditError::Rejected(unit01.into())));

    // Just opened: 0..1 values could be hertz, so the mask's data may go to cutoff, but only
    // when the person picks it; values up to 2200 can't be 0..1.
    let mut s = fixture.open();
    assert_eq!(offered(&s, cutoff), one("cutoff"));
    assert_eq!(offered(&s, drop), [("drop".to_string(), true), ("cutoff".to_string(), false)]);
    let reaches = "Sub Bass › drop is a control<unit01> lane, which can't read automation that reaches 2200";
    assert_eq!(s.relink(s.accepted().number, Path::new(cutoff), "Sub Bass", "drop"), Err(EditError::Rejected(reaches.into())));
    assert!(fixture.read("timeline/sub-bass.drop.json").is_none() && fixture.read(cutoff).is_some(), "nothing moved");
}

#[test]
fn deleted_data_comes_back_byte_for_byte_with_undo() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let bytes = fixture.read(OLD).unwrap();
    fixture.rename_kick(&mut s, "Kicks");
    let deleted = s.delete_data(s.accepted().number, Path::new(OLD)).unwrap();
    assert_eq!((deleted.file.as_str(), deleted.before.as_str(), deleted.after.as_str()), (OLD, "unused notes", "deleted"));
    assert_eq!(fixture.read(OLD), None);
    assert!(s.warnings().is_empty());

    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("deleted", "unused notes"));
    assert_eq!(fixture.read(OLD), Some(bytes.clone()));
    assert_eq!(s.warnings().len(), 1);

    // Deleted again, then something else takes the name: undo leaves it be.
    s.redo().unwrap();
    fixture.write(OLD, r#"{"clips": []}"#);
    assert!(s.refresh());
    assert!(!s.can_undo() && s.undo().is_err());
    assert_eq!(fixture.read(OLD).unwrap(), r#"{"clips": []}"#);
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
    assert!(matches!(s.undo(), Err(EditError::Rejected(why)) if why.contains("no longer sets this value")));
    // Back as it was, it can be undone again.
    fixture.write("tracks/bass.ts", &bass);
    assert!(s.refresh() && s.can_undo());
    s.undo().unwrap();
    assert!(fixture.read("tracks/bass.ts").unwrap().contains("khz(2.5)"));
}
