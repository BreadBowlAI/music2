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
//!
//! Each module also lists its frames: every component call and `key()` frame evaluated while
//! it was built, with the ComponentId the call is an instance of (ID-15, PKG-9). A node names
//! the innermost frame it was made in, and a declared control's constant names its instance
//! and slot in its origin (PKG-3).

pub mod check;
pub mod key;
pub mod kind;
pub mod sig;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

pub use check::{Consumer, FNode, FOp, Flat, Input, NoteField, OverlaySite, PortRef, Voice, check};
pub use kind::{EventType, Kind, PortKind, Unit};
pub use sig::{KernelSig, Kernels, PortSig};

/// 2: frames (`Module.frames`, `Node.frame`) and declared controls in origins (step 5).
pub const IR_VERSION: u32 = 2;

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
    /// Mute in song (MON-5): the track is silent in every render, its sidechain uses aside. Only a
    /// track has it. Written only when set, so an IR without it keeps its JSON and hash (ID-11).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    /// A bus (RENDER-34): a track the song feeds, which reads what it is fed as its `input`, an
    /// import of the song's export [`feed_export`]. It has no notes or audio lanes. Written only
    /// when set, so an IR without it keeps its JSON and hash (ID-11).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bus: bool,
    #[serde(default)]
    pub lanes: Vec<LaneDecl>,
    pub nodes: Vec<Node>,
    pub exports: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    /// Every component call and `key()` frame evaluated while the module was built, in the
    /// order they opened (ID-15). Nodes and origins name them by index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
}

/// The song's export that a bus keyed `bus` reads as its `input` (RENDER-34): `bus:<key>`.
pub fn feed_export(bus: &str) -> String {
    format!("bus:{bus}")
}

impl Module {
    /// What to show for the module: its label, or else its key.
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.key)
    }

    /// The frames around `frame`, outermost first, ending with it: empty for `None` or an
    /// index the module doesn't have.
    pub fn chain(&self, frame: Option<u32>) -> Vec<&Frame> {
        let mut chain = vec![];
        let mut at = frame;
        while let Some(f) = at.and_then(|i| self.frames.get(i as usize)) {
            chain.push(f);
            // A parent is always an earlier frame (the checker holds IRs to it), so this ends.
            at = f.parent.filter(|p| Some(*p) < at);
        }
        chain.reverse();
        chain
    }
}

/// One evaluation of a component call or of a `key()` frame while its module was built (ID-15).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Frame {
    /// The call.
    pub site: u32,
    /// The ComponentId: the package and the descriptor's `id` (`std:delay`), or a project
    /// component's bare id (PKG-9). Absent for a `key()` frame and for a component declared
    /// the old way, `component({ controls })`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// Which evaluation of `site` within its parent frame this is: 0, 1, … (ID-14).
    #[serde(default)]
    pub occurrence: u32,
    /// A `key()` frame's key (ID-7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The frame it was opened in: always an earlier index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    /// Which argument of a component call held its options (an index), when one did: an
    /// object, or `undefined` where the options go. Only the call can tell `lowpass(x, opts)`'s
    /// options from a cutoff; an editor writes a control the call leaves out there (PKG-3).
    /// Where the source spells it, not what the graph is: comparing two builds ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<u32>,
    /// What each sidechain input of the call received (PKG-13): where monitoring mute and
    /// solo must not reach (MON-2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sidechain: Vec<Sidechain>,
    /// Turned off with `bypass: true` in its options (Next D1): its body ran, so its nodes and
    /// knobs are here, and the call gave its audio input on, so nothing reads them. Written only
    /// when true, so every other IR keeps its bytes (ID-11).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass: bool,
}

/// A sidechain input of a component call, and the signal it received.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sidechain {
    /// The input's name in the descriptor, or a dictionary entry's input's path
    /// (`others.hats.side`).
    pub input: String,
    /// A port reference in the frame's module (`"n12.lp"`).
    #[serde(rename = "ref")]
    pub source: String,
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
    /// The innermost frame it was made in, an index into its module's `frames` (ID-15): the
    /// component calls it was made inside are the component frames of [`Module::chain`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
}

/// Where a node came from in the source: a call site (see `m2_script::sites`) and,
/// for a constant, the path to its argument (`["1"]`, `["0", "hats", "pan"]`). A declared
/// control left out (inherited) has its instance's call as its site and no path (PKG-3).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Origin {
    pub site: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arg: Vec<String>,
    /// The constant is the source number times this (`khz(2.5)` has scale 1000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// The declared control this constant is, when a component descriptor declared it (PKG-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<Slot>,
}

/// A declared control of one component instance (PKG-3, ID-15).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Slot {
    /// The instance: an index into its module's `frames`.
    pub frame: u32,
    /// The control's name; a dictionary slot's is `<slot>.<key>.<control>` (`channels.hats.db`).
    pub slot: String,
    /// Left out, so the descriptor's default ("inherited", not "written"). `Identical` and
    /// `Values` will ignore it (WRITE-6).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherited: bool,
    /// Its cell ramps over one block on every change (PKG-14, RENDER-22a).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub smooth: bool,
}

impl Origin {
    /// Whether two origins are the same value in the code: one written literal (whatever slots
    /// it reaches), or one inherited slot of one call (every evaluation of it).
    pub fn same_value(&self, other: &Origin) -> bool {
        self.site == other.site && self.arg == other.arg && (!self.arg.is_empty() || self.slot() == other.slot())
    }

    /// The declared control's name, when the constant is one.
    pub fn slot(&self) -> Option<&str> {
        self.control.as_ref().map(|c| c.slot.as_str())
    }

    /// Whether the constant is a declared control left out: the descriptor's default.
    pub fn inherited(&self) -> bool {
        self.control.as_ref().is_some_and(|c| c.inherited)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    /// A typed timeline input of the module. Inside a `poly` body: a note field.
    Lane {
        lane: String,
    },
    Time {
        of: TimeOf,
    },
    /// A constant. Without a unit it is an untyped literal that fits any control.
    Const {
        value: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<Unit>,
    },
    /// A package kernel. `assets` and `values` are its settings (kernel ABI v1): the names of the
    /// project's audio files it reads, and numbers fixed at build. Both are part of its shape
    /// (RENDER-25), so changing one makes it new. Written only when given, so an IR without them
    /// keeps its JSON and hash (ID-11).
    Kernel {
        kernel: String,
        #[serde(default)]
        inputs: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        assets: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        values: Vec<f64>,
    },
    /// One copy of `body` per sounding note of `events`, summed, given voices by `alloc`
    /// (RENDER-16). `glide` is a ref to a node outside the body, read where a slide starts. Both
    /// are written only when they aren't the default, so a poly without them keeps its JSON and
    /// hash (ID-11).
    Poly {
        events: String,
        voices: u32,
        #[serde(default, skip_serializing_if = "Alloc::is_default")]
        alloc: Alloc,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        glide: Option<String>,
        body: Body,
    },
    /// The value of `input` one block ago. The only way to close a loop.
    Feedback {
        kind: Kind,
        input: String,
    },
    /// Another module's export, by module key.
    Import {
        module: String,
        export: String,
    },
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

/// How a `poly` gives notes voices (RENDER-16): each field is written only when it isn't the
/// default, today's policy.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Alloc {
    #[serde(default, skip_serializing_if = "is_default")]
    pub mode: Mode,
    /// A voice given a new note is reset; `false`: its gate falls for one sample instead, if it
    /// was open, so envelopes start again from where they are.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub retrigger: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub steal: Steal,
}

impl Default for Alloc {
    fn default() -> Alloc {
        Alloc { mode: Mode::Poly, retrigger: true, steal: Steal::Oldest }
    }
}

impl Alloc {
    pub fn is_default(&self) -> bool {
        *self == Alloc::default()
    }
}

/// How many notes a `poly` plays at once.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// A voice for each note.
    #[default]
    Poly,
    /// One voice: every note starts again, sliding from the last over the glide.
    Mono,
    /// One voice: a note that starts while another is held slides to it without starting again.
    Legato,
}

/// Which sounding note stops when a note finds no free voice.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Steal {
    #[default]
    Oldest,
    /// The voice with the lowest peak over its last block; a tie goes to the oldest.
    Quietest,
    /// None: the new note isn't played.
    None,
}

fn is_default<T: Default + PartialEq>(x: &T) -> bool {
    *x == T::default()
}

fn yes() -> bool {
    true
}

fn is_yes(x: &bool) -> bool {
    *x
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Body {
    pub nodes: Vec<Node>,
    pub out: String,
}

/// What a `time` node reads: song time, never engine time (RENDER-7).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TimeOf {
    /// The song's position in beats.
    Beats,
    /// The song's position in seconds.
    Seconds,
    /// 1 on a frame where the song plays (the transport moves), else 0.
    Playing,
}

impl TimeOf {
    /// The unit of what it reads.
    pub fn unit(self) -> Unit {
        match self {
            TimeOf::Beats => Unit::Beats,
            TimeOf::Seconds => Unit::Seconds,
            TimeOf::Playing => Unit::Unit01,
        }
    }
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
    /// For an error `src` places at a call in the project, where inside a package it happened
    /// (`std/src/filter.ts:26:21`): the technical place, for the Details (the studio plan's Q46).
    /// Boxed, so a `Result` carrying a diagnostic stays small.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner: Option<Box<String>>,
}

impl Diagnostic {
    /// An Error with a code from the contract's table.
    pub fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic { severity: Severity::Error, code, message: message.into(), module: None, node: None, src: None, inner: None }
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
