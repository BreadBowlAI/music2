//! The music2 graph IR.
//!
//! Every song, however it was written, compiles to this: modules made of nodes, where
//! every node is one of eight ops (`lane`, `time`, `const`, `kernel`, `poly`,
//! `feedback`, `import`, `plugin`). [`check`] validates an IR independently of whatever produced
//! it and flattens it into one typed, ordered [`Flat`] graph for the runtime.
//!
//! Port references are strings: `"n3.lp"` is output `lp` of node `n3`, and a bare
//! `"n3"` means its `out` port.

pub mod check;
pub mod kind;
pub mod sig;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

pub use check::{Flat, FNode, FOp, Input, NoteField, PortRef, Voice, check};
pub use kind::{EventType, Kind, PortKind, Unit};

impl Origin {
    /// Whether two origins name the same written value (site and argument).
    pub fn same_value(&self, other: &Origin) -> bool {
        self.site == other.site && self.arg == other.arg
    }
}
pub use sig::{KernelSig, Kernels, PortSig};

pub const IR_VERSION: u32 = 0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Ir {
    pub ir: u32,
    pub modules: Vec<Module>,
    /// The export that is the song's output.
    pub root: Root,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Root {
    pub module: String,
    pub export: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Module {
    pub name: String,
    /// Whether the module is a track (has rows on the timeline). A host convention.
    #[serde(default)]
    pub track: bool,
    #[serde(default)]
    pub lanes: Vec<LaneDecl>,
    pub nodes: Vec<Node>,
    pub exports: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LaneDecl {
    pub id: String,
    pub kind: Kind,
    /// Value where no clip covers the time. Controls only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: String,
    #[serde(flatten)]
    pub op: Op,
    /// Source location, `file:line:col`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    /// Which call made this node, and for constants which argument holds the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}

/// Where a node came from in the source: a call site (see `m2_script::sites`) and,
/// for a constant, the path to its argument (`["1"]`, `["0", "hats", "pan"]`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Origin {
    pub site: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arg: Vec<String>,
    /// The constant is the source number times this (`khz(2.5)` has scale 1000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    /// A typed timeline input of the module. Inside a `poly` body: a note field.
    Lane { lane: String },
    Time { of: TimeOf },
    /// A constant. Without a unit it is an untyped literal that fits any control.
    Const {
        value: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<Unit>,
    },
    Kernel {
        kernel: String,
        #[serde(default)]
        inputs: BTreeMap<String, String>,
    },
    /// One copy of `body` per sounding note of `events`, summed.
    Poly { events: String, voices: u32, body: Body },
    /// The value of `input` one block ago. The only way to close a loop.
    Feedback { kind: Kind, input: String },
    /// Another module's export.
    Import { module: String, export: String },
    /// A hosted plugin (`clap:<plugin id>`). Inputs are `in` (audio), `notes` (from a
    /// notes lane) and parameters by port name. `state` names a saved state asset.
    Plugin {
        plugin: String,
        #[serde(default)]
        inputs: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Body {
    pub nodes: Vec<Node>,
    pub out: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TimeOf {
    Beats,
    Seconds,
}

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Op::Lane { .. } => "lane",
            Op::Time { .. } => "time",
            Op::Const { .. } => "const",
            Op::Kernel { .. } => "kernel",
            Op::Poly { .. } => "poly",
            Op::Feedback { .. } => "feedback",
            Op::Import { .. } => "import",
            Op::Plugin { .. } => "plugin",
        }
    }
}

/// Split `"n3.lp"` into `("n3", "lp")`; a bare `"n3"` is `("n3", "out")`.
pub fn split_ref(r: &str) -> (&str, &str) {
    r.rsplit_once('.').unwrap_or((r, "out"))
}

/// A problem found while building or checking a song.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Diagnostic {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>) -> Diagnostic {
        Diagnostic { message: message.into(), ..Default::default() }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(src) = &self.src {
            write!(f, "{src}: ")?;
        }
        match (&self.module, &self.node) {
            (Some(m), Some(n)) => write!(f, "[{m}/{n}] ")?,
            (Some(m), None) => write!(f, "[{m}] ")?,
            _ => {}
        }
        write!(f, "{}", self.message)
    }
}
