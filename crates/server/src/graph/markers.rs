//! The ruler's markers (Q13): the whole list written into `layout.json`, one undo step with no
//! revision (PLANE-3), as a reorder is.

use anyhow::Result;
use serde_json::Value;

use super::Graph;
use m2_editor::markers::Marker;

impl Graph {
    /// Set the song's markers to `markers` (`m2_editor::markers::set`). Answers `{file, before,
    /// after, revision}`.
    pub fn markers(&self, token: u64, markers: Vec<Marker>) -> Result<Value> {
        self.edit(token, move |s| m2_editor::markers::set(s, markers))
    }
}
