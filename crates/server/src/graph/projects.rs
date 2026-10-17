//! Polish lane C's work on the open song that needs its session: the song's folder by its session,
//! and Save As's copy, made on the session's actor so no edit of the song is half way while it is
//! copied.

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::Graph;

impl Graph {
    /// The open song's folder, if `token` names it.
    pub fn dir_of(&self, token: u64) -> Result<PathBuf> {
        let cp = self.inner.control.lock();
        cp.open.as_ref().filter(|p| p.engine.token == token).map(|p| p.shown.dir.clone()).context("the open song changed; refresh first")
    }

    /// Copy the open song into the new folder `folder` in `parent`, named `name` (Save As, Q19):
    /// its files as they are on disk, without hidden ones (`.music2/`, `.git`). On the session's
    /// actor, between edits. Returns the copy's folder.
    pub fn copy_song(&self, token: u64, parent: PathBuf, folder: String, name: String) -> Result<PathBuf> {
        let from = self.dir_of(token)?;
        self.call(move |_| crate::project::copy_into(&from, &parent, &folder, &name))
    }
}
