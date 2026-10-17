//! Lane D (next): turn an effect off and on, through `m2_editor::effect` on the actor, as every
//! builder runs.

use anyhow::Result;
use serde_json::Value;

use super::Graph;

impl Graph {
    /// Turn effect call `call` (`file#ordinal`) off (`on`: `bypass: true` in its options) or on
    /// again. One Tx and one undo step; refused in words when it can't be done here.
    pub fn bypass(&self, token: u64, revision: u64, call: &str, on: bool) -> Result<Value> {
        let call = call.to_string();
        self.edit(token, move |s| m2_editor::effect::bypass(s, revision, &call, on))
    }
}
