//! Fixes for lane data no lane reads (see [`crate::warnings`]): move it to the lane it belongs
//! to now, or delete it. Each is an edit like any other: verified on a candidate first, one
//! undo step, and made only when asked, never because a warning offers it.

use std::path::Path;

use super::{Change, EditError, Edited, Session, Undo};
use crate::warnings::{Held, declared, noun, used};

impl Session {
    /// Move `from`, a lane file no lane reads, to `module`'s lane `lane`, which must read its
    /// kind of data (automation of its unit, as far as can be told) and have none yet, as
    /// planned at revision `at`. It is renamed (one rename, after checking the new name is free
    /// in any capitals) once the project compiles with the lane reading it. Undo renames it
    /// back while it is unchanged and its old name is free.
    pub fn relink(&mut self, at: u64, from: &Path, module: &str, lane: &str) -> Result<Edited, EditError> {
        self.ready()?;
        let held = self.unclaimed(at, from)?;
        let rev = self.accepted();
        let no = |what: String| EditError::Rejected(format!("there is no {what} to move it to"));
        let m = rev.compiled.ir.modules.iter().find(|m| m.name == module).ok_or_else(|| no(format!("track {module}")))?;
        let decl = m.lanes.iter().find(|l| l.id == lane).ok_or_else(|| no(format!("lane {lane} on {module}")))?;
        let place = format!("{module} › {lane}");
        if let Some(what) = held.unreadable(decl.kind) {
            return Err(EditError::Rejected(format!("{place} is a {} lane, which can't read {what}", decl.kind)));
        }
        let to = crate::lanes::file(module, lane)?;
        if rev.snapshot.files.contains_key(&to) {
            return Err(EditError::Rejected(format!("{place} has data of its own in {}, which isn't replaced", to.display())));
        }
        let text = rev.snapshot.files.get(from).cloned();
        let revision = self.move_file(from, &to, |project, _| {
            // It compiled, so the data parses as the lane's kind; the lane must also be the one reading it.
            match project.timeline.lane_file(module, lane) {
                Some(p) if p == project.dir.join(&to) => Ok(()),
                _ => Err(EditError::Rejected(format!("{place} wouldn't read {}", to.display()))),
            }
        })?;
        let before = format!("unused {}", noun(held.data));
        let undo = Undo::Move { back: from.to_path_buf() };
        self.record(Change { file: to, left: text, start: 0, before: before.clone(), after: place.clone(), undo });
        Ok(Edited { revision, file: from.display().to_string(), before, after: place })
    }

    /// Delete `file`, a lane file no lane reads, as planned at revision `at`. Undo writes its
    /// bytes back while nothing has taken its name.
    pub fn delete_data(&mut self, at: u64, file: &Path) -> Result<Edited, EditError> {
        self.ready()?;
        let before = format!("unused {}", noun(self.unclaimed(at, file)?.data));
        self.write_file(file, None, (&before, "deleted"), |accepted, _, compiled| {
            if compiled.ir != accepted.compiled.ir {
                return Err(EditError::Rejected(format!("deleting {} would change the graph", file.display())));
            }
            Ok(())
        })
    }

    /// What `file` holds, if it is a lane file no lane reads in the accepted revision and
    /// hasn't changed since revision `at`.
    fn unclaimed(&self, at: u64, file: &Path) -> Result<Held, EditError> {
        let rev = self.accepted();
        let (path, name) = (rev.project.dir.join(file), file.display());
        // A path as the project spells its files, never one that only leads to one.
        if !rev.snapshot.files.contains_key(file) {
            return Err(EditError::Rejected(format!("the project has no {name}")));
        }
        let (ir, timeline) = (&rev.compiled.ir, &rev.project.timeline);
        let Some(u) = timeline.unused(&used(ir)).into_iter().find(|u| u.path == path) else {
            let reader = declared(ir).find(|(m, l)| timeline.lane_file(&m.name, &l.id) == Some(path.as_path()));
            let why = match reader {
                Some((m, l)) => format!("{} › {} reads {name}", m.name, l.id),
                None => format!("{name} isn't lane data"),
            };
            return Err(EditError::Rejected(format!("only lane data no lane reads is moved or deleted this way, and {why}")));
        };
        let now = rev.snapshot.files.get(file);
        if at != rev.number && self.snapshot(at).map(|s| s.files.get(file)) != Some(now) {
            return Err(EditError::Rejected(format!("{name} changed since; look at it again")));
        }
        Ok(Held::of(timeline, &u, &self.seen))
    }
}

#[cfg(test)]
#[path = "relink_tests.rs"]
mod tests;
