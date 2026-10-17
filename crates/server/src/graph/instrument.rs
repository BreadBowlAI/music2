//! Change a track's instrument (polish A3): `m2_editor::swap` on the actor, as every builder runs.

use anyhow::Result;
use serde_json::Value;

use super::Graph;

impl Graph {
    /// Change track `module`'s instrument to `component` (`std:synth-pad`): its one instrument call
    /// becomes a call of that one, reading the same notes, at its own settings. One Tx and one undo
    /// step, labelled "Bass plays Pad". Refused in words when the track has no instrument call of
    /// its own, two, a plugin, or a setting the code computes.
    pub fn swap_instrument(&self, token: u64, module: &str, component: &str) -> Result<Value> {
        let (module, component) = (module.to_string(), component.to_string());
        self.edit(token, move |s| m2_editor::swap::swap(s, &module, &component))
    }
}
