//! Undo's names (gaps #29, WRITE-21; next lane C3): what Undo and Redo would take back, in words,
//! for every window, the app's Edit menu, and after a reload.
//!
//! A name lives on its undo step, in the session ([`m2_session::Session::name_step`]), by the
//! revision the change made: the window that made an edit names it ("Hats pan", `POST
//! /api/graph/step`), and the name follows the step through undo and redo. A step nobody named (an
//! agent's Tx, `m2`, a save in another editor) is named plainly by what it changed: "Bass notes",
//! "the tempo or time signature", "the code in tracks/bass.ts". The state says both tops as
//! `undo_name` and `redo_name` (`{name, structural, given}`, or null); an undo or a redo answers
//! with the name of what it took back. Names last while the song is open (gaps #29's second half
//! is left: they go when it closes).

use anyhow::{Context, Result, bail};
use m2_session::StepName;
use serde_json::{Value, json};

use super::{Graph, Inner, Timeline};

/// A step's name as the state says it: `{name, structural, given}`; null with no step. `given`:
/// a window named it; else it is the plain name of what it changed.
pub(super) fn named(step: Option<&StepName>, timeline: &Timeline) -> Value {
    let Some(step) = step else { return Value::Null };
    match &step.name {
        Some(name) => json!({"name": name, "structural": step.structural, "given": true}),
        None => json!({"name": plain(&step.file, timeline), "structural": false, "given": false}),
    }
}

/// What a change to `file` is called, by what the file holds: a lane by its track and kind, the
/// arrangement by what it sets, code by its file.
pub(super) fn plain(file: &str, timeline: &Timeline) -> String {
    if file == "data/arrangement.json" {
        return "the tempo or time signature".into();
    }
    if let Some(rest) = file.strip_prefix("data/")
        && let Some((module, lane)) = rest.split_once('/')
    {
        let key = lane.strip_suffix(".json").unwrap_or(lane);
        let row = timeline.tracks.iter().find(|t| t.module == module);
        let track = row.and_then(|t| t.head.get("label")).and_then(Value::as_str).unwrap_or(module);
        let lane = row.and_then(|t| t.lanes.iter().find(|l| l.key == key));
        return match lane.and_then(|l| l.json.get("kind")).and_then(Value::as_str) {
            Some("notes") => format!("{track} notes"),
            Some("control") => {
                let label = lane.and_then(|l| l.json.get("label")).and_then(Value::as_str).unwrap_or(key);
                format!("{track} {label} automation")
            }
            Some("audio") => format!("{track} audio clips"),
            _ => format!("the file {file}"),
        };
    }
    if file.ends_with(".ts") {
        return format!("the code in {file}");
    }
    match file {
        "music2.toml" => "the song's settings".into(),
        "layout.json" => "the track order, colours or markers".into(),
        _ if file.starts_with("assets/") => format!("the audio file {}", &file["assets/".len()..]),
        _ => format!("the file {file}"),
    }
}

/// The names of Undo's and Redo's steps, as last published.
fn shown(inner: &Inner) -> (Value, Value) {
    let (undo, redo, _) = shown_at(inner);
    (undo, redo)
}

/// [`shown`], and the revision they are the tops of (0 with no song open).
fn shown_at(inner: &Inner) -> (Value, Value, u64) {
    let cp = inner.control.lock();
    match cp.open.as_ref() {
        Some(p) => (p.shown.state["undo_name"].clone(), p.shown.state["redo_name"].clone(), p.shown.revision),
        None => (Value::Null, Value::Null, 0),
    }
}

impl Graph {
    /// Name the undo step of the edit this window made as `revision` (gaps #29): "Hats pan", and
    /// whether it adds or removes a part. Answers `{named, revision, undo_name, redo_name}`: the
    /// names after it, of the revision `revision`; `named` false when no step has that revision
    /// (nothing changed, or it is gone).
    pub fn name_step(&self, token: u64, revision: u64, name: String, structural: bool) -> Result<Value> {
        let name = name.trim().to_string();
        if name.is_empty() {
            bail!("a step's name can't be empty");
        }
        let named = self.call(move |a| {
            let o = a.open.as_mut().context("no project is open")?;
            if o.token != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            let named = o.session.name_step(revision, &name, structural);
            if named {
                a.republish();
            }
            Ok(named)
        })?;
        let (undo, redo, at) = shown_at(&self.inner);
        Ok(json!({"named": named, "revision": at, "undo_name": undo, "redo_name": redo}))
    }

    /// Undo's and Redo's names, as the app's Edit menu says them: None with nothing to undo or
    /// redo (or no song open).
    pub fn undo_names(&self) -> (Option<String>, Option<String>) {
        let (undo, redo) = shown(&self.inner);
        let name = |v: &Value| v["name"].as_str().map(String::from);
        (name(&undo), name(&redo))
    }

    /// Undo, or redo: the answer of any edit, and the name of what it took back (`name`,
    /// `structural`), read before it, in the same turn of the session.
    pub(super) fn step(&self, token: u64, redo: bool) -> Result<Value> {
        let inner = self.inner.clone();
        self.call(move |a| {
            let (undo, again) = shown(&inner);
            let took = if redo { again } else { undo };
            let mut v = if redo { a.edit(token, m2_editor::redo)? } else { a.edit(token, m2_editor::undo)? };
            v["name"] = took["name"].clone();
            v["structural"] = json!(took["structural"].as_bool().unwrap_or(false));
            Ok(v)
        })
    }
}

#[cfg(test)]
#[path = "steps_tests.rs"]
mod tests;
