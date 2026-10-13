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

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use m2_ir::{Diagnostic, Ir, Node, Op, Origin, Unit};
use m2_runtime::PackageSet;
use m2_script::sites::Site;
use m2_song::{Project, Snapshot};

use crate::{Compiled, Seen, compile_sources};

mod cable;
mod data;
mod effect;
mod lens;
mod relink;
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

impl Control {
    /// The value `value` plays as, exactly as a commit would write it (the literal's spelling,
    /// rounded through its units): what a gesture's update sends the engine (WRITE-22). Pure.
    pub fn audition(&self, value: f64) -> Result<f64, EditError> {
        Ok(self.lens.propose(value)?.value)
    }
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

/// One undoable change to one file (a move also knows where the file was). For a value edit,
/// `before` and `after` are what the byte range at `start` said on either side; an exact
/// change restores the whole file, so for it they only describe the change.
///
/// A change carries the file as it left it, not a revision number: it stays undoable however
/// many revisions come after, and other files changing (a lane, a save in another file) never
/// stand in its way.
struct Change {
    file: PathBuf,
    /// The file as the change left it (None: removed).
    left: Option<String>,
    start: u32,
    before: String,
    after: String,
    undo: Undo,
}

/// How reverting a change is verified.
enum Undo {
    /// A value edit: reverting sets the value back, carried past later unrelated edits. The
    /// value is found by where its bytes are, never by an origin: call numbers run across the
    /// whole project, so a call added in any file loaded earlier renumbers it.
    Value { scale: f64, value_before: f64, value_after: f64 },
    /// A whole-file edit: reverting writes `restore` back (None: removes the file the edit
    /// created). It is allowed only while the file is exactly as the change left it, and the
    /// project must compile with it.
    Exact { restore: Option<String> },
    /// Data files or folders moved, removed or created as they were (see `data`): reverting
    /// makes `steps`, only while every path the edit touched holds what it `left` there and the
    /// names `steps` make are free. `back` is where the change's file is once reverted.
    Data { steps: Vec<data::Step>, left: data::Left, back: PathBuf },
}

/// An edit compiled from memory, not yet written or accepted. It is built on the accepted
/// revision and committed before any other revision is accepted. A whole-file edit can
/// create the file or remove it: whether it exists is whether `snapshot` has it.
struct Candidate {
    file: PathBuf,
    start: u32,
    /// The file as accepted (empty if there is none), and the replaced and replacing text.
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

/// How many changes undo goes back through; the oldest are forgotten first.
const HISTORY: usize = 500;

/// Write `text` to `path` whole or not at all, for the file watcher: into a new file beside
/// it, renamed over it. The temporary file is always created new, never opened by a name
/// that is already there, so a link planted beside the file can't send the write elsewhere.
pub fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    for n in 0.. {
        let tmp = dir.join(format!(".{name}.{}-{n}.m2tmp", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(mut f) => {
                let written = f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).and_then(|_| std::fs::rename(&tmp, path));
                if written.is_err() {
                    let _ = std::fs::remove_file(&tmp);
                }
                return written;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 64 => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("the loop returns")
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
    /// What every revision accepted so far declared, so data a rename left behind is named as
    /// its track was and offered only to lanes that read it.
    seen: Seen,
}

/// Why `name` isn't created: `by`, which has its name on a disk that ignores capitals (see
/// [`data::taken`]), is there already.
fn there_already(name: &Path, by: &Path) -> String {
    if by == name || by.starts_with(name) {
        format!("{} is there already, and isn't replaced", by.display())
    } else if name.starts_with(by) || by.components().count() < name.components().count() {
        format!(
            "{} is there already, and on a disk that ignores capitals {} would be written into it, where music2 doesn't read it. \
             Rename it as its key is spelled, or remove it",
            by.display(),
            name.display()
        )
    } else {
        format!("{} is there already, and on a disk that ignores capitals it is {}; it isn't replaced", by.display(), name.display())
    }
}

/// Where bytes `start..end` of `old` are in `new`, if no change touched them (WRITE-12): they lie
/// in the text the two share before the first difference or after the last one, or on lines a
/// line diff keeps (edits above and below them, as when an agent adds an import and a line at
/// the end). Conservative either way: a change on one of the range's own lines touches it.
pub(crate) fn carry(old: &str, new: &str, start: usize, end: usize) -> Option<usize> {
    if old == new {
        return Some(start);
    }
    carry_ends(old, new, start, end).or_else(|| carry_lines(old, new, start, end))
}

/// [`carry`] by the common prefix and suffix alone: everything between the first and last
/// difference counts as changed.
fn carry_ends(old: &str, new: &str, start: usize, end: usize) -> Option<usize> {
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

/// Lines a line diff may find different (about 2 MB of work at worst); a save that changes more
/// carries nothing by lines.
const DIFF_LINES: usize = 500;

/// [`carry`] by a line diff: the lines `start..end` spans must all be kept, in one run.
fn carry_lines(old: &str, new: &str, start: usize, end: usize) -> Option<usize> {
    if start >= old.len() || end < start || end > old.len() {
        return None;
    }
    // Each line with its newline, and where it starts.
    fn lines(s: &str) -> Vec<(usize, &str)> {
        let mut at = 0;
        s.split_inclusive('\n')
            .map(|l| {
                at += l.len();
                (at - l.len(), l)
            })
            .collect()
    }
    let (a, b) = (lines(old), lines(new));
    // The line holding `start`, and the one holding the range's last byte.
    let line_of = |byte: usize| a.partition_point(|(s, _)| *s <= byte) - 1;
    let (first, last) = (line_of(start), line_of(end.max(start + 1) - 1));
    let (at, bt): (Vec<&str>, Vec<&str>) = (a.iter().map(|l| l.1).collect(), b.iter().map(|l| l.1).collect());
    let kept = kept_lines(&at, &bt)?;
    let to_first = kept[first]?;
    if (first..=last).any(|l| kept[l] != Some(to_first + (l - first))) {
        return None;
    }
    let to = b[to_first].0 + (start - a[first].0);
    (new.get(to..to + (end - start)) == old.get(start..end)).then_some(to)
}

/// For each line of `a`, the line of `b` a shortest edit script keeps it as, or None when it is
/// deleted (Myers' greedy algorithm, O((N+M)·D)). None when more than [`DIFF_LINES`] lines differ.
fn kept_lines<T: PartialEq>(a: &[T], b: &[T]) -> Option<Vec<Option<usize>>> {
    let mut kept = vec![None; a.len()];
    // Lines both start and end with are kept as they are; the diff runs between them.
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    for (i, k) in kept.iter_mut().enumerate().take(head) {
        *k = Some(i);
    }
    for i in 0..tail {
        kept[a.len() - 1 - i] = Some(b.len() - 1 - i);
    }
    let (xs, ys) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let (n, m) = (xs.len() as isize, ys.len() as isize);
    let max = ((n + m) as usize).min(DIFF_LINES) as isize;
    // v[k + max] is the furthest x reached on diagonal k; trace[d] is v after round d.
    let at = |k: isize| (k + max) as usize;
    let mut v = vec![0isize; 2 * max as usize + 2];
    let mut trace: Vec<Vec<isize>> = vec![];
    let mut found = None;
    'rounds: for d in 0..=max {
        for k in (-d..=d).step_by(2) {
            let mut x = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) { v[at(k + 1)] } else { v[at(k - 1)] + 1 };
            let mut y = x - k;
            while x < n && y < m && xs[x as usize] == ys[y as usize] {
                (x, y) = (x + 1, y + 1);
            }
            v[at(k)] = x;
            if x >= n && y >= m {
                found = Some(d);
                trace.push(v.clone());
                break 'rounds;
            }
        }
        trace.push(v.clone());
    }
    // Back from the end: each round's edit, then the lines its snake kept.
    let (mut d, mut x, mut y) = (found?, n, m);
    while d > 0 {
        let prev = &trace[d as usize - 1];
        let k = x - y;
        let down = k == -d || (k != d && prev[at(k - 1)] < prev[at(k + 1)]);
        let pk = if down { k + 1 } else { k - 1 };
        let (px, py) = (prev[at(pk)], prev[at(pk)] - pk);
        let (sx, sy) = if down { (px, py + 1) } else { (px + 1, py) };
        while x > sx && y > sy {
            (x, y) = (x - 1, y - 1);
            kept[head + x as usize] = Some(head + y as usize);
        }
        (x, y, d) = (px, py, d - 1);
    }
    while x > 0 && y > 0 {
        (x, y) = (x - 1, y - 1);
        kept[head + x as usize] = Some(head + y as usize);
    }
    Some(kept)
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
    // `tracks/bass.ts#8/1`: the file may hold `/`, the argument path never does.
    let (call, arg) = id.rsplit_once('/')?;
    let (file, ordinal) = call.rsplit_once('#')?;
    let ordinal: u32 = ordinal.parse().ok()?;
    let site = sites.iter().find(|s| s.file == file && s.ordinal == ordinal)?;
    Some((site, arg.split('.').map(str::to_string).collect()))
}

/// Build a project read from a snapshot, given the data last accepted, which a lane whose file
/// fails keeps playing (FMT-9).
fn build(
    project: &Project,
    packages: &mut PackageSet,
    seen: &Seen,
    previous: Option<&m2_song::DataSet>,
) -> Result<Compiled, Vec<Diagnostic>> {
    compile_sources(project, packages, seen, previous)
}

/// A project that can't be read from its snapshot: the manifest's or the arrangement's errors.
fn unread(d: Vec<m2_song::Diagnostic>) -> Vec<Diagnostic> {
    d.into_iter().map(crate::diagnostic).collect()
}

/// The folder couldn't be read at all.
fn unreadable(e: anyhow::Error) -> Vec<Diagnostic> {
    vec![Diagnostic::error("manifest.invalid", format!("{e:#}"))]
}

/// The control constant `origin` with `value` shows in `rev`, if it is a number written in
/// the project's own code.
fn control(rev: &Revision, origin: &Origin, value: f64, unit: Option<Unit>) -> Option<Control> {
    if origin.arg.is_empty() {
        return None;
    }
    let site = rev.compiled.sites.iter().find(|s| s.id == origin.site && s.in_project)?;
    let text = rev.snapshot.files.get(Path::new(&site.file))?;
    let lens = LiteralLens::read(site, origin, text, value)?;
    Some(Control {
        id: format!("{}#{}/{}", site.file, site.ordinal, origin.arg.join(".")),
        revision: rev.number,
        file: site.file.clone(),
        line: site.line,
        value,
        literal: lens.literal.clone(),
        unit,
        scale: lens.scale,
        uses: 1,
        origin: origin.clone(),
        lens,
    })
}

impl Session {
    pub fn open(dir: &Path, packages: PackageSet) -> Result<Session, Vec<Diagnostic>> {
        let mut packages = packages;
        let snapshot = Snapshot::read(dir).map_err(unreadable)?;
        let project = Project::from_snapshot(&snapshot).map_err(unread)?;
        let compiled = build(&project, &mut packages, &Seen::default(), None)?;
        let mut seen = Seen::default();
        seen.add(&compiled.ir);
        Ok(Session {
            packages,
            dir: snapshot.dir.clone(),
            draft: snapshot.clone(),
            seen,
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

    /// What needs attention in the accepted revision though it plays, such as data no lane
    /// reads, with the fixes each offers (DIAG-1, DIAG-4).
    pub fn problems(&self) -> &[m2_song::Diagnostic] {
        &self.accepted.compiled.problems
    }

    fn accept(&mut self, snapshot: Snapshot, project: Project, compiled: Compiled) -> u64 {
        self.seen.add(&compiled.ir);
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
                self.draft_errors = unreadable(e);
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
        let previous = Some(&self.accepted.compiled.data);
        let built = Project::from_snapshot(&snapshot)
            .map_err(unread)
            .and_then(|p| build(&p, &mut self.packages, &self.seen, previous).map(|c| (p, c)));
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
            if let Some(c) = control(rev, o, *value, *unit) {
                found.entry(c.id.clone()).and_modify(|k| k.uses += 1).or_insert(c);
            }
        };
        for m in &rev.compiled.ir.modules {
            walk(&m.nodes, &mut visit);
        }
        found.into_values().collect()
    }

    /// The control written at bytes `start..end` of `file` in the accepted revision, if there
    /// is one: found by the calls around those bytes, whatever they are numbered now.
    fn control_at(&self, file: &Path, start: u32, end: u32) -> Option<Control> {
        let rev = &self.accepted;
        let around = |s: &&Site| s.in_project && Path::new(&s.file) == file && s.start <= start && end <= s.end;
        let calls: Vec<u32> = rev.compiled.sites.iter().filter(around).map(|s| s.id).collect();
        let mut found = None;
        let mut visit = |n: &Node| {
            if let (None, Op::Const { value, unit }, Some(o)) = (&found, &n.op, &n.origin)
                && calls.contains(&o.site)
            {
                found = control(rev, o, *value, *unit).filter(|c| c.lens.start == start && c.lens.end == end);
            }
        };
        for m in &rev.compiled.ir.modules {
            walk(&m.nodes, &mut visit);
        }
        found
    }

    /// [`Session::controls`] by id.
    pub fn controls_by_id(&self) -> HashMap<String, Control> {
        self.controls().into_iter().map(|c| (c.id.clone(), c)).collect()
    }

    /// The control `id` named at revision `at`, as it is in the accepted revision: carried
    /// forward when the files changed without touching its call (WRITE-12, WRITE-22). `current`
    /// is the accepted revision's [`Session::controls_by_id`], so many can be carried for one walk
    /// of the graph. Never builds anything.
    pub fn carry(&self, at: u64, id: &str, current: &HashMap<String, Control>) -> Result<Control, EditError> {
        self.resolve_with(at, id, |id| current.get(id).cloned())
    }

    /// The control `id` named at revision `at`, as it is in the accepted revision.
    fn resolve(&self, at: u64, id: &str) -> Result<Control, EditError> {
        let current = self.controls();
        self.resolve_with(at, id, |id| current.iter().find(|c| c.id == id).cloned())
    }

    fn resolve_with(&self, at: u64, id: &str, find: impl Fn(&str) -> Option<Control>) -> Result<Control, EditError> {
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
        let scale = origin.scale.unwrap_or(1.0);
        let record = value_before.map(|value_before| Undo::Value { scale, value_before, value_after: value });
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
        let (project, compiled) = self.compile_snapshot(&snapshot)?;
        Ok(Candidate { file: file.to_path_buf(), start, old, before, after: text.to_string(), new, snapshot, project, compiled })
    }

    /// Build the accepted revision with `file` replaced as a whole by `text`, or removed
    /// with `None`, from memory.
    fn candidate_file(&mut self, file: &Path, text: Option<&str>) -> Result<Candidate, EditError> {
        let old = self.accepted.snapshot.files.get(file).cloned().unwrap_or_default();
        let mut snapshot = self.accepted.snapshot.clone();
        match text {
            Some(text) => snapshot.files.insert(file.to_path_buf(), text.to_string()),
            None => snapshot.files.remove(file),
        };
        let (project, compiled) = self.compile_snapshot(&snapshot)?;
        let new = text.unwrap_or_default().to_string();
        Ok(Candidate { file: file.to_path_buf(), start: 0, before: old.clone(), after: new.clone(), old, new, snapshot, project, compiled })
    }

    fn compile_snapshot(&mut self, snapshot: &Snapshot) -> Result<(Project, Compiled), EditError> {
        let rejected = |d: Vec<Diagnostic>| EditError::Rejected(d.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; "));
        let project = Project::from_snapshot(snapshot).map_err(|d| rejected(unread(d)))?;
        let compiled = build(&project, &mut self.packages, &self.seen, Some(&self.accepted.compiled.data)).map_err(rejected)?;
        Ok((project, compiled))
    }

    /// Replace `file` as a whole with `text`, or remove it with `None`, if `check` accepts
    /// the candidate (given the accepted revision, then the candidate's project and
    /// build). Writes it and records it for exact undo, described as `before` → `after`.
    pub(crate) fn write_file(
        &mut self,
        file: &Path,
        text: Option<&str>,
        (before, after): (&str, &str),
        check: impl FnOnce(&Revision, &Project, &Compiled) -> Result<(), EditError>,
    ) -> Result<Edited, EditError> {
        let mut c = self.candidate_file(file, text)?;
        check(&self.accepted, &c.project, &c.compiled)?;
        (c.before, c.after) = (before.to_string(), after.to_string());
        let undo = self.exact(file);
        let revision = self.commit(c, Some(undo))?;
        Ok(Edited { revision, file: file.display().to_string(), before: before.to_string(), after: after.to_string() })
    }

    /// How a whole-file edit of `file`, made now, is undone: by writing back what it says.
    fn exact(&self, file: &Path) -> Undo {
        Undo::Exact { restore: self.accepted.snapshot.files.get(file).cloned() }
    }

    /// Take in `file` just saved by an editor in the app, which said `before` in the accepted
    /// revision. If the save is accepted as written (it compiles), it is an edit made here
    /// like any other: undo puts `before` back. Returns whether the draft changed.
    pub fn saved(&mut self, file: &Path, before: Option<String>) -> bool {
        let was = self.accepted.number;
        let changed = self.refresh();
        let now = self.accepted.snapshot.files.get(file);
        let link = std::fs::symlink_metadata(self.dir.join(file)).is_ok_and(|m| m.file_type().is_symlink());
        // Undo writes the file in place, which would replace a link: a save through one isn't recorded.
        if self.accepted.number != was && now != before.as_ref() && !link {
            let (b, a) = summarize(before.as_deref().unwrap_or(""), now.map_or("", String::as_str));
            let undo = Undo::Exact { restore: before };
            self.record(Change { file: file.to_path_buf(), left: now.cloned(), start: 0, before: b, after: a, undo });
        }
        changed
    }

    /// A new edit: remembered for undo, ending any redo.
    fn record(&mut self, change: Change) {
        self.history.push(change);
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        self.redo.clear();
    }

    /// Write a verified candidate if the file on disk is still what was accepted, and
    /// accept it. With `record`, it is a new edit: recorded for undo, ending any redo.
    /// Returns the new revision.
    fn commit(&mut self, c: Candidate, record: Option<Undo>) -> Result<u64, EditError> {
        let path = self.dir.join(&c.file);
        let io = |e: std::io::Error| EditError::Io(e.to_string());
        // A new file would land on one whose name differs only in capitals, or in a folder that
        // does, and read as it or not at all.
        if !self.accepted.snapshot.files.contains_key(&c.file)
            && c.snapshot.files.contains_key(&c.file)
            && let Some(by) = data::taken(&self.accepted.snapshot, &c.file)
        {
            return Err(EditError::Rejected(there_already(&c.file, &by)));
        }
        let disk = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io(e)),
        };
        if disk.as_ref() != self.accepted.snapshot.files.get(&c.file) {
            return Err(EditError::WriteConflict(c.file.display().to_string()));
        }
        if c.snapshot.files.contains_key(&c.file) {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(io)?;
            }
            write_whole(&path, &c.new).map_err(io)?;
        } else {
            std::fs::remove_file(&path).map_err(io)?;
        }

        self.draft = c.snapshot.clone();
        let left = c.snapshot.files.get(&c.file).cloned();
        let number = self.accept(c.snapshot, c.project, c.compiled);
        if let Some(undo) = record {
            self.record(Change { file: c.file, left, start: c.start, before: c.before, after: c.after, undo });
        }
        Ok(number)
    }

    /// The snapshot of revision `number`, if it is the accepted one or still remembered.
    pub(crate) fn snapshot(&self, number: u64) -> Option<&Snapshot> {
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

    /// Whether undo would go ahead: there is a change, the files compile, and the change's
    /// file is still as it left it. A change something else edited over stays, and can be
    /// undone again once that edit is taken back.
    pub fn can_undo(&self) -> bool {
        self.can_step(&self.history)
    }

    pub fn can_redo(&self) -> bool {
        self.can_step(&self.redo)
    }

    fn can_step(&self, stack: &[Change]) -> bool {
        self.draft_errors.is_empty() && stack.last().is_some_and(|c| self.revertible(c))
    }

    /// Whether `c` can be reverted now, so undo never offers what would be refused for
    /// certain: its file is exactly as it left it, or for a value, the value's own bytes are
    /// untouched and still a control of the same units. A name a revert creates (a move's old
    /// name, or a removed file's) must be free in any capitals.
    fn revertible(&self, c: &Change) -> bool {
        let files = &self.accepted.snapshot.files;
        match &c.undo {
            Undo::Value { scale, .. } => self.carried(c).is_some_and(|(_, k)| k.is_some_and(|k| k.scale == *scale)),
            Undo::Exact { restore } => {
                let free = || data::taken(&self.accepted.snapshot, &c.file).is_none();
                files.get(&c.file) == c.left.as_ref() && (c.left.is_some() || restore.is_none() || free())
            }
            Undo::Data { steps, left, .. } => self.unchanged(steps, left).is_ok(),
        }
    }

    /// Where a value change's bytes are in the accepted revision, if no change touched them,
    /// and the control written there, if they still are one.
    fn carried(&self, c: &Change) -> Option<(u32, Option<Control>)> {
        let (was, now) = (c.left.as_ref()?, self.accepted.snapshot.files.get(&c.file)?);
        let start = carry(was, now, c.start as usize, c.start as usize + c.after.len())? as u32;
        Some((start, self.control_at(&c.file, start, start + c.after.len() as u32)))
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
        let now = self.accepted.snapshot.files.get(&c.file).cloned();
        match &c.undo {
            Undo::Value { scale, value_before, value_after } => {
                let (start, control) =
                    self.carried(c).ok_or_else(|| EditError::Rejected("that value has been edited since; undo would overwrite it".into()))?;
                // The control written there now, with its current origin and argument path.
                let control = control.filter(|k| k.scale == *scale).ok_or_else(|| {
                    EditError::Rejected("that number no longer sets this value in the graph; change it in the code".into())
                })?;
                let end = start + c.after.len() as u32;
                let done = self.apply(&c.file, start, end, &c.before, &control.origin, *value_before, None)?;
                let inverse = Change {
                    file: c.file.clone(),
                    left: self.accepted.snapshot.files.get(&c.file).cloned(),
                    start,
                    before: c.after.clone(),
                    after: c.before.clone(),
                    undo: Undo::Value { scale: *scale, value_before: *value_after, value_after: *value_before },
                };
                Ok((Edited { revision: done.revision, file, before: done.before, after: done.after }, inverse))
            }
            Undo::Exact { restore } => {
                // The file must be exactly as the change left it; what it said before (or
                // its absence) goes back, and the project must compile with it.
                if now != c.left {
                    return Err(EditError::Rejected(format!("{file} changed since this edit; change it in the code")));
                }
                let candidate = self.candidate_file(&c.file, restore.as_deref())?;
                let revision = self.commit(candidate, None)?;
                let inverse = Change {
                    file: c.file.clone(),
                    left: restore.clone(),
                    start: c.start,
                    before: c.after.clone(),
                    after: c.before.clone(),
                    undo: Undo::Exact { restore: now },
                };
                Ok((Edited { revision, file, before: c.after.clone(), after: c.before.clone() }, inverse))
            }
            Undo::Data { steps, left, back } => {
                self.unchanged(steps, left)?;
                let (revision, steps, left) = self.make(steps, |_, _| Ok(()))?;
                let undo = Undo::Data { steps, left, back: c.file.clone() };
                let inverse = Change { file: back.clone(), left: None, start: 0, before: c.after.clone(), after: c.before.clone(), undo };
                Ok((Edited { revision, file, before: c.after.clone(), after: c.before.clone() }, inverse))
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod restricted_tests;
