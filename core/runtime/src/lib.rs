//! Runs a checked song graph.
//!
//! Audio is processed in blocks of [`BLOCK`] samples. Every kernel node is its own
//! WebAssembly instance; `poly` keeps a fixed pool of voice graphs, so rendering never
//! instantiates anything after start. The same project always renders to the same
//! samples.

pub mod packages;

use anyhow::{Context, Result, bail};
use m2_ir::{FNode, FOp, Flat, Input, Kind, NoteField, PortRef, TimeOf};
use m2_song::{AbsNote, ControlLane, TempoMap, Timeline};
use wasmtime::{Instance, Memory, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};

pub use packages::{Package, PackageSet};

pub const BLOCK: usize = 128;

/// Memory one kernel instance may use.
const KERNEL_MEMORY: usize = 32 << 20;

/// Per-block timing shared by every node.
struct Block {
    frames: usize,
    /// Absolute sample of the block's first frame.
    start: u64,
    beats: [f32; BLOCK],
    seconds: [f32; BLOCK],
    /// Beat of each frame, in full precision, for lane lookups.
    beats64: [f64; BLOCK],
}

/// One output port's samples, channel-major, `channels × BLOCK`.
type Buf = Vec<f32>;

struct KernelNode {
    name: String,
    memory: Memory,
    reset: TypedFunc<(), ()>,
    process: TypedFunc<u32, ()>,
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
    /// When the voice started, for stealing the oldest.
    born: u64,
    quiet_blocks: u32,
    dirty: bool,
}

struct PolyNode {
    notes: Vec<SampleNote>,
    next: usize,
    voices: Vec<VoiceSlot>,
    out: PortRef,
    born: u64,
}

enum Exec {
    Control { lane: ControlLane, default: f32 },
    Notes,
    Time(TimeOf),
    Const(f32),
    Kernel(Box<KernelNode>),
    Poly(Box<PolyNode>),
    Feedback { input: PortRef, prev: Buf },
    NoteField(NoteField),
    Capture(PortRef),
}

struct Graph {
    exec: Vec<Exec>,
    order: Vec<usize>,
    /// `bufs[node][port]`.
    bufs: Vec<Vec<Buf>>,
}

/// Builds instances for a graph.
struct Builder<'a> {
    store: &'a mut Store<StoreLimits>,
    packages: &'a PackageSet,
    timeline: &'a Timeline,
    tempo: &'a TempoMap,
    sample_rate: f64,
}

impl Builder<'_> {
    fn graph(&mut self, nodes: &[FNode], order: &[usize], in_voice: bool) -> Result<Graph> {
        let mut exec = Vec::with_capacity(nodes.len());
        let mut bufs: Vec<Vec<Buf>> = Vec::with_capacity(nodes.len());
        let used: std::collections::HashSet<usize> = order.iter().copied().collect();
        for (i, n) in nodes.iter().enumerate() {
            bufs.push(n.outputs.iter().map(|k| vec![0.0; if matches!(k, Kind::Events(_)) { 0 } else { k.channels() as usize * BLOCK }]).collect());
            if !used.contains(&i) {
                exec.push(Exec::Const(0.0));
                continue;
            }
            let e = match &n.op {
                FOp::Lane { module, lane, kind: Kind::Control(_), default } => {
                    Exec::Control { lane: self.timeline.control(module, lane)?, default: *default as f32 }
                }
                FOp::Lane { kind: Kind::Events(_), .. } => Exec::Notes,
                FOp::Lane { kind: Kind::Audio(_), lane, .. } => bail!("{}: audio lanes (`{lane}`) aren't supported yet", n.name),
                FOp::Time(t) => Exec::Time(*t),
                FOp::Const(v) => Exec::Const(*v as f32),
                FOp::Kernel { kernel, channels, inputs } => Exec::Kernel(Box::new(self.kernel(n, kernel, *channels, inputs)?)),
                FOp::Poly { lane, voices, voice, .. } => {
                    if in_voice {
                        bail!("{}: poly inside poly", n.name);
                    }
                    let FOp::Lane { module, lane: lane_id, .. } = &nodes[*lane].op else { bail!("{}: poly needs a notes lane", n.name) };
                    let notes = self.timeline.notes(module, lane_id)?.iter().map(|a| self.sample_note(a)).collect();
                    let mut slots = Vec::with_capacity(*voices as usize);
                    for _ in 0..*voices {
                        let graph = self.graph(&voice.nodes, &voice.order, true).with_context(|| format!("{}: voice", n.name))?;
                        slots.push(VoiceSlot { graph, note: None, born: 0, quiet_blocks: 0, dirty: false });
                    }
                    Exec::Poly(Box::new(PolyNode { notes, next: 0, voices: slots, out: voice.out, born: 0 }))
                }
                FOp::Feedback { input, channels } => Exec::Feedback { input: *input, prev: vec![0.0; *channels as usize * BLOCK] },
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
        Ok(Graph { exec, order: order.to_vec(), bufs })
    }

    fn sample_note(&self, n: &AbsNote) -> SampleNote {
        let at = |beat: f64| (self.tempo.seconds_at(beat) * self.sample_rate).round().max(0.0) as u64;
        SampleNote { start: at(n.start), end: at(n.end), pitch: n.pitch as f32, velocity: n.velocity as f32 }
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

struct VoiceState {
    note: SampleNote,
}

impl Graph {
    fn process(&mut self, store: &mut Store<StoreLimits>, b: &Block, outer: Option<&[Vec<Buf>]>, voice: Option<&VoiceState>) -> Result<()> {
        let frames = b.frames;
        for oi in 0..self.order.len() {
            let i = self.order[oi];
            let mut out = std::mem::take(&mut self.bufs[i]);
            let bufs = &self.bufs;
            match &mut self.exec[i] {
                Exec::Control { lane, default } => {
                    let o = &mut out[0][..frames];
                    if lane.is_empty() {
                        o.fill(*default);
                    } else {
                        for (x, &beat) in o.iter_mut().zip(&b.beats64) {
                            *x = lane.value_at(beat, *default as f64) as f32;
                        }
                    }
                }
                Exec::Notes | Exec::Const(_) => {}
                Exec::Time(TimeOf::Beats) => out[0][..frames].copy_from_slice(&b.beats[..frames]),
                Exec::Time(TimeOf::Seconds) => out[0][..frames].copy_from_slice(&b.seconds[..frames]),
                Exec::Kernel(k) => {
                    let data = k.memory.data_mut(&mut *store);
                    for (inp, &ptr) in k.inputs.iter().zip(&k.in_ptrs) {
                        if let Input::Port { src, src_channels, channels } = inp {
                            let s = &bufs[src.node][src.port];
                            for c in 0..*channels as usize {
                                let sc = if c < *src_channels as usize { c } else { 0 };
                                floats(data, ptr + c * BLOCK * 4, frames).copy_from_slice(&s[sc * BLOCK..sc * BLOCK + frames]);
                            }
                        }
                    }
                    k.process.call(&mut *store, frames as u32).wctx(|| format!("{} crashed", k.name))?;
                    let data = k.memory.data_mut(&mut *store);
                    for (o, (&ptr, &ch)) in out.iter_mut().zip(k.out_ptrs.iter().zip(&k.out_channels)) {
                        for c in 0..ch as usize {
                            o[c * BLOCK..c * BLOCK + frames].copy_from_slice(floats(data, ptr + c * BLOCK * 4, frames));
                        }
                    }
                }
                Exec::Poly(p) => p.process(store, b, bufs, &mut out[0])?,
                Exec::Feedback { prev, .. } => out[0].copy_from_slice(prev),
                Exec::NoteField(f) => {
                    let v = voice.expect("note fields only run in voices");
                    let n = v.note;
                    let o = &mut out[0][..frames];
                    match f {
                        NoteField::Pitch => o.fill(n.pitch),
                        NoteField::Velocity => o.fill(n.velocity),
                        NoteField::Pressure | NoteField::Timbre => o.fill(0.0),
                        NoteField::Gate => {
                            for (k, x) in o.iter_mut().enumerate() {
                                let t = b.start + k as u64;
                                *x = (t >= n.start && t < n.end) as u8 as f32;
                            }
                        }
                    }
                }
                Exec::Capture(p) => {
                    let src = &outer.expect("captures only run in voices")[p.node][p.port];
                    let len = out[0].len().min(src.len());
                    out[0][..len].copy_from_slice(&src[..len]);
                }
            }
            self.bufs[i] = out;
        }
        // Feedback reads its input one block later.
        for i in 0..self.exec.len() {
            if let Exec::Feedback { input, prev } = &mut self.exec[i] {
                let src = &self.bufs[input.node][input.port];
                let src_ch = src.len() / BLOCK;
                for c in 0..prev.len() / BLOCK {
                    let sc = if c < src_ch { c } else { 0 };
                    prev[c * BLOCK..c * BLOCK + frames].copy_from_slice(&src[sc * BLOCK..sc * BLOCK + frames]);
                }
            }
        }
        Ok(())
    }

    fn reset(&mut self, store: &mut Store<StoreLimits>) -> Result<()> {
        for e in &mut self.exec {
            match e {
                Exec::Kernel(k) => k.reset.call(&mut *store, ())?,
                Exec::Feedback { prev, .. } => prev.fill(0.0),
                _ => {}
            }
        }
        Ok(())
    }
}

impl PolyNode {
    fn process(&mut self, store: &mut Store<StoreLimits>, b: &Block, outer: &[Vec<Buf>], out: &mut Buf) -> Result<()> {
        let end = b.start + b.frames as u64;
        // Skip notes that started before this block and were never heard (seeking).
        while self.next < self.notes.len() && self.notes[self.next].start < b.start {
            self.next += 1;
        }
        while self.next < self.notes.len() && self.notes[self.next].start < end {
            let note = self.notes[self.next];
            self.next += 1;
            let slot = match self.voices.iter().position(|v| v.note.is_none()) {
                Some(i) => i,
                None => (0..self.voices.len()).min_by_key(|&i| self.voices[i].born).expect("at least one voice"),
            };
            let v = &mut self.voices[slot];
            if v.dirty {
                v.graph.reset(store)?;
            }
            self.born += 1;
            v.note = Some(note);
            v.born = self.born;
            v.quiet_blocks = 0;
            v.dirty = true;
        }

        out.fill(0.0);
        let out_ch = out.len() / BLOCK;
        for v in &mut self.voices {
            let Some(note) = v.note else { continue };
            v.graph.process(store, b, Some(outer), Some(&VoiceState { note }))?;
            let vo = &v.graph.bufs[self.out.node][self.out.port];
            let v_ch = vo.len() / BLOCK;
            let mut peak = 0.0f32;
            for c in 0..out_ch {
                let sc = if c < v_ch { c } else { 0 };
                for f in 0..b.frames {
                    let x = vo[sc * BLOCK + f];
                    out[c * BLOCK + f] += x;
                    peak = peak.max(x.abs());
                }
            }
            // A released voice is freed once it has been quiet (-80 dB) for two blocks.
            if end >= note.end {
                v.quiet_blocks = if peak < 1e-4 { v.quiet_blocks + 1 } else { 0 };
                if v.quiet_blocks >= 2 {
                    v.note = None;
                }
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
    pos: u64,
    non_finite: u64,
}

impl Renderer {
    pub fn new(flat: &Flat, timeline: &Timeline, packages: &PackageSet, sample_rate: u32) -> Result<Renderer> {
        let limits = StoreLimitsBuilder::new().memory_size(KERNEL_MEMORY).instances(1_000_000).memories(1_000_000).tables(1_000_000).build();
        let mut store = Store::new(&packages.engine, limits);
        store.limiter(|l| l);
        let tempo = timeline.tempo()?;
        let graph = Builder { store: &mut store, packages, timeline, tempo: &tempo, sample_rate: sample_rate as f64 }.graph(&flat.nodes, &flat.order, false)?;
        Ok(Renderer { store, graph, out: flat.out, tempo, sample_rate: sample_rate as f64, pos: 0, non_finite: 0 })
    }

    /// Render the next `out.len()` frames, stereo.
    pub fn process(&mut self, out: &mut [[f32; 2]]) -> Result<()> {
        let mut done = 0;
        while done < out.len() {
            let frames = (out.len() - done).min(BLOCK);
            let mut b = Block { frames, start: self.pos, beats: [0.0; BLOCK], seconds: [0.0; BLOCK], beats64: [0.0; BLOCK] };
            for f in 0..frames {
                let s = (self.pos + f as u64) as f64 / self.sample_rate;
                let beat = self.tempo.beat_at(s);
                b.seconds[f] = s as f32;
                b.beats[f] = beat as f32;
                b.beats64[f] = beat;
            }
            self.graph.process(&mut self.store, &b, None, None)?;
            let o = &self.graph.bufs[self.out.node][self.out.port];
            let stereo = o.len() >= 2 * BLOCK;
            for f in 0..frames {
                let l = o[f];
                let mut frame = [l, if stereo { o[BLOCK + f] } else { l }];
                for x in &mut frame {
                    if !x.is_finite() {
                        *x = 0.0;
                        self.non_finite += 1;
                    }
                }
                out[done + f] = frame;
            }
            self.pos += frames as u64;
            done += frames;
        }
        Ok(())
    }

    /// Render `seconds` of audio from the current position.
    pub fn render(&mut self, seconds: f64) -> Result<Vec<[f32; 2]>> {
        let mut out = vec![[0.0; 2]; (seconds * self.sample_rate).round() as usize];
        self.process(&mut out)?;
        Ok(out)
    }

    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Output samples that were NaN or infinite and were replaced with silence.
    /// Non-zero means some kernel is broken.
    pub fn non_finite(&self) -> u64 {
        self.non_finite
    }
}

#[cfg(test)]
mod tests;
