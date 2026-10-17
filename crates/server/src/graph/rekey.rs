//! Change a track's ID (polish A5, design-8b §11): `m2_editor::rekey` on the actor, as every
//! builder runs, and the check the dialog makes as you type.

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use super::Graph;

impl Graph {
    /// Change track `module`'s ID to `key`: its `track()` call's key, its channel's when that is the
    /// same, its data folder and its layout entry. One Tx and one undo step.
    pub fn rekey(&self, token: u64, module: &str, key: &str) -> Result<Value> {
        let (module, key) = (module.to_string(), key.to_string());
        self.edit(token, move |s| m2_editor::rekey::rekey(s, &module, &key))
    }

    /// Whether `key` can be track `module`'s new ID: `{ok: true}`, or `{ok: false, error}` in ID
    /// words ("low is taken: another track or its data has that ID"). Reads only.
    pub fn rekey_check(&self, token: u64, module: &str, key: &str) -> Result<Value> {
        let (module, key) = (module.to_string(), key.to_string());
        self.call(move |a| {
            let o = a.open.as_ref().ok_or_else(|| anyhow!("no project is open"))?;
            if o.token != token {
                return Err(anyhow!("the open project changed; refresh the graph before editing"));
            }
            Ok(match m2_editor::rekey::check(&o.session, &module, &key) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e.to_string().trim_start_matches("edit rejected: ")}),
            })
        })
    }
}
