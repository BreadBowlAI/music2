//! The music2 graph IR.
//!
//! Every song, however it was written, compiles to this: modules made of nodes, where
//! every node is one of eight ops (`lane`, `time`, `const`, `kernel`, `poly`,
//! `feedback`, `import`, `plugin`). [`check`] validates an IR independently of whatever produced
//! it and flattens it into one typed, ordered [`Flat`] graph for the runtime.
//!
//! Port references are strings: `"n3.lp"` is output `lp` of node `n3`, and a bare
//! `"n3"` means its `out` port.
//!
//! Modules, lanes and plugin states are addressed by keys ([`key`]); labels are display
//! text that no address uses (ID-10, ID-11).

pub mod check;
pub mod key;
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

pub const IR_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Ir {
    pub ir: u32,
    pub modules: Vec<Module>,
    /// The export that is the song's output.
    pub root: Root,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Root {
    /// A module key.
    pub module: String,
    pub export: String,
}

/// A module: the root `song`, or a track (ID-11).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Module {
    /// The key every address uses: imports, flat node names, lane addresses, data folders.
    pub key: String,
    /// Display text only, never an address (ID-10). Absent means the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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

impl Module {
    /// What to show for the module: its label, or else its key.
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.key)
    }
}

/// A lane, addressed by its module's key and its own (ID-6).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LaneDecl {
    pub key: String,
    pub kind: Kind,
    /// Value where no clip covers the time. Controls only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<f64>,
    /// Display text only (ID-10). Absent means the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl LaneDecl {
    /// What to show for the lane: its label, or else its key.
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.key)
    }
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
    /// The component calls (sites) this node was made inside, outermost first: the
    /// `bass(...)` call for every node the bass instrument builds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub within: Vec<u32>,
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
    /// Another module's export, by module key.
    Import { module: String, export: String },
    /// A hosted plugin (`clap:<plugin id>`). Inputs are `in` (audio), `notes` (from a
    /// notes lane) and parameters by port name. Its state, if it has one, is filed under
    /// its module, its `key()` frames and its key (ID-8): see [`Op::state_path`].
    Plugin {
        plugin: String,
        #[serde(default)]
        inputs: BTreeMap<String, String>,
        /// The plugin's key, when written (`plugin(id, { key: "lead" })`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        /// The keys of the `key()` frames it was made in, outermost first.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        frames: Vec<String>,
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
    /// Where a plugin's state is filed below its module, as keys (ID-8): its frames, then
    /// its key. A plugin without a key takes its innermost frame's key as its file name.
    /// `None` for a plugin with neither, which has no saved state, and for other ops.
    pub fn state_path(&self) -> Option<Vec<String>> {
        let Op::Plugin { key, frames, .. } = self else { return None };
        match key {
            Some(k) => Some(frames.iter().cloned().chain([k.clone()]).collect()),
            None if frames.is_empty() => None,
            None => Some(frames.clone()),
        }
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Notice,
}

/// A problem found while building or checking a song (DIAG-1). The same shape as the data
/// diagnostics `core/song` raises, without `fixes`: a build or check error has none.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// A code from the contract's table (DIAG-2), such as `key.duplicate`.
    pub code: &'static str,
    pub message: String,
    /// The module's key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// The node, for a graph error: narrows `src`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// `file:line:col`, or a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
}

impl Diagnostic {
    /// An Error with a code from the contract's table.
    pub fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic { severity: Severity::Error, code, message: message.into(), module: None, node: None, src: None }
    }

    pub fn module(mut self, key: impl Into<String>) -> Diagnostic {
        self.module = Some(key.into());
        self
    }

    pub fn at(mut self, src: Option<String>) -> Diagnostic {
        self.src = src;
        self
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
