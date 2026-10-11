//! Compiling a graph definition into a runnable, allocation-free processor, plus the
//! three device runtimes (polyphonic instrument, kit, effect).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use m2_dsp::Frame;

use crate::def::*;
use crate::nodes::*;

/// Maximum frames per `process` call. Callers split larger spans.
pub const BLOCK: usize = 64;

const SRC_FREQ: usize = 0;
const SRC_GATE: usize = 1;
const SRC_VEL: usize = 2;
const SRC_IN: usize = 3;
/// Another track's signal, routed by the host (`Device.sidechain`). Falls back to `in`.
const SRC_SC: usize = 4;
const N_SOURCES: usize = 5;

fn source_index(name: &str) -> Option<usize> {
    Some(match name {
        "note.freq" => SRC_FREQ,
        "note.gate" => SRC_GATE,
        "note.velocity" => SRC_VEL,
        "in" => SRC_IN,
        "sidechain" => SRC_SC,
        _ => return None,
    })
}

pub type MacroValues = BTreeMap<String, f32>;

/// Defaults from the plugin, overridden by the device's stored values.
pub fn resolve_macros(def: &PluginDef, overrides: &BTreeMap<String, f32>) -> MacroValues {
    def.macros.iter().map(|m| (m.id.clone(), overrides.get(&m.id).copied().unwrap_or(m.default).clamp(0.0, 1.0))).collect()
}

enum Slot {
    None,
    One(usize),
    Sum(Vec<usize>, Vec<Frame>),
}

struct CompiledNode {
    node: Box<dyn Node>,
    slots: Vec<Slot>,
    bindings: Vec<(String, MacroBinding)>,
}

pub struct GraphRuntime {
    nodes: Vec<CompiledNode>,
    bufs: Vec<Vec<Frame>>,
    out: usize,
}

impl GraphRuntime {
    pub fn compile(def: &GraphDef, allow_in: bool, macros: &MacroValues, env: &BuildEnv) -> Result<GraphRuntime> {
        let mut index: HashMap<&str, usize> = HashMap::new();
        for (i, n) in def.nodes.iter().enumerate() {
            if source_index(&n.id).is_some() {
                bail!("node id `{}` is reserved", n.id);
            }
            if index.insert(n.id.as_str(), i).is_some() {
                bail!("duplicate node id `{}`", n.id);
            }
        }
        let resolve = |r: &str| -> Result<Result<usize, usize>> {
            if let Some(s) = source_index(r) {
                if (s == SRC_IN || s == SRC_SC) && !allow_in {
                    bail!("`in` is only available in effect and post graphs");
                }
                return Ok(Err(s));
            }
            index.get(r).map(|i| Ok(*i)).ok_or_else(|| anyhow!("unknown input `{r}`"))
        };

        // Kahn topological sort over node→node edges.
        let mut indeg = vec![0usize; def.nodes.len()];
        let mut users: Vec<Vec<usize>> = vec![Vec::new(); def.nodes.len()];
        for (i, n) in def.nodes.iter().enumerate() {
            for r in n.inputs.values().flat_map(|r| r.refs()) {
                if let Ok(src) = resolve(r)? {
                    indeg[i] += 1;
                    users[src].push(i);
                }
            }
        }
        let mut order = Vec::with_capacity(def.nodes.len());
        let mut ready: Vec<usize> = (0..def.nodes.len()).filter(|i| indeg[*i] == 0).collect();
        while let Some(i) = ready.pop() {
            order.push(i);
            for &u in &users[i] {
                indeg[u] -= 1;
                if indeg[u] == 0 {
                    ready.push(u);
                }
            }
        }
        if order.len() != def.nodes.len() {
            bail!("graph has a cycle");
        }
        let mut buf_of = vec![0usize; def.nodes.len()];
        for (pos, &i) in order.iter().enumerate() {
            buf_of[i] = N_SOURCES + pos;
        }

        let mut nodes = Vec::with_capacity(order.len());
        for &i in &order {
            let nd = &def.nodes[i];
            let names = input_names(&nd.kind)
                .ok_or_else(|| anyhow!("node `{}`: unknown type `{}` (known: {})", nd.id, nd.kind, NODE_TYPES.join(", ")))?;
            let mut strs = BTreeMap::new();
            for (k, v) in &nd.params {
                if let ParamValue::Str(s) = v {
                    if !is_string_param(&nd.kind, k) {
                        bail!("node `{}`: param `{k}` must be a number or macro binding", nd.id);
                    }
                    strs.insert(k.clone(), s.clone());
                }
            }
            let mut node = make_node(&nd.kind, &strs, env)?;
            let mut bindings = Vec::new();
            for (k, v) in &nd.params {
                let value = match v {
                    ParamValue::Str(_) => continue,
                    ParamValue::Num(x) => *x,
                    ParamValue::Macro(b) => {
                        let mv = macros.get(&b.macro_id).ok_or_else(|| anyhow!("node `{}`: unknown macro `{}`", nd.id, b.macro_id))?;
                        bindings.push((k.clone(), b.clone()));
                        b.map(*mv)
                    }
                };
                if !node.set(k, value) {
                    bail!("node `{}` ({}): unknown param `{k}`", nd.id, nd.kind);
                }
            }
            let mut slots: Vec<Slot> = names.iter().map(|_| Slot::None).collect();
            for (name, r) in &nd.inputs {
                let slot = names
                    .iter()
                    .position(|n| n == name)
                    .ok_or_else(|| anyhow!("node `{}` ({}) has no input `{name}` (inputs: {})", nd.id, nd.kind, names.join(", ")))?;
                let bufs: Vec<usize> =
                    r.refs().into_iter().map(|r| resolve(r).map(|x| x.map(|n| buf_of[n]).unwrap_or_else(|s| s))).collect::<Result<_>>()?;
                slots[slot] = match bufs.len() {
                    0 => Slot::None,
                    1 => Slot::One(bufs[0]),
                    _ => Slot::Sum(bufs, vec![[0.0; 2]; BLOCK]),
                };
            }
            nodes.push(CompiledNode { node, slots, bindings });
        }
        let out_node = *index.get(def.out.as_str()).ok_or_else(|| anyhow!("output node `{}` not found", def.out))?;
        Ok(GraphRuntime { nodes, bufs: vec![vec![[0.0; 2]; BLOCK]; N_SOURCES + order.len()], out: buf_of[out_node] })
    }

    pub fn set_macros(&mut self, macros: &MacroValues) {
        for cn in &mut self.nodes {
            for (p, b) in &cn.bindings {
                if let Some(v) = macros.get(&b.macro_id) {
                    cn.node.set(p, b.map(*v));
                }
            }
        }
    }

    pub fn reset(&mut self) {
        for cn in &mut self.nodes {
            cn.node.reset();
        }
    }

    pub fn gate(&mut self, on: bool) {
        for cn in &mut self.nodes {
            cn.node.gate(on);
        }
    }

    /// True if any node reports it is still sounding.
    pub fn active(&self) -> bool {
        self.nodes.iter().any(|n| n.node.activity() == Some(true))
    }

    /// Run `n <= BLOCK` frames. Returns the output buffer.
    pub fn process(&mut self, ctx: &Ctx, input: Option<&[Frame]>, n: usize) -> &[Frame] {
        self.process_sc(ctx, input, None, n)
    }

    /// Like `process`, with a sidechain signal. Without one, the graph keys off its own input.
    pub fn process_sc(&mut self, ctx: &Ctx, input: Option<&[Frame]>, sidechain: Option<&[Frame]>, n: usize) -> &[Frame] {
        debug_assert!(n <= BLOCK);
        self.bufs[SRC_FREQ][..n].fill([ctx.note_freq; 2]);
        self.bufs[SRC_GATE][..n].fill([if ctx.gate { 1.0 } else { 0.0 }; 2]);
        self.bufs[SRC_VEL][..n].fill([ctx.velocity; 2]);
        match input {
            Some(x) => self.bufs[SRC_IN][..n].copy_from_slice(&x[..n]),
            None => self.bufs[SRC_IN][..n].fill([0.0; 2]),
        }
        match sidechain.or(input) {
            Some(x) => self.bufs[SRC_SC][..n].copy_from_slice(&x[..n]),
            None => self.bufs[SRC_SC][..n].fill([0.0; 2]),
        }
        for (pos, cn) in self.nodes.iter_mut().enumerate() {
            let own = N_SOURCES + pos;
            for slot in &mut cn.slots {
                if let Slot::Sum(srcs, scratch) = slot {
                    scratch[..n].fill([0.0; 2]);
                    for &s in srcs.iter() {
                        for (acc, x) in scratch[..n].iter_mut().zip(&self.bufs[s][..n]) {
                            acc[0] += x[0];
                            acc[1] += x[1];
                        }
                    }
                }
            }
            let mut out = std::mem::take(&mut self.bufs[own]);
            let mut ins: [Option<&[Frame]>; 2] = [None, None];
            for (k, slot) in cn.slots.iter().enumerate().take(2) {
                ins[k] = match slot {
                    Slot::None => None,
                    Slot::One(b) => Some(&self.bufs[*b][..n]),
                    Slot::Sum(_, scratch) => Some(&scratch[..n]),
                };
            }
            cn.node.process(ctx, &ins[..cn.slots.len().min(2)], &mut out[..n]);
            self.bufs[own] = out;
        }
        &self.bufs[self.out][..n]
    }
}

// ---------------------------------------------------------------- devices

struct Voice {
    graph: GraphRuntime,
    pitch: u8,
    velocity: f32,
    gate: bool,
    active: bool,
    age: u64,
}

impl Voice {
    fn render(&mut self, base: &Ctx, n: usize, out: &mut [Frame]) {
        if !self.active {
            return;
        }
        let ctx = Ctx { note_freq: m2_theory::midi_to_hz(self.pitch as f32), velocity: self.velocity, gate: self.gate, ..*base };
        let buf = self.graph.process(&ctx, None, n);
        for (o, x) in out[..n].iter_mut().zip(buf) {
            o[0] += x[0];
            o[1] += x[1];
        }
        if !self.gate && !self.graph.active() {
            self.active = false;
        }
    }

    fn start(&mut self, pitch: u8, velocity: f32, age: u64) {
        if !self.sounding() {
            self.graph.reset();
        }
        self.pitch = pitch;
        self.velocity = velocity;
        self.gate = true;
        self.active = true;
        self.age = age;
        self.graph.gate(true);
    }

    /// Whether the voice is still making sound, evaluated from node state rather than
    /// the per-block `active` flag, so allocation doesn't depend on block boundaries.
    fn sounding(&self) -> bool {
        self.active && (self.gate || self.graph.active())
    }

    fn release(&mut self) {
        if self.gate {
            self.gate = false;
            self.graph.gate(false);
        }
    }
}

pub struct PolyRt {
    voices: Vec<Voice>,
    post: Option<GraphRuntime>,
    age: u64,
    scratch: Vec<Frame>,
}

struct Pad {
    pitch: u8,
    choke: Option<u8>,
    voices: Vec<Voice>,
}

pub struct KitRt {
    pads: Vec<Pad>,
    post: Option<GraphRuntime>,
    age: u64,
    scratch: Vec<Frame>,
}

pub struct EffectRt {
    graph: GraphRuntime,
}

/// A running instance of a plugin.
pub enum DeviceRt {
    Poly(PolyRt),
    Kit(KitRt),
    Effect(EffectRt),
}

impl DeviceRt {
    pub fn new(def: &PluginDef, macros: &MacroValues, samples: &BTreeMap<String, Arc<SampleData>>, sr: f32) -> Result<DeviceRt> {
        let env = BuildEnv { sr, samples };
        Ok(match def.kind {
            PluginKind::Instrument => {
                let voice = def.voice.as_ref().ok_or_else(|| anyhow!("instrument `{}` has no `voice` graph", def.id))?;
                let voices = (0..def.polyphony.clamp(1, 32))
                    .map(|_| {
                        Ok(Voice {
                            graph: GraphRuntime::compile(voice, false, macros, &env)?,
                            pitch: 60,
                            velocity: 1.0,
                            gate: false,
                            active: false,
                            age: 0,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let post = def.post.as_ref().map(|g| GraphRuntime::compile(g, true, macros, &env)).transpose()?;
                DeviceRt::Poly(PolyRt { voices, post, age: 0, scratch: vec![[0.0; 2]; BLOCK] })
            }
            PluginKind::Kit => {
                if def.pads.is_empty() {
                    bail!("kit `{}` has no pads", def.id);
                }
                let pads = def
                    .pads
                    .iter()
                    .map(|p| {
                        let voices = (0..2)
                            .map(|_| {
                                Ok(Voice {
                                    graph: GraphRuntime::compile(&p.voice, false, macros, &env)
                                        .map_err(|e| anyhow!("pad `{}`: {e}", p.name))?,
                                    pitch: p.pitch,
                                    velocity: 1.0,
                                    gate: false,
                                    active: false,
                                    age: 0,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        Ok(Pad { pitch: p.pitch, choke: p.choke, voices })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let post = def.post.as_ref().map(|g| GraphRuntime::compile(g, true, macros, &env)).transpose()?;
                DeviceRt::Kit(KitRt { pads, post, age: 0, scratch: vec![[0.0; 2]; BLOCK] })
            }
            PluginKind::Effect => {
                let g = def.graph.as_ref().ok_or_else(|| anyhow!("effect `{}` has no `graph`", def.id))?;
                DeviceRt::Effect(EffectRt { graph: GraphRuntime::compile(g, true, macros, &env)? })
            }
        })
    }

    pub fn is_instrument(&self) -> bool {
        !matches!(self, DeviceRt::Effect(_))
    }

    pub fn set_macros(&mut self, m: &MacroValues) {
        match self {
            DeviceRt::Poly(p) => {
                for v in &mut p.voices {
                    v.graph.set_macros(m);
                }
                if let Some(g) = &mut p.post {
                    g.set_macros(m);
                }
            }
            DeviceRt::Kit(k) => {
                for pad in &mut k.pads {
                    for v in &mut pad.voices {
                        v.graph.set_macros(m);
                    }
                }
                if let Some(g) = &mut k.post {
                    g.set_macros(m);
                }
            }
            DeviceRt::Effect(e) => e.graph.set_macros(m),
        }
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        let vel = velocity as f32 / 127.0;
        match self {
            DeviceRt::Poly(p) => {
                p.age += 1;
                // Retrigger the same pitch, else a free voice, else steal the oldest.
                let idx = p
                    .voices
                    .iter()
                    .position(|v| v.sounding() && v.pitch == pitch)
                    .or_else(|| p.voices.iter().position(|v| !v.sounding()))
                    .unwrap_or_else(|| p.voices.iter().enumerate().min_by_key(|(_, v)| (v.gate, v.age)).map(|(i, _)| i).unwrap_or(0));
                p.voices[idx].start(pitch, vel, p.age);
            }
            DeviceRt::Kit(k) => {
                k.age += 1;
                // Unknown pitches go to the nearest pad so any note makes a sound.
                let Some(pi) = k.pads.iter().enumerate().min_by_key(|(_, p)| (p.pitch as i32 - pitch as i32).abs()).map(|(i, _)| i) else {
                    return;
                };
                if let Some(group) = k.pads[pi].choke {
                    for (j, pad) in k.pads.iter_mut().enumerate() {
                        if j != pi && pad.choke == Some(group) {
                            pad.voices.iter_mut().for_each(|v| v.active = false);
                        }
                    }
                }
                let pad = &mut k.pads[pi];
                let vi = pad
                    .voices
                    .iter()
                    .position(|v| !v.sounding())
                    .unwrap_or_else(|| pad.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0));
                let pitch = pad.pitch;
                pad.voices[vi].start(pitch, vel, k.age);
            }
            DeviceRt::Effect(_) => {}
        }
    }

    pub fn note_off(&mut self, pitch: u8) {
        match self {
            DeviceRt::Poly(p) => p.voices.iter_mut().filter(|v| v.pitch == pitch).for_each(|v| v.release()),
            // Drum hits are one-shots: their envelopes decay on their own.
            DeviceRt::Kit(_) | DeviceRt::Effect(_) => {}
        }
    }

    /// Release everything; `hard` also silences tails immediately (seek/stop).
    pub fn all_notes_off(&mut self, hard: bool) {
        let each = |v: &mut Voice| {
            v.release();
            if hard {
                v.active = false;
            }
        };
        match self {
            DeviceRt::Poly(p) => p.voices.iter_mut().for_each(each),
            DeviceRt::Kit(k) => k.pads.iter_mut().flat_map(|p| p.voices.iter_mut()).for_each(each),
            DeviceRt::Effect(_) => {}
        }
    }

    /// Instruments: overwrite `out[..n]` with the device's output.
    /// Effects: process `out[..n]` in place.
    pub fn process(&mut self, ctx: &Ctx, out: &mut [Frame], n: usize) {
        self.process_sc(ctx, out, None, n)
    }

    /// Instruments ignore the sidechain; effect graphs read it from their `sidechain` source.
    pub fn process_sc(&mut self, ctx: &Ctx, out: &mut [Frame], sidechain: Option<&[Frame]>, n: usize) {
        match self {
            DeviceRt::Poly(p) => {
                out[..n].fill([0.0; 2]);
                for v in &mut p.voices {
                    v.render(ctx, n, out);
                }
                if let Some(post) = &mut p.post {
                    p.scratch[..n].copy_from_slice(&out[..n]);
                    let y = post.process(ctx, Some(&p.scratch[..n]), n);
                    out[..n].copy_from_slice(y);
                }
            }
            DeviceRt::Kit(k) => {
                out[..n].fill([0.0; 2]);
                for pad in &mut k.pads {
                    for v in &mut pad.voices {
                        v.render(ctx, n, out);
                    }
                }
                if let Some(post) = &mut k.post {
                    k.scratch[..n].copy_from_slice(&out[..n]);
                    let y = post.process(ctx, Some(&k.scratch[..n]), n);
                    out[..n].copy_from_slice(y);
                }
            }
            DeviceRt::Effect(e) => {
                let y = e.graph.process_sc(ctx, Some(&out[..n]), sidechain, n);
                out[..n].copy_from_slice(y);
            }
        }
    }
}
