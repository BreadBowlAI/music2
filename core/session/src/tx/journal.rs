//! The commit journal and recovery as a diagnostic (WRITE-15, WRITE-16, DIAG-5).
//!
//! A commit writes `.music2/tx/<jid>.json` before it changes the project: the host, and the
//! before and after hashes of every target, with the before bytes of every text target inline
//! and of a binary one in `.music2/tx/blobs/<sha>`. A journal left behind is read when a session
//! next takes the lock, oldest first:
//!
//! - every target at its after hash: a completed commit awaiting its tidy, removed silently,
//!   since that changes no project file (P3: tidying, not recovery; C9);
//! - a target at neither hash: someone changed it since; the journal is set aside in
//!   `.music2/tx/aside/` and reported with no fix, and no project file is touched;
//! - otherwise `tx.interrupted`, with FinishTx (from the staged files, when they are all there)
//!   and DiscardTx (from the before bytes), each a Tx committed only when asked.
//!
//! A journal from another machine (a synced folder) is `tx.foreign`, with no fix. Recovery
//! never runs by itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_song::tx::{Base, Expect, Op, Payload, Tx, TxPath, valid_journal};
use m2_song::{Diagnostic, Fix, Sha, Stored};
use serde_json::{Value, json};

use super::stage::{self, Before, Disk, tx_dir};

/// A commit as its journal records it.
#[derive(Clone, Debug, PartialEq)]
pub struct Journal {
    pub id: String,
    pub tx: String,
    pub host: String,
    pub host_id: String,
    pub pid: u32,
    pub program: String,
    pub label: (String, String),
    pub targets: Vec<JTarget>,
}

/// One target of a journaled commit.
#[derive(Clone, Debug, PartialEq)]
pub enum JTarget {
    /// A file written (`before` None: created) or removed (`after` None). `text`: its before
    /// bytes, inline; `blob`: they are in `blobs/<before>`; `staged`: its new bytes are
    /// `.music2/tx/<jid>/<staged>`.
    File { path: String, before: Option<Sha>, after: Option<Sha>, text: Option<String>, blob: bool, staged: Option<usize> },
    /// A file or a folder renamed; `hash` is its content's (a folder's directory hash).
    Rename { from: String, to: String, hash: Sha, dir: bool },
}

/// How a target's before bytes are kept: inline when they are a text file's UTF-8, else as a
/// blob. None for a created file.
pub fn before_image(path: &str, before: Option<&Before>) -> (Option<String>, bool) {
    let Some(b) = before else { return (None, false) };
    let text = TxPath::new(path).is_ok_and(|p| p.is_text());
    match std::str::from_utf8(&b.bytes) {
        Ok(t) if text => (Some(t.to_string()), false),
        _ => (None, true),
    }
}

fn sha_json(s: &Option<Sha>) -> Value {
    s.map_or(Value::Null, |s| json!(s.prefixed()))
}

impl Journal {
    pub fn to_json(&self) -> String {
        let targets: Vec<Value> = self
            .targets
            .iter()
            .map(|t| match t {
                JTarget::File { path, before, after, text, blob, staged } => {
                    let mut v = json!({"path": path, "before": sha_json(before), "after": sha_json(after)});
                    if let Some(t) = text {
                        v["text"] = json!(t);
                    }
                    if *blob {
                        v["blob"] = json!(true);
                    }
                    if let Some(k) = staged {
                        v["staged"] = json!(k);
                    }
                    v
                }
                JTarget::Rename { from, to, hash, dir } => json!({"rename": [from, to], "hash": hash.prefixed(), "dir": dir}),
            })
            .collect();
        json!({"journal": 1, "id": self.id, "tx": self.tx, "host": self.host, "host_id": self.host_id, "pid": self.pid,
               "program": self.program, "label": [self.label.0, self.label.1], "targets": targets})
        .to_string()
    }

    /// A journal from its file, or why not.
    pub fn parse(text: &str) -> Result<Journal, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if v["journal"] != json!(1) {
            return Err("it isn't a version 1 journal".into());
        }
        let s = |k: &str| v[k].as_str().map(str::to_string).ok_or_else(|| format!("no `{k}`"));
        let sha = |x: &Value| -> Result<Option<Sha>, String> {
            match x {
                Value::Null => Ok(None),
                Value::String(s) => Sha::parse_prefixed(s).map(Some).ok_or_else(|| format!("`{s}` isn't a hash")),
                _ => Err("a hash must be a string".into()),
            }
        };
        let path = |x: &Value| -> Result<String, String> {
            let p = x.as_str().ok_or("a path must be a string")?;
            TxPath::new(p).map(|p| p.to_string())
        };
        let mut targets = vec![];
        for t in v["targets"].as_array().ok_or("no `targets`")? {
            targets.push(match t.get("rename").and_then(Value::as_array).map(Vec::as_slice) {
                Some([from, to]) => JTarget::Rename {
                    from: path(from)?,
                    to: path(to)?,
                    hash: sha(&t["hash"])?.ok_or("a rename needs its hash")?,
                    dir: t["dir"].as_bool().unwrap_or(false),
                },
                Some(_) => return Err("a rename is [from, to]".into()),
                None => JTarget::File {
                    path: path(&t["path"])?,
                    before: sha(&t["before"])?,
                    after: sha(&t["after"])?,
                    text: t.get("text").and_then(Value::as_str).map(str::to_string),
                    blob: t["blob"].as_bool().unwrap_or(false),
                    staged: t.get("staged").and_then(Value::as_u64).map(|k| k as usize),
                },
            });
        }
        let id = s("id")?;
        if !valid_journal(&id) {
            return Err(format!("`{id}` isn't a journal's name"));
        }
        let label = (v["label"][0].as_str().unwrap_or("").to_string(), v["label"][1].as_str().unwrap_or("").to_string());
        let pid = v["pid"].as_u64().unwrap_or(0) as u32;
        Ok(Journal { id, tx: s("tx")?, host: s("host")?, host_id: s("host_id")?, pid, program: s("program")?, label, targets })
    }

    /// The paths its targets change.
    pub fn paths(&self) -> Vec<&str> {
        self.targets
            .iter()
            .flat_map(|t| match t {
                JTarget::File { path, .. } => vec![path.as_str()],
                JTarget::Rename { from, to, .. } => vec![from.as_str(), to.as_str()],
            })
            .collect()
    }

    /// The before-images its targets keep in `blobs/`.
    pub fn blobs(&self) -> Vec<Sha> {
        self.targets
            .iter()
            .filter_map(|t| match t {
                JTarget::File { before: Some(b), blob: true, .. } => Some(*b),
                _ => None,
            })
            .collect()
    }
}

/// Where a target is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    Before,
    After,
    Neither,
}

/// A file's hash in the project at `dir`, or None when it isn't there.
fn file_sha(dir: &Path, p: &str) -> Option<Sha> {
    Sha::of_file(&dir.join(p)).ok()
}

/// A path's hash: a file's, or for a folder its directory hash.
fn sha_of(dir: &Path, p: &str, folder: bool) -> Option<Sha> {
    if folder { super::stage::folder_sha(&mut super::stage::Real, dir, p).ok().flatten() } else { file_sha(dir, p) }
}

/// Where target `t` is in the project at `dir`.
pub fn at(dir: &Path, t: &JTarget) -> At {
    match t {
        JTarget::File { path, before, after, .. } => {
            let now = file_sha(dir, path);
            if now == *after {
                At::After
            } else if now == *before {
                At::Before
            } else {
                At::Neither
            }
        }
        JTarget::Rename { from, to, hash, dir: folder } => match (sha_of(dir, from, *folder), sha_of(dir, to, *folder)) {
            (None, Some(h)) if h == *hash => At::After,
            (Some(h), None) if h == *hash => At::Before,
            _ => At::Neither,
        },
    }
}

/// What a scan of the journals found.
#[derive(Debug, Default)]
pub struct Scan {
    /// The session's problems: `tx.interrupted` (with fixes when it can be finished or
    /// discarded) and `tx.foreign`.
    pub problems: Vec<Diagnostic>,
    /// The interrupted journals that offer fixes: a Tx touching one of their targets is refused
    /// until it is finished or discarded.
    pub interrupted: Vec<Journal>,
    /// Completed journals tidied, and journals set aside (a writer's scan only).
    pub tidied: Vec<String>,
    pub aside: Vec<String>,
    /// Completed journals whose tidy found the barrier failing: their folders are pushed, and they
    /// go after the session's next barrier, as its own commits' do (its `pending`).
    pub awaiting: Vec<String>,
    /// The before-images journals left alone still name: another machine's, which can still be
    /// discarded there, and those set aside, for whoever checks them by hand. The tidy of
    /// `blobs/` keeps them (6a.2 step 6: blobs no journal names).
    pub keep: Vec<Sha>,
}

/// `<unix ms>-<pid>-<n>` as numbers, for oldest first.
fn order(id: &str) -> Vec<u128> {
    id.split('-').map(|p| p.parse().unwrap_or(u128::MAX)).collect()
}

/// The journals in `.music2/tx/` of the project at `dir`, oldest first, each with its text or
/// why it can't be read.
fn journals(dir: &Path) -> Vec<(String, Result<Journal, String>)> {
    let Ok(entries) = std::fs::read_dir(tx_dir(dir)) else { return vec![] };
    let mut found: Vec<(String, Result<Journal, String>)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json").filter(|_| !name.starts_with('.'))?.to_string();
            let parsed = std::fs::read_to_string(e.path()).map_err(|e| e.to_string()).and_then(|t| Journal::parse(&t));
            Some((id, parsed))
        })
        .collect();
    found.sort_by_key(|(id, _)| order(id));
    found
}

/// Read every journal of the project at `dir` (6a.4), as `host_id` (this machine). A writer
/// (`tidy`, through its disk) removes completed ones and sets aside the ones it can't fix, and
/// sweeps what a crash left half made (temporary files, a staging folder whose journal went); a
/// reader changes nothing and only reports. `skip` names journals of the scanning session's own
/// completed commits, which await its next barrier.
pub fn scan(dir: &Path, host_id: &str, tidy: Option<&mut dyn Disk>, skip: &[String]) -> Scan {
    let mut out = Scan::default();
    let all: Vec<(String, Result<Journal, String>)> = journals(dir).into_iter().filter(|(id, _)| !skip.contains(id)).collect();
    let mut completed: Vec<&Journal> = vec![];
    for (n, (id, parsed)) in all.iter().enumerate() {
        let src = format!(".music2/tx/{id}.json");
        let j = match parsed {
            Ok(j) => j,
            Err(why) => {
                if tidy.is_some() {
                    set_aside(dir, id);
                    out.aside.push(id.clone());
                }
                out.problems.push(
                    Diagnostic::error(
                        "tx.interrupted",
                        format!(
                            "{src} records a commit that may not have finished, but it can't be read ({why}). Check the files it \
                             may have changed by hand"
                        ),
                    )
                    .file(&src),
                );
                continue;
            }
        };
        if j.host_id != host_id {
            out.keep.extend(j.blobs());
            out.problems.push(
                Diagnostic::warning(
                    "tx.foreign",
                    format!(
                        "{src} records a commit made on another computer ({}) that may not have finished; recover it there, or \
                         remove the file once the project is as it should be",
                        j.host
                    ),
                )
                .file(&src),
            );
            continue;
        }
        let newer: Vec<&Journal> = all[n + 1..].iter().filter_map(|(_, j)| j.as_ref().ok()).collect();
        let states = states(dir, j, &newer);
        if states.iter().all(|s| *s == At::After) {
            completed.push(j);
            continue;
        }
        let what = if j.label.1.is_empty() { j.tx.clone() } else { format!("{} → {}", j.label.0, j.label.1) };
        if states.contains(&At::Neither) {
            let changed: Vec<&str> =
                j.targets.iter().zip(&states).filter(|(_, s)| **s == At::Neither).flat_map(|(t, _)| target_paths(t)).collect();
            out.keep.extend(j.blobs());
            if tidy.is_some() {
                set_aside(dir, id);
                out.aside.push(id.clone());
            }
            out.problems.push(
                Diagnostic::error(
                    "tx.interrupted",
                    format!(
                        "A commit ({what}) was interrupted, and {} changed since, so it can't be finished or undone safely. Check \
                         {} by hand{}",
                        changed.join(", "),
                        if changed.len() == 1 { "it" } else { "them" },
                        if tidy.is_some() { "; the journal is in .music2/tx/aside/" } else { "" }
                    ),
                )
                .file(&src),
            );
            continue;
        }
        let finish = finish_tx(dir, j, &states);
        let discard = discard_tx(dir, j, &states);
        let mut d = Diagnostic::error(
            "tx.interrupted",
            format!(
                "A commit ({what}) was interrupted: {} of its {} changes reached the files. Finish it, or discard it to put every \
                 file back as it was",
                states.iter().filter(|s| **s == At::After).count(),
                states.len()
            ),
        )
        .file(&src);
        // The problem names the journal (its `src`), and each Tx's expectation does, so the app
        // and `m2 fix` tell two interrupted commits' fixes apart.
        if let Some(tx) = finish {
            d = d.fix(Fix { code: "finish-tx", title: "Finish it".into(), likely: true, tx });
        }
        if let Some(tx) = discard {
            d = d.fix(Fix { code: "discard-tx", title: "Discard it".into(), likely: false, tx });
        }
        out.problems.push(d);
        out.interrupted.push(j.clone());
    }
    // Journals set aside earlier still name their before-images.
    if let Ok(entries) = std::fs::read_dir(tx_dir(dir).join("aside")) {
        for e in entries.flatten() {
            if let Ok(j) = std::fs::read_to_string(e.path()).map_err(|e| e.to_string()).and_then(|t| Journal::parse(&t)) {
                out.keep.extend(j.blobs());
            }
        }
    }
    if let Some(disk) = tidy {
        tidy_completed(disk, dir, &completed, &mut out);
        sweep(dir);
    }
    out
}

/// Remove the journals of `completed` commits once their renames are durable (WRITE-15): their
/// folders pushed, as each commit's step 8 would have (it may have been killed before), then one
/// barrier. A barrier that fails leaves them `awaiting` the session's next barrier: left to the next
/// lock alone, a later commit to the same file would leave it at neither of their hashes.
fn tidy_completed(disk: &mut dyn Disk, dir: &Path, completed: &[&Journal], out: &mut Scan) {
    let Some(first) = completed.first() else { return };
    let paths = completed.iter().flat_map(|j| &j.targets).flat_map(|t| {
        // A file written over is renamed in its folder alone; any other target makes, removes or
        // moves a name, maybe with folders above it.
        let deep = !matches!(t, JTarget::File { before: Some(_), after: Some(_), .. });
        target_paths(t).into_iter().map(move |p| (p, deep))
    });
    stage::push_parents(disk, dir, paths);
    if disk.barrier(&tx_dir(dir).join(format!("{}.json", first.id))).is_err() {
        out.awaiting.extend(completed.iter().map(|j| j.id.clone()));
        return;
    }
    for j in completed {
        // Its staging first: a journal never outlives its staging unseen (6a.3).
        stage::tidy(disk, &tx_dir(dir), &j.id);
        out.tidied.push(j.id.clone());
    }
}

/// Remove what a commit killed half way left that no journal accounts for (6a.3): a journal's
/// temporary file, a before-image's, and a staging folder whose journal is gone (its commit was
/// tidied, but not yet its staging). Only under the lock, when no commit of this process is
/// half made.
fn sweep(dir: &Path) {
    let txd = tx_dir(dir);
    let Ok(entries) = std::fs::read_dir(&txd) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let journal = |id: &str| txd.join(format!("{id}.json")).exists() || txd.join("aside").join(format!("{id}.json")).exists();
        if name.starts_with('.') && name.ends_with(".json.part") {
            let _ = std::fs::remove_file(e.path());
        } else if valid_journal(&name) && e.file_type().is_ok_and(|t| t.is_dir()) && !journal(&name) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    if let Ok(entries) = std::fs::read_dir(txd.join("blobs")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name.ends_with(".part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Where each target of `j` is, in the project at `dir`. A target at a newer journal's start or
/// end, that journal having started where this one ended, counts as at this one's end (R7): this
/// commit completed, and a newer one (its undo, say) went on from there.
pub fn states(dir: &Path, j: &Journal, newer: &[&Journal]) -> Vec<At> {
    j.targets
        .iter()
        .map(|t| match at(dir, t) {
            At::After => At::After,
            _ if shadowed(dir, t, newer) => At::After,
            s => s,
        })
        .collect()
}

fn target_paths(t: &JTarget) -> Vec<&str> {
    match t {
        JTarget::File { path, .. } => vec![path],
        JTarget::Rename { from, to, .. } => vec![from, to],
    }
}

/// Whether target `t` is where a newer journal that started at `t`'s end starts or ends: the same
/// file from `t`'s after hash, or the folder or file `t` renamed, moved on from where `t` left it.
fn shadowed(dir: &Path, t: &JTarget, newer: &[&Journal]) -> bool {
    match t {
        JTarget::File { path, after, .. } => {
            let now = file_sha(dir, path);
            newer.iter().flat_map(|j| &j.targets).any(|k| match k {
                JTarget::File { path: p, before, after: a, .. } => p == path && before == after && (now == *before || now == *a),
                JTarget::Rename { .. } => false,
            })
        }
        JTarget::Rename { to, hash, .. } => newer
            .iter()
            .flat_map(|j| &j.targets)
            .any(|k| matches!(k, JTarget::Rename { from, hash: h, .. } if from == to && h == hash) && at(dir, k) != At::Neither),
    }
}

/// Move journal `id` to `.music2/tx/aside/` (a Local file, never a project file).
fn set_aside(dir: &Path, id: &str) {
    let aside = tx_dir(dir).join("aside");
    let _ = std::fs::create_dir_all(&aside);
    let _ = std::fs::rename(tx_dir(dir).join(format!("{id}.json")), aside.join(format!("{id}.json")));
}

/// A recovery fix's Tx id: from the journal and the fix, so it is the same wherever it is read.
fn fix_id(jid: &str, code: &str) -> String {
    let sha = Sha::of(format!("tx.interrupted\0{jid}\0{code}").as_bytes());
    format!("fix-{}", &sha.hex()[..12])
}

fn path(p: &str) -> TxPath {
    TxPath::new(p).expect("a journal's paths are checked when it is read")
}

fn base(sha: Option<Sha>) -> Base {
    sha.map_or(Base::Absent, Base::Sha)
}

/// FinishTx: every target still at its before hash gets its staged bytes, is removed, or is
/// renamed. Offered only when every staged file a write needs is there with its after hash.
pub fn finish_tx(dir: &Path, j: &Journal, states: &[At]) -> Option<Tx> {
    let mut tx = Tx::new(&fix_id(&j.id, "finish-tx"), j.label.clone(), vec![], Expect::Journal(j.id.clone()));
    for (t, s) in j.targets.iter().zip(states) {
        if *s != At::Before {
            continue;
        }
        match t {
            JTarget::File { path: p, before, after: Some(after), staged: Some(k), .. } => {
                let local = format!(".music2/tx/{}/{k}", j.id);
                if Sha::of_file(&dir.join(&local)).ok() != Some(*after) {
                    return None;
                }
                tx.base.insert(path(p), base(*before));
                tx.ops.push(Op::Write { file: path(p), bytes: Payload::Local { path: local, sha: *after } });
            }
            JTarget::File { path: p, before: Some(before), after: None, .. } => {
                tx.base.insert(path(p), Base::Sha(*before));
                tx.ops.push(Op::Remove { file: path(p) });
            }
            JTarget::Rename { from, to, hash, .. } => {
                tx.base.insert(path(from), Base::Sha(*hash));
                tx.base.insert(path(to), Base::Absent);
                tx.ops.push(Op::Rename { from: path(from), to: path(to) });
            }
            JTarget::File { .. } => return None,
        }
    }
    Some(tx)
}

/// DiscardTx: every target at its after hash gets its before bytes back, is made again, is
/// removed (a file the commit created), or is renamed back. Always possible, since before-images
/// are durable before any rename.
pub fn discard_tx(dir: &Path, j: &Journal, states: &[At]) -> Option<Tx> {
    let mut tx = Tx::new(&fix_id(&j.id, "discard-tx"), (j.label.1.clone(), j.label.0.clone()), vec![], Expect::Journal(j.id.clone()));
    for (t, s) in j.targets.iter().zip(states) {
        if *s != At::After {
            continue;
        }
        match t {
            JTarget::File { path: p, before: None, after: Some(after), .. } => {
                tx.base.insert(path(p), Base::Sha(*after));
                tx.ops.push(Op::Remove { file: path(p) });
            }
            JTarget::File { path: p, before: Some(before), after, text, blob, .. } => {
                let bytes = match (text, blob) {
                    (Some(t), _) => Payload::Text(t.as_str().into()),
                    (None, true) => {
                        let local = format!(".music2/tx/blobs/{}", before.hex());
                        if Sha::of_file(&dir.join(&local)).ok() != Some(*before) {
                            return None;
                        }
                        Payload::Local { path: local, sha: *before }
                    }
                    (None, false) => return None,
                };
                tx.base.insert(path(p), base(*after));
                tx.ops.push(Op::Write { file: path(p), bytes });
            }
            JTarget::Rename { from, to, hash, .. } => {
                tx.base.insert(path(to), Base::Sha(*hash));
                tx.base.insert(path(from), Base::Absent);
                tx.ops.push(Op::Rename { from: path(to), to: path(from) });
            }
            JTarget::File { .. } => return None,
        }
    }
    Some(tx)
}

/// Whether `tx` is exactly the FinishTx or DiscardTx `journal` offers now (C3): what an
/// `Expect::Journal` Tx is checked against, never the graph.
pub fn restores(dir: &Path, j: &Journal, tx: &Tx) -> bool {
    // Judged as the scan that offered it judged the targets, newer journals included.
    let all = journals(dir);
    let newer: Vec<&Journal> =
        all.iter().filter(|(id, _)| order(id) > order(&j.id)).filter_map(|(_, x)| x.as_ref().ok()).collect();
    let states = states(dir, j, &newer);
    if states.contains(&At::Neither) {
        return false;
    }
    let same = |o: Option<Tx>| o.is_some_and(|o| o.ops == tx.ops && o.base == tx.base);
    same(finish_tx(dir, j, &states)) || same(discard_tx(dir, j, &states))
}

/// The bytes a `Payload::Local` names, checked against its hash (the step 6 design's C3).
pub fn local(dir: &Path, p: &str, sha: &Sha) -> Result<Arc<[u8]>, String> {
    if !m2_song::tx::valid_local(p) {
        return Err(format!("`{p}` isn't a file under .music2/tx/"));
    }
    let file = dir.join(p);
    if std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{p} is a link"));
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("{p} can't be read: {e}"))?;
    if Sha::of(&bytes) != *sha {
        return Err(format!("{p} isn't the content its hash says"));
    }
    Ok(bytes.into())
}

/// Each interrupted journal's targets, by path: what a Tx may not touch until it is finished or
/// discarded.
pub fn blocked(interrupted: &[Journal]) -> BTreeMap<PathBuf, String> {
    interrupted.iter().flat_map(|j| j.paths().into_iter().map(move |p| (PathBuf::from(p), j.id.clone()))).collect()
}

/// A stored hash, if it is one.
pub fn stored_sha(s: &Stored) -> Option<Sha> {
    match s {
        Stored::Hash(h) => Some(*h),
        _ => None,
    }
}
