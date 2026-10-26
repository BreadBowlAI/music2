//! The watcher of the open project (WRITE-19): outside writers (editors, agents, git) are taken in
//! once their changes settle, by core's settle rule ([`m2_session::watch::Settle`]). A batch that
//! settled is read once and taken in whole by the session's actor; the session's own writes are
//! told to the rule by their stamps, so they make no batch.
//!
//! What a tick looks at:
//! - With events (the OS's file-system events, on a local volume): the paths they named, the open
//!   batch's, and what the session just wrote, each stat'd. An idle tick stats nothing. The whole
//!   project is walked at open, every 5 s as a backstop, when events were lost or too many came,
//!   and whenever a batch is due, since only a poll that saw everything makes a batch ready (a
//!   late event for another half of a change must never let the first half play alone).
//! - Without (a network volume, where events report only this computer's writes; events that
//!   won't start; events that missed changes twice in 10 minutes): a walk of everything, every
//!   tick while a batch is open, else every 50 ms to 1 s, so it uses at most about 5% of a core.
//!
//! A tick holds the watch lock only to plan what to look at and to apply what it found, never over
//! a stat ([`Plan::look`]): the actor, which tells it what the session wrote after every job,
//! never waits for a walk. What the session wrote while a tick looked is left out of that tick
//! (the listing may hold either version) and looked at again by the next.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use m2_session::Revision;
use m2_session::watch::{self, Git, Listing, Poll, Scope, Settle, Stamp, Track};
use m2_song::{AssetName, DataPath, LaneKind, Sha};

use super::events;

/// How often the whole project is walked while events name what changes.
const BACKSTOP: Duration = Duration::from_secs(5);
/// Without events, how long an idle tick waits for the next walk, at least and at most: 20 times
/// the last walk's time between them, so walks use at most about 5% of a core.
const WALK_MIN: Duration = Duration::from_millis(50);
const WALK_MAX: Duration = Duration::from_secs(1);
/// How long a change a walk found may wait for its event before it counts as missed: a file
/// written while open is named only when it is closed.
const GRACE: Duration = Duration::from_secs(60);
/// Events that missed this many changes within `MISSES_IN` aren't trusted on this volume until
/// the project is opened again.
const MISSES: usize = 2;
const MISSES_IN: Duration = Duration::from_secs(600);

/// What wakes the watcher's thread before its next tick: an event naming a path.
#[derive(Default)]
pub(crate) struct Wake {
    heard: std::sync::Mutex<bool>,
    told: std::sync::Condvar,
}

impl Wake {
    pub fn wake(&self) {
        *self.heard.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.told.notify_all();
    }

    /// What woke it is spent: a tick took what the events said (WRITE-19). An event after this
    /// wakes it again; one just before may too, for a tick that finds nothing new, never too few.
    /// One wake serves the whole graph, every stream: while a project opens, the old watch's tick
    /// can spend a wake the new project's stream made. That costs its first wait at most the
    /// watcher's 50 ms; nothing is lost, as the hints stay and a new watch's first tick walks all.
    pub fn spend(&self) {
        *self.heard.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }

    /// Wait until `until`, or, with `heard`, until an event named a path since the last wait that
    /// returned for one, or the last tick that took what they said. Whether one did.
    pub fn wait(&self, until: std::time::Instant, heard: bool) -> bool {
        let mut h = self.heard.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if heard && std::mem::take(&mut *h) {
                return true;
            }
            let now = std::time::Instant::now();
            if now >= until {
                return false;
            }
            h = self.told.wait_timeout(h, until - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

/// Where a tick learns what changed.
enum Source {
    /// File-system events name the paths.
    Events(Arc<events::Stream>),
    /// Only a walk of everything.
    Walk,
}

/// What a watcher does, for the gates and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WatchStats {
    /// Events name what changes (else every tick that looks walks everything).
    pub events: bool,
    /// Events delivered so far.
    pub hints: u64,
    /// Ticks that walked everything, that stat'd only some paths, and that looked at nothing.
    pub walks: u64,
    pub scoped: u64,
    pub idle: u64,
    /// Changes a walk found that no event named in time.
    pub misses: u64,
    /// Walks of everything that lost events asked for (their changes are no misses).
    pub rescans: u64,
    /// How long the last walk of everything took, in milliseconds.
    pub last_walk_ms: f64,
}

/// The settle rule's state for one open project.
pub(crate) struct Watch {
    /// The open project's session token: a batch of a project since closed is dropped.
    pub token: u64,
    dir: PathBuf,
    git: Option<PathBuf>,
    /// The accepted revision whose assets `named` lists.
    number: u64,
    /// The assets some audio lane plays: tracked by stamp. Any other file in `assets/` is tracked
    /// only by being there, so a render growing there holds nothing back (R14).
    named: Arc<BTreeSet<String>>,
    pub settle: Settle,
    /// What the rule said once to tell the user (`git.locked`).
    pub notice: Option<String>,
    source: Source,
    /// When the next walk of everything is due: the backstop with events, the walk without.
    pub(crate) next_walk: Instant,
    /// When the last tick planned.
    last_tick: Instant,
    /// Paths the next tick looks at, named or not: what the session wrote (an outside write may
    /// land on it right after), what a read found written again since it settled, and assets a
    /// lane names now or no longer.
    queued: BTreeSet<String>,
    /// What the session wrote since the tick in progress planned.
    wrote_since: BTreeSet<String>,
    /// Changes a walk found that no event named yet, and when.
    unnamed: Vec<(String, Instant)>,
    /// When events missed a change, within the last 10 minutes.
    missed: VecDeque<Instant>,
    pub stats: WatchStats,
    /// Whether the tick that planned last should look at `layout.json` (FMT-19), which no snapshot
    /// holds, so the settle rule never sees it: every tick of a walk, else when an event named it or
    /// the tick walks everything.
    layout: bool,
    /// `BACKSTOP` and `GRACE`, shorter in tests.
    pub(crate) backstop: Duration,
    pub(crate) grace: Duration,
    /// Tests: every event is lost, as on a volume whose events never come.
    #[cfg(test)]
    pub(crate) deaf: bool,
    /// Tests: run as each tick begins to look, with no lock held.
    #[cfg(test)]
    pub(crate) looking: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// The assets an audio clip of `rev` plays, as project paths.
fn named(rev: &Revision) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (addr, _) in rev.compiled.decls.lanes.iter().filter(|(_, k)| *k == LaneKind::Audio) {
        for clip in rev.compiled.data.audio(addr.module.as_str(), addr.lane.as_str()) {
            if let Ok(name) = AssetName::new(&clip.asset) {
                out.insert(m2_song::path::slashed(&DataPath::Asset(name).path()));
            }
        }
    }
    out
}

/// A stat walk taken before a project is read: what its watcher starts from, so a write that
/// lands while the session reads it is still seen as a change. Events start first, so one that
/// lands between the walk and the read is named too.
pub(crate) struct Before {
    listing: Listing,
    read_ns: i128,
    source: Source,
}

impl Before {
    /// `walk`: never use events (the gates' walk mode; `M2_WATCH=walk` says the same). Events wake
    /// `wake`.
    pub fn take(dir: &Path, walk: bool, wake: &Arc<Wake>) -> Before {
        let source = source(dir, walk, wake);
        Before { listing: watch::listing_stamped(dir, &BTreeSet::new()), read_ns: watch::wall_ns(), source }
    }
}

/// Events for `dir` when they can be trusted there, else walks.
fn source(dir: &Path, walk: bool, wake: &Arc<Wake>) -> Source {
    if walk || std::env::var("M2_WATCH").as_deref() == Ok("walk") {
        return Source::Walk;
    }
    let why = if !m2_session::lock::local_volume(dir) {
        "it may be shared with other computers (it is on a network drive, or on a disk music2 can't tell from one), and \
         file-system events name only this computer's writes"
            .to_string()
    } else {
        // Events spell the folder as the disk does.
        match dir.canonicalize().and_then(|d| events::Stream::start(&d, wake.clone())) {
            Ok(s) => return Source::Events(Arc::new(s)),
            Err(e) => format!("file-system events didn't start: {e}"),
        }
    };
    eprintln!("music2: watching {} by walking it, looking at every file for changes: {why}", dir.display());
    Source::Walk
}

/// What a tick looks at, planned under the watch lock and looked at outside it.
pub(crate) struct Plan {
    pub token: u64,
    dir: PathBuf,
    named: Arc<BTreeSet<String>>,
    git: Option<PathBuf>,
    /// Walk everything.
    all: bool,
    /// Walk everything unless git is writing: the open batch is due.
    due: bool,
    /// Lost events asked for the walk: what it finds is no miss.
    rescan: bool,
    /// Else these paths, each with what is under it.
    paths: BTreeSet<String>,
    /// The paths events named.
    named_by_events: BTreeSet<String>,
    stream: Option<Arc<events::Stream>>,
    #[cfg(test)]
    looking: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    deaf: bool,
}

/// A batch that settled: what the actor reads and takes in.
pub(crate) struct Ready {
    pub paths: BTreeSet<PathBuf>,
    /// The Ignored files the walk that made it ready listed (`Snapshot::unknown`).
    pub unknown: BTreeSet<PathBuf>,
    /// The assets a lane names, which the watcher tracked by their hashes.
    pub named: Arc<BTreeSet<String>>,
    /// Git wrote it: the rule waited for git's `index.lock` while it settled (AGENT-2).
    pub git: bool,
}

/// What a tick found.
pub(crate) struct Looked {
    listing: Listing,
    /// None: the whole project.
    scope: Option<BTreeSet<String>>,
    git: Git,
    /// After a walk of everything, what events had named by when it ended.
    after: BTreeSet<String>,
    took: Duration,
}

/// Whether an event naming one of `hints` covers project path `p`: names it, or a folder it is in.
fn covered(p: &str, hints: &BTreeSet<String>) -> bool {
    hints.iter().any(|h| p == h || (p.starts_with(h.as_str()) && p.as_bytes().get(h.len()) == Some(&b'/')))
}

impl Plan {
    /// Look at what was planned: the walk, or the stats, with no lock held.
    pub fn look(&self) -> Looked {
        #[cfg(test)]
        if let Some(f) = &self.looking {
            f();
        }
        let t = Instant::now();
        let git = watch::git(self.git.as_deref());
        let all = self.all || (self.due && !git.busy());
        let (listing, scope) = if all {
            if let Some(s) = &self.stream {
                s.flush();
            }
            (watch::listing(&self.dir, &self.named), None)
        } else {
            (watch::listing_of(&self.dir, &self.paths, &self.named), Some(self.paths.clone()))
        };
        let took = t.elapsed();
        // What events named by the end of a walk: a change it found that they haven't is either on
        // its way (in a later tick's hints) or missed.
        let after = match (all, &self.stream) {
            (true, Some(s)) => {
                s.flush();
                s.named()
            }
            _ => BTreeSet::new(),
        };
        #[cfg(test)]
        let after = if self.deaf { BTreeSet::new() } else { after };
        Looked { listing, scope, git, after, took }
    }
}

impl Watch {
    /// The watcher of the project `rev` opened as `token`, from the walk taken before it was read.
    pub fn new(token: u64, rev: &Revision, before: Before) -> Watch {
        let dir = rev.project.dir.clone();
        let named = named(rev);
        let mut listing = before.listing;
        for p in &named {
            if let Some(e) = listing.entries.get_mut(p) {
                e.0 = Track::Binary;
            }
        }
        let settle = Settle::new(&listing, before.read_ns);
        let events = !matches!(before.source, Source::Walk);
        Watch {
            token,
            git: watch::git_dir(&dir),
            dir,
            number: rev.number,
            named: Arc::new(named),
            settle,
            notice: None,
            source: before.source,
            // The first tick walks everything, as every tick did before events.
            next_walk: Instant::now(),
            last_tick: Instant::now(),
            queued: BTreeSet::new(),
            wrote_since: BTreeSet::new(),
            unnamed: vec![],
            missed: VecDeque::new(),
            stats: WatchStats { events, ..WatchStats::default() },
            layout: true,
            backstop: BACKSTOP,
            grace: GRACE,
            #[cfg(test)]
            deaf: false,
            #[cfg(test)]
            looking: None,
        }
    }

    /// What the tick at `now` looks at: None when there is nothing to (no event named a path, no
    /// batch is open, no walk is due).
    pub fn plan(&mut self, now: Instant) -> Option<Plan> {
        self.wrote_since.clear();
        self.last_tick = now;
        let settling = self.settle.settling();
        let mut plan = Plan {
            token: self.token,
            dir: self.dir.clone(),
            named: self.named.clone(),
            git: self.git.clone(),
            all: now >= self.next_walk,
            due: false,
            rescan: false,
            paths: BTreeSet::new(),
            named_by_events: BTreeSet::new(),
            stream: None,
            #[cfg(test)]
            looking: self.looking.clone(),
            #[cfg(test)]
            deaf: self.deaf,
        };
        match &self.source {
            Source::Events(stream) => {
                let h = stream.take();
                #[cfg(test)]
                let h = if self.deaf { events::Hints::default() } else { h };
                self.stats.hints = stream.events();
                plan.all |= h.rescan;
                plan.rescan = h.rescan;
                plan.due = self.settle.due(now);
                plan.named_by_events = h.paths;
                plan.stream = Some(stream.clone());
                // Their events may come once they are closed: each one that does is no miss.
                self.unnamed.retain(|(p, _)| !covered(p, &plan.named_by_events));
            }
            // Without events every tick of an open batch walks everything: only a walk sees every
            // path WRITE-19's quiet is judged over.
            Source::Walk => plan.all |= settling,
        }
        self.layout = match self.source {
            Source::Walk => true,
            Source::Events(_) => plan.all || covered(m2_song::path::LAYOUT, &plan.named_by_events),
        };
        if !plan.all && !plan.due && !settling && plan.named_by_events.is_empty() && self.queued.is_empty() {
            self.stats.idle += 1;
            self.count_misses(now);
            return None;
        }
        plan.paths = plan.named_by_events.iter().cloned().chain(std::mem::take(&mut self.queued)).collect();
        plan.paths.extend(self.settle.batch_paths().map(String::from));
        Some(plan)
    }

    /// Apply what the tick planned as `plan` found, at `now`: the batch that settled, if one did.
    pub fn apply(&mut self, plan: &Plan, looked: Looked, now: Instant) -> Option<Ready> {
        let except = std::mem::take(&mut self.wrote_since);
        let before: BTreeSet<String> = self.settle.batch_paths().map(String::from).collect();
        let scope = match &looked.scope {
            None => Scope::all(),
            Some(p) => Scope::paths(p),
        };
        let dir = self.dir.clone();
        let mut hash = |p: &Path| std::fs::read(dir.join(p)).ok().map(|b| Sha::of(&b));
        let polled = self.settle.poll_in(now, watch::wall_ns(), &looked.listing, scope.except(&except), looked.git, &mut hash);
        if let Some(n) = self.settle.notice.take() {
            self.notice = Some(n.message);
        }
        // Only a walk of everything makes a batch ready, so its listing has every Ignored file.
        let ready = match polled {
            Poll::Ready(paths) => {
                Some(Ready { paths, unknown: looked.listing.unknown, named: self.named.clone(), git: self.settle.by_git() })
            }
            Poll::Idle | Poll::Settling => None,
        };
        if looked.scope.is_some() {
            self.stats.scoped += 1;
        } else {
            self.stats.walks += 1;
            self.stats.last_walk_ms = looked.took.as_secs_f64() * 1e3;
            self.next_walk = now
                + match self.source {
                    Source::Events(_) => self.backstop,
                    Source::Walk => (looked.took * 20).clamp(WALK_MIN, WALK_MAX),
                };
            if plan.rescan {
                // Lost events asked for this walk: what it found was never named, and neither
                // was what waits for its event. Neither is a miss.
                self.stats.rescans += 1;
                self.unnamed.clear();
            } else if self.stats.events {
                // A change only the walk found: unless an event for it is on its way, events
                // missed it. What the tick looked at anyway (the session's own writes, an asset a
                // lane names now) changed as expected.
                let found = self
                    .settle
                    .batch_paths()
                    .map(String::from)
                    .chain(ready.iter().flat_map(|r| &r.paths).map(|p| m2_song::path::slashed(p)));
                let heard: BTreeSet<String> = plan.paths.union(&looked.after).cloned().collect();
                // A change of its change time alone (a hard link to it made or removed elsewhere)
                // changes no content, and is named only to a watch on the file itself: no miss.
                let settle = &self.settle;
                let ctime_only = |p: &str| match (settle.stamp(p), looked.listing.entries.get(p)) {
                    (Some(was), Some((_, now))) => was.ctime != now.ctime && Stamp { ctime: was.ctime, ..*now } == was,
                    _ => false,
                };
                let missed: Vec<String> =
                    found.filter(|p| !before.contains(p) && !except.contains(p) && !covered(p, &heard) && !ctime_only(p)).collect();
                for p in missed {
                    self.unnamed.push((p, now));
                }
            }
        }
        self.count_misses(now);
        ready
    }

    /// Count what waited for its event past the grace as missed; trust events no more after too
    /// many.
    fn count_misses(&mut self, now: Instant) {
        let grace = self.grace;
        let late: Vec<String> = self.unnamed.iter().filter(|(_, at)| now.duration_since(*at) >= grace).map(|(p, _)| p.clone()).collect();
        if late.is_empty() {
            return;
        }
        self.unnamed.retain(|(_, at)| now.duration_since(*at) < grace);
        self.stats.misses += late.len() as u64;
        self.missed.push_back(now);
        while self.missed.front().is_some_and(|t| now.duration_since(*t) > MISSES_IN) {
            self.missed.pop_front();
        }
        eprintln!("music2: file system events never named {} in {}", late.join(", "), self.dir.display());
        if self.missed.len() >= MISSES && self.stats.events {
            eprintln!("music2: watching {} by walking it from now on: its file system events missed changes", self.dir.display());
            self.source = Source::Walk;
            self.stats.events = false;
            self.next_walk = now;
        }
    }

    /// When the next tick should come sooner than the watcher's usual 50 ms: when the open batch
    /// becomes due, once (a batch git holds is looked at every tick anyway); and whether an event
    /// naming a path should wake it at once: only while no batch is open, since one that is ticks
    /// every 50 ms.
    pub fn wake_at(&self) -> (Option<Instant>, bool) {
        let due = self.settle.due_at().filter(|at| *at > self.last_tick);
        (due, !self.settle.settling())
    }

    /// Tests: have every event for a change made before now heard.
    #[cfg(test)]
    pub(crate) fn flush(&self) {
        if let Source::Events(s) = &self.source {
            s.flush();
        }
    }

    /// Whether the tick that planned last should look at `layout.json`.
    pub fn look_at_layout(&self) -> bool {
        self.layout
    }

    /// Look at `paths` again at the next tick, whether events name them or not.
    pub fn requeue(&mut self, paths: impl IntoIterator<Item = String>) {
        self.queued.extend(paths);
    }

    /// The session wrote `paths` itself (R12), and accepted `rev`.
    pub fn wrote(&mut self, paths: &[(PathBuf, Option<Stamp>)], rev: &Revision) {
        if !paths.is_empty() {
            self.settle.wrote(paths, watch::wall_ns());
            for (p, _) in paths {
                let p = m2_song::path::slashed(p);
                // A tick looking meanwhile may have either version; the next looks again, so an
                // outside write that replaced it right after is still seen (R12).
                self.wrote_since.insert(p.clone());
                self.queued.insert(p);
            }
        }
        if rev.number != self.number {
            let now = named(rev);
            // An asset a lane names now is tracked by its stamp, one no lane names only by being
            // there: the next tick looks at each again.
            self.queued.extend(now.symmetric_difference(&self.named).cloned());
            (self.number, self.named) = (rev.number, Arc::new(now));
        }
    }
}
