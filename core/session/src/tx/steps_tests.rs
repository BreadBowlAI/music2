//! Undo steps with ids and sources (AGENT-2, track 1 of the agents design): an outside batch that
//! builds is one step, source outside, whose inverse puts its text files back; binaries, new assets
//! and git make none; the session's own writes and saves keep their one step each; ids are never
//! shared, and follow a step through undo and redo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use m2_song::Snapshot;
use m2_song::tx::{Base, Expect, Op, Payload, Tx};

use super::tests::{Dir, p, pan};
use crate::take::Outside;
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
// change that made no step (git's) means undo is a conflict naming the file, and nothing changes.
#[test]
fn undoing_an_outside_edit_after_a_change_without_a_step_conflicts() {
    let d = Dir::first_song();
    let mut s = d.open();
    d.write("song.ts", &panned(&d, "0.5"));
    let Outside::Step(id) = batch(&d, &mut s, &["song.ts"]) else { panic!("one step") };
    let checkout = d.read("song.ts").replacen("pan: 0.5", "pan: 0.6", 1);
    d.write("song.ts", &checkout);
    let by_git = s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths(&["song.ts"]), true).1;
    assert_eq!(by_git, Outside::Git(1), "git's batch makes no step");
    match s.undo() {
        Err(StepError::Refused(Refusal::Conflict { files })) => assert_eq!(files[0].file, "song.ts"),
        other => panic!("a conflict naming song.ts: {other:?}"),
    }
    assert_eq!(d.read("song.ts"), checkout, "nothing changed");
    assert_eq!(top(&s).map(|t| t.0), Some(id), "the step stays");
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
    assert_eq!(made, Outside::Binary("assets/keys-bounce.wav".into()));
    assert!(s.history.is_empty(), "no step");
    assert!(s.accepted().number > number, "taken in all the same");
    std::fs::remove_file(d.0.join("assets/keys-bounce.wav.json")).unwrap();
    assert_eq!(batch(&d, &mut s, &["assets/keys-bounce.wav.json"]), Outside::Binary("assets/keys-bounce.wav.json".into()), "removed too");
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

// AGENT-2: what git wrote makes no step (git can undo it), and says how many files it changed.
#[test]
fn a_batch_git_wrote_makes_no_step() {
    let d = Dir::first_song();
    let mut s = d.open();
    d.write("song.ts", &panned(&d, "0.5"));
    d.write("tracks/bass.ts", &format!("{}\n// checked out\n", d.read("tracks/bass.ts")));
    let (taken, made) = s.take_in_batch(Snapshot::read(&d.0).unwrap(), &paths(&["song.ts", "tracks/bass.ts"]), true);
    assert_eq!(made, Outside::Git(2));
    assert!(taken.accepted.is_some(), "taken in");
    assert!(s.history.is_empty() && s.step_names().0.is_none(), "no step");
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
