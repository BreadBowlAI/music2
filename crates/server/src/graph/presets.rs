//! Presets (FMT-22, gaps #26): a sound card's list, Save as preset… and loading one. Yours are in
//! the Presets folder (`m2_os::dirs::presets`), std's in std's `presets` folder. Their files are
//! read and written on the caller's thread: the actor only reads the call, or commits the load's
//! one Tx.

use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use m2_editor::presets::{self, NotStored, Why};
use serde_json::{Value, json};

use super::Graph;

/// Your Presets folder.
fn yours() -> Result<PathBuf> {
    m2_os::dirs::presets().ok_or_else(|| anyhow!("can't find your home folder ($HOME isn't set)"))
}

/// The folder presets `from` (`std` or `yours`) for `sound` are in.
fn folder(from: &str, sound: &str) -> Result<PathBuf> {
    match from {
        "std" => presets::factory(&crate::resources::std_dir(), sound).ok_or_else(|| anyhow!("music2 has no presets of its own for this sound")),
        "yours" => Ok(yours()?.join(presets::folder(sound))),
        other => bail!("there are no presets from `{other}`"),
    }
}

/// Each control a preset left alone: `{label, why}`, `why` being `lane`, `code` or `units`.
fn named(controls: &[(String, Why)]) -> Value {
    controls.iter().map(|(label, why)| json!({"label": label, "why": why})).collect()
}

impl Graph {
    /// Run `f` on the open session when it is still `token`'s.
    fn with_session<T: Send + 'static>(&self, token: u64, f: impl FnOnce(&m2_session::Session) -> Result<T> + Send + 'static) -> Result<T> {
        self.call(move |a| match a.open.as_ref().filter(|o| o.token == token) {
            Some(o) => f(&o.session),
            None => bail!("the open project changed; refresh the graph before editing"),
        })
    }

    /// The sound call `node` (named at revision `at`, else the accepted one) is: its ComponentId
    /// and its label.
    fn preset_sound(&self, token: u64, at: Option<u64>, node: &str) -> Result<(String, String)> {
        let node = node.to_string();
        self.with_session(token, move |s| {
            let sound = presets::sound(s, at.unwrap_or(s.accepted().number), &node).map_err(|e| anyhow!("{e}"))?;
            Ok((sound.component, sound.label))
        })
    }

    /// The presets for call `node`: `{sound, label, std: [{name}], yours: [{name}], plugin: []}`.
    pub fn presets(&self, token: u64, at: Option<u64>, node: &str) -> Result<Value> {
        let (sound, label) = self.preset_sound(token, at, node)?;
        let names = |from: &str| -> Vec<Value> {
            let listed = folder(from, &sound).map(|d| presets::list(&d, &sound)).unwrap_or_default();
            listed.into_iter().map(|l| json!({"name": l.name})).collect()
        };
        Ok(json!({"sound": sound, "label": label, "std": names("std"), "yours": names("yours"), "plugin": []}))
    }

    /// Save call `node`'s settings as your preset `name`: `{name, left}`; `{taken}` when one of
    /// that name is there and `replace` isn't set. No song file changes.
    pub fn save_preset(&self, token: u64, at: Option<u64>, node: &str, name: &str, replace: bool) -> Result<Value> {
        let (node, name) = (node.to_string(), name.trim().to_string());
        let saved = self.with_session(token, move |s| {
            presets::capture(s, at.unwrap_or(s.accepted().number), &node, &name).map_err(|e| anyhow!("{e}"))
        })?;
        match presets::store(&folder("yours", &saved.preset.sound)?, &saved.preset, replace) {
            Ok(_) => Ok(json!({"name": saved.preset.name, "left": named(&saved.left)})),
            Err(NotStored::Taken(name)) => Ok(json!({"taken": name})),
            Err(NotStored::Refused(why)) => bail!(why),
        }
    }

    /// Load preset `name` from `from` (`std` or `yours`) into call `node`, named at revision `at`:
    /// one Tx. Answers as an edit does, with `changed` and the controls it left alone (`kept`).
    pub fn load_preset(&self, token: u64, at: u64, node: &str, from: &str, name: &str) -> Result<Value> {
        let (sound, _) = self.preset_sound(token, Some(at), node)?;
        let preset = presets::read(&folder(from, &sound)?, &sound, name).map_err(|e| anyhow!(e))?;
        let node = node.to_string();
        self.call(move |a| {
            a.edit_and(token, |s| {
                let loaded = presets::load(s, at, &node, &preset)?;
                let changed = loaded.edited.before != loaded.edited.after;
                Ok((loaded.edited, json!({"changed": changed, "kept": named(&loaded.kept)})))
            })
        })
    }
}
