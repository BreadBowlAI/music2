//! The host's hook between a commit's pre-check and its journal (PLANE-5, WRITE-15): when it is
//! told, what it is told, that nothing is accepted before the commit is durable, and that it is
//! never told of a refused Tx, a save or a recovery.

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use m2_song::tx::{Base, Confined, Expect, Op, Payload};
use m2_song::{LaneAddr, LaneData};
use serde_json::{Value, json};

use super::stage_tests::{Act, Recorder, journals, recorded};
use super::tests::{Dir, based, p, pan, replace, std_packages};
use super::{Refusal, Writing};
use crate::{Session, Uncommitted};

/// A notes lane of two clips, `a` and `b`.
const TWO: &str = "{\"clips\": [\n  \
                   {\"id\": \"a\", \"start\": 0, \"length\": 4, \"notes\": [\n    \
                   {\"pitch\": \"C1\", \"start\": 0, \"length\": 1, \"velocity\": 0.8}\n  ]},\n  \
                   {\"id\": \"b\", \"start\": 4, \"length\": 4, \"notes\": [\n    \
                   {\"pitch\": \"C1\", \"start\": 0, \"length\": 1, \"velocity\": 0.8}\n  ]}\n]}\n";

fn kick_hits() -> LaneAddr {
    LaneAddr::new("kick", "hits").unwrap()
}

fn clip(id: &str, pitch: &str) -> Value {
    json!({"id": id, "start": 0, "length": 4, "notes": [{"pitch": pitch, "start": 0, "length": 1, "velocity": 0.8}]})
}

fn lanes() -> Expect {
    Expect::Confined(Confined { lanes: vec![kick_hits()], ..Confined::default() })
}

/// first-song with the kick's lane of two clips.
fn project() -> Dir {
    let d = Dir::first_song();
    d.write("data/kick/hits.json", TWO);
    d
}

/// Clip `a` of the kick's lane played at `pitch`: one swap (WRITE-15 step 7).
fn one(d: &Dir, pitch: &str) -> m2_song::Tx {
    based(d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("a", pitch)], delete: vec![] }], lanes())
}

/// [`one`], and a new lane file beside it: a swap, then a create.
fn two(d: &Dir) -> m2_song::Tx {
    let ops = vec![
        Op::Clips { lane: kick_hits(), put: vec![clip("a", "D1")], delete: vec![] },
        Op::Write { file: p("data/hats/extra.json"), bytes: Payload::Text("{\"clips\": []}\n".into()) },
    ];
    based(d, ops, lanes())
}

/// What a hook was told, as it was told it: the call count of `r` then, whether the commit is
/// data only, the accepted revision's number, what the candidate plays in the kick's lane, and
/// the lane file on disk.
#[derive(Clone, Debug)]
struct Told {
    at: usize,
    data_only: bool,
    accepted: (u64, u64),
    candidate: Option<Arc<LaneData>>,
    disk: String,
}

/// A hook that records what it was told into the returned list.
fn listening(d: &Dir, r: &Recorder) -> (super::BeforeWrite, Arc<Mutex<Vec<Told>>>) {
    let told = Arc::new(Mutex::new(vec![]));
    let (t, r, file) = (told.clone(), r.clone(), d.0.join("data/kick/hits.json"));
    let hook = Box::new(move |w: &Writing| {
        t.lock().unwrap().push(Told {
            at: r.count(),
            data_only: w.data_only(),
            accepted: (w.accepted.number, w.accepted.data_rev),
            candidate: w.candidate.compiled.data.lane("kick", "hits").cloned(),
            disk: std::fs::read_to_string(&file).unwrap_or_default(),
        })
    });
    (hook, told)
}

/// What the session has accepted: what must not move when a commit fails.
fn accepted(s: &Session) -> (usize, u64, u64, usize, usize, usize, bool) {
    let a = &s.accepted;
    (Arc::as_ptr(a) as usize, a.number, a.data_rev, s.history.len(), s.redo.len(), s.texts.len(), s.can_undo())
}

// PLANE-5, WRITE-15: the host is told of a data commit once every target reads as its base (the
// pre-check's reads done) and before the journal's first byte: the accepted revision is still the
// one before, the file still holds its old bytes, and the candidate plays the new clip. Once the
// commit returns there is revision n+1 and one more undo step.
#[test]
fn the_host_hears_a_candidate_between_the_precheck_and_the_journal() {
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let (hook, told) = listening(&d, &r);
    s.before_write(Some(hook));
    let start = r.count();
    let before = accepted(&s);
    s.commit(one(&d, "D1")).unwrap();
    let told = told.lock().unwrap().clone();
    assert_eq!(told.len(), 1, "told once: {told:?}");
    let t = &told[0];
    let calls = r.calls.lock().unwrap().clone();
    let file = d.0.canonicalize().unwrap().join("data/kick/hits.json");
    assert!(calls[t.at - 1].starts_with(&format!("read {}", file.display())), "after the pre-check's read: {calls:#?}");
    assert!(calls[t.at].starts_with("mkdirs") && calls[t.at + 1].starts_with("create") && calls[t.at + 1].ends_with(".json.part"));
    assert!(calls[start..t.at].iter().all(|c| !c.starts_with("create") && !c.starts_with("rename")), "nothing written before");
    assert!(t.data_only);
    assert_eq!(t.accepted, (before.1, before.2), "nothing accepted yet");
    assert_eq!(t.disk, TWO, "the file is as it was");
    let LaneData::Notes(clips) = &**t.candidate.as_ref().unwrap() else { panic!("notes") };
    assert_eq!(clips[0].notes[0].pitch, 26.0, "the candidate plays D1");
    let after = accepted(&s);
    assert_eq!((after.1, after.2, after.3), (before.1 + 1, before.1 + 1, before.3 + 1), "accepted, with its undo step");
    assert!(Arc::ptr_eq(s.accepted.compiled.data.lane("kick", "hits").unwrap(), t.candidate.as_ref().unwrap()), "what it heard");
}

/// The disk calls of [`two`] committed on a fresh project, from the commit's first, and where
/// the host was told among them.
fn probe() -> (Vec<String>, usize) {
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let (hook, told) = listening(&d, &r);
    s.before_write(Some(hook));
    let start = r.count();
    s.commit(two(&d)).unwrap();
    let at = told.lock().unwrap()[0].at - start;
    (r.calls.lock().unwrap()[start..].to_vec(), at)
}

// PLANE-5, WRITE-15, 6a.6: a commit that fails after the host heard it accepts nothing: the
// accepted revision, its data_rev, undo and redo and the text store stay. Failed at the journal's
// create, a staged file's create, the barrier, a swap whose read-back finds an outside write, and a
// rollback that fails too (the journal stays, `tx.interrupted` at once).
#[test]
fn a_commit_that_fails_after_its_preview_accepts_nothing() {
    let (calls, hook) = probe();
    let nth = |what: &str, n: usize| hook + calls[hook..].iter().enumerate().filter(|(_, c)| c.starts_with(what)).nth(n).unwrap().0;
    let renames = into_project(&calls);
    assert!(calls[renames[0]].contains("hits.json") && calls[renames[0]].ends_with("Swap") && calls[renames[1]].contains("extra.json"));
    for case in ["journal", "staged", "barrier", "swap", "rollback"] {
        let d = project();
        let tree = d.tree();
        let r = Recorder::default();
        let mut s = recorded(&d, &r);
        let (hook, told) = listening(&d, &r);
        s.before_write(Some(hook));
        let start = r.count();
        let at = |k: usize, act: Act| r.act(start + k, act);
        match case {
            "journal" => at(nth("create", 0), Act::Fail),
            "staged" => at(nth("create", 1), Act::Fail),
            "barrier" => at(nth("barrier", 0), Act::Fail),
            "swap" => {
                // Another program replaces the lane file by rename just before the swap.
                let outside = d.0.join("data/kick/hits.json");
                at(
                    renames[0],
                    Act::Before(Arc::new(move || {
                        let tmp = outside.with_extension("tmp");
                        std::fs::write(&tmp, TWO.replace("C1", "G1")).unwrap();
                        std::fs::rename(&tmp, &outside).unwrap();
                    })),
                );
            }
            _ => {
                at(renames[1], Act::Fail);
                at(renames[1] + 1, Act::Fail);
            }
        }
        let before = accepted(&s);
        let e = s.commit(two(&d)).unwrap_err();
        assert_eq!(told.lock().unwrap().len(), 1, "{case}: the host heard it once");
        assert_eq!(accepted(&s), before, "{case}: nothing accepted: {e}");
        match case {
            "swap" => {
                assert!(matches!(e, Refusal::Conflict { .. }), "{case}: {e:?}");
                assert_eq!(d.read("data/kick/hits.json"), TWO.replace("C1", "G1"), "the outside writer's bytes stay");
                assert!(journals(&d).is_empty());
            }
            "rollback" => {
                assert!(e.to_string().contains("couldn't be undone"), "{e}");
                assert!(s.problems().iter().any(|p| p.code == "tx.interrupted"), "{:?}", s.problems());
                assert_eq!(journals(&d).len(), 1);
            }
            _ => {
                assert!(matches!(e, Refusal::Io { .. }), "{case}: {e:?}");
                assert_eq!(
                    e.to_string(),
                    "data/kick/hits.json, data/hats/extra.json couldn't be saved: injected",
                    "{case}: said with what wasn't saved"
                );
                assert_eq!(d.tree(), tree, "{case}: the project is as it was");
                assert!(journals(&d).is_empty(), "{case}: no journal is left");
            }
        }
    }
}

/// Where among `calls` a rename into the project (not into `.music2/tx/`) is.
fn into_project(calls: &[String]) -> Vec<usize> {
    let into = |c: &String| c.starts_with("rename") && !c.split(' ').nth(2).unwrap().contains(".music2/tx/");
    calls.iter().enumerate().filter(|(_, c)| into(c)).map(|(i, _)| i).collect()
}

/// The disk calls of [`two`] when its second rename into the project fails, from the commit's
/// first: the rollback's own calls among them.
fn rolled_back() -> Vec<String> {
    let (calls, _) = probe();
    let second = into_project(&calls)[1];
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let start = r.count();
    r.act(start + second, Act::Fail);
    assert!(s.commit(two(&d)).is_err());
    r.calls.lock().unwrap()[start..].to_vec()
}

// 6a.6, WRITE-15: a rollback whose barrier fails keeps its journal and staging until a later
// barrier makes the rollback durable (the next commit's, or the close): never tidied before.
// Nothing is accepted and the project is as it was.
#[test]
fn a_rollback_whose_barrier_fails_keeps_its_journal_until_the_next() {
    let calls = rolled_back();
    let barrier = calls.iter().rposition(|c| c.starts_with("barrier")).unwrap();
    assert!(calls[barrier..].iter().any(|c| c.starts_with("remove_file") && c.ends_with(".json")), "tidied after: {calls:#?}");
    let d = project();
    let tree = d.tree();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let (hook, told) = listening(&d, &r);
    s.before_write(Some(hook));
    let start = r.count();
    r.act(start + into_project(&probe().0)[1], Act::Fail);
    r.act(start + barrier, Act::Fail);
    let before = accepted(&s);
    let e = s.commit(two(&d)).unwrap_err();
    assert!(matches!(e, Refusal::Io { .. }), "{e:?}");
    assert_eq!((told.lock().unwrap().len(), accepted(&s)), (1, before), "heard once, nothing accepted");
    assert_eq!(d.tree(), tree, "rolled back");
    assert_eq!(journals(&d).len(), 1, "the journal waits for a barrier");
    assert!(s.problems().iter().all(|p| !p.code.starts_with("tx.")), "{:?}", s.problems());
    // The next commit's barrier covers the rollback: its journal goes then.
    s.commit(one(&d, "E1")).unwrap();
    let names: Vec<String> = journals(&d).iter().map(|j| j.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 1, "only the new commit's journal awaits its tidy: {names:?}");
    drop(s);
    assert!(journals(&d).is_empty());
}

// WRITE-15: a close whose barrier fails removes no journal and no before-image: the renames they
// cover may not be durable. The next writer's lock tidies them after its own barrier, saying
// nothing (review dur-2 of the platform step).
#[test]
fn a_close_whose_barrier_fails_keeps_its_journal_until_the_next_lock() {
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    s.commit(one(&d, "D1")).unwrap();
    let close = r.count();
    r.act(close, Act::Fail);
    drop(s);
    let calls = r.calls.lock().unwrap()[close..].to_vec();
    assert!(calls[0].starts_with("barrier"), "the close's barrier: {calls:#?}");
    assert_eq!(calls.len(), 1, "nothing after it: {calls:#?}");
    assert_eq!(journals(&d).len(), 1, "the journal stays");
    let s = d.open();
    assert!(s.problems().iter().all(|p| !p.code.starts_with("tx.")), "{:?}", s.problems());
    assert!(journals(&d).is_empty(), "tidied at the lock");
    assert!(d.read("data/kick/hits.json").contains("D1"));
}

// PLANE-5: the host is never told of a Tx that is refused (bases that don't apply, a broken
// expectation, a planner's assertion, a target changed between the read and the pre-check, a
// read-only session), of one that writes nothing, of a save, or of the undo of a save, which is
// committed raw, like a recovery.
#[test]
fn the_hook_is_never_called_for_a_refused_tx() {
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let (hook, told) = listening(&d, &r);
    s.before_write(Some(hook));
    let heard = || told.lock().unwrap().len();

    // Planned, then its clip changed on disk.
    let tx = one(&d, "D1");
    d.write("data/kick/hits.json", &TWO.replacen("C1", "F1", 1));
    assert!(matches!(s.commit(tx).unwrap_err(), Refusal::Conflict { .. }));
    d.write("data/kick/hits.json", TWO);
    // Its expectation broken: a lane it doesn't confine.
    let tx =
        based(&d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("a", "D1")], delete: vec![] }], Expect::Confined(Confined::default()));
    let e = s.commit(tx).unwrap_err();
    assert!(e.to_string().contains("the edit was refused"), "{e}");
    // The planner says no.
    let said = s.commit_with(one(&d, "D1"), |_, _| Err("no")).unwrap_err();
    assert!(matches!(said, Uncommitted::Asserted("no")));
    // Changed between the read and the pre-check: at the pre-check's first call.
    let (file, start) = (d.0.join("data/kick/hits.json"), r.count());
    let precheck = probe().0.iter().position(|c| c.starts_with("kind")).unwrap();
    r.act(start + precheck, Act::Before(Arc::new(move || std::fs::write(&file, TWO.replacen("C1", "A1", 2)).unwrap())));
    assert!(matches!(s.commit(one(&d, "D1")).unwrap_err(), Refusal::Conflict { .. }));
    d.write("data/kick/hits.json", TWO);
    // Nothing to write.
    let same = based(&d, vec![Op::Clips { lane: kick_hits(), put: vec![clip("a", "C1")], delete: vec![] }], lanes());
    let n = s.accepted().number;
    assert_eq!(s.commit(same).unwrap().number, n);
    // A save, and its undo (committed raw).
    let song = d.read("song.ts");
    s.save(Path::new("song.ts"), Base::text(&song), &format!("// saved\n{song}")).unwrap();
    s.undo().unwrap();
    assert_eq!(d.read("song.ts"), song);
    assert_eq!(heard(), 0, "{:?}", told.lock().unwrap());
    // A session that can't write.
    let mut ro = Session::open_read_only(&d.0, std_packages()).unwrap();
    let (hook, told) = listening(&d, &r);
    ro.before_write(Some(hook));
    assert!(ro.commit(one(&d, "D1")).is_err());
    assert!(told.lock().unwrap().is_empty());
    drop(ro);
    // And the same lane write, unrefused, is heard.
    s.commit(one(&d, "D1")).unwrap();
    assert_eq!(heard(), 1);
}

// PLANE-1, PLANE-5: a commit is data only when every file it changes is data and its candidate
// plays the accepted code: a Clips Tx is; a Values Tx, a code edit and an arrangement write
// aren't.
#[test]
fn a_tx_touching_code_is_not_data_only() {
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let (hook, told) = listening(&d, &r);
    s.before_write(Some(hook));
    let kick = d.read("tracks/kick.ts");
    let arrangement = d.read("data/arrangement.json");
    let txs = [
        one(&d, "D1"),
        pan(&d, 0.45),
        based(&d, vec![replace("tracks/kick.ts", &kick, "Exported", "other", "the other").0], Expect::Seen(m2_song::Sha::of(b""))),
        based(
            &d,
            vec![Op::Write { file: p("data/arrangement.json"), bytes: Payload::Text(arrangement.replace("124", "125").into()) }],
            Expect::Seen(m2_song::Sha::of(b"")),
        ),
    ];
    for (k, tx) in txs.into_iter().enumerate() {
        let tx = if k >= 2 {
            // Committed on the delta its check shows (WRITE-11).
            let mut tx = tx;
            tx.expect = Expect::Seen(s.check(&tx).unwrap().hash);
            tx
        } else {
            tx
        };
        s.commit(tx).unwrap_or_else(|e| panic!("{k}: {e}"));
    }
    let only: Vec<bool> = told.lock().unwrap().iter().map(|t| t.data_only).collect();
    assert_eq!(only, [true, false, false, false]);
}

// PLANE-5, WRITE-17: the lock is checked again once the host was told (its hook may take a
// while): one lost meanwhile (`.music2/` removed, as `git clean -fdx` does) fails the commit with
// nothing written or accepted, and the next commit takes the lock again.
#[test]
fn a_lock_lost_while_the_host_heard_it_fails_the_commit() {
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let lock = d.0.join(".music2/lock");
    s.before_write(Some(Box::new(move |_| std::fs::remove_file(&lock).unwrap())));
    let (tree, before) = (d.tree(), accepted(&s));
    let e = s.commit(one(&d, "D1")).unwrap_err();
    assert!(matches!(e, Refusal::Io { .. }) && e.to_string().contains("lock was let go"), "{e:?}");
    assert_eq!((d.tree(), accepted(&s)), (tree, before), "nothing written or accepted");
    assert!(journals(&d).is_empty());
    s.before_write(None);
    s.commit(one(&d, "D1")).unwrap();
    assert!(d.0.join(".music2/lock").exists() && d.read("data/kick/hits.json").contains("D1"));
}

/// The child of [`killed_in_the_hook_the_project_is_as_before`]: in the project `M2_TX_HOOK_DIR`,
/// a lane write whose host aborts as it is told (no destructors run, the OS lets go of the lock).
#[test]
fn hook_child() {
    let Ok(dir) = std::env::var("M2_TX_HOOK_DIR") else { return };
    super::kill_tests::no_core_dump();
    let d = Dir(PathBuf::from(dir));
    let mut s = d.open();
    s.before_write(Some(Box::new(|_| std::process::abort())));
    let _ = s.commit(one(&d, "D1"));
    std::mem::forget(d);
}

// PLANE-5, WRITE-16: killed while the host is told (after the pre-check, before the journal), the
// project reopens as it was: the lane file's old bytes, no journal, no diagnostic, the old data.
#[test]
fn killed_in_the_hook_the_project_is_as_before() {
    let d = project();
    let tree = d.tree();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tx::hook_tests::hook_child", "--nocapture", "--test-threads", "1"])
        .env("M2_TX_HOOK_DIR", &d.0)
        .output()
        .unwrap()
        .status;
    assert_eq!(status.signal(), Some(libc::SIGABRT), "killed in the hook ({status:?})");
    assert_eq!(d.tree(), tree);
    assert!(journals(&d).is_empty());
    let s = d.open();
    assert!(s.problems().iter().all(|p| !p.code.starts_with("tx.")), "{:?}", s.problems());
    let LaneData::Notes(clips) = &**s.accepted().compiled.data.lane("kick", "hits").unwrap() else { panic!("notes") };
    assert_eq!(clips[0].notes[0].pitch, 24.0, "C1, as before");
}
