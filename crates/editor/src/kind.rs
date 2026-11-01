//! What a value on a card is (CARD-2): one closed vocabulary, said by the descriptor (a setting's
//! kind, a control's unit, CARD-20) or by a plugin's parameter (CARD-6), never by how it is drawn.
//! The app picks the widget: a knob, a menu, a switch, a number box, a waveform, a menu of tracks
//! or a sentence. This module is the one place that says what any value is, and how a choice's or
//! a switch's number is written in code.

use m2_ir::Unit;
use m2_script::component::{SettingDesc, SettingKind};
use serde::Serialize;
use serde_json::Value as Json;

/// What a value is. Adding a word here is a change to the contract (the design's §2.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A number: a knob for a control, a number box for a setting.
    #[default]
    Number,
    /// One of its `options`: a menu.
    Choice,
    /// Off or on, its two `options` in that order: a switch.
    Switch,
    /// A file in the song's `assets/`: its waveform, and a file dropped on it replaces it.
    Recording,
    /// Which recording plays on which notes (PKG-15): a sentence until views draw it.
    Regions,
    /// MIDI notes to files: a sentence until views draw it.
    Keymap,
    /// A track, by its key: a menu of the song's tracks.
    Track,
}

/// One option of a choice or a switch: the value written in the code, and what the menu says.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Choice {
    pub value: Json,
    pub label: String,
}

impl Choice {
    pub fn new(value: impl Into<Json>, label: impl Into<String>) -> Choice {
        Choice { value: value.into(), label: label.into() }
    }
}

/// A switch's two options, off then on.
pub fn switch(off: impl Into<Json>, on: impl Into<Json>, names: Option<(&str, &str)>) -> Vec<Choice> {
    let (a, b) = names.unwrap_or(("Off", "On"));
    vec![Choice::new(off, a), Choice::new(on, b)]
}

/// A choice's options: each name, written as itself, is its own label.
fn names<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<Choice> {
    names.into_iter().map(|n| Choice::new(n, n)).collect()
}

/// What a control is (CARD-20), by its constant's unit: a choice of its names, a switch of `false`
/// and `true` (as the code writes it, never 0 and 1), else a number.
pub fn of_unit(unit: Option<Unit>) -> (Kind, Vec<Choice>) {
    match unit {
        Some(Unit::Switch) => (Kind::Switch, switch(false, true, None)),
        Some(Unit::Choice(o)) => (Kind::Choice, names(o.names().iter().map(|n| &**n))),
        _ => (Kind::Number, vec![]),
    }
}

/// What a setting declared as `d` is (CARD-2), whether it is a whole number, and its options. A key
/// carries none: it names one of the song's tracks, which the view gives once (its timeline).
pub fn of_setting(d: &SettingDesc) -> (Kind, bool, Vec<Choice>) {
    match d.kind {
        SettingKind::Number => (Kind::Number, false, vec![]),
        SettingKind::Int => (Kind::Number, true, vec![]),
        SettingKind::Bool => (Kind::Switch, false, switch(false, true, None)),
        SettingKind::Choice => (Kind::Choice, false, names(d.options.iter().flatten().map(String::as_str))),
        SettingKind::Asset => (Kind::Recording, false, vec![]),
        SettingKind::Keymap => (Kind::Keymap, false, vec![]),
        SettingKind::Regions => (Kind::Regions, false, vec![]),
        SettingKind::Key => (Kind::Track, false, vec![]),
    }
}

/// What a control or a setting is called (CARD-34): the one rule every name of a value on a card, in
/// its undo steps, in `m2 graph` and in the API follows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Name {
    /// Its label, else its name; a dictionary entry's after the entry's key (`vox.to`, `vox.goes to`).
    pub label: String,
    /// A dictionary entry's key, which `label` starts with (`vox`): the window names the entry by its
    /// track. Said apart, so no one looks for an entry in a label's own dots ("approx. time").
    pub entry: Option<String>,
}

/// What the control or setting declared at `slot` with `label` is called (CARD-34): its label when it
/// has one, else its name. A dictionary entry's (`channels.vox.to`) is named by its key first, then
/// its label or its name (`vox.to`, `vox.goes to`), its key apart.
pub fn named(label: Option<&str>, slot: &str) -> Name {
    let Some((_, key)) = slot.split_once('.') else {
        return Name { label: label.unwrap_or(slot).to_string(), entry: None };
    };
    let (entry, path) = match key.rsplit_once('.') {
        Some((path, _)) => (path.split('.').next().map(str::to_string), Some(path)),
        None => (None, None),
    };
    let label = match (label, path) {
        (Some(label), Some(path)) => format!("{path}.{label}"),
        (Some(label), None) => label.to_string(),
        (None, _) => key.to_string(),
    };
    Name { label, entry }
}

/// What a value no declaration names is called, by where it is written in its call after its
/// argument (`ceiling`, `hats.in`): a property of a property is a dictionary entry's, by its key.
pub fn written(path: &[String]) -> Name {
    Name { label: path.join("."), entry: path.first().filter(|_| path.len() > 1).cloned() }
}

/// What a cell's number is, as the code writes it (CARD-20): a choice's place as its name, a
/// switch's 1 or 0 as `true` or `false`, a number as itself. None for a place no option has, or a
/// number that isn't finite.
pub fn said(unit: Option<Unit>, value: f64) -> Option<Json> {
    match unit {
        Some(Unit::Switch) => Some(Json::Bool(value != 0.0)),
        Some(Unit::Choice(o)) => {
            let name = o.name(value as usize).filter(|_| value >= 0.0 && value.fract() == 0.0)?;
            Some(Json::String(name.to_string()))
        }
        _ => value.is_finite().then(|| number_json(value)),
    }
}

/// A number as JSON: a whole number as an integer, so `8` written reads as the `8` a descriptor
/// declares, and a plugin's step as the step its option writes.
pub fn number_json(v: f64) -> Json {
    match v.fract() == 0.0 && v.abs() < 9.0e15 {
        true => Json::from(v as i64),
        false => serde_json::Number::from_f64(v).map_or(Json::Null, Json::Number),
    }
}

/// The number a value written as `value` plays as in a cell of `unit` (CARD-21): a choice's name
/// as its place, `true` or `false` as 1 or 0, a number as itself. Why not, in words that say what
/// `name` takes, when `value` isn't one.
pub fn number(unit: Option<Unit>, name: &str, value: &Json) -> Result<f64, String> {
    match unit {
        Some(Unit::Switch) => match value {
            Json::Bool(b) => Ok(f64::from(u8::from(*b))),
            _ => Err(format!("{name} is a switch: true or false, not {value}")),
        },
        Some(Unit::Choice(o)) => match value.as_str().and_then(|v| o.place(v)) {
            Some(place) => Ok(place as f64),
            None => Err(format!("{name} is one of {}, not {value}", one_of(o.names()))),
        },
        _ => value.as_f64().filter(|v| v.is_finite()).ok_or_else(|| format!("{name} is a number, not {value}")),
    }
}

/// What a hosted plugin's parameter is on a card (CARD-6), from what the plugin says of it: stepped
/// from 0 to 1, a switch; stepped with 2 to 64 steps, each named differently by the plugin (`steps`,
/// its names from `min` up), a choice of those names, each writing its step's number (the code
/// holds the number, FMT-18); anything else, a number.
pub fn of_param(stepped: bool, min: f64, max: f64, steps: Option<&[String]>) -> (Kind, Vec<Choice>) {
    if !stepped || max <= min || min.fract() != 0.0 || max.fract() != 0.0 {
        return (Kind::Number, vec![]);
    }
    if (min, max) == (0.0, 1.0) {
        let names = steps.filter(|n| n.len() == 2 && n.iter().all(|t| !t.is_empty()) && n[0] != n[1]);
        return (Kind::Switch, switch(0, 1, names.map(|n| (n[0].as_str(), n[1].as_str()))));
    }
    let count = max - min + 1.0;
    let Some(names) = steps.filter(|n| n.len() as f64 == count && n.len() <= 64) else { return (Kind::Number, vec![]) };
    let mut seen = std::collections::HashSet::new();
    if !names.iter().all(|n| !n.is_empty() && seen.insert(n.as_str())) {
        return (Kind::Number, vec![]);
    }
    let options = names.iter().enumerate().map(|(i, n)| Choice::new(min as i64 + i as i64, n.clone())).collect();
    (Kind::Choice, options)
}

/// A choice's names in words: `"saw", "square" or "sine"` (the one way they are listed).
pub fn one_of(names: &[impl AsRef<str>]) -> String {
    let listed: Vec<String> = names.iter().map(|n| format!("{:?}", n.as_ref())).collect();
    match listed.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => listed.join(""),
    }
}

/// A word typed for a value of `kind` (`m2 set`), read as the value takes it: one of its
/// `options` by its value or its name (`square`, `On`, `Master`), `true` or `false` for a switch,
/// a number as the code writes it (`2`, never `2.0`), else the word itself.
pub fn word(kind: Kind, options: &[Choice], w: &str) -> Json {
    if let Some(o) = options.iter().find(|o| o.value.as_str() == Some(w) || o.label.eq_ignore_ascii_case(w)) {
        return o.value.clone();
    }
    match (kind, w.parse::<f64>()) {
        (Kind::Switch, _) if w == "true" || w == "false" => Json::Bool(w == "true"),
        (_, Ok(n)) if n.is_finite() => number_json(n),
        _ => Json::String(w.to_string()),
    }
}
