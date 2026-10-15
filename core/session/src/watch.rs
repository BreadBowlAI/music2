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
//! A poll sees the whole project, or only some paths ([`Scope`]): the server's watcher stats only
//! what the file system's events name, and walks everything now and then. Only a poll that saw
//! every path makes a batch ready: events can arrive late, so a change whose other half they
//! haven't named yet is never taken in half-applied ([`Settle::due`] says when to look).
//!
//! What is tracked (R14 of the step 6 design): revision inputs by their stamp (the manifest,
//! code, the arrangement, lane files, sidecars, state blobs, and the assets some lane names);
//! other files under `assets/` only by being there, so a render growing in `assets/` holds
//! nothing back; nothing else (PLANE-3). Text inputs are hashed when their stamp changes, and
//! again on every poll while their mtime is within 2 s of when it was read (FMT-16's racy rule),
//! so a same-size rewrite within a coarse timestamp is never missed; a binary input is hashed only
//! once its stamp is the same on two polls, never while it grows.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m2_song::{DataPath, Sha};

/// How long the folder must be quiet (WRITE-19).
pub const QUIET: Duration = Duration::from_millis(150);
/// A git `index.lock` older than this was left by a git that crashed or was killed.
pub const GIT_STALE: Duration = Duration::from_secs(10);
/// The coarsest timestamp resolution of a volume a project may be on (FMT-16).
const RACY_NS: i128 = 2_000_000_000;

/// What identifies a file's version without reading it: one type for the watcher and the hash
/// cache.
pub use m2_os::fs::Stamp;

/// How a path is tracked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    /// A text revision input: by its stamp, then its hash.
    Text,
    /// A binary revision input: by its stamp, hashed once stable.
    Binary,
    /// Only by being there: its stamp is never read (a walk lists it with the default one).
    Present,
}

/// One stat walk of a project, or of some paths of it: each tracked path, how, and its stamp.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    /// By path relative to the project, `/`-separated.
    pub entries: BTreeMap<String, (Track, Stamp)>,
    /// The files a snapshot lists as Ignored (`Snapshot::unknown`), as the walk found them: whole
    /// only in a walk of the whole project.
    pub unknown: BTreeSet<PathBuf>,
}

/// How the file at project path `rel` is tracked, if it is: `named` is the assets some lane of
/// the draft names.
pub fn track(rel: &str, named: &BTreeSet<String>) -> Option<Track> {
    track_of(&DataPath::parse(Path::new(rel)), rel, named)
}

fn track_of(d: &DataPath, rel: &str, named: &BTreeSet<String>) -> Option<Track> {
    Some(match d {
        DataPath::Manifest
        | DataPath::Code(_)
        | DataPath::Arrangement
        | DataPath::Lane { .. }
        | DataPath::PluginState { sidecar: true, .. } => Track::Text,
        DataPath::PluginState { sidecar: false, .. } => Track::Binary,
        DataPath::Asset(_) if named.contains(rel) => Track::Binary,
        DataPath::Asset(_) => Track::Present,
        _ => return None,
    })
}

/// Whether a walk enters a folder of this name: never dot folders (`.music2`, `.git`) or
/// `node_modules`, as `Snapshot::read`.
fn entered(name: &str) -> bool {
    !name.starts_with('.') && name != "node_modules"
}

/// Whether anything at project path `rel` can be tracked: none of its parts is a folder a walk
/// never enters. No tracked file has a part that starts with a dot either (FMT-14, keys).
pub fn walked(rel: &str) -> bool {
    rel.split('/').all(entered)
}

/// Walk the project at `dir` with `Snapshot::read`'s rules, reading stamps, never contents.
/// `named` is the assets some lane of the draft names. An asset only tracked by being there is
/// listed from its folder entry, never stat'd (the default stamp).
pub fn listing(dir: &Path, named: &BTreeSet<String>) -> Listing {
    let mut out = Listing::default();
    walk(dir, "", named, false, &mut out);
    out
}

/// [`listing`] with every file stat'd, the assets no lane names too: what a watcher starts from
/// before the session's read says which assets its lanes name.
pub fn listing_stamped(dir: &Path, named: &BTreeSet<String>) -> Listing {
    let mut out = Listing::default();
    walk(dir, "", named, true, &mut out);
    out
}

/// The stat walk of only `paths` (each project path, `/`-separated) with [`listing`]'s rules: a
/// tracked file is stat'd, a folder walked, a path with nothing there gives nothing.
pub fn listing_of(dir: &Path, paths: &BTreeSet<String>, named: &BTreeSet<String>) -> Listing {
    let mut out = Listing::default();
    // A path inside a folder walked already is in the listing.
    let mut walked_at: Option<&str> = None;
    let mut folders = BTreeSet::new();
    for p in paths {
        if p.is_empty() {
            walk(dir, "", named, false, &mut out);
            break;
        }
        if walked_at.is_some_and(|w| p.starts_with(w) && p.as_bytes().get(w.len()) == Some(&b'/')) || !walked(p) {
            continue;
        }
        // Only through folders a walk enters, each spelled as the disk spells it: never a link to
        // one, nor a name a disk that ignores capitals or Unicode forms still opens after a rename
        // changed only those (a rename's old name), which would give a file a second key.
        if !reached(dir, p, &mut folders) || !spelled(&dir.join(p)) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(dir.join(p)) else { continue };
        if meta.is_dir() {
            walk(dir, p, named, false, &mut out);
            walked_at = Some(p);
        } else if let Some(t) = track(p, named) {
            // As a walk lists it, so a poll of some paths and one of everything agree.
            let stamp = if t == Track::Present { Stamp::default() } else { Stamp::of(&meta) };
            out.entries.insert(p.clone(), (t, stamp));
        }
    }
    out
}

/// Whether every folder `rel` is in is one (not a link to one) spelled as the disk spells it, as a
/// walk from the project reaches it. `folders` holds those known to be.
fn reached(dir: &Path, rel: &str, folders: &mut BTreeSet<String>) -> bool {
    let mut at = 0;
    while let Some(i) = rel[at..].find('/') {
        let folder = &rel[..at + i];
        if !folders.contains(folder) {
            let p = dir.join(folder);
            if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) || !spelled(&p) {
                return false;
            }
            folders.insert(folder.to_string());
        }
        at += i + 1;
    }
    true
}

/// Whether the last name of `p` is spelled as the disk stores it (`m2_os::fs::stored_name`), or
/// there is nothing there, or the OS can't tell. A disk that ignores capitals or Unicode forms
/// opens `Song.ts` as `song.ts`.
fn spelled(p: &Path) -> bool {
    p.file_name().is_none_or(|name| m2_os::fs::stored_name(p).is_none_or(|stored| stored == name))
}

/// Walk the folder `from` of the project at `dir` ("" for the project) into `out`.
fn walk(dir: &Path, from: &str, named: &BTreeSet<String>, stat_all: bool, out: &mut Listing) {
    let mut stack = vec![from.to_string()];
    while let Some(at) = stack.pop() {
        let folder = if at.is_empty() { dir.to_path_buf() } else { dir.join(&at) };
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for e in entries.flatten() {
            // A name that isn't UTF-8 is no project path (`DataPath`), nor is anything under it.
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            let rel = if at.is_empty() { name.to_string() } else { format!("{at}/{name}") };
            // The folder entry's type, without a stat where the volume gives it; a link is never
            // followed, so a link to a folder is a file here, as for `Snapshot::read`.
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                if entered(name) {
                    stack.push(rel);
                }
                continue;
            }
            let d = DataPath::parse(Path::new(&rel));
            let Some(track) = track_of(&d, &rel, named) else {
                if m2_song::path::listed_unknown(&d, Path::new(&rel)) {
                    out.unknown.insert(PathBuf::from(rel));
                }
                continue;
            };
            let stamp = match (track, stat_all) {
                (Track::Present, false) => Stamp::default(),
                _ => match std::fs::symlink_metadata(folder.join(name)) {
                    Ok(m) => Stamp::of(&m),
                    Err(_) => continue,
                },
            };
            out.entries.insert(rel, (track, stamp));
        }
    }
}

/// What a poll looked at: every path of the project, or only some, and none of `except`.
#[derive(Clone, Copy, Debug)]
pub struct Scope<'a> {
    /// Only these paths, each with every path under it (a folder's): one in scope and absent from
    /// the listing is gone, one out of scope is as it was. None: every path.
    only: Option<&'a BTreeSet<String>>,
    /// Paths left as they were: the session wrote them while the listing was taken, so it may have
    /// either version of them ([`Settle::wrote`] says which is its own).
    except: &'a BTreeSet<String>,
}

static NONE: BTreeSet<String> = BTreeSet::new();

impl<'a> Scope<'a> {
    /// The whole project.
    pub fn all() -> Scope<'static> {
        Scope { only: None, except: &NONE }
    }

    /// Only `paths`, each with everything under it.
    pub fn paths(paths: &'a BTreeSet<String>) -> Scope<'a> {
        Scope { only: Some(paths), except: &NONE }
    }

    /// The same, without `paths`.
    pub fn except(self, paths: &'a BTreeSet<String>) -> Scope<'a> {
        Scope { except: paths, ..self }
    }

    /// Whether it is the whole project.
    pub fn whole(&self) -> bool {
        self.only.is_none()
    }
}

/// The entries of `map` at `q` or under it (`q/...`); every entry for "".
fn under<'m, V>(map: &'m BTreeMap<String, V>, q: &'m str) -> impl Iterator<Item = &'m String> + 'm {
    map.range::<str, _>((Bound::Included(q), Bound::Unbounded))
        .take_while(move |(k, _)| k.starts_with(q))
        .filter(move |(k, _)| q.is_empty() || k.len() == q.len() || k.as_bytes()[q.len()] == b'/')
        .map(|(k, _)| k)
}

/// What a path was when last taken in.
#[derive(Clone, Debug, PartialEq)]
enum Seen {
    Input { track: Track, stamp: Stamp, sha: Option<Sha>, read_ns: i128 },
    Present,
}

/// A path of an open batch: what it was at the last poll, its hash, and on how many polls in a
/// row since it was the same.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    now: Option<(Track, Stamp)>,
    sha: Option<Sha>,
    same: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct Batch {
    last_change: Instant,
    paths: BTreeMap<String, Pending>,
}

/// Git's state for the project.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Git {
    /// How old `index.lock` is in the project's git folder, when it is there.
    pub lock_age: Option<Duration>,
}

impl Git {
    /// Whether git is writing: a batch waits for it.
    pub fn busy(&self) -> bool {
        self.lock_age.is_some_and(|age| age < GIT_STALE)
    }
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
#[derive(Clone, Debug)]
pub struct Settle {
    quiet: Duration,
    baseline: BTreeMap<String, Seen>,
    batch: Option<Batch>,
    /// The stale `index.lock` already said, so it is said once until it goes.
    told: bool,
    /// A Notice for the session to show: `git.locked`, once.
    pub notice: Option<m2_song::Diagnostic>,
}

/// Whether `p`, last seen as `seen`, differs in `now` (a poll's entry for it). A text input whose
/// stamp is racy is hashed (FMT-16): its first hash is recorded, a later one compared.
fn differs(
    seen: Option<&mut Seen>,
    now: Option<(Track, Stamp)>,
    p: &str,
    wall_ns: i128,
    hash: &mut dyn FnMut(&Path) -> Option<Sha>,
) -> bool {
    match (seen, now) {
        (None, None) => false,
        (Some(Seen::Present), Some((Track::Present, _))) => false,
        (Some(Seen::Input { track, stamp, sha, read_ns }), Some((t, s))) if *track == t => {
            if *stamp != s {
                true
            } else if t == Track::Text && s.mtime > *read_ns - RACY_NS {
                // Racy: a same-size rewrite within the timestamp's resolution shows only in its
                // content (FMT-16).
                let h = hash(Path::new(p));
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
    }
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

    /// The paths of the open batch: every poll must stat them, scoped or not.
    pub fn batch_paths(&self) -> impl Iterator<Item = &str> {
        self.batch.iter().flat_map(|b| b.paths.keys().map(String::as_str))
    }

    /// Whether the open batch has been quiet long enough to be ready at `now`, if a poll that sees
    /// every path finds nothing else changed: when a watcher polling only some paths must look at
    /// all of them.
    pub fn due(&self, now: Instant) -> bool {
        self.due_at().is_some_and(|at| now >= at)
    }

    /// When the open batch is due ([`Settle::due`]), if nothing else changes first.
    pub fn due_at(&self) -> Option<Instant> {
        self.batch.as_ref().map(|b| b.last_change + self.quiet)
    }

    /// One poll of the whole project at `now` (and `wall_ns`, the wall clock in nanoseconds),
    /// given the stat walk, git's state and a way to hash a path.
    pub fn poll(&mut self, now: Instant, wall_ns: i128, listing: &Listing, git: Git, hash: &mut dyn FnMut(&Path) -> Option<Sha>) -> Poll {
        self.poll_in(now, wall_ns, listing, Scope::all(), git, hash)
    }

    /// One poll of `scope`, whose paths `listing` stats: the same rule, applied to the paths it saw
    /// (and the open batch's, which every poll stats). Only a poll of the whole project makes a
    /// batch ready.
    pub fn poll_in(
        &mut self,
        now: Instant,
        wall_ns: i128,
        listing: &Listing,
        scope: Scope<'_>,
        git: Git,
        hash: &mut dyn FnMut(&Path) -> Option<Sha>,
    ) -> Poll {
        let changed = self.visit(wall_ns, listing, scope, hash);
        self.conclude(changed, now, wall_ns, listing, scope, git, hash)
    }

    /// The paths of `scope` that changed since last taken in, or are in the open batch (which
    /// every poll looks at again).
    fn visit(&mut self, wall_ns: i128, listing: &Listing, scope: Scope<'_>, hash: &mut dyn FnMut(&Path) -> Option<Sha>) -> Vec<String> {
        let mut changed: Vec<String> = vec![];
        let Settle { baseline, batch, .. } = self;
        // A batch's own paths too: one that was there for a single poll, then went, is polled
        // again and settles as gone, instead of holding its batch open for good.
        match scope.only {
            None => {
                // The listing, the baseline and the batch merged, each sorted, so nothing is
                // cloned or looked up but what changed.
                let mut b = baseline.iter_mut().peekable();
                let mut l = listing.entries.iter().peekable();
                let mut q = batch.iter().flat_map(|b| b.paths.keys()).peekable();
                loop {
                    let (kb, kl, kq) = (b.peek().map(|e| e.0.as_str()), l.peek().map(|e| e.0.as_str()), q.peek().map(|k| k.as_str()));
                    let Some(min) = [kb, kl, kq].into_iter().flatten().min() else { break };
                    let (hb, hl, hq) = (kb == Some(min), kl == Some(min), kq == Some(min));
                    let (be, le, qe) = (hb.then(|| b.next()).flatten(), hl.then(|| l.next()).flatten(), hq.then(|| q.next()).flatten());
                    let key: &String = be.as_ref().map(|e| e.0).or(le.map(|e| e.0)).or(qe).expect("one has it");
                    if scope.except.contains(key) {
                        continue;
                    }
                    if differs(be.map(|e| e.1), le.map(|e| e.1).copied(), key, wall_ns, hash) || hq {
                        changed.push(key.clone());
                    }
                }
            }
            Some(only) => {
                let mut keys: BTreeSet<String> = BTreeSet::new();
                for p in only {
                    keys.extend(under(baseline, p).cloned());
                    keys.extend(under(&listing.entries, p).cloned());
                }
                keys.extend(batch.iter().flat_map(|b| b.paths.keys()).cloned());
                for k in keys {
                    if scope.except.contains(&k) {
                        continue;
                    }
                    let in_batch = batch.as_ref().is_some_and(|b| b.paths.contains_key(&k));
                    if differs(baseline.get_mut(&k), listing.entries.get(&k).copied(), &k, wall_ns, hash) || in_batch {
                        changed.push(k);
                    }
                }
            }
        }
        changed
    }

    /// The rest of a poll, given what [`Settle::visit`] found: the batch opened or extended, and
    /// whether it is ready.
    #[allow(clippy::too_many_arguments)]
    fn conclude(
        &mut self,
        changed: Vec<String>,
        now: Instant,
        wall_ns: i128,
        listing: &Listing,
        scope: Scope<'_>,
        git: Git,
        hash: &mut dyn FnMut(&Path) -> Option<Sha>,
    ) -> Poll {
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
                        let h = hash(Path::new(&p));
                        same = h == pending.sha;
                        pending.sha = h;
                    }
                    if same {
                        pending.same += 1;
                    } else {
                        pending.now = entry;
                        pending.same = 0;
                        if entry.is_some_and(|(t, _)| t == Track::Text) {
                            pending.sha = hash(Path::new(&p));
                        }
                        batch.last_change = now;
                    }
                }
                None => {
                    let sha = if entry.is_some_and(|(t, _)| t == Track::Text) { hash(Path::new(&p)) } else { None };
                    batch.paths.insert(p, Pending { now: entry, sha, same: 0 });
                    batch.last_change = now;
                }
            }
        }
        // A binary input is hashed only once it is the same on two polls: never while it grows.
        for (p, pending) in batch.paths.iter_mut() {
            if pending.same >= 1 && pending.sha.is_none() && pending.now.is_some_and(|(t, _)| t == Track::Binary) {
                pending.sha = hash(Path::new(p));
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
        // A scoped poll can't tell that nothing else changed: events for the rest of a change may
        // still be on their way (WRITE-19).
        if !quiet || !stable || git_busy || !scope.whole() {
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
            out.insert(PathBuf::from(p));
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
            let key = m2_song::path::slashed(p);
            let settled = match self.baseline.get(&key) {
                Some(Seen::Input { sha: Some(sha), .. }) => Some(Some(*sha)),
                Some(Seen::Input { sha: None, .. }) | Some(Seen::Present) => None,
                None => Some(None),
            };
            match settled {
                Some(sha) if sha != now(p) => {
                    self.baseline.remove(&key);
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
            let p = m2_song::path::slashed(p);
            match stamp {
                Some(s) => {
                    let track = self.baseline.get(&p).map_or(Track::Text, |b| match b {
                        Seen::Input { track, .. } => *track,
                        Seen::Present => Track::Present,
                    });
                    self.baseline.insert(p.clone(), seen(track, *s, None, wall_ns));
                }
                None => {
                    self.baseline.remove(&p);
                }
            }
            if let Some(b) = &mut self.batch {
                b.paths.remove(&p);
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
mod scope_tests;
#[cfg(test)]
mod tests;
