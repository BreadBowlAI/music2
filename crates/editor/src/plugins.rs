//! Plugins: an installed CLAP plugin put after a call's audio, or played on a new track (gaps #32).
//!
//! A plugin is named by its graph id, `clap:<id>`, as its code writes it and as the plugin list
//! gives it (`GET /api/plugins`). Before planning it is described through the session's host
//! (`PackageSet::describe_plugins`: in the host process, HOST-3), so one that isn't installed, or
//! is of the wrong kind, is refused in words and nothing is written.
//!
//! - **After a sound** (the effect browser): the call is wrapped where it is written, as a std
//!   effect wraps it ([`crate::effect::wrap_call`]): `kick(x)` becomes
//!   `plugin("clap:m2.probe", { in: kick(x), key: "m2-probe" })`, two insertions around it (R5,
//!   WRITE-8), with `plugin` imported from `@m2/core`, one `Confined` Tx. The key is always written,
//!   so the plugin's settings can be saved (HOME-6): its name as a key, numbered while another
//!   plugin in its frame has that key. The planner asserts that the candidate has the plugin
//!   installed, that the new call made exactly one node, that plugin with that key, and that it
//!   reads what the old call made where the readers did (WRITE-9).
//! - **A new track** (+ Track): [`crate::tracks::add_tracks`] writes a file playing it from a notes
//!   lane, keyed as the track, as `examples/with-plugins` does:
//!   `plugin("clap:<id>", { notes: lane.notes("notes"), key: "<track key>" })`.

use std::path::Path;

use m2_ir::{Op, split_ref};
use m2_session::{Checked, Compiled, Session};

use crate::cable::current_site;
use crate::controls::walk;
use crate::effect::{Wrap, made_by, wrap_call};
use crate::sites::import_insertion;
use crate::tracks::quoted;
use crate::{EditError, Edited, keys, ready};

/// What an installed plugin is, as adding it needs to know.
struct Described {
    name: String,
    /// It takes audio in, and notes.
    audio_in: bool,
    note_in: bool,
    /// Its CLAP features say instrument, and not effect.
    instrument_only: bool,
}

/// The installed plugin `component` (`clap:<id>`), described through the host: its ports and what
/// it is. Refused in words when it isn't installed, or doesn't load.
fn described(s: &mut Session, component: &str) -> Result<Described, EditError> {
    let why = s.packages.describe_plugins([component]).pop();
    match (s.packages.plugin_info(component), why) {
        (Some(info), None) => Ok(Described {
            name: info.found.name.clone(),
            audio_in: info.audio_in.is_some(),
            note_in: info.note_in,
            instrument_only: info.found.is_instrument() && !info.found.is_effect(),
        }),
        (_, why) => Err(not_installed(component, why)),
    }
}

/// Why plugin `component` can't be added: it isn't installed (removed since the list was made), or
/// its library doesn't load (`why`).
fn not_installed(component: &str, why: Option<String>) -> EditError {
    let id = component.strip_prefix("clap:").unwrap_or(component);
    EditError::Missing(match why {
        Some(why) if !why.starts_with("no CLAP plugin") => format!("{id} doesn't load on this computer ({why}), so it can't be added."),
        _ => format!("{id} isn't installed on this computer, so it can't be added. Install it, then choose Look again."),
    })
}

/// The planner's check that the candidate plays plugin `component`: it has no `plugin.missing`
/// its baseline didn't have.
pub(crate) fn installed_in(checked: &Checked, component: &str) -> Result<(), EditError> {
    let missing = |c: &Compiled| c.problems.iter().filter(|d| d.code == "plugin.missing").count();
    match missing(&checked.candidate().compiled) > missing(&checked.baseline().compiled) {
        true => Err(not_installed(component, None)),
        false => Ok(()),
    }
}

/// What adding plugin `id` after a call writes around it: before it and after it.
pub(crate) fn effect_text(id: &str, key: &str) -> (String, String) {
    (format!("plugin({}, {{ in: ", quoted(id)), format!(", key: {} }})", quoted(key)))
}

/// The `track(…)` call a new track playing instrument plugin `id` makes: from its notes lane, keyed
/// as the track. Also checked as a call form (PKG-11, `planned_call_forms_are_current`).
pub(crate) fn track_call_text(id: &str, key: &str, label: &str) -> String {
    let options = if label == key { String::new() } else { format!(", {{ name: {} }}", quoted(label)) };
    format!(
        "track({}{options}, ({{ lane }}) => plugin({}, {{ notes: lane.notes(\"notes\"), key: {} }}))",
        quoted(key),
        quoted(id),
        quoted(key)
    )
}

/// The text of a new track file playing instrument plugin `id`.
pub(crate) fn track_text(id: &str, key: &str, label: &str) -> String {
    format!("import {{ plugin, track }} from \"@m2/core\";\n\nexport default {};\n", track_call_text(id, key, label))
}

/// The name of instrument plugin `component`, which a new track plays; or why it can't, in words.
pub(crate) fn instrument(s: &mut Session, component: &str) -> Result<String, EditError> {
    let info = described(s, component)?;
    let name = info.name;
    match info.note_in {
        true => Ok(name),
        false => Err(EditError::Rejected(format!(
            "{name} doesn't play notes, so it can't be a track's instrument. Drop it on a track to put it after the sound."
        ))),
    }
}

/// The key a plugin added after call `site` gets (ID-2): its name as a key, numbered while another
/// plugin in the same module and `key()` frames has it, in any capitals (the build refuses two,
/// `key.duplicate`, and their saved settings would share one file).
fn free_key(s: &Session, site: u32, name: &str) -> String {
    let c = &s.accepted().compiled;
    // Where the call's output is made: its module, and the `key()` frames around it.
    let place = c.outputs.iter().find(|o| o.site == site).and_then(|o| {
        let m = c.ir.modules.iter().find(|m| m.key == o.module)?;
        let mut frame = None;
        walk(&m.nodes, &mut |n| {
            if n.id == split_ref(&o.port).0 {
                frame = Some(n.frame);
            }
        });
        let frames: Vec<String> = m.chain(frame?).into_iter().filter_map(|f| f.key.clone()).collect();
        Some((m, frames))
    });
    let mut taken: Vec<String> = vec![];
    if let Some((m, frames)) = place {
        walk(&m.nodes, &mut |n| {
            if let Op::Plugin { key: Some(k), frames: f, .. } = &n.op
                && *f == frames
            {
                taken.push(k.to_lowercase());
            }
        });
    }
    let is_taken = |k: &str| taken.contains(&k.to_lowercase());
    match keys::propose(name, is_taken) {
        k if k.is_empty() => keys::propose("plugin", is_taken),
        k => k,
    }
}

/// Whether call `site` of `c` made exactly one node: plugin `id`, keyed `key`.
fn one_plugin(c: &Compiled, site: u32, id: &str, key: &str) -> bool {
    let Some((module, ids)) = made_by(c, site) else { return false };
    let Some(m) = c.ir.modules.iter().find(|m| m.key == module) else { return false };
    let mut made = vec![];
    walk(&m.nodes, &mut |n| {
        if ids.contains(&n.id) {
            made.push(n.op.clone());
        }
    });
    matches!(&made[..], [Op::Plugin { plugin, key: Some(k), .. }] if plugin == id && k == key)
}

/// Put installed plugin `component` (`clap:<id>`) after call `call` (`file#ordinal` at revision
/// `at`): an effect, or a plugin that is both. One that only plays notes is refused, pointing at
/// + Track.
pub(crate) fn add_effect(s: &mut Session, at: u64, call: &str, component: &str) -> Result<Edited, EditError> {
    ready(s)?;
    let site = current_site(s, at, call)?.clone();
    if !site.in_project {
        return Err(EditError::NotFound(call.to_string()));
    }
    let info = described(s, component)?;
    let name = info.name;
    if !info.audio_in || info.instrument_only {
        return Err(EditError::Rejected(match info.note_in {
            true => format!("{name} plays notes; it can't go after a sound. Add it with + Track."),
            false => format!("{name} takes no sound in, so it can't go after one."),
        }));
    }
    let key = free_key(s, site.id, &name);
    let text = s.accepted().snapshot.files.get(Path::new(&site.file)).ok_or_else(|| EditError::NotFound(site.file.clone()))?;
    let imports = import_insertion(text, "@m2/core", "plugin").into_iter().collect();
    let (open, close) = effect_text(component, &key);
    let wrap = Wrap { open, close, imports, callee: "plugin".into(), label: name.clone(), also: Default::default() };
    wrap_call(s, &site, wrap, |_, checked, new| {
        installed_in(checked, component)?;
        match one_plugin(&checked.candidate().compiled, new, component, &key) {
            true => Ok(()),
            false => Err(EditError::Rejected(format!("adding {name} here would change more than putting it after this sound"))),
        }
    })
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
mod tests;
