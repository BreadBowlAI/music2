//! Taking in what changed on disk (WRITE-19, WRITE-13, WRITE-14, PLANE-1, PLANE-2), and the code
//! editor's save (WRITE-2).
//!
//! The draft is the last settled read of the folder. A batch is what one take-in changed in it,
//! and it is taken in whole or not at all, as one revision (WRITE-19):
//!
//! - No code, manifest or arrangement change: the data path, its data read against the accepted
//!   code with no QuickJS run, taken even while the draft's code is broken; `data_rev` moves.
//! - A code, manifest or arrangement change, and the draft builds: the whole draft, compiled
//!   when code or the manifest differ (PLANE-2), else the data path with its arrangement;
//!   `code_rev` and/or `data_rev` move, and nothing waits any more.
//! - A code, manifest or arrangement change, and the draft doesn't build: `draft_errors`, and
//!   every path of the batch waits; no revision.
//! - Anything, with no accepted code (opened broken): the whole draft must build, as above.
//!
//! A path that waits stays waiting until a batch makes the draft build, and a Tx touching one is
//! a conflict: so a lane moved together with a broken code change never plays empty.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_song::tx::{Base, Expect, Op, Payload, Tx, TxPath};
use m2_song::{DataPath, Diagnostic, LaneKind, Sha, Snapshot, Stored};

use crate::session::Session;
use crate::tx::stage::{Before, Target};
use crate::tx::{Kind, Refusal, TxStep, changed_paths, kind_of};

/// The plane a change takes (PLANE-1, PLANE-2): the slowest of its files'.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Plane {
    /// Nothing that makes a revision (PLANE-3).
    None,
    /// Notes and control lanes: swapped in.
    Data,
    /// Audio lanes, assets, plugin state: prepared off the audio thread.
    PreparedData,
    /// Tempo and meter.
    Arrangement,
    /// Code and the manifest: compile, check, a new plan.
    Code,
}

/// What a take-in accepted.
#[derive(Clone, Debug)]
pub struct Accepted {
    pub number: u64,
    pub code_rev: u64,
    pub data_rev: u64,
    pub plane: Plane,
    /// Session-only notices for the outside write: `data.claimed` (DIAG-3). Never in a project
    /// read.
    pub notices: Vec<Diagnostic>,
}

/// What a take-in did.
#[derive(Clone, Debug, Default)]
pub struct TakenIn {
    pub draft_changed: bool,
    pub accepted: Option<Accepted>,
}

/// What an outside batch is for undo (AGENT-2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Outside {
    /// No step, and nothing to say: nothing was accepted, or the batch only made binary files
    /// (new assets), which an undo would leave anyway.
    #[default]
    Nothing,
    /// One undo step, with this id.
    Step(u64),
    /// No step: git wrote it (the watcher waited for its `index.lock`), and git can undo it. How
    /// many files it changed.
    Git(usize),
    /// No step: undo couldn't put `file` back, for `why`, so nothing of the batch is undone; it
    /// changed `others` files besides.
    Cant { file: String, why: Cant, others: usize },
}

/// Why undo couldn't put a file of an outside batch back (AGENT-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cant {
    /// A binary file (an asset, a plugin's state), or one that couldn't be read: its bytes before
    /// are gone (the agents design's D5).
    Bytes,
    /// It is a link, or in a folder that is one: music2 never writes through one (WRITE-3a).
    Link,
    /// Its name is another file's in other capitals, or the disk spells it another way: a write
    /// of it would reach that file (WRITE-3a).
    Capitals,
    /// The batch is more than undo keeps (WRITE-21's 64 MiB).
    TooBig,
}

/// What outside writers changed in the draft that no revision holds yet (AGENT-2): each path a
/// watcher batch changed while it waits, and in `was` what the app knew there before the first of
/// those batches (nothing: it wasn't there). The outside step that takes them in puts that back. A
/// save takes its file out, so the musician's own text is never an outside change.
#[derive(Clone, Debug, Default)]
pub(crate) struct OutsideWas {
    pub(crate) paths: BTreeSet<PathBuf>,
    pub(crate) was: Snapshot,
}

/// What a save did.
#[derive(Clone, Debug)]
pub struct Saved {
    pub taken: TakenIn,
    /// Recorded for undo: it was taken into an accepted revision as written.
    pub recorded: bool,
}

/// What taking in a draft decides ([`Session::settling`]): built, before anything changes.
enum Settle {
    /// No revision: the draft is the accepted one (`clear`: nothing waits any more), or what
    /// changed waits already.
    Same { clear: bool },
    /// This snapshot, built so, is the new accepted revision (`clear`: nothing waits any more).
    Accept { built: Box<(Snapshot, m2_song::Project, crate::Compiled)>, clear: bool },
    /// It doesn't build: these paths wait, for these errors. `packs`: those the draft resolved when
    /// its code didn't build (None: the accepted revision's).
    Wait { errors: Vec<m2_ir::Diagnostic>, paths: BTreeSet<PathBuf>, packs: Option<Vec<(String, PathBuf)>> },
}

/// Only the entries of `paths` of `s`.
fn pick(s: &Snapshot, paths: &BTreeSet<PathBuf>) -> Snapshot {
    overlay(&Snapshot { dir: s.dir.clone(), ..Snapshot::default() }, s, paths)
}

/// `base` with the entries of `paths` taken from `from`.
fn overlay(base: &Snapshot, from: &Snapshot, paths: &BTreeSet<PathBuf>) -> Snapshot {
    let mut s = base.clone();
    for p in paths {
        match from.files.get(p) {
            Some(v) => s.files.insert(p.clone(), v.clone()),
            None => s.files.remove(p),
        };
        // Shared until changed: only a batch with a binary in it copies the map.
        if from.binaries.get(p) != s.binaries.get(p) {
            match from.binaries.get(p) {
                Some(v) => Arc::make_mut(&mut s.binaries).insert(p.clone(), v.clone()),
                None => Arc::make_mut(&mut s.binaries).remove(p),
            };
        }
        match from.unreadable.get(p) {
            Some(v) => s.unreadable.insert(p.clone(), v.clone()),
            None => s.unreadable.remove(p),
        };
        match from.not_utf8.get(p) {
            Some(v) => s.not_utf8.insert(p.clone(), *v),
            None => s.not_utf8.remove(p),
        };
    }
    s.unknown = from.unknown.clone();
    s
}

impl Session {
    /// Take in `snapshot`, read once a batch of outside changes settled (WRITE-19): by the table
    /// of the module.
    pub fn take_in(&mut self, snapshot: Snapshot) -> TakenIn {
        if snapshot == self.draft {
            return TakenIn::default();
        }
        let batch = changed_paths(&self.draft, &snapshot);
        let settled = self.settling(&snapshot, &batch);
        self.take_in_settled(snapshot, settled)
    }

    /// The store changed: a pack was installed (a Get, REG-15). The lists are made again, so they
    /// offer it (PACK-11); and a draft that waits (a song whose lock names a version that wasn't
    /// installed, `pkg.missing`) is built again, so it plays once it builds (REG-17).
    pub fn store_changed(&mut self) -> TakenIn {
        (self.catalog, self.offered) = crate::session::lists(&self.packages);
        self.problems = self.gather_problems();
        if self.draft_errors.is_empty() {
            return TakenIn::default();
        }
        let (draft, batch) = (self.draft.clone(), self.waiting.clone());
        let settled = self.settling(&draft, &batch);
        self.take_in_settled(draft, settled)
    }

    /// [`Session::take_in`] of `snapshot`, whose batch `settled` decided already.
    fn take_in_settled(&mut self, snapshot: Snapshot, settled: Settle) -> TakenIn {
        self.draft = snapshot;
        self.cache = None;
        let before = self.accepted.clone();
        let taken = self.settle(settled);
        let accepted = taken.then(|| {
            let after = self.accepted.clone();
            let plane = plane(&before, &after);
            let notices = crate::tx::claimed(&before, &after);
            Accepted { number: after.number, code_rev: after.code_rev, data_rev: after.data_rev, plane, notices }
        });
        if accepted.is_some() {
            self.last = accepted.clone();
        }
        TakenIn { draft_changed: true, accepted }
    }

    /// Take in `paths` as `snapshot` has them, and nothing else of it: the watcher's batch, once
    /// it settled (WRITE-19). A path that changed since and isn't in the batch stays as the draft
    /// has it, for the batch it is in, so a file someone is still writing is never taken in.
    /// `snapshot` may be a whole read or one of the paths alone (`Snapshot::read_paths`): the
    /// assets the lanes then name are hashed, the others listed, as a whole read lists them.
    pub fn take_in_only(&mut self, snapshot: Snapshot, paths: &BTreeSet<PathBuf>) -> TakenIn {
        // New assets a forwarded commit will name: the holder takes them in first (AGENT-1).
        if self.forward.is_some() {
            self.forward_assets.extend(paths.iter().filter(|p| p.starts_with(m2_song::path::ASSETS)).cloned());
        }
        let mut only = overlay(&self.draft, &snapshot, paths);
        crate::tx::settle_assets(&mut only);
        self.take_in(only)
    }

    /// Take in the watcher's batch, as [`Session::take_in_only`] does, and make what outside
    /// writers changed one undo step (AGENT-2), source outside. Its inverse puts back what the app
    /// knew of each file before they changed it ([`OutsideWas`]: a save that didn't build yet is the
    /// musician's, and stays), guarded by the hash the batch left, and removes each file they made,
    /// other than under `assets/`. `git`: the watcher saw git's `index.lock` while the batch
    /// settled. Only the watcher's job calls this: files the app takes in for its own Tx (an
    /// import's) make no step.
    pub fn take_in_batch(&mut self, snapshot: Snapshot, paths: &BTreeSet<PathBuf>, git: bool) -> (TakenIn, Outside) {
        if !self.access.writer() {
            // A window that can't undo keeps no history of what others change.
            self.outside_was = OutsideWas::default();
            return (self.take_in_only(snapshot, paths), Outside::Nothing);
        }
        // A path no longer waiting went into a revision since (a save or an undo made the draft
        // build): what the draft has there now is what the app knows.
        let waiting = &self.waiting;
        self.outside_was.paths.retain(|p| waiting.contains(p));
        let fresh: BTreeSet<PathBuf> = paths.difference(&self.outside_was.paths).cloned().collect();
        self.outside_was.was = overlay(&self.outside_was.was, &self.draft, &fresh);
        self.outside_was.paths.extend(fresh);
        let taken = self.take_in_only(snapshot, paths);
        if taken.accepted.is_none() {
            return (taken, Outside::Nothing);
        }
        // What went into the revision: every outside path that doesn't wait now.
        let waiting = &self.waiting;
        let (left, went): (BTreeSet<PathBuf>, BTreeSet<PathBuf>) =
            self.outside_was.paths.iter().cloned().partition(|p| waiting.contains(p));
        let was = pick(&self.outside_was.was, &went);
        self.outside_was = OutsideWas { was: pick(&self.outside_was.was, &left), paths: left };
        let is = &self.accepted.snapshot;
        let changed = changed_paths(&was, &pick(is, &went));
        if git && !changed.is_empty() && crate::watch::git_has(&self.dir, &changed) {
            return (taken, Outside::Git(changed.len()));
        }
        let others = changed.len().saturating_sub(1);
        let mut ops = vec![];
        let mut bases = vec![];
        for p in &changed {
            let name = m2_song::path::slashed(p);
            let cant = |why| Outside::Cant { file: name.clone(), why, others };
            // Its bytes before are gone, or were never read: nothing can put them back (D5).
            if binary(&was, p) || was.unreadable.contains_key(p) || is.unreadable.contains_key(p) {
                return (taken, cant(Cant::Bytes));
            }
            let Ok(file) = TxPath::new(&name) else { return (taken, cant(Cant::Bytes)) };
            let op = match text_of(&was, p) {
                Some(old) => Op::Write { file: file.clone(), bytes: Payload::Text(old.clone()) },
                // A file made under `assets/` stays: an asset no lane names plays nothing.
                None if matches!(file.data(), DataPath::Asset(_)) => continue,
                None => Op::Remove { file: file.clone() },
            };
            // Guarded by what the batch left there (WRITE-21).
            let left = match (is.files.get(p), is.binaries.get(p)) {
                (Some(t), _) => Base::Sha(is.not_utf8.get(p).copied().unwrap_or_else(|| Sha::of(t.as_bytes()))),
                (None, Some(Stored::Hash(sha))) => Base::Sha(*sha),
                (None, Some(_)) => return (taken, cant(Cant::Bytes)),
                (None, None) => Base::Absent,
            };
            // Refused as a commit would refuse it, it could never be undone (WRITE-3a).
            if let Some(why) = self.unwritable(&file, &left) {
                return (taken, cant(why));
            }
            bases.push((file, left));
            ops.push(op);
        }
        if ops.is_empty() {
            return (taken, Outside::Nothing);
        }
        let file = bases[0].0.to_string();
        let shown = match (&ops[..], text_of(&was, Path::new(&file)), text_of(is, Path::new(&file))) {
            ([_], Some(old), Some(new)) => summarize(old, new),
            ([_], ..) => (file.clone(), file.clone()),
            (many, ..) => (format!("{} files", many.len()), format!("{} files", many.len())),
        };
        let mut inverse = Tx::new("undo-outside", (shown.1, shown.0), ops, Expect::Identical);
        inverse.base.extend(bases);
        // Raw, as a save's step is: taken in like an outside write when undone (WRITE-21).
        let mut step = TxStep::new(inverse, true, false, file.clone());
        // More than undo keeps would push every other step out of it.
        if step.retained() > crate::session::RETAINED {
            return (taken, Outside::Cant { file, why: Cant::TooBig, others });
        }
        step.source = crate::session::Source::Outside;
        let id = self.record_tx(step);
        (taken, Outside::Step(id))
    }

    /// Why undo couldn't write `p` back over `base`, the way a commit would refuse it: a name that
    /// is another file's in other capitals, a link, or a name the disk spells another way
    /// (WRITE-3a).
    fn unwritable(&self, p: &TxPath, base: &Base) -> Option<Cant> {
        if crate::tx::taken_by(&self.accepted.snapshot, p, base).is_some() {
            return Some(Cant::Capitals);
        }
        match crate::tx::spelling(&self.dir, p) {
            Ok(()) => None,
            Err(crate::tx::Spelling::Link(_)) => Some(Cant::Link),
            Err(crate::tx::Spelling::Other(_)) => Some(Cant::Capitals),
            Err(crate::tx::Spelling::Io(_)) => Some(Cant::Bytes),
        }
    }

    /// What taking in `draft` decides, `batch` being what changed in it, as the module's table says:
    /// built, but nothing changed yet, so a save knows it before it writes.
    fn settling(&mut self, draft: &Snapshot, batch: &BTreeSet<PathBuf>) -> Settle {
        let accepted = self.accepted.clone();
        if accepted.has_code() && *draft == accepted.snapshot {
            return Settle::Same { clear: true };
        }
        let kinds: BTreeSet<Kind> = batch.iter().map(|p| kind_of(p)).collect();
        let code_like = kinds.contains(&Kind::Code) || kinds.contains(&Kind::Arrangement);
        if !code_like && accepted.has_code() {
            // The data path: the batch's data read against the accepted code (WRITE-14), its
            // paths that wait staying out.
            let take: BTreeSet<PathBuf> = batch.iter().filter(|p| !self.waiting.contains(*p)).cloned().collect();
            let snap = overlay(&accepted.snapshot, draft, &take);
            if take.is_empty() || snap == accepted.snapshot {
                return Settle::Same { clear: false };
            }
            return match self.build(&snap, Some(accepted.compiled.code().clone()), &accepted.compiled.data) {
                Ok((project, compiled)) => Settle::Accept { built: Box::new((snap, project, compiled)), clear: false },
                Err(errors) => Settle::Wait { errors, paths: take, packs: None },
            };
        }
        // The whole draft, which must build: a compile when its code differs from the accepted.
        let code = !accepted.has_code() || changed_paths(&accepted.snapshot, draft).iter().any(|p| kind_of(p) == Kind::Code);
        match self.build(draft, (!code).then(|| accepted.compiled.code().clone()), &accepted.compiled.data) {
            Ok((project, compiled)) => Settle::Accept { built: Box::new((draft.clone(), project, compiled)), clear: true },
            Err(errors) => Settle::Wait { errors, paths: batch.clone(), packs: crate::resolved(draft, &self.packages) },
        }
    }

    /// Do what [`Session::settling`] decided. Whether a revision was accepted.
    fn settle(&mut self, settled: Settle) -> bool {
        match settled {
            Settle::Same { clear } => {
                if clear {
                    self.draft_errors.clear();
                    self.draft_packs = None;
                    self.waiting.clear();
                }
                false
            }
            Settle::Accept { built, clear } => {
                let (snap, project, compiled) = *built;
                self.accept(snap, project, compiled);
                if clear {
                    self.draft_errors.clear();
                    self.draft_packs = None;
                    self.waiting.clear();
                }
                true
            }
            Settle::Wait { errors, paths, packs } => {
                self.draft_errors = errors;
                self.draft_packs = packs;
                self.waiting.extend(paths);
                false
            }
        }
    }

    /// Save `text` to `file` from the code editor (WRITE-2): a draft write, compared and swapped
    /// on `base`, written whole through the commit's staging, never refused for errors. It is
    /// then taken in with the draft (never a fresh read of the folder, R12), and recorded for
    /// undo when it was taken into an accepted revision as written: its undo writes the old text
    /// back while the file is as the save left it (WRITE-21).
    pub fn save(&mut self, file: &Path, base: Base, text: &str) -> Result<Saved, Refusal> {
        self.save_as(file, base, text, false)
    }

    /// [`Session::save`], written only when the song takes the file in as written, so it is one undo
    /// step: an agent's save (AGENT-1, WRITE-2), which `m2 edit` and `m2 write` make while the code
    /// on disk doesn't build. What it would make is settled before anything is written; one the song
    /// wouldn't take is refused with why (its build's errors), and nothing is written. A read-only
    /// session given the lock's holder sends it there, and then reads what landed.
    pub fn save_built(&mut self, file: &Path, base: Base, text: &str) -> Result<Saved, Refusal> {
        if self.forward.is_none() {
            return self.save_as(file, base, text, true);
        }
        let p = TxPath::new(&m2_song::path::slashed(file))
            .map_err(|e| Refusal::Rejected { diagnostics: vec![Diagnostic::error("tx.path", e)], unexpected: None })?;
        let to = self.forward.as_mut().expect("a forwarding session");
        let landed = to.save(&p, &base, text)?;
        self.landed = Some(landed);
        let paths = BTreeSet::from([file.to_path_buf()]);
        let taken = match Snapshot::read_paths(&self.dir, &paths, &|_| true, BTreeSet::new()) {
            Ok(snapshot) => self.take_in_only(snapshot, &paths),
            Err(_) => TakenIn::default(),
        };
        Ok(Saved { taken, recorded: true })
    }

    fn save_as(&mut self, file: &Path, base: Base, text: &str, built: bool) -> Result<Saved, Refusal> {
        if let Some(r) = self.writable() {
            return Err(Refusal::Rejected { diagnostics: vec![r], unexpected: None });
        }
        let p = TxPath::new(&m2_song::path::slashed(file))
            .map_err(|e| Refusal::Rejected { diagnostics: vec![Diagnostic::error("tx.path", e)], unexpected: None })?;
        if !p.is_text() {
            let why = format!("{p} isn't a text file, so the code editor doesn't save it");
            return Err(Refusal::Rejected { diagnostics: vec![Diagnostic::error("tx.invalid", why)], unexpected: None });
        }
        if let Base::WithText { sha, text } = &base
            && Sha::of(text.as_bytes()) != *sha
        {
            let why = format!("the base text given for {p} isn't the one its hash says");
            return Err(Refusal::Rejected { diagnostics: vec![Diagnostic::error("tx.invalid", why)], unexpected: None });
        }
        let cur = self.read_current(&p)?;
        if base.sha() != cur.sha {
            let files = vec![crate::tx::ConflictFile { file: p.to_string(), current: cur.sha, clip: None, why: None }];
            return Err(Refusal::Conflict { files });
        }
        if cur.bytes.as_deref() == Some(text.as_bytes()) {
            return Ok(Saved { taken: TakenIn::default(), recorded: false });
        }
        let after = Sha::of(text.as_bytes());
        // Taken in with the draft as it was settled, this file replaced: decided first, so a save
        // that must go in as written (`built`) is refused before anything is written.
        let mut draft = self.draft.clone();
        crate::tx::put_file(&mut draft, file, Some(text.as_bytes()));
        let settled = (draft != self.draft).then(|| {
            let batch = changed_paths(&self.draft, &draft);
            self.settling(&draft, &batch)
        });
        if built {
            let why = match &settled {
                Some(Settle::Accept { built, .. }) if built.0.files.get(file).map(|t| &**t) == Some(text) => None,
                Some(Settle::Wait { errors, .. }) => Some(errors.iter().cloned().map(crate::problem).collect()),
                _ => Some(vec![Diagnostic::error("tx.waiting", format!("{p} waits for the song's code to build first"))]),
            };
            if let Some(diagnostics) = why {
                return Err(Refusal::Rejected { diagnostics, unexpected: None });
            }
        }
        let before = cur.bytes.clone().map(|bytes| Before { sha: cur.sha.expect("bytes have a hash"), bytes });
        let target = Target::Write { path: p.to_string(), before, bytes: Arc::from(text.as_bytes()), after };
        let tx = Tx::new("save", (p.to_string(), p.to_string()), vec![], Expect::Identical);
        self.stage(&tx, &[target], vec![], None)?;
        // Whatever outside writers did to it before, the file is the musician's now (AGENT-2).
        self.outside_was.paths.remove(file);
        let taken = match settled {
            Some(settled) => self.take_in_settled(draft, settled),
            None => TakenIn::default(),
        };
        let recorded = taken.accepted.is_some() && self.accepted.snapshot.files.get(file).map(|t| &**t) == Some(text);
        if recorded {
            let undo = match &cur.bytes {
                Some(old) => Op::Write { file: p.clone(), bytes: Payload::Text(String::from_utf8_lossy(old).into()) },
                None => Op::Remove { file: p.clone() },
            };
            let shown = summarize(cur.text.as_deref().unwrap_or(""), text);
            let mut inverse = Tx::new("undo-save", (shown.1, shown.0), vec![undo], Expect::Identical);
            inverse.base.insert(p.clone(), Base::Sha(after));
            self.record_tx(TxStep::new(inverse, true, false, p.to_string()));
        }
        Ok(Saved { taken, recorded })
    }
}

/// The text `s` has at `p`: none when it isn't there, or isn't UTF-8 (its entry in `files` isn't its
/// bytes then).
fn text_of<'a>(s: &'a Snapshot, p: &Path) -> Option<&'a Arc<str>> {
    s.files.get(p).filter(|_| !s.not_utf8.contains_key(p))
}

/// Whether `s` has at `p` what isn't text: a binary, or a text file that isn't UTF-8.
fn binary(s: &Snapshot, p: &Path) -> bool {
    s.binaries.contains_key(p) || s.not_utf8.contains_key(p)
}

/// The plane a change from `a` to `b` took: the slowest of its paths' (PLANE-2).
fn plane(a: &crate::session::Revision, b: &crate::session::Revision) -> Plane {
    let mut out = Plane::None;
    for p in changed_paths(&a.snapshot, &b.snapshot) {
        let this = match kind_of(&p) {
            Kind::Code => Plane::Code,
            Kind::Arrangement => Plane::Arrangement,
            Kind::None => Plane::None,
            Kind::Data => match DataPath::parse(&p) {
                DataPath::Lane { module, lane } => {
                    let audio = b.compiled.decls.lanes.iter().any(|(l, k)| l.module == module && l.lane == lane && *k == LaneKind::Audio);
                    if audio { Plane::PreparedData } else { Plane::Data }
                }
                _ => Plane::PreparedData,
            },
        };
        out = out.max(this);
    }
    out
}

/// What a whole-file change did, to show it and its undo: the one line it changed, or how
/// many. `pan: 0.3,` → `pan: 0.45,`; `3 lines` → `5 lines`.
fn summarize(old: &str, new: &str) -> (String, String) {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let side = |lines: &[&str]| match lines {
        [] => "nothing".to_string(),
        [one] => {
            let one = one.trim();
            if one.chars().count() > 48 { format!("{}…", one.chars().take(47).collect::<String>()) } else { one.to_string() }
        }
        many => format!("{} lines", many.len()),
    };
    (side(&a[head..a.len() - tail]), side(&b[head..b.len() - tail]))
}
