//! Flatten, type-check and order an IR.
//!
//! The checker trusts nothing about where the IR came from. It resolves imports,
//! sorts the whole song into a processing order (a loop without `feedback` is an
//! error that names the loop), infers every node's output kinds, and checks every
//! connection's kind and unit.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::kind::{EventType, Kind, PortKind, Unit};
use crate::sig::{KernelSig, Kernels};
use crate::{Body, Diagnostic, Ir, Module, Node, Op, TimeOf, split_ref};

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
}

impl NoteField {
    pub const ALL: [(&'static str, NoteField); 5] = [
        ("pitch", NoteField::Pitch),
        ("gate", NoteField::Gate),
        ("velocity", NoteField::Velocity),
        ("pressure", NoteField::Pressure),
        ("timbre", NoteField::Timbre),
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum FOp {
    Lane { module: String, lane: String, kind: Kind, default: f64 },
    Time(TimeOf),
    Const(f64),
    /// `inputs` are in the kernel's declared order; `channels` is passed to `init`.
    Kernel { kernel: String, channels: u8, inputs: Vec<Input> },
    /// `lane` is the notes lane node feeding the voices.
    Poly { lane: usize, voices: u32, voice: Box<Voice>, channels: u8 },
    Feedback { input: PortRef, channels: u8 },
    /// Voice graphs only: a field of the voice's note.
    NoteField(NoteField),
    /// Voice graphs only: an output of a node outside the voice.
    Capture(PortRef),
}

impl FOp {
    /// Nodes in the same graph this op reads in the current block.
    pub fn deps(&self) -> Vec<usize> {
        match self {
            FOp::Kernel { inputs, .. } => inputs
                .iter()
                .filter_map(|i| match i {
                    Input::Port { src, .. } => Some(src.node),
                    Input::Const(_) => None,
                })
                .collect(),
            FOp::Poly { lane, voice, .. } => {
                let mut d = vec![*lane];
                d.extend(voice.nodes.iter().filter_map(|n| match n.op {
                    FOp::Capture(p) => Some(p.node),
                    _ => None,
                }));
                d
            }
            _ => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FNode {
    /// `Module/node`.
    pub name: String,
    pub module: String,
    pub src: Option<String>,
    pub op: FOp,
    pub outputs: Vec<Kind>,
}

/// The graph instantiated once per voice by `poly`.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub nodes: Vec<FNode>,
    pub order: Vec<usize>,
    pub out: PortRef,
}

/// A whole song after checking: one graph, typed and ordered.
#[derive(Clone, Debug, PartialEq)]
pub struct Flat {
    pub nodes: Vec<FNode>,
    /// Processing order of the nodes the output depends on.
    pub order: Vec<usize>,
    pub out: PortRef,
    pub out_kind: Kind,
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
        ck.resolve(&m.name, r, 0)
    }

    fn lane(&self, m: &'a Module, lane: &str) -> Result<(FOp, Kind), String> {
        let d = m.lanes.iter().find(|l| l.id == lane).ok_or_else(|| {
            let have: Vec<&str> = m.lanes.iter().map(|l| l.id.as_str()).collect();
            format!("no lane `{lane}` declared (lanes: {})", if have.is_empty() { "none".into() } else { have.join(", ") })
        })?;
        if matches!(d.kind, Kind::Audio(_)) {
            return Err(format!("lane `{lane}`: audio lanes aren't supported yet"));
        }
        let op = FOp::Lane { module: m.name.clone(), lane: lane.to_string(), kind: d.kind, default: d.default.unwrap_or(0.0) };
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
        let outer = ck.resolve(&m.name, r, 0)?;
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
            .ok_or_else(|| format!("inside poly, lanes are note fields (pitch, gate, velocity, pressure, timbre), not `{lane}`"))?;
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
    Poly { events: PortRef, captures: Vec<PortRef> },
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

fn diag(m: &Module, n: Option<&Node>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        message: message.into(),
        module: Some(m.name.clone()),
        node: n.map(|n| n.id.clone()),
        src: n.and_then(|n| n.src.clone()).or_else(|| m.src.clone()),
    }
}

impl<'a> Checker<'a> {
    fn new(ir: &'a Ir, kernels: &'a dyn Kernels) -> Checker<'a> {
        let mut ck = Checker { kernels, modules: HashMap::new(), nodes: HashMap::new(), slot_of: HashMap::new(), diags: vec![] };
        if ir.ir != crate::IR_VERSION {
            ck.diags.push(Diagnostic::new(format!("IR version {} is not supported (expected {})", ir.ir, crate::IR_VERSION)));
        }
        for m in &ir.modules {
            if ck.modules.insert(&m.name, m).is_some() {
                ck.diags.push(diag(m, None, format!("two modules are named `{}`", m.name)));
            }
            let map = ck.nodes.entry(&m.name).or_default();
            for n in &m.nodes {
                if map.insert(&n.id, n).is_some() {
                    ck.diags.push(diag(m, Some(n), format!("node id `{}` is used twice", n.id)));
                }
            }
            let mut lanes = HashSet::new();
            for l in &m.lanes {
                if !lanes.insert(&l.id) {
                    ck.diags.push(diag(m, None, format!("lane `{}` is declared twice", l.id)));
                }
                if l.default.is_some() && !matches!(l.kind, Kind::Control(_)) {
                    ck.diags.push(diag(m, None, format!("lane `{}`: only control lanes have defaults", l.id)));
                }
            }
        }
        ck
    }

    fn run(&mut self, ir: &'a Ir) -> Option<Flat> {
        let mut items = vec![];
        for m in &ir.modules {
            for n in &m.nodes {
                if !matches!(n.op, Op::Import { .. }) {
                    self.slot_of.insert((&m.name, &n.id), items.len());
                    items.push((m, n));
                }
            }
        }
        let (nodes, mut order, _) = self.check_graph(&items, &mut Song)?;

        for m in &ir.modules {
            for (name, r) in &m.exports {
                if let Err(e) = self.resolve(&m.name, r, 0) {
                    self.diags.push(diag(m, None, format!("export `{name}`: {e}")));
                }
            }
        }
        let Some(root) = self.modules.get(ir.root.module.as_str()).copied() else {
            self.diags.push(Diagnostic::new(format!("the root module `{}` does not exist", ir.root.module)));
            return None;
        };
        let Some(r) = root.exports.get(&ir.root.export) else {
            self.diags.push(diag(root, None, format!("the root export `{}` does not exist", ir.root.export)));
            return None;
        };
        let out = self.resolve(&root.name, r, 0).ok()?;
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
        Some(Flat { nodes, order, out, out_kind })
    }

    /// Resolve a ref in module `module` to a top-level node, following imports.
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
        if let Op::Kernel { kernel, .. } = &node.op
            && let Some(sig) = self.kernels.kernel(kernel)
        {
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
                Op::Kernel { inputs, .. } => {
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
                Op::Poly { events, body, .. } => {
                    let events = scope.resolve(self, m, events);
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
                    match events {
                        Ok(events) => Res::Poly { events, captures },
                        Err(e) => {
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
                    Res::Poly { events, captures } => std::iter::once(events.node).chain(captures.iter().map(|c| c.node)).collect(),
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
                        op => format!(" ({})", op.name()),
                    };
                    format!("{}/{}{}", m.name, node.id, what)
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
                (Op::Time { of }, _) => {
                    let u = if *of == TimeOf::Beats { Unit::Beats } else { Unit::Seconds };
                    Ok((FOp::Time(*of), vec![Kind::Control(Some(u))]))
                }
                (Op::Const { value, unit }, _) => Ok((FOp::Const(*value), vec![Kind::Control(*unit)])),
                (Op::Kernel { kernel, .. }, Res::Kernel(conns)) => {
                    let Some(sig) = self.kernels.kernel(kernel) else {
                        self.diags.push(diag(m, Some(node), format!("unknown kernel `{kernel}`")));
                        continue;
                    };
                    if sig.latency > 0 {
                        self.diags.push(diag(m, Some(node), format!("`{kernel}` adds {} samples of latency, and latency compensation isn't implemented yet", sig.latency)));
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
                    if !unknown.is_empty() {
                        let have: Vec<&str> = sig.inputs.iter().map(|p| p.name.as_str()).collect();
                        Err(format!("`{kernel}` has no input `{}` (inputs: {})", unknown.join("`, `"), have.join(", ")))
                    } else {
                        type_kernel(sig, &typed_conns).map(|(mut inputs, channels, outs)| {
                            for inp in &mut inputs {
                                if let Input::Port { src, .. } = inp
                                    && let Some(Some(FOp::Const(v))) = ops.get(src.node)
                                {
                                    *inp = Input::Const(*v);
                                }
                            }
                            (FOp::Kernel { kernel: kernel.clone(), channels, inputs }, outs)
                        })
                    }
                }
                (Op::Poly { voices, body, .. }, Res::Poly { events, .. }) => {
                    let lane_ok = matches!(ops.get(events.node), Some(Some(FOp::Lane { .. })));
                    match kind_of(&kinds, *events) {
                        None => continue,
                        Some(Kind::Events(EventType::Note)) if lane_ok => {
                            if !(1..=128).contains(voices) {
                                Err(format!("voices must be 1..128, not {voices}"))
                            } else {
                                match self.check_voice(m, body, &kinds) {
                                    Some((voice, Kind::Audio(c))) => {
                                        Ok((FOp::Poly { lane: events.node, voices: *voices, voice: Box::new(voice), channels: c }, vec![Kind::Audio(c)]))
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
                name: format!("{}/{}", m.name, node.id),
                module: m.name.clone(),
                src: node.src.clone(),
                op: op.unwrap_or(FOp::Const(0.0)),
                outputs: k.clone().unwrap_or_default(),
            })
            .collect();
        let mut order = order;
        for (j, (op, k)) in extras.into_iter().enumerate() {
            let module = items.first().map(|(m, _)| m.name.clone()).unwrap_or_default();
            nodes.push(FNode { name: format!("{module}/capture{j}"), module, src: None, op, outputs: vec![k] });
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
        Some((Voice { nodes, order, out }, kind))
    }
}

/// Every ref string used inside a poly body.
fn body_refs(body: &Body) -> Vec<&str> {
    let mut v = vec![body.out.as_str()];
    for n in &body.nodes {
        match &n.op {
            Op::Kernel { inputs, .. } => v.extend(inputs.values().map(String::as_str)),
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
fn type_kernel(sig: &KernelSig, conns: &HashMap<&str, (PortRef, Kind)>) -> Result<(Vec<Input>, u8, Vec<Kind>), String> {
    let mut any: Option<Kind> = None;
    let mut chans: Option<u8> = None;
    let mut unit: Option<Unit> = None;
    let mut errs = vec![];
    for p in &sig.inputs {
        let k = match (conns.get(p.name.as_str()), p.default) {
            (Some((_, k)), _) => *k,
            (None, Some(_)) => Kind::Control(None),
            (None, None) => {
                errs.push(format!("input `{}` ({}) is not connected", p.name, p.kind));
                continue;
            }
        };
        let mismatch = || format!("input `{}` expects {}, got {k}", p.name, p.kind);
        if matches!(k, Kind::Events(_)) {
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
        ] {
            m.insert(id.to_string(), KernelSig::parse(id, "", &s(i), &s(o), 0).unwrap());
        }
        m
    }

    fn ir(v: serde_json::Value) -> Ir {
        serde_json::from_value(v).unwrap()
    }

    fn song(nodes: serde_json::Value, out: &str) -> Ir {
        ir(json!({"ir": 0, "root": {"module": "song", "export": "out"},
            "modules": [{"name": "song", "nodes": nodes, "exports": {"out": out}}]}))
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

    #[test]
    fn imports_cross_modules_and_cycles_across_tracks_are_caught() {
        let ok = ir(json!({"ir": 0, "root": {"module": "song", "export": "out"}, "modules": [
            {"name": "Kick", "nodes": [
                {"id": "o", "op": "kernel", "kernel": "t:saw"},
                {"id": "e", "op": "kernel", "kernel": "t:follow", "inputs": {"in": "o"}}],
             "exports": {"out": "o", "env": "e"}},
            {"name": "song", "nodes": [
                {"id": "k", "op": "import", "module": "Kick", "export": "out"},
                {"id": "ke", "op": "import", "module": "Kick", "export": "env"},
                {"id": "v", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "k", "g": "ke"}}],
             "exports": {"out": "v"}}]}));
        let f = check(&ok, &kernels()).unwrap();
        assert_eq!(f.out_kind, Kind::Audio(1));

        let bad = ir(json!({"ir": 0, "root": {"module": "A", "export": "out"}, "modules": [
            {"name": "A", "nodes": [
                {"id": "b", "op": "import", "module": "B", "export": "out"},
                {"id": "x", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "b"}}],
             "exports": {"out": "x"}},
            {"name": "B", "nodes": [
                {"id": "a", "op": "import", "module": "A", "export": "out"},
                {"id": "y", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "a"}}],
             "exports": {"out": "y"}}]}));
        let e = check(&bad, &kernels()).unwrap_err();
        assert!(e[0].message.contains("A/x (t:vca) → B/y (t:vca) → A/x"), "{}", e[0]);

        let missing = ir(json!({"ir": 0, "root": {"module": "song", "export": "out"}, "modules": [
            {"name": "song", "nodes": [{"id": "k", "op": "import", "module": "Kick", "export": "out"}], "exports": {"out": "k"}}]}));
        let e = check(&missing, &kernels()).unwrap_err();
        assert!(e.iter().any(|d| d.message.contains("no module `Kick`")), "{e:?}");
    }

    #[test]
    fn unsupported_things_are_rejected_at_check_time() {
        let mut k = kernels();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        k.insert("t:look".into(), KernelSig::parse("t:look", "", &s(&["in: audio<1>"]), &s(&["out: audio<1>"]), 64).unwrap());
        let ir = |nodes: serde_json::Value, lanes: serde_json::Value| {
            serde_json::from_value::<Ir>(json!({"ir": 0, "root": {"module": "song", "export": "out"},
                "modules": [{"name": "song", "lanes": lanes, "nodes": nodes, "exports": {"out": "x"}}]}))
            .unwrap()
        };
        let e = check(&ir(json!([{"id": "o", "op": "kernel", "kernel": "t:saw"}, {"id": "x", "op": "kernel", "kernel": "t:look", "inputs": {"in": "o"}}]), json!([])), &k).unwrap_err();
        assert!(e[0].message.contains("latency compensation"), "{}", e[0]);
        let e = check(&ir(json!([{"id": "x", "op": "lane", "lane": "vox"}]), json!([{"id": "vox", "kind": "audio<2>"}])), &k).unwrap_err();
        assert!(e[0].message.contains("audio lanes"), "{}", e[0]);
        let e = check(
            &ir(json!([{"id": "n", "op": "lane", "lane": "n"}, {"id": "x", "op": "kernel", "kernel": "t:add", "inputs": {"a": "n", "b": "n"}}]), json!([{"id": "n", "kind": "events<note>"}])),
            &k,
        )
        .unwrap_err();
        assert!(e[0].message.contains("events can't feed a kernel"), "{}", e[0]);
    }

    #[test]
    fn poly_voices_read_note_fields_and_outer_signals() {
        let p = ir(json!({"ir": 0, "root": {"module": "song", "export": "out"}, "modules": [
            {"name": "song",
             "lanes": [{"id": "notes", "kind": "events<note>"}, {"id": "cutoff", "kind": "control<hz>", "default": 800}],
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
}
