//! Runs a checked song graph.
//!
//! # Render specification
//!
//! - Audio is processed on a fixed grid of [`BLOCK`]-sample blocks starting at sample 0.
//!   How many frames a caller asks for at a time never changes the result.
//! - Controls, gates and time are computed per sample.
//! - `feedback` is a delay of exactly [`BLOCK`] samples.
//! - A `poly` voice changes note at the exact sample the note starts: the voice graph
//!   is processed up to that sample with the old note, reset, then continues with the
//!   new one. Where a voice graph is split is decided by note data alone.
//! - A voice ends once its note is over, its output is quiet (-80 dB) for a block and
//!   no kernel in it reports `active` (an envelope still releasing, echoes in a delay).
//! - Every kernel instance starts from `init` and is reset when its voice is reused.
//!   Rendering from sample `s` is defined as rendering from 0 and dropping what comes
//!   before `s`; [`Renderer::seek`] does exactly that.
//! - Given the same project, packages, sample rate and this runtime, output is bit-
//!   identical. A NaN or infinity is an error in strict mode (the default) and is
//!   silenced in live mode.

pub mod audio;
pub mod data;
pub mod packages;

use anyhow::{Context, Result, bail};
use m2_ir::{FNode, FOp, Flat, Input, Kind, NoteField, Origin, PortRef, TimeOf};
use m2_song::{AbsNote, ControlLane, DataSet, TempoMap};
use wasmtime::{Instance, Memory, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};

pub use data::{decls, lane_kind};
pub use packages::{Effect, Package, PackageSet};

pub const BLOCK: usize = 128;

/// Memory one kernel instance may use.
const KERNEL_MEMORY: usize = 32 << 20;

/// Output below this is "quiet" when deciding whether a voice has finished (-80 dB).
const QUIET: f32 = 1e-4;

/// Per-block timing shared by every node. Blocks are always [`BLOCK`] samples.
struct Block {
    /// Absolute sample of the block's first frame.
    start: u64,
    beats: [f32; BLOCK],
    seconds: [f32; BLOCK],
    /// Beat of each frame, in full precision, for lane lookups.
    beats64: [f64; BLOCK],
    sample_rate: f64,
    /// Tempo at the block's start, for plugins that sync to the host.
    bpm: f64,
    strict: bool,
}

/// The part of a block a graph is processing: frames `off..off + n`.
#[derive(Clone, Copy)]
struct Seg {
    off: usize,
    n: usize,
}

impl Seg {
    const WHOLE: Seg = Seg { off: 0, n: BLOCK };

    fn range(self, ch: usize) -> std::ops::Range<usize> {
        let s = ch * BLOCK + self.off;
        s..s + self.n
    }
}

/// One output port's samples, channel-major, `channels × BLOCK`, indexed by position in
/// the block.
type Buf = Vec<f32>;

struct KernelNode {
    name: String,
    memory: Memory,
    reset: TypedFunc<(), ()>,
    process: TypedFunc<u32, ()>,
    active: TypedFunc<(), u32>,
    in_ptrs: Vec<usize>,
    out_ptrs: Vec<usize>,
    inputs: Vec<Input>,
    out_channels: Vec<u8>,
}

/// A note in samples.
#[derive(Clone, Copy, Debug)]
struct SampleNote {
    start: u64,
    end: u64,
    pitch: f32,
    velocity: f32,
}

struct VoiceSlot {
    graph: Graph,
    note: Option<SampleNote>,
    /// When the voice last started a note, for stealing the oldest.
    born: u64,
    /// Whether the graph has run since its last reset.
    dirty: bool,
}

struct PolyNode {
    notes: Vec<SampleNote>,
    next: usize,
    voices: Vec<VoiceSlot>,
    out: PortRef,
    born: u64,
}

/// A hosted plugin and what feeds it.
struct PluginNode {
    name: String,
    inst: m2_plugins::Instance,
    audio_in: Option<Input>,
    notes: Vec<SampleNote>,
    next: usize,
    /// Sounding notes: (note id, key, end sample).
    playing: Vec<(i32, i16, u64)>,
    note_ids: i32,
    /// (parameter id, input, value last sent).
    params: Vec<(u32, Input, Option<f32>)>,
}

/// How often automation is sampled for plugins, in samples.
const PARAM_STEP: usize = 16;

impl PluginNode {
    fn process(&mut self, b: &Block, bufs: &[Vec<Buf>], out: &mut Buf) -> Result<()> {
        use m2_plugins::Event;
        let (start, end) = (b.start, b.start + BLOCK as u64);
        let mut events = vec![];
        // Note-offs first so a key can be struck again at the same sample.
        self.playing.retain(|&(id, key, stop)| {
            if stop < end {
                events.push(Event::NoteOff { time: stop.saturating_sub(start) as u32, id, key });
                false
            } else {
                true
            }
        });
        while self.next < self.notes.len() && self.notes[self.next].start < start {
            self.next += 1;
        }
        while self.next < self.notes.len() && self.notes[self.next].start < end {
            let n = self.notes[self.next];
            self.next += 1;
            self.note_ids += 1;
            let key = n.pitch.round().clamp(0.0, 127.0) as i16;
            let time = (n.start - start) as u32;
            events.push(Event::NoteOn { time, id: self.note_ids, key, velocity: n.velocity as f64 });
            let tune = n.pitch as f64 - key as f64;
            if tune.abs() > 1e-6 {
                events.push(Event::Tuning { time, id: self.note_ids, key, semitones: tune });
            }
            if n.end < end {
                events.push(Event::NoteOff { time: (n.end - start) as u32, id: self.note_ids, key });
            } else {
                self.playing.push((self.note_ids, key, n.end));
            }
        }
        for (id, input, last) in &mut self.params {
            match input {
                Input::Const(v) => {
                    if last.is_none() {
                        events.push(Event::Param { time: 0, id: *id, value: *v });
                        *last = Some(*v as f32);
                    }
                }
                Input::Unset => {}
                Input::Port { src, .. } => {
                    let s = &bufs[src.node][src.port];
                    for f in (0..BLOCK).step_by(PARAM_STEP) {
                        if *last != Some(s[f]) {
                            events.push(Event::Param { time: f as u32, id: *id, value: s[f] as f64 });
                            *last = Some(s[f]);
                        }
                    }
                }
            }
        }
        let ins: Vec<&[f32]> = match &self.audio_in {
            Some(Input::Port { src, src_channels, .. }) => {
                (0..*src_channels as usize).map(|c| &bufs[src.node][src.port][c * BLOCK..(c + 1) * BLOCK]).collect()
            }
            _ => vec![],
        };
        let mut outs: Vec<&mut [f32]> = out.chunks_mut(BLOCK).collect();
        let transport = m2_plugins::Transport { beats: b.beats64[0], seconds: b.seconds[0] as f64, bpm: b.bpm };
        self.inst.process(BLOCK as u32, &ins, &mut outs, &events, transport).with_context(|| self.name.clone())?;
        if b.strict
            && let Some(at) = out.iter().position(|x| !x.is_finite())
        {
            bail!("{} produced {} at {:.3}s", self.name, out[at], (b.start + (at % BLOCK) as u64) as f64 / b.sample_rate);
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.inst.reset()?;
        self.next = 0;
        self.playing.clear();
        for p in &mut self.params {
            p.2 = None;
        }
        Ok(())
    }
}

enum Exec {
    Control {
        lane: ControlLane,
        default: f32,
    },
    Audio(audio::AudioLane),
    Notes,
    Time(TimeOf),
    Const(f32),
    Kernel(Box<KernelNode>),
    Plugin(Box<PluginNode>),
    /// A plugin that isn't installed (FMT-18a): its audio input passed through, or silence.
    Missing(Option<Input>),
    Poly(Box<PolyNode>),
    /// A delay of exactly `BLOCK` samples; `ring` is indexed by absolute sample mod `BLOCK`.
    Feedback {
        input: PortRef,
        ring: Buf,
    },
    NoteField(NoteField),
    Capture(PortRef),
}

struct Graph {
    exec: Vec<Exec>,
    /// Where each constant was written, for live value changes.
    origins: Vec<Option<Origin>>,
    order: Vec<usize>,
    /// `bufs[node][port]`.
    bufs: Vec<Vec<Buf>>,
}

/// Builds instances for a graph.
struct Builder<'a> {
    store: &'a mut Store<StoreLimits>,
    packages: &'a PackageSet,
    /// The revision's data: lanes, the tempo, and where assets and verified state blobs are.
    data: &'a DataSet,
    tempo: &'a TempoMap,
    sample_rate: f64,
    /// Audio files used by clips, loaded once.
    audio: audio::Assets,
}

impl Builder<'_> {
    fn graph(&mut self, nodes: &[FNode], order: &[usize], in_voice: bool) -> Result<Graph> {
        let mut exec = Vec::with_capacity(nodes.len());
        let mut bufs: Vec<Vec<Buf>> = Vec::with_capacity(nodes.len());
        let used: std::collections::HashSet<usize> = order.iter().copied().collect();
        for (i, n) in nodes.iter().enumerate() {
            bufs.push(
                n.outputs.iter().map(|k| vec![0.0; if matches!(k, Kind::Events(_)) { 0 } else { k.channels() as usize * BLOCK }]).collect(),
            );
            if !used.contains(&i) {
                exec.push(Exec::Const(0.0));
                continue;
            }
            let e = match &n.op {
                FOp::Lane { module, lane, kind: Kind::Control(_), default } => {
                    Exec::Control { lane: self.data.control(module, lane), default: *default as f32 }
                }
                FOp::Lane { kind: Kind::Events(_), .. } => Exec::Notes,
                FOp::Lane { module, lane, kind: Kind::Audio(ch), .. } => {
                    let clips = self.data.audio(module, lane);
                    Exec::Audio(audio::AudioLane::build(clips, self.data, &mut self.audio, self.tempo, self.sample_rate, *ch as usize))
                }
                FOp::Time(t) => Exec::Time(*t),
                FOp::Const(v) => Exec::Const(*v as f32),
                FOp::Kernel { kernel, channels, inputs } => Exec::Kernel(Box::new(self.kernel(n, kernel, *channels, inputs)?)),
                FOp::Plugin { plugin, inputs, .. } if self.packages.plugin_missing(plugin) => {
                    // Typed with `in` first (see `PackageSet::bind_plugins`).
                    Exec::Missing(inputs.first().filter(|i| matches!(i, Input::Port { .. })).cloned())
                }
                FOp::Plugin { plugin, inputs, state, .. } => {
                    Exec::Plugin(Box::new(self.plugin(nodes, n, plugin, inputs, state.as_deref())?))
                }
                FOp::Poly { lane, voices, voice, .. } => {
                    if in_voice {
                        bail!("{}: poly inside poly", n.name);
                    }
                    let FOp::Lane { module, lane: lane_id, .. } = &nodes[*lane].op else { bail!("{}: poly needs a notes lane", n.name) };
                    let notes = self.data.notes(module, lane_id).iter().map(|a| self.sample_note(a)).filter(|n| n.end > n.start).collect();
                    let mut slots = Vec::with_capacity(*voices as usize);
                    for _ in 0..*voices {
                        let graph = self.graph(&voice.nodes, &voice.order, true).with_context(|| format!("{}: voice", n.name))?;
                        slots.push(VoiceSlot { graph, note: None, born: 0, dirty: false });
                    }
                    Exec::Poly(Box::new(PolyNode { notes, next: 0, voices: slots, out: voice.out, born: 0 }))
                }
                FOp::Feedback { input, channels } => Exec::Feedback { input: *input, ring: vec![0.0; *channels as usize * BLOCK] },
                FOp::NoteField(f) => Exec::NoteField(*f),
                FOp::Capture(p) => Exec::Capture(*p),
            };
            exec.push(e);
        }
        // Constants never change: fill them once.
        for (i, e) in exec.iter().enumerate() {
            if let Exec::Const(v) = e {
                for b in &mut bufs[i] {
                    b.fill(*v);
                }
            }
        }
        Ok(Graph { exec, origins: nodes.iter().map(|n| n.origin.clone()).collect(), order: order.to_vec(), bufs })
    }

    fn sample_note(&self, n: &AbsNote) -> SampleNote {
        let at = |beat: f64| (self.tempo.seconds_at(beat) * self.sample_rate).round().max(0.0) as u64;
        SampleNote { start: at(n.start), end: at(n.end), pitch: n.pitch as f32, velocity: n.velocity as f32 }
    }

    fn plugin(&mut self, nodes: &[FNode], n: &FNode, id: &str, inputs: &[Input], state: Option<&[String]>) -> Result<PluginNode> {
        let info = self.packages.plugin_info(id).with_context(|| format!("{}: plugin `{id}` was not described", n.name))?;
        let mut inst = m2_plugins::Instance::new(&info.found, self.sample_rate, BLOCK as u32).with_context(|| n.name.clone())?;
        // Only a blob whose sidecar names this plugin is loaded (FMT-17); without one the
        // plugin plays its defaults, and the data's diagnostics say why.
        let keys: Vec<&str> = state.unwrap_or_default().iter().map(String::as_str).collect();
        if let Some(path) = state.and_then(|_| self.data.state(&n.module, &keys)) {
            let bytes = std::fs::read(&path).with_context(|| format!("{}: reading plugin state {}", n.name, path.display()))?;
            inst.load_state(&bytes).with_context(|| format!("{}: loading {}", n.name, path.display()))?;
        }
        // Inputs come in the order of the plugin's ports; a parameter is bound by the id in its
        // address, never by its place in the list (FMT-18).
        let sig = m2_ir::Kernels::plugin(self.packages, id).with_context(|| format!("{}: plugin `{id}` was not described", n.name))?;
        let (mut audio_in, mut notes, mut params) = (None, vec![], vec![]);
        for (port, input) in sig.inputs.iter().zip(inputs) {
            match (port.name.as_str(), input) {
                ("in", _) => audio_in = Some(input.clone()),
                ("notes", Input::Port { src, .. }) => {
                    if let FOp::Lane { module, lane, .. } = &nodes[src.node].op {
                        notes = self.data.notes(module, lane).iter().map(|a| self.sample_note(a)).filter(|n| n.end > n.start).collect();
                    }
                }
                (_, Input::Unset) => {}
                (name, _) => {
                    let (_, pid) = packages::address(name).with_context(|| format!("{}: `{name}` isn't a parameter address", n.name))?;
                    params.push((pid, input.clone(), None));
                }
            }
        }
        Ok(PluginNode { name: n.name.clone(), inst, audio_in, notes, next: 0, playing: vec![], note_ids: 0, params })
    }

    fn kernel(&mut self, n: &FNode, id: &str, channels: u8, inputs: &[Input]) -> Result<KernelNode> {
        let (module, prefix) = self.packages.module_for(id).with_context(|| format!("{}: kernel `{id}` is not loaded", n.name))?;
        let instance = Instance::new(&mut *self.store, module, &[]).wctx(|| format!("{}: instantiating `{id}`", n.name))?;
        let f = |name: &str| format!("{prefix}_{name}");
        let store = &mut *self.store;
        let init = instance.get_typed_func::<(f32, u32, u32), ()>(&mut *store, &f("init"))?;
        let ports = instance.get_typed_func::<(), u32>(&mut *store, &f("ports"))?;
        let port = instance.get_typed_func::<u32, u32>(&mut *store, &f("port"))?;
        let reset = instance.get_typed_func::<(), ()>(&mut *store, &f("reset"))?;
        let process = instance.get_typed_func::<u32, ()>(&mut *store, &f("process"))?;
        let active = instance.get_typed_func::<(), u32>(&mut *store, &f("active"))?;
        let memory = instance.get_memory(&mut *store, "memory").context("kernel has no memory")?;
        init.call(&mut *store, (self.sample_rate as f32, BLOCK as u32, channels as u32))?;

        let (n_in, n_out) = (inputs.len(), n.outputs.len());
        let declared = ports.call(&mut *store, ())?;
        if declared != ((n_in as u32) << 8 | n_out as u32) {
            bail!(
                "{}: `{id}` has {} inputs and {} outputs, but its manifest declares {n_in} and {n_out}",
                n.name,
                declared >> 8,
                declared & 0xff
            );
        }
        let mut ptrs = vec![];
        for i in 0..(n_in + n_out) as u32 {
            let p = port.call(&mut *store, i)? as usize;
            if p == 0 || !p.is_multiple_of(4) || p + 2 * BLOCK * 4 > memory.data_size(&*store) {
                bail!("{}: `{id}` returned a bad buffer for port {i}", n.name);
            }
            ptrs.push(p);
        }
        let out_ptrs = ptrs.split_off(n_in);
        // Unconnected and constant inputs are written once; kernels never write inputs.
        let data = memory.data_mut(&mut *store);
        for (inp, &ptr) in inputs.iter().zip(&ptrs) {
            if let Input::Const(v) = inp {
                floats(data, ptr, 2 * BLOCK).fill(*v as f32);
            }
        }
        Ok(KernelNode {
            name: n.name.clone(),
            memory,
            reset,
            process,
            active,
            in_ptrs: ptrs,
            out_ptrs,
            inputs: inputs.to_vec(),
            out_channels: n.outputs.iter().map(|k| k.channels()).collect(),
        })
    }
}

/// `with_context` for wasmtime's own error type.
pub(crate) trait WasmContext<T> {
    fn wctx<C: std::fmt::Display + Send + Sync + 'static>(self, f: impl FnOnce() -> C) -> Result<T>;
}

impl<T> WasmContext<T> for wasmtime::Result<T> {
    fn wctx<C: std::fmt::Display + Send + Sync + 'static>(self, f: impl FnOnce() -> C) -> Result<T> {
        self.map_err(|e| anyhow::Error::from(e).context(f()))
    }
}

fn floats(data: &mut [u8], ptr: usize, len: usize) -> &mut [f32] {
    bytemuck::cast_slice_mut(&mut data[ptr..ptr + len * 4])
}

impl Graph {
    /// Process frames `seg` of block `b`. Buffers are indexed by position in the block.
    fn process(
        &mut self,
        store: &mut Store<StoreLimits>,
        b: &Block,
        seg: Seg,
        outer: Option<&[Vec<Buf>]>,
        note: Option<SampleNote>,
    ) -> Result<()> {
        let n = seg.n;
        for oi in 0..self.order.len() {
            let i = self.order[oi];
            let mut out = std::mem::take(&mut self.bufs[i]);
            let bufs = &self.bufs;
            match &mut self.exec[i] {
                Exec::Control { lane, default } => {
                    let o = &mut out[0][seg.range(0)];
                    if lane.is_empty() {
                        o.fill(*default);
                    } else {
                        for (x, &beat) in o.iter_mut().zip(&b.beats64[seg.off..]) {
                            *x = lane.value_at(beat, *default as f64) as f32;
                        }
                    }
                }
                Exec::Audio(lane) => lane.fill(&mut out[0], BLOCK, seg.off, b.start + seg.off as u64, seg.n),
                Exec::Notes | Exec::Const(_) => {}
                Exec::Time(TimeOf::Beats) => out[0][seg.range(0)].copy_from_slice(&b.beats[seg.off..seg.off + n]),
                Exec::Time(TimeOf::Seconds) => out[0][seg.range(0)].copy_from_slice(&b.seconds[seg.off..seg.off + n]),
                Exec::Kernel(k) => {
                    let data = k.memory.data_mut(&mut *store);
                    for (inp, &ptr) in k.inputs.iter().zip(&k.in_ptrs) {
                        if let Input::Port { src, src_channels, channels } = inp {
                            let s = &bufs[src.node][src.port];
                            for c in 0..*channels as usize {
                                let sc = if c < *src_channels as usize { c } else { 0 };
                                floats(data, ptr + c * BLOCK * 4, n).copy_from_slice(&s[seg.range(sc)]);
                            }
                        }
                    }
                    k.process.call(&mut *store, n as u32).wctx(|| format!("{} crashed", k.name))?;
                    let data = k.memory.data_mut(&mut *store);
                    for (o, (&ptr, &ch)) in out.iter_mut().zip(k.out_ptrs.iter().zip(&k.out_channels)) {
                        for c in 0..ch as usize {
                            let dst = &mut o[seg.range(c)];
                            dst.copy_from_slice(floats(data, ptr + c * BLOCK * 4, n));
                            if b.strict
                                && let Some(at) = dst.iter().position(|x| !x.is_finite())
                            {
                                let t = (b.start + (seg.off + at) as u64) as f64 / b.sample_rate;
                                bail!("{} produced {} at {t:.3}s", k.name, dst[at]);
                            }
                        }
                    }
                }
                Exec::Poly(p) => p.process(store, b, bufs, &mut out[0])?,
                Exec::Plugin(p) => p.process(b, bufs, &mut out[0])?,
                Exec::Missing(input) => {
                    for c in 0..out[0].len() / BLOCK {
                        match input {
                            Some(Input::Port { src, src_channels, .. }) => {
                                let sc = if c < *src_channels as usize { c } else { 0 };
                                out[0][seg.range(c)].copy_from_slice(&bufs[src.node][src.port][seg.range(sc)]);
                            }
                            _ => out[0][seg.range(c)].fill(0.0),
                        }
                    }
                }
                Exec::Feedback { ring, .. } => {
                    for c in 0..out[0].len() / BLOCK {
                        for f in 0..n {
                            let at = (b.start as usize + seg.off + f) % BLOCK;
                            out[0][c * BLOCK + seg.off + f] = ring[c * BLOCK + at];
                        }
                    }
                }
                Exec::NoteField(field) => {
                    let note = note.expect("note fields only run in voices");
                    let o = &mut out[0][seg.range(0)];
                    match field {
                        NoteField::Pitch => o.fill(note.pitch),
                        NoteField::Velocity => o.fill(note.velocity),
                        NoteField::Pressure | NoteField::Timbre => o.fill(0.0),
                        NoteField::Gate => {
                            for (k, x) in o.iter_mut().enumerate() {
                                let t = b.start + (seg.off + k) as u64;
                                *x = (t >= note.start && t < note.end) as u8 as f32;
                            }
                        }
                    }
                }
                Exec::Capture(p) => {
                    let src = &outer.expect("captures only run in voices")[p.node][p.port];
                    for c in 0..out[0].len() / BLOCK {
                        out[0][seg.range(c)].copy_from_slice(&src[seg.range(c.min(src.len() / BLOCK - 1))]);
                    }
                }
            }
            self.bufs[i] = out;
        }
        // Feedback stores its input for `BLOCK` samples later.
        for i in 0..self.exec.len() {
            if let Exec::Feedback { input, ring } = &mut self.exec[i] {
                let src = &self.bufs[input.node][input.port];
                let src_ch = src.len() / BLOCK;
                for c in 0..ring.len() / BLOCK {
                    let sc = if c < src_ch { c } else { 0 };
                    for f in 0..n {
                        let at = (b.start as usize + seg.off + f) % BLOCK;
                        ring[c * BLOCK + at] = src[sc * BLOCK + seg.off + f];
                    }
                }
            }
        }
        Ok(())
    }

    /// Back to the state right after building.
    fn reset(&mut self, store: &mut Store<StoreLimits>) -> Result<()> {
        for e in &mut self.exec {
            match e {
                Exec::Kernel(k) => k.reset.call(&mut *store, ())?,
                Exec::Plugin(p) => p.reset()?,
                Exec::Feedback { ring, .. } => ring.fill(0.0),
                Exec::Poly(p) => {
                    p.next = 0;
                    p.born = 0;
                    for v in &mut p.voices {
                        if v.dirty {
                            v.graph.reset(store)?;
                        }
                        *v = VoiceSlot { graph: std::mem::replace(&mut v.graph, Graph::empty()), note: None, born: 0, dirty: false };
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn empty() -> Graph {
        Graph { exec: vec![], origins: vec![], order: vec![], bufs: vec![] }
    }

    /// Change every constant written at `origin`, including inside voices. Returns how
    /// many were changed.
    fn set_const(&mut self, origin: &Origin, value: f32) -> usize {
        let mut n = 0;
        for (i, e) in self.exec.iter_mut().enumerate() {
            match e {
                Exec::Const(v) if self.origins[i].as_ref().is_some_and(|o| o.same_value(origin)) => {
                    *v = value;
                    for b in &mut self.bufs[i] {
                        b.fill(value);
                    }
                    n += 1;
                }
                Exec::Poly(p) => {
                    for voice in &mut p.voices {
                        n += voice.graph.set_const(origin, value);
                    }
                }
                _ => {}
            }
        }
        n
    }

    /// Whether any kernel still holds sound that will come out later.
    fn active(&self, store: &mut Store<StoreLimits>) -> Result<bool> {
        for e in &self.exec {
            match e {
                Exec::Kernel(k) if k.active.call(&mut *store, ())? != 0 => return Ok(true),
                Exec::Feedback { ring, .. } if ring.iter().any(|x| x.abs() > QUIET) => return Ok(true),
                _ => {}
            }
        }
        Ok(false)
    }
}

impl PolyNode {
    fn process(&mut self, store: &mut Store<StoreLimits>, b: &Block, outer: &[Vec<Buf>], out: &mut Buf) -> Result<()> {
        let end = b.start + BLOCK as u64;
        // Notes that started before this block were never heard (seeking skips them).
        while self.next < self.notes.len() && self.notes[self.next].start < b.start {
            self.next += 1;
        }
        // Assign this block's notes to voices, in time order.
        let mut plan: Vec<Vec<(usize, SampleNote)>> = vec![vec![]; self.voices.len()];
        let mut busy: Vec<bool> = self.voices.iter().map(|v| v.note.is_some()).collect();
        while self.next < self.notes.len() && self.notes[self.next].start < end {
            let note = self.notes[self.next];
            self.next += 1;
            let slot = match busy.iter().position(|b| !b) {
                Some(i) => i,
                None => (0..self.voices.len()).min_by_key(|&i| self.voices[i].born).expect("at least one voice"),
            };
            self.born += 1;
            self.voices[slot].born = self.born;
            busy[slot] = true;
            plan[slot].push(((note.start - b.start) as usize, note));
        }

        out.fill(0.0);
        let out_ch = out.len() / BLOCK;
        for (v, plan) in self.voices.iter_mut().zip(plan) {
            if v.note.is_none() && plan.is_empty() {
                continue;
            }
            // Split the block wherever this voice starts a new note.
            let mut at = 0;
            let mut segs: Vec<(Seg, SampleNote, bool)> = vec![];
            let mut current = v.note;
            for (off, note) in plan {
                if let Some(c) = current
                    && off > at
                {
                    segs.push((Seg { off: at, n: off - at }, c, false));
                }
                segs.push((Seg { off, n: 0 }, note, true));
                current = Some(note);
                at = off;
            }
            if let Some(c) = current {
                segs.push((Seg { off: at, n: BLOCK - at }, c, false));
            }
            let mut peak = 0.0f32;
            for (seg, note, starts) in segs {
                if starts {
                    if v.dirty {
                        v.graph.reset(store)?;
                    }
                    v.dirty = true;
                    v.note = Some(note);
                    peak = 0.0;
                    continue;
                }
                if seg.n == 0 {
                    continue;
                }
                v.graph.process(store, b, seg, Some(outer), Some(note))?;
                let vo = &v.graph.bufs[self.out.node][self.out.port];
                let v_ch = vo.len() / BLOCK;
                for c in 0..out_ch {
                    let sc = if c < v_ch { c } else { 0 };
                    for (o, x) in out[seg.range(c)].iter_mut().zip(&vo[seg.range(sc)]) {
                        *o += x;
                        peak = peak.max(x.abs());
                    }
                }
            }
            if let Some(note) = v.note
                && note.end <= end
                && peak < QUIET
                && !v.graph.active(store)?
            {
                v.note = None;
            }
        }
        Ok(())
    }
}

/// Renders a checked song.
pub struct Renderer {
    store: Store<StoreLimits>,
    graph: Graph,
    out: PortRef,
    tempo: TempoMap,
    sample_rate: f64,
    /// Start of the next block to process.
    next_block: u64,
    /// Frames already rendered but not yet handed out.
    pending: std::collections::VecDeque<[f32; 2]>,
    delivered: u64,
    strict: bool,
    non_finite: u64,
}

impl Renderer {
    /// A renderer for a checked graph and the data it plays: a revision's [`DataSet`], which
    /// also says where its assets and verified plugin states are ([`DataSet::empty`] for none).
    pub fn new(flat: &Flat, data: &DataSet, packages: &PackageSet, sample_rate: u32) -> Result<Renderer> {
        let limits =
            StoreLimitsBuilder::new().memory_size(KERNEL_MEMORY).instances(1_000_000).memories(1_000_000).tables(1_000_000).build();
        let mut store = Store::new(&packages.engine, limits);
        store.limiter(|l| l);
        let tempo = data.tempo().clone();
        let graph =
            Builder { store: &mut store, packages, data, tempo: &tempo, sample_rate: sample_rate as f64, audio: audio::Assets::default() }
                .graph(&flat.nodes, &flat.order, false)?;
        Ok(Renderer {
            store,
            graph,
            out: flat.out,
            tempo,
            sample_rate: sample_rate as f64,
            next_block: 0,
            pending: Default::default(),
            delivered: 0,
            strict: true,
            non_finite: 0,
        })
    }

    /// Strict (the default): a NaN or infinity from any kernel is an error naming it.
    /// Live: such samples are silenced at the output and counted in [`Self::non_finite`].
    pub fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    fn block(&mut self) -> Result<()> {
        let mut b = Block {
            start: self.next_block,
            beats: [0.0; BLOCK],
            seconds: [0.0; BLOCK],
            beats64: [0.0; BLOCK],
            sample_rate: self.sample_rate,
            bpm: self.tempo.bpm_at(self.tempo.beat_at(self.next_block as f64 / self.sample_rate)),
            strict: self.strict,
        };
        for f in 0..BLOCK {
            let s = (b.start + f as u64) as f64 / self.sample_rate;
            let beat = self.tempo.beat_at(s);
            b.seconds[f] = s as f32;
            b.beats[f] = beat as f32;
            b.beats64[f] = beat;
        }
        self.graph.process(&mut self.store, &b, Seg::WHOLE, None, None)?;
        let o = &self.graph.bufs[self.out.node][self.out.port];
        let stereo = o.len() >= 2 * BLOCK;
        for f in 0..BLOCK {
            let mut frame = [o[f], if stereo { o[BLOCK + f] } else { o[f] }];
            for x in &mut frame {
                if !x.is_finite() {
                    if self.strict {
                        bail!("the song's output became {x} at {:.3}s", (b.start + f as u64) as f64 / self.sample_rate);
                    }
                    *x = 0.0;
                    self.non_finite += 1;
                }
            }
            self.pending.push_back(frame);
        }
        self.next_block += BLOCK as u64;
        Ok(())
    }

    /// Render the next `out.len()` frames, stereo.
    pub fn process(&mut self, out: &mut [[f32; 2]]) -> Result<()> {
        for frame in out.iter_mut() {
            if self.pending.is_empty() {
                self.block()?;
            }
            *frame = self.pending.pop_front().expect("a block was just rendered");
        }
        self.delivered += out.len() as u64;
        Ok(())
    }

    /// Render `seconds` of audio from the current position.
    pub fn render(&mut self, seconds: f64) -> Result<Vec<[f32; 2]>> {
        let mut out = vec![[0.0; 2]; (seconds * self.sample_rate).round() as usize];
        self.process(&mut out)?;
        Ok(out)
    }

    /// Continue from sample `to`, exactly as if everything before it had been rendered.
    /// Implemented by resetting and replaying from 0.
    pub fn seek(&mut self, to: u64) -> Result<()> {
        self.graph.reset(&mut self.store)?;
        self.next_block = 0;
        self.pending.clear();
        self.delivered = 0;
        let mut scratch = vec![[0.0; 2]; 4096];
        while self.delivered < to {
            let n = (to - self.delivered).min(scratch.len() as u64) as usize;
            self.process(&mut scratch[..n])?;
        }
        Ok(())
    }

    /// Frames handed out so far (the playback position).
    pub fn position(&self) -> u64 {
        self.delivered
    }

    /// The rate it renders at: the project's (RENDER-31).
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate as u32
    }

    /// Change a written constant while playing: the value takes effect at the next
    /// block, with no rebuild. `value` is in the constant's own units (the source
    /// number times its scale). Returns how many nodes it reached (0: not found).
    pub fn set_const(&mut self, origin: &Origin, value: f64) -> usize {
        self.graph.set_const(origin, value as f32)
    }

    /// Voices currently sounding, across every `poly`.
    pub fn active_voices(&self) -> usize {
        self.graph.exec.iter().map(|e| if let Exec::Poly(p) = e { p.voices.iter().filter(|v| v.note.is_some()).count() } else { 0 }).sum()
    }

    /// Samples silenced in live mode because they were NaN or infinite.
    pub fn non_finite(&self) -> u64 {
        self.non_finite
    }
}

#[cfg(test)]
mod tests;
