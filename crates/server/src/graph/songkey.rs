//! The song's key (the polish plan's D2): a write of `layout.json` alone, through the session.

use anyhow::Result;
use m2_editor::songkey::SongKey;
use serde_json::Value;

use super::Graph;

impl Graph {
    /// Set the song's key, or with None take it out (`m2_editor::songkey::set`): one undo step, no
    /// revision (PLANE-3). Answers `{file, before, after, revision}`.
    pub fn song_key(&self, token: u64, key: Option<SongKey>) -> Result<Value> {
        self.edit(token, move |s| m2_editor::songkey::set(s, key))
    }
}
