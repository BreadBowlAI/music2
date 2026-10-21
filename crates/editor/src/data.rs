//! Data fixes and data writes as Tx builders (DIAG-1, DIAG-6, WRITE-1, WRITE-10): move data no
//! lane or plugin reads to where it belongs now (a lane file, a track's whole folder, a plugin's
//! state), delete it, apply any fix a problem offers, save a plugin's state, and place a bounce
//! on an audio lane. Each is made only when asked (DIAG-5), checked, journaled and one undo step.
//!
//! A fix is a ready Tx built where the problem was raised (`core/song`, `unclaimed.rs`):
//! applying one is committing it, and a stale base is a conflict. `relink` and `delete_data`
//! take paths, as the CLI's commands and the strip's buttons name them: they commit the fix the
//! accepted revision offers for those paths, or build the same Tx after the same checks.

use std::path::{Path, PathBuf};

use m2_session::Session;
use m2_session::unclaimed::{Held, name_of, noun, used};
use m2_song::tx::{Base, Confined, Expect, Op, Payload, Tx, TxPath};
use m2_song::{Content, DataPath, Fix, Key, LaneAddr, LaneData, LaneKind, ROOT, Sidecar, Snapshot, Stored};

use crate::plan;
use crate::{EditError, Edited, ready};

/// The codes of the fixes that rewrite one lane file (FMT-10, FMT-12, FMT-15).
const REWRITES: [&str; 5] = ["assign-clip-ids", "renumber-clips", "repin-asset", "fix-asset-name", "retag"];

/// Why `name` isn't created: `by`, which has its name on a disk that ignores capitals (see
/// [`Snapshot::taken`]), is there already.
pub(crate) fn there_already(name: &Path, by: &Path) -> String {
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

/// Commit `tx` as the fix or data edit it is, shown as `file`. A recovery fix is checked against
/// its journal, never the graph, so it is committed as it is: what it restores needn't compile
/// (P3, the step 6 design's 6.5).
fn commit(
    s: &mut Session,
    tx: Tx,
    file: &Path,
    assert: impl FnOnce(&Session, &m2_session::Checked) -> Result<(), EditError>,
) -> Result<Edited, EditError> {
    let (before, after) = tx.label.clone();
    let done = match tx.expect {
        Expect::Journal(_) => s.commit(tx).map_err(EditError::from)?,
        _ => plan::run(s, tx, assert)?,
    };
    Ok(Edited { revision: done.number, file: file.display().to_string(), before, after })
}

/// Commit a fix a problem offered (DIAG-1): its Tx, as it came, guarded by its bases. A fix that
/// rewrites a lane file must leave the lane reading it (the planner's assertion); a recovery
/// fix needs no code that compiles (P3).
pub fn apply_fix(s: &mut Session, fix: &Fix) -> Result<Edited, EditError> {
    let file = fix.tx.touched().first().map(|p| p.path()).unwrap_or_default();
    // Shown as the problem that offers it is about (a folder, for a fix that deletes its files).
    let src = s.problems().iter().find(|d| d.fixes.iter().any(|f| f.tx == fix.tx)).and_then(|d| d.src.clone());
    let shown = src.map_or_else(|| file.clone(), PathBuf::from);
    let rewrite = REWRITES.contains(&fix.code);
    commit(s, fix.tx.clone(), &shown, |_, checked| {
        if !rewrite {
            return Ok(());
        }
        let shown = m2_song::path::slashed(&file);
        let unread =
            checked.candidate().compiled.problems.iter().any(|d| d.src.as_deref() == Some(shown.as_str()) && d.code.starts_with("lane."));
        let read = match DataPath::parse(&file) {
            DataPath::Lane { module, lane } => checked.candidate().compiled.data.lane(module.as_str(), lane.as_str()).is_some(),
            _ => false,
        };
        match read && !unread {
            true => Ok(()),
            false => Err(EditError::Rejected(format!("the fix would leave {shown} unread"))),
        }
    })
}

/// Commit a fix's Tx as a client sends it back (`/api/graph/fix`, `m2 fix`): the fix a problem of
/// the accepted revision offers with that Tx, asserted as [`apply_fix`] does; or, offered no more
/// (its files changed), the Tx as it came, which its bases then refuse as a conflict.
pub fn apply_tx(s: &mut Session, tx: Tx) -> Result<Edited, EditError> {
    let offered = s.problems().iter().flat_map(|d| &d.fixes).find(|f| f.tx == tx).cloned();
    match offered {
        Some(fix) => apply_fix(s, &fix),
        None => {
            let file = tx.touched().first().map(|p| p.path()).unwrap_or_default();
            commit(s, tx, &file, |_, _| Ok(()))
        }
    }
}

/// What a relink does: its renames, its label, the lanes it makes play (its `Confined`), and the
/// lane that must read the moved file (a lane file's relink).
struct Relink {
    moves: Vec<(PathBuf, PathBuf)>,
    label: (String, String),
    lanes: Vec<LaneAddr>,
    reads: Option<LaneAddr>,
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

/// What `path` is, if it is data no lane or plugin reads in the accepted revision and hasn't
/// changed since revision `at`.
fn unclaimed(s: &Session, at: u64, path: &Path) -> Result<Unclaimed, EditError> {
    let rev = s.accepted();
    let name = path.display();
    // A path as the project spells its files, never one that only leads to one.
    let files = rev.snapshot.under(path);
    if files.is_empty() {
        return Err(EditError::Rejected(format!("the project has no {name}")));
    }
    // A folder holding a file that can't be read is moved or deleted whole, or not at all.
    if let Some(why) = rev.snapshot.unread(path) {
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
            Unclaimed::Lane(Held::of(&u, s.seen()))
        }
        DataPath::Module { module } if ir.modules.iter().any(|m| m.key == module.as_str()) => {
            return refused(format!("{} reads {name}", name_of(ir, s.seen(), module.as_str())));
        }
        DataPath::Module { .. } => Unclaimed::Folder,
        DataPath::PluginState { module, path: keys, .. } => {
            let keys: Vec<&str> = keys.iter().map(Key::as_str).collect();
            if rev.compiled.data.state(module.as_str(), &keys).is_some() {
                return refused(format!("a plugin in {} loads {name}", name_of(ir, s.seen(), module.as_str())));
            }
            let side = state_file(path, true);
            let sidecar = rev.snapshot.files.get(&side);
            Unclaimed::State { plugin: sidecar.and_then(|t| Sidecar::read(t).ok()).map(|s| s.plugin), sidecar: sidecar.map(|_| side) }
        }
        _ => return refused(format!("{name} isn't lane data")),
    };
    if at != rev.number && s.snapshot(at).map(|x| x.under(path)) != Some(files) {
        return Err(EditError::Rejected(format!("{name} changed since; look at it again")));
    }
    Ok(what)
}

/// `p` as a Tx names it.
fn tx_path(p: &Path) -> Result<TxPath, EditError> {
    plan::path(&m2_song::path::slashed(p))
}

/// A new name `to` must be free in any capitals, and nothing that can't be read may move.
fn free(snap: &Snapshot, from: &Path, to: &Path) -> Result<(), EditError> {
    if let Some(why) = snap.unread(from) {
        return Err(EditError::Rejected(why));
    }
    match snap.taken(to).filter(|by| !by.starts_with(from)) {
        Some(by) => Err(EditError::Rejected(there_already(to, &by))),
        None => Ok(()),
    }
}

/// Move `from` to `to`, where it is read now: a lane file no lane reads to a declared lane that
/// can read it and has none (`data/kick/hits.json` → `data/kicks/hits.json`); a track's folder
/// no module claims to a declared module with none (`data/kick` → `data/kicks`, lanes and state
/// together, one `Rename` of the folder, ID-17); or a plugin state that isn't loaded, with its
/// sidecar, to a stateful plugin of the plugin that saved it, with none (FMT-17). Planned at
/// revision `at`. One Tx, confined to the lanes it makes play (WRITE-10); undo renames back while
/// the data is as the relink left it and its old name is free.
pub fn relink(s: &mut Session, at: u64, from: &Path, to: &Path) -> Result<Edited, EditError> {
    ready(s)?;
    let what = unclaimed(s, at, from)?;
    let rev = s.accepted();
    let ir = &rev.compiled.ir;
    let no = |what: String| EditError::Rejected(format!("there is no {what} to move it to"));
    let module = |key: &Key| ir.modules.iter().find(|m| m.key == key.as_str()).ok_or_else(|| no(format!("track {key}")));
    let Relink { moves, label, lanes, reads: assert_lane } = match (what, DataPath::parse(to)) {
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
            let label = (format!("unused {}", noun(held.data)), place);
            Relink { moves: vec![(from.to_path_buf(), to.to_path_buf())], label, lanes: vec![addr.clone()], reads: Some(addr) }
        }
        (Unclaimed::Folder, DataPath::Module { module: m }) => {
            let target = module(&m)?;
            if m.as_str() == ROOT || !rev.snapshot.under(to).is_empty() {
                return Err(EditError::Rejected(format!("{} has data of its own in {}, which isn't replaced", m, to.display())));
            }
            // Confined to every lane of the module the folder holds a file for (ID-17).
            let lanes = target
                .lanes
                .iter()
                .filter_map(|l| LaneAddr::new(&target.key, &l.key).ok())
                .filter(|a| rev.snapshot.files.contains_key(&from.join(DataPath::lane(a).path().file_name().unwrap_or_default())))
                .collect();
            let label = ("unused data".to_string(), name_of(ir, s.seen(), target.key.as_str()));
            Relink { moves: vec![(from.to_path_buf(), to.to_path_buf())], label, lanes, reads: None }
        }
        (Unclaimed::State { plugin, sidecar }, DataPath::PluginState { module: m, path, sidecar: false }) => {
            let Some(plugin) = plugin else {
                return Err(EditError::Rejected(format!("{} has no sidecar saying which plugin saved it", from.display())));
            };
            let declared = ir.modules.iter().any(|x| x.key == m.as_str())
                && m2_runtime::decls(ir).states.iter().any(|s| s.module == m && s.path == path && s.plugin == plugin);
            let at = path.iter().map(Key::as_str).collect::<Vec<_>>().join("/");
            if !declared {
                return Err(EditError::Rejected(format!("there is no {plugin} plugin keyed {at} in {m} to move it to")));
            }
            let side = DataPath::PluginState { module: m.clone(), path: path.clone(), sidecar: true }.path();
            if !rev.snapshot.under(to).is_empty() || !rev.snapshot.under(&side).is_empty() {
                return Err(EditError::Rejected(format!("{m} › {at} has state of its own, which isn't replaced")));
            }
            let mut moves = vec![(from.to_path_buf(), to.to_path_buf())];
            moves.extend(sidecar.map(|x| (x, side)));
            Relink { moves, label: ("unused plugin state".to_string(), format!("{m} › {at}")), lanes: vec![], reads: None }
        }
        (_, _) => {
            return Err(EditError::Rejected(format!("{} can't be moved to {}: it is another kind of data", from.display(), to.display())));
        }
    };
    let mut ops = vec![];
    for (a, b) in &moves {
        free(&rev.snapshot, a, b)?;
        ops.push(Op::Rename { from: tx_path(a)?, to: tx_path(b)? });
    }
    let tx = plan::planned(s, label, ops, Expect::Confined(Confined { lanes, ..Confined::default() }));
    commit(s, tx, from, |_, checked| {
        // The planner's assertion: the lane moved to is the one reading the file now.
        let Some(addr) = &assert_lane else { return Ok(()) };
        match checked.candidate().project.lane_file(addr) {
            Some(p) if p == to => Ok(()),
            _ => Err(EditError::Rejected(format!("{} › {} wouldn't read {}", addr.module, addr.lane, to.display()))),
        }
    })
}

/// Delete `path`, data no lane or plugin reads: a lane file, a track's folder, or a plugin state
/// that isn't loaded with its sidecar, as planned at revision `at`. One Tx that changes nothing
/// that plays (`Identical`); undo writes its bytes back while nothing has taken its names.
pub fn delete_data(s: &mut Session, at: u64, path: &Path) -> Result<Edited, EditError> {
    ready(s)?;
    let what = unclaimed(s, at, path)?;
    let rev = s.accepted();
    let (files, before) = match &what {
        Unclaimed::Lane(held) => (vec![path.to_path_buf()], format!("unused {}", noun(held.data))),
        Unclaimed::Folder => (rev.snapshot.under(path).into_iter().map(|(p, _)| p).collect(), "unused data".into()),
        Unclaimed::State { sidecar, .. } => {
            let blob = state_file(path, false);
            let blob = rev.snapshot.binaries.contains_key(&blob).then_some(blob);
            (blob.into_iter().chain(sidecar.clone()).collect::<Vec<_>>(), "unused plugin state".into())
        }
    };
    let mut ops = vec![];
    for f in &files {
        if let Some(why) = rev.snapshot.unread(f) {
            return Err(EditError::Rejected(why));
        }
        // A binary whose hash isn't known can't be guarded, so it isn't removed.
        if let Some(Content::Binary(Stored::Listed | Stored::Unreadable(_))) = rev.snapshot.under(f).first().map(|x| &x.1) {
            return Err(EditError::Rejected(format!("{} can't be read; delete it by hand", f.display())));
        }
        ops.push(Op::Remove { file: tx_path(f)? });
    }
    let tx = plan::planned(s, (before, "deleted".into()), ops, Expect::Identical);
    commit(s, tx, path, |_, _| Ok(()))
}

/// Save a plugin's state (WRITE-1, FMT-17): the blob and the sidecar that says which plugin wrote
/// it, together, as one Tx on their current hashes (or absent). It may replace a state that is
/// there, as one undoable step. The state it loads is its own write (C1).
pub fn plugin_state(s: &mut Session, module: &Key, path: &[Key], blob: Vec<u8>, sidecar: &Sidecar) -> Result<Edited, EditError> {
    let w = state_writes(s, module, path, blob, sidecar)?;
    let place = format!("{module} › {}", path.iter().map(Key::as_str).collect::<Vec<_>>().join("/"));
    let was = if s.accepted().snapshot.binaries.contains_key(&w.blob) { "saved state" } else { "no saved state" };
    let mut tx =
        plan::planned(s, (format!("{place}: {was}"), format!("{place}: {}", sidecar.plugin)), w.ops, Expect::Confined(Confined::default()));
    tx.base.extend(w.bases);
    commit(s, tx, &w.blob, |_, _| Ok(()))
}

/// A plugin state's two writes (FMT-17): the blob and its sidecar, and their bases.
pub(crate) struct StateWrites {
    pub ops: Vec<Op>,
    /// Each file as it is on disk now, so a state saved here replaces what is there (one step
    /// undo puts back).
    pub bases: Vec<(TxPath, Base)>,
    pub blob: PathBuf,
    /// The blob there now, by its hash; None when there is none.
    pub held: Option<m2_song::Sha>,
}

/// The writes that save `blob` as the state of the plugin at `path` of `module`, with its sidecar.
pub(crate) fn state_writes(s: &Session, module: &Key, path: &[Key], blob: Vec<u8>, sidecar: &Sidecar) -> Result<StateWrites, EditError> {
    let at = |sidecar| DataPath::PluginState { module: module.clone(), path: path.to_vec(), sidecar }.path();
    let (blob_path, side_path) = (at(false), at(true));
    let ops = vec![
        Op::Write { file: tx_path(&blob_path)?, bytes: Payload::Bytes(blob.into()) },
        Op::Write { file: tx_path(&side_path)?, bytes: Payload::Text(sidecar.canonical().into()) },
    ];
    let mut bases = vec![];
    let mut held = None;
    for p in [&blob_path, &side_path] {
        let base = match std::fs::read(s.accepted().project.dir.join(p)) {
            Ok(bytes) => Base::Sha(m2_song::Sha::of(&bytes)),
            Err(_) => Base::Absent,
        };
        if let (true, Base::Sha(sha)) = (p == &blob_path, &base) {
            held = Some(*sha);
        }
        bases.push((tx_path(p)?, base));
    }
    Ok(StateWrites { ops, bases, blob: blob_path, held })
}

/// Put one clip on audio lane `addr` (WRITE-1's `m2 bounce --place`): a `Clips` op when the lane
/// is addressable, else the whole lane, on the lane file's text `was` (read before the render, so
/// a lane changed meanwhile is a conflict). The clip's id is derived from the Tx's (FMT-11).
pub fn place_clip(s: &mut Session, addr: &LaneAddr, was: Option<&str>, clip: m2_song::AudioClip) -> Result<Edited, EditError> {
    let path = TxPath::lane(addr);
    let id = m2_song::tx::random_id();
    let mut clips = match was {
        Some(t) => match LaneData::read(t, &LaneKind::Audio) {
            Ok(LaneData::Audio(c)) => c,
            _ => return Err(EditError::Rejected(format!("{path} isn't an audio lane's data; place the clip by hand"))),
        },
        None => vec![],
    };
    let new_id = (0..)
        .map(|n| m2_song::ClipId::derive(&id, addr, n))
        .find(|i| !clips.iter().any(|c| c.id.as_deref() == Some(i)))
        .expect("an id is free");
    let clip = m2_song::AudioClip { id: Some(new_id), ..clip };
    let addressable = LaneData::Audio(clips.clone()).addressable() && was.is_some();
    let op = match addressable {
        true => {
            let line = LaneData::Audio(vec![clip.clone()]).clip_lines().remove(0).1;
            Op::Clips { lane: addr.clone(), put: vec![serde_json::from_str(&line).expect("a canonical line is JSON")], delete: vec![] }
        }
        false => {
            clips.push(clip.clone());
            let mut data = LaneData::Audio(clips);
            data.sort();
            Op::Write { file: path.clone(), bytes: Payload::Text(m2_song::canonical(&data).into()) }
        }
    };
    let label =
        (format!("{} › {}", addr.module, addr.lane), format!("clip of {} at beat {}", clip.asset, m2_song::lane::number(clip.start)));
    let lanes = Expect::Confined(Confined { lanes: vec![addr.clone()], ..Confined::default() });
    let mut tx = plan::planned(s, label, vec![op], lanes);
    tx.id = id;
    tx.base.insert(path, was.map_or(Base::Absent, Base::text));
    commit(s, tx, &DataPath::lane(addr).path(), |_, _| Ok(()))
}

#[cfg(test)]
#[path = "data_tests.rs"]
mod tests;
