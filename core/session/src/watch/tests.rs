//! The settle rule's fake-clock tests: each poll sees the whole project (`Settle::poll`).

use super::*;

pub(super) fn stamp(size: u64, mtime: i128) -> Stamp {
    Stamp { size, mtime, ctime: mtime, inode: 1 }
}

pub(super) fn list(entries: &[(&str, Track, Stamp)]) -> Listing {
    Listing { entries: entries.iter().map(|(p, t, s)| (p.to_string(), (*t, *s))).collect(), ..Listing::default() }
}

pub(super) const OLD: i128 = 1_000 * 1_000_000_000;
pub(super) const WALL: i128 = 2_000 * 1_000_000_000;

// WRITE-19: a change every 40 ms keeps a batch settling; 150 ms of quiet with two equal polls
// makes it ready, as one batch.
#[test]
fn a_batch_is_ready_once_quiet_and_stable() {
    let t0 = Instant::now();
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/kick/hits.json", Track::Text, stamp(20, OLD))]);
    let mut s = Settle::new(&base, WALL);
    let mut hash = |_: &Path| Some(Sha::of(b"x"));
    assert_eq!(s.poll(t0, WALL, &base, Git::default(), &mut hash), Poll::Idle);
    // The lane file written in two halves, 40 ms apart, then left.
    let hits = |size: u64| {
        list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/kick/hits.json", Track::Text, stamp(size, OLD + size as i128))])
    };
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    assert_eq!(s.poll(at(40), WALL, &hits(5), Git::default(), &mut hash), Poll::Settling, "the first half");
    assert_eq!(s.poll(at(80), WALL, &hits(22), Git::default(), &mut hash), Poll::Settling, "the second half");
    for ms in [120, 160, 200] {
        assert_eq!(s.poll(at(ms), WALL, &hits(22), Git::default(), &mut hash), Poll::Settling, "{ms} ms: not quiet for 150 ms yet");
    }
    let l = hits(22);
    assert_eq!(s.poll(at(240), WALL, &l, Git::default(), &mut hash), Poll::Ready([PathBuf::from("data/kick/hits.json")].into()));
    let at = at(240);
    assert_eq!(s.poll(at + Duration::from_millis(50), WALL, &l, Git::default(), &mut hash), Poll::Idle, "taken: the baseline moved");
}

// WRITE-19, R13: a fresh `index.lock` holds a batch; one older than 10 s doesn't, and is said
// once as `git.locked`.
#[test]
fn git_holds_a_batch_while_it_writes() {
    let t0 = Instant::now();
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD))]);
    let mut s = Settle::new(&base, WALL);
    let mut hash = |_: &Path| Some(Sha::of(b"x"));
    let changed = list(&[("song.ts", Track::Text, stamp(11, OLD))]);
    let busy = Git { lock_age: Some(Duration::from_secs(1)) };
    s.poll(t0, WALL, &changed, busy, &mut hash);
    assert_eq!(s.poll(t0 + Duration::from_secs(1), WALL, &changed, busy, &mut hash), Poll::Settling);
    let stale = Git { lock_age: Some(Duration::from_secs(11)) };
    assert!(matches!(s.poll(t0 + Duration::from_secs(2), WALL, &changed, stale, &mut hash), Poll::Ready(_)));
    assert_eq!(s.notice.take().map(|n| n.code), Some("git.locked"));
    let again = list(&[("song.ts", Track::Text, stamp(12, OLD))]);
    s.poll(t0 + Duration::from_secs(3), WALL, &again, stale, &mut hash);
    s.poll(t0 + Duration::from_secs(4), WALL, &again, stale, &mut hash);
    assert!(s.notice.is_none(), "said once");
}

// WRITE-19 (the step 6 design's 8.2): a batch read once it settled is taken in only where the
// read is what settled; a file written to again since goes on, as changed at the next poll.
#[test]
fn a_path_written_after_it_settled_goes_on() {
    let t0 = Instant::now();
    let base = list(&[("a.ts", Track::Text, stamp(10, OLD)), ("b.ts", Track::Text, stamp(10, OLD))]);
    let mut s = Settle::new(&base, WALL);
    let mut hash = |p: &Path| Some(Sha::of(p.to_string_lossy().as_bytes()));
    let l = list(&[("a.ts", Track::Text, stamp(11, OLD)), ("b.ts", Track::Text, stamp(11, OLD))]);
    s.poll(t0, WALL, &l, Git::default(), &mut hash);
    s.poll(t0 + Duration::from_millis(50), WALL, &l, Git::default(), &mut hash);
    let Poll::Ready(paths) = s.poll(t0 + Duration::from_millis(200), WALL, &l, Git::default(), &mut hash) else { panic!() };
    // The read found a.ts as it settled, b.ts written again.
    let read = |p: &Path| Some(if p == Path::new("a.ts") { Sha::of(b"a.ts") } else { Sha::of(b"more") });
    assert_eq!(s.confirm(&paths, &read), [PathBuf::from("a.ts")].into());
    assert!(matches!(s.poll(t0 + Duration::from_millis(250), WALL, &l, Git::default(), &mut hash), Poll::Settling));
}

// R12: the session's own writes make no batch; an outside write elsewhere meanwhile is still
// taken in, and an open batch survives.
#[test]
fn own_writes_make_no_batch() {
    let t0 = Instant::now();
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/kick/hits.json", Track::Text, stamp(20, OLD))]);
    let mut s = Settle::new(&base, WALL);
    let mut hash = |_: &Path| Some(Sha::of(b"x"));
    // An outside writer changes the lane; the session commits song.ts.
    let l = list(&[("song.ts", Track::Text, stamp(11, OLD + 5)), ("data/kick/hits.json", Track::Text, stamp(21, OLD))]);
    s.poll(t0, WALL, &l, Git::default(), &mut hash);
    s.wrote(&[(PathBuf::from("song.ts"), Some(stamp(11, OLD + 5)))], WALL);
    assert!(s.settling(), "the outside write's batch stays open");
    s.poll(t0 + Duration::from_millis(50), WALL, &l, Git::default(), &mut hash);
    assert_eq!(
        s.poll(t0 + Duration::from_millis(200), WALL, &l, Git::default(), &mut hash),
        Poll::Ready([PathBuf::from("data/kick/hits.json")].into())
    );
    // A file replaced after the session wrote it (another inode) is taken in.
    let replaced =
        list(&[("song.ts", Track::Text, Stamp { inode: 9, ..stamp(11, OLD + 5) }), ("data/kick/hits.json", Track::Text, stamp(21, OLD))]);
    s.poll(t0 + Duration::from_millis(250), WALL, &replaced, Git::default(), &mut hash);
    s.poll(t0 + Duration::from_millis(300), WALL, &replaced, Git::default(), &mut hash);
    assert_eq!(
        s.poll(t0 + Duration::from_millis(450), WALL, &replaced, Git::default(), &mut hash),
        Poll::Ready([PathBuf::from("song.ts")].into())
    );
}

// R14: a file growing in assets/ that no lane names holds nothing back; a growing state blob
// is hashed only once stable.
#[test]
fn growing_binaries() {
    let t0 = Instant::now();
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("assets/render.wav", Track::Present, stamp(1, OLD))]);
    let mut s = Settle::new(&base, WALL);
    let mut hashed = vec![];
    let mut hash = |p: &Path| {
        hashed.push(p.to_path_buf());
        Some(Sha::of(b"x"))
    };
    for i in 0..5 {
        let l = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("assets/render.wav", Track::Present, stamp(100 * i, OLD + i as i128))]);
        assert_eq!(s.poll(t0 + Duration::from_millis(50 * i), WALL, &l, Git::default(), &mut hash), Poll::Idle, "poll {i}");
    }
    let blob = |size: u64| list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/lead/lead.state", Track::Binary, stamp(size, OLD))]);
    let mut s = Settle::new(&list(&[("song.ts", Track::Text, stamp(10, OLD))]), WALL);
    s.poll(t0, WALL, &blob(1), Git::default(), &mut hash);
    s.poll(t0 + Duration::from_millis(50), WALL, &blob(2), Git::default(), &mut hash);
    drop(hash);
    assert!(hashed.is_empty(), "not while it grows: {hashed:?}");
    let mut hashed = vec![];
    let mut hash = |p: &Path| {
        hashed.push(p.to_path_buf());
        Some(Sha::of(b"x"))
    };
    s.poll(t0 + Duration::from_millis(100), WALL, &blob(2), Git::default(), &mut hash);
    assert!(matches!(s.poll(t0 + Duration::from_millis(250), WALL, &blob(2), Git::default(), &mut hash), Poll::Ready(_)));
    assert_eq!(hashed, [PathBuf::from("data/lead/lead.state")], "hashed once, when stable");
}

// WRITE-19: a tracked path that is there for one poll only (a script's temporary file, a file
// moved away and back) settles as gone, and never holds its batch, or every change after it,
// open.
#[test]
fn a_path_there_for_one_poll_settles_as_gone() {
    let t0 = Instant::now();
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD))]);
    let mut s = Settle::new(&base, WALL);
    let mut hash = |_: &Path| Some(Sha::of(b"x"));
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let phantom = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("tracks/scratch.ts", Track::Text, stamp(5, OLD))]);
    assert_eq!(s.poll(at(0), WALL, &phantom, Git::default(), &mut hash), Poll::Settling);
    assert_eq!(s.poll(at(50), WALL, &base, Git::default(), &mut hash), Poll::Settling, "gone: polled again");
    assert_eq!(s.poll(at(200), WALL, &base, Git::default(), &mut hash), Poll::Ready([PathBuf::from("tracks/scratch.ts")].into()));
    assert_eq!(s.poll(at(250), WALL, &base, Git::default(), &mut hash), Poll::Idle);
    let edited = list(&[("song.ts", Track::Text, stamp(11, OLD))]);
    s.poll(at(300), WALL, &edited, Git::default(), &mut hash);
    s.poll(at(350), WALL, &edited, Git::default(), &mut hash);
    assert_eq!(s.poll(at(500), WALL, &edited, Git::default(), &mut hash), Poll::Ready([PathBuf::from("song.ts")].into()), "taken in");
}

// FMT-16, R14: a same-size rewrite within 2 s of when a text file was read shows in its hash.
#[test]
fn a_racy_rewrite_is_seen() {
    let t0 = Instant::now();
    let racy = stamp(10, WALL - 1_000_000_000);
    let base = list(&[("song.ts", Track::Text, racy)]);
    let mut s = Settle::new(&base, WALL);
    let mut content = Sha::of(b"one");
    let mut hash = |_: &Path| Some(content);
    assert_eq!(s.poll(t0, WALL, &base, Git::default(), &mut hash), Poll::Idle, "its hash recorded");
    content = Sha::of(b"two");
    let mut hash = |_: &Path| Some(content);
    assert_eq!(
        s.poll(t0 + Duration::from_millis(50), WALL, &base, Git::default(), &mut hash),
        Poll::Settling,
        "the same stamp, another content"
    );
}

#[test]
fn the_git_folder_is_found_above_and_through_a_gitdir_file() {
    let root = std::env::temp_dir().join(format!("m2-watch-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("songs/a")).unwrap();
    assert_eq!(git_dir(&root.join("songs/a")), Some(root.join(".git")));
    std::fs::create_dir_all(root.join("wt/b")).unwrap();
    std::fs::write(root.join("wt/.git"), "gitdir: ../.git/worktrees/wt\n").unwrap();
    assert_eq!(git_dir(&root.join("wt/b")), Some(root.join("wt/../.git/worktrees/wt")));
    std::fs::write(root.join(".git/index.lock"), "").unwrap();
    assert!(git(git_dir(&root.join("songs/a")).as_deref()).lock_age.is_some());
    std::fs::remove_dir_all(&root).ok();
}
