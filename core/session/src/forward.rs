//! Forwarding (AGENT-1): a read-only session whose commits go to the lock's holder, the app that
//! has the song open. Each Tx is checked here first, under its own expectation and its planner's
//! assertion, and sent with the hash of that check; the holder commits it only while its delta has
//! that hash. Undo and redo are the holder's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use m2_song::tx::{Tx, TxPath};
use m2_song::{DataPath, Sha};

use crate::session::Session;
use crate::tx::Refusal;

/// The lock's holder, which takes a read-only session's commits (AGENT-1).
pub trait Forward: Send {
    /// Commit `sent.tx` in the holder, only if its delta's hash is `sent.seen`.
    fn commit(&mut self, sent: &Sent) -> Result<Landed, Refusal>;
}

/// A Tx checked here, as it is sent to the holder.
pub struct Sent<'a> {
    pub tx: &'a Tx,
    /// The hash of the delta this session's check found.
    pub seen: Sha,
    /// The new files under `assets/` it names: the holder takes them in first.
    pub assets: Vec<TxPath>,
    /// What each file it touches holds once it landed, by this check (None: nothing): how a
    /// sender that lost the holder on the way tells whether it landed.
    pub after: BTreeMap<TxPath, Option<Sha>>,
}

/// Where a forwarded Tx landed: the holder's undo step, and its revision's numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Landed {
    pub step: u64,
    pub number: u64,
    pub code_rev: u64,
    pub data_rev: u64,
}

impl Session {
    /// Hand this session's commits to `to`, the lock's holder (AGENT-1). Only a read-only
    /// session forwards: a writer commits itself.
    pub fn forward_to(&mut self, to: Box<dyn Forward>) {
        if !self.access.writer() {
            self.forward = Some(to);
        }
    }

    /// Whether this session's commits go to the lock's holder.
    pub fn forwards(&self) -> bool {
        self.forward.is_some()
    }

    /// Where the last forwarded Tx landed.
    pub fn landed(&self) -> Option<Landed> {
        self.landed
    }
}

/// Every file a landed Tx may have changed, as the project names them: its targets, each file of
/// a folder it touched (as the session knew it, and as the disk has it now), and `assets`.
pub(crate) fn landed_paths(s: &Session, tx: &Tx, assets: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let snap = &s.accepted().snapshot;
    let mut out: BTreeSet<PathBuf> = assets.clone();
    for p in tx.touched() {
        let path = p.path();
        if matches!(p.data(), DataPath::Module { .. }) {
            out.extend(m2_song::under(&snap.files, &path).map(|(k, _)| k.clone()));
            out.extend(m2_song::under(&snap.binaries, &path).map(|(k, _)| k.clone()));
            files_under(&s.dir, &path, &mut out);
        } else {
            out.insert(path);
        }
    }
    out
}

/// Every file under `rel` in the project at `dir`, by its path in the project.
fn files_under(dir: &Path, rel: &Path, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir.join(rel)) else { return };
    for e in entries.flatten() {
        let child = rel.join(e.file_name());
        match e.file_type() {
            Ok(t) if t.is_dir() => files_under(dir, &child, out),
            Ok(_) => {
                out.insert(child);
            }
            Err(_) => {}
        }
    }
}
