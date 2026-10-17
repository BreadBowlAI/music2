//! The editing session: the one authority that accepts revisions of a project.
//!
//! A [`Revision`] is immutable: one snapshot of the project's files (code, data, manifest), the
//! project parsed from it, and the graph compiled from it. Everything shown or played comes
//! from the accepted revision, so its code, graph and data always belong together. Files on disk
//! are the draft; they are taken in (`take`) only as a whole batch that builds, and otherwise the
//! accepted revision stays.
//!
//! Every write is a `save()` or a Tx (P4): the session holds no gesture (P10, WRITE-9). What a
//! gesture means is planned and asserted in `crates/editor`, which builds a Tx from the accepted
//! revision and commits it here. Undo commits each Tx's stored inverse (WRITE-21).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_ir::Diagnostic;
use m2_runtime::PackageSet;
use m2_script::Catalog;
use m2_script::sites::Site;
use m2_song::{Project, Sha, Snapshot, TempoMap};

use crate::lock::{Access, ReadOnly};
use crate::texts::Texts;
use crate::tx::journal::Journal;
use crate::tx::stage::{self, Disk};
use crate::tx::{Prepared, Refusal, TxStep};
use crate::{Compiled, Seen};

/// One accepted state of the project (WRITE-13): the pair `(code_rev, data_rev)`, each the
/// number of the revision whose files of that kind this one has, and `number`, the session's
/// sequence. A change to code or the manifest moves `code_rev`; one to data or the arrangement
/// only `data_rev`.
pub struct Revision {
    pub number: u64,
    /// 0: no code ever compiled (WRITE-20).
    pub code_rev: u64,
    pub data_rev: u64,
    pub snapshot: Snapshot,
    pub project: Project,
    pub compiled: Compiled,
}

impl Revision {
    /// A copy, sharing the build.
    pub fn duplicate(&self) -> Revision {
        Revision {
            number: self.number,
            code_rev: self.code_rev,
            data_rev: self.data_rev,
            snapshot: self.snapshot.clone(),
            project: self.project.clone(),
            compiled: self.compiled.clone(),
        }
    }

    /// Whether code has ever compiled into it: a project opened broken has none until it does.
    pub fn has_code(&self) -> bool {
        self.code_rev > 0
    }

    /// The tempo map, which reading the project checked.
    pub fn tempo(&self) -> TempoMap {
        self.compiled.data.tempo().clone()
    }
}

/// What an earlier revision looked like: what a client a few revisions behind names its controls
/// and calls in, which `crates/editor` carries forward (WRITE-12, WRITE-22).
pub struct Past {
    pub number: u64,
    pub snapshot: Snapshot,
    /// Its code's call sites, shared with its build.
    sites: Arc<[Site]>,
}

impl Past {
    /// The call sites its code had.
    pub fn sites(&self) -> &[Site] {
        &self.sites
    }
}

/// What an undo or a redo committed (WRITE-21): the revision it made, the file it is shown as,
/// and what it said before and says now (its Tx's label, as the builder made it).
#[derive(Clone, Debug)]
pub struct Stepped {
    pub number: u64,
    pub file: String,
    pub before: String,
    pub after: String,
    pub committed: crate::tx::Committed,
}

/// The step on top of undo or of redo, as the app names it (WRITE-21, gaps #29): the name the
/// window that made it gave, whether it adds or removes a part, and the file it is shown as (what
/// a step nobody named is called by).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepName {
    pub name: Option<String>,
    pub structural: bool,
    pub file: String,
}

/// Why an undo or a redo didn't go ahead.
#[derive(Debug)]
pub enum StepError {
    /// There is nothing to undo, or to redo.
    Nothing(&'static str),
    /// Its inverse touches code, and the draft doesn't compile (the draft rule, C7).
    DraftInvalid(Vec<Diagnostic>),
    /// The project is open read-only here (WRITE-17).
    ReadOnly(String),
    /// Its inverse was refused: a conflict names what changed since (WRITE-21).
    Refused(Refusal),
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            StepError::Nothing(what) => write!(f, "{what}"),
            StepError::DraftInvalid(d) => write!(f, "the project doesn't compile right now ({} problems), so it can't be edited", d.len()),
            StepError::ReadOnly(why) => write!(f, "{why}"),
            StepError::Refused(r) => write!(f, "{r}"),
        }
    }
}

/// How many past revisions an edit can be planned against.
const PAST: usize = 64;

/// How many changes undo goes back through; the oldest are forgotten first.
const HISTORY: usize = 500;

/// How many bytes of text and data undo keeps alive at most (WRITE-21); the oldest steps are
/// forgotten first, though never the newest.
const RETAINED: usize = 64 << 20;

pub struct Session {
    pub packages: PackageSet,
    pub(crate) dir: PathBuf,
    pub(crate) accepted: Arc<Revision>,
    /// The last settled read of the folder: what the watcher, a save or `refresh` last took in.
    pub(crate) draft: Snapshot,
    /// Why the draft doesn't compile, if it doesn't.
    pub draft_errors: Vec<Diagnostic>,
    /// Paths of changes held back with code (or an arrangement) that doesn't build: taken in with
    /// it once it does, never half (WRITE-19).
    pub(crate) waiting: BTreeSet<PathBuf>,
    pub(crate) past: VecDeque<Past>,
    /// Each Tx's inverse, newest last (WRITE-21).
    pub(crate) history: Vec<TxStep>,
    /// Undone changes, as their inverses, newest last. Any new edit clears it.
    pub(crate) redo: Vec<TxStep>,
    /// What every revision accepted so far declared, so data a rename left behind is named as
    /// its track was and offered only to lanes that read it.
    pub(crate) seen: Seen,
    /// Every component the packages declare, described once at open (PKG-11): what the effect
    /// browser offers and the effect planner adds. Packages don't change while a project is open.
    pub(crate) catalog: Arc<Catalog>,
    /// The session's own problems, then the packages' (`pkg.invalid`), then the accepted
    /// revision's.
    pub(crate) problems: Vec<m2_song::Diagnostic>,
    /// What this session may write (WRITE-17).
    pub(crate) access: Access,
    /// The session's own problems: interrupted commits, a lock it couldn't take.
    pub(crate) own: Vec<m2_song::Diagnostic>,
    /// Interrupted commits offering fixes: a Tx touching their targets waits for them.
    pub(crate) interrupted: Vec<Journal>,
    /// Before-images that journals left alone still name (another machine's, ones set aside):
    /// never tidied from `blobs/` here.
    pub(crate) kept: Vec<Sha>,
    /// Recent texts by hash: where a Tx's base text is found (WRITE-12).
    pub(crate) texts: Texts,
    /// Every disk call a commit makes, and a writer's tidy of completed journals.
    pub(crate) disk: Box<dyn Disk>,
    /// Journals of commits whose renames await the next barrier, then their tidy.
    pub(crate) pending: Vec<String>,
    pub(crate) host: String,
    pub(crate) host_id: String,
    pub(crate) program: String,
    /// What the last `check` built, for a `commit` of the same Tx on the same files.
    pub(crate) cache: Option<Prepared>,
    /// How many times the journals were read again ([`Session::rescan`]): what a commit planned
    /// before one must be judged anew after.
    pub(crate) scans: u64,
    /// What the last take-in accepted.
    pub(crate) last: Option<crate::take::Accepted>,
    /// Every path this session wrote since [`Session::take_wrote`] last asked, with the stamp of
    /// its own file there: what the watcher's settle rule must not take for an outside write.
    pub(crate) wrote: Vec<(PathBuf, Option<crate::watch::Stamp>)>,
    /// What the host does with a commit about to be written (PLANE-5).
    pub(crate) before_write: Option<crate::tx::BeforeWrite>,
}

/// A project that can't be read from its snapshot: the manifest's or the arrangement's errors.
fn unread(d: Vec<m2_song::Diagnostic>) -> Vec<Diagnostic> {
    d.into_iter().map(crate::diagnostic).collect()
}

/// The folder couldn't be read at all.
fn unreadable(e: anyhow::Error) -> Vec<Diagnostic> {
    vec![Diagnostic::error("manifest.invalid", format!("{e:#}"))]
}

impl Session {
    /// Open the project at `dir` (WRITE-20): for writing when its lock can be taken, else
    /// read-only (WRITE-17). It fails only when the folder can't be read, has no `music2.toml`,
    /// or its `format` isn't this app's (FMT-3): a project whose code doesn't compile opens with
    /// `draft_errors` and no accepted code, so the code editor can fix it.
    pub fn open(dir: &Path, packages: PackageSet) -> Result<Session, Vec<Diagnostic>> {
        Session::open_as(dir, packages, "music2")
    }

    /// [`Session::open`], with the program the lock names as its holder (`music2`, `m2`).
    pub fn open_as(dir: &Path, packages: PackageSet, program: &str) -> Result<Session, Vec<Diagnostic>> {
        Session::opening(dir, packages, Some(program), Box::new(stage::Real))
    }

    /// Open the project read-only: a reading command (WRITE-18). It takes the lock only for a
    /// journal scan, when it is free, and reports interrupted commits (the step 6 design's C19).
    pub fn open_read_only(dir: &Path, packages: PackageSet) -> Result<Session, Vec<Diagnostic>> {
        Session::opening(dir, packages, None, Box::new(stage::Real))
    }

    /// [`Session::open_as`], every disk call a commit makes going through `disk`: what tests of
    /// other crates fail, slow down or race (the `faults` feature).
    #[cfg(feature = "faults")]
    #[doc(hidden)]
    pub fn open_with_disk(dir: &Path, packages: PackageSet, program: &str, disk: Box<dyn Disk>) -> Result<Session, Vec<Diagnostic>> {
        Session::opening(dir, packages, Some(program), disk)
    }

    /// Open with `disk` for every disk call a commit makes (tests count, fail and kill them).
    pub(crate) fn opening(
        dir: &Path,
        packages: PackageSet,
        program: Option<&str>,
        mut disk: Box<dyn Disk>,
    ) -> Result<Session, Vec<Diagnostic>> {
        let mut packages = packages;
        let snapshot = Snapshot::read(dir).map_err(unreadable)?;
        // Only a folder that isn't a project, or one in another format, doesn't open (FMT-3).
        if let Err(d) = Project::from_snapshot(&snapshot)
            && (!snapshot.files.contains_key(Path::new(m2_song::path::MANIFEST)) || d.iter().any(|d| d.code.starts_with("format.")))
        {
            return Err(unread(d));
        }
        let dir = snapshot.dir.clone();
        let (host_id, host) = (crate::lock::host_id(), m2_os::host::name());
        let mut own = vec![];
        let mut interrupted = vec![];
        let mut kept = vec![];
        let mut pending = vec![];
        let access = match program {
            Some(program) => match crate::lock::acquire(&dir, program, crate::lock::RETRY) {
                Ok(lock) => {
                    own.extend(lock.notices.iter().cloned());
                    // Journals are read once the lock is ours: completed ones tidied, the rest
                    // reported with their fixes (WRITE-16, 6a.4).
                    let scan = crate::tx::journal::scan(&dir, &host_id, Some(&mut *disk), &[]);
                    own.extend(scan.problems);
                    interrupted = scan.interrupted;
                    kept = scan.keep;
                    pending = scan.awaiting;
                    Access::Writer(lock)
                }
                Err(why) => {
                    let a = Access::ReadOnly(why);
                    own.extend(a.refusal());
                    a
                }
            },
            None => {
                own.extend(journal_problems(&dir));
                Access::ReadOnly(ReadOnly::Asked)
            }
        };
        // Never fails (WRITE-20): a package module that doesn't load is `pkg.invalid`, and only
        // its components are missing from the catalog.
        let catalog = Arc::new(crate::catalog(&packages));
        let built = Project::from_snapshot(&snapshot)
            .map_err(unread)
            .and_then(|p| crate::compile_sources(&p, &mut packages, &Seen::default(), None).map(|c| (p, c)));
        let (accepted, draft_errors, waiting) = match built {
            Ok((project, compiled)) => {
                let rev = Revision { number: 1, code_rev: 1, data_rev: 1, snapshot: snapshot.clone(), project, compiled };
                (rev, vec![], BTreeSet::new())
            }
            Err(d) => {
                // No code: the empty revision, silent until the code compiles; everything waits.
                let project = Project::empty(&snapshot);
                let all = crate::tx::changed_paths(&Snapshot { dir: snapshot.dir.clone(), ..Snapshot::default() }, &snapshot);
                (Revision { number: 1, code_rev: 0, data_rev: 0, snapshot: snapshot.clone(), project, compiled: Compiled::none() }, d, all)
            }
        };
        let mut seen = Seen::default();
        seen.add(&accepted.compiled.ir);
        let mut texts = Texts::default();
        for (p, t) in &accepted.snapshot.files {
            if !accepted.snapshot.not_utf8.contains_key(p) {
                texts.put_shared(t);
            }
        }
        let mut s = Session {
            packages,
            dir,
            draft: snapshot,
            seen,
            accepted: Arc::new(accepted),
            draft_errors,
            waiting,
            past: VecDeque::new(),
            history: vec![],
            redo: vec![],
            catalog,
            problems: vec![],
            access,
            own,
            interrupted,
            kept,
            texts,
            disk,
            pending,
            host,
            host_id,
            program: program.unwrap_or("m2").to_string(),
            cache: None,
            scans: 0,
            last: None,
            wrote: vec![],
            before_write: None,
        };
        s.problems = s.gather_problems();
        Ok(s)
    }

    /// The last revision that compiled. Everything shown or played comes from it.
    pub fn accepted(&self) -> &Revision {
        &self.accepted
    }

    /// The accepted revision, shared: what a renderer built from it can keep.
    pub fn accepted_arc(&self) -> Arc<Revision> {
        self.accepted.clone()
    }

    /// Revision `number`, if it is still remembered: what a client a few revisions behind
    /// planned on. The accepted revision is [`Session::accepted`].
    pub fn past(&self, number: u64) -> Option<&Past> {
        self.past.iter().find(|p| p.number == number)
    }

    /// The snapshot of revision `number`, if it is the accepted one or still remembered.
    pub fn snapshot(&self, number: u64) -> Option<&Snapshot> {
        if number == self.accepted.number {
            return Some(&self.accepted.snapshot);
        }
        self.past(number).map(|p| &p.snapshot)
    }

    /// What every revision accepted so far declared: how data a rename left behind is named.
    pub fn seen(&self) -> &Seen {
        &self.seen
    }

    /// What needs attention though the song plays, with the fixes each offers (DIAG-1, DIAG-4):
    /// the session's own (an interrupted commit, a lock it couldn't take), a package module that
    /// doesn't load (`pkg.invalid`), then the accepted revision's problems, such as data no lane
    /// reads.
    pub fn problems(&self) -> &[m2_song::Diagnostic] {
        &self.problems
    }

    pub(crate) fn gather_problems(&self) -> Vec<m2_song::Diagnostic> {
        let packages = self.catalog.problems.iter().cloned().map(crate::problem);
        self.own.iter().cloned().chain(packages).chain(self.accepted.compiled.problems.iter().cloned()).collect()
    }

    /// Every component the packages declare, and where each is exported (PKG-11).
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// What this session may write (WRITE-17).
    pub fn access(&self) -> &Access {
        &self.access
    }

    /// Paths held back with code that doesn't build yet (WRITE-19).
    pub fn waiting(&self) -> &BTreeSet<PathBuf> {
        &self.waiting
    }

    /// The text whose hash is `sha`, if the session has it (WRITE-12).
    pub fn text(&self, sha: &Sha) -> Option<Arc<str>> {
        self.texts.get(sha)
    }

    /// The paths this session wrote since it was last asked (a commit, a save, an undo, a
    /// recovery), each with the stamp of its own file after its rename, None when gone or not
    /// known to be its own: for the watcher's [`crate::watch::Settle::wrote`] (WRITE-19, R12).
    pub fn take_wrote(&mut self) -> Vec<(PathBuf, Option<crate::watch::Stamp>)> {
        std::mem::take(&mut self.wrote)
    }

    /// What the last take-in of the disk accepted, if it accepted anything.
    pub fn last_taken(&self) -> Option<&crate::take::Accepted> {
        self.last.as_ref()
    }

    /// Try again to take the project's lock (the app's Retry): when it is free, the session can
    /// write, and interrupted commits are read first.
    pub fn retry_lock(&mut self) -> bool {
        if self.access.writer() {
            return self.writable().is_none();
        }
        match crate::lock::acquire(&self.dir, &self.program, std::time::Duration::ZERO) {
            Ok(lock) => {
                let scan = crate::tx::journal::scan(&self.dir, &self.host_id, Some(&mut *self.disk), &[]);
                self.own = lock.notices.iter().cloned().chain(scan.problems).collect();
                self.interrupted = scan.interrupted;
                self.kept = scan.keep;
                self.pending.extend(scan.awaiting);
                self.access = Access::Writer(lock);
                self.problems = self.gather_problems();
                true
            }
            Err(_) => false,
        }
    }

    /// Why a write is refused here, if it is, with writer access checked again first (WRITE-17):
    /// a lock whose file was removed while held (`git clean -fdx` takes `.music2/` away) excludes
    /// nobody, since the next opener makes a new one. It is taken again, making `.music2/` anew,
    /// or, when another process took it meanwhile, the session turns read-only naming it.
    pub(crate) fn writable(&mut self) -> Option<m2_song::Diagnostic> {
        if let Access::Writer(lock) = &self.access
            && !lock.holds(&self.dir)
        {
            match crate::lock::acquire(&self.dir, &self.program, std::time::Duration::ZERO) {
                Ok(lock) => {
                    self.access = Access::Writer(lock);
                    // What another writer left meanwhile is read as at any lock.
                    self.rescan();
                }
                Err(why) => {
                    self.access = Access::ReadOnly(why);
                    self.own.retain(|d| !d.code.starts_with("lock."));
                    self.own.extend(self.access.refusal());
                    self.problems = self.gather_problems();
                }
            }
        }
        self.access.refusal()
    }

    /// Give the lock up, keeping the session read-only: so another session of the same project
    /// (the app opening it again) can take it.
    pub fn unlock(&mut self) {
        if self.access.writer() {
            let keep = self.interrupted.iter().flat_map(|j| j.blobs()).chain(self.kept.iter().copied()).collect();
            stage::close(&mut *self.disk, &self.dir, &mut self.pending, &keep);
            self.access = Access::ReadOnly(ReadOnly::Asked);
        }
    }

    /// Take in a revision built from `snapshot`, with its numbers (WRITE-13): `code_rev` moves
    /// when code or the manifest differ from the accepted revision's, `data_rev` when data or the
    /// arrangement do.
    pub(crate) fn accept(&mut self, snapshot: Snapshot, project: Project, compiled: Compiled) -> u64 {
        self.accept_known(snapshot, project, compiled, &BTreeMap::new())
    }

    /// [`Session::accept`], given the hashes of texts it changed that are known already (a
    /// commit's own writes), so the text store doesn't hash them again.
    pub(crate) fn accept_known(&mut self, snapshot: Snapshot, project: Project, compiled: Compiled, known: &BTreeMap<PathBuf, Sha>) -> u64 {
        self.seen.add(&compiled.ir);
        let number = self.accepted.number + 1;
        let changed = crate::tx::changed_paths(&self.accepted.snapshot, &snapshot);
        let kinds: BTreeSet<crate::tx::Kind> = changed.iter().map(|p| crate::tx::kind_of(p)).collect();
        let no_code = !self.accepted.has_code();
        let code = no_code || kinds.contains(&crate::tx::Kind::Code) || kinds.is_empty();
        let data = no_code || kinds.contains(&crate::tx::Kind::Data) || kinds.contains(&crate::tx::Kind::Arrangement);
        let code_rev = if code { number } else { self.accepted.code_rev };
        let data_rev = if data { number } else { self.accepted.data_rev };
        for p in &changed {
            if let Some(t) = snapshot.files.get(p)
                && !snapshot.not_utf8.contains_key(p)
            {
                // The text is the snapshot's own, now shared by the store and its hash known.
                if let Some(sha) = known.get(p) {
                    Sha::remember(t, *sha);
                }
                self.texts.put_shared(t);
            }
        }
        let new = Arc::new(Revision { number, code_rev, data_rev, snapshot, project, compiled });
        let old = std::mem::replace(&mut self.accepted, new);
        let (old_number, old_snapshot, sites) = match Arc::try_unwrap(old) {
            Ok(r) => (r.number, r.snapshot, r.compiled.sites.clone()),
            Err(r) => (r.number, r.snapshot.clone(), r.compiled.sites.clone()),
        };
        self.past.push_back(Past { number: old_number, snapshot: old_snapshot, sites });
        if self.past.len() > PAST {
            self.past.pop_front();
        }
        self.problems = self.gather_problems();
        number
    }

    /// Pick up changes made on disk: code, data or manifest, read now as one batch, without the
    /// watcher's settle rule (the CLI and tests; the app's watcher settles first, WRITE-19). A
    /// batch is accepted whole when its code compiles, or without QuickJS when it changes only
    /// data; otherwise it waits whole, the accepted revision stays and `draft_errors` says why.
    /// Returns whether the draft changed.
    pub fn refresh(&mut self) -> bool {
        match Snapshot::read(&self.dir) {
            Ok(s) => self.take_in(s).draft_changed,
            Err(e) => {
                self.draft_errors = unreadable(e);
                true
            }
        }
    }

    /// A new Tx's undo step: remembered, ending any redo. It is named by the revision it made: the
    /// accepted one now.
    pub(crate) fn record_tx(&mut self, mut step: TxStep) {
        step.made = self.accepted.number;
        self.push_history(step);
        self.redo.clear();
    }

    /// Name the undo step of the change that made revision `made` (gaps #29): "Hats pan", and
    /// whether it adds or removes a part. Any depth of undo or redo. False when no step has it:
    /// it is gone, or nothing changed. The name follows the step through undo and redo.
    pub fn name_step(&mut self, made: u64, name: &str, structural: bool) -> bool {
        match self.history.iter_mut().chain(self.redo.iter_mut()).rev().find(|t| t.made == made) {
            Some(t) => {
                t.name = Some((name.to_string(), structural));
                true
            }
            None => false,
        }
    }

    /// The steps on top of undo and of redo, as the app names them (read only).
    pub fn step_names(&self) -> (Option<StepName>, Option<StepName>) {
        let named = |t: &TxStep| StepName {
            name: t.name.as_ref().map(|n| n.0.clone()),
            structural: t.name.as_ref().is_some_and(|n| n.1),
            file: t.file.clone(),
        };
        (self.history.last().map(named), self.redo.last().map(named))
    }

    fn push_history(&mut self, step: TxStep) {
        self.history.push(step);
        let mut kept: usize = self.history.iter().map(TxStep::retained).sum();
        while self.history.len() > HISTORY || (kept > RETAINED && self.history.len() > 1) {
            kept -= self.history.remove(0).retained();
        }
    }

    /// Undo the newest change made in this session: commit its stored inverse (WRITE-21), which
    /// undoes only what it did, or is refused naming what changed since.
    pub fn undo(&mut self) -> Result<Stepped, StepError> {
        self.step(false)
    }

    /// Redo the newest change undone, the same way.
    pub fn redo(&mut self) -> Result<Stepped, StepError> {
        self.step(true)
    }

    /// Whether undo would go ahead, judged without compiling (WRITE-21): writer access, the draft
    /// rule, no target waiting, and every base of its inverse applies. A change something else
    /// edited over stays, and can be undone again once that edit is taken back.
    pub fn can_undo(&self) -> bool {
        self.history.last().is_some_and(|t| self.can_tx(t))
    }

    pub fn can_redo(&self) -> bool {
        self.redo.last().is_some_and(|t| self.can_tx(t))
    }

    /// Commit the inverse on top of one stack; its own inverse goes on the other.
    fn step(&mut self, redo: bool) -> Result<Stepped, StepError> {
        let stack = if redo { &mut self.redo } else { &mut self.history };
        let Some(step) = stack.pop() else {
            return Err(StepError::Nothing(if redo { "nothing to redo" } else { "nothing to undo" }));
        };
        let tx = self.step_tx(&step);
        let code = tx.touched().iter().any(|p| matches!(p.class(), m2_song::Class::Code | m2_song::Class::Manifest));
        let refused = if let Some(r) = self.writable() {
            Some(StepError::ReadOnly(r.message))
        } else if code && !self.draft_errors.is_empty() {
            Some(StepError::DraftInvalid(self.draft_errors.clone()))
        } else {
            None
        };
        let result = match refused {
            Some(e) => Err(e),
            None => self.commit_tx(&tx, step.raw).map_err(StepError::Refused),
        };
        match result {
            Ok((done, inverse)) => {
                if let Some(mut inverse) = inverse {
                    // The step it pushes is the same change: it keeps its name.
                    (inverse.made, inverse.name) = (done.number, step.name.clone());
                    if redo { &mut self.history } else { &mut self.redo }.push(inverse);
                }
                let (before, after) = tx.label.clone();
                Ok(Stepped { number: done.number, file: step.file.clone(), before, after, committed: done })
            }
            Err(e) => {
                if redo { &mut self.redo } else { &mut self.history }.push(step);
                Err(e)
            }
        }
    }
}

/// Interrupted commits of the project at `dir`, as a reader reports them (WRITE-16, C19): its lock
/// is taken without waiting, held for the scan only and only when it is free, and nothing is
/// changed. A command that compiles without a session (`m2 check`, `m2 render`) says them too.
pub fn journal_problems(dir: &Path) -> Vec<m2_song::Diagnostic> {
    let Ok(dir) = dir.canonicalize() else { return vec![] };
    match crate::lock::probe(&dir) {
        Some(_held) => crate::tx::journal::scan(&dir, &crate::lock::host_id(), None, &[]).problems,
        None => vec![],
    }
}

/// The last commit's journal goes once its renames are durable: one more barrier at close.
impl Drop for Session {
    fn drop(&mut self) {
        self.unlock();
    }
}
