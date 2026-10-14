//! A commit's disk calls (WRITE-15, the step 6 design's 6a.2 and 6a.6): their order, a failure at
//! each of them, a rollback that fails, outside writers caught at the rename, and before-images
//! that share no inode with the project.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use m2_song::tx::{Confined, Declared, Expect, Op, Payload, Range};

use super::stage::{Disk, How, Kind, Real};
use super::tests::{Dir, based, p, pan, std_packages};
use crate::Session;

/// What a test does at a disk call.
#[derive(Clone)]
pub(crate) enum Act {
    Fail,
    Abort,
    /// Run something first: an outside writer at that moment.
    Before(Arc<dyn Fn() + Send + Sync>),
}

/// A disk that records its calls and acts at chosen ones (counted from 0, over the session's life).
#[derive(Clone, Default)]
pub(crate) struct Recorder {
    pub calls: Arc<Mutex<Vec<String>>>,
    pub n: Arc<AtomicUsize>,
    pub at: Arc<Mutex<Vec<(usize, Act)>>>,
    /// The volume can't swap or rename exclusively.
    pub plain: bool,
}

impl Recorder {
    fn tick(&self, what: String) -> io::Result<()> {
        let n = self.n.fetch_add(1, Ordering::SeqCst);
        self.calls.lock().unwrap().push(what);
        let act = self.at.lock().unwrap().iter().find(|(k, _)| *k == n).map(|(_, a)| a.clone());
        match act {
            Some(Act::Abort) => std::process::abort(),
            Some(Act::Fail) => Err(io::Error::other("injected")),
            Some(Act::Before(f)) => {
                f();
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub fn count(&self) -> usize {
        self.n.load(Ordering::SeqCst)
    }

    pub fn act(&self, at: usize, act: Act) {
        self.at.lock().unwrap().push((at, act));
    }
}

impl Disk for Recorder {
    fn read(&mut self, p: &Path) -> io::Result<Vec<u8>> {
        self.tick(format!("read {}", p.display()))?;
        Real.read(p)
    }
    fn kind(&mut self, p: &Path) -> io::Result<Option<Kind>> {
        self.tick(format!("kind {}", p.display()))?;
        Real.kind(p)
    }
    fn create(&mut self, p: &Path, bytes: &[u8]) -> io::Result<()> {
        self.tick(format!("create {}", p.display()))?;
        Real.create(p, bytes)
    }
    fn mkdirs(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("mkdirs {}", p.display()))?;
        Real.mkdirs(p)
    }
    fn sync_dir(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("sync_dir {}", p.display()))?;
        Real.sync_dir(p)
    }
    fn barrier(&mut self, file: &Path) -> io::Result<()> {
        self.tick(format!("barrier {}", file.display()))?;
        Real.barrier(file)
    }
    fn rename(&mut self, from: &Path, to: &Path, how: How) -> io::Result<()> {
        self.tick(format!("rename {} {} {how:?}", from.display(), to.display()))?;
        if self.plain && how != How::Plain {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "no swap here"));
        }
        Real.rename(from, to, how)
    }
    fn remove_file(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("remove_file {}", p.display()))?;
        Real.remove_file(p)
    }
    fn remove_dir(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("remove_dir {}", p.display()))?;
        Real.remove_dir(p)
    }
    fn remove_tree(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("remove_tree {}", p.display()))?;
        Real.remove_tree(p)
    }
    fn clone_file(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.tick(format!("clone_file {} {}", from.display(), to.display()))?;
        Real.clone_file(from, to)
    }
    fn list(&mut self, dir: &Path) -> io::Result<Vec<String>> {
        self.tick(format!("list {}", dir.display()))?;
        Real.list(dir)
    }
}

/// A session of the project at `d` whose commits go through `r`.
pub(crate) fn recorded(d: &Dir, r: &Recorder) -> Session {
    Session::opening(&d.0, std_packages(), Some("music2"), Box::new(r.clone())).unwrap_or_else(|e| panic!("{e:?}"))
}

/// The multi-file Tx of the kill test: a value in `song.ts`, a new lane file, a removed one, and a
/// folder renamed.
pub(crate) fn several(d: &Dir) -> m2_song::Tx {
    let mut t = pan(d, 0.45);
    let Expect::Values(_) = &t.expect else { unreachable!() };
    let Op::TextEdit { edits, .. } = &t.ops[0] else { unreachable!() };
    let m2_song::Pos::Byte(s) = edits[0].start else { unreachable!() };
    let ops = vec![
        t.ops[0].clone(),
        Op::Write { file: p("data/hats/extra.json"), bytes: Payload::Text("{\"clips\": []}\n".into()) },
        Op::Remove { file: p("data/kick/stray.json") },
        Op::Rename { from: p("data/old"), to: p("data/older") },
    ];
    let mut c = Confined::default();
    c.ranges.insert(p("song.ts"), Declared::Ranges(vec![Range::bytes(s, s + 3)]));
    t = based(d, ops, Expect::Confined(c));
    t
}

/// A project the multi-file Tx can run on.
pub(crate) fn project() -> Dir {
    let d = Dir::first_song();
    d.write("data/old/hits.json", "{\"clips\": []}\n");
    d.write("data/kick/stray.json", "{\"clips\": [{\"id\": \"s\", \"start\": 0, \"length\": 1, \"notes\": []}]}\n");
    d
}

/// The journals in `.music2/tx/`.
pub(crate) fn journals(d: &Dir) -> Vec<PathBuf> {
    let Ok(e) = std::fs::read_dir(d.0.join(".music2/tx")) else { return vec![] };
    e.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json") && !p.file_name().unwrap().to_string_lossy().starts_with('.'))
        .collect()
}

// WRITE-15, R1: the journal, the staged files and the blobs are written and synced, their folders
// synced, then one barrier; nothing in the project changes before it; then the renames, then each
// target's folder synced.
#[test]
fn the_order_of_a_commit() {
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    s.commit(several(&d)).unwrap();
    let calls = r.calls.lock().unwrap().clone();
    let tx = d.0.canonicalize().unwrap().join(".music2/tx");
    let at = |what: &str| calls.iter().position(|c| c.starts_with(what)).unwrap_or_else(|| panic!("no {what}: {calls:#?}"));
    let barrier = at("barrier");
    let first_rename_into_project =
        calls.iter().position(|c| c.starts_with("rename") && !c.split(' ').nth(2).unwrap().starts_with(&*tx.to_string_lossy())).unwrap();
    assert!(barrier < first_rename_into_project, "{calls:#?}");
    for c in &calls[..barrier] {
        if c.starts_with("create") || c.starts_with("clone_file") {
            assert!(
                c.split(' ').next_back().unwrap().starts_with(&*tx.to_string_lossy()),
                "only .music2/tx/ is written before the barrier: {c}"
            );
        }
    }
    let syncs: Vec<&String> = calls[..barrier].iter().filter(|c| c.starts_with("sync_dir")).collect();
    assert!(syncs.iter().any(|c| c.ends_with(".music2/tx")), "the tx folder's entries are synced: {syncs:?}");
    assert_eq!(calls.iter().filter(|c| c.starts_with("barrier")).count(), 1, "one barrier per commit");
    assert!(calls[first_rename_into_project..].iter().any(|c| c.starts_with("sync_dir")), "the renames are pushed");
    assert_eq!(journals(&d).len(), 1, "the journal stays until the next barrier");
    // The next commit's barrier covers this one's renames: its journal goes then.
    s.commit(pan(&d, 0.5)).unwrap();
    assert_eq!(journals(&d).len(), 1);
    drop(s);
    assert!(journals(&d).is_empty(), "closing tidies the last");
}

// 6a.6: a failure at any disk call before the first rename leaves the project and `.music2/tx/`
// as they were; one after it rolls the commit back; either way the result is an error, never a
// half commit.
#[test]
fn a_failure_at_any_call_changes_nothing() {
    let probe = project();
    let r = Recorder::default();
    let mut s = recorded(&probe, &r);
    let start = r.count();
    s.commit(several(&probe)).unwrap();
    let n = r.count() - start;
    drop(s);
    for k in 0..n {
        let d = project();
        let before = d.tree();
        let r = Recorder::default();
        let mut s = recorded(&d, &r);
        r.act(r.count() + k, Act::Fail);
        let result = s.commit(several(&d));
        let calls = r.calls.lock().unwrap().clone();
        match result {
            // A failure in the tidy of nothing or the folder syncs after the renames doesn't
            // undo a commit that landed.
            Ok(_) => {
                let failed = &calls[start + k];
                let best_effort = ["sync_dir", "remove", "list", "kind"].iter().any(|x| failed.starts_with(x));
                assert!(best_effort, "call {k} ({failed}) failed, yet it committed");
            }
            Err(_) => {
                assert_eq!(d.tree(), before, "call {k} ({}): the project is as it was", calls.get(k).map_or("", String::as_str));
                assert!(journals(&d).is_empty(), "call {k}: no journal is left");
                assert_eq!(s.accepted().number, 1);
            }
        }
    }
}

// 6a.6: a rollback that fails too leaves the journal, and the session says `tx.interrupted` at once.
#[test]
fn a_rollback_that_fails_is_reported_at_once() {
    // Where the second rename into the project is, in a commit that runs through.
    let probe = project();
    let pr = Recorder::default();
    let mut ps = recorded(&probe, &pr);
    let pstart = pr.count();
    ps.commit(several(&probe)).unwrap();
    let calls = pr.calls.lock().unwrap().clone();
    let tx = probe.0.canonicalize().unwrap().join(".music2/tx");
    let renames: Vec<usize> = calls
        .iter()
        .enumerate()
        .skip(pstart)
        .filter(|(_, c)| c.starts_with("rename") && !c.split(' ').nth(2).unwrap().starts_with(&*tx.to_string_lossy()))
        .map(|(i, _)| i - pstart)
        .collect();
    // Fail it, then the rollback's first step.
    let d = project();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let start = r.count();
    r.act(start + renames[1], Act::Fail);
    r.act(start + renames[1] + 1, Act::Fail);
    let e = s.commit(several(&d)).unwrap_err();
    assert!(e.to_string().contains("couldn't be undone"), "{e}");
    assert_eq!(s.problems()[0].code, "tx.interrupted", "{:?}", s.problems());
    assert_eq!(journals(&d).len(), 1);
}

// R9: an outside writer that replaces a target by rename between the pre-check and its swap is
// caught at the swap: the commit is a conflict, and the file holds the outside writer's bytes; one
// that makes a file at a name the commit creates keeps it.
#[test]
fn outside_writers_are_caught_at_the_rename() {
    for plain in [false, true] {
        let d = project();
        let r = Recorder { plain, ..Recorder::default() };
        let mut s = recorded(&d, &r);
        let tx = pan(&d, 0.45);
        let song = d.0.join("song.ts");
        // The swap is the first rename whose target is in the project.
        let probe = project();
        let pr = Recorder { plain, ..Recorder::default() };
        recorded(&probe, &pr).commit(pan(&probe, 0.45)).unwrap();
        let calls = pr.calls.lock().unwrap().clone();
        let at = calls.iter().position(|c| c.starts_with("rename") && c.contains("song.ts")).unwrap();
        let outside = song.clone();
        r.act(
            r.count() + at,
            Act::Before(Arc::new(move || {
                let tmp = outside.with_extension("tmp");
                std::fs::write(&tmp, "// someone else's\n").unwrap();
                std::fs::rename(&tmp, &outside).unwrap();
            })),
        );
        let e = s.commit(tx).unwrap_err();
        assert!(matches!(e, super::Refusal::Conflict { .. }), "{plain}: {e:?}");
        assert_eq!(d.read("song.ts"), "// someone else's\n", "{plain}");
        assert!(journals(&d).is_empty());

        // A file made where the commit creates one.
        let d = project();
        let r = Recorder { plain, ..Recorder::default() };
        let mut s = recorded(&d, &r);
        let made = d.0.join("data/hats/extra.json");
        let ops = vec![Op::Write { file: p("data/hats/extra.json"), bytes: Payload::Text("{\"clips\": []}\n".into()) }];
        let tx = based(&d, ops, Expect::Confined(Confined::default()));
        let probe = project();
        let pr = Recorder { plain, ..Recorder::default() };
        let ptx = based(
            &probe,
            vec![Op::Write { file: p("data/hats/extra.json"), bytes: Payload::Text("{\"clips\": []}\n".into()) }],
            Expect::Confined(Confined::default()),
        );
        recorded(&probe, &pr).commit(ptx).unwrap();
        let calls = pr.calls.lock().unwrap().clone();
        let at = calls.iter().position(|c| c.starts_with("rename") && c.contains("extra.json")).unwrap();
        r.act(r.count() + at, Act::Before(Arc::new(move || std::fs::write(&made, "mine").unwrap())));
        assert!(matches!(s.commit(tx).unwrap_err(), super::Refusal::Conflict { .. }));
        assert_eq!(d.read("data/hats/extra.json"), "mine");
    }
}

// R10: a binary target's before-image shares no inode with it: rewritten in place after the
// commit, the image is intact, and undo, once the file is back as the commit left it, restores the
// bytes from it.
#[test]
fn before_images_share_no_inode() {
    let d = project();
    let old: Vec<u8> = (0..4096u32).map(|i| (i * 7 % 251) as u8).collect();
    d.write_bytes("data/lead/lead.state", &old);
    let mut s = d.open();
    let new: Vec<u8> = old.iter().map(|b| b.wrapping_add(1)).collect();
    let ops = vec![Op::Write { file: p("data/lead/lead.state"), bytes: Payload::Bytes(new.clone().into()) }];
    s.commit(based(&d, ops, Expect::Confined(Confined::default()))).unwrap();
    let blob = d.0.join(format!(".music2/tx/blobs/{}", m2_song::Sha::of(&old).hex()));
    assert_eq!(std::fs::read(&blob).unwrap(), old, "the before-image");
    // Rewritten in place (the same inode).
    use std::io::Write;
    std::fs::OpenOptions::new().write(true).open(d.0.join("data/lead/lead.state")).unwrap().write_all(&[0; 16]).unwrap();
    assert_eq!(std::fs::read(&blob).unwrap(), old, "untouched by a write to the project's file");
    std::fs::write(d.0.join("data/lead/lead.state"), &new).unwrap();
    s.undo().unwrap();
    assert_eq!(std::fs::read(d.0.join("data/lead/lead.state")).unwrap(), old);
}

// 6a.6, WRITE-15: a rollback pushes every folder it renamed into or out of (a removed file put
// back, a created one unlinked, the folders above them) before its barrier, and the journal goes
// only after that barrier: never durably before the rollback is.
#[test]
fn a_rollback_is_pushed_before_its_journal_goes() {
    let probe = project();
    let pr = Recorder::default();
    let mut ps = recorded(&probe, &pr);
    let pstart = pr.count();
    ps.commit(several(&probe)).unwrap();
    let calls = pr.calls.lock().unwrap().clone();
    let tx = probe.0.canonicalize().unwrap().join(".music2/tx");
    let last = calls
        .iter()
        .enumerate()
        .skip(pstart)
        .filter(|(_, c)| c.starts_with("rename") && !c.split(' ').nth(2).unwrap().starts_with(&*tx.to_string_lossy()))
        .map(|(i, _)| i - pstart)
        .next_back()
        .unwrap();
    // The last rename into the project (the folder) fails: everything before it is rolled back.
    let d = project();
    let before = d.tree();
    let r = Recorder::default();
    let mut s = recorded(&d, &r);
    let start = r.count();
    r.act(start + last, Act::Fail);
    assert!(s.commit(several(&d)).is_err());
    assert_eq!(d.tree(), before);
    let calls = r.calls.lock().unwrap()[start + last..].to_vec();
    let dir = d.0.canonicalize().unwrap();
    let barrier = calls.iter().position(|c| c.starts_with("barrier")).unwrap_or_else(|| panic!("{calls:#?}"));
    for f in ["data/kick", "data/hats", "data", ""] {
        let sync = format!("sync_dir {}", dir.join(f).display());
        let sync = sync.trim_end_matches('/');
        assert!(calls[..barrier].iter().any(|c| c == sync), "{f} pushed before the barrier: {calls:#?}");
    }
    let gone = calls.iter().position(|c| c.starts_with("remove_file") && c.contains(".music2/tx/") && c.ends_with(".json")).unwrap();
    assert!(gone > barrier, "{calls:#?}");
}
