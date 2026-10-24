//! Undo steps with ids and sources (AGENT-2, track 1 of the agents design): an outside batch that
//! builds is one step, source outside, whose inverse puts back what outside writers changed and
//! leaves the musician's own saves; binaries, links, new assets, git and a read-only window make
//! none; a step that can't be undone any more can be skipped; the session's own writes and saves
//! keep their one step each; ids are never shared, and follow a step through undo and redo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use m2_song::Snapshot;
use m2_song::tx::{Base, Expect, Op, Payload, Tx};

use super::tests::{Dir, p, pan, std_packages};
use crate::take::{Cant, Outside};
use crate::{Refusal, Session, Source, StepError};

fn paths(ps: &[&str]) -> BTreeSet<PathBuf> {
    ps.iter().map(PathBuf::from).collect()
}

/// Take in what the watcher settled on `ps`, as the app's watcher job does.
fn batch(d: &Dir, s: &mut Session, ps: &[&str]) -> Outside {
    s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths(ps), false).1
}

/// The id, source and files of the step on top of undo.
fn top(s: &Session) -> Option<(u64, Source, Vec<String>)> {
    s.step_names().0.map(|n| (n.id, n.source, n.files))
}

/// `song.ts` with the hats' pan at `to`, as an editor saves it.
fn panned(d: &Dir, to: &str) -> String {
    d.read("song.ts").replacen("pan: 0.3", &format!("pan: {to}"), 1)
}

/// Run git in `d`, with the tests' own name and none of this machine's settings.
fn git(d: &Dir, args: &[&str]) {
    let ran = std::process::Command::new("git")
        .current_dir(&d.0)
        .args(["-c", "user.name=m2", "-c", "user.email=m2@example.invalid", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(ran.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ran.stderr));
}

/// `d` as a git repository, everything in it committed.
fn committed(d: &Dir) {
    git(d, &["init", "-q"]);
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "-m", "one"]);
}

// AGENT-2: a text file saved outside the app, whose batch builds, is one undo step with an id,
// source outside, shown by its file. It ends redo, as every new change does.
#[test]
fn an_outside_edit_that_builds_is_one_undo_step() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.4)).unwrap();
    s.undo().unwrap();
    assert!(s.step_names().1.is_some(), "a step to redo");
    let song = panned(&d, "0.5");
    d.write("song.ts", &song);
    let made = batch(&d, &mut s, &["song.ts"]);
    let Outside::Step(id) = made else { panic!("one step: {made:?}") };
    assert!(id > 0, "an id: {id}");
    let name = s.step_names().0.expect("a step");
    assert_eq!((name.id, &name.source, name.made), (id, &Source::Outside, s.accepted().number));
    assert_eq!((name.file.as_str(), name.files.clone()), ("song.ts", vec!["song.ts".to_string()]));
    assert_eq!(name.name, None, "named plainly, by its file");
    assert_eq!(s.history.len(), 1, "one step");
    assert!(s.step_names().1.is_none(), "it ended redo");
    assert!(s.accepted().snapshot.files[Path::new("song.ts")].contains("pan: 0.5"));
}

// AGENT-2: a batch that doesn't build waits, and makes no step. Once a later batch makes it build,
// there is one step, from the last accepted revision to the new one: undo puts both batches back.
#[test]
fn an_outside_batch_that_waits_is_one_step_once_it_builds() {
    let d = Dir::first_song();
    let mut s = d.open();
    let (bass, notes) = (d.read("tracks/bass.ts"), d.read("data/bass/notes.json"));
    d.write("tracks/bass.ts", &format!("{bass}\nconst broken = ;\n"));
    let moved = notes.replacen("\"start\": 0", "\"start\": 0.5", 1);
    assert_ne!(moved, notes);
    d.write("data/bass/notes.json", &moved);
    assert_eq!(batch(&d, &mut s, &["tracks/bass.ts", "data/bass/notes.json"]), Outside::Nothing, "it waits: no step");
    assert!(!s.draft_errors.is_empty() && s.history.is_empty());
    d.write("tracks/bass.ts", &format!("{bass}\n// fixed\n"));
    let Outside::Step(id) = batch(&d, &mut s, &["tracks/bass.ts"]) else { panic!("one step once it builds") };
    let (top_id, source, mut files) = top(&s).unwrap();
    files.sort();
    assert_eq!((top_id, source), (id, Source::Outside));
    assert_eq!(files, vec!["data/bass/notes.json".to_string(), "tracks/bass.ts".to_string()], "both batches, one step");
    assert_eq!(s.history.len(), 1);
    s.undo().unwrap();
    assert_eq!((d.read("tracks/bass.ts"), d.read("data/bass/notes.json")), (bass, notes), "undo puts both back");
}

// AGENT-2: an outside step's inverse writes back each text file it changed or removed, and removes
// each it made. Redo makes the batch's files again.
#[test]
fn undoing_an_outside_edit_puts_back_its_text() {
    let d = Dir::first_song();
    let mut s = d.open();
    let (song, hits) = (d.read("song.ts"), d.read("data/kick/hits.json"));
    d.write("song.ts", &panned(&d, "0.5"));
    std::fs::remove_file(d.0.join("data/kick/hits.json")).unwrap();
    d.write("data/hats/extra.json", r#"{"clips": []}"#);
    let Outside::Step(_) = batch(&d, &mut s, &["song.ts", "data/kick/hits.json", "data/hats/extra.json"]) else { panic!("one step") };
    let after = (d.read("song.ts"), d.0.join("data/kick/hits.json").exists(), d.read("data/hats/extra.json"));
    s.undo().unwrap();
    assert_eq!(d.read("song.ts"), song, "the changed file has its text back");
    assert_eq!(d.read("data/kick/hits.json"), hits, "the removed file is back");
    assert!(!d.0.join("data/hats/extra.json").exists(), "the made file is gone");
    assert!(s.accepted().snapshot.files[Path::new("song.ts")].contains("pan: 0.3"), "and it plays the text before");
    s.redo().unwrap();
    assert_eq!((d.read("song.ts"), d.0.join("data/kick/hits.json").exists(), d.read("data/hats/extra.json")), after);
}

// WRITE-21, AGENT-2: an outside step undoes only while its file is as the batch left it. A later
// change that made no step (git's checkout) means undo is a conflict naming the file, and nothing
// changes.
#[test]
fn undoing_an_outside_edit_after_a_change_without_a_step_conflicts() {
    let d = Dir::first_song();
    committed(&d);
    let mut s = d.open();
    d.write("song.ts", &panned(&d, "0.5"));
    let Outside::Step(id) = batch(&d, &mut s, &["song.ts"]) else { panic!("one step") };
    git(&d, &["checkout", "-q", "--", "song.ts"]);
    let checkout = d.read("song.ts");
    let by_git = s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths(&["song.ts"]), true).1;
    assert_eq!(by_git, Outside::Git(1), "git's batch makes no step");
    match s.undo() {
        Err(StepError::Refused(Refusal::Conflict { files })) => assert_eq!(files[0].file, "song.ts"),
        other => panic!("a conflict naming song.ts: {other:?}"),
    }
    assert_eq!(d.read("song.ts"), checkout, "nothing changed");
    assert_eq!(top(&s).map(|t| t.0), Some(id), "the step stays");
}

// AGENT-2: a step whose undo is refused because its file changed since can be skipped: nothing is
// written, and the step before it undoes. One that undo can take back isn't skipped.
#[test]
fn a_step_whose_file_changed_since_can_be_skipped() {
    let d = Dir::first_song();
    committed(&d);
    let mut s = d.open();
    let bass = d.read("tracks/bass.ts");
    s.save(Path::new("tracks/bass.ts"), Base::text(&bass), &format!("{bass}\n// mine\n")).unwrap();
    let mine = top(&s).unwrap().0;
    assert!(!s.skip_undo(mine), "a step undo can take back isn't skipped");
    d.write("song.ts", &panned(&d, "0.5"));
    let Outside::Step(id) = batch(&d, &mut s, &["song.ts"]) else { panic!("one step") };
    git(&d, &["checkout", "-q", "--", "song.ts"]);
    let checkout = d.read("song.ts");
    assert_eq!(s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths(&["song.ts"]), true).1, Outside::Git(1));
    assert!(s.undo().is_err(), "song.ts changed since");
    assert!(s.undo_stuck(), "the newest step can only be skipped");
    assert!(!s.skip_undo(mine), "only the newest");
    assert!(s.skip_undo(id), "skipped");
    assert_eq!(d.read("song.ts"), checkout, "nothing was written");
    assert_eq!(top(&s).map(|t| t.0), Some(mine), "the step before it is the newest");
    assert!(!s.undo_stuck() && s.can_undo());
    s.undo().unwrap();
    assert_eq!(d.read("tracks/bass.ts"), bass, "and it undoes");
}

// AGENT-2, D5: a batch that changes or removes a binary file makes no step, since its bytes
// before are gone, and says which; the batch is still taken in.
#[test]
fn an_outside_binary_change_makes_no_step() {
    let d = Dir::copy_of("examples/resample");
    let mut s = d.open();
    let wav = std::fs::read(d.0.join("assets/keys-bounce.wav")).unwrap();
    d.write_bytes("assets/keys-bounce.wav", &wav[..4096]);
    let song = d.read("song.ts");
    d.write("song.ts", &format!("{song}\n// and a text change\n"));
    let number = s.accepted().number;
    let made = batch(&d, &mut s, &["assets/keys-bounce.wav", "song.ts"]);
    let cant = |file: &str, others| Outside::Cant { file: file.into(), why: Cant::Bytes, others };
    assert_eq!(made, cant("assets/keys-bounce.wav", 1), "and it says the text file can't be undone either");
    assert!(s.history.is_empty(), "no step");
    assert!(s.accepted().number > number, "taken in all the same");
    std::fs::remove_file(d.0.join("assets/keys-bounce.wav.json")).unwrap();
    assert_eq!(batch(&d, &mut s, &["assets/keys-bounce.wav.json"]), cant("assets/keys-bounce.wav.json", 0), "removed too");
    assert!(s.history.is_empty());
}

// AGENT-2: a batch that only makes files under `assets/` makes no step and says nothing. Beside a
// text change, a new asset stays when the step is undone.
#[test]
fn a_batch_that_only_adds_assets_makes_no_step() {
    let d = Dir::first_song();
    let mut s = d.open();
    d.write_bytes("assets/new-take.wav", b"RIFF");
    assert_eq!(batch(&d, &mut s, &["assets/new-take.wav"]), Outside::Nothing);
    assert!(s.history.is_empty(), "no step");
    d.write_bytes("assets/other-take.wav", b"RIFF");
    let song = d.read("song.ts");
    d.write("song.ts", &panned(&d, "0.5"));
    let Outside::Step(_) = batch(&d, &mut s, &["assets/other-take.wav", "song.ts"]) else { panic!("a step for the text") };
    assert_eq!(top(&s).unwrap().2, vec!["song.ts".to_string()], "only the text file is in it");
    s.undo().unwrap();
    assert_eq!(d.read("song.ts"), song);
    assert!(d.0.join("assets/other-take.wav").exists(), "the new asset stays");
}

// AGENT-2: what git wrote makes no step (git can undo it), and says how many files it changed: the
// watcher saw git's lock, and each file is as git's index has it.
#[test]
fn a_batch_git_wrote_makes_no_step() {
    let d = Dir::first_song();
    committed(&d);
    d.write("song.ts", &panned(&d, "0.5"));
    d.write("tracks/bass.ts", &format!("{}\n// two\n", d.read("tracks/bass.ts")));
    git(&d, &["commit", "-q", "-a", "-m", "two"]);
    let mut s = d.open();
    git(&d, &["checkout", "-q", "HEAD~1", "--", "song.ts", "tracks/bass.ts"]);
    let (taken, made) = s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths(&["song.ts", "tracks/bass.ts"]), true);
    assert_eq!(made, Outside::Git(2));
    assert!(taken.accepted.is_some(), "taken in");
    assert!(s.history.is_empty() && s.step_names().0.is_none(), "no step");
}

// AGENT-2: git running while a batch settles (`git status` takes git's lock, as a shell's prompt
// runs it) doesn't make the batch git's. A save git's index doesn't have is one step.
#[test]
fn a_save_while_git_runs_is_one_step() {
    let d = Dir::first_song();
    committed(&d);
    let mut s = d.open();
    let song = d.read("song.ts");
    d.write("song.ts", &panned(&d, "0.5"));
    let made = s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths(&["song.ts"]), true).1;
    let Outside::Step(id) = made else { panic!("one step: {made:?}") };
    assert_eq!(top(&s).map(|t| (t.0, t.1)), Some((id, Source::Outside)));
    s.undo().unwrap();
    assert_eq!(d.read("song.ts"), song, "undo takes it back");
}

// AGENT-2: the musician's code-editor save that doesn't build yet is theirs. An outside batch that
// makes it build is one step of the outside file alone: undo takes that back, and leaves the save.
#[test]
fn an_outside_batch_that_makes_a_waiting_save_build_leaves_the_save_on_undo() {
    let d = Dir::first_song();
    let mut s = d.open();
    let song = d.read("song.ts");
    let mine = song.replacen(
        "import kick from \"./tracks/kick\";",
        "import kick from \"./tracks/kick\";\nimport { pad } from \"./tracks/pad\";\nvoid pad;",
        1,
    );
    assert_ne!(mine, song);
    let saved = s.save(Path::new("song.ts"), Base::text(&song), &mine).unwrap();
    assert!(!saved.recorded && !s.draft_errors.is_empty(), "it doesn't build yet: no step");
    d.write("tracks/pad.ts", "export const pad = 1;\n");
    let Outside::Step(id) = batch(&d, &mut s, &["tracks/pad.ts"]) else { panic!("one step") };
    assert!(s.draft_errors.is_empty(), "it builds");
    assert_eq!(top(&s), Some((id, Source::Outside, vec!["tracks/pad.ts".to_string()])), "only the outside file");
    s.undo().unwrap();
    assert_eq!(d.read("song.ts"), mine, "the musician's save stays");
    assert!(!d.0.join("tracks/pad.ts").exists(), "the outside file is gone");
}

// AGENT-2: an outside change to a file whose save didn't build yet is undone to the musician's
// save, the text the app last knew there, not to the last version that built.
#[test]
fn undoing_an_outside_fix_of_a_waiting_save_puts_the_save_back() {
    let d = Dir::first_song();
    let mut s = d.open();
    let bass = d.read("tracks/bass.ts");
    let mine = format!("{bass}\nconst broken = ;\n");
    let saved = s.save(Path::new("tracks/bass.ts"), Base::text(&bass), &mine).unwrap();
    assert!(!saved.recorded && !s.draft_errors.is_empty(), "it doesn't build yet: no step");
    d.write("tracks/bass.ts", &format!("{bass}\nconst fixed = 1;\n"));
    let Outside::Step(_) = batch(&d, &mut s, &["tracks/bass.ts"]) else { panic!("one step") };
    s.undo().unwrap();
    assert_eq!(d.read("tracks/bass.ts"), mine, "the musician's save is back");
}

// AGENT-2, WRITE-3a: music2 never writes through a link, so a file made outside as one can't be
// undone: the batch makes no step and says why, and the steps before it still undo.
#[test]
fn an_outside_link_makes_no_step_and_older_steps_still_undo() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.4)).unwrap();
    let mine = top(&s).unwrap().0;
    let target = d.0.with_extension("extra.json");
    std::fs::write(&target, r#"{"clips": []}"#).unwrap();
    std::os::unix::fs::symlink(&target, d.0.join("data/hats/extra.json")).unwrap();
    let made = batch(&d, &mut s, &["data/hats/extra.json"]);
    assert_eq!(made, Outside::Cant { file: "data/hats/extra.json".into(), why: Cant::Link, others: 0 });
    assert_eq!(top(&s).map(|t| t.0), Some(mine), "no step");
    s.undo().unwrap();
    assert!(d.read("song.ts").contains("pan: 0.3"), "the step before it undoes");
    let _ = std::fs::remove_file(&target);
}

// AGENT-2, WRITE-3a: a file renamed in capitals alone can't come back under its old name while the
// new one is there, on a disk that ignores capitals: no step.
#[test]
fn an_outside_rename_in_capitals_makes_no_step() {
    let d = Dir::first_song();
    d.write("data/hats/extra.json", r#"{"clips": []}"#);
    let mut s = d.open();
    std::fs::rename(d.0.join("data/hats/extra.json"), d.0.join("data/hats/Extra.json")).unwrap();
    let made = batch(&d, &mut s, &["data/hats/extra.json", "data/hats/Extra.json"]);
    assert_eq!(made, Outside::Cant { file: "data/hats/extra.json".into(), why: Cant::Capitals, others: 0 });
    assert!(s.history.is_empty(), "no step");
}

// AGENT-2: a plugin state made outside changes the sound: its step removes it. A new asset beside
// it stays.
#[test]
fn an_outside_plugin_state_is_removed_by_undo() {
    let d = Dir::first_song();
    let mut s = d.open();
    d.write_bytes("data/hats/hats.state", b"\x00\x01 a plugin's state");
    let Outside::Step(_) = batch(&d, &mut s, &["data/hats/hats.state"]) else { panic!("one step") };
    assert_eq!(top(&s).unwrap().2, vec!["data/hats/hats.state".to_string()]);
    s.undo().unwrap();
    assert!(!d.0.join("data/hats/hats.state").exists(), "undo removes it");
}

// AGENT-2: a window that has the song read-only can't undo, so it records no outside step for
// what the writing window changes; it takes the change in all the same.
#[test]
fn a_read_only_window_records_no_outside_step() {
    let d = Dir::first_song();
    let mut w = d.open();
    let mut r = Session::open_read_only(&d.0, std_packages()).unwrap_or_else(|d| panic!("{d:?}"));
    w.commit(pan(&d, 0.5)).unwrap();
    assert_eq!(batch(&d, &mut r, &["song.ts"]), Outside::Nothing);
    assert!(r.step_names().0.is_none(), "no step");
    assert!(r.accepted().snapshot.files[Path::new("song.ts")].contains("pan: 0.5"), "taken in");
}

// WRITE-2, AGENT-2: a save from the code editor is one step, the page's, as before; the watcher's
// look at the file it wrote makes no second one.
#[test]
fn a_code_editor_save_is_still_one_step() {
    let d = Dir::first_song();
    let mut s = d.open();
    let song = d.read("song.ts");
    s.save(Path::new("song.ts"), Base::text(&song), &panned(&d, "0.5")).unwrap();
    let (id, source, files) = top(&s).expect("a step");
    assert_eq!((source, files), (Source::Page, vec!["song.ts".to_string()]));
    assert!(id > 0, "with an id");
    assert_eq!(batch(&d, &mut s, &["song.ts"]), Outside::Nothing, "the watcher sees nothing new");
    assert_eq!(s.history.len(), 1, "one step");
}

// AGENT-2, WRITE-19: a Tx the session committed is one step, the page's; its own write makes no
// outside step.
#[test]
fn the_sessions_own_commit_is_not_an_outside_step() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.5)).unwrap();
    let (id, source, _) = top(&s).expect("a step");
    assert_eq!(source, Source::Page);
    assert!(id > 0, "with an id");
    assert_eq!(batch(&d, &mut s, &["song.ts"]), Outside::Nothing);
    assert_eq!(s.history.len(), 1);
    assert_eq!(top(&s).unwrap().0, id);
}

// AGENT-2: a revision can't name a step, since a layout-only Tx keeps the revision number (PLANE-3).
// Each step has an id of its own.
#[test]
fn a_layout_step_has_its_own_id() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.5)).unwrap();
    let first = s.step_names().0.unwrap();
    let layout = r#"{"order": ["kick", "hats", "bass", "keys"]}"#;
    let mut tx = Tx::new(
        "tx-layout",
        ("".into(), "order".into()),
        vec![Op::Write { file: p("layout.json"), bytes: Payload::Text(layout.into()) }],
        Expect::Identical,
    );
    tx.base.insert(p("layout.json"), if d.0.join("layout.json").exists() { Base::text(&d.read("layout.json")) } else { Base::Absent });
    s.commit(tx).unwrap();
    let second = s.step_names().0.unwrap();
    assert_eq!(first.made, second.made, "one revision");
    assert!(first.id > 0 && second.id > first.id, "two ids: {} then {}", first.id, second.id);
}

// AGENT-2: undo and redo carry a step's id and source to the step they push: a redone step is the
// same step, and an outside one stays outside.
#[test]
fn a_redone_step_keeps_its_id_and_source() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.4)).unwrap();
    let mine = top(&s).unwrap();
    let text = d.read("tracks/bass.ts");
    d.write("tracks/bass.ts", &format!("{text}\n// from outside\n"));
    let Outside::Step(id) = batch(&d, &mut s, &["tracks/bass.ts"]) else { panic!("one step") };
    assert_ne!(id, mine.0);
    s.undo().unwrap();
    let redo = s.step_names().1.unwrap();
    assert_eq!((redo.id, redo.source), (id, Source::Outside), "redo's step is the outside one");
    assert_eq!(top(&s).unwrap().0, mine.0);
    s.undo().unwrap();
    assert_eq!(s.step_names().1.map(|n| (n.id, n.source)), Some((mine.0, Source::Page)));
    s.redo().unwrap();
    s.redo().unwrap();
    assert_eq!(top(&s).map(|t| (t.0, t.1)), Some((id, Source::Outside)), "redone: the same id and source");
    s.undo().unwrap();
    assert_eq!(d.read("tracks/bass.ts"), text, "and it still undoes");
}

// AGENT-3, D2 (the review of ag-see, F3): Compare's Before is the song with the newest step taken
// back, built in memory: nothing is written, and undo and redo stay as they were. The undo that may
// follow takes back the same, and an outside step's Before is built the same way.
#[test]
fn undone_builds_before_and_writes_nothing() {
    let d = Dir::first_song();
    let mut s = d.open();
    assert!(matches!(s.undone(), Err(StepError::Nothing(_))), "nothing to undo");
    let was = d.read("song.ts");
    s.commit(pan(&d, 0.4)).unwrap();
    let after = d.read("song.ts");
    let (number, was_top) = (s.accepted().number, top(&s));
    let before = s.undone().unwrap();
    assert!(before.snapshot.files[Path::new("song.ts")].contains("pan: 0.3"), "Before: the change taken back");
    assert_eq!(d.read("song.ts"), after, "nothing written");
    assert_eq!((s.accepted().number, top(&s), s.step_names().1.is_none()), (number, was_top, true), "undo and redo as they were");
    s.undo().unwrap();
    assert_eq!(d.read("song.ts"), was, "the undo takes back the same");
    assert_eq!(s.accepted().snapshot.files[Path::new("song.ts")], before.snapshot.files[Path::new("song.ts")]);
    // An outside step.
    s.redo().unwrap();
    let saved = d.read("song.ts").replacen("pan: 0.4", "pan: 0.5", 1);
    d.write("song.ts", &saved);
    let made = batch(&d, &mut s, &["song.ts"]);
    let Outside::Step(_) = made else { panic!("an outside step: {made:?}") };
    let before = s.undone().unwrap();
    assert!(before.snapshot.files[Path::new("song.ts")].contains("pan: 0.4"), "the outside save taken back");
    assert_eq!(d.read("song.ts"), saved, "never rewritten");
}
