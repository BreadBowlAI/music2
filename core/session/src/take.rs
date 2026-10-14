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
use m2_song::{DataPath, Diagnostic, LaneKind, Sha, Snapshot};

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

/// What a save did.
#[derive(Clone, Debug)]
pub struct Saved {
    pub taken: TakenIn,
    /// Recorded for undo: it was taken into an accepted revision as written.
    pub recorded: bool,
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
        self.draft = snapshot;
        self.cache = None;
        let before = self.accepted.clone();
        let taken = self.settle_batch(&batch);
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
        let mut only = overlay(&self.draft, &snapshot, paths);
        crate::tx::settle_assets(&mut only);
        self.take_in(only)
    }

    /// Accept what `batch` changed in the draft, as the module's table says. Whether a revision
    /// was accepted.
    fn settle_batch(&mut self, batch: &BTreeSet<PathBuf>) -> bool {
        let accepted = self.accepted.clone();
        if accepted.has_code() && self.draft == accepted.snapshot {
            self.draft_errors.clear();
            self.waiting.clear();
            return false;
        }
        let kinds: BTreeSet<Kind> = batch.iter().map(|p| kind_of(p)).collect();
        let code_like = kinds.contains(&Kind::Code) || kinds.contains(&Kind::Arrangement);
        if !code_like && accepted.has_code() {
            // The data path: the batch's data read against the accepted code (WRITE-14), its
            // paths that wait staying out.
            let take: BTreeSet<PathBuf> = batch.iter().filter(|p| !self.waiting.contains(*p)).cloned().collect();
            let snap = overlay(&accepted.snapshot, &self.draft, &take);
            if take.is_empty() || snap == accepted.snapshot {
                return false;
            }
            return match self.build(&snap, Some(accepted.compiled.code().clone()), &accepted.compiled.data) {
                Ok((project, compiled)) => {
                    self.accept(snap, project, compiled);
                    true
                }
                Err(d) => {
                    self.draft_errors = d;
                    self.waiting.extend(take);
                    false
                }
            };
        }
        // The whole draft, which must build: a compile when its code differs from the accepted.
        let draft = self.draft.clone();
        let code = !accepted.has_code() || changed_paths(&accepted.snapshot, &draft).iter().any(|p| kind_of(p) == Kind::Code);
        match self.build(&draft, (!code).then(|| accepted.compiled.code().clone()), &accepted.compiled.data) {
            Ok((project, compiled)) => {
                self.accept(draft, project, compiled);
                self.draft_errors.clear();
                self.waiting.clear();
                true
            }
            Err(d) => {
                self.draft_errors = d;
                self.waiting.extend(batch.iter().cloned());
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
        let before = cur.bytes.clone().map(|bytes| Before { sha: cur.sha.expect("bytes have a hash"), bytes });
        let target = Target::Write { path: p.to_string(), before, bytes: Arc::from(text.as_bytes()), after };
        let tx = Tx::new("save", (p.to_string(), p.to_string()), vec![], Expect::Identical);
        self.stage(&tx, &[target], vec![], None)?;
        // Taken in with the draft as it was settled, this file replaced.
        let mut draft = self.draft.clone();
        crate::tx::put_file(&mut draft, file, Some(text.as_bytes()));
        let taken = self.take_in(draft);
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
