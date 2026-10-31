//! Undo that survives closing (WRITE-23 to WRITE-26): the history kept in `.music2/undo/` and read
//! back when the song opens; each step offered only while its files hold the bytes it left; a crash
//! or a damaged file that loses steps but never offers a wrong undo; the bound, on disk too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m2_song::LaneAddr;
use m2_song::tx::{Base, Confined, Expect, Op, Payload};
use serde_json::{Value, json};

use crate::history::{Bound, End};
use crate::tx::tests::{Dir, based, p, pan};
use crate::{Session, Source};

type Tree = BTreeMap<String, Vec<u8>>;

fn kick_hits() -> LaneAddr {
    LaneAddr::new("kick", "hits").unwrap()
}

fn bass_notes() -> LaneAddr {
    LaneAddr::new("bass", "notes").unwrap()
}

/// A clip `id` of one note at `pitch`.
fn clip(id: &str, pitch: &str) -> Value {
    json!({"id": id, "start": 64, "length": 4, "notes": [{"pitch": pitch, "start": 0, "length": 1, "velocity": 0.8}]})
}

/// Clip `id` put in `lane`: a data Tx.
fn put(d: &Dir, lane: LaneAddr, id: &str, pitch: &str) -> m2_song::Tx {
    let lanes = Expect::Confined(Confined { lanes: vec![lane.clone()], ..Confined::default() });
    based(d, vec![Op::Clips { lane, put: vec![clip(id, pitch)], delete: vec![] }], lanes)
}

/// `song.ts` saved with a comment line added, as the code editor saves it.
fn save(d: &Dir, s: &mut Session, comment: &str) {
    let song = d.read("song.ts");
    let saved = s.save(Path::new("song.ts"), Base::text(&song), &format!("{song}// {comment}\n")).unwrap();
    assert!(saved.recorded, "the save is one step");
}

/// Three steps of three kinds, each on its own file: clips in a lane, a value in code, clips in
/// another lane. The project's files before each, and after the last.
fn three(d: &Dir, s: &mut Session) -> Vec<Tree> {
    let mut trees = vec![d.tree()];
    s.commit(put(d, kick_hits(), "k1", "C1")).unwrap();
    trees.push(d.tree());
    s.commit(pan(d, 0.4)).unwrap();
    trees.push(d.tree());
    s.commit(put(d, bass_notes(), "b1", "E1")).unwrap();
    trees.push(d.tree());
    trees
}

fn undo_dir(d: &Dir) -> PathBuf {
    d.0.join(".music2/undo")
}

/// A copy of `.music2/undo/` as it is now, put back by [`restore`].
fn keep(d: &Dir) -> Vec<(PathBuf, Vec<u8>)> {
    std::fs::read_dir(undo_dir(d)).unwrap().flatten().map(|e| (e.path(), std::fs::read(e.path()).unwrap())).collect()
}

/// `.music2/undo/` as [`keep`] copied it: what a crash before the history's write leaves.
fn restore(d: &Dir, kept: &[(PathBuf, Vec<u8>)]) {
    std::fs::remove_dir_all(undo_dir(d)).unwrap();
    std::fs::create_dir_all(undo_dir(d)).unwrap();
    for (p, b) in kept {
        std::fs::write(p, b).unwrap();
    }
}

fn index(d: &Dir) -> Value {
    serde_json::from_slice(&std::fs::read(undo_dir(d).join("index.json")).unwrap()).unwrap()
}

/// The records in `.music2/undo/`, by name.
fn records(d: &Dir) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(undo_dir(d))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "index.json")
        .collect();
    out.sort();
    out
}

fn codes(s: &Session) -> Vec<&'static str> {
    s.problems().iter().map(|d| d.code).collect()
}

// WRITE-23, WRITE-24: closed and opened again, undo takes back three steps of three kinds, each
// leaving the files byte for byte as they were before it; redo, after closing again, brings each
// back. Each step keeps its id, source and name, and new ids go on after them.
#[test]
fn undo_after_closing_puts_back_each_step_byte_for_byte() {
    let d = Dir::first_song();
    let mut s = d.open();
    let trees = three(&d, &mut s);
    let made = s.accepted().number;
    assert!(s.name_step(made, "Bass notes added", true));
    s.act_as(Source::Agent("Claude".into()), Some("a clip".into()));
    save(&d, &mut s, "an agent's save");
    let trees = [trees, vec![d.tree()]].concat();
    let tops = s.step_names().0.unwrap();
    drop(s);

    let mut s = d.open();
    let read = s.step_names().0.expect("undo survives closing");
    assert_eq!((read.id, &read.source, &read.name), (tops.id, &Source::Agent("Claude".into()), &Some("a clip".into())));
    for i in (0..4).rev() {
        assert!(s.can_undo(), "undo {i} is offered");
        if i == 2 {
            let named = s.step_names().0.unwrap();
            assert_eq!((named.name.as_deref(), named.structural), (Some("Bass notes added"), true), "the window's name too");
        }
        s.undo().unwrap();
        assert!(d.tree() == trees[i], "undo {i} leaves the files as they were before its step");
    }
    assert!(!s.can_undo() && s.undo_end().is_none(), "nothing more, and nothing cut it");
    drop(s);

    let mut s = d.open();
    for (i, tree) in trees.iter().enumerate().skip(1) {
        assert!(s.can_redo(), "redo {i} survives closing too");
        s.redo().unwrap();
        assert!(d.tree() == *tree, "redo {i}");
    }
    s.commit(pan(&d, 0.6)).unwrap();
    assert_eq!(s.last_step(), tops.id + 1, "ids go on after the ones read back");
}

// WRITE-24: an outside change made while the song was closed ends undo at the step whose file it
// touched: the step after it is undone byte for byte; it, and the one before it on another file,
// are offered no more, and the session says why. Redo the same.
#[test]
fn an_outside_change_while_closed_ends_undo_there() {
    let d = Dir::first_song();
    let mut s = d.open();
    let trees = three(&d, &mut s);
    drop(s);
    let song = format!("{}// changed outside\n", d.read("song.ts"));
    d.write("song.ts", &song);

    let mut s = d.open();
    s.undo().unwrap();
    let mut was = trees[2].clone();
    was.insert("song.ts".into(), song.clone().into_bytes());
    assert!(d.tree() == was, "the step after the change is undone, and the change stays");
    assert!(!s.can_undo() && s.step_names().0.is_none(), "undo ends at the changed step, the older kick step with it");
    assert_eq!(s.undo_end(), Some(End::Outside));

    // Redo: a change to the file of the step it would bring back ends it.
    drop(s);
    d.write("data/bass/notes.json", &format!("{}\n", d.read("data/bass/notes.json")));
    let s = d.open();
    assert!(!s.can_redo() && s.step_names().1.is_none());
    assert_eq!(s.redo_end(), Some(End::Outside));
    assert!(d.read("data/kick/hits.json").contains("k1"), "the kick step stays done");
}

// WRITE-24: a step whose files all hold what they held before it was undone, but a crash cut off
// the history's write: it is left out, and the steps before it are still offered.
#[test]
fn a_step_undone_before_its_history_was_written_is_left_out() {
    let d = Dir::first_song();
    let mut s = d.open();
    let trees = three(&d, &mut s);
    let kept = keep(&d);
    s.undo().unwrap();
    drop(s);
    restore(&d, &kept);

    let mut s = d.open();
    assert!(s.can_undo(), "the steps before it are offered");
    s.undo().unwrap();
    assert!(d.tree() == trees[1]);
    s.undo().unwrap();
    assert!(d.tree() == trees[0]);
    assert!(!s.can_undo() && s.undo_end().is_none());
}

// WRITE-25: a commit that landed but whose history wasn't written (a crash between them) is met as
// an outside change: on the file of the step below it, undo ends there; on another file, the step
// below is undone exactly, and the change it doesn't know stays.
#[test]
fn a_commit_whose_history_wasnt_written_offers_no_wrong_undo() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.4)).unwrap();
    let kept = keep(&d);
    s.commit(pan(&d, 0.5)).unwrap();
    drop(s);
    restore(&d, &kept);
    let s = d.open();
    assert!(!s.can_undo(), "no undo of the pan to 0.4 over the pan to 0.5");
    assert_eq!(s.undo_end(), Some(End::Outside));
    drop(s);

    let d = Dir::first_song();
    let mut s = d.open();
    let before = d.tree();
    s.commit(pan(&d, 0.4)).unwrap();
    let kept = keep(&d);
    s.commit(put(&d, kick_hits(), "k1", "C1")).unwrap();
    let hits = d.read("data/kick/hits.json");
    drop(s);
    restore(&d, &kept);
    let mut s = d.open();
    s.undo().unwrap();
    let mut was = before;
    was.insert("data/kick/hits.json".into(), hits.into_bytes());
    assert!(d.tree() == was, "the pan is undone exactly, and the clip it doesn't know stays");
}

// WRITE-25: an index cut short, or not JSON, opens the song with no undo and the Notice
// `undo.unread`, changing no file; the next step starts a history of its own.
#[test]
fn a_corrupt_or_cut_history_opens_with_no_undo_and_a_note() {
    for damage in ["cut", "garbage", "version"] {
        let d = Dir::first_song();
        let mut s = d.open();
        three(&d, &mut s);
        s.undo().unwrap();
        drop(s);
        let file = undo_dir(&d).join("index.json");
        let text = std::fs::read(&file).unwrap();
        match damage {
            "cut" => std::fs::write(&file, &text[..text.len() / 2]).unwrap(),
            "garbage" => std::fs::write(&file, b"\x00\x01 not JSON").unwrap(),
            _ => {
                let mut v = index(&d);
                v["version"] = json!(99);
                std::fs::write(&file, v.to_string()).unwrap();
            }
        }
        let tree = d.tree();
        let mut s = d.open();
        assert!(!s.can_undo() && !s.can_redo(), "{damage}: no undo, no redo");
        assert!(codes(&s).contains(&"undo.unread"), "{damage}: a note: {:?}", codes(&s));
        assert_eq!(s.undo_end(), Some(End::Unread));
        assert!(d.tree() == tree, "{damage}: no file changed");
        s.commit(pan(&d, 0.7)).unwrap();
        drop(s);
        let s = d.open();
        assert!(s.can_undo() && !codes(&s).contains(&"undo.unread"), "{damage}: a history of its own from then");
    }
}

// WRITE-25: a record whose bytes don't match its name (damaged on disk), or that is missing, ends
// undo there, with the Notice `undo.unread`: the steps after it are still offered.
#[test]
fn a_record_that_doesnt_match_its_name_ends_undo_there() {
    for damage in ["bytes", "json", "missing"] {
        let d = Dir::first_song();
        let mut s = d.open();
        let trees = three(&d, &mut s);
        drop(s);
        let second = index(&d)["undo"][1]["record"].as_str().unwrap().to_string();
        let file = undo_dir(&d).join(format!("{second}.json"));
        let text = std::fs::read_to_string(&file).unwrap();
        match damage {
            "bytes" => std::fs::write(&file, text.bytes().map(|b| b ^ 1).collect::<Vec<u8>>()).unwrap(),
            // Still a Tx that reads: only its name, the hash of its bytes, says it isn't the step's.
            "json" => {
                let other = text.replacen("0.4", "0.9", 1);
                assert!(other != text && m2_song::Tx::from_json(&serde_json::from_str(&other).unwrap()).is_ok());
                std::fs::write(&file, other).unwrap();
            }
            _ => std::fs::remove_file(&file).unwrap(),
        }
        let mut s = d.open();
        s.undo().unwrap();
        assert!(d.tree() == trees[2], "{damage}: the newest step is undone");
        assert!(!s.can_undo(), "{damage}: undo ends at the damaged step");
        assert_eq!(s.undo_end(), Some(End::Unread), "{damage}");
        assert!(codes(&s).contains(&"undo.unread"), "{damage}: a note: {:?}", codes(&s));
    }
}

// WRITE-26: past the bound the oldest steps go, in memory and on disk: once the history is next
// written, the index names the newest, their records alone are kept, and undo goes back only
// through them.
#[test]
fn the_bound_drops_the_oldest_steps_on_disk_too() {
    let d = Dir::first_song();
    crate::history::test_bound(Bound { steps: 2, ..crate::history::BOUND });
    let mut s = d.open();
    let trees = three(&d, &mut s);
    assert_eq!(s.history.len(), 2, "two steps kept");
    drop(s);
    let mut s = d.open();
    assert_eq!(index(&d)["undo"].as_array().unwrap().len(), 2, "two steps on disk");
    assert_eq!(records(&d).len(), 2, "and their records alone");
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(d.tree() == trees[1]);
    assert!(!s.can_undo(), "the oldest went");
    assert_eq!(s.undo_end(), None, "the bound isn't an outside change");
}

/// 1 MiB of a plugin's state.
fn state(seed: u8) -> Vec<u8> {
    (0..1u32 << 20).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8 ^ seed).collect()
}

/// A plugin's state written, as its autosave writes it (HOME-6).
fn write_state(d: &Dir, s: &mut Session, seed: u8) {
    let ops = vec![Op::Write { file: p("data/lead/lead.state"), bytes: Payload::Bytes(state(seed).into()) }];
    s.commit(based(d, ops, Expect::Confined(Confined::default()))).unwrap();
}

fn blobs(d: &Dir) -> Vec<String> {
    let Ok(e) = std::fs::read_dir(d.0.join(".music2/tx/blobs")) else { return vec![] };
    let mut out: Vec<String> = e.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    out.sort();
    out
}

// WRITE-26, FMT-17: a plugin state's before-image, kept by its hash in `.music2/tx/blobs/`, is
// kept through closing, and undo after opening puts the old state back. Past the bound for
// before-images the oldest step goes, and its image with it.
#[test]
fn a_plugin_states_before_image_survives_closing() {
    let d = Dir::first_song();
    d.write_bytes("data/lead/lead.state", &state(1));
    let mut s = d.open();
    write_state(&d, &mut s, 2);
    drop(s);
    assert_eq!(blobs(&d), [m2_song::Sha::of(&state(1)).hex()], "the before-image outlives the session");
    let mut s = d.open();
    s.undo().unwrap();
    assert_eq!(std::fs::read(d.0.join("data/lead/lead.state")).unwrap(), state(1));
    drop(s);

    let d = Dir::first_song();
    d.write_bytes("data/lead/lead.state", &state(1));
    crate::history::test_bound(Bound { images: 3 << 20, ..crate::history::BOUND });
    let mut s = d.open();
    for seed in 2..=5 {
        write_state(&d, &mut s, seed);
    }
    drop(s);
    let kept: Vec<String> = (2..=4).map(|seed| m2_song::Sha::of(&state(seed)).hex()).collect::<Vec<_>>();
    let mut kept = kept;
    kept.sort();
    assert_eq!(blobs(&d), kept, "three images fit; the oldest went with its step");
    let mut s = d.open();
    for _ in 0..3 {
        s.undo().unwrap();
    }
    assert_eq!(std::fs::read(d.0.join("data/lead/lead.state")).unwrap(), state(2));
    assert!(!s.can_undo());
}

// WRITE-23: a read-only session neither reads the history nor writes it, and its open changes
// nothing in `.music2/undo/`.
#[test]
fn a_read_only_session_keeps_no_history() {
    let d = Dir::first_song();
    let mut s = d.open();
    three(&d, &mut s);
    let kept = keep(&d);
    let r = Session::open_read_only(&d.0, crate::tx::tests::std_packages()).unwrap();
    assert!(!r.can_undo() && r.step_names().0.is_none());
    drop(r);
    let mut now = keep(&d);
    let mut was = kept;
    now.sort();
    was.sort();
    assert_eq!(now, was);
}

/// Run `change` on `s`, keeping `.music2/undo/` as it is the moment the change's files are about to
/// be written (its PLANE-5 hook, after the history's own write ahead of it): put back with
/// [`restore`] once the session is gone, what a crash right after the change's renames leaves.
fn crashed(d: &Dir, s: &mut Session, change: impl FnOnce(&mut Session)) -> Vec<(PathBuf, Vec<u8>)> {
    let kept = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let (into, dir) = (kept.clone(), undo_dir(d));
    s.before_write(Some(Box::new(move |_| {
        *into.lock().unwrap() = std::fs::read_dir(&dir).unwrap().flatten().map(|e| (e.path(), std::fs::read(e.path()).unwrap())).collect();
    })));
    change(s);
    s.before_write(None);
    let kept = kept.lock().unwrap().clone();
    assert!(!kept.is_empty(), "the hook ran");
    kept
}

// WRITE-25: a crash right after a redo landed keeps every undo step: the redone one on top, the
// older ones, on other files too, below it; nothing is said to be outside music2.
#[test]
fn a_crash_right_after_a_redo_keeps_every_undo_step() {
    let d = Dir::first_song();
    let mut s = d.open();
    let mut trees = vec![d.tree()];
    s.commit(put(&d, kick_hits(), "k1", "C1")).unwrap();
    trees.push(d.tree());
    s.commit(pan(&d, 0.4)).unwrap();
    trees.push(d.tree());
    s.commit(pan(&d, 0.5)).unwrap();
    trees.push(d.tree());
    s.undo().unwrap();
    let kept = crashed(&d, &mut s, |s| drop(s.redo().unwrap()));
    drop(s);
    restore(&d, &kept);

    let mut s = d.open();
    assert_eq!(s.undo_end(), None);
    for i in (0..3).rev() {
        assert!(s.can_undo(), "undo {i} is offered");
        s.undo().unwrap();
        assert!(d.tree() == trees[i], "undo {i}");
    }
}

// WRITE-25: a crash right after an undo landed puts its step on redo, and undo goes on below it; a
// change whose files never landed (a crash before its renames) isn't offered at all.
#[test]
fn a_crash_right_after_an_undo_puts_its_step_on_redo() {
    let d = Dir::first_song();
    let mut s = d.open();
    let trees = three(&d, &mut s);
    let kept = crashed(&d, &mut s, |s| drop(s.undo().unwrap()));
    drop(s);
    restore(&d, &kept);
    let mut s = d.open();
    assert!(s.can_redo(), "the undone step is on redo");
    s.redo().unwrap();
    assert!(d.tree() == trees[3]);
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(d.tree() == trees[1], "and undo goes on below it");

    // Pan's undo written ahead, its files never landed: the history is as before it.
    let kept = crashed(&d, &mut s, |s| drop(s.redo().unwrap()));
    s.undo().unwrap();
    drop(s);
    restore(&d, &kept);
    let mut s = d.open();
    assert!(d.tree() == trees[1]);
    s.undo().unwrap();
    assert!(d.tree() == trees[0], "the kick step is next, as if the redo had never begun");
    assert!(s.can_redo(), "and pan waits on redo");
}

// WRITE-24, AGENT-2: a new change ends redo, across closing too: one the history wrote ahead of a
// crash, and one made outside music2 while the song was closed, whatever file it touched.
#[test]
fn a_new_change_ends_redo_across_closing() {
    let d = Dir::first_song();
    let mut s = d.open();
    let before = d.tree();
    s.commit(pan(&d, 0.4)).unwrap();
    s.undo().unwrap();
    let kept = crashed(&d, &mut s, |s| drop(s.commit(put(&d, kick_hits(), "k1", "C1")).unwrap()));
    drop(s);
    restore(&d, &kept);
    let mut s = d.open();
    assert!(!s.can_redo(), "the clip ended redo");
    s.undo().unwrap();
    assert!(d.tree() == before, "and undo takes the clip back");
    drop(s);

    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.4)).unwrap();
    s.undo().unwrap();
    drop(s);
    let s = d.open();
    assert!(s.can_redo(), "closed and opened as it was, redo stays");
    drop(s);
    d.write("data/kick/hits.json", &format!("{}\n", d.read("data/kick/hits.json")));
    let s = d.open();
    assert!(!s.can_redo(), "a file changed outside, any file, ends it");
    assert_eq!(s.redo_end(), Some(End::Outside));
}

// WRITE-24: why undo ends is said by the session that met it, and no later one.
#[test]
fn an_end_is_said_only_by_the_session_that_met_it() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.4)).unwrap();
    drop(s);
    std::fs::write(undo_dir(&d).join("index.json"), b"not an index").unwrap();
    let mut s = d.open();
    assert_eq!(s.undo_end(), Some(End::Unread));
    s.commit(pan(&d, 0.5)).unwrap();
    drop(s);
    let mut s = d.open();
    s.undo().unwrap();
    assert!(!s.can_undo());
    assert_eq!(s.undo_end(), None, "a later session's history is whole");
}

/// 1 MiB of a plugin's state, seeded by `seed`, and `len` bytes of it.
fn state_of(seed: u8, len: usize) -> Vec<u8> {
    let mut s = state(seed);
    s.resize(len, seed);
    s
}

// WRITE-26: the bound counts both stacks, and each before-image once however many steps name it:
// a state that grew (a small image before it, a large one after) is bounded once undone too.
#[test]
fn the_bound_counts_redo_and_each_image_once() {
    // X, Y, X, Y, X: four steps naming two images, 2 MiB in all, under a bound of 2.5 MiB.
    let d = Dir::first_song();
    d.write_bytes("data/lead/lead.state", &state(1));
    crate::history::test_bound(Bound { images: 5 << 19, ..crate::history::BOUND });
    let mut s = d.open();
    for seed in [2, 1, 2, 1] {
        write_state(&d, &mut s, seed);
    }
    for _ in 0..4 {
        assert!(s.can_undo());
        s.undo().unwrap();
    }
    assert_eq!(std::fs::read(d.0.join("data/lead/lead.state")).unwrap(), state(1));
    drop(s);

    // Grown from 1 KiB: undone, the after-images on redo are past the bound, so the farthest goes.
    let d = Dir::first_song();
    d.write_bytes("data/lead/lead.state", &state_of(9, 1024));
    crate::history::test_bound(Bound { images: 5 << 19, ..crate::history::BOUND });
    let mut s = d.open();
    for seed in [2, 3, 4] {
        write_state(&d, &mut s, seed);
    }
    for _ in 0..3 {
        s.undo().unwrap();
    }
    drop(s);
    let mut s = d.open();
    s.redo().unwrap();
    s.redo().unwrap();
    assert!(!s.can_redo(), "the farthest redo went");
    assert_eq!(std::fs::read(d.0.join("data/lead/lead.state")).unwrap(), state(3));
    assert!(blobs(&d).iter().all(|b| *b != m2_song::Sha::of(&state(4)).hex()), "and its image");
}

// WRITE-26: a history read back is bounded as it is read, whatever bound wrote it.
#[test]
fn the_bound_holds_for_a_history_read_back() {
    let d = Dir::first_song();
    let mut s = d.open();
    let trees = three(&d, &mut s);
    drop(s);
    crate::history::test_bound(Bound { steps: 2, ..crate::history::BOUND });
    let mut s = d.open();
    assert_eq!(index(&d)["undo"].as_array().map(Vec::len), Some(2), "written back within it");
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(d.tree() == trees[1] && !s.can_undo());
}

// AGENT-2, WRITE-23: a step skipped stays skipped after closing, and the one before it is next.
#[test]
fn a_skipped_step_stays_skipped_after_closing() {
    let d = Dir::first_song();
    let mut s = d.open();
    let before = d.tree();
    s.commit(put(&d, kick_hits(), "k1", "C1")).unwrap();
    s.commit(pan(&d, 0.4)).unwrap();
    let pan_step = s.last_step();
    // Its value changed in another editor, which no batch took in: undo can't take it back.
    d.write("song.ts", &d.read("song.ts").replacen("pan: 0.4", "pan: 0.7", 1));
    let r = s.undo();
    assert!(r.is_err() && s.undo_stuck(), "{:?} stuck={}", r.as_ref().map(|_| ()), s.undo_stuck());
    assert!(s.skip_undo(pan_step));
    let song = d.read("song.ts");
    drop(s);
    let mut s = d.open();
    assert_ne!(s.step_names().0.map(|t| t.id), Some(pan_step), "the skipped step is gone");
    s.undo().unwrap();
    let mut was = before;
    was.insert("song.ts".into(), song.into_bytes());
    assert!(d.tree() == was, "the clip is undone, the pan stays as the editor left it");
}

// AGENT-3, WRITE-23: a change taken back stays taken back after closing, with redo as it was
// before it.
#[test]
fn a_change_taken_back_stays_taken_back_after_closing() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.4)).unwrap();
    s.undo().unwrap();
    let redo = s.step_names().1.unwrap().id;
    s.act_as(Source::Agent("Claude".into()), None);
    s.commit(put(&d, kick_hits(), "k1", "C1")).unwrap();
    s.retract().unwrap();
    s.act_as(Source::Page, None);
    drop(s);
    let s = d.open();
    assert_eq!(s.step_names().1.map(|t| t.id), Some(redo), "redo is as before the agent's change");
    assert!(s.step_names().0.is_none(), "and the agent's change is no step");
}
