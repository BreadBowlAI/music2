//! Component descriptors and the unit table, as a build emits them for editors and gestures
//! (PKG-11). `@m2/core`'s `component()` checks a descriptor where it is declared; these types
//! carry what it emits. Numbers are in base units; a default or an insert also carries the
//! spelling and the argument it was written with.

use std::collections::BTreeMap;
use std::sync::Arc;

use m2_ir::{Diagnostic, Unit};
use serde::{Deserialize, Serialize};

/// A component as its descriptor declares it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Descriptor {
    /// The ComponentId: `<package>:<id>` (`std:delay`), or a project component's bare id (PKG-9).
    pub id: String,
    pub label: String,
    /// The module that defines it, as imported: `std/fx`, or a project path (`lib/fx.ts`).
    pub module: String,
    /// The defining file, as shown to people: `std/src/fx.ts`.
    pub file: String,
    /// Where `component()` was called: `std/src/fx.ts:12:22`.
    pub at: String,
    pub inputs: Vec<PortDesc>,
    pub controls: Vec<ControlDesc>,
    pub settings: Vec<SettingDesc>,
    pub output: Option<PortDesc>,
    /// Slots written in order after the inputs it doesn't name.
    pub positional: Vec<String>,
    pub editor: Editor,
}

/// An input (named) or the output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortDesc {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: PortKind,
    /// A control port's unit, when it declares one.
    pub unit: Option<Unit>,
    /// It only listens, so monitoring mute and solo never act on it (PKG-13).
    #[serde(default)]
    pub sidechain: bool,
    /// It takes any number of signals: the last input of a call only.
    #[serde(default)]
    pub many: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortKind {
    Audio,
    Control,
    Notes,
    Any,
}

/// A declared control (PKG-3), or a dictionary slot when `each` is set (PKG-6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlDesc {
    pub name: String,
    /// None for `control.any()`, typed by what flows in, and for a dictionary slot.
    pub unit: Option<Unit>,
    /// What a call that leaves it out gets (PKG-4); none when a call must give it.
    pub default: Option<Spelled>,
    /// The range a knob shows, in base units: a hint unless `strict` (PKG-8).
    pub range: Option<(f64, f64)>,
    pub log: bool,
    pub strict: bool,
    /// Other units a value may have, with the range a knob shows for each.
    pub accepts: Vec<Accepts>,
    /// Its cell ramps over one block on every change (PKG-14).
    pub smooth: bool,
    /// Left out, it makes no node (PKG-5).
    pub optional: bool,
    pub label: Option<String>,
    pub each: Option<Box<EachDesc>>,
}

/// One entry of a dictionary slot: its inputs and controls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EachDesc {
    pub inputs: Vec<PortDesc>,
    pub controls: Vec<ControlDesc>,
}

/// A number as a descriptor wrote it: its value in base units, and the spelling and argument it
/// was written with (`ms(45)`: 0.045…, `ms`, 45). Writing `spelled(written)` gives `value` exactly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spelled {
    pub value: f64,
    pub spelled: Option<String>,
    pub written: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Accepts {
    pub unit: Unit,
    pub range: Option<(f64, f64)>,
    pub log: bool,
}

/// A build-time value (a voice count, a seed): never a knob or a cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingDesc {
    pub name: String,
    pub kind: SettingKind,
    pub default: Option<serde_json::Value>,
    pub options: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    Number,
    Int,
    Bool,
    Choice,
}

/// What an editor offers. A component with a category is an effect, added after any audio, when
/// it takes audio first, and an instrument a new track plays when it takes notes first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Editor {
    /// A free string the UI groups (PKG-7).
    pub category: Option<String>,
    pub description: String,
    /// What adding it writes, in order.
    pub insert: Vec<Insert>,
    /// What an instrument's notes play, by MIDI note (`36: "Kick"`): the piano roll's row names.
    #[serde(default)]
    pub notes: BTreeMap<u8, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Insert {
    pub slot: String,
    #[serde(flatten)]
    pub value: Spelled,
}

/// One unit constructor of `@m2/core` (PKG-10, PKG-11): `ms` is seconds times 0.001.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitCtor {
    pub name: String,
    pub unit: Unit,
    pub scale: f64,
    /// The spelling a value in base units is written with: exactly one per unit.
    pub canonical: bool,
}

impl Descriptor {
    /// The control at `slot`, following `<slot>.<key>.<control>` into a dictionary slot.
    pub fn control(&self, slot: &str) -> Option<&ControlDesc> {
        let mut parts = slot.split('.');
        let name = parts.next()?;
        let c = self.controls.iter().find(|c| c.name == name)?;
        match (parts.next(), parts.next(), parts.next()) {
            (None, ..) => Some(c),
            (Some(_key), Some(name), None) => c.each.as_ref()?.controls.iter().find(|x| x.name == name),
            _ => None,
        }
    }

    /// The range and log a value of `unit` shows with at `slot`: the declared ones for the
    /// declared unit, an `accepts` entry's for that unit, else none (a value in another unit
    /// gets no range of the declared one's).
    pub fn range_for(&self, slot: &str, unit: Option<Unit>) -> Option<((f64, f64), bool)> {
        let c = self.control(slot)?;
        if unit == c.unit {
            return c.range.map(|r| (r, c.log));
        }
        let a = c.accepts.iter().find(|a| Some(a.unit) == unit)?;
        a.range.map(|r| (r, a.log))
    }

    /// Whether editors offer it as an effect: a category, and audio first.
    pub fn is_effect(&self) -> bool {
        self.editor.category.is_some() && self.inputs.first().is_some_and(|i| i.kind == PortKind::Audio)
    }

    /// Whether a new track can play it (PKG-7): notes first, its only input, and audio out. The
    /// + Track list offers it only when it has a category, which groups it.
    pub fn is_instrument(&self) -> bool {
        matches!(&self.inputs[..], [i] if i.kind == PortKind::Notes)
            && self.output.as_ref().is_some_and(|o| o.kind == PortKind::Audio)
            && self.controls.iter().all(|c| c.each.is_none() && (c.optional || c.default.is_some()))
            && self.settings.iter().all(|s| s.default.is_some())
    }
}

/// Every component the packages declare, and where each is exported (`m2_script::describe`).
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    /// By ComponentId.
    pub components: BTreeMap<String, Arc<Descriptor>>,
    /// Where each component can be imported from, by ComponentId: its defining module first.
    pub exports: BTreeMap<String, Vec<Export>>,
    /// A module that throws while loading, an invalid descriptor, or two components with one id
    /// (`pkg.invalid`). Those components are left out.
    pub problems: Vec<Diagnostic>,
}

/// An export of a component: `import { <name> } from "<module>"`, or `osc.saw` for a member of
/// an exported object.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Export {
    pub module: String,
    pub name: String,
}

impl Catalog {
    /// The components editors offer as effects.
    pub fn effects(&self) -> impl Iterator<Item = &Arc<Descriptor>> {
        self.components.values().filter(|d| d.is_effect())
    }

    /// The components a new track can play.
    pub fn instruments(&self) -> impl Iterator<Item = &Arc<Descriptor>> {
        self.components.values().filter(|d| d.is_instrument())
    }
}
