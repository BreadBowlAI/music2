//! Lane D (next): turn an effect off and on, move it, and choose what a Duck listens to, through
//! `m2_editor::effect` on the actor, as every builder runs.

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

    /// Move effect call `call` before the effect it is written around (`earlier`) or after the
    /// one written around it. One Tx and one undo step; refused in words when the two aren't
    /// written one inside the other.
    pub fn move_effect(&self, token: u64, revision: u64, call: &str, earlier: bool) -> Result<Value> {
        let call = call.to_string();
        self.edit(token, move |s| m2_editor::effect::reorder(s, revision, &call, earlier))
    }

    /// Make Duck call `call` listen to track `track` (by key). One Tx and one undo step; refused
    /// in words when it can't hear that track from where it is written.
    pub fn listen_to(&self, token: u64, revision: u64, call: &str, track: &str) -> Result<Value> {
        let (call, track) = (call.to_string(), track.to_string());
        self.edit(token, move |s| m2_editor::effect::sidechain(s, revision, &call, &track))
    }
}
