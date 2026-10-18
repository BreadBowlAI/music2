//! Flatten, type-check and order an IR.
//!
//! The checker trusts nothing about where the IR came from. It resolves imports,
//! sorts the whole song into a processing order (a loop without `feedback` is an
//! error that names the loop), infers every node's output kinds, and checks every
//! connection's kind and unit. It also checks the keys the IR files data under (ID-1,
//! ID-3, ID-5, ID-8): everything is addressed by module key, never by label (ID-11), and that
//! frames, the controls in origins and sidechain records point where they must (ID-15, PKG-3,
//! PKG-13).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::key::{self, ROOT};
use crate::kind::{EventType, Kind, PortKind, Unit};
use crate::sig::{KernelSig, Kernels};
use crate::{Alloc, Body, Diagnostic, Ir, Mode, Module, Node, Op, Origin, TimeOf, split_ref};

/// Output `port` of node `node` in a [`Flat`] or [`Voice`] graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PortRef {
    pub node: usize,
    pub port: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteField {
    Pitch,
    Gate,
    Velocity,
    Pressure,
    Timbre,
    /// The lane's mod wheel, 0 to 1, the same for every voice (RENDER-14's `Midi`, from live input).
    ModWheel,
}

impl NoteField {
    pub const ALL: [(&'static str, NoteField); 6] = [
        ("pitch", NoteField::Pitch),
        ("gate", NoteField::Gate),
        ("velocity", NoteField::Velocity),
        ("pressure", NoteField::Pressure),
        ("timbre", NoteField::Timbre),
        ("modWheel", NoteField::ModWheel),
    ];

    pub fn kind(self) -> Kind {
        match self {
            NoteField::Pitch => Kind::Control(Some(Unit::Semitones)),
            _ => Kind::Control(Some(Unit::Unit01)),
        }
    }
}

/// A kernel input after checking.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// Connected. `channels` is the port's channel count; a mono source feeding a
    /// stereo port is copied to both channels.
    Port { src: PortRef, src_channels: u8, channels: u8 },
    /// Unconnected (the port's default) or connected to a constant.
    Const(f64),
    /// Unconnected plugin parameter: the plugin keeps its own value (from its saved state).
    Unset,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FOp {
    /// A lane, addressed by module key and lane key (ID-6).
    Lane { module: String, lane: String, kind: Kind, default: f64 },
    Time(TimeOf),
    Const(f64),
    /// `inputs` are in the kernel's declared order; `channels` is the count resolved for its
    /// `audio<*>` ports; `assets` and `values` are its settings, as the IR gives them.
    Kernel { kernel: String, channels: u8, inputs: Vec<Input>, assets: Vec<String>, values: Vec<f64> },
    /// A hosted plugin; `inputs` are in its signature's order (`in`, `notes`, parameters).
    /// `state` is where its state is filed below its module, as keys ([`Op::state_path`]).
    Plugin { plugin: String, inputs: Vec<Input>, channels: u8, state: Option<Vec<String>> },
    /// `lane` is the notes lane node feeding the voices; `alloc` how they are given notes, and
    /// `glide` what a slide's time is read from (RENDER-16).
    Poly { lane: usize, voices: u32, voice: Box<Voice>, channels: u8, alloc: Alloc, glide: Option<PortRef> },
    Feedback { input: PortRef, channels: u8 },
    /// Voice graphs only: a field of the voice's note.
    NoteField(NoteField),
    /// Voice graphs only: an output of a node outside the voice.
    Capture(PortRef),
    /// A delay of `frames` samples on an audio or control signal, which the runtime's planner
    /// inserts so every path into a node arrives aligned (RENDER-18). Never in a checked graph.
    Delay { input: PortRef, frames: u32 },
    /// The notes of lane node `lane`, `frames` samples late (RENDER-18): inserted as [`FOp::Delay`].
    NoteDelay { lane: usize, frames: u32 },
}

impl FOp {
    /// Nodes in the same graph this op reads in the current block.
    pub fn deps(&self) -> Vec<usize> {
        match self {
            FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. } => inputs
                .iter()
                .filter_map(|i| match i {
                    Input::Port { src, .. } => Some(src.node),
                    Input::Const(_) | Input::Unset => None,
                })
                .collect(),
            FOp::Poly { lane, voice, glide, .. } => {
                let mut d = vec![*lane];
                d.extend(glide.map(|g| g.node));
                d.extend(voice.nodes.iter().filter_map(|n| match n.op {
                    FOp::Capture(p) => Some(p.node),
                    _ => None,
                }));
                d
            }
            FOp::Delay { input, .. } => vec![input.node],
            FOp::NoteDelay { lane, .. } => vec![*lane],
            _ => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FNode {
    /// `<module key>/<node id>`.
    pub name: String,
    /// The module's key.
    pub module: String,
    pub src: Option<String>,
    /// Where it was written. Constants with an origin stay addressable (never folded
    /// into kernel inputs) so their value can change while the song plays.
    pub origin: Option<Origin>,
    pub op: FOp,
    pub outputs: Vec<Kind>,
}

/// The graph instantiated once per voice by `poly`.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub nodes: Vec<FNode>,
    pub order: Vec<usize>,
    pub out: PortRef,
    /// The outputs that feed a sidechain input inside the voice ([`Flat::sidechain`]).
    pub sidechain: Vec<PortRef>,
}

/// A whole song after checking: one graph, typed and ordered.
#[derive(Clone, Debug, PartialEq)]
pub struct Flat {
    pub nodes: Vec<FNode>,
    /// Processing order of the nodes the output depends on.
    pub order: Vec<usize>,
    pub out: PortRef,
    pub out_kind: Kind,
    /// The outputs that feed a component's sidechain input (PKG-13), as the frames recorded
    /// them, in module and frame order: where monitoring mute and solo must not reach (MON-2).
    /// A signal made inside a voice is in that voice's list. Empty when no sidechain input is
    /// used, and the same for the same code.
    pub sidechain: Vec<PortRef>,
    /// Where monitoring mute and solo act (MON-1): every place a track's audio enters another
    /// module, other than a sidechain input (MON-2), and the song's output when it reads a track.
    /// In node order, the output last; the same for the same code. Every plan puts an overlay there.
    pub overlays: Vec<OverlaySite>,
    /// The tracks muted in the song (MON-5), by key, sorted: silent in every render.
    pub muted: Vec<String>,
}

/// One place a track's audio enters another module (MON-1): where an overlay goes.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlaySite {
    /// The track whose audio it carries: its source's module.
    pub module: String,
    /// The module that reads it: its consumer's, or the root's for the song's output.
    pub into: String,
    /// What it reads.
    pub src: PortRef,
    pub consumer: Consumer,
    /// The source's channels.
    pub channels: u8,
}

/// Which input reads an overlay. The same cases as the runtime's edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    /// A kernel's input, or a plugin's main audio input, by its place in the node's inputs.
    Input { node: usize, input: u16 },
    /// What a `poly`'s voices read from outside, by the capture's place in node order.
    Capture { poly: usize, index: u16 },
    Feedback { node: usize },
    /// The song's output.
    Out,
}

/// Check `ir` against the kernels available.
pub fn check(ir: &Ir, kernels: &dyn Kernels) -> Result<Flat, Vec<Diagnostic>> {
    let mut ck = Checker::new(ir, kernels);
    let flat = ck.run(ir);
    let mut seen = HashSet::new();
    ck.diags.retain(|d| seen.insert((d.message.clone(), d.module.clone(), d.node.clone())));
    match flat {
        Some(f) if ck.diags.is_empty() => Ok(f),
        _ => Err(ck.diags),
    }
}

// ---------------------------------------------------------------- internals

/// How refs and lanes resolve in the graph being checked: the whole song, or a voice.
trait Scope<'a> {
    fn resolve(&mut self, ck: &Checker<'a>, m: &'a Module, r: &str) -> Result<PortRef, String>;
    fn lane(&self, m: &'a Module, lane: &str) -> Result<(FOp, Kind), String>;
    fn in_voice(&self) -> bool;
    /// Nodes appended after the scope's own (captures), with their kinds.
    fn extras(&self) -> Vec<(FOp, Kind)>;
}

struct Song;

impl<'a> Scope<'a> for Song {
    fn resolve(&mut self, ck: &Checker<'a>, m: &'a Module, r: &str) -> Result<PortRef, String> {
        ck.resolve(&m.key, r, 0)
    }

    fn lane(&self, m: &'a Module, lane: &str) -> Result<(FOp, Kind), String> {
        let d = m.lanes.iter().find(|l| l.key == lane).ok_or_else(|| {
            let have: Vec<&str> = m.lanes.iter().map(|l| l.key.as_str()).collect();
            format!("no lane `{lane}` declared (lanes: {})", if have.is_empty() { "none".into() } else { have.join(", ") })
        })?;
        let op = FOp::Lane { module: m.key.clone(), lane: lane.to_string(), kind: d.kind, default: d.default.unwrap_or(0.0) };
        Ok((op, d.kind))
    }

    fn in_voice(&self) -> bool {
        false
    }

    fn extras(&self) -> Vec<(FOp, Kind)> {
        vec![]
    }
}

struct VoiceScope<'a, 'k> {
    local: HashMap<&'a str, (usize, &'a Node)>,
    captures: Vec<PortRef>,
    outer_kinds: &'k [Option<Vec<Kind>>],
}

impl<'a> Scope<'a> for VoiceScope<'a, '_> {
    fn resolve(&mut self, ck: &Checker<'a>, m: &'a Module, r: &str) -> Result<PortRef, String> {
        let (id, port) = split_ref(r);
        if let Some(&(i, node)) = self.local.get(id) {
            return Ok(PortRef { node: i, port: ck.port_index(node, port)? });
        }
        let outer = ck.resolve(&m.key, r, 0)?;
        let ci = self.captures.iter().position(|c| *c == outer).unwrap_or_else(|| {
            self.captures.push(outer);
            self.captures.len() - 1
        });
        Ok(PortRef { node: self.local.len() + ci, port: 0 })
    }

    fn lane(&self, _: &'a Module, lane: &str) -> Result<(FOp, Kind), String> {
        let (_, f) = NoteField::ALL
            .iter()
            .find(|(n, _)| *n == lane)
            .ok_or_else(|| format!("inside poly, lanes are note fields (pitch, gate, velocity, pressure, timbre, modWheel), not `{lane}`"))?;
        Ok((FOp::NoteField(*f), f.kind()))
    }

    fn in_voice(&self) -> bool {
        true
    }

    fn extras(&self) -> Vec<(FOp, Kind)> {
        self.captures
            .iter()
            .map(|c| {
                let k = self.outer_kinds.get(c.node).and_then(|k| k.as_ref()).and_then(|k| k.get(c.port)).copied();
                (FOp::Capture(*c), k.unwrap_or(Kind::Control(None)))
            })
            .collect()
    }
}

/// A checked graph: nodes, processing order, and output kinds per node.
type Checked = (Vec<FNode>, Vec<usize>, Vec<Option<Vec<Kind>>>);

enum Res {
    None,
    Kernel(Vec<(String, PortRef)>),
    Poly { events: PortRef, captures: Vec<PortRef>, glide: Option<PortRef> },
    Feedback(PortRef),
    Broken,
}

struct Checker<'a> {
    kernels: &'a dyn Kernels,
    modules: HashMap<&'a str, &'a Module>,
    /// Top-level nodes by module and id, imports included.
    nodes: HashMap<&'a str, HashMap<&'a str, &'a Node>>,
    /// Index of each non-import top-level node in the flat graph.
    slot_of: HashMap<(&'a str, &'a str), usize>,
    diags: Vec<Diagnostic>,
}

/// A `code.check` error at a node, or at the module.
fn diag(m: &Module, n: Option<&Node>, message: impl Into<String>) -> Diagnostic {
    coded("code.check", m, n, message)
}

fn coded(code: &'static str, m: &Module, n: Option<&Node>, message: impl Into<String>) -> Diagnostic {
    let mut d = Diagnostic::error(code, message).module(m.key.clone()).at(n.and_then(|n| n.src.clone()).or_else(|| m.src.clone()));
    d.node = n.map(|n| n.id.clone());
    d
}

impl<'a> Checker<'a> {
    fn new(ir: &'a Ir, kernels: &'a dyn Kernels) -> Checker<'a> {
        let mut ck = Checker { kernels, modules: HashMap::new(), nodes: HashMap::new(), slot_of: HashMap::new(), diags: vec![] };
        if ir.ir != crate::IR_VERSION {
            let e = format!("IR version {} is not supported (expected {})", ir.ir, crate::IR_VERSION);
            ck.diags.push(Diagnostic::error("code.check", e));
        }
        if ir.root.module != ROOT {
            ck.diags.push(Diagnostic::error("key.invalid", format!("the root module's key must be `{ROOT}`, not `{}`", ir.root.module)));
        }
        for m in &ir.modules {
            ck.keys(m);
            let map = ck.nodes.entry(&m.key).or_default();
            for n in &m.nodes {
                if map.insert(&n.id, n).is_some() {
                    ck.diags.push(diag(m, Some(n), format!("node id `{}` is used twice", n.id)));
                }
            }
            for l in &m.lanes {
                if l.default.is_some() && !matches!(l.kind, Kind::Control(_)) {
                    ck.diags.push(diag(m, None, format!("lane `{}`: only control lanes have defaults", l.key)));
                }
            }
            ck.frames(m);
        }
        ck.state_paths(ir);
        ck
    }

    /// Frames, the frames nodes and declared controls name, and sidechain records point where
    /// they must (ID-15, PKG-3, PKG-13). Frame keys follow the key rule (ID-1).
    fn frames(&mut self, m: &'a Module) {
        let n = m.frames.len();
        let mut ids = HashSet::new();
        walk(&m.nodes, &mut |node| {
            ids.insert(node.id.as_str());
        });
        for (i, f) in m.frames.iter().enumerate() {
            if f.parent.is_some_and(|p| p as usize >= i) {
                self.diags.push(diag(m, None, format!("frame {i} (site {}) names a parent that isn't an earlier frame", f.site)));
            }
            if let Some(k) = &f.key
                && let Some(e) = key::invalid("the key() frame key", k)
            {
                self.diags.push(coded("key.invalid", m, None, e));
            }
            for s in &f.sidechain {
                if !ids.contains(split_ref(&s.source).0) {
                    let e = format!("frame {i} (site {}): sidechain input `{}` reads `{}`, which isn't a node of the module", f.site, s.input, s.source);
                    self.diags.push(diag(m, None, e));
                }
            }
        }
        let mut errs = vec![];
        walk(&m.nodes, &mut |node| {
            if let Some(f) = node.frame
                && f as usize >= n
            {
                errs.push(diag(m, Some(node), format!("node `{}` names frame {f}, but the module has {n}", node.id)));
            }
            let Some(c) = node.origin.as_ref().and_then(|o| o.control.as_ref()) else { return };
            match m.frames.get(c.frame as usize) {
                None => errs.push(diag(m, Some(node), format!("control `{}` names frame {}, but the module has {n}", c.slot, c.frame))),
                Some(f) if f.component.is_none() => {
                    errs.push(diag(m, Some(node), format!("control `{}` names frame {}, which isn't a component's", c.slot, c.frame)))
                }
                Some(_) => {}
            }
        });
        self.diags.extend(errs);
    }

    /// Where each sidechain record's signal is: in the song graph, or in the voice of the poly
    /// whose body made it (node ids are unique within a module, voices included).
    fn sidechains(&mut self, ir: &'a Ir, nodes: &mut [FNode]) -> Vec<PortRef> {
        let mut top = vec![];
        for m in &ir.modules {
            for s in m.frames.iter().flat_map(|f| &f.sidechain) {
                let (id, port) = split_ref(&s.source);
                let in_body = m.nodes.iter().find_map(|n| match &n.op {
                    Op::Poly { body, .. } => body.nodes.iter().position(|b| b.id == id).map(|i| (n, &body.nodes[i], i)),
                    _ => None,
                });
                let found = match in_body {
                    // A ref to no node is reported by `frames`.
                    None if !self.nodes.get(m.key.as_str()).is_some_and(|ns| ns.contains_key(id)) => continue,
                    Some((poly, node, i)) => self.port_index(node, port).map(|port| {
                        let at = self.slot_of[&(m.key.as_str(), poly.id.as_str())];
                        if let FOp::Poly { voice, .. } = &mut nodes[at].op {
                            voice.sidechain.push(PortRef { node: i, port });
                        }
                    }),
                    None => self.resolve(&m.key, &s.source, 0).map(|p| top.push(p)),
                };
                if let Err(e) = found {
                    self.diags.push(diag(m, None, format!("sidechain input `{}`: {e}", s.input)));
                }
            }
        }
        top
    }

    /// Module, lane, plugin and frame keys follow the key rule and are unique where they
    /// must be (ID-1, ID-3, ID-5, ID-6). The SDK reports these first, with both sites; this
    /// holds for an IR from anywhere.
    fn keys(&mut self, m: &'a Module) {
        if let Some(e) = key::invalid("the module key", &m.key) {
            self.diags.push(coded("key.invalid", m, None, e));
        } else if m.key == ROOT && m.track {
            self.diags.push(coded("key.invalid", m, None, format!("`{ROOT}` is reserved for the root module; give the track another key")));
        }
        if self.modules.insert(&m.key, m).is_some() {
            self.diags.push(coded("key.duplicate", m, None, format!("two modules have the key `{}`", m.key)));
        }
        let mut lanes = HashSet::new();
        for l in &m.lanes {
            if let Some(e) = key::invalid("the lane key", &l.key) {
                self.diags.push(coded("key.invalid", m, None, e));
            }
            if !lanes.insert(&l.key) {
                self.diags.push(coded("lane.conflict", m, None, format!("lane `{}` is declared twice", l.key)));
            }
        }
        let mut plugins: HashMap<(&[String], &str), &Node> = HashMap::new();
        for n in &m.nodes {
            let Op::Plugin { key: k, frames, .. } = &n.op else { continue };
            for f in frames {
                if let Some(e) = key::invalid("the key() frame key", f) {
                    self.diags.push(coded("key.invalid", m, Some(n), e));
                }
            }
            let Some(k) = k else { continue };
            if let Some(e) = key::invalid("the plugin key", k) {
                self.diags.push(coded("key.invalid", m, Some(n), e));
            } else if let Some(first) = plugins.insert((frames.as_slice(), k.as_str()), n) {
                let at = first.src.as_deref().map_or(String::new(), |s| format!(" (first at {s})"));
                let e = format!("the plugin key `{k}` is used twice in one frame{at}; rename one");
                self.diags.push(coded("key.duplicate", m, Some(n), e));
            }
        }
    }

    /// Two plugins must never share a state file (ID-8). A plugin with no key takes its
    /// innermost `key()` frame's key as its file name, which only the one stateful plugin of a
    /// frame may: one beside another stateful plugin in its frame needs a key. And one beside a
    /// plugin keyed like the frame, in the frame around it, collides. Every plugin with a state
    /// path counts as stateful, since the build never reads data.
    fn state_paths(&mut self, ir: &'a Ir) {
        for m in &ir.modules {
            let mut held: HashMap<&[String], Vec<&Node>> = HashMap::new();
            for n in &m.nodes {
                if let (Some(_), Op::Plugin { frames, .. }) = (n.op.state_path(), &n.op) {
                    held.entry(frames.as_slice()).or_default().push(n);
                }
            }
            let mut told = HashSet::new();
            let mut seen: HashMap<Vec<String>, &Node> = HashMap::new();
            for n in &m.nodes {
                let (Some(path), Op::Plugin { key: own, frames, .. }) = (n.op.state_path(), &n.op) else { continue };
                if own.is_none()
                    && let Some(other) = held[frames.as_slice()].iter().find(|o| !std::ptr::eq(**o, n))
                {
                    // Once per frame: every unkeyed plugin in it needs the same fix.
                    if told.insert(frames.as_slice()) {
                        let at = other.src.as_deref().map_or(String::new(), |s| format!(" (the other is at {s})"));
                        let frame = frames.last().expect("a plugin with no key has a state path only in a frame");
                        let e = format!(
                            "a plugin with no key keeps its state under its key() frame's key, `{frame}`, but that frame holds \
                             another plugin with saved state{at}; give it a key, plugin(id, {{ key: \"…\" }}), or wrap one in key()"
                        );
                        self.diags.push(coded("plugin.state-collision", m, Some(n), e));
                    }
                    continue;
                }
                let Some(first) = seen.get(&path) else {
                    seen.insert(path, n);
                    continue;
                };
                // Two plugins with one key in one frame are already `key.duplicate`.
                let same_key = matches!(&first.op, Op::Plugin { key: Some(k), frames: f, .. } if Some(k) == own.as_ref() && f == frames);
                if same_key {
                    continue;
                }
                let at = first.src.as_deref().map_or(String::new(), |s| format!(" (the other is at {s})"));
                self.diags.push(coded(
                    "plugin.state-collision",
                    m,
                    Some(n),
                    format!("two plugins in `{}` would keep their state in one place, `{}`{at}; wrap one in key()", m.key, path.join("/")),
                ));
            }
        }
    }

    fn run(&mut self, ir: &'a Ir) -> Option<Flat> {
        let mut items = vec![];
        for m in &ir.modules {
            for n in &m.nodes {
                if !matches!(n.op, Op::Import { .. }) {
                    self.slot_of.insert((&m.key, &n.id), items.len());
                    items.push((m, n));
                }
            }
        }
        let (nodes, mut order, _) = self.check_graph(&items, &mut Song)?;

        for m in &ir.modules {
            for (name, r) in &m.exports {
                if let Err(e) = self.resolve(&m.key, r, 0) {
                    self.diags.push(diag(m, None, format!("export `{name}`: {e}")));
                }
            }
        }
        let Some(root) = self.modules.get(ir.root.module.as_str()).copied() else {
            self.diags.push(Diagnostic::error("code.check", format!("the root module `{}` does not exist", ir.root.module)));
            return None;
        };
        let Some(r) = root.exports.get(&ir.root.export) else {
            self.diags.push(diag(root, None, format!("the root export `{}` does not exist", ir.root.export)));
            return None;
        };
        let out = self.resolve(&root.key, r, 0).ok()?;
        let out_kind = *nodes[out.node].outputs.get(out.port)?;
        if !matches!(out_kind, Kind::Audio(_)) {
            self.diags.push(diag(root, None, format!("the song's output must be audio, not {out_kind}")));
        }

        // Keep only what the output depends on (feedback inputs included).
        let mut reach = vec![false; nodes.len()];
        let mut stack = vec![out.node];
        while let Some(i) = stack.pop() {
            if std::mem::replace(&mut reach[i], true) {
                continue;
            }
            stack.extend(nodes[i].op.deps());
            if let FOp::Feedback { input, .. } = nodes[i].op {
                stack.push(input.node);
            }
        }
        order.retain(|&i| reach[i]);
        let mut nodes = nodes;
        let sidechain = self.sidechains(ir, &mut nodes);
        let overlays = self.overlays(ir, &nodes, &reach, out, out_kind);
        let muted = self.muted(ir);
        Some(Flat { nodes, order, out, out_kind, sidechain, overlays, muted })
    }

    /// The tracks muted in the song, sorted. `muted` only on a track: never on the root module,
    /// which can't be muted (MON-1), nor on any other module, which no overlay would carry.
    fn muted(&mut self, ir: &'a Ir) -> Vec<String> {
        let mut muted = vec![];
        for m in ir.modules.iter().filter(|m| m.muted) {
            if m.key == ROOT {
                self.diags.push(diag(m, None, format!("the root module `{ROOT}` can't be muted in the song; mute a track instead")));
            } else if !m.track {
                self.diags.push(diag(m, None, format!("`{}` isn't a track: only a track can be muted in the song", m.key)));
            } else {
                muted.push(m.key.clone());
            }
        }
        muted.sort();
        muted
    }

    /// The signals the sidechain inputs of the frames around `frame` received (PKG-13): what an
    /// input made inside them reads without being heard, so no overlay acts on it (MON-2). Only
    /// signals of the song's graph: one made in a voice never leaves its module.
    fn side_sources(&self, m: &Module, frame: Option<u32>) -> Vec<PortRef> {
        m.chain(frame).iter().flat_map(|f| &f.sidechain).filter_map(|s| self.resolve(&m.key, &s.source, 0).ok()).collect()
    }

    /// Where overlays go (MON-1): each input of a node the output reaches, and each capture of a
    /// voice, that reads the audio of a track other than its own module, unless it is a sidechain
    /// input (MON-2); then the song's output when it reads a track. A capture that a sidechain
    /// input reads isn't one: the sidechain hears it unmuted.
    fn overlays(&self, ir: &'a Ir, nodes: &[FNode], reach: &[bool], out: PortRef, out_kind: Kind) -> Vec<OverlaySite> {
        let track = |m: &str| self.modules.get(m).is_some_and(|m| m.track);
        let crosses = |p: PortRef, into: &str| {
            let n = &nodes[p.node];
            n.module != into && track(&n.module) && matches!(n.outputs.get(p.port), Some(Kind::Audio(_)))
        };
        let site = |p: PortRef, into: &str, consumer: Consumer| {
            let channels = nodes[p.node].outputs[p.port].channels();
            OverlaySite { module: nodes[p.node].module.clone(), into: into.to_string(), src: p, consumer, channels }
        };
        let mut sites = vec![];
        for m in &ir.modules {
            for n in &m.nodes {
                let Some(&i) = self.slot_of.get(&(m.key.as_str(), n.id.as_str())) else { continue };
                if !reach[i] {
                    continue;
                }
                let into = m.key.as_str();
                match &nodes[i].op {
                    FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. } => {
                        // A plugin's audio inputs other than its main one are sidechain ports (PKG-13).
                        let main = match &nodes[i].op {
                            FOp::Plugin { plugin, .. } => self.kernels.plugin(plugin).and_then(|s| s.inputs.iter().position(|p| p.name == "in")),
                            _ => None,
                        };
                        let side = self.side_sources(m, n.frame);
                        let plugin = matches!(nodes[i].op, FOp::Plugin { .. });
                        for (k, input) in inputs.iter().enumerate() {
                            if let Input::Port { src, .. } = input
                                && crosses(*src, into)
                                && !side.contains(src)
                                && (!plugin || main == Some(k))
                            {
                                sites.push(site(*src, into, Consumer::Input { node: i, input: k as u16 }));
                            }
                        }
                    }
                    FOp::Feedback { input, .. } => {
                        if crosses(*input, into) && !self.side_sources(m, n.frame).contains(input) {
                            sites.push(site(*input, into, Consumer::Feedback { node: i }));
                        }
                    }
                    FOp::Poly { voice, .. } => {
                        let Op::Poly { body, .. } = &n.op else { continue };
                        let captures = voice.nodes.iter().enumerate().filter_map(|(j, v)| if let FOp::Capture(p) = v.op { Some((j, p)) } else { None });
                        for (index, (j, p)) in captures.enumerate() {
                            if !crosses(p, into) {
                                continue;
                            }
                            // The voice's nodes are its body's, in order, then its captures.
                            let heard_as_side = voice.nodes.iter().zip(&body.nodes).any(|(v, b)| {
                                v.op.deps().contains(&j) || matches!(v.op, FOp::Feedback { input, .. } if input.node == j)
                            } && self.side_sources(m, b.frame).contains(&p));
                            if !heard_as_side {
                                sites.push(site(p, into, Consumer::Capture { poly: i, index: index as u16 }));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if matches!(out_kind, Kind::Audio(_)) && crosses(out, ROOT) {
            sites.push(site(out, ROOT, Consumer::Out));
        }
        sites
    }

    /// Resolve a ref in the module keyed `module` to a top-level node, following imports.
    fn resolve(&self, module: &str, r: &str, depth: usize) -> Result<PortRef, String> {
        let (id, port) = split_ref(r);
        let node = self.nodes.get(module).and_then(|m| m.get(id)).ok_or_else(|| format!("no node `{id}` in module `{module}`"))?;
        if let Op::Import { module: target, export } = &node.op {
            if depth > 32 {
                return Err("imports go round in a circle".into());
            }
            if port != "out" {
                return Err(format!("`{id}` is an import and has one output, `out`"));
            }
            let m = self.modules.get(target.as_str()).ok_or_else(|| format!("no module `{target}`"))?;
            let r = m.exports.get(export).ok_or_else(|| {
                let have: Vec<&str> = m.exports.keys().map(String::as_str).collect();
                format!("module `{target}` has no export `{export}` (exports: {})", have.join(", "))
            })?;
            return self.resolve(target, r, depth + 1);
        }
        let node_idx = self.slot_of[&(module, id)];
        Ok(PortRef { node: node_idx, port: self.port_index(node, port)? })
    }

    fn port_index(&self, node: &Node, port: &str) -> Result<usize, String> {
        let sig = match &node.op {
            Op::Kernel { kernel, .. } => self.kernels.kernel(kernel).map(|s| (kernel, s)),
            Op::Plugin { plugin, .. } => self.kernels.plugin(plugin).map(|s| (plugin, s)),
            _ => None,
        };
        if let Some((kernel, sig)) = sig {
            return sig.output_index(port).ok_or_else(|| {
                let have: Vec<&str> = sig.outputs.iter().map(|p| p.name.as_str()).collect();
                format!("`{kernel}` has no output `{port}` (outputs: {})", have.join(", "))
            });
        }
        if port == "out" { Ok(0) } else { Err(format!("`{}` has one output, `out`, not `{port}`", node.id)) }
    }

    /// Check one graph: the whole song, or a voice body. Returns its nodes, processing
    /// order and output kinds (indexed like the nodes).
    fn check_graph(&mut self, items: &[(&'a Module, &'a Node)], scope: &mut dyn Scope<'a>) -> Option<Checked> {
        let n = items.len();
        let before = self.diags.len();

        // 1. Resolve every reference.
        let mut res = Vec::with_capacity(n);
        for &(m, node) in items {
            let r = match &node.op {
                Op::Plugin { .. } if scope.in_voice() => {
                    self.diags.push(diag(m, Some(node), "plugins can't be inside poly: an instrument plugin plays all the notes itself"));
                    Res::Broken
                }
                Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => {
                    let mut v = vec![];
                    for (port, r) in inputs {
                        match scope.resolve(self, m, r) {
                            Ok(p) => v.push((port.clone(), p)),
                            Err(e) => self.diags.push(diag(m, Some(node), format!("input `{port}`: {e}"))),
                        }
                    }
                    Res::Kernel(v)
                }
                Op::Poly { .. } if scope.in_voice() => {
                    self.diags.push(diag(m, Some(node), "poly can't be used inside another poly"));
                    Res::Broken
                }
                Op::Poly { events, body, glide, .. } => {
                    let events = scope.resolve(self, m, events);
                    let glide = match glide.as_deref().map(|g| scope.resolve(self, m, g)) {
                        None => Some(None),
                        Some(Ok(p)) => Some(Some(p)),
                        Some(Err(e)) => {
                            self.diags.push(diag(m, Some(node), format!("glide: {e}")));
                            None
                        }
                    };
                    let mut captures = vec![];
                    let local: HashSet<&str> = body.nodes.iter().map(|b| b.id.as_str()).collect();
                    for r in body_refs(body) {
                        if local.contains(split_ref(r).0) {
                            continue;
                        }
                        match scope.resolve(self, m, r) {
                            Ok(p) if !captures.contains(&p) => captures.push(p),
                            Ok(_) => {}
                            Err(e) => self.diags.push(diag(m, Some(node), format!("voice reads `{r}`: {e}"))),
                        }
                    }
                    match (events, glide) {
                        (Ok(events), Some(glide)) => Res::Poly { events, captures, glide },
                        (Ok(_), None) => Res::Broken,
                        (Err(e), _) => {
                            self.diags.push(diag(m, Some(node), format!("events: {e}")));
                            Res::Broken
                        }
                    }
                }
                Op::Feedback { input, .. } => match scope.resolve(self, m, input) {
                    Ok(p) => Res::Feedback(p),
                    Err(e) => {
                        self.diags.push(diag(m, Some(node), format!("feedback input: {e}")));
                        Res::Broken
                    }
                },
                Op::Import { .. } => {
                    self.diags.push(diag(m, Some(node), "imports can't be inside poly; import outside it and read it from the voice"));
                    Res::Broken
                }
                _ => Res::None,
            };
            res.push(r);
        }

        // 2. Order (feedback inputs don't count: they are read one block late).
        let deps: Vec<Vec<usize>> = res
            .iter()
            .map(|r| {
                let mut d: Vec<usize> = match r {
                    Res::Kernel(v) => v.iter().map(|(_, p)| p.node).collect(),
                    Res::Poly { events, captures, glide } => {
                        std::iter::once(events.node).chain(captures.iter().map(|c| c.node)).chain(glide.map(|g| g.node)).collect()
                    }
                    _ => vec![],
                };
                d.retain(|&x| x < n);
                d.sort_unstable();
                d.dedup();
                d
            })
            .collect();
        let order = match topo_order(&deps) {
            Ok(o) => o,
            Err(stuck) => {
                let cycle = find_cycle(&deps, &stuck);
                let describe = |i: usize| {
                    let (m, node) = items[i];
                    let what = match &node.op {
                        Op::Kernel { kernel, .. } => format!(" ({kernel})"),
                        Op::Plugin { plugin, .. } => format!(" ({plugin})"),
                        op => format!(" ({})", op.name()),
                    };
                    format!("{}/{}{}", m.key, node.id, what)
                };
                let path: Vec<String> = cycle.iter().map(|&i| describe(i)).collect();
                let (m, node) = items[cycle[0]];
                self.diags.push(diag(
                    m,
                    Some(node),
                    format!("these nodes form a loop: {} → {}. Break it with feedback(), which delays one block", path.join(" → "), path[0]),
                ));
                return None;
            }
        };

        // 3. Types, in order.
        let extras = scope.extras();
        let mut kinds: Vec<Option<Vec<Kind>>> = vec![None; n];
        kinds.extend(extras.iter().map(|(_, k)| Some(vec![*k])));
        let mut ops: Vec<Option<FOp>> = (0..n).map(|_| None).collect();
        let kind_of = |kinds: &[Option<Vec<Kind>>], p: PortRef| kinds.get(p.node).and_then(|k| k.as_ref()).and_then(|k| k.get(p.port)).copied();
        for &i in &order {
            let (m, node) = items[i];
            let typed: Result<(FOp, Vec<Kind>), String> = match (&node.op, &res[i]) {
                (Op::Lane { lane }, _) => scope.lane(m, lane).map(|(op, k)| (op, vec![k])),
                (Op::Time { of }, _) => Ok((FOp::Time(*of), vec![Kind::Control(Some(of.unit()))])),
                (Op::Const { value, unit }, _) => Ok((FOp::Const(*value), vec![Kind::Control(*unit)])),
                (Op::Kernel { kernel, .. } | Op::Plugin { plugin: kernel, .. }, Res::Kernel(conns)) => {
                    let is_plugin = matches!(node.op, Op::Plugin { .. });
                    let sig = if is_plugin { self.kernels.plugin(kernel) } else { self.kernels.kernel(kernel) };
                    let Some(sig) = sig else {
                        if !is_plugin && self.kernels.offline(kernel) {
                            let message = format!(
                                "`{kernel}` is an offline kernel: music2 runs it to make a clip's render, and song code can't call it"
                            );
                            self.diags.push(coded("kernel.offline", m, Some(node), message));
                            continue;
                        }
                        let what = if is_plugin { "plugin (is it installed?)" } else { "kernel" };
                        self.diags.push(diag(m, Some(node), format!("unknown {what} `{kernel}`")));
                        continue;
                    };
                    // The planner compensates latency at the top level (RENDER-18), never inside a
                    // voice, whose notes start at their own sample.
                    if sig.latency > 0 && scope.in_voice() {
                        self.diags.push(diag(
                            m,
                            Some(node),
                            format!("`{kernel}` adds {} samples of latency, which can't be made up for inside poly; use it after the voices", sig.latency),
                        ));
                        continue;
                    }
                    let mut typed_conns = HashMap::new();
                    let mut upstream_broken = false;
                    for (port, p) in conns {
                        match kind_of(&kinds, *p) {
                            Some(k) => {
                                typed_conns.insert(port.as_str(), (*p, k));
                            }
                            None => upstream_broken = true,
                        }
                    }
                    if upstream_broken {
                        continue;
                    }
                    let unknown: Vec<&str> = conns.iter().map(|(p, _)| p.as_str()).filter(|p| !sig.inputs.iter().any(|s| s.name == *p)).collect();
                    let notes_not_lane = is_plugin
                        && typed_conns.get("notes").is_some_and(|(p, _)| !matches!(ops.get(p.node), Some(Some(FOp::Lane { .. }))));
                    if notes_not_lane {
                        Err("a plugin's notes must come straight from a notes lane".into())
                    } else if !unknown.is_empty() {
                        let have: Vec<&str> = sig.inputs.iter().map(|p| p.name.as_str()).collect();
                        Err(format!("`{kernel}` has no input `{}` (inputs: {})", unknown.join("`, `"), have.join(", ")))
                    } else {
                        type_kernel(sig, &typed_conns, is_plugin).map(|(mut inputs, channels, outs)| {
                            for inp in &mut inputs {
                                if let Input::Port { src, .. } = inp
                                    && let Some(Some(FOp::Const(v))) = ops.get(src.node)
                                    && items.get(src.node).is_some_and(|(_, n)| n.origin.is_none())
                                {
                                    *inp = Input::Const(*v);
                                }
                            }
                            let op = match &node.op {
                                Op::Plugin { .. } => FOp::Plugin {
                                    plugin: kernel.clone(),
                                    inputs,
                                    channels: outs.first().map_or(2, |k| k.channels()),
                                    state: node.op.state_path(),
                                },
                                Op::Kernel { assets, values, .. } => {
                                    FOp::Kernel { kernel: kernel.clone(), channels, inputs, assets: assets.clone(), values: values.clone() }
                                }
                                _ => unreachable!("a kernel or a plugin"),
                            };
                            (op, outs)
                        })
                    }
                }
                (Op::Poly { voices, body, alloc, .. }, Res::Poly { events, glide, .. }) => {
                    let lane_ok = matches!(ops.get(events.node), Some(Some(FOp::Lane { .. })));
                    let seconds = Kind::Control(Some(Unit::Seconds));
                    match kind_of(&kinds, *events) {
                        None => continue,
                        Some(Kind::Events(EventType::Note)) if lane_ok => {
                            let glide_kind = glide.and_then(|g| kind_of(&kinds, g));
                            if !(1..=128).contains(voices) {
                                Err(format!("voices must be 1..128, not {voices}"))
                            } else if alloc.mode != Mode::Poly && *voices != 1 {
                                Err(format!("a poly in {:?} mode has one voice, not {voices}", alloc.mode).to_lowercase())
                            } else if let Some(k) = glide_kind.filter(|&k| !fits(k, seconds)) {
                                Err(format!("glide must be a time in seconds, not {k}"))
                            } else {
                                match self.check_voice(m, body, &kinds) {
                                    Some((voice, Kind::Audio(c))) => {
                                        let (voice, alloc, glide) = (Box::new(voice), *alloc, *glide);
                                        Ok((FOp::Poly { lane: events.node, voices: *voices, voice, channels: c, alloc, glide }, vec![Kind::Audio(c)]))
                                    }
                                    Some((_, k)) => Err(format!("a voice must output audio, not {k}")),
                                    None => continue,
                                }
                            }
                        }
                        Some(Kind::Events(_)) => Err("poly needs a notes lane as its events".into()),
                        Some(k) => Err(format!("poly needs events<note>, got {k}")),
                    }
                }
                (Op::Feedback { kind, input }, Res::Feedback(p)) => {
                    let _ = input;
                    Ok((FOp::Feedback { input: *p, channels: kind.channels() }, vec![*kind]))
                }
                _ => continue,
            };
            match typed {
                Ok((op, outs)) => {
                    ops[i] = Some(op);
                    kinds[i] = Some(outs);
                }
                Err(e) => self.diags.push(diag(m, Some(node), e)),
            }
        }

        // Feedback inputs are typed only now.
        for (i, &(m, node)) in items.iter().enumerate() {
            if let (Op::Feedback { kind, .. }, Res::Feedback(p)) = (&node.op, &res[i])
                && let Some(src) = kind_of(&kinds, *p)
                && !fits(src, *kind)
            {
                self.diags.push(diag(m, Some(node), format!("feedback is declared {kind} but its input is {src}")));
            }
        }

        if self.diags.len() > before {
            return None;
        }
        let mut nodes: Vec<FNode> = items
            .iter()
            .zip(ops)
            .zip(&kinds)
            .map(|((&(m, node), op), k)| FNode {
                name: format!("{}/{}", m.key, node.id),
                module: m.key.clone(),
                src: node.src.clone(),
                origin: node.origin.clone(),
                op: op.unwrap_or(FOp::Const(0.0)),
                outputs: k.clone().unwrap_or_default(),
            })
            .collect();
        let mut order = order;
        for (j, (op, k)) in extras.into_iter().enumerate() {
            let module = items.first().map(|(m, _)| m.key.clone()).unwrap_or_default();
            nodes.push(FNode { name: format!("{module}/capture{j}"), module, src: None, origin: None, op, outputs: vec![k] });
            order.insert(j, n + j);
        }
        Some((nodes, order, kinds))
    }

    fn check_voice(&mut self, m: &'a Module, body: &'a Body, outer_kinds: &[Option<Vec<Kind>>]) -> Option<(Voice, Kind)> {
        let items: Vec<(&'a Module, &'a Node)> = body.nodes.iter().map(|n| (m, n)).collect();
        let local = body.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), (i, n))).collect();
        let mut scope = VoiceScope { local, captures: vec![], outer_kinds };
        let (nodes, order, kinds) = self.check_graph(&items, &mut scope)?;
        let out = match scope.resolve(self, m, &body.out) {
            Ok(p) => p,
            Err(e) => {
                self.diags.push(diag(m, None, format!("voice output: {e}")));
                return None;
            }
        };
        let kind = kinds.get(out.node).and_then(|k| k.as_ref()).and_then(|k| k.get(out.port)).copied()?;
        Some((Voice { nodes, order, out, sidechain: vec![] }, kind))
    }
}

/// Every node of `ns`, voices' included, outermost first.
fn walk<'n>(ns: &'n [Node], f: &mut impl FnMut(&'n Node)) {
    for n in ns {
        f(n);
        if let Op::Poly { body, .. } = &n.op {
            walk(&body.nodes, f);
        }
    }
}

/// Every ref string used inside a poly body.
fn body_refs(body: &Body) -> Vec<&str> {
    let mut v = vec![body.out.as_str()];
    for n in &body.nodes {
        match &n.op {
            Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => v.extend(inputs.values().map(String::as_str)),
            Op::Feedback { input, .. } => v.push(input),
            _ => {}
        }
    }
    v
}

/// Kahn's algorithm, lowest index first so the order is deterministic.
/// On a loop, returns the nodes that could not be ordered.
fn topo_order(deps: &[Vec<usize>]) -> Result<Vec<usize>, Vec<usize>> {
    let n = deps.len();
    let mut indeg: Vec<usize> = deps.iter().map(Vec::len).collect();
    let mut users = vec![vec![]; n];
    for (i, d) in deps.iter().enumerate() {
        for &j in d {
            users[j].push(i);
        }
    }
    let mut ready: BinaryHeap<Reverse<usize>> = (0..n).filter(|&i| indeg[i] == 0).map(Reverse).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(Reverse(i)) = ready.pop() {
        order.push(i);
        for &u in &users[i] {
            indeg[u] -= 1;
            if indeg[u] == 0 {
                ready.push(Reverse(u));
            }
        }
    }
    if order.len() == n { Ok(order) } else { Err((0..n).filter(|&i| indeg[i] > 0).collect()) }
}

/// A loop among `stuck`, as a path of nodes where each reads the next.
fn find_cycle(deps: &[Vec<usize>], stuck: &[usize]) -> Vec<usize> {
    let stuck: HashSet<usize> = stuck.iter().copied().collect();
    let mut at = *stuck.iter().min().expect("a loop has nodes");
    let mut path = vec![];
    loop {
        if let Some(pos) = path.iter().position(|&p| p == at) {
            return path[pos..].to_vec();
        }
        path.push(at);
        at = *deps[at].iter().find(|d| stuck.contains(d)).expect("a stuck node waits on another stuck node");
    }
}

/// Whether a signal of kind `src` may connect where `dst` is expected.
fn fits(src: Kind, dst: Kind) -> bool {
    match (src, dst) {
        (Kind::Audio(a), Kind::Audio(b)) => a <= b,
        (Kind::Control(None), Kind::Control(_)) => true,
        (Kind::Control(Some(a)), Kind::Control(Some(b))) => a.fits(b),
        (Kind::Control(Some(_)), Kind::Control(None)) => true,
        (Kind::Events(a), Kind::Events(b)) => a == b,
        _ => false,
    }
}

fn join_units(a: Option<Unit>, b: Option<Unit>) -> Result<Option<Unit>, String> {
    match (a, b) {
        (None, x) | (x, None) => Ok(x),
        (Some(a), Some(b)) if a.fits(b) => Ok(Some(b)),
        (Some(a), Some(b)) if b.fits(a) => Ok(Some(a)),
        (Some(a), Some(b)) => Err(format!("mixes control<{}> with control<{}>", a.name(), b.name())),
    }
}

fn join(a: Kind, b: Kind) -> Result<Kind, String> {
    match (a, b) {
        (Kind::Audio(x), Kind::Audio(y)) => Ok(Kind::Audio(x.max(y))),
        (Kind::Control(x), Kind::Control(y)) => join_units(x, y).map(Kind::Control),
        (Kind::Events(x), Kind::Events(y)) if x == y => Ok(a),
        _ => Err(format!("mixes {a} with {b}")),
    }
}

/// Unify a kernel's type variables with what is connected to it.
/// Returns the inputs in declared order, the channel count, and the output kinds.
/// `plugin`: the node is a hosted plugin, so `notes` may take events and unconnected
/// parameters are left to the plugin.
fn type_kernel(sig: &KernelSig, conns: &HashMap<&str, (PortRef, Kind)>, plugin: bool) -> Result<(Vec<Input>, u8, Vec<Kind>), String> {
    let mut any: Option<Kind> = None;
    let mut chans: Option<u8> = None;
    let mut unit: Option<Unit> = None;
    let mut errs = vec![];
    for p in &sig.inputs {
        let k = match (conns.get(p.name.as_str()), p.default) {
            (Some((_, k)), _) => *k,
            (None, Some(_)) => Kind::Control(None),
            // A plugin's unconnected inputs are silence (audio, notes) or its own value (parameters).
            (None, None) if plugin => continue,
            (None, None) => {
                errs.push(format!("input `{}` ({}) is not connected", p.name, p.kind));
                continue;
            }
        };
        let mismatch = || format!("input `{}` expects {}, got {k}", p.name, p.kind);
        if matches!(k, Kind::Events(_)) && !(plugin && matches!(p.kind, PortKind::Events(_))) {
            errs.push(format!("input `{}`: events can't feed a kernel; play notes with poly", p.name));
            continue;
        }
        match p.kind {
            PortKind::Any => match any.map_or(Ok(k), |a| join(a, k)) {
                Ok(j) => any = Some(j),
                Err(e) => errs.push(format!("input `{}` {e}", p.name)),
            },
            PortKind::Audio(None) => match k {
                Kind::Audio(c) => chans = Some(chans.map_or(c, |x| x.max(c))),
                _ => errs.push(mismatch()),
            },
            PortKind::Audio(Some(n)) => match k {
                Kind::Audio(c) if c <= n => {}
                Kind::Audio(_) => errs.push(format!("input `{}` is mono ({}), but a stereo signal is connected", p.name, p.kind)),
                _ => errs.push(mismatch()),
            },
            PortKind::Control(None) => match k {
                Kind::Control(u) => match join_units(unit, u) {
                    Ok(j) => unit = j,
                    Err(e) => errs.push(format!("input `{}` {e}", p.name)),
                },
                _ => errs.push(mismatch()),
            },
            PortKind::Control(Some(want)) => match k {
                Kind::Control(None) => {}
                Kind::Control(Some(u)) if u.fits(want) => {}
                _ => errs.push(mismatch()),
            },
            PortKind::Events(e) => match k {
                Kind::Events(x) if x == e => {}
                _ => errs.push(mismatch()),
            },
        }
    }
    if !errs.is_empty() {
        return Err(errs.join("; "));
    }
    let any_audio = match any {
        Some(Kind::Audio(c)) => Some(c),
        _ => None,
    };
    let channels = any_audio.into_iter().chain(chans).max().unwrap_or(1);
    let inputs = sig
        .inputs
        .iter()
        .map(|p| match conns.get(p.name.as_str()) {
            Some((src, k)) => {
                let dst = match p.kind {
                    PortKind::Audio(Some(n)) => n,
                    PortKind::Audio(None) => channels,
                    PortKind::Any if any_audio.is_some() => channels,
                    _ => 1,
                };
                Input::Port { src: *src, src_channels: k.channels(), channels: dst }
            }
            None if plugin && p.default.is_none() => Input::Unset,
            None => Input::Const(p.default.unwrap_or(0.0)),
        })
        .collect();
    let outs = sig
        .outputs
        .iter()
        .map(|o| match o.kind {
            PortKind::Any => match any {
                Some(Kind::Audio(_)) => Kind::Audio(channels),
                Some(k) => k,
                None => Kind::Control(None),
            },
            PortKind::Audio(None) => Kind::Audio(channels),
            PortKind::Audio(Some(n)) => Kind::Audio(n),
            PortKind::Control(u) => Kind::Control(u.or(unit)),
            PortKind::Events(e) => Kind::Events(e),
        })
        .collect();
    Ok((inputs, channels, outs))
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod overlay_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ir, KernelSig};
    use serde_json::json;

    fn kernels() -> HashMap<String, KernelSig> {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let mut m = HashMap::new();
        for (id, i, o) in [
            ("t:saw", &["freq: control<hz> = 440"][..], &["out: audio<1>"][..]),
            ("t:svf", &["in: audio<*>", "cutoff: control<hz> = 1000"], &["lp: audio<*>", "hp: audio<*>"]),
            ("t:add", &["a: *", "b: *"], &["out: *"]),
            ("t:pan", &["in: audio<1>"], &["out: audio<2>"]),
            ("t:mtof", &["pitch: control<semitones>"], &["out: control<hz>"]),
            ("t:follow", &["in: audio<*>"], &["out: control<unit01>"]),
            ("t:vca", &["in: audio<*>", "g: control<gain> = 1"], &["out: audio<*>"]),
            ("t:wobble", &["in: audio<*>", "playing: control<unit01> = 1"], &["out: audio<*>"]),
        ] {
            m.insert(id.to_string(), KernelSig::parse(id, "", &s(i), &s(o), 0).unwrap());
        }
        m
    }

    fn ir(v: serde_json::Value) -> Ir {
        serde_json::from_value(v).unwrap()
    }

    fn song(nodes: serde_json::Value, out: &str) -> Ir {
        ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"},
            "modules": [{"key": "song", "nodes": nodes, "exports": {"out": out}}]}))
    }

    #[test]
    fn simple_chain_types_and_orders() {
        let f = check(
            &song(
                json!([
                    {"id": "f", "op": "kernel", "kernel": "t:svf", "inputs": {"in": "o", "cutoff": "c"}},
                    {"id": "o", "op": "kernel", "kernel": "t:saw"},
                    {"id": "c", "op": "const", "value": 800, "unit": "hz"},
                ]),
                "f.lp",
            ),
            &kernels(),
        )
        .unwrap();
        assert_eq!(f.out_kind, Kind::Audio(1));
        let names: Vec<&str> = f.order.iter().map(|&i| f.nodes[i].name.as_str()).collect();
        // The constant is folded into the kernel input, so it drops out of the order.
        assert_eq!(names, ["song/o", "song/f"]);
        assert!(matches!(&f.nodes[0].op, FOp::Kernel { inputs, .. } if inputs[1] == Input::Const(800.0)));
    }

    #[test]
    fn unit_mismatch_is_an_error() {
        let e = check(
            &song(
                json!([
                    {"id": "o", "op": "kernel", "kernel": "t:saw", "inputs": {"freq": "c"}, "src": "song.ts:3:9"},
                    {"id": "c", "op": "const", "value": -6, "unit": "db"},
                ]),
                "o",
            ),
            &kernels(),
        )
        .unwrap_err();
        assert_eq!(e.len(), 1);
        assert!(e[0].message.contains("expects control<hz>, got control<db>"), "{}", e[0]);
        assert_eq!(e[0].src.as_deref(), Some("song.ts:3:9"));
    }

    #[test]
    fn generic_kernels_unify() {
        // add(mono, stereo) is stereo; add(hz, hz) is hz; add(audio, control) is an error.
        let f = check(
            &song(
                json!([
                    {"id": "o", "op": "kernel", "kernel": "t:saw"},
                    {"id": "p", "op": "kernel", "kernel": "t:pan", "inputs": {"in": "o"}},
                    {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "o", "b": "p"}},
                ]),
                "a",
            ),
            &kernels(),
        )
        .unwrap();
        assert_eq!(f.out_kind, Kind::Audio(2));
        let e = check(
            &song(
                json!([
                    {"id": "o", "op": "kernel", "kernel": "t:saw"},
                    {"id": "c", "op": "const", "value": 1, "unit": "hz"},
                    {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "o", "b": "c"}},
                ]),
                "a",
            ),
            &kernels(),
        )
        .unwrap_err();
        assert!(e[0].message.contains("mixes audio<1> with control<hz>"), "{}", e[0]);
    }

    #[test]
    fn loops_are_named_and_feedback_breaks_them() {
        let looped = song(
            json!([
                {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "o", "b": "b"}},
                {"id": "b", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "a"}},
                {"id": "o", "op": "kernel", "kernel": "t:saw"},
            ]),
            "a",
        );
        let e = check(&looped, &kernels()).unwrap_err();
        assert!(e[0].message.contains("song/a (t:add) → song/b (t:vca) → song/a"), "{}", e[0]);

        let fixed = song(
            json!([
                {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "o", "b": "fb"}},
                {"id": "b", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "a"}},
                {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "b"},
                {"id": "o", "op": "kernel", "kernel": "t:saw"},
            ]),
            "a",
        );
        let f = check(&fixed, &kernels()).unwrap();
        assert_eq!(f.order.len(), 4);
    }

    // ID-11: imports, flat node names and lane addresses use the module key, never the label.
    #[test]
    fn imports_cross_modules_and_cycles_across_tracks_are_caught() {
        let ok = ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
            {"key": "kick", "label": "Big Kick", "track": true,
             "lanes": [{"key": "hits", "kind": "events<note>", "label": "Hits"}], "nodes": [
                {"id": "o", "op": "kernel", "kernel": "t:saw"},
                {"id": "e", "op": "kernel", "kernel": "t:follow", "inputs": {"in": "o"}}],
             "exports": {"out": "o", "env": "e"}},
            {"key": "song", "nodes": [
                {"id": "k", "op": "import", "module": "kick", "export": "out"},
                {"id": "ke", "op": "import", "module": "kick", "export": "env"},
                {"id": "v", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "k", "g": "ke"}}],
             "exports": {"out": "v"}}]}));
        let f = check(&ok, &kernels()).unwrap();
        assert_eq!(f.out_kind, Kind::Audio(1));
        let names: Vec<&str> = f.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(f.nodes.iter().any(|n| n.name == "kick/o" && n.module == "kick"), "{names:?}");
        assert_eq!(ok.modules[0].label(), "Big Kick");
        assert_eq!(ok.modules[1].label(), "song", "a label defaults to the key");

        // An import names a module by key; a label is not an address.
        let mut by_label = ok.clone();
        if let Op::Import { module, .. } = &mut by_label.modules[1].nodes[0].op {
            *module = "Big Kick".into();
        }
        let e = check(&by_label, &kernels()).unwrap_err();
        assert!(e.iter().any(|d| d.message.contains("no module `Big Kick`")), "{e:?}");

        let bad = ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
            {"key": "song", "nodes": [
                {"id": "b", "op": "import", "module": "b", "export": "out"},
                {"id": "x", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "b"}}],
             "exports": {"out": "x"}},
            {"key": "b", "nodes": [
                {"id": "a", "op": "import", "module": "song", "export": "out"},
                {"id": "y", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "a"}}],
             "exports": {"out": "y"}}]}));
        let e = check(&bad, &kernels()).unwrap_err();
        assert!(e[0].message.contains("song/x (t:vca) → b/y (t:vca) → song/x"), "{}", e[0]);

        let missing = ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
            {"key": "song", "nodes": [{"id": "k", "op": "import", "module": "kick", "export": "out"}], "exports": {"out": "k"}}]}));
        let e = check(&missing, &kernels()).unwrap_err();
        assert!(e.iter().any(|d| d.message.contains("no module `kick`")), "{e:?}");
    }

    // ID-10: labels are display text, so changing one changes nothing the engine sees.
    #[test]
    fn labels_never_reach_the_flat_graph() {
        let song = |label: &str| {
            ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
                {"key": "lead-vox", "label": label, "track": true,
                 "lanes": [{"key": "cutoff", "kind": "control<hz>", "default": 800, "label": format!("{label} cutoff")}],
                 "nodes": [
                    {"id": "c", "op": "lane", "lane": "cutoff"},
                    {"id": "o", "op": "kernel", "kernel": "t:saw", "inputs": {"freq": "c"}}],
                 "exports": {"out": "o"}},
                {"key": "song", "nodes": [{"id": "v", "op": "import", "module": "lead-vox", "export": "out"}], "exports": {"out": "v"}}]}))
        };
        let a = check(&song("Lead Vox"), &kernels()).unwrap();
        let b = check(&song("Main Vocal"), &kernels()).unwrap();
        assert_eq!(a, b);
        assert!(a.nodes.iter().any(|n| matches!(&n.op, FOp::Lane { module, lane, .. } if module == "lead-vox" && lane == "cutoff")));
    }

    // ID-1, ID-3, ID-5, ID-6: keys in an IR from anywhere are checked, with stable codes.
    #[test]
    fn keys_are_checked() {
        let codes = |v: serde_json::Value| -> Vec<String> {
            check(&ir(v), &kernels()).err().unwrap_or_default().into_iter().map(|d| d.code.to_string()).collect()
        };
        let one = |m: serde_json::Value| {
            json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
                m, {"key": "song", "nodes": [{"id": "o", "op": "kernel", "kernel": "t:saw"}], "exports": {"out": "o"}}]})
        };
        let track = |key: &str, lanes: serde_json::Value| json!({"key": key, "track": true, "lanes": lanes, "nodes": [], "exports": {}});
        assert_eq!(codes(one(track("808", json!([])))), Vec::<String>::new());
        assert_eq!(codes(one(track("Lead Vox", json!([])))), ["key.invalid"]);
        assert_eq!(codes(one(track("song", json!([])))), ["key.invalid", "key.duplicate"]);
        assert_eq!(codes(one(track("kick", json!([{"key": "Hits", "kind": "events<note>"}])))), ["key.invalid"]);
        let twice = json!([{"key": "hits", "kind": "events<note>"}, {"key": "hits", "kind": "control<hz>"}]);
        assert_eq!(codes(one(track("kick", twice))), ["lane.conflict"]);
        let e = check(&ir(one(track("Lead Vox", json!([])))), &kernels()).unwrap_err();
        assert!(e[0].message.contains("Write \"lead-vox\""), "{}", e[0]);

        let two = json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
            track("kick", json!([])), track("kick", json!([])),
            {"key": "song", "nodes": [{"id": "o", "op": "kernel", "kernel": "t:saw"}], "exports": {"out": "o"}}]});
        assert_eq!(codes(two), ["key.duplicate"]);

        // The root is `song`.
        let root = json!({"ir": crate::IR_VERSION, "root": {"module": "main", "export": "out"}, "modules": [
            {"key": "main", "nodes": [{"id": "o", "op": "kernel", "kernel": "t:saw"}], "exports": {"out": "o"}}]});
        assert_eq!(codes(root), ["key.invalid"]);
    }

    /// Kernels plus one instrument plugin and one effect plugin.
    struct WithPlugins(HashMap<String, KernelSig>, HashMap<String, KernelSig>);

    impl Kernels for WithPlugins {
        fn kernel(&self, id: &str) -> Option<&KernelSig> {
            self.0.get(id)
        }
        fn plugin(&self, id: &str) -> Option<&KernelSig> {
            self.1.get(id)
        }
    }

    fn with_plugins() -> WithPlugins {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let mut p = HashMap::new();
        let fx = KernelSig::parse("clap:fx", "", &s(&["in: audio<2>", "mix: control<scalar>"]), &s(&["out: audio<2>"]), 0).unwrap();
        p.insert("clap:fx".to_string(), fx);
        WithPlugins(kernels(), p)
    }

    fn plugins(nodes: serde_json::Value, out: &str) -> Ir {
        ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
            {"key": "vox", "track": true, "nodes": nodes, "exports": {"out": out}},
            {"key": "song", "nodes": [{"id": "v", "op": "import", "module": "vox", "export": "out"}], "exports": {"out": "v"}}]}))
    }

    // ID-8: a plugin's state path is its frames, then its key; without a key, the innermost frame's key.
    #[test]
    fn plugin_state_paths_compose_frames_and_keys() {
        let f = check(
            &plugins(
                json!([
                    {"id": "o", "op": "kernel", "kernel": "t:saw"},
                    {"id": "a", "op": "plugin", "plugin": "clap:fx", "inputs": {"in": "o"}, "key": "tune", "frames": ["chain-l"]},
                    {"id": "b", "op": "plugin", "plugin": "clap:fx", "inputs": {"in": "a"}, "frames": ["chain-r"]},
                    {"id": "c", "op": "plugin", "plugin": "clap:fx", "inputs": {"in": "b"}, "key": "tune"},
                    {"id": "d", "op": "plugin", "plugin": "clap:fx", "inputs": {"in": "c"}},
                ]),
                "d",
            ),
            &with_plugins(),
        )
        .unwrap();
        let state = |name: &str| match &f.nodes.iter().find(|n| n.name == name).unwrap().op {
            FOp::Plugin { state, .. } => state.clone(),
            op => panic!("{op:?}"),
        };
        let path = |v: &[&str]| Some(v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(state("vox/a"), path(&["chain-l", "tune"]));
        assert_eq!(state("vox/b"), path(&["chain-r"]));
        assert_eq!(state("vox/c"), path(&["tune"]));
        assert_eq!(state("vox/d"), None, "no key and no frame: no saved state, only a RuntimeId");
    }

    // ID-5, ID-8: a plugin key is unique in its frame, and no two plugins share a state path.
    #[test]
    fn plugin_keys_and_state_collisions() {
        let codes = |nodes: serde_json::Value| -> Vec<(String, String)> {
            let e = check(&plugins(nodes, "o"), &with_plugins()).err().unwrap_or_default();
            e.into_iter().map(|d| (d.code.to_string(), d.message)).collect()
        };
        let p = |id: &str, extra: serde_json::Value| {
            let mut v = json!({"id": id, "op": "plugin", "plugin": "clap:fx", "inputs": {}});
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            v
        };
        let o = json!({"id": "o", "op": "kernel", "kernel": "t:saw"});
        // The same key in two frames is fine.
        assert_eq!(codes(json!([o, p("a", json!({"key": "tune", "frames": ["l"]})), p("b", json!({"key": "tune", "frames": ["r"]}))])), []);
        let dup = codes(json!([o, p("a", json!({"key": "tune"})), p("b", json!({"key": "tune"}))]));
        assert_eq!(dup.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["key.duplicate"]);
        // Two plugins with no key in one frame both take the frame's key.
        let shared = codes(json!([o, p("a", json!({"frames": ["chain"]})), p("b", json!({"frames": ["chain"]}))]));
        assert_eq!(shared.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["plugin.state-collision"]);
        assert!(shared[0].1.contains("wrap one in key()"), "{}", shared[0].1);
        // A plugin keyed like a frame beside it collides with the frame's unkeyed plugin.
        let beside = codes(json!([o, p("a", json!({"key": "chain"})), p("b", json!({"frames": ["chain"]}))]));
        assert_eq!(beside.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["plugin.state-collision"]);
        // ID-8: a plugin takes its frame's key only while that frame holds no other stateful
        // plugin, keyed or not, though their files would differ (chain.state, chain/tune.state).
        let held = codes(json!([o, p("a", json!({"key": "tune", "frames": ["chain"]})), p("b", json!({"frames": ["chain"]}))]));
        assert_eq!(held.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["plugin.state-collision"]);
        assert!(held[0].1.contains("`chain`") && held[0].1.contains("give it a key"), "{}", held[0].1);
        // A stateful plugin in a frame inside it is that inner frame's.
        assert_eq!(
            codes(json!([o, p("a", json!({"key": "tune", "frames": ["chain", "inner"]})), p("b", json!({"frames": ["chain"]}))])),
            []
        );
        assert_eq!(
            codes(json!([o, p("a", json!({"key": "tune", "frames": ["chain"]})), p("b", json!({"key": "pitch", "frames": ["chain"]}))])),
            []
        );
        let bad = codes(json!([o, p("a", json!({"key": "Tune", "frames": ["Chain L"]}))]));
        assert_eq!(bad.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["key.invalid", "key.invalid"]);
    }

    #[test]
    fn unsupported_things_are_rejected_at_check_time() {
        let mut k = kernels();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        k.insert("t:look".into(), KernelSig::parse("t:look", "", &s(&["in: audio<1>"]), &s(&["out: audio<1>"]), 64).unwrap());
        let ir = |nodes: serde_json::Value, lanes: serde_json::Value| {
            serde_json::from_value::<Ir>(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"},
                "modules": [{"key": "song", "lanes": lanes, "nodes": nodes, "exports": {"out": "x"}}]}))
            .unwrap()
        };
        // Latency: compensated at the top level (RENDER-18), refused inside a voice.
        assert!(check(&ir(json!([{"id": "o", "op": "kernel", "kernel": "t:saw"}, {"id": "x", "op": "kernel", "kernel": "t:look", "inputs": {"in": "o"}}]), json!([])), &k).is_ok());
        let voiced = json!([
            {"id": "n", "op": "lane", "lane": "n"},
            {"id": "x", "op": "poly", "events": "n", "voices": 2, "body": {"out": "l", "nodes": [
                {"id": "o", "op": "kernel", "kernel": "t:saw"},
                {"id": "l", "op": "kernel", "kernel": "t:look", "inputs": {"in": "o"}},
            ]}},
        ]);
        let e = check(&ir(voiced, json!([{"key": "n", "kind": "events<note>"}])), &k).unwrap_err();
        assert!(e[0].message.contains("inside poly"), "{}", e[0]);
        // Audio lanes are supported: a lane can be the song's output.
        assert!(check(&ir(json!([{"id": "x", "op": "lane", "lane": "vox"}]), json!([{"key": "vox", "kind": "audio<2>"}])), &k).is_ok());
        let e = check(
            &ir(json!([{"id": "n", "op": "lane", "lane": "n"}, {"id": "x", "op": "kernel", "kernel": "t:add", "inputs": {"a": "n", "b": "n"}}]), json!([{"key": "n", "kind": "events<note>"}])),
            &k,
        )
        .unwrap_err();
        assert!(e[0].message.contains("events can't feed a kernel"), "{}", e[0]);
    }

    #[test]
    fn poly_voices_read_note_fields_and_outer_signals() {
        let p = ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
            {"key": "song",
             "lanes": [{"key": "notes", "kind": "events<note>"}, {"key": "cutoff", "kind": "control<hz>", "default": 800}],
             "nodes": [
                {"id": "n", "op": "lane", "lane": "notes"},
                {"id": "c", "op": "lane", "lane": "cutoff"},
                {"id": "v", "op": "poly", "events": "n", "voices": 8, "body": {"out": "f.lp", "nodes": [
                    {"id": "p", "op": "lane", "lane": "pitch"},
                    {"id": "hz", "op": "kernel", "kernel": "t:mtof", "inputs": {"pitch": "p"}},
                    {"id": "o", "op": "kernel", "kernel": "t:saw", "inputs": {"freq": "hz"}},
                    {"id": "f", "op": "kernel", "kernel": "t:svf", "inputs": {"in": "o", "cutoff": "c"}}]}}],
             "exports": {"out": "v"}}]}));
        let f = check(&p, &kernels()).unwrap();
        let FOp::Poly { voice, voices, .. } = &f.nodes[f.out.node].op else { panic!() };
        assert_eq!(*voices, 8);
        assert!(voice.nodes.iter().any(|n| matches!(n.op, FOp::Capture(_))));
        // The cutoff lane is ordered before the poly that captures it.
        let pos = |name: &str| f.order.iter().position(|&i| f.nodes[i].name == name).unwrap();
        assert!(pos("song/c") < pos("song/v"));

        // A body lane that is not a note field is an error.
        let mut bad = p.clone();
        if let Op::Poly { body, .. } = &mut bad.modules[0].nodes[2].op {
            body.nodes[0].op = Op::Lane { lane: "cutoff".into() };
        }
        let e = check(&bad, &kernels()).unwrap_err();
        assert!(e[0].message.contains("note fields"), "{}", e[0]);
    }
    // RENDER-16: a poly's options. Left out, they leave its JSON as before (ID-11); written, they
    // reach the flat graph, its glide read from outside the voice and ordered before it.
    #[test]
    fn poly_options_reach_the_flat_graph() {
        let poly = |extra: serde_json::Value| {
            let mut v = json!({"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "o", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "hz", "op": "kernel", "kernel": "t:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "t:saw", "inputs": {"freq": "hz"}}]}});
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
                {"key": "song", "lanes": [{"key": "notes", "kind": "events<note>"}],
                 "nodes": [{"id": "n", "op": "lane", "lane": "notes"}, v, {"id": "g", "op": "const", "value": 0.07, "unit": "seconds"},
                    {"id": "f", "op": "const", "value": 3, "unit": "hz"}],
                 "exports": {"out": "v"}}]}))
        };
        let plain = poly(json!({}));
        let Op::Poly { alloc, glide, .. } = &plain.modules[0].nodes[1].op else { panic!() };
        assert_eq!((*alloc, glide.clone()), (Alloc::default(), None));
        let text = serde_json::to_string(&plain.modules[0].nodes[1]).unwrap();
        assert!(!text.contains("alloc") && !text.contains("glide"), "{text}");

        let legato = poly(json!({"alloc": {"mode": "legato", "retrigger": false}, "glide": "g"}));
        let f = check(&legato, &kernels()).unwrap_or_else(|e| panic!("{e:?}"));
        let at = f.nodes.iter().position(|n| n.name == "song/v").unwrap();
        let FOp::Poly { alloc, glide, .. } = &f.nodes[at].op else { panic!() };
        assert_eq!(*alloc, Alloc { mode: Mode::Legato, retrigger: false, steal: crate::Steal::Oldest });
        let g = glide.expect("a glide");
        assert_eq!(f.nodes[g.node].name, "song/g");
        let pos = |i: usize| f.order.iter().position(|&j| j == i).unwrap();
        assert!(pos(g.node) < pos(at), "the glide is ordered before the poly");
        assert_eq!(serde_json::to_value(&legato.modules[0].nodes[1]).unwrap()["alloc"], json!({"mode": "legato", "retrigger": false}));

        for (extra, says) in [
            (json!({"alloc": {"mode": "mono"}, "voices": 4}), "a poly in mono mode has one voice, not 4"),
            (json!({"glide": "f"}), "glide must be a time in seconds, not control<hz>"),
            (json!({"glide": "nope"}), "glide:"),
        ] {
            let e = check(&poly(extra.clone()), &kernels()).unwrap_err();
            assert!(e[0].message.contains(says), "{extra}: {}", e[0]);
        }
    }

    // RENDER-7: a `time` node reads song time in its own unit. `playing` is 0..1, and reaches a
    // kernel at the top level and in a voice; beats and seconds don't fit where it goes.
    #[test]
    fn time_reads_in_its_own_unit() {
        let p = ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
            {"key": "song",
             "lanes": [{"key": "notes", "kind": "events<note>"}],
             "nodes": [
                {"id": "n", "op": "lane", "lane": "notes"},
                {"id": "t", "op": "time", "of": "playing"},
                {"id": "o", "op": "kernel", "kernel": "t:saw"},
                {"id": "w", "op": "kernel", "kernel": "t:wobble", "inputs": {"in": "o", "playing": "t"}},
                {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "vw", "nodes": [
                    {"id": "vt", "op": "time", "of": "playing"},
                    {"id": "vo", "op": "kernel", "kernel": "t:saw"},
                    {"id": "vw", "op": "kernel", "kernel": "t:wobble", "inputs": {"in": "vo", "playing": "vt"}}]}},
                {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "w", "b": "v"}}],
             "exports": {"out": "a"}}]}));
        let f = check(&p, &kernels()).unwrap_or_else(|e| panic!("{e:?}"));
        let playing = |n: &FNode| n.op == FOp::Time(TimeOf::Playing) && n.outputs == [Kind::Control(Some(Unit::Unit01))];
        assert!(f.nodes.iter().any(|n| n.name == "song/t" && playing(n)), "at the top level");
        let FOp::Poly { voice, .. } = &f.nodes.iter().find(|n| n.name == "song/v").unwrap().op else { panic!("the poly") };
        assert!(voice.nodes.iter().any(playing), "in a voice");
        assert_eq!(serde_json::to_value(TimeOf::Playing).unwrap(), json!("playing"));
        for (of, unit) in [("beats", "beats"), ("seconds", "seconds")] {
            let e = check(
                &song(
                    json!([
                        {"id": "t", "op": "time", "of": of},
                        {"id": "o", "op": "kernel", "kernel": "t:saw"},
                        {"id": "w", "op": "kernel", "kernel": "t:wobble", "inputs": {"in": "o", "playing": "t"}}]),
                    "w",
                ),
                &kernels(),
            )
            .unwrap_err();
            assert!(e[0].message.contains(&format!("expects control<unit01>, got control<{unit}>")), "{}", e[0]);
        }
    }

    /// A song with frames: `o` made in a component frame 0 inside the `key()` frame 1's parent.
    fn framed(frames: serde_json::Value, nodes: serde_json::Value) -> Ir {
        ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"},
            "modules": [{"key": "song", "frames": frames, "nodes": nodes, "exports": {"out": "f.lp"}}]}))
    }

    // ID-15, PKG-3: frames are metadata. With or without them, and with or without a declared
    // control in an origin, the flat graph is the same apart from the origins.
    #[test]
    fn frames_change_nothing_the_engine_runs() {
        let plain = framed(
            json!([]),
            json!([
                {"id": "o", "op": "kernel", "kernel": "t:saw"},
                {"id": "c", "op": "const", "value": 800, "unit": "hz", "origin": {"site": 4, "arg": ["1"]}},
                {"id": "f", "op": "kernel", "kernel": "t:svf", "inputs": {"in": "o", "cutoff": "c"}},
            ]),
        );
        let with = framed(
            json!([{"site": 3, "key": "chain"}, {"site": 4, "component": "std:lowpass", "parent": 0}]),
            json!([
                {"id": "o", "op": "kernel", "kernel": "t:saw", "frame": 0},
                {"id": "c", "op": "const", "value": 800, "unit": "hz", "frame": 1,
                 "origin": {"site": 4, "arg": ["1"], "control": {"frame": 1, "slot": "cutoff", "smooth": true}}},
                {"id": "f", "op": "kernel", "kernel": "t:svf", "inputs": {"in": "o", "cutoff": "c"}, "frame": 1},
            ]),
        );
        let (a, mut b) = (check(&plain, &kernels()).unwrap(), check(&with, &kernels()).unwrap());
        assert!(b.nodes.iter().any(|n| n.origin.as_ref().and_then(|o| o.slot()) == Some("cutoff")));
        b.nodes.iter_mut().for_each(|n| {
            if let Some(o) = &mut n.origin {
                o.control = None;
            }
        });
        assert_eq!(a, b);
        assert!(a.sidechain.is_empty());
        // The constant with an origin stays a node, as every editable constant does.
        assert!(b.nodes.iter().any(|n| matches!(n.op, FOp::Const(800.0))));
    }

    // ID-15, ID-1, PKG-3, PKG-13: frames, the frames nodes and controls name, and sidechain
    // records are checked in an IR from anywhere.
    #[test]
    fn frames_controls_and_sidechains_are_checked() {
        let codes = |frames: serde_json::Value, extra: serde_json::Value| -> Vec<(String, String)> {
            let mut nodes = vec![json!({"id": "o", "op": "kernel", "kernel": "t:saw"})];
            nodes.extend(extra.as_array().unwrap().iter().cloned());
            nodes.push(json!({"id": "f", "op": "kernel", "kernel": "t:svf", "inputs": {"in": "o"}}));
            let e = check(&framed(frames, json!(nodes)), &kernels()).err().unwrap_or_default();
            e.into_iter().map(|d| (d.code.to_string(), d.message)).collect()
        };
        let lp = json!({"site": 4, "component": "std:lowpass"});
        let control = |frame: u32| json!({"id": "c", "op": "const", "value": 1, "origin": {"site": 4, "control": {"frame": frame, "slot": "q", "inherited": true}}});
        assert_eq!(codes(json!([lp]), json!([control(0)])), []);
        let one = |v: Vec<(String, String)>| {
            assert_eq!(v.len(), 1, "{v:?}");
            v.into_iter().next().unwrap()
        };
        let (code, m) = one(codes(json!([lp]), json!([control(1)])));
        assert!(code == "code.check" && m.contains("names frame 1, but the module has 1"), "{m}");
        let (_, m) = one(codes(json!([{"site": 3, "key": "chain"}]), json!([control(0)])));
        assert!(m.contains("isn't a component's"), "{m}");
        let (_, m) = one(codes(json!([lp]), json!([{"id": "x", "op": "const", "value": 1, "frame": 2}])));
        assert!(m.contains("names frame 2"), "{m}");
        let (_, m) = one(codes(json!([{"site": 1, "parent": 0}]), json!([])));
        assert!(m.contains("isn't an earlier frame"), "{m}");
        let (code, _) = one(codes(json!([{"site": 1, "key": "Chain L"}]), json!([])));
        assert_eq!(code, "key.invalid");
        let (_, m) = one(codes(json!([{"site": 4, "component": "std:follow", "sidechain": [{"input": "in", "ref": "nope"}]}]), json!([])));
        assert!(m.contains("sidechain input `in` reads `nope`"), "{m}");
        let (_, m) = one(codes(json!([{"site": 4, "component": "std:follow", "sidechain": [{"input": "in", "ref": "o.nope"}]}]), json!([])));
        assert!(m.contains("sidechain input `in`") && m.contains("no output `nope`"), "{m}");
    }

    // PKG-13: what a sidechain input received is carried into the graph it is in: the song's,
    // through imports, or a voice's.
    #[test]
    fn sidechain_records_reach_the_flat_graph() {
        let p = ir(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
            {"key": "kick", "track": true, "nodes": [{"id": "o", "op": "kernel", "kernel": "t:saw"}], "exports": {"out": "o"}},
            {"key": "song",
             "lanes": [{"key": "notes", "kind": "events<note>"}],
             "frames": [
                {"site": 1, "component": "std:follow", "sidechain": [{"input": "in", "ref": "k"}]},
                {"site": 2, "component": "std:follow", "sidechain": [{"input": "in", "ref": "f.hp"}]}],
             "nodes": [
                {"id": "k", "op": "import", "module": "kick", "export": "out"},
                {"id": "n", "op": "lane", "lane": "notes"},
                {"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "f.lp", "nodes": [
                    {"id": "o2", "op": "kernel", "kernel": "t:saw"},
                    {"id": "f", "op": "kernel", "kernel": "t:svf", "inputs": {"in": "o2"}}]}},
                {"id": "x", "op": "kernel", "kernel": "t:follow", "inputs": {"in": "k"}, "frame": 0},
                {"id": "out", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "v", "g": "x"}}],
             "exports": {"out": "out"}}]}));
        let f = check(&p, &kernels()).unwrap();
        let kick = f.nodes.iter().position(|n| n.name == "kick/o").unwrap();
        assert_eq!(f.sidechain, [PortRef { node: kick, port: 0 }], "an import resolves to the export it reads");
        let FOp::Poly { voice, .. } = &f.nodes.iter().find(|n| n.name == "song/v").unwrap().op else { panic!() };
        assert_eq!(voice.sidechain, [PortRef { node: 1, port: 1 }], "`f.hp` in the voice graph");
        assert_eq!(check(&p, &kernels()).unwrap(), f, "the same for the same code");
    }

    // ID-15: a frame's chain is its parents, outermost first.
    #[test]
    fn frame_chains() {
        let m: Module = serde_json::from_value(json!({"key": "bass", "nodes": [], "exports": {}, "frames": [
            {"site": 3, "key": "chain-l"}, {"site": 7, "component": "std:bass", "parent": 0},
            {"site": 9, "component": "std:lowpass", "occurrence": 1, "parent": 1}, {"site": 2}]}))
        .unwrap();
        let sites: Vec<u32> = m.chain(Some(2)).iter().map(|f| f.site).collect();
        assert_eq!(sites, [3, 7, 9]);
        assert!(m.chain(None).is_empty() && m.chain(Some(9)).is_empty());
    }

    // PKG-3, WRITE-6: one written literal is one value whatever slots it reaches; an inherited
    // control is one value per call and slot.
    #[test]
    fn same_value_for_written_and_inherited_controls() {
        let o = |v: serde_json::Value| -> Origin { serde_json::from_value(v).unwrap() };
        let written = o(json!({"site": 3, "arg": ["0"], "control": {"frame": 1, "slot": "feedback"}}));
        assert!(written.same_value(&o(json!({"site": 3, "arg": ["0"], "control": {"frame": 1, "slot": "mix"}}))));
        assert!(written.same_value(&o(json!({"site": 3, "arg": ["0"]}))), "a literal is the same value with or without a slot");
        let q = o(json!({"site": 9, "control": {"frame": 2, "slot": "q", "inherited": true}}));
        assert!(q.inherited() && q.slot() == Some("q"));
        assert!(q.same_value(&o(json!({"site": 9, "control": {"frame": 5, "slot": "q", "inherited": true}}))), "another evaluation of the call");
        assert!(!q.same_value(&o(json!({"site": 9, "control": {"frame": 2, "slot": "cutoff", "inherited": true}}))));
        assert!(!written.inherited());
        // Inherited and smooth are written only when set.
        assert_eq!(serde_json::to_value(&written).unwrap(), json!({"site": 3, "arg": ["0"], "control": {"frame": 1, "slot": "feedback"}}));
    }
}
