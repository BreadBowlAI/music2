//! Edits that move, remove or restore data files and `data/<module>/` folders as they are,
//! byte for byte: the fixes for data no lane or plugin reads (DIAG-6). Each is built on the
//! accepted snapshot in memory and compiled as a candidate first; since only data moves, the
//! graph must come out the same. The disk must still be as accepted, every name a step creates
//! must be free in any capitals, and the edit is one undo step whose inverse is checked the same
//! way. Step 6 makes these Tx.

use std::io::Write;
use std::path::{Path, PathBuf};

use m2_song::{DataPath, Project, Sha, Snapshot, Stored};

use super::{Change, EditError, Edited, Session, Undo};
use crate::Compiled;

/// One step of a data edit.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    /// Rename a file, or a `data/<module>/` folder with everything in it.
    Rename { from: PathBuf, to: PathBuf },
    /// Remove a file.
    Remove { path: PathBuf },
    /// Create a file that isn't there.
    Create { path: PathBuf, bytes: Vec<u8> },
}

/// What a snapshot knows of a file.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Content {
    Text(String),
    Binary(Stored),
}

/// Every file of `snap` at `p` or inside it, with what it holds, in path order.
pub(crate) fn under(snap: &Snapshot, p: &Path) -> Vec<(PathBuf, Content)> {
    let text = snap.files.iter().filter(|(k, _)| k.starts_with(p)).map(|(k, t)| (k.clone(), Content::Text(t.clone())));
    let bin = snap.binaries.iter().filter(|(k, _)| k.starts_with(p)).map(|(k, b)| (k.clone(), Content::Binary(b.clone())));
    let mut all: Vec<(PathBuf, Content)> = text.chain(bin).collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

/// A file or folder of `snap` that has the name `p` on a disk that ignores capitals: `p` itself,
/// `p` in other capitals, a file inside such a folder, or a folder on the way to `p` spelled in
/// other capitals (`data/Bass` for `data/bass/notes.json`), which such a disk would write into.
/// Every path the snapshot lists counts, files music2 doesn't read and ones it can't included.
/// Judged by the names alone, so a project acts the same on every disk it is checked out on.
pub(crate) fn taken(snap: &Snapshot, p: &Path) -> Option<PathBuf> {
    let want: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let paths = snap.files.keys().chain(snap.binaries.keys()).chain(&snap.unknown).chain(snap.unreadable.keys());
    paths.into_iter().find_map(|k| {
        let mut at = PathBuf::new();
        for (i, c) in k.components().enumerate() {
            let c = c.as_os_str().to_string_lossy();
            match want.get(i) {
                // `k` is `p`, or inside it, in any capitals.
                None => return Some(k.clone()),
                Some(w) if *w == c => {}
                Some(w) if w.eq_ignore_ascii_case(&c) => return Some(at.join(c.as_ref())),
                Some(_) => return None,
            }
            at.push(c.as_ref());
        }
        // `k` is `p` exactly, or a folder on the way to it in the same capitals.
        (k.components().count() == want.len()).then(|| k.clone())
    })
}

/// Why `p`, something in it, or a folder it is in can't be read, if that is so (FMT-9): what
/// can't be read can't be checked, so it is never moved or removed.
pub(crate) fn unread(snap: &Snapshot, p: &Path) -> Option<String> {
    let (at, why) = snap.unreadable.iter().find(|(u, _)| u.starts_with(p) || p.starts_with(u))?;
    Some(format!("{} can't be read ({why}); fix that first, or move it by hand", at.display()))
}

/// `snap` with `step` made.
fn apply(snap: &mut Snapshot, step: &Step) {
    match step {
        Step::Rename { from, to } => {
            let moved =
                |k: &Path| k.strip_prefix(from).ok().map(|rest| if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) });
            let files = std::mem::take(&mut snap.files);
            snap.files = files.into_iter().map(|(k, v)| (moved(&k).unwrap_or(k), v)).collect();
            let bins = std::mem::take(&mut snap.binaries);
            snap.binaries = bins.into_iter().map(|(k, v)| (moved(&k).unwrap_or(k), v)).collect();
            let unknown = std::mem::take(&mut snap.unknown);
            snap.unknown = unknown.into_iter().map(|k| moved(&k).unwrap_or(k)).collect();
            let unreadable = std::mem::take(&mut snap.unreadable);
            snap.unreadable = unreadable.into_iter().map(|(k, v)| (moved(&k).unwrap_or(k), v)).collect();
        }
        Step::Remove { path } => {
            snap.files.remove(path);
            snap.binaries.remove(path);
        }
        Step::Create { path, bytes } => match DataPath::parse(path) {
            DataPath::PluginState { sidecar: false, .. } | DataPath::Asset(_) => {
                snap.binaries.insert(path.clone(), Stored::Hash(Sha::of(bytes)));
            }
            _ => {
                snap.files.insert(path.clone(), String::from_utf8_lossy(bytes).into_owned());
            }
        },
    }
}

/// Whether the disk holds at `path` what `c` says.
fn on_disk(path: &Path, c: &Content) -> Result<bool, EditError> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(EditError::Io(e.to_string())),
    };
    Ok(match c {
        Content::Text(t) => bytes == t.as_bytes(),
        Content::Binary(Stored::Hash(sha)) => Sha::of(&bytes) == *sha,
        Content::Binary(_) => true,
    })
}

/// Remove the folders `path` leaves empty, up to `data/`.
fn prune(dir: &Path, path: &Path) {
    let mut at = path.parent();
    while let Some(p) = at.filter(|p| p.components().count() > 1) {
        if std::fs::remove_dir(dir.join(p)).is_err() {
            break;
        }
        at = p.parent();
    }
}

/// Make `step` in the project at `dir`.
fn perform(dir: &Path, step: &Step) -> std::io::Result<()> {
    match step {
        Step::Rename { from, to } => {
            if let Some(parent) = dir.join(to).parent() {
                std::fs::create_dir_all(parent)?;
            }
            // One rename, so the data is never in two places or in none.
            std::fs::rename(dir.join(from), dir.join(to))?;
            prune(dir, from);
        }
        Step::Remove { path } => {
            std::fs::remove_file(dir.join(path))?;
            prune(dir, path);
        }
        Step::Create { path, bytes } => {
            let file = dir.join(path);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&file)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
    }
    Ok(())
}

/// What a data edit left at each path it touched: every file there, or none.
pub(crate) type Left = Vec<(PathBuf, Vec<(PathBuf, Content)>)>;

impl Session {
    /// Make `steps` on the accepted revision as one edit, if `check` accepts the candidate (its
    /// project and build), and record it for undo as `before` → `after`, shown as `file`.
    pub(crate) fn data_edit(
        &mut self,
        steps: Vec<Step>,
        file: &Path,
        (before, after): (&str, &str),
        check: impl FnOnce(&Project, &Compiled) -> Result<(), EditError>,
    ) -> Result<Edited, EditError> {
        let (revision, inverse, left) = self.make(&steps, check)?;
        let back = file.to_path_buf();
        let shown = moved_to(&steps, file);
        let (before, after) = (before.to_string(), after.to_string());
        let undo = Undo::Data { steps: inverse, left, back };
        self.record(Change { file: shown, left: None, start: 0, before: before.clone(), after: after.clone(), undo });
        Ok(Edited { revision, file: file.display().to_string(), before, after })
    }

    /// Make `steps`: verify, write and accept. Returns the new revision, the steps that undo
    /// them, and what they left.
    pub(crate) fn make(
        &mut self,
        steps: &[Step],
        check: impl FnOnce(&Project, &Compiled) -> Result<(), EditError>,
    ) -> Result<(u64, Vec<Step>, Left), EditError> {
        let rejected = |why: String| EditError::Rejected(why);
        let accepted = &self.accepted.snapshot;
        let mut snapshot = accepted.clone();
        let (mut inverse, mut touched) = (vec![], vec![]);
        for step in steps {
            // What can't be read can't be checked, so it is never moved or removed (FMT-9).
            let gone = match step {
                Step::Rename { from: p, .. } | Step::Remove { path: p } => Some(p),
                Step::Create { .. } => None,
            };
            if let Some(why) = gone.and_then(|g| unread(&snapshot, g)) {
                return Err(rejected(why));
            }
            match step {
                Step::Rename { from, to } => {
                    if under(&snapshot, from).is_empty() {
                        return Err(rejected(format!("the project has no {}", from.display())));
                    }
                    if let Some(by) = taken(&snapshot, to).filter(|by| !by.starts_with(from)) {
                        return Err(rejected(super::there_already(to, &by)));
                    }
                    inverse.push(Step::Rename { from: to.clone(), to: from.clone() });
                    touched.push(to.clone());
                }
                Step::Remove { path } => {
                    let bytes = match under(&snapshot, path).into_iter().find(|(p, _)| p == path) {
                        Some((_, Content::Text(t))) => t.into_bytes(),
                        // A binary's bytes aren't in the snapshot: read them, and check they are its.
                        Some((_, c @ Content::Binary(_))) => {
                            let file = self.dir.join(path);
                            let bytes = std::fs::read(&file).map_err(|e| EditError::Io(e.to_string()))?;
                            if !on_disk(&file, &c)? {
                                return Err(EditError::WriteConflict(path.display().to_string()));
                            }
                            bytes
                        }
                        None => return Err(rejected(format!("the project has no {}", path.display()))),
                    };
                    inverse.push(Step::Create { path: path.clone(), bytes });
                    touched.push(path.clone());
                }
                Step::Create { path, .. } => {
                    if let Some(by) = taken(&snapshot, path) {
                        return Err(rejected(super::there_already(path, &by)));
                    }
                    inverse.push(Step::Remove { path: path.clone() });
                    touched.push(path.clone());
                }
            }
            apply(&mut snapshot, step);
        }
        inverse.reverse();
        let (project, compiled) = self.compile_snapshot(&snapshot)?;
        if compiled.ir != self.accepted.compiled.ir {
            return Err(rejected("it would change the graph, not only where data is".into()));
        }
        check(&project, &compiled)?;

        // The disk must still be as accepted: what is moved or removed as it was, and every
        // name made free.
        let mut disk = self.accepted.snapshot.clone();
        for step in steps {
            let (gone, made) = match step {
                Step::Rename { from, to } => (Some(from), Some(to)),
                Step::Remove { path } => (Some(path), None),
                Step::Create { path, .. } => (None, Some(path)),
            };
            for (p, c) in gone.map(|g| under(&disk, g)).unwrap_or_default() {
                if !on_disk(&self.dir.join(&p), &c)? {
                    return Err(EditError::WriteConflict(p.display().to_string()));
                }
            }
            if let Some(m) = made
                && std::fs::symlink_metadata(self.dir.join(m)).is_ok()
            {
                return Err(EditError::WriteConflict(m.display().to_string()));
            }
            apply(&mut disk, step);
        }
        for step in steps {
            perform(&self.dir, step).map_err(|e| EditError::Io(e.to_string()))?;
        }
        let left = touched.into_iter().map(|p| (p.clone(), under(&snapshot, &p))).collect();
        self.draft = snapshot.clone();
        Ok((self.accept(snapshot, project, compiled), inverse, left))
    }

    /// Whether the accepted revision still has, at every path a data edit touched, exactly what
    /// it left there, and every name its inverse makes is free. The name of one that isn't.
    pub(crate) fn unchanged(&self, steps: &[Step], left: &Left) -> Result<(), EditError> {
        let snap = &self.accepted.snapshot;
        if let Some((p, _)) = left.iter().find(|(p, files)| under(snap, p) != *files) {
            return Err(EditError::Rejected(format!("{} changed since this edit; change it by hand", p.display())));
        }
        for step in steps {
            let made = match step {
                Step::Rename { from, to } => taken(snap, to).filter(|by| !by.starts_with(from)),
                Step::Create { path, .. } => taken(snap, path),
                Step::Remove { .. } => None,
            };
            if let Some(by) = made {
                return Err(EditError::Rejected(format!("{} is taken, so this can't be undone; move it by hand", by.display())));
            }
        }
        Ok(())
    }
}

/// Where `file` is once `steps` are made.
fn moved_to(steps: &[Step], file: &Path) -> PathBuf {
    steps
        .iter()
        .find_map(|s| match s {
            Step::Rename { from, to } => {
                file.strip_prefix(from).ok().map(|rest| if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) })
            }
            _ => None,
        })
        .unwrap_or_else(|| file.to_path_buf())
}
