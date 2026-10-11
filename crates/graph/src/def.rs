//! The `m2plug` manifest format (serde types). See docs/music2-design.md §6.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const SPEC_VERSION: &str = "0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// A polyphonic voice graph played by notes, plus an optional `post` effect graph.
    Instrument,
    /// A graph from the `in` source to `out`.
    Effect,
    /// A set of pads (one voice graph per pitch) — drum kits.
    Kit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDef {
    /// Spec version this plugin targets.
    pub m2plug: String,
    pub id: String,
    pub name: String,
    pub kind: PluginKind,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub macros: Vec<MacroDef>,
    /// Named sample slots a device instance can bind assets to (e.g. `"sample"`).
    #[serde(default)]
    pub asset_slots: Vec<String>,
    #[serde(default)]
    pub voice: Option<GraphDef>,
    #[serde(default)]
    pub post: Option<GraphDef>,
    #[serde(default)]
    pub graph: Option<GraphDef>,
    #[serde(default)]
    pub pads: Vec<PadDef>,
    #[serde(default = "default_polyphony")]
    pub polyphony: usize,
}

fn default_polyphony() -> usize {
    12
}

impl PluginDef {
    /// Whether any graph reads the `sidechain` source (the UI then offers a source picker).
    pub fn uses_sidechain(&self) -> bool {
        self.voice
            .iter()
            .chain(self.post.iter())
            .chain(self.graph.iter())
            .any(|g| g.nodes.iter().any(|n| n.inputs.values().any(|r| r.refs().contains(&"sidechain"))))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroDef {
    pub id: String,
    pub name: String,
    /// Normalized 0..1.
    pub default: f32,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDef {
    pub nodes: Vec<NodeDef>,
    pub out: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDef {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Num(f32),
    Str(String),
    Macro(MacroBinding),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroBinding {
    #[serde(rename = "macro")]
    pub macro_id: String,
    pub from: f32,
    pub to: f32,
    #[serde(default)]
    pub curve: Curve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    #[default]
    Lin,
    Exp,
}

impl MacroBinding {
    pub fn map(&self, v: f32) -> f32 {
        let v = v.clamp(0.0, 1.0);
        match self.curve {
            Curve::Exp if self.from > 0.0 && self.to > 0.0 => self.from * (self.to / self.from).powf(v),
            _ => self.from + (self.to - self.from) * v,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputRef {
    One(String),
    Many(Vec<String>),
}

impl InputRef {
    pub fn refs(&self) -> Vec<&str> {
        match self {
            InputRef::One(s) => vec![s.as_str()],
            InputRef::Many(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PadDef {
    pub pitch: u8,
    pub name: String,
    pub voice: GraphDef,
    /// Pads in the same choke group cut each other off (open/closed hats).
    #[serde(default)]
    pub choke: Option<u8>,
}
