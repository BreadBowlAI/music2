//! The fixes the session applies (DIAG-1, DIAG-6), as named edits: move data no lane or plugin
//! reads to where it belongs now (a lane file, a track's whole folder, a plugin's state), delete
//! it, or rewrite a lane file as a data problem's fix says. Each is an edit like any other:
//! verified on a candidate first, one undo step, and made only when asked, never because a
//! problem offers it (DIAG-5). Step 6 makes each fix a Tx.

use std::path::{Path, PathBuf};

use m2_song::diag::Op;
use m2_song::{DataPath, Key, LaneAddr, ROOT, Sidecar};

use super::data::{Step, under, unread};
use super::{EditError, Edited, Session};
use crate::unclaimed::{Held, name_of, noun, used};

/// The codes of the fixes that rewrite one lane file (FMT-10, FMT-12, FMT-15).
const REWRITES: [&str; 5] = ["assign-clip-ids", "renumber-clips", "repin-asset", "fix-asset-name", "retag"];

impl Session {
    /// Apply the fix `code` with `edit` (as the fix carries it, in JSON) that a problem of the
    /// accepted revision offers, as planned at revision `at`. A fix no problem offers any more
    /// (the files changed, so its bases did) is refused.
    pub fn fix(&mut self, at: u64, code: &str, edit: &serde_json::Value) -> Result<Edited, EditError> {
        self.ready()?;
        let offered = self.accepted().compiled.problems.iter().find_map(|d| {
            let f = d.fixes.iter().find(|f| f.code == code && serde_json::to_value(&f.edit).ok().as_ref() == Some(edit))?;
            Some((d.src.clone(), f.clone()))
        });
        let Some((src, fix)) = offered else {
            return Err(EditError::Rejected("that fix isn't offered any more: the project changed since; look at it again".into()));
        };
        match (fix.code, &fix.edit.ops[..]) {
            ("relink", [Op::Rename { from, to }, ..]) => self.relink(at, Path::new(from), Path::new(to)),
            ("delete-data", _) => self.delete_data(at, Path::new(src.as_deref().unwrap_or_default())),
            (code, [Op::Write { file, text }]) if REWRITES.contains(&code) => self.rewrite_lane(at, Path::new(file), text, &fix.title),
            (code, _) => Err(EditError::Rejected(format!("the `{code}` fix can't be applied here yet"))),
        }
    }

    /// Move `from` to `to`, where it is read now: a lane file no lane reads to a declared lane
    /// that can read it and has none (`data/kick/hits.json` → `data/kicks/hits.json`); a track's
    /// folder no module claims to a declared module with none (`data/kick` → `data/kicks`, lanes
    /// and state together); or a plugin state that isn't loaded, with its sidecar, to a
    /// stateful plugin of the plugin that saved it, with none (FMT-17). Planned at revision `at`.
    /// One rename per file or folder, after checking the new name is free; undo renames back
    /// while the data is unchanged and its old name is free.
    pub fn relink(&mut self, at: u64, from: &Path, to: &Path) -> Result<Edited, EditError> {
        self.ready()?;
        let what = self.unclaimed(at, from)?;
        let rev = self.accepted();
        let ir = &rev.compiled.ir;
        let no = |what: String| EditError::Rejected(format!("there is no {what} to move it to"));
        let module = |key: &Key| ir.modules.iter().find(|m| m.key == key.as_str()).ok_or_else(|| no(format!("track {key}")));
        match (what, DataPath::parse(to)) {
            (Unclaimed::Lane(held), DataPath::Lane { module: m, lane }) => {
                let decl = module(&m)?.lanes.iter().find(|l| l.key == lane.as_str()).ok_or_else(|| no(format!("lane {lane} on {m}")))?;
                let place = format!("{m} › {lane}");
                if let Some(what) = held.unreadable(decl.kind) {
                    return Err(EditError::Rejected(format!("{place} is a {} lane, which can't read {what}", decl.kind)));
                }
                if rev.snapshot.files.contains_key(to) {
                    return Err(EditError::Rejected(format!("{place} has data of its own in {}, which isn't replaced", to.display())));
                }
                let addr = LaneAddr { module: m, lane };
                let steps = vec![Step::Rename { from: from.to_path_buf(), to: to.to_path_buf() }];
                self.data_edit(steps, from, (&format!("unused {}", noun(held.data)), &place), |project, _| {
                    // It compiled, so the data parses as the lane's kind; the lane must also be the one reading it.
                    match project.lane_file(&addr) {
                        Some(p) if p == to => Ok(()),
                        _ => Err(EditError::Rejected(format!("{place} wouldn't read {}", to.display()))),
                    }
                })
            }
            (Unclaimed::Folder, DataPath::Module { module: m }) => {
                let target = module(&m)?;
                if m.as_str() == ROOT || !under(&rev.snapshot, to).is_empty() {
                    return Err(EditError::Rejected(format!("{} has data of its own in {}, which isn't replaced", m, to.display())));
                }
                let name = name_of(ir, &self.seen, target.key.as_str());
                let steps = vec![Step::Rename { from: from.to_path_buf(), to: to.to_path_buf() }];
                self.data_edit(steps, from, ("unused data", &name), |_, _| Ok(()))
            }
            (Unclaimed::State { plugin, sidecar }, DataPath::PluginState { module: m, path, sidecar: false }) => {
                let Some(plugin) = plugin else {
                    return Err(EditError::Rejected(format!("{} has no sidecar saying which plugin saved it", from.display())));
                };
                let declared = rev.compiled.ir.modules.iter().any(|x| x.key == m.as_str())
                    && m2_runtime::decls(ir).states.iter().any(|s| s.module == m && s.path == path && s.plugin == plugin);
                let at = path.iter().map(Key::as_str).collect::<Vec<_>>().join("/");
                if !declared {
                    return Err(EditError::Rejected(format!("there is no {plugin} plugin keyed {at} in {m} to move it to")));
                }
                let side = DataPath::PluginState { module: m.clone(), path: path.clone(), sidecar: true }.path();
                if !under(&rev.snapshot, to).is_empty() || !under(&rev.snapshot, &side).is_empty() {
                    return Err(EditError::Rejected(format!("{m} › {at} has state of its own, which isn't replaced")));
                }
                let mut steps = vec![Step::Rename { from: from.to_path_buf(), to: to.to_path_buf() }];
                steps.extend(sidecar.map(|s| Step::Rename { from: s, to: side }));
                self.data_edit(steps, from, ("unused plugin state", &format!("{m} › {at}")), |_, _| Ok(()))
            }
            (_, _) => {
                Err(EditError::Rejected(format!("{} can't be moved to {}: it is another kind of data", from.display(), to.display())))
            }
        }
    }

    /// Delete `path`, data no lane or plugin reads: a lane file, a track's folder, or a plugin
    /// state that isn't loaded with its sidecar, as planned at revision `at`. Undo writes its
    /// bytes back while nothing has taken its names.
    pub fn delete_data(&mut self, at: u64, path: &Path) -> Result<Edited, EditError> {
        self.ready()?;
        let what = self.unclaimed(at, path)?;
        let rev = self.accepted();
        let (files, before) = match &what {
            Unclaimed::Lane(held) => (vec![path.to_path_buf()], format!("unused {}", noun(held.data))),
            Unclaimed::Folder => (under(&rev.snapshot, path).into_iter().map(|(p, _)| p).collect(), "unused data".into()),
            Unclaimed::State { sidecar, .. } => {
                let blob = state_file(path, false);
                let blob = rev.snapshot.binaries.contains_key(&blob).then_some(blob);
                (blob.into_iter().chain(sidecar.clone()).collect(), "unused plugin state".into())
            }
        };
        let steps = files.into_iter().map(|path| Step::Remove { path }).collect();
        self.data_edit(steps, path, (&before, "deleted"), |_, _| Ok(()))
    }

    /// Rewrite lane file `file` as `text`, a data problem's fix (`title`) offered at revision
    /// `at`: clip ids, asset names and pins, or the recorded unit. The graph must stay as it is,
    /// and the lane must read the new file.
    fn rewrite_lane(&mut self, at: u64, file: &Path, text: &str, title: &str) -> Result<Edited, EditError> {
        let rev = self.accepted();
        let DataPath::Lane { module, lane } = DataPath::parse(file) else {
            return Err(EditError::Rejected(format!("{} isn't a lane file", file.display())));
        };
        let now = rev.snapshot.files.get(file);
        if at != rev.number && self.snapshot(at).map(|s| s.files.get(file)) != Some(now) {
            return Err(EditError::Rejected(format!("{} changed since; look at it again", file.display())));
        }
        let shown = file.display().to_string();
        self.write_file(file, Some(text), (&format!("{module} › {lane}"), &title.to_lowercase()), |accepted, _, compiled| {
            let unread = compiled.problems.iter().any(|d| d.src.as_deref() == Some(shown.as_str()) && d.code.starts_with("lane."));
            if compiled.ir != accepted.compiled.ir || unread || compiled.data.lane(module.as_str(), lane.as_str()).is_none() {
                return Err(EditError::Rejected(format!("the fix would leave {shown} unread, or change the graph")));
            }
            Ok(())
        })
    }

    /// What `path` is, if it is data no lane or plugin reads in the accepted revision and hasn't
    /// changed since revision `at`.
    fn unclaimed(&self, at: u64, path: &Path) -> Result<Unclaimed, EditError> {
        let rev = self.accepted();
        let name = path.display();
        // A path as the project spells its files, never one that only leads to one.
        let files = under(&rev.snapshot, path);
        if files.is_empty() {
            return Err(EditError::Rejected(format!("the project has no {name}")));
        }
        // A folder holding a file that can't be read is moved or deleted whole, or not at all.
        if let Some(why) = unread(&rev.snapshot, path) {
            return Err(EditError::Rejected(why));
        }
        let refused =
            |why: String| Err(EditError::Rejected(format!("only data no lane or plugin reads is moved or deleted this way, and {why}")));
        let (ir, project) = (&rev.compiled.ir, &rev.project);
        let what = match DataPath::parse(path) {
            DataPath::Lane { .. } => {
                let Some(u) = project.unused(&used(ir)).into_iter().find(|u| u.path == path) else {
                    let reader = project.lane_files().find(|(_, p, _)| *p == path).map(|(a, ..)| a);
                    return refused(match reader {
                        Some(a) => format!("{} › {} reads {name}", a.module, a.lane),
                        None => format!("{name} isn't lane data"),
                    });
                };
                Unclaimed::Lane(Held::of(&u, &self.seen))
            }
            DataPath::Module { module } if ir.modules.iter().any(|m| m.key == module.as_str()) => {
                return refused(format!("{} reads {name}", name_of(ir, &self.seen, module.as_str())));
            }
            DataPath::Module { .. } => Unclaimed::Folder,
            DataPath::PluginState { module, path: keys, .. } => {
                let keys: Vec<&str> = keys.iter().map(Key::as_str).collect();
                if rev.compiled.data.state(module.as_str(), &keys).is_some() {
                    return refused(format!("a plugin in {} loads {name}", name_of(ir, &self.seen, module.as_str())));
                }
                let side = state_file(path, true);
                let sidecar = rev.snapshot.files.get(&side);
                Unclaimed::State { plugin: sidecar.and_then(|t| Sidecar::read(t).ok()).map(|s| s.plugin), sidecar: sidecar.map(|_| side) }
            }
            _ => return refused(format!("{name} isn't lane data")),
        };
        if at != rev.number && self.snapshot(at).map(|s| under(s, path)) != Some(files) {
            return Err(EditError::Rejected(format!("{name} changed since; look at it again")));
        }
        Ok(what)
    }
}

/// What a path no lane or plugin reads holds.
enum Unclaimed {
    Lane(Held),
    /// A `data/<module>/` folder no module claims.
    Folder,
    /// A plugin state: the plugin its sidecar names, and the sidecar.
    State {
        plugin: Option<String>,
        sidecar: Option<PathBuf>,
    },
}

/// The blob or sidecar of the plugin state `path` is part of.
fn state_file(path: &Path, sidecar: bool) -> PathBuf {
    match DataPath::parse(path) {
        DataPath::PluginState { module, path, .. } => DataPath::PluginState { module, path, sidecar }.path(),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
#[path = "relink_tests.rs"]
mod tests;
