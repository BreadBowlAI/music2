//! WRITE-21a: a raw step (a save's, an outside step's, an upgrade's) whose undo or redo makes every
//! file it touches the revision that plays goes ahead though the files on disk don't build; a
//! judged step keeps the draft rule. And a save that lands while the code is broken (WRITE-14's data
//! path) is one undo step (WRITE-2, AGENT-1). From the upgrade track's second review (G2-1 to G2-4).

use std::path::Path;

use m2_song::Snapshot;
use m2_song::tx::{Base, Expect, Op, Payload};

use super::tests::{Dir, based, p};
use crate::StepError;

// WRITE-2, WRITE-14, AGENT-1: an agent's save of a lane file while the code on disk doesn't build is
// taken into the revision that plays as written, so it is one undo step, the change AGENT-3 takes
// back.
#[test]
fn an_agents_data_save_while_the_code_is_broken_is_one_step() {
    let d = Dir::first_song();
    let mut s = d.open();
    let kick = Path::new("tracks/kick.ts");
    let good = d.read("tracks/kick.ts");
    let broken = good.replace("ms(150)", "ms(150");
    assert!(!s.save(kick, Base::text(&good), &broken).unwrap().recorded, "the musician's mistake: no step");
    assert!(!s.draft_errors.is_empty());
    let steps = s.last_step();
    let hits = Path::new("data/kick/hits.json");
    let old = d.read("data/kick/hits.json");
    let new = old.replacen("\"velocity\": 0.8", "\"velocity\": 0.7", 1);
    let saved = s.save_built(hits, Base::text(&old), &new).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(d.read("data/kick/hits.json"), new, "written");
    assert_eq!(s.accepted().snapshot.files.get(hits).map(|t| t.to_string()), Some(new.clone()), "in the revision that plays");
    assert!(saved.recorded, "AGENT-1: an agent's save that lands is one undo step");
    assert_eq!(s.last_step(), steps + 1, "one step");
    // AGENT-3's take-back is of this save, the last change.
    s.retract().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(d.read("data/kick/hits.json"), old, "taken back");
}

// WRITE-2, WRITE-14: the code editor's save of a lane file while the code is broken compiles against
// the accepted code, so it is recorded for undo, as ever.
#[test]
fn a_code_editor_data_save_while_the_code_is_broken_is_one_step() {
    let d = Dir::first_song();
    let mut s = d.open();
    let kick = Path::new("tracks/kick.ts");
    let good = d.read("tracks/kick.ts");
    let broken = good.replace("ms(150)", "ms(150");
    assert!(!s.save(kick, Base::text(&good), &broken).unwrap().recorded);
    let steps = s.last_step();
    let hits = Path::new("data/kick/hits.json");
    let old = d.read("data/kick/hits.json");
    let new = old.replacen("\"velocity\": 0.8", "\"velocity\": 0.7", 1);
    let saved = s.save(hits, Base::text(&old), &new).unwrap();
    assert_eq!(s.accepted().snapshot.files.get(hits).map(|t| t.to_string()), Some(new.clone()));
    assert!(saved.recorded, "recorded, as before");
    assert_eq!(s.last_step(), steps + 1);
    assert!(s.can_undo());
    s.undo().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(d.read("data/kick/hits.json"), old);
}

fn git(d: &Dir, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(&d.0)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
}

/// A song whose `tracks/kick.ts` reads `../util`, as a step (made by `step`) left it; then git puts
/// kick.ts back and takes util.ts away (no step), and the musician writes kick.ts as the step left
/// it in another editor: the files on disk don't build, and the step's undo would make them the
/// revision that plays. The session, and kick.ts as the step left it.
fn landing(step: impl FnOnce(&Dir, &mut crate::Session, &str, &str)) -> (Dir, crate::Session, String) {
    let d = Dir::first_song();
    d.write("util.ts", "export const release = 150;\n");
    let x = d.read("tracks/kick.ts");
    git(&d, &["init", "-q"]);
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "c1"]);
    let mut s = d.open();
    assert!(s.accepted().has_code(), "{:?}", s.draft_errors);
    let y = x
        .replace("import { follow } from \"std/dsp\";\n", "import { follow } from \"std/dsp\";\nimport { release } from \"../util\";\n")
        .replace("ms(150)", "ms(release)");
    step(&d, &mut s, &x, &y);
    assert!(s.can_undo());
    d.write("tracks/kick.ts", &x);
    std::fs::remove_file(d.0.join("util.ts")).unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "c2"]);
    let paths = [Path::new("tracks/kick.ts").to_path_buf(), Path::new("util.ts").to_path_buf()].into_iter().collect();
    let (taken, _) = s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths, true);
    assert!(taken.accepted.is_some(), "{:?}", s.draft_errors);
    d.write("tracks/kick.ts", &y);
    let paths = [Path::new("tracks/kick.ts").to_path_buf()].into_iter().collect();
    s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths, false);
    assert!(!s.draft_errors.is_empty(), "kick.ts doesn't build without util.ts");
    (d, s, y)
}

// WRITE-21a: a judged step keeps the draft rule: while the files on disk don't build, its undo is
// refused for that (not offered, then refused as a conflict and marked stuck).
#[test]
fn a_judged_step_that_lands_keeps_the_draft_rule() {
    let (d, mut s, y) = landing(|d, s, _, y| {
        let h = based(d, vec![Op::Write { file: p("tracks/kick.ts"), bytes: Payload::Text(y.into()) }], Expect::Identical);
        s.commit(h).unwrap_or_else(|e| panic!("H: {e:?}"));
    });
    assert!(!s.can_undo(), "the draft rule: the code doesn't build");
    assert!(matches!(s.undo(), Err(StepError::DraftInvalid(_))));
    assert!(!s.undo_stuck(), "not stuck: it can be undone once the code builds");
    assert_eq!(d.read("tracks/kick.ts"), y, "nothing written");
}

// WRITE-21a: a raw step (the code editor's save) whose undo makes the files the revision that plays
// goes ahead, and its Before (Compare's) is that revision.
#[test]
fn a_raw_step_that_lands_goes_ahead_and_its_before_is_what_plays() {
    let (d, mut s, _) = landing(|_, s, x, y| {
        assert!(s.save(Path::new("tracks/kick.ts"), Base::text(x), y).unwrap().recorded);
    });
    assert!(s.can_undo(), "it lands on the revision that plays");
    let before = s.undone().unwrap_or_else(|e| panic!("its Before: {e:?}"));
    assert!(std::ptr::eq(&*before, s.accepted()), "its Before is the revision that plays");
    let playing = s.accepted().snapshot.files.get(Path::new("tracks/kick.ts")).map(|t| t.to_string());
    s.undo().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(Some(d.read("tracks/kick.ts")), playing, "the files are the revision that plays");
    assert!(s.draft_errors.is_empty(), "and build");
}
