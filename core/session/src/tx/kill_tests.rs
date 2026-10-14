//! Killing the process at every disk call of a commit, and recovery as a diagnostic (WRITE-15,
//! WRITE-16, DIAG-5; the step 6 gate).
//!
//! The child is this test binary run again (`--exact tx::kill_tests::child`), committing through a
//! disk that aborts at its n-th call: no destructors run and the OS lets go of the lock, as in a
//! crash. Every disk call is a kill point, counted on a run that completes, so none is named by
//! hand (R20).

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use m2_song::Sha;
use m2_song::tx::{Base, Confined, Expect, Op, Payload};

use super::journal::{JTarget, Journal};
use super::stage_tests::{Act, Recorder, journals, project, recorded, several};
use super::tests::{Dir, based, p, pan, replace, std_packages};
use crate::Session;

/// 4 MB of state, as a plugin would save it.
fn blob(seed: u8) -> Vec<u8> {
    (0..4u32 << 20).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8 ^ seed).collect()
}

/// The project a case starts from.
fn start(case: &str) -> Dir {
    let d = project();
    if case == "blob" {
        d.write_bytes("data/lead/lead.state", &blob(1));
    }
    d
}

/// What the case commits. `code` changes two code files whose halves don't compile alone (an
/// export renamed, and the import of it), so a kill between its renames reopens with code that
/// doesn't compile; `dataonly` is `several` without its code edit.
fn tx(case: &str, d: &Dir) -> m2_song::Tx {
    match case {
        "blob" => {
            let ops = vec![Op::Write { file: p("data/lead/lead.state"), bytes: Payload::Bytes(blob(2).into()) }];
            based(d, ops, Expect::Confined(Default::default()))
        }
        "code" => {
            let (hats, song) = (d.read("tracks/hats.ts"), d.read("song.ts"));
            let ops = vec![
                replace("tracks/hats.ts", &hats, "export", "export default track(", "export const hatsTrack = track(").0,
                replace("song.ts", &song, "import hats", "import hats from", "import { hatsTrack as hats } from").0,
            ];
            // Committed on the delta its check shows (WRITE-11).
            let mut t = based(d, ops, Expect::Seen(Sha::of(b"")));
            let mut r = Session::open_read_only(&d.0, std_packages()).unwrap();
            t.expect = Expect::Seen(r.check(&t).unwrap().hash);
            t
        }
        "dataonly" => {
            let ops = vec![
                Op::Write { file: p("data/hats/extra.json"), bytes: Payload::Text("{\"clips\": []}\n".into()) },
                Op::Remove { file: p("data/kick/stray.json") },
                Op::Rename { from: p("data/old"), to: p("data/older") },
            ];
            based(d, ops, Expect::Confined(Confined::default()))
        }
        _ => several(d),
    }
}

/// What the session does before the killed commit: for `two`, a commit that completes, so its
/// journal still awaits its tidy; for `dataonly`, a save that breaks `song.ts` (WRITE-2), so the
/// project reopens with code that doesn't compile, whatever the commit reached.
fn prelude(case: &str, d: &Dir, s: &mut Session) {
    match case {
        "two" => {
            s.commit(pan(d, 0.4)).unwrap();
        }
        "dataonly" => {
            s.save(Path::new("song.ts"), Base::text(&d.read("song.ts")), "export default (;\n").unwrap();
        }
        _ => {}
    }
}

/// The child's part: in the project `M2_TX_KILL_DIR`, commit case `M2_TX_KILL_CASE` after its
/// prelude, aborting at its `M2_TX_KILL_AT`-th disk call.
#[test]
fn child() {
    let (Ok(dir), Ok(at), Ok(case)) = (std::env::var("M2_TX_KILL_DIR"), std::env::var("M2_TX_KILL_AT"), std::env::var("M2_TX_KILL_CASE"))
    else {
        return;
    };
    let d = Dir(PathBuf::from(dir));
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    prelude(&case, &d, &mut s);
    let t = tx(&case, &d);
    r.act(r.count() + at.parse::<usize>().unwrap(), Act::Abort);
    let _ = s.commit(t);
    std::mem::forget(d);
}

/// Run the child on `d`, killed at call `at`.
fn kill(d: &Dir, case: &str, at: usize) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tx::kill_tests::child", "--nocapture", "--test-threads", "1"])
        .env("M2_TX_KILL_DIR", &d.0)
        .env("M2_TX_KILL_CASE", case)
        .env("M2_TX_KILL_AT", at.to_string())
        .output()
        .unwrap()
        .status
}

/// A copy of the whole folder `from`, `.music2/` included.
fn copy(from: &Dir) -> Dir {
    fn walk(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (f, t) = (e.path(), to.join(e.file_name()));
            if e.file_type().unwrap().is_dir() { walk(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
        }
    }
    let d = Dir::new();
    walk(&from.0, &d.0);
    d
}

/// A project's files and their bytes.
type Tree = std::collections::BTreeMap<String, Vec<u8>>;

/// The tree before the killed commit, after it completes, and how many disk calls it makes.
fn ends(case: &str) -> (Tree, Tree, usize) {
    let d = start(case);
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    prelude(case, &d, &mut s);
    let before = d.tree();
    let at = r.count();
    s.commit(tx(case, &d)).unwrap();
    (before, d.tree(), r.count() - at)
}

// WRITE-15, WRITE-16, C9, the step 6 gate: killed at every disk call of a commit, the project
// reopens either as it was (killed before the journal), or holding the commit with no diagnostic
// (every target renamed: the journal is tidied silently), or offering `tx.interrupted`, whose
// FinishTx (when every staged file is whole) and DiscardTx each leave a project that opens and
// holds every target's after, or before, bytes. They apply when the project reopens with code
// that doesn't compile too (6.5, risk 8): the half-applied `code`, and `dataonly`, whose song.ts
// was saved broken; recovery puts back recorded bytes whatever they compile to (P3).
#[test]
fn killed_at_every_commit_step_the_project_recovers() {
    for case in ["several", "blob", "two", "code", "dataonly"] {
        // `dataonly`'s song.ts doesn't compile before or after it.
        let compiles = case != "dataonly";
        let (before, after, n) = ends(case);
        let (mut offered, mut silent, mut finished) = (0, 0, 0);
        for k in 0..n {
            let a = start(case);
            let status = kill(&a, case, k);
            assert_eq!(status.signal(), Some(libc::SIGABRT), "{case} {k}: the kill point fired ({status:?})");
            let b = copy(&a);
            let mut s = Session::open(&a.0, std_packages()).unwrap_or_else(|e| panic!("{case} {k}: {e:?}"));
            let interrupted = s.problems().iter().find(|d| d.code == "tx.interrupted").cloned();
            let Some(problem) = interrupted else {
                let tree = a.tree();
                assert!(tree == before || tree == after, "{case} {k}: reopened with no diagnostic, half changed");
                silent += usize::from(tree == after);
                drop(s);
                assert!(journals(&a).is_empty(), "{case} {k}: a completed journal is tidied");
                continue;
            };
            offered += 1;
            assert!(problem.message.contains("interrupted"), "{}", problem.message);
            let fix = |code: &str| problem.fixes.iter().find(|f| f.code == code).map(|f| f.tx.clone());
            let discard = fix("discard-tx").unwrap_or_else(|| panic!("{case} {k}: Discard is always offered: {problem:?}"));
            if let Some(finish) = fix("finish-tx") {
                s.commit(finish).unwrap_or_else(|e| panic!("{case} {k}: finish: {e}"));
                assert_eq!(a.tree(), after, "{case} {k}: Finish holds every target's after bytes");
                assert!(s.problems().iter().all(|d| d.code != "tx.interrupted"));
                drop(s);
                assert!(journals(&a).is_empty(), "{case} {k}: no journal is left");
                assert_eq!(Session::open(&a.0, std_packages()).unwrap().accepted().has_code(), compiles, "{case} {k}");
                finished += 1;
            } else {
                drop(s);
            }
            // Discard through the session's fix, as the app's strip and `m2 fix` apply it.
            let mut s = Session::open(&b.0, std_packages()).unwrap();
            let offered = s
                .problems()
                .iter()
                .find(|d| d.code == "tx.interrupted")
                .unwrap()
                .fixes
                .iter()
                .find(|f| f.code == "discard-tx")
                .unwrap()
                .clone();
            assert_eq!(offered.tx, discard);
            // Applying a fix is committing its Tx (DIAG-1).
            s.commit(offered.tx).unwrap_or_else(|e| panic!("{case} {k}: discard: {e}"));
            assert_eq!(b.tree(), before, "{case} {k}: Discard holds every target's before bytes");
            drop(s);
            assert!(journals(&b).is_empty(), "{case} {k}: no journal is left");
            assert_eq!(Session::open(&b.0, std_packages()).unwrap().accepted().has_code(), compiles, "{case} {k}");
        }
        assert!(offered > 0 && silent > 0 && finished > 0, "{case}: {offered} offered, {silent} silent, {finished} finished of {n}");
    }
}

/// A journal of `targets` in the project at `d`, as this machine writes it.
fn journal(d: &Dir, id: &str, targets: Vec<JTarget>) -> Journal {
    let j = Journal {
        id: id.into(),
        tx: "tx-x".into(),
        host: "here".into(),
        host_id: crate::lock::host_id(),
        pid: 1,
        program: "music2".into(),
        label: ("a".into(), "b".into()),
        targets,
    };
    d.write(&format!(".music2/tx/{id}.json"), &j.to_json());
    j
}

fn file(path: &str, before: &str, after: &str) -> JTarget {
    JTarget::File {
        path: path.into(),
        before: Some(Sha::of(before.as_bytes())),
        after: Some(Sha::of(after.as_bytes())),
        text: Some(before.into()),
        blob: false,
        staged: None,
    }
}

// WRITE-16, R7, R8, R11, R16: a target changed by hand sets the journal aside with no fix; an older
// completed journal shadowed by a newer one on the same file is tidied, only the newer reported; a
// journal that doesn't parse is set aside and reported; another machine's is `tx.foreign`, left
// alone; a reader reports and changes nothing.
#[test]
fn journals_found_at_open() {
    let d = Dir::first_song();
    d.write("data/kick/a.json", "C");
    d.write("data/kick/b.json", "x");
    // J1 took a from A to B; J2 from B to C (renamed) and b from x to y (not yet).
    journal(&d, "100-1-0", vec![file("data/kick/a.json", "A", "B")]);
    journal(&d, "200-1-0", vec![file("data/kick/a.json", "B", "C"), file("data/kick/b.json", "x", "y")]);
    let r = Session::open_read_only(&d.0, std_packages()).unwrap();
    let found: Vec<_> = r.problems().iter().filter(|d| d.code.starts_with("tx.")).map(|d| (d.code, d.src.clone())).collect();
    assert_eq!(found, [("tx.interrupted", Some(".music2/tx/200-1-0.json".to_string()))]);
    drop(r);
    assert_eq!(journals(&d).len(), 2, "a reader changes nothing");
    let s = d.open();
    let found: Vec<_> = s.problems().iter().filter(|d| d.code.starts_with("tx.")).map(|d| d.src.clone().unwrap()).collect();
    assert_eq!(found, [".music2/tx/200-1-0.json"]);
    assert!(!d.0.join(".music2/tx/100-1-0.json").exists(), "the shadowed one is tidied");
    // A Tx touching an interrupted journal's target waits for it.
    let ops = vec![Op::Write { file: p("data/kick/b.json"), bytes: Payload::Text("z".into()) }];
    let mut s = s;
    let refused = s.commit(based(&d, ops, Expect::Confined(Default::default()))).unwrap_err();
    assert!(refused.to_string().contains("interrupted"), "{refused}");
    drop(s);

    // Changed by hand: set aside, no fix, nothing touched.
    let d = Dir::first_song();
    d.write("data/kick/a.json", "B");
    d.write("data/kick/b.json", "by hand");
    journal(&d, "300-1-0", vec![file("data/kick/a.json", "A", "B"), file("data/kick/b.json", "x", "y")]);
    let s = d.open();
    let p0 = s.problems().iter().find(|p| p.code == "tx.interrupted").unwrap();
    assert!(p0.fixes.is_empty() && p0.message.contains("data/kick/b.json"), "{p0:?}");
    assert!(d.0.join(".music2/tx/aside/300-1-0.json").exists());
    assert_eq!(d.read("data/kick/b.json"), "by hand");
    drop(s);

    // Doesn't parse; another machine's.
    let d = Dir::first_song();
    d.write(".music2/tx/400-1-0.json", "{not json");
    let mut other = journal(&d, "500-1-0", vec![file("data/kick/a.json", "A", "B")]);
    other.host_id = "ANOTHER-MAC".into();
    other.host = "here".into();
    d.write(".music2/tx/500-1-0.json", &other.to_json());
    let s = d.open();
    let codes: Vec<_> = s.problems().iter().filter(|p| p.code.starts_with("tx.")).map(|p| p.code).collect();
    assert_eq!(codes, ["tx.interrupted", "tx.foreign"]);
    assert!(d.0.join(".music2/tx/aside/400-1-0.json").exists() && d.0.join(".music2/tx/500-1-0.json").exists());
}

/// The fix `code` the interrupted commit `jid` offers in `s`, if it does.
fn offered(s: &Session, jid: &str, code: &str) -> Option<m2_song::Tx> {
    let src = format!(".music2/tx/{jid}.json");
    let problem = s.problems().iter().find(|d| d.code == "tx.interrupted" && d.src.as_deref() == Some(src.as_str()))?;
    problem.fixes.iter().find(|f| f.code == code).map(|f| f.tx.clone())
}

/// The directory hash of folder `rel` of `d` (WRITE-12).
fn folder(d: &Dir, rel: &str) -> Sha {
    super::stage::folder_sha(&mut super::stage::Real, &d.0, rel).unwrap().unwrap()
}

// WRITE-16, 6a.4, R15: while a commit is interrupted, a Tx overlapping its targets waits for it,
// not only one naming a target: moving the folder that holds one, or writing into a folder it
// renames, would leave the journal nothing it can finish or undo. Its fixes still apply.
#[test]
fn a_tx_overlapping_an_interrupted_commit_waits() {
    let d = Dir::first_song();
    d.write("data/a/one.json", "B");
    d.write("data/old/x.json", "X");
    d.write("data/gone/y.json", "{\"clips\": []}\n");
    journal(&d, "700-1-0", vec![file("data/a/one.json", "A", "B"), file("data/old/x.json", "X", "Y")]);
    let hash = folder(&d, "data/gone");
    journal(&d, "800-1-0", vec![JTarget::Rename { from: "data/gone".into(), to: "data/went".into(), hash, dir: true }]);
    let mut s = d.open();
    assert!(offered(&s, "700-1-0", "discard-tx").is_some() && offered(&s, "800-1-0", "finish-tx").is_some(), "{:?}", s.problems());
    let tree = d.tree();
    let none = || Expect::Confined(Confined::default());
    let txs = [
        based(&d, vec![Op::Rename { from: p("data/old"), to: p("data/older") }], none()),
        based(&d, vec![Op::Write { file: p("data/gone/new.json"), bytes: Payload::Text("{\"clips\": []}\n".into()) }], none()),
    ];
    for tx in txs {
        let r = s.commit(tx).unwrap_err();
        assert!(r.to_string().contains("was interrupted"), "{r}");
    }
    assert_eq!(d.tree(), tree);
    s.commit(offered(&s, "700-1-0", "discard-tx").unwrap()).unwrap();
    assert_eq!((d.read("data/a/one.json"), d.read("data/old/x.json")), ("A".into(), "X".into()));
    s.commit(offered(&s, "800-1-0", "finish-tx").unwrap()).unwrap();
    assert!(d.0.join("data/went/y.json").exists() && s.problems().iter().all(|p| p.code != "tx.interrupted"));
}

// WRITE-16, R7: a commit whose journal outlived its tidy (its unlink lost to a power cut), then
// undone by a newer commit, is complete: its targets are where its undo left them, so both
// journals are tidied with no diagnostic, whether it wrote, removed or renamed. Offering to finish
// it would redo what was undone.
#[test]
fn an_undone_commit_whose_journal_outlived_its_tidy_is_tidied() {
    let d = Dir::first_song();
    d.write("data/kick/stray.json", "x");
    d.write("data/kick/gone.json", "z");
    d.write("data/old/hits.json", "{\"clips\": []}\n");
    let hash = folder(&d, "data/old");
    let z = Some(Sha::of(b"z"));
    let removed = |before: Option<Sha>, after: Option<Sha>| JTarget::File {
        path: "data/kick/gone.json".into(),
        before,
        after,
        text: before.map(|_| "z".into()),
        blob: false,
        staged: after.map(|_| 1),
    };
    let rename = |from: &str, to: &str| JTarget::Rename { from: from.into(), to: to.into(), hash, dir: true };
    journal(&d, "100-1-0", vec![file("data/kick/stray.json", "x", "y"), removed(z, None), rename("data/old", "data/older")]);
    journal(&d, "200-1-0", vec![file("data/kick/stray.json", "y", "x"), removed(None, z), rename("data/older", "data/old")]);
    let tree = d.tree();
    let s = d.open();
    assert!(s.problems().iter().all(|p| !p.code.starts_with("tx.")), "{:?}", s.problems());
    assert!(journals(&d).is_empty(), "both tidied");
    assert_eq!(d.tree(), tree, "and nothing redone");
}

// WRITE-16, DIAG-1, 6a.4: a Finish that was itself interrupted is finished first (the older
// commit's fixes wait for it); then the older commit is complete, so it is tidied at once: no fix
// stays offered that can no longer apply, and its files take edits again.
#[test]
fn finishing_an_interrupted_finish_completes_both() {
    let d = Dir::first_song();
    let staged = |path: &str, before: &str, after: &str, k: usize| JTarget::File {
        path: path.into(),
        before: Some(Sha::of(before.as_bytes())),
        after: Some(Sha::of(after.as_bytes())),
        text: Some(before.into()),
        blob: false,
        staged: Some(k),
    };
    // J1 took a A→B, b X→Y and c P→Q; only a reached. J2, J1's Finish, reached b, not c.
    let (a, b, c) = ("data/kick/a.json", "data/kick/b.json", "data/kick/c.json");
    journal(&d, "100-1-0", vec![staged(a, "A", "B", 0), staged(b, "X", "Y", 1), staged(c, "P", "Q", 2)]);
    journal(&d, "200-1-0", vec![staged(b, "X", "Y", 0), staged(c, "P", "Q", 1)]);
    for (f, t) in [("100-1-0/0", "B"), ("100-1-0/1", "Y"), ("100-1-0/2", "Q"), ("200-1-0/0", "Y"), ("200-1-0/1", "Q")] {
        d.write(&format!(".music2/tx/{f}"), t);
    }
    for (f, t) in [("a", "B"), ("b", "Y"), ("c", "P")] {
        d.write(&format!("data/kick/{f}.json"), t);
    }
    let mut s = d.open();
    let older = offered(&s, "100-1-0", "finish-tx").unwrap();
    assert!(s.commit(older).unwrap_err().to_string().contains("200-1-0"), "the newer goes first");
    s.commit(offered(&s, "200-1-0", "finish-tx").unwrap()).unwrap();
    let files: Vec<String> = ["a", "b", "c"].iter().map(|f| d.read(&format!("data/kick/{f}.json"))).collect();
    assert_eq!(files, ["B", "Y", "Q"]);
    assert!(s.problems().iter().all(|p| !p.code.starts_with("tx.")), "{:?}", s.problems());
    let ops = vec![Op::Write { file: p("data/kick/a.json"), bytes: Payload::Text("Z".into()) }];
    s.commit(based(&d, ops, Expect::Confined(Default::default()))).unwrap();
    drop(s);
    assert!(journals(&d).is_empty());
}

// 6a.3, FMT-20: what a commit killed half way leaves that no journal accounts for (a journal's
// temporary file, a before-image's, a staging folder whose journal was tidied first) is swept when
// a writer next takes the lock. A reader changes nothing, and a journal set aside keeps its
// staging, for whoever checks it by hand.
#[test]
fn leftovers_of_a_killed_commit_are_swept() {
    let d = Dir::first_song();
    let part = format!(".music2/tx/blobs/.{}.100-1-0.part", Sha::of(b"x").hex());
    let left = [".music2/tx/.100-1-0.json.part", part.as_str(), ".music2/tx/100-1-1/0"];
    for f in left {
        d.write(f, "x");
    }
    journal(&d, "100-1-2", vec![file("data/kick/a.json", "A", "B")]);
    d.write("data/kick/a.json", "neither");
    d.write(".music2/tx/100-1-2/0", "B");
    drop(Session::open_read_only(&d.0, std_packages()).unwrap());
    assert!(left.iter().all(|f| d.0.join(f).exists()), "a reader leaves them");
    drop(d.open());
    assert!(left.iter().all(|f| !d.0.join(f).exists()), "swept");
    assert!(d.0.join(".music2/tx/aside/100-1-2.json").exists() && d.0.join(".music2/tx/100-1-2/0").exists());
}

// 6a.4, 6a.2 step 6: the before-images of journals left alone are kept by the tidy of `blobs/`,
// through this machine's commits and its close: another machine's, so Discard is still offered
// there, and one set aside.
#[test]
fn journals_left_alone_keep_their_before_images() {
    let d = Dir::first_song();
    let (old, new) = (vec![1u8; 64], vec![2u8; 64]);
    d.write_bytes("data/lead/lead.state", &new);
    let blob = format!(".music2/tx/blobs/{}", Sha::of(&old).hex());
    d.write_bytes(&blob, &old);
    let state = JTarget::File {
        path: "data/lead/lead.state".into(),
        before: Some(Sha::of(&old)),
        after: Some(Sha::of(&new)),
        text: None,
        blob: true,
        staged: None,
    };
    let mut other = journal(&d, "900-1-0", vec![state, file("data/kick/b.json", "x", "y")]);
    d.write("data/kick/b.json", "x");
    other.host_id = "ANOTHER-MAC".into();
    d.write(".music2/tx/900-1-0.json", &other.to_json());
    let mut s = d.open();
    assert!(s.problems().iter().any(|p| p.code == "tx.foreign"));
    s.commit(pan(&d, 0.4)).unwrap();
    s.commit(pan(&d, 0.5)).unwrap();
    drop(s);
    assert!(d.0.join(&blob).exists(), "kept for the machine that made it");
    // On that machine, Discard puts the state back from it.
    other.host_id = crate::lock::host_id();
    d.write(".music2/tx/900-1-0.json", &other.to_json());
    let mut s = d.open();
    s.commit(offered(&s, "900-1-0", "discard-tx").unwrap()).unwrap();
    assert_eq!(std::fs::read(d.0.join("data/lead/lead.state")).unwrap(), old);
}
