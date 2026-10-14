//! The watcher of the open project (WRITE-19): outside writers (editors, agents, git) are taken in
//! once their changes settle, by core's settle rule ([`m2_session::watch::Settle`]), fed a stat walk
//! of the project every poll. A batch that settled is read once and taken in whole by the session's
//! actor; the session's own writes are told to the rule by their stamps, so they make no batch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use m2_session::Revision;
use m2_session::watch::{self, Listing, Poll, Settle, Stamp, Track};
use m2_song::{AssetName, DataPath, LaneKind, Sha};

/// The settle rule's state for one open project.
pub(crate) struct Watch {
    /// The open project's session token: a batch of a project since closed is dropped.
    pub token: u64,
    dir: PathBuf,
    git: Option<PathBuf>,
    /// The accepted revision whose assets `named` lists.
    number: u64,
    /// The assets some audio lane plays: tracked by stamp. Any other file in `assets/` is tracked
    /// only by being there, so a render growing there holds nothing back (R14).
    named: BTreeSet<PathBuf>,
    pub settle: Settle,
    /// What the rule said once to tell the user (`git.locked`).
    pub notice: Option<String>,
}

/// The assets an audio clip of `rev` plays, as project paths.
fn named(rev: &Revision) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    for (addr, _) in rev.compiled.decls.lanes.iter().filter(|(_, k)| *k == LaneKind::Audio) {
        for clip in rev.compiled.data.audio(addr.module.as_str(), addr.lane.as_str()) {
            if let Ok(name) = AssetName::new(&clip.asset) {
                out.insert(DataPath::Asset(name).path());
            }
        }
    }
    out
}

/// A stat walk taken before a project is read: what its watcher starts from, so a write that
/// lands while the session reads it is still seen as a change.
pub(crate) struct Before {
    listing: Listing,
    read_ns: i128,
}

impl Before {
    pub fn take(dir: &Path) -> Before {
        Before { listing: watch::listing(dir, &BTreeSet::new()), read_ns: watch::wall_ns() }
    }
}

impl Watch {
    /// The watcher of the project `rev` opened as `token`, from the walk taken before it was read.
    pub fn new(token: u64, rev: &Revision, before: Before) -> Watch {
        let dir = rev.project.dir.clone();
        let named = named(rev);
        let mut listing = before.listing;
        for p in &named {
            if let Some(e) = listing.entries.get_mut(p) {
                e.0 = Track::Binary;
            }
        }
        let settle = Settle::new(&listing, before.read_ns);
        Watch { token, git: watch::git_dir(&dir), dir, number: rev.number, named, settle, notice: None }
    }

    /// One poll: the paths of a batch that settled, if one did.
    pub fn poll(&mut self) -> Option<BTreeSet<PathBuf>> {
        let listing = watch::listing(&self.dir, &self.named);
        let dir = self.dir.clone();
        let mut hash = |p: &Path| std::fs::read(dir.join(p)).ok().map(|b| Sha::of(&b));
        let polled = self.settle.poll(Instant::now(), watch::wall_ns(), &listing, watch::git(self.git.as_deref()), &mut hash);
        if let Some(n) = self.settle.notice.take() {
            self.notice = Some(n.message);
        }
        match polled {
            Poll::Ready(paths) => Some(paths),
            Poll::Idle | Poll::Settling => None,
        }
    }

    /// The session wrote `paths` itself (R12), and accepted `rev`.
    pub fn wrote(&mut self, paths: &[(PathBuf, Option<Stamp>)], rev: &Revision) {
        if !paths.is_empty() {
            self.settle.wrote(paths, watch::wall_ns());
        }
        if rev.number != self.number {
            (self.number, self.named) = (rev.number, named(rev));
        }
    }
}
