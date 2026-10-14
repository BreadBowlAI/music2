//! The watcher's settle rule (WRITE-19): outside writers (editors, agents, git, a synced folder)
//! are taken in only once their changes settle, so a file caught half-written, or a checkout
//! caught half-applied, never plays.
//!
//! A pure state machine, driven by polls of a stat walk and a clock, so it is tested with a fake
//! one. A poll that finds a revision input changed opens a batch, or extends it. The batch is
//! ready when nothing tracked changed for 150 ms, every changed input was the same on two polls in
//! a row, and git isn't writing (its `index.lock` younger than 10 s). The caller then reads one
//! snapshot and takes it in as one revision.
//!
//! What is tracked (R14 of the step 6 design): revision inputs by their stamp (the manifest,
//! code, the arrangement, lane files, sidecars, state blobs, and the assets some lane names);
//! other files under `assets/` only by being there, so a render growing in `assets/` holds
//! nothing back; nothing else (PLANE-3). Text inputs are hashed when their stamp changes, and
//! again on every poll while their mtime is within 2 s of when it was read (FMT-16's racy rule),
//! so a same-size rewrite within a coarse timestamp is never missed; a binary input is hashed only
//! once its stamp is the same on two polls, never while it grows.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m2_song::{DataPath, Sha};

/// How long the folder must be quiet (WRITE-19).
pub const QUIET: Duration = Duration::from_millis(150);
/// A git `index.lock` older than this was left by a git that crashed or was killed.
pub const GIT_STALE: Duration = Duration::from_secs(10);
/// The coarsest timestamp resolution of a volume a project may be on (FMT-16).
const RACY_NS: i128 = 2_000_000_000;

/// What identifies a file's version without reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    /// Nanoseconds since the epoch.
    pub mtime: i128,
    pub ctime: i128,
    pub inode: u64,
}

impl Stamp {
    pub fn of(meta: &std::fs::Metadata) -> Stamp {
        let ns =
            |t: std::io::Result<SystemTime>| t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as i128);
        #[cfg(unix)]
        let (ctime, inode) = {
            use std::os::unix::fs::MetadataExt;
            (meta.ctime() as i128 * 1_000_000_000 + meta.ctime_nsec() as i128, meta.ino())
        };
        #[cfg(not(unix))]
        let (ctime, inode) = (ns(meta.created()), 0);
        Stamp { size: meta.len(), mtime: ns(meta.modified()), ctime, inode }
    }

    /// The stamp of the file at `p`, if there is one.
    pub fn at(p: &Path) -> Option<Stamp> {
        std::fs::symlink_metadata(p).ok().map(|m| Stamp::of(&m))
    }
}

/// How a path is tracked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    /// A text revision input: by its stamp, then its hash.
    Text,
    /// A binary revision input: by its stamp, hashed once stable.
    Binary,
    /// Only by being there.
    Present,
}

/// One stat walk of a project: each tracked path, how, and its stamp.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub entries: BTreeMap<PathBuf, (Track, Stamp)>,
}

/// Walk the project at `dir` with `Snapshot::read`'s rules, reading stamps, never contents.
/// `named` is the assets some lane of the draft names.
pub fn listing(dir: &Path, named: &BTreeSet<PathBuf>) -> Listing {
    let mut out = Listing::default();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            if meta.is_dir() {
                if !name.starts_with('.') && name != "node_modules" {
                    stack.push(p);
                }
                continue;
            }
            let rel = p.strip_prefix(dir).expect("inside").to_path_buf();
            let track = match DataPath::parse(&rel) {
                DataPath::Manifest
                | DataPath::Code(_)
                | DataPath::Arrangement
                | DataPath::Lane { .. }
                | DataPath::PluginState { sidecar: true, .. } => Track::Text,
                DataPath::PluginState { sidecar: false, .. } => Track::Binary,
                DataPath::Asset(_) if named.contains(&rel) => Track::Binary,
                DataPath::Asset(_) => Track::Present,
                _ => continue,
            };
            out.entries.insert(rel, (track, Stamp::of(&meta)));
        }
    }
    out
}

/// What a path was when last taken in.
#[derive(Clone, Debug, PartialEq)]
enum Seen {
    Input { track: Track, stamp: Stamp, sha: Option<Sha>, read_ns: i128 },
    Present,
}

/// A path of an open batch: what it was at the last poll, its hash, and on how many polls in a
/// row since it was the same.
#[derive(Clone, Debug)]
struct Pending {
    now: Option<(Track, Stamp)>,
    sha: Option<Sha>,
    same: u32,
}

#[derive(Clone, Debug)]
struct Batch {
    last_change: Instant,
    paths: BTreeMap<PathBuf, Pending>,
}

/// Git's state for the project.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Git {
    /// How old `index.lock` is in the project's git folder, when it is there.
    pub lock_age: Option<Duration>,
}

/// What a poll found.
#[derive(Clone, Debug, PartialEq)]
pub enum Poll {
    Idle,
    Settling,
    /// The batch settled: these paths changed. Read one snapshot and take it in.
    Ready(BTreeSet<PathBuf>),
}

/// The settle rule's state.
pub struct Settle {
    quiet: Duration,
    baseline: BTreeMap<PathBuf, Seen>,
    batch: Option<Batch>,
    /// The stale `index.lock` already said, so it is said once until it goes.
    told: bool,
    /// A Notice for the session to show: `git.locked`, once.
    pub notice: Option<m2_song::Diagnostic>,
}

impl Settle {
    /// Start from `listing`, taken when the project was read (`read_ns`, wall clock).
    pub fn new(listing: &Listing, read_ns: i128) -> Settle {
        let baseline = listing.entries.iter().map(|(p, (t, s))| (p.clone(), seen(*t, *s, None, read_ns))).collect();
        Settle { quiet: QUIET, baseline, batch: None, told: false, notice: None }
    }

    /// Whether a batch is open.
    pub fn settling(&self) -> bool {
        self.batch.is_some()
    }

    /// One poll at `now` (and `wall_ns`, the wall clock in nanoseconds), given the stat walk,
    /// git's state and a way to hash a path.
    pub fn poll(&mut self, now: Instant, wall_ns: i128, listing: &Listing, git: Git, hash: &mut dyn FnMut(&Path) -> Option<Sha>) -> Poll {
        // A batch's own paths too: one that was there for a single poll, then went, is polled
        // again and settles as gone, instead of holding its batch open for good.
        let batch = self.batch.iter().flat_map(|b| b.paths.keys());
        let paths: BTreeSet<PathBuf> = listing.entries.keys().chain(self.baseline.keys()).chain(batch).cloned().collect();
        let mut changed: Vec<PathBuf> = vec![];
        for p in &paths {
            let now_entry = listing.entries.get(p).copied();
            let differs = match (self.baseline.get_mut(p), now_entry) {
                (None, None) => false,
                (Some(Seen::Present), Some((Track::Present, _))) => false,
                (Some(Seen::Input { track, stamp, sha, read_ns }), Some((t, s))) if *track == t => {
                    if *stamp != s {
                        true
                    } else if t == Track::Text && s.mtime > *read_ns - RACY_NS {
                        // Racy: a same-size rewrite within the timestamp's resolution shows only
                        // in its content (FMT-16).
                        let h = hash(p);
                        match sha {
                            None => {
                                *sha = h;
                                *read_ns = wall_ns;
                                false
                            }
                            Some(old) => {
                                let differs = h.is_some_and(|h| h != *old);
                                if !differs {
                                    *read_ns = wall_ns;
                                }
                                differs
                            }
                        }
                    } else {
                        false
                    }
                }
                _ => true,
            };
            if differs || self.batch.as_ref().is_some_and(|b| b.paths.contains_key(p)) {
                changed.push(p.clone());
            }
        }
        if changed.is_empty() && self.batch.is_none() {
            return Poll::Idle;
        }
        let batch = self.batch.get_or_insert_with(|| Batch { last_change: now, paths: BTreeMap::new() });
        for p in changed {
            let entry = listing.entries.get(&p).copied();
            match batch.paths.get_mut(&p) {
                Some(pending) => {
                    let mut same = pending.now == entry;
                    // A text input is hashed when its stamp changes, and while racy.
                    if same && entry.is_some_and(|(t, s)| t == Track::Text && s.mtime > wall_ns - RACY_NS) {
                        let h = hash(&p);
                        same = h == pending.sha;
                        pending.sha = h;
                    }
                    if same {
                        pending.same += 1;
                    } else {
                        pending.now = entry;
                        pending.same = 0;
                        if entry.is_some_and(|(t, _)| t == Track::Text) {
                            pending.sha = hash(&p);
                        }
                        batch.last_change = now;
                    }
                }
                None => {
                    let sha = if entry.is_some_and(|(t, _)| t == Track::Text) { hash(&p) } else { None };
                    batch.paths.insert(p, Pending { now: entry, sha, same: 0 });
                    batch.last_change = now;
                }
            }
        }
        // A binary input is hashed only once it is the same on two polls: never while it grows.
        for (p, pending) in batch.paths.iter_mut() {
            if pending.same >= 1 && pending.sha.is_none() && pending.now.is_some_and(|(t, _)| t == Track::Binary) {
                pending.sha = hash(p);
            }
        }
        let git_busy = match git.lock_age {
            Some(age) if age < GIT_STALE => true,
            Some(_) => {
                if !self.told {
                    self.told = true;
                    self.notice = Some(m2_song::Diagnostic::notice(
                        "git.locked",
                        "git's index.lock is more than 10 seconds old, so it was left by a git that stopped: outside changes are taken in \
                         without waiting for it. Remove the file once no git command is running",
                    ));
                }
                false
            }
            None => {
                self.told = false;
                false
            }
        };
        let quiet = now.duration_since(batch.last_change) >= self.quiet;
        let stable = batch.paths.values().all(|p| p.same >= 1);
        if !quiet || !stable || git_busy {
            return Poll::Settling;
        }
        let batch = self.batch.take().expect("open");
        let mut out = BTreeSet::new();
        for (p, pending) in batch.paths {
            match pending.now {
                Some((t, s)) => {
                    self.baseline.insert(p.clone(), seen(t, s, pending.sha, wall_ns));
                }
                None => {
                    self.baseline.remove(&p);
                }
            }
            out.insert(p);
        }
        Poll::Ready(out)
    }

    /// After a `Ready`, the snapshot read for it: which of the batch's `paths` read as they settled
    /// (`now` gives a path's hash in the snapshot, None when it isn't there). One whose content
    /// changed since was written to again after the batch settled: it isn't taken in, and goes on
    /// as changed at the next poll (the step 6 design's 8.2).
    pub fn confirm(&mut self, paths: &BTreeSet<PathBuf>, now: &dyn Fn(&Path) -> Option<Sha>) -> BTreeSet<PathBuf> {
        let mut out = BTreeSet::new();
        for p in paths {
            let settled = match self.baseline.get(p) {
                Some(Seen::Input { sha: Some(sha), .. }) => Some(Some(*sha)),
                Some(Seen::Input { sha: None, .. }) | Some(Seen::Present) => None,
                None => Some(None),
            };
            match settled {
                Some(sha) if sha != now(p) => {
                    self.baseline.remove(p);
                }
                _ => {
                    out.insert(p.clone());
                }
            }
        }
        out
    }

    /// The session wrote these paths itself (a commit, a save): their baseline becomes the stamps
    /// of its own files, and they leave an open batch, so its own writes make no batch. Nothing
    /// else changes: an outside write that landed on another path meanwhile, or one that replaced
    /// a path the session wrote (another inode), is still taken in (R12).
    pub fn wrote(&mut self, paths: &[(PathBuf, Option<Stamp>)], wall_ns: i128) {
        for (p, stamp) in paths {
            match stamp {
                Some(s) => {
                    let track = self.baseline.get(p).map_or(Track::Text, |b| match b {
                        Seen::Input { track, .. } => *track,
                        Seen::Present => Track::Present,
                    });
                    self.baseline.insert(p.clone(), seen(track, *s, None, wall_ns));
                }
                None => {
                    self.baseline.remove(p);
                }
            }
            if let Some(b) = &mut self.batch {
                b.paths.remove(p);
                if b.paths.is_empty() {
                    self.batch = None;
                }
            }
        }
    }
}

fn seen(track: Track, stamp: Stamp, sha: Option<Sha>, read_ns: i128) -> Seen {
    match track {
        Track::Present => Seen::Present,
        t => Seen::Input { track: t, stamp, sha, read_ns },
    }
}

/// The git folder of the project at `dir`: `.git` in it or in a folder above, following a `.git`
/// file's `gitdir:` line (worktrees, submodules).
pub fn git_dir(dir: &Path) -> Option<PathBuf> {
    let mut at = Some(dir);
    while let Some(d) = at {
        let dot = d.join(".git");
        if dot.is_dir() {
            return Some(dot);
        }
        if let Ok(text) = std::fs::read_to_string(&dot)
            && let Some(rest) = text.lines().next().and_then(|l| l.strip_prefix("gitdir:"))
        {
            let to = Path::new(rest.trim());
            return Some(if to.is_absolute() { to.to_path_buf() } else { d.join(to) });
        }
        at = d.parent();
    }
    None
}

/// Git's state in `git_dir` now.
pub fn git(git_dir: Option<&Path>) -> Git {
    let age = git_dir
        .and_then(|g| std::fs::metadata(g.join("index.lock")).ok())
        .and_then(|m| m.modified().ok())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or(Duration::ZERO));
    Git { lock_age: age }
}

/// The wall clock, in nanoseconds since the epoch.
pub fn wall_ns() -> i128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i128)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(size: u64, mtime: i128) -> Stamp {
        Stamp { size, mtime, ctime: mtime, inode: 1 }
    }

    fn list(entries: &[(&str, Track, Stamp)]) -> Listing {
        Listing { entries: entries.iter().map(|(p, t, s)| (PathBuf::from(p), (*t, *s))).collect() }
    }

    const OLD: i128 = 1_000 * 1_000_000_000;
    const WALL: i128 = 2_000 * 1_000_000_000;

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
        let replaced = list(&[
            ("song.ts", Track::Text, Stamp { inode: 9, ..stamp(11, OLD + 5) }),
            ("data/kick/hits.json", Track::Text, stamp(21, OLD)),
        ]);
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
            let l =
                list(&[("song.ts", Track::Text, stamp(10, OLD)), ("assets/render.wav", Track::Present, stamp(100 * i, OLD + i as i128))]);
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
}
