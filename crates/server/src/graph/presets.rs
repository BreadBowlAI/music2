//! Presets (FMT-22, HOST-8, gaps #26): a sound card's list, Save as preset… and loading one. Yours
//! are in the Presets folder (`m2_os::dirs::presets`), std's in std's `presets` folder. Their
//! files are read and written on the caller's thread: the actor only reads the call, or commits
//! the load's one Tx.
//!
//! A plugin's card lists yours and the plugin's own (`PluginHost::presets`, kept per plugin for the
//! app run). Saving one asks its live instance for its state. Loading one of the plugin's own
//! makes an instance for it in the host process, loads the preset there (CLAP's preset-load),
//! saves its state and reads the parameters its code writes, then destroys it: the instance that
//! plays changes only once the load's Tx commits, through the usual state load (RENDER-26b).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use m2_editor::presets::{self, NotStored, Of, Preset, Why};
use m2_plugins::{Found, MainCall, Offered, Owner, Reply};
use serde_json::{Value, json};

use super::Graph;

/// How long a plugin may take to save its state for a preset.
const SAVE_WAIT: Duration = Duration::from_secs(60);

/// The folder presets `from` (`std`: the package's own, std's or a pack's, in `own`, its folder;
/// or `yours`, in your Presets folder `yours`) for `sound` are in.
fn folder(yours: Option<&std::path::Path>, from: &str, sound: &str, own: Option<&std::path::Path>) -> Result<PathBuf> {
    let yours = || yours.map(std::path::Path::to_path_buf).ok_or_else(|| anyhow!("can't find your home folder ($HOME isn't set)"));
    match from {
        "std" => {
            let dir = own.map_or_else(crate::resources::std_dir, std::path::Path::to_path_buf);
            presets::factory(&dir, sound).ok_or_else(|| anyhow!("music2 has no presets of its own for this sound"))
        }
        "yours" => Ok(yours()?.join(presets::folder(sound))),
        other => bail!("there are no presets from `{other}`"),
    }
}

/// Each control a preset left alone: `{label, why}`, `why` being `lane`, `code` or `units`.
fn named(controls: &[(String, Why)]) -> Value {
    controls.iter().map(|(label, why)| json!({"label": label, "why": why})).collect()
}

/// The names of `sound`'s presets from `from`.
fn names(yours: Option<&std::path::Path>, from: &str, sound: &str, own: Option<&std::path::Path>) -> Vec<Value> {
    let listed = folder(yours, from, sound, own).map(|d| presets::list(&d, sound)).unwrap_or_default();
    listed.into_iter().map(|l| json!({"name": l.name})).collect()
}

/// A call to presets, and the plugin as its host found it (a plugin's, when installed).
type Call = (Of, Option<Found>);

impl Graph {
    /// Run `f` on the open session when it is still `token`'s.
    fn with_session<T: Send + 'static>(&self, token: u64, f: impl FnOnce(&m2_session::Session) -> Result<T> + Send + 'static) -> Result<T> {
        self.call(move |a| match a.open.as_ref().filter(|o| o.token == token) {
            Some(o) => f(&o.session),
            None => bail!("the open project changed; refresh the graph before editing"),
        })
    }

    /// The folder of the package that makes `sound`, as the open session has it (PACK-11).
    fn package_dir(&self, token: u64, sound: &str) -> Option<PathBuf> {
        let sound = sound.to_string();
        self.with_session(token, move |s| Ok(presets::package_dir(s, &sound))).ok().flatten()
    }

    /// What call `node` (named at revision `at`, else the accepted one) is to presets.
    fn preset_call(&self, token: u64, at: Option<u64>, node: &str) -> Result<Call> {
        let node = node.to_string();
        self.with_session(token, move |s| {
            let of = presets::of(s, at.unwrap_or(s.accepted().number), &node).map_err(|e| anyhow!("{e}"))?;
            let found = match &of {
                Of::Plugin(p) => s.packages.plugin_info(&p.plugin).map(|i| i.found.clone()),
                Of::Sound(_) => None,
            };
            Ok((of, found))
        })
    }

    /// The presets `found` offers itself, found once per app run (HOST-8); why not, in words.
    fn offered(&self, found: &Found) -> Result<Vec<Offered>, String> {
        if let Some(r) = self.inner.offered.lock().get(&found.id) {
            return r.clone();
        }
        let r = self.inner.host.presets(found).map_err(|e| format!("{e:#}"));
        self.inner.offered.lock().insert(found.id.clone(), r.clone());
        r
    }

    /// The presets for call `node`: `{sound, label, std: [{name}], yours: [{name}], plugin:
    /// [{name}]}`, and `plugin_error` when the plugin's own couldn't be read.
    pub fn presets(&self, token: u64, at: Option<u64>, node: &str) -> Result<Value> {
        let (of, found) = self.preset_call(token, at, node)?;
        let (sound, label) = of.named();
        let own = self.package_dir(token, &sound);
        let mut out = json!({"sound": sound, "label": label, "std": [], "yours": names(self.presets_dir().as_deref(), "yours", &sound, None), "plugin": []});
        match (&of, found) {
            (Of::Sound(_), _) => out["std"] = names(None, "std", &sound, own.as_deref()).into(),
            (Of::Plugin(_), Some(found)) => match self.offered(&found) {
                Ok(offered) => out["plugin"] = offered.iter().map(|o| json!({"name": o.name})).collect(),
                Err(e) => out["plugin_error"] = format!("Couldn't read {label}'s presets: {e}").into(),
            },
            (Of::Plugin(_), None) => {}
        }
        Ok(out)
    }

    /// Save call `node`'s settings as your preset `name`: `{name, left}`; `{taken}` when one of
    /// that name is there and `replace` isn't set. No song file changes. A plugin's state comes
    /// from its live instance.
    pub fn save_preset(&self, token: u64, at: Option<u64>, node: &str, name: &str, replace: bool) -> Result<Value> {
        let (node, name) = (node.to_string(), name.trim().to_string());
        let saved = match self.preset_call(token, at, &node)? {
            (Of::Sound(_), _) => self.with_session(token, move |s| {
                presets::capture(s, at.unwrap_or(s.accepted().number), &node, &name).map_err(|e| anyhow!("{e}"))
            })?,
            (Of::Plugin(p), _) => {
                // Refused before the plugin is asked for its state.
                presets::capture_plugin(&p, &name, &[]).map_err(|e| anyhow!("{e}"))?;
                let flat = p.flat.clone();
                let (inst, _) = self.call(move |a| a.plugin_at(token, &flat))?;
                let blob = inst
                    .host()
                    .save_state(inst.handle())
                    .recv_timeout(SAVE_WAIT)
                    .map_err(|_| anyhow!("{} didn't answer", p.label))?
                    .map_err(|e| anyhow!("{} couldn't save its settings: {e}", p.label))?;
                presets::capture_plugin(&p, &name, &blob.bytes).map_err(|e| anyhow!("{e}"))?
            }
        };
        match presets::store(&folder(self.presets_dir().as_deref(), "yours", &saved.preset.sound, None)?, &saved.preset, replace) {
            Ok(_) => Ok(json!({"name": saved.preset.name, "left": named(&saved.left)})),
            Err(NotStored::Taken(name)) => Ok(json!({"taken": name})),
            Err(NotStored::Refused(why)) => bail!(why),
        }
    }

    /// Load preset `name` from `from` (`std`, `yours`, or `plugin`: the plugin's own) into call
    /// `node`, named at revision `at`: one Tx. Answers as an edit does, with `changed` and the
    /// controls it left alone (`kept`).
    pub fn load_preset(&self, token: u64, at: u64, node: &str, from: &str, name: &str) -> Result<Value> {
        let node = node.to_string();
        let answer = |loaded: presets::Loaded| {
            let changed = loaded.edited.before != loaded.edited.after;
            Ok((loaded.edited, json!({"changed": changed, "kept": named(&loaded.kept)})))
        };
        match self.preset_call(token, Some(at), &node)? {
            (Of::Sound(x), _) => {
                let own = self.package_dir(token, &x.component);
                let preset = presets::read(&folder(self.presets_dir().as_deref(), from, &x.component, own.as_deref())?, &x.component, name)
                    .map_err(|e| anyhow!(e))?;
                self.call(move |a| a.edit_and(token, |s| answer(presets::load(s, at, &node, &preset)?)))
            }
            (Of::Plugin(p), found) => {
                p.keyed().map_err(|e| anyhow!("{e}"))?;
                let (preset, blob) = match from {
                    "plugin" => {
                        let found = found
                            .ok_or_else(|| anyhow!("{} isn't installed on this computer, so its presets can't be loaded.", p.label))?;
                        self.scratch(&found, &p, name)?
                    }
                    _ => {
                        let preset = presets::read(&folder(self.presets_dir().as_deref(), from, &p.plugin, None)?, &p.plugin, name)
                            .map_err(|e| anyhow!(e))?;
                        let blob = preset.blob().map_err(|e| anyhow!(e))?;
                        (preset, blob)
                    }
                };
                self.call(move |a| a.edit_and(token, |s| answer(presets::load_plugin(s, at, &node, &preset, blob)?)))
            }
        }
    }

    /// The plugin's own preset `name` as a preset of yours would hold it (HOST-8): loaded on an
    /// instance made for it in the plugin's host, whose state is saved and whose code-owned
    /// parameters are read; then the instance is destroyed.
    fn scratch(&self, found: &Found, p: &presets::PluginCall, name: &str) -> Result<(Preset, Vec<u8>)> {
        let offered = self.offered(found).map_err(|e| anyhow!("Couldn't read {}'s presets: {e}", p.label))?;
        let o = offered.into_iter().find(|o| o.name == name).ok_or_else(|| anyhow!("{} has no preset called {name} any more.", p.label))?;
        let host = self.inner.host.clone();
        let (h, _) = host.create(found, None, &Owner::discard())?;
        let made = (|| -> Result<(Vec<u8>, BTreeMap<String, f64>)> {
            host.main_call(h, MainCall::LoadPreset { location: o.location, key: o.key })?;
            let blob = host.save_state(h).recv_timeout(SAVE_WAIT).map_err(|_| anyhow!("it didn't answer"))?.map_err(|e| anyhow!(e))?;
            let mut values = BTreeMap::new();
            for id in p.owned() {
                if let Reply::Value(Some(v)) = host.main_call(h, MainCall::Value(id))? {
                    values.insert(id.to_string(), v);
                }
            }
            Ok((blob.bytes, values))
        })();
        host.destroy(h);
        let (blob, values) = made.map_err(|e| anyhow!("{} couldn't load {name}: {e:#}", p.label))?;
        let preset =
            Preset { name: name.into(), sound: p.plugin.clone(), values, plugin_version: Some(found.version.clone()), state: None };
        Ok((preset, blob))
    }
}

#[cfg(test)]
#[path = "presets_tests.rs"]
mod tests;
