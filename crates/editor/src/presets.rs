//! Presets (FMT-22, gaps #26): a sound's settings under a name, one JSON file each. Yours are in
//! the Presets folder (`m2_os::dirs::presets()`), one folder per sound; std's ship in
//! `std/presets/<component>/`. A preset's identity is its `sound` and its `name`, never its folder
//! (P1): a file whose `sound` isn't the call's is left out of its list.
//!
//! Saving writes a file outside the song: no Tx. Loading is one `Values` Tx, as a knob's edit is
//! (WRITE-10): each number written in the call is rewritten through its lens, keeping its
//! spelling (`khz(0.9)`), and each control the call leaves out is written into it (PKG-3), several
//! as one insertion. A control a lane or other code drives keeps what drives it, and is named.
//!
//! A plugin's preset (HOST-8) is its state and the parameters its code writes as numbers. Saving
//! takes the live instance's state, which the session asks the host for. Loading is one
//! `Confined` Tx: the state and its sidecar (FMT-17, as `data::plugin_state` writes them), and
//! each parameter its code writes rewritten to the preset's value. The usual state load then
//! reaches the instance that plays (RENDER-26b). A parameter a lane or other code drives keeps
//! it, and is named; one the code doesn't give is the state's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_ir::{Op, split_ref};
use m2_script::Descriptor;
use m2_script::component::ControlDesc;
use m2_script::sites::{Arg, Site};
use m2_session::Session;
use m2_session::text::Edits;
use m2_song::tx::{Anchor, Confined, Declared, Expect, Range, Spot};
use m2_song::{Key, Sidecar};
use serde_json::Value;

use crate::controls::{self, Control, Edit, options_of, walk};
use crate::effect::calls_of;
use crate::lens::{self, Insertion};
use crate::plan::{self, path};
use crate::{EditError, Edited, ready};

/// The format's version, `m2preset`: a newer one is refused in words.
pub const FORMAT: u64 = 1;

/// A preset as its file holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: String,
    /// What it is for: a ComponentId (`std:synth-pluck`) or a plugin's id (`clap:…`).
    pub sound: String,
    /// Numbers by name: a std control's slot, in base units; a plugin parameter by its CLAP id.
    pub values: BTreeMap<String, f64>,
    /// A plugin's only: its version, and its state as base64.
    pub plugin_version: Option<String>,
    pub state: Option<String>,
}

const NOT: &str = "This file isn't a music2 preset.";

impl Preset {
    /// Read a preset file's text, or why it can't be one, in words.
    pub fn parse(text: &str) -> Result<Preset, String> {
        let v: Value = serde_json::from_str(text).map_err(|_| NOT.to_string())?;
        let o = v.as_object().ok_or(NOT)?;
        let version = o.get("m2preset").and_then(Value::as_u64).ok_or(NOT)?;
        if version > FORMAT {
            return Err("This preset was saved by a newer music2. Update music2 to load it.".into());
        }
        let name = o.get("name").and_then(Value::as_str).ok_or(NOT)?;
        if let Some(why) = name_error(name) {
            return Err(format!("This preset's name won't do: {why}."));
        }
        let sound = o.get("sound").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or(NOT)?;
        let mut values = BTreeMap::new();
        for (k, v) in o.get("values").map(|v| v.as_object().ok_or(NOT)).transpose()?.into_iter().flatten() {
            let n = v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| format!("This preset's {k} isn't a number."))?;
            values.insert(k.clone(), n);
        }
        let text = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
        Ok(Preset { name: name.into(), sound: sound.into(), values, plugin_version: text("plugin_version"), state: text("state") })
    }

    /// A plugin preset's state, as bytes; why not in words.
    pub fn blob(&self) -> Result<Vec<u8>, String> {
        let bad = || format!("{}'s saved plugin state can't be read.", self.name);
        m2_song::tx::unbase64(self.state.as_deref().ok_or_else(bad)?).ok_or_else(bad)
    }

    /// The file's text: two-space JSON, keys in the format's order, values by name.
    pub fn text(&self) -> String {
        let s = |x: &str| serde_json::to_string(x).expect("a string is JSON");
        let mut out = format!("{{\n  \"m2preset\": {FORMAT},\n  \"name\": {},\n  \"sound\": {},\n  \"values\": {{", s(&self.name), s(&self.sound));
        for (i, (k, v)) in self.values.iter().enumerate() {
            out += if i == 0 { "\n" } else { ",\n" };
            // Rust writes the shortest form that reads back as the same number: `4500`, `0.25`.
            out += &format!("    {}: {v}", s(k));
        }
        out += if self.values.is_empty() { "}" } else { "\n  }" };
        if let Some(v) = &self.plugin_version {
            out += &format!(",\n  \"plugin_version\": {}", s(v));
        }
        if let Some(state) = &self.state {
            out += &format!(",\n  \"state\": {}", s(state));
        }
        out + "\n}\n"
    }
}

/// Why `name` can't be a preset's, or None: one line of 1 to 40 characters, none of `/ \ : * ? "
/// < > |`, not starting with a dot, and no blank at either end or dot at the end (so it is a
/// file name on every OS).
pub fn name_error(name: &str) -> Option<&'static str> {
    let n = name.chars().count();
    if n == 0 || n > 40 || name.chars().any(char::is_control) {
        return Some("a preset's name is 1 to 40 characters on one line");
    }
    if name.chars().any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Some("a preset's name can't have / \\ : * ? \" < > or |");
    }
    if name.starts_with('.') {
        return Some("a preset's name can't start with a dot");
    }
    if name.trim() != name || name.ends_with('.') {
        return Some("a preset's name can't start or end with a space, or end with a dot");
    }
    None
}

/// A folder name for `part` of an id: every character outside `A–Z a–z 0–9 . _ -` turned to `_`,
/// and never `.` or `..`.
fn folder_name(part: &str) -> String {
    let name: String = part.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect();
    match name.trim_matches('.').is_empty() {
        true => name.replace('.', "_") + "_",
        false => name,
    }
}

/// Where your presets for `sound` go inside the Presets folder: `std/synth-pluck` for
/// `std:synth-pluck`, `clap/org.surge-synth-team.surge-xt` for a plugin. A project's own
/// component, which has no package, goes in `project`.
pub fn folder(sound: &str) -> PathBuf {
    let (package, name) = sound.split_once(':').unwrap_or(("project", sound));
    Path::new(&folder_name(package)).join(folder_name(name))
}

/// Where std's own presets for `sound` are, in std's folder `std`: `presets/synth-pluck`. None for
/// a sound std doesn't make.
pub fn factory(std: &Path, sound: &str) -> Option<PathBuf> {
    Some(std.join("presets").join(folder_name(sound.strip_prefix("std:")?)))
}

/// A preset in a folder: its name and its file.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub name: String,
    pub file: PathBuf,
}

/// The presets for `sound` in `dir`, by name (in any capitals; the first of two that differ only
/// in capitals). A file that isn't a preset, or is another sound's, is left out.
pub fn list(dir: &Path, sound: &str) -> Vec<Listed> {
    let mut out: Vec<Listed> = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let file = e.path();
        if file.extension().is_none_or(|x| x != "json") {
            continue;
        }
        match std::fs::read_to_string(&file).ok().and_then(|t| Preset::parse(&t).ok()) {
            Some(p) if p.sound == sound => out.push(Listed { name: p.name, file }),
            _ => {}
        }
    }
    out.sort_by_key(|l| (l.name.to_lowercase(), l.name.clone(), l.file.clone()));
    out.dedup_by(|b, a| a.name.to_lowercase() == b.name.to_lowercase());
    out
}

/// The preset for `sound` called `name` in `dir`, read.
pub fn read(dir: &Path, sound: &str, name: &str) -> Result<Preset, String> {
    let found = list(dir, sound).into_iter().find(|l| l.name == name).ok_or_else(|| format!("There is no preset called {name} any more."))?;
    let text = std::fs::read_to_string(&found.file).map_err(|e| format!("{name} couldn't be read: {e}"))?;
    Preset::parse(&text)
}

/// Why a preset wasn't stored.
#[derive(Debug, PartialEq)]
pub enum NotStored {
    /// A preset of this name, in any capitals, is there: its name. Store again with `replace`.
    Taken(String),
    /// Said in words.
    Refused(String),
}

/// Write `p` into `dir` as `<name>.json`: to a temporary file, then renamed into place, so a
/// preset is never half written. A preset whose name differs only in capitals is the same one
/// (as a song's folder is, PLAT-7): it is replaced only with `replace`.
pub fn store(dir: &Path, p: &Preset, replace: bool) -> Result<PathBuf, NotStored> {
    if let Some(why) = name_error(&p.name) {
        return Err(NotStored::Refused(format!("{why}.")));
    }
    let taken = list(dir, &p.sound).into_iter().find(|l| l.name.to_lowercase() == p.name.to_lowercase());
    if let Some(t) = taken.as_ref().filter(|_| !replace) {
        return Err(NotStored::Taken(t.name.clone()));
    }
    let file = dir.join(format!("{}.json", p.name));
    if taken.is_none() && file.exists() {
        return Err(NotStored::Refused(format!("Another file called {}.json is in the Presets folder.", p.name)));
    }
    let io = |e: std::io::Error| NotStored::Refused(format!("The preset couldn't be saved: {e}."));
    std::fs::create_dir_all(dir).map_err(io)?;
    let temp = dir.join(format!(".{}.json.{}.tmp", p.name, std::process::id()));
    std::fs::write(&temp, p.text()).map_err(io)?;
    // The one it replaces, under its old capitals.
    if let Some(t) = taken.filter(|t| t.file != file) {
        let _ = std::fs::remove_file(&t.file);
    }
    std::fs::rename(&temp, &file).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        io(e)
    })?;
    Ok(file)
}

/// A call of a component, whose settings a preset holds.
pub struct Sound {
    /// The call, as the view names it (`tracks/keys.ts#3`), in the accepted revision.
    pub node: String,
    pub component: String,
    /// Its descriptor's label (`Synth pluck`): what the app calls it.
    pub label: String,
    site: Site,
    desc: Arc<Descriptor>,
    options: Option<usize>,
}

/// The call `node` names at revision `at` (`file#ordinal`), as it is in the accepted revision:
/// carried when the files changed without touching it (WRITE-12), else stale.
fn call_at(s: &Session, at: u64, node: &str) -> Result<Site, EditError> {
    let find = |sites: &[Site]| controls::site_by_id(sites, &format!("{node}/")).map(|(site, _)| site.clone());
    let accepted = s.accepted();
    if at == accepted.number {
        return find(&accepted.compiled.sites).ok_or_else(|| EditError::NotFound(node.to_string()));
    }
    let carried = || -> Option<Site> {
        let past = s.past(at)?;
        let old = find(past.sites())?;
        let file = Path::new(&old.file);
        let (was, is) = (past.snapshot.files.get(file)?, accepted.snapshot.files.get(file)?);
        let to = Edits::between(was, is).carry(old.start as usize..old.end as usize)?.start as u32;
        accepted.compiled.sites.iter().find(|x| x.file == old.file && x.start == to && x.end == to + (old.end - old.start)).cloned()
    };
    carried().ok_or(EditError::Stale { current: accepted.number })
}

/// The sound call `node` (named at revision `at`) is: a component's instance.
pub fn sound(s: &Session, at: u64, node: &str) -> Result<Sound, EditError> {
    let site = call_at(s, at, node)?;
    let rev = s.accepted();
    let frame = rev.compiled.ir.modules.iter().find_map(|m| {
        let f = m.frames.iter().find(|f| f.site == site.id && f.component.is_some())?;
        Some((m, f.component.clone()?))
    });
    let not = || EditError::Missing("This isn't a sound whose settings can be saved.".into());
    let (m, component) = frame.ok_or_else(not)?;
    let desc = rev.compiled.components.get(&component).cloned().ok_or_else(not)?;
    Ok(Sound {
        node: format!("{}#{}", site.file, site.ordinal),
        component,
        label: desc.label.clone(),
        options: options_of(m, site.id).ok().flatten(),
        site,
        desc,
    })
}

/// What drives a control a preset leaves alone: a lane, other code (a cable, or a value other
/// calls share), or a value written in other units (`beats(1)` for a time in seconds).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Why {
    Lane,
    Code,
    Units,
}

/// A declared control of a sound's call, as a preset sees it.
enum Slotted {
    /// Its value is this call's alone: written in the code, or left out of the call.
    Mine(Control),
    /// A value other calls share too: saved, never loaded.
    Shared(Control),
    /// Driven by something a preset leaves alone.
    Kept(Why),
    /// Not written and not writable here (an optional control left out): nothing to save or load.
    Absent,
}

/// What a control is called in words: its label, else its name.
fn label(d: &ControlDesc) -> String {
    d.label.clone().unwrap_or_else(|| d.name.clone())
}

/// Each declared control of `sound` that holds a number (a dictionary's are left out), with how it
/// is written in the call.
fn slots(s: &Session, sound: &Sound) -> Vec<(ControlDesc, Slotted)> {
    let all = controls::controls(s);
    let rev = s.accepted();
    let text = rev.snapshot.files.get(Path::new(&sound.site.file)).map_or("", |t| &t[..]);
    let desc = &sound.desc;
    let mut out = vec![];
    for d in desc.controls.iter().filter(|d| d.each.is_none()) {
        let found = all.iter().find(|c| c.calls.contains(&sound.site.id) && c.declared.iter().any(|x| x.component == desc.id && x.slot == d.name));
        let slotted = match found {
            Some(c) if c.unit != d.unit => Slotted::Kept(Why::Units),
            Some(c) if c.calls.len() == 1 && c.declared.len() == 1 => Slotted::Mine(c.clone()),
            Some(c) => Slotted::Shared(c.clone()),
            None => match written_at(&sound.site, desc, &d.name, sound.options, text) {
                Some(w) if w.contains("lane.") => Slotted::Kept(Why::Lane),
                Some(_) => Slotted::Kept(Why::Code),
                None => Slotted::Absent,
            },
        };
        out.push((d.clone(), slotted));
    }
    out
}

/// What is written at control `slot` of `desc` in call `site` (whose options are argument
/// `options`), if anything: at its place among the arguments, or in the options object.
fn written_at<'a>(site: &Site, desc: &Descriptor, slot: &str, options: Option<usize>, text: &'a str) -> Option<&'a str> {
    let mut params: Vec<&str> = desc.inputs.iter().filter(|i| !desc.positional.contains(&i.name)).map(|i| i.name.as_str()).collect();
    params.extend(desc.positional.iter().map(String::as_str));
    let given = options.unwrap_or(site.args.len());
    let arg = match params.iter().position(|p| *p == slot).filter(|p| *p < given) {
        Some(p) => site.args.get(p)?,
        None => match options.and_then(|i| site.args.get(i))? {
            Arg::Object { props, .. } => &props.iter().find(|(k, _)| k == slot)?.1,
            _ => return None,
        },
    };
    let (start, end) = arg.span();
    text.get(start as usize..end as usize)
}

/// A preset made from a call's settings, and the controls it left out: each one's label, and why.
#[derive(Debug, PartialEq)]
pub struct Saved {
    pub preset: Preset,
    pub left: Vec<(String, Why)>,
}

/// The settings of call `node` (named at revision `at`) as a preset called `name`: every declared
/// control's number, by its slot in base units, whether written or left out (its default). One a
/// lane or other code drives is left out, and named. Writes nothing.
pub fn capture(s: &Session, at: u64, node: &str, name: &str) -> Result<Saved, EditError> {
    if let Some(why) = name_error(name) {
        return Err(EditError::Missing(format!("{}{}.", why[..1].to_uppercase(), &why[1..])));
    }
    let sound = sound(s, at, node)?;
    let mut values = BTreeMap::new();
    let mut left = vec![];
    for (d, slotted) in slots(s, &sound) {
        match slotted {
            Slotted::Mine(c) | Slotted::Shared(c) => {
                values.insert(d.name.clone(), c.value);
            }
            Slotted::Kept(why) => left.push((label(&d), why)),
            Slotted::Absent => {}
        }
    }
    Ok(Saved { preset: Preset { name: name.into(), sound: sound.component, values, plugin_version: None, state: None }, left })
}

/// A loaded preset: the edit, and the controls it left alone, each by its label and why.
#[derive(Debug, PartialEq)]
pub struct Loaded {
    pub edited: Edited,
    pub kept: Vec<(String, Why)>,
}

/// Load `preset` into call `node` (named at revision `at`): one `Values` Tx. Each number written in
/// the call is rewritten in its units; each control the call leaves out is written in (several in
/// one insertion); a control the preset doesn't name keeps its value. A control a lane or other
/// code drives keeps it, and is named. A preset for another sound is refused in words.
pub fn load(s: &mut Session, at: u64, node: &str, preset: &Preset) -> Result<Loaded, EditError> {
    ready(s)?;
    let sound = sound(s, at, node)?;
    if preset.sound != sound.component {
        let other = s.catalog().components.get(&preset.sound).map_or_else(|| preset.sound.clone(), |d| d.label.clone());
        return Err(EditError::Missing(format!("This preset is for {other}, not {}.", sound.label)));
    }
    let site = sound.site.clone();
    let mut kept = vec![];
    let mut anchors = vec![];
    // Per file: its literals' rewrites; and the insertions, all in the call's file.
    let mut rewrites: BTreeMap<String, Vec<(u32, u32, String)>> = BTreeMap::new();
    let mut inserts: Vec<(Insertion, String)> = vec![];
    for (d, slotted) in slots(s, &sound) {
        let Some(&value) = preset.values.get(&d.name) else { continue };
        let c = match slotted {
            Slotted::Mine(c) => c,
            Slotted::Shared(_) => {
                kept.push((label(&d), Why::Code));
                continue;
            }
            Slotted::Kept(why) => {
                kept.push((label(&d), why));
                continue;
            }
            Slotted::Absent => continue,
        };
        match &c.edit {
            Edit::Literal { lens, .. } => {
                let edit = lens.propose(value)?;
                if edit.value == c.value || edit.text == c.literal {
                    continue;
                }
                let file = path(&c.file)?;
                anchors.push((Anchor::Written { file, range: Range::bytes(lens.start, lens.end) }, edit.value));
                rewrites.entry(c.file.clone()).or_default().push((lens.start, lens.end, edit.text));
            }
            Edit::Insert(insertion) => {
                let edit = lens::propose(value, 1.0)?;
                if edit.value == c.value {
                    continue;
                }
                let spot = Spot { file: path(&site.file)?, range: Range::bytes(site.start, site.end) };
                anchors.push((Anchor::Slot { site: spot, slot: d.name.clone() }, edit.value));
                inserts.push((insertion.clone(), insertion.number(&edit.text)));
            }
        }
    }
    let said = format!("{} preset {}", sound.label, preset.name);
    if anchors.is_empty() {
        let edited = Edited { revision: s.accepted().number, file: site.file.clone(), before: said.clone(), after: said };
        return Ok(Loaded { edited, kept });
    }
    let joined = lens::together(&inserts).ok_or_else(|| EditError::Rejected("these settings can't be written into the call at once".into()))?;
    rewrites.entry(site.file.clone()).or_default().extend(joined);
    let mut ops = vec![];
    for (file, mut edits) in rewrites {
        edits.sort_by_key(|e| (e.0, e.1));
        if edits.windows(2).any(|w| w[1].0 < w[0].1) {
            return Err(EditError::Rejected("two settings are written in the same place".into()));
        }
        let old = &s.accepted().snapshot.files[Path::new(&file)];
        ops.push(plan::text_edit(&path(&file)?, old, &edits));
    }
    let label = (format!("{} settings", sound.label), said);
    let tx = plan::planned(s, label.clone(), ops, Expect::Values(anchors));
    let refused = |why: &str| EditError::Rejected(format!("loading the preset would change more than {}'s settings ({why})", sound.label));
    let done = plan::run_as(s, tx, refused, |_, _| Ok(()))?;
    Ok(Loaded { edited: Edited { revision: done.number, file: site.file, before: label.0, after: label.1 }, kept })
}

// ---------------------------------------------------------------- plugins (HOST-8)

/// Why a plugin call's settings can't be saved or loaded: it has no key, so no state file.
const NO_KEY: &str = "Give this plugin a key in its code to save its settings, such as key: \"lead\".";

/// A plugin's call, whose settings a preset holds: its state, and the parameters its code writes.
pub struct PluginCall {
    /// The call, as the view names it (`tracks/keys.ts#3`), in the accepted revision.
    pub node: String,
    /// Its graph id (`clap:m2.probe`): a preset's `sound`.
    pub plugin: String,
    /// Its own name ("Surge XT"), else its id.
    pub label: String,
    /// The version installed; None when it isn't installed.
    pub version: Option<String>,
    /// Its node in the checked graph (`<module>/<node>`): where its live instance is (HOST-7).
    pub flat: String,
    /// Where its state is filed (ID-8): its module, then its frames and key. None without either.
    state: Option<(Key, Vec<Key>)>,
    /// Each parameter its code gives: its CLAP id, its name, and how it is written.
    params: Vec<(u32, String, Slotted)>,
    file: String,
}

impl PluginCall {
    /// The CLAP ids of the parameters its code writes as numbers: what a preset holds beside the
    /// state.
    pub fn owned(&self) -> Vec<u32> {
        self.params.iter().filter(|p| matches!(p.2, Slotted::Mine(_) | Slotted::Shared(_))).map(|p| p.0).collect()
    }

    /// Refused in words when it has no state file to save or load: no key.
    pub fn keyed(&self) -> Result<(), EditError> {
        match self.state {
            Some(_) => Ok(()),
            None => Err(EditError::Missing(NO_KEY.into())),
        }
    }
}

/// What a call is to presets: a component's instance, or a plugin.
pub enum Of {
    Sound(Sound),
    Plugin(PluginCall),
}

impl Of {
    /// Its `sound` (a ComponentId or a plugin's id) and what the app calls it.
    pub fn named(&self) -> (String, String) {
        match self {
            Of::Sound(x) => (x.component.clone(), x.label.clone()),
            Of::Plugin(p) => (p.plugin.clone(), p.label.clone()),
        }
    }
}

/// What call `node` (named at revision `at`) is to presets.
pub fn of(s: &Session, at: u64, node: &str) -> Result<Of, EditError> {
    match sound(s, at, node) {
        Ok(x) => Ok(Of::Sound(x)),
        Err(EditError::Missing(why)) => plugin_call(s, at, node).map(Of::Plugin).map_err(|_| EditError::Missing(why)),
        Err(e) => Err(e),
    }
}

/// The text of property `key` of call `site`'s options (an object argument).
fn prop_text<'a>(site: &Site, key: &str, text: &'a str) -> Option<&'a str> {
    let arg = site.args.iter().find_map(|a| match a {
        Arg::Object { props, .. } => props.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    })?;
    let (start, end) = arg.span();
    text.get(start as usize..end as usize)
}

/// The plugin call `node` (named at revision `at`) is: the one plugin it made.
pub fn plugin_call(s: &Session, at: u64, node: &str) -> Result<PluginCall, EditError> {
    let site = call_at(s, at, node)?;
    let rev = s.accepted();
    let mut made = vec![];
    for m in &rev.compiled.ir.modules {
        walk(&m.nodes, &mut |n| {
            if matches!(n.op, Op::Plugin { .. }) && (n.origin.as_ref().is_some_and(|o| o.site == site.id) || calls_of(m, n).any(|f| f == site.id)) {
                made.push((m, n.clone()));
            }
        });
    }
    let [(m, n)] = &made[..] else { return Err(EditError::Missing("This isn't a sound whose settings can be saved.".into())) };
    let Op::Plugin { plugin, inputs, .. } = &n.op else { unreachable!("a plugin node") };
    let info = s.packages.plugin_info(plugin);
    let label = info.map_or_else(|| plugin.strip_prefix("clap:").unwrap_or(plugin).to_string(), |i| i.found.name.clone());
    let state = n.op.state_path().and_then(|p| Some((Key::new(&m.key).ok()?, p.iter().map(|k| Key::new(k).ok()).collect::<Option<Vec<_>>>()?)));
    let all = controls::controls(s);
    let text = rev.snapshot.files.get(Path::new(&site.file)).map_or("", |t| &t[..]);
    let mut params = vec![];
    for (port, r) in inputs {
        let Some((_, id)) = m2_runtime::packages::address(port) else { continue };
        let name = info.and_then(|i| i.params.iter().find(|p| p.id == id)).map_or_else(|| port.clone(), |p| p.name.clone());
        let mut src = None;
        walk(&m.nodes, &mut |x| {
            if x.id == split_ref(r).0 {
                src = Some(x.clone());
            }
        });
        let literal = src.and_then(|x| match (&x.op, &x.origin) {
            (Op::Const { .. }, Some(origin)) => all.iter().find(|c| c.origin.same_value(origin)).cloned(),
            _ => None,
        });
        let slotted = match literal {
            Some(c) if c.unit.is_some() => Slotted::Kept(Why::Units),
            Some(c) if c.uses == 1 && matches!(c.edit, Edit::Literal { .. }) => Slotted::Mine(c),
            Some(c) => Slotted::Shared(c),
            None if prop_text(&site, port, text).is_some_and(|w| w.contains("lane.")) => Slotted::Kept(Why::Lane),
            None => Slotted::Kept(Why::Code),
        };
        params.push((id, name, slotted));
    }
    Ok(PluginCall {
        node: format!("{}#{}", site.file, site.ordinal),
        plugin: plugin.clone(),
        label,
        version: info.map(|i| i.found.version.clone()),
        flat: format!("{}/{}", m.key, n.id),
        state,
        params,
        file: site.file.clone(),
    })
}

/// A plugin call's settings as a preset called `name`: `blob`, the state its live instance saved,
/// and the number of each parameter its code writes, by CLAP id. One a lane or other code drives
/// is left out, and named. Writes nothing.
pub fn capture_plugin(call: &PluginCall, name: &str, blob: &[u8]) -> Result<Saved, EditError> {
    if let Some(why) = name_error(name) {
        return Err(EditError::Missing(format!("{}{}.", why[..1].to_uppercase(), &why[1..])));
    }
    call.keyed()?;
    let mut values = BTreeMap::new();
    let mut left = vec![];
    for (id, label, slotted) in &call.params {
        match slotted {
            Slotted::Mine(c) | Slotted::Shared(c) => {
                values.insert(id.to_string(), c.value);
            }
            Slotted::Kept(why) => left.push((label.clone(), *why)),
            Slotted::Absent => {}
        }
    }
    let preset = Preset {
        name: name.into(),
        sound: call.plugin.clone(),
        values,
        plugin_version: call.version.clone(),
        state: Some(m2_song::tx::base64(blob)),
    };
    Ok(Saved { preset, left })
}

/// Load `preset`, with its state `blob`, into plugin call `node` (named at revision `at`): one
/// `Confined` Tx that writes the state and its sidecar and rewrites each parameter the code writes
/// as a number. A parameter a lane or other code drives keeps it, and is named. Refused in words
/// for another plugin's preset, a plugin that isn't installed, or a call with no key.
pub fn load_plugin(s: &mut Session, at: u64, node: &str, preset: &Preset, blob: Vec<u8>) -> Result<Loaded, EditError> {
    ready(s)?;
    let call = plugin_call(s, at, node)?;
    if preset.sound != call.plugin {
        let other = s.packages.plugin_info(&preset.sound).map(|i| i.found.name.clone());
        let other = other.or_else(|| s.catalog().components.get(&preset.sound).map(|d| d.label.clone()));
        let other = other.unwrap_or_else(|| preset.sound.strip_prefix("clap:").unwrap_or(&preset.sound).to_string());
        return Err(EditError::Missing(format!("This preset is for {other}, not {}.", call.label)));
    }
    let Some(installed) = &call.version else {
        return Err(EditError::Missing(format!("{} isn't installed on this computer, so its presets can't be loaded.", call.label)));
    };
    let Some((module, keys)) = &call.state else { return Err(EditError::Missing(NO_KEY.into())) };
    let mut kept = vec![];
    let mut edits: BTreeMap<String, Vec<(u32, u32, String)>> = BTreeMap::new();
    for (id, label, slotted) in &call.params {
        let Some(&value) = preset.values.get(&id.to_string()) else { continue };
        let c = match slotted {
            Slotted::Mine(c) => c,
            Slotted::Shared(_) => {
                kept.push((label.clone(), Why::Code));
                continue;
            }
            Slotted::Kept(why) => {
                kept.push((label.clone(), *why));
                continue;
            }
            Slotted::Absent => continue,
        };
        let Edit::Literal { lens, .. } = &c.edit else { continue };
        let edit = lens.propose(value)?;
        if edit.value != c.value && edit.text != c.literal {
            edits.entry(c.file.clone()).or_default().push((lens.start, lens.end, edit.text));
        }
    }
    let version = preset.plugin_version.clone().unwrap_or_else(|| installed.clone());
    let side = Sidecar { plugin: call.plugin.clone(), plugin_version: version };
    let same = m2_song::Sha::of(&blob);
    let w = crate::data::state_writes(s, module, keys, blob, &side)?;
    let said = format!("{} preset {}", call.label, preset.name);
    let state_changes = w.held != Some(same);
    if !state_changes && edits.is_empty() {
        let edited = Edited { revision: s.accepted().number, file: call.file.clone(), before: said.clone(), after: said };
        return Ok(Loaded { edited, kept });
    }
    let mut ops = vec![];
    let mut ranges = BTreeMap::new();
    for (file, mut e) in edits {
        e.sort_by_key(|x| (x.0, x.1));
        let old = &s.accepted().snapshot.files[Path::new(&file)];
        let tx_file = path(&file)?;
        ops.push(plan::text_edit(&tx_file, old, &e));
        ranges.insert(tx_file, Declared::Ranges(e.iter().map(|x| Range::bytes(x.0, x.1)).collect()));
    }
    if state_changes {
        ops.extend(w.ops);
    }
    let label = (format!("{} settings", call.label), said);
    let mut tx = plan::planned(s, label.clone(), ops, Expect::Confined(Confined { ranges, ..Confined::default() }));
    if state_changes {
        tx.base.extend(w.bases);
    }
    let refused = |why: &str| EditError::Rejected(format!("loading the preset would change more than {}'s settings ({why})", call.label));
    let done = plan::run_as(s, tx, refused, |_, _| Ok(()))?;
    Ok(Loaded { edited: Edited { revision: done.number, file: call.file, before: label.0, after: label.1 }, kept })
}

#[cfg(test)]
#[path = "presets_tests.rs"]
mod tests;
