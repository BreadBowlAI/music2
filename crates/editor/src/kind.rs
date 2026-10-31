//! What a value on a card is (CARD-2): one closed vocabulary, said by the descriptor (a setting's
//! kind) or by a plugin's parameter (CARD-6), never by how it is drawn. The app picks the widget:
//! a knob, a menu, a switch, a number box, a waveform, a menu of tracks or a sentence.

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
