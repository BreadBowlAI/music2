//! The editing session: the authority that accepts revisions of a project.
//!
//! It keeps a snapshot of the project's source files and the last revision that
//! compiled ("accepted"). Files on disk are the draft: [`Session::refresh`] picks up
//! edits made elsewhere, and if they don't compile the accepted revision keeps playing.
//!
//! Edits from the graph editor or an agent are structured operations, never text.
//! This first slice supports one: set a written number ([`Session::set`]). Every edit
//! is compiled as a candidate from memory and accepted only if the new graph equals
//! the old graph with exactly that change; only then is the file written, and only if
//! the file on disk still matches the snapshot. Undo is the guarded inverse.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use m2_ir::{Diagnostic, Ir, Node, Op, Origin, Unit};
use m2_runtime::PackageSet;
use m2_script::sites::{Arg, Site};
use m2_song::Project;

use crate::{Compiled, compile_sources};

/// A number written in the project's code that the graph can show as a knob.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    /// `file#call/arg`: the file, the call's position among the file's calls, and the
    /// argument path. Stable while calls before it in the file don't change.
    pub id: String,
    pub file: String,
    pub line: u32,
    /// The current value, in the constant's units (written number × scale).
    pub value: f64,
    /// The text written in the code, e.g. `0.3` or `2.5` (in `khz(2.5)`).
    pub literal: String,
    pub unit: Option<Unit>,
    /// The constant is the written number times this.
    pub scale: f64,
    /// How many graph nodes use it (a helper called twice gives two).
    pub uses: usize,
    pub origin: Origin,
    /// Byte range of the number in the file.
    start: u32,
    end: u32,
}

/// An accepted change.
#[derive(Clone, Debug, PartialEq)]
pub struct Committed {
    pub revision: u64,
    pub file: String,
    /// What the edited range said before and says now.
    pub before: String,
    pub after: String,
}

#[derive(Debug, PartialEq)]
pub enum EditError {
    /// No such control in the current revision.
    NotFound(String),
    /// The value is not what the caller expected: someone else changed it.
    Conflict { current: f64 },
    /// The files on disk don't currently compile, so there is no base to edit.
    DraftInvalid(Vec<Diagnostic>),
    /// The edit didn't compile, or didn't change exactly what it should have.
    Rejected(String),
    /// The file changed on disk between planning and writing.
    WriteConflict(String),
    Io(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EditError::NotFound(id) => write!(f, "no control `{id}` in the current revision"),
            EditError::Conflict { current } => write!(f, "the value changed elsewhere (it is now {current})"),
            EditError::DraftInvalid(d) => write!(f, "the project doesn't compile right now ({} problems), so it can't be edited", d.len()),
            EditError::Rejected(why) => write!(f, "edit rejected: {why}"),
            EditError::WriteConflict(file) => write!(f, "{file} changed on disk while the edit was being made"),
            EditError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// One undoable change: a range of one file.
struct Change {
    path: PathBuf,
    file: String,
    start: u32,
    before: String,
    after: String,
    origin: Origin,
    value_before: f64,
}

pub struct Session {
    pub project: Project,
    pub packages: PackageSet,
    /// The source snapshot: every `.ts` file in the project, by canonical path.
    files: BTreeMap<PathBuf, String>,
    /// Bumps on every accepted edit and every change picked up from disk.
    pub revision: u64,
    accepted: Compiled,
    /// Why the files on disk don't compile, if they don't.
    pub draft_errors: Vec<Diagnostic>,
    history: Vec<Change>,
}

fn read_sources(dir: &Path) -> std::io::Result<BTreeMap<PathBuf, String>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "assets" {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "ts") {
                out.insert(p.canonicalize()?, std::fs::read_to_string(&p)?);
            }
        }
    }
    Ok(out)
}

/// The shortest decimal that reads back as `v` (valid JavaScript).
fn format_number(v: f64) -> String {
    let s = format!("{v}");
    if s == "-0" { "0".into() } else { s }
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

/// `ir` with every constant written at `origin` set to `value`.
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

impl Session {
    pub fn open(dir: &Path, packages: PackageSet) -> Result<Session, Vec<Diagnostic>> {
        let mut packages = packages;
        let project = Project::open(dir).map_err(|e| vec![Diagnostic::new(format!("{e:#}"))])?;
        let files = read_sources(&project.dir).map_err(|e| vec![Diagnostic::new(e.to_string())])?;
        let accepted = compile_sources(&project, &mut packages, files.clone().into_iter().collect())?;
        Ok(Session { project, packages, files, revision: 1, accepted, draft_errors: vec![], history: vec![] })
    }

    /// The last revision that compiled.
    pub fn accepted(&self) -> &Compiled {
        &self.accepted
    }

    /// Pick up changes made on disk. Returns whether anything changed. If the new
    /// files don't compile, the accepted revision stays and `draft_errors` says why.
    pub fn refresh(&mut self) -> bool {
        let Ok(files) = read_sources(&self.project.dir) else { return false };
        let project = Project::open(&self.project.dir);
        let timeline_changed = project.as_ref().is_ok_and(|p| p.timeline.end() != self.project.timeline.end());
        if files == self.files && !timeline_changed {
            return false;
        }
        self.files = files;
        self.revision += 1;
        match project {
            Ok(p) => self.project = p,
            Err(e) => {
                self.draft_errors = vec![Diagnostic::new(format!("{e:#}"))];
                return true;
            }
        }
        match compile_sources(&self.project, &mut self.packages, self.files.clone().into_iter().collect()) {
            Ok(c) => {
                self.accepted = c;
                self.draft_errors.clear();
            }
            Err(d) => self.draft_errors = d,
        }
        true
    }

    fn site(&self, id: u32) -> Option<&Site> {
        self.accepted.sites.iter().find(|s| s.id == id)
    }

    fn path_of(&self, file: &str) -> Option<PathBuf> {
        self.project.dir.join(file).canonicalize().ok().filter(|p| self.files.contains_key(p))
    }

    /// Every number written in the project's own files that feeds the graph.
    pub fn controls(&self) -> Vec<Control> {
        let mut found: BTreeMap<String, Control> = BTreeMap::new();
        let mut visit = |n: &Node| {
            let (Op::Const { value, unit }, Some(o)) = (&n.op, &n.origin) else { return };
            if o.arg.is_empty() {
                return;
            }
            let Some(site) = self.site(o.site).filter(|s| s.in_project) else { return };
            let Some(Arg::Number { start, end, value: written }) = site.arg(&o.arg) else { return };
            let scale = o.scale.unwrap_or(1.0);
            // The written number must be what made the value (not a computed one).
            if written * scale != *value {
                return;
            }
            let Some(text) = self.path_of(&site.file).and_then(|p| self.files.get(&p)) else { return };
            let id = format!("{}#{}/{}", site.file, site.ordinal, o.arg.join("."));
            found
                .entry(id.clone())
                .and_modify(|c| c.uses += 1)
                .or_insert_with(|| Control {
                    id,
                    file: site.file.clone(),
                    line: site.line,
                    value: *value,
                    literal: text[*start as usize..*end as usize].to_string(),
                    unit: *unit,
                    scale,
                    uses: 1,
                    origin: o.clone(),
                    start: *start,
                    end: *end,
                });
        };
        fn walk(ns: &[Node], f: &mut impl FnMut(&Node)) {
            for n in ns {
                f(n);
                if let Op::Poly { body, .. } = &n.op {
                    walk(&body.nodes, f);
                }
            }
        }
        for m in &self.accepted.ir.modules {
            walk(&m.nodes, &mut visit);
        }
        found.into_values().collect()
    }

    /// Set a written number. `value` is in the control's units (so `khz(2.5)` set to
    /// 900 is written as `khz(0.9)`). With `expect`, the edit only applies if the
    /// value is still what the caller last saw.
    pub fn set(&mut self, id: &str, value: f64, expect: Option<f64>) -> Result<Committed, EditError> {
        self.refresh();
        if !self.draft_errors.is_empty() {
            return Err(EditError::DraftInvalid(self.draft_errors.clone()));
        }
        if !value.is_finite() {
            return Err(EditError::Rejected(format!("{value} is not a number that can be written")));
        }
        let c = self.controls().into_iter().find(|c| c.id == id).ok_or_else(|| EditError::NotFound(id.to_string()))?;
        if let Some(e) = expect
            && e != c.value
        {
            return Err(EditError::Conflict { current: c.value });
        }
        let text = format_number(value / c.scale);
        let path = self.path_of(&c.file).ok_or_else(|| EditError::NotFound(id.to_string()))?;
        self.apply(&path, &c.file, c.start, c.end, &text, &c.origin, c.value)
    }

    /// Replace bytes `start..end` of one file, verify, write, and record for undo.
    #[allow(clippy::too_many_arguments)]
    fn apply(&mut self, path: &Path, file: &str, start: u32, end: u32, text: &str, origin: &Origin, value_before: f64) -> Result<Committed, EditError> {
        let old = self.files[path].clone();
        let before = old[start as usize..end as usize].to_string();
        if before == text {
            return Ok(Committed { revision: self.revision, file: file.to_string(), before, after: text.to_string() });
        }
        let new = format!("{}{text}{}", &old[..start as usize], &old[end as usize..]);

        // Build the candidate from memory, then check it changed exactly one value.
        let mut sources: HashMap<PathBuf, String> = self.files.clone().into_iter().collect();
        sources.insert(path.to_path_buf(), new.clone());
        let candidate = compile_sources(&self.project, &mut self.packages, sources).map_err(|d| {
            EditError::Rejected(d.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; "))
        })?;
        let written: f64 = text.parse().map_err(|_| EditError::Rejected(format!("`{text}` is not a number")))?;
        let (expected, changed) = with_value(&self.accepted.ir, origin, written * origin.scale.unwrap_or(1.0));
        if changed == 0 {
            return Err(EditError::Rejected("the control is no longer in the graph".into()));
        }
        if without_src(&candidate.ir) != without_src(&expected) {
            return Err(EditError::Rejected("the edit would change more than this value".into()));
        }

        // Write only if the file on disk is still the snapshot.
        let disk = std::fs::read_to_string(path).map_err(|e| EditError::Io(e.to_string()))?;
        if disk != old {
            return Err(EditError::WriteConflict(file.to_string()));
        }
        let tmp = path.with_extension("ts.m2tmp");
        std::fs::write(&tmp, &new).and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| EditError::Io(e.to_string()))?;

        self.files.insert(path.to_path_buf(), new);
        self.accepted = candidate;
        self.revision += 1;
        self.history.push(Change {
            path: path.to_path_buf(),
            file: file.to_string(),
            start,
            before: before.clone(),
            after: text.to_string(),
            origin: origin.clone(),
            value_before,
        });
        Ok(Committed { revision: self.revision, file: file.to_string(), before, after: text.to_string() })
    }

    /// Undo the last accepted edit, if its text is still there.
    pub fn undo(&mut self) -> Result<Committed, EditError> {
        self.refresh();
        let Some(c) = self.history.pop() else { return Err(EditError::NotFound("nothing to undo".into())) };
        let current = self.files.get(&c.path).cloned().unwrap_or_default();
        let end = c.start as usize + c.after.len();
        if current.get(c.start as usize..end) != Some(c.after.as_str()) {
            self.history.push(c);
            return Err(EditError::Rejected("the edited text has changed since; undo would overwrite it".into()));
        }
        let result = self.apply(&c.path, &c.file, c.start, end as u32, &c.before, &c.origin, c.value_before);
        // Undoing is not itself undoable here; drop the record `apply` pushed.
        if result.is_ok() {
            self.history.pop();
        } else {
            self.history.push(c);
        }
        result
    }
}

#[cfg(test)]
mod tests;
