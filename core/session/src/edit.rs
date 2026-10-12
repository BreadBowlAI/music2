//! The editing session: the one authority that accepts revisions of a project.
//!
//! A [`Revision`] is immutable: one snapshot of the project's text files (code,
//! timeline, manifest), the project parsed from it, and the graph compiled from it.
//! Everything shown or played comes from the accepted revision, so its code, graph and
//! timeline always belong together. Files on disk are the draft; [`Session::refresh`]
//! accepts them only if they compile, and otherwise keeps the accepted revision.
//!
//! Edits are structured operations, never text. This slice supports one: set a
//! written number. An edit names the revision it was planned against. If the files
//! changed since, its target is carried forward only when no change touched it (its
//! call lies entirely outside the changed bytes and reads the same); otherwise it is
//! refused as stale. The edit is compiled as a candidate from memory and accepted only
//! if the new graph equals the old one with exactly that change; then the file is
//! written, if the disk still matches. Undo is the same operation, inverted.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

use m2_ir::{Diagnostic, Ir, Node, Op, Origin, Unit};
use m2_runtime::PackageSet;
use m2_script::sites::Site;
use m2_song::{Project, Snapshot};

use crate::{Compiled, compile_sources};

mod cable;
mod effect;
mod lens;
pub use cable::{Inlet, Outlet};
use lens::LiteralLens;

/// One accepted state of the project.
pub struct Revision {
    pub number: u64,
    pub snapshot: Snapshot,
    pub project: Project,
    pub compiled: Compiled,
}

/// A number written in the project's code that the graph can show as a knob.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    /// `file#call/arg`: the file, the call's position among the file's calls, and the
    /// argument path. Meaningful together with `revision`.
    pub id: String,
    /// The accepted revision this control was read from.
    pub revision: u64,
    pub file: String,
    pub line: u32,
    /// The current value, in the constant's units (written number × scale).
    pub value: f64,
    /// The text written in the code, e.g. `0.3`, or `2.5` in `khz(2.5)`.
    pub literal: String,
    pub unit: Option<Unit>,
    /// The constant is the written number times this.
    pub scale: f64,
    /// How many graph nodes use it (a helper called twice gives two).
    pub uses: usize,
    pub origin: Origin,
    lens: LiteralLens,
}

/// Where a previewed value lands in the accepted revision.
#[derive(Clone, Debug, PartialEq)]
pub struct Audition {
    /// The control's id in the accepted revision (it may differ from the id it was
    /// named by, if the files changed without touching it).
    pub id: String,
    pub origin: Origin,
    /// The value the preview plays, exactly as a commit would write it.
    pub value: f64,
    /// The value in the accepted revision, to return to if the preview is cancelled.
    pub current: f64,
}

/// An accepted source edit.
#[derive(Clone, Debug, PartialEq)]
pub struct Edited {
    pub revision: u64,
    pub file: String,
    /// What the edited range said before and says now.
    pub before: String,
    pub after: String,
}

/// An accepted value change.
#[derive(Clone, Debug, PartialEq)]
pub struct Committed {
    pub revision: u64,
    pub file: String,
    /// What the edited range said before and says now.
    pub before: String,
    pub after: String,
    /// The value that changed, and what it is now (in its units).
    pub origin: Origin,
    pub value: f64,
}

#[derive(Debug, PartialEq)]
pub enum EditError {
    /// No such control in that revision.
    NotFound(String),
    /// The files changed since the edit's revision in a way that touches its target.
    /// List the controls again and retry.
    Stale {
        current: u64,
    },
    /// The value is not what the caller expected: someone else changed it.
    Conflict {
        current: f64,
    },
    /// The files on disk don't compile, so there is no base to edit.
    DraftInvalid(Vec<Diagnostic>),
    /// The edit didn't compile, or didn't change exactly what it should have.
    Rejected(String),
    /// The file changed on disk while the edit was being made.
    WriteConflict(String),
    Io(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EditError::NotFound(id) => write!(f, "no control `{id}` in that revision"),
            EditError::Stale { current } => {
                write!(f, "the code changed around this control; it is revision {current} now, list the controls again")
            }
            EditError::Conflict { current } => write!(f, "the value changed elsewhere (it is now {current})"),
            EditError::DraftInvalid(d) => write!(f, "the project doesn't compile right now ({} problems), so it can't be edited", d.len()),
            EditError::Rejected(why) => write!(f, "edit rejected: {why}"),
            EditError::WriteConflict(file) => write!(f, "{file} changed on disk while the edit was being made"),
            EditError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// One undoable change: a byte range of one file, with what it said on either side.
struct Change {
    revision: u64,
    file: PathBuf,
    start: u32,
    before: String,
    after: String,
    undo: Undo,
}

/// How reverting a change is verified.
enum Undo {
    /// A value edit: reverting sets the value back, carried past later unrelated edits.
    Value { origin: Origin, value_before: f64, value_after: f64 },
    /// A structural edit: reverting restores revision `base` exactly, so it is allowed
    /// only while the change is the latest revision.
    Exact { base: u64 },
}

/// An edit compiled from memory, not yet written or accepted.
struct Candidate {
    file: PathBuf,
    start: u32,
    /// The file as accepted, and the replaced and replacing text.
    old: String,
    before: String,
    after: String,
    new: String,
    snapshot: Snapshot,
    project: Project,
    compiled: Compiled,
}

/// What an earlier revision looked like, for carrying edits forward.
struct Past {
    number: u64,
    snapshot: Snapshot,
    sites: Vec<Site>,
}

/// How many past revisions an edit can be planned against.
const PAST: usize = 64;

pub struct Session {
    pub packages: PackageSet,
    dir: PathBuf,
    accepted: Revision,
    /// The latest snapshot read from disk.
    draft: Snapshot,
    /// Why the draft doesn't compile, if it doesn't.
    pub draft_errors: Vec<Diagnostic>,
    past: VecDeque<Past>,
    history: Vec<Change>,
    /// Undone changes, as their inverses, newest last. Any new edit clears it.
    redo: Vec<Change>,
}

/// Where bytes `start..end` of `old` are in `new`, if no change touched them.
/// Conservative: everything between the first and last difference counts as changed.
fn carry(old: &str, new: &str, start: usize, end: usize) -> Option<usize> {
    if old == new {
        return Some(start);
    }
    let p = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let s = old.as_bytes()[p..].iter().rev().zip(new.as_bytes()[p..].iter().rev()).take_while(|(a, b)| a == b).count();
    let to = if end <= p {
        start
    } else if start >= old.len() - s {
        start + new.len() - old.len()
    } else {
        return None;
    };
    (new.get(to..to + (end - start)) == old.get(start..end)).then_some(to)
}

/// The IR with source positions removed, for comparing two builds.
fn without_src(ir: &Ir) -> Ir {
    fn nodes(ns: &mut [Node]) {
        for n in ns {
            n.src = None;
            if let Op::Poly { body, .. } = &mut n.op {
                nodes(&mut body.nodes);
            }
        }
    }
    let mut ir = ir.clone();
    for m in &mut ir.modules {
        m.src = None;
        nodes(&mut m.nodes);
    }
    ir
}

/// `ir` with every constant written at `origin` set to `value`, and how many there were.
fn with_value(ir: &Ir, origin: &Origin, value: f64) -> (Ir, usize) {
    fn nodes(ns: &mut [Node], origin: &Origin, value: f64) -> usize {
        let mut n = 0;
        for node in ns {
            if let (Op::Const { value: v, .. }, Some(o)) = (&mut node.op, &node.origin)
                && o.same_value(origin)
            {
                *v = value;
                n += 1;
            }
            if let Op::Poly { body, .. } = &mut node.op {
                n += nodes(&mut body.nodes, origin, value);
            }
        }
        n
    }
    let mut ir = ir.clone();
    let n = ir.modules.iter_mut().map(|m| nodes(&mut m.nodes, origin, value)).sum();
    (ir, n)
}

fn walk(ns: &[Node], f: &mut impl FnMut(&Node)) {
    for n in ns {
        f(n);
        if let Op::Poly { body, .. } = &n.op {
            walk(&body.nodes, f);
        }
    }
}

fn site_by_id<'a>(sites: &'a [Site], id: &str) -> Option<(&'a Site, Vec<String>)> {
    let (call, arg) = id.split_once('/')?;
    let (file, ordinal) = call.rsplit_once('#')?;
    let ordinal: u32 = ordinal.parse().ok()?;
    let site = sites.iter().find(|s| s.file == file && s.ordinal == ordinal)?;
    Some((site, arg.split('.').map(str::to_string).collect()))
}

fn build(project: &Project, snapshot: &Snapshot, packages: &mut PackageSet) -> Result<Compiled, Vec<Diagnostic>> {
    compile_sources(project, packages, snapshot.sources().map(|(p, t)| (p, t.clone())).collect())
}

impl Session {
    pub fn open(dir: &Path, packages: PackageSet) -> Result<Session, Vec<Diagnostic>> {
        let mut packages = packages;
        let err = |e: anyhow::Error| vec![Diagnostic::new(format!("{e:#}"))];
        let snapshot = Snapshot::read(dir).map_err(err)?;
        let project = Project::from_snapshot(&snapshot).map_err(err)?;
        let compiled = build(&project, &snapshot, &mut packages)?;
        Ok(Session {
            packages,
            dir: snapshot.dir.clone(),
            draft: snapshot.clone(),
            accepted: Revision { number: 1, snapshot, project, compiled },
            draft_errors: vec![],
            past: VecDeque::new(),
            history: vec![],
            redo: vec![],
        })
    }

    /// The last revision that compiled. Everything shown or played comes from it.
    pub fn accepted(&self) -> &Revision {
        &self.accepted
    }

    fn accept(&mut self, snapshot: Snapshot, project: Project, compiled: Compiled) -> u64 {
        let number = self.accepted.number + 1;
        let old = std::mem::replace(&mut self.accepted, Revision { number, snapshot, project, compiled });
        self.past.push_back(Past { number: old.number, snapshot: old.snapshot, sites: old.compiled.sites });
        if self.past.len() > PAST {
            self.past.pop_front();
        }
        number
    }

    /// Pick up changes made on disk: code, timeline or manifest. They are accepted if
    /// they compile; otherwise the accepted revision stays and `draft_errors` says why.
    /// Returns whether the draft changed.
    pub fn refresh(&mut self) -> bool {
        let snapshot = match Snapshot::read(&self.dir) {
            Ok(s) => s,
            Err(e) => {
                self.draft_errors = vec![Diagnostic::new(format!("{e:#}"))];
                return true;
            }
        };
        if snapshot == self.draft {
            return false;
        }
        self.draft = snapshot.clone();
        if snapshot == self.accepted.snapshot {
            self.draft_errors.clear();
            return true;
        }
        let built = Project::from_snapshot(&snapshot)
            .map_err(|e| vec![Diagnostic::new(format!("{e:#}"))])
            .and_then(|p| build(&p, &snapshot, &mut self.packages).map(|c| (p, c)));
        match built {
            Ok((project, compiled)) => {
                self.accept(snapshot, project, compiled);
                self.draft_errors.clear();
            }
            Err(d) => self.draft_errors = d,
        }
        true
    }

    /// Every number written in the project's own files that feeds the graph.
    pub fn controls(&self) -> Vec<Control> {
        let rev = &self.accepted;
        let mut found: BTreeMap<String, Control> = BTreeMap::new();
        let mut visit = |n: &Node| {
            let (Op::Const { value, unit }, Some(o)) = (&n.op, &n.origin) else { return };
            if o.arg.is_empty() {
                return;
            }
            let Some(site) = rev.compiled.sites.iter().find(|s| s.id == o.site && s.in_project) else { return };
            let Some(text) = rev.snapshot.files.get(Path::new(&site.file)) else { return };
            let Some(lens) = LiteralLens::read(site, o, text, *value) else { return };
            let id = format!("{}#{}/{}", site.file, site.ordinal, o.arg.join("."));
            found.entry(id.clone()).and_modify(|c| c.uses += 1).or_insert_with(|| Control {
                id,
                revision: rev.number,
                file: site.file.clone(),
                line: site.line,
                value: *value,
                literal: lens.literal.clone(),
                unit: *unit,
                scale: lens.scale,
                uses: 1,
                origin: o.clone(),
                lens,
            });
        };
        for m in &rev.compiled.ir.modules {
            walk(&m.nodes, &mut visit);
        }
        found.into_values().collect()
    }

    /// The control `id` named at revision `at`, as it is in the accepted revision.
    fn resolve(&self, at: u64, id: &str) -> Result<Control, EditError> {
        let current = self.controls();
        let find = |id: &str| current.iter().find(|c| c.id == id).cloned();
        if at == self.accepted.number {
            return find(id).ok_or_else(|| EditError::NotFound(id.to_string()));
        }
        // Carry the target from revision `at`: its call must be untouched and in place.
        let stale = EditError::Stale { current: self.accepted.number };
        let carried = || -> Option<Control> {
            let past = self.past.iter().find(|p| p.number == at)?;
            let (old, arg) = site_by_id(&past.sites, id)?;
            let file = Path::new(&old.file);
            let to = carry(past.snapshot.files.get(file)?, self.accepted.snapshot.files.get(file)?, old.start as usize, old.end as usize)?
                as u32;
            let site =
                self.accepted.compiled.sites.iter().find(|s| s.file == old.file && s.start == to && s.end == to + (old.end - old.start))?;
            find(&format!("{}#{}/{}", site.file, site.ordinal, arg.join(".")))
        };
        carried().ok_or(stale)
    }

    /// Set a written number. `at` is the revision the caller's control came from.
    /// `value` is in the control's units (so `khz(2.5)` set to 900 is written as
    /// `khz(0.9)`). With `expect`, the edit applies only if the value is still that.
    pub fn set(&mut self, at: u64, id: &str, value: f64, expect: Option<f64>) -> Result<Committed, EditError> {
        self.refresh();
        if !self.draft_errors.is_empty() {
            return Err(EditError::DraftInvalid(self.draft_errors.clone()));
        }
        if !value.is_finite() {
            return Err(EditError::Rejected(format!("{value} is not a number that can be written")));
        }
        let c = self.resolve(at, id)?;
        if let Some(e) = expect
            && e != c.value
        {
            return Err(EditError::Conflict { current: c.value });
        }
        let edit = c.lens.propose(value)?;
        if edit.value == c.value {
            // Closing a preview at its starting value is a real gesture, but not
            // a source edit. Preserve spellings such as `0x320`, `8e2` and `-0`.
            return Ok(Committed {
                revision: self.accepted.number,
                file: c.file,
                before: c.literal.clone(),
                after: c.literal,
                origin: c.origin,
                value: c.value,
            });
        }
        self.apply(Path::new(&c.file), c.lens.start, c.lens.end, &edit.text, &c.origin, edit.value, Some(c.value))
    }

    /// Resolve a live edit using exactly the same literal lens as a saved edit.
    /// Preview never carries an old origin into a different accepted graph. The
    /// host watches/refreshes the draft separately; no build runs on this path.
    ///
    /// Like [`Session::set`], the control is carried forward from revision `at` when the
    /// files changed without touching it, so an unrelated save during a drag doesn't end
    /// the drag; a change that touched it makes the preview stale.
    pub fn preview(&self, at: u64, id: &str, value: f64) -> Result<Audition, EditError> {
        if !self.draft_errors.is_empty() {
            return Err(EditError::DraftInvalid(self.draft_errors.clone()));
        }
        let c = self.resolve(at, id)?;
        let edit = c.lens.propose(value)?;
        Ok(Audition { id: c.id, origin: c.origin, value: edit.value, current: c.value })
    }

    /// Replace bytes `start..end` of one file with `text`, which must set the constant
    /// at `origin` to `value`; verify, write and accept. With the value it replaces, it is
    /// recorded for undo.
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        file: &Path,
        start: u32,
        end: u32,
        text: &str,
        origin: &Origin,
        value: f64,
        value_before: Option<f64>,
    ) -> Result<Committed, EditError> {
        let name = file.display().to_string();
        let old = self.accepted.snapshot.files.get(file).ok_or_else(|| EditError::NotFound(name.clone()))?;
        if old[start as usize..end as usize] == *text {
            return Ok(Committed {
                revision: self.accepted.number,
                file: name,
                before: text.to_string(),
                after: text.to_string(),
                origin: origin.clone(),
                value,
            });
        }
        // The candidate must change exactly this value.
        let c = self.candidate(file, start, end, text)?;
        let (expected, changed) = with_value(&self.accepted.compiled.ir, origin, value);
        if changed == 0 {
            return Err(EditError::Rejected("the control is no longer in the graph".into()));
        }
        if without_src(&c.compiled.ir) != without_src(&expected) {
            return Err(EditError::Rejected("the edit would change more than this value".into()));
        }
        let (before, after) = (c.before.clone(), c.after.clone());
        let record = value_before.map(|value_before| Undo::Value { origin: origin.clone(), value_before, value_after: value });
        let number = self.commit(c, record)?;
        Ok(Committed { revision: number, file: name, before, after, origin: origin.clone(), value })
    }

    /// Build the accepted revision with bytes `start..end` of `file` replaced by `text`,
    /// from memory.
    fn candidate(&mut self, file: &Path, start: u32, end: u32, text: &str) -> Result<Candidate, EditError> {
        self.candidate_edits(file, &[(start, end, text.to_string())])
    }

    /// [`Session::candidate`] with several non-overlapping replacements in one file. The
    /// candidate records the smallest range covering them all.
    fn candidate_edits(&mut self, file: &Path, edits: &[(u32, u32, String)]) -> Result<Candidate, EditError> {
        let old = self.accepted.snapshot.files.get(file).cloned().ok_or_else(|| EditError::NotFound(file.display().to_string()))?;
        let mut edits = edits.to_vec();
        edits.sort_by_key(|(start, ..)| *start);
        let outside = || EditError::Rejected("the edit is outside the file".into());
        if edits.is_empty() || edits.windows(2).any(|w| w[0].1 > w[1].0) || old.get(..edits.last().expect("some").1 as usize).is_none() {
            return Err(outside());
        }
        let mut new = String::new();
        let mut at = 0;
        for (start, end, text) in &edits {
            new += old.get(at..*start as usize).ok_or_else(outside)?;
            new += text;
            at = *end as usize;
        }
        new += &old[at..];
        let (start, end) = (edits[0].0, edits.last().expect("some").1);
        let grown = new.len() as i64 - old.len() as i64;
        let before = old[start as usize..end as usize].to_string();
        let text = new[start as usize..(end as i64 + grown) as usize].to_string();
        let text = text.as_str();
        let mut snapshot = self.accepted.snapshot.clone();
        snapshot.files.insert(file.to_path_buf(), new.clone());
        let project = Project::from_snapshot(&snapshot).map_err(|e| EditError::Rejected(format!("{e:#}")))?;
        let compiled = build(&project, &snapshot, &mut self.packages)
            .map_err(|d| EditError::Rejected(d.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; ")))?;
        Ok(Candidate { file: file.to_path_buf(), start, old, before, after: text.to_string(), new, snapshot, project, compiled })
    }

    /// Write a verified candidate if the file on disk is still what was accepted, and
    /// accept it. With `record`, it is a new edit: recorded for undo, ending any redo.
    /// Returns the new revision.
    fn commit(&mut self, c: Candidate, record: Option<Undo>) -> Result<u64, EditError> {
        let path = self.dir.join(&c.file);
        let disk = std::fs::read_to_string(&path).map_err(|e| EditError::Io(e.to_string()))?;
        if disk != c.old {
            return Err(EditError::WriteConflict(c.file.display().to_string()));
        }
        let tmp = path.with_extension("m2tmp");
        std::fs::write(&tmp, &c.new).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| EditError::Io(e.to_string()))?;

        self.draft = c.snapshot.clone();
        let number = self.accept(c.snapshot, c.project, c.compiled);
        if let Some(undo) = record {
            self.history.push(Change { revision: number, file: c.file, start: c.start, before: c.before, after: c.after, undo });
            self.redo.clear();
        }
        Ok(number)
    }

    /// The snapshot of revision `number`, if it is the accepted one or still remembered.
    fn snapshot(&self, number: u64) -> Option<&Snapshot> {
        if number == self.accepted.number {
            return Some(&self.accepted.snapshot);
        }
        self.past.iter().find(|p| p.number == number).map(|p| &p.snapshot)
    }

    /// Undo the last edit made in this session: restore its exact original bytes, if
    /// they haven't been edited since (other changes in the file are kept).
    pub fn undo(&mut self) -> Result<Edited, EditError> {
        self.step(false)
    }

    /// Redo the last undone edit, verified the same way.
    pub fn redo(&mut self) -> Result<Edited, EditError> {
        self.step(true)
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Revert the newest change on one stack and record its inverse on the other.
    fn step(&mut self, redo: bool) -> Result<Edited, EditError> {
        self.refresh();
        if !self.draft_errors.is_empty() {
            return Err(EditError::DraftInvalid(self.draft_errors.clone()));
        }
        let what = if redo { "nothing to redo" } else { "nothing to undo" };
        let stack = if redo { &mut self.redo } else { &mut self.history };
        let Some(c) = stack.pop() else { return Err(EditError::NotFound(what.into())) };
        match self.revert(&c) {
            Ok((edited, inverse)) => {
                if redo { &mut self.history } else { &mut self.redo }.push(inverse);
                Ok(edited)
            }
            Err(e) => {
                if redo { &mut self.redo } else { &mut self.history }.push(c);
                Err(e)
            }
        }
    }

    /// Apply the inverse of `c`, verified as its kind of edit requires, without
    /// recording it. Returns what changed and the change that would revert it again.
    fn revert(&mut self, c: &Change) -> Result<(Edited, Change), EditError> {
        let file = c.file.display().to_string();
        match &c.undo {
            Undo::Value { origin, value_before, value_after } => {
                let end = c.start as usize + c.after.len();
                let start = self
                    .snapshot(c.revision)
                    .and_then(|s| s.files.get(&c.file))
                    .zip(self.accepted.snapshot.files.get(&c.file))
                    .and_then(|(was, now)| carry(was, now, c.start as usize, end))
                    .ok_or_else(|| EditError::Rejected("that value has been edited since; undo would overwrite it".into()))?;
                let end = start + c.after.len();
                let done = self.apply(&c.file, start as u32, end as u32, &c.before, origin, *value_before, None)?;
                let inverse = Change {
                    revision: done.revision,
                    file: c.file.clone(),
                    start: start as u32,
                    before: c.after.clone(),
                    after: c.before.clone(),
                    undo: Undo::Value { origin: origin.clone(), value_before: *value_after, value_after: *value_before },
                };
                Ok((Edited { revision: done.revision, file, before: done.before, after: done.after }, inverse))
            }
            Undo::Exact { base } => {
                // The code must be exactly as the change left it, and restoring the bytes
                // must give back exactly the revision it was made on, which was accepted
                // then, so it means exactly that graph.
                let changed = || EditError::Rejected("the code changed since this edit; change it in the code".into());
                if self.snapshot(c.revision) != Some(&self.accepted.snapshot) {
                    return Err(changed());
                }
                let base = self.snapshot(*base).ok_or_else(changed)?.clone();
                let end = c.start + c.after.len() as u32;
                let candidate = self.candidate(&c.file, c.start, end, &c.before)?;
                if candidate.snapshot != base {
                    return Err(changed());
                }
                let (before, after) = (candidate.before.clone(), candidate.after.clone());
                let undone = self.accepted.number;
                let revision = self.commit(candidate, None)?;
                let inverse = Change {
                    revision,
                    file: c.file.clone(),
                    start: c.start,
                    before: c.after.clone(),
                    after: c.before.clone(),
                    undo: Undo::Exact { base: undone },
                };
                Ok((Edited { revision, file, before, after }, inverse))
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod restricted_tests;

