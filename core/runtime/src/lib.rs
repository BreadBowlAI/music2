//! Runs a checked song graph.
//!
//! # Render specification
//!
//! - Audio is processed on a fixed grid of [`BLOCK`]-sample blocks of the engine clock,
//!   starting at sample 0. How many frames a caller asks for at a time never changes the
//!   result.
//! - There are two clocks (RENDER-6). The engine clock only moves forward: it drives the
//!   block grid, feedback, voice gates and plugins' `steady_time`. The transport is the song
//!   position ([`Transport`]): it drives lanes, notes, `time` and what plugins are told, and
//!   it can stop, jump and loop. Offline they are equal: playing from 0, never looping.
//! - Controls, gates and time are computed per sample.
//! - `feedback` is a delay of exactly [`BLOCK`] samples.
//! - A notes lane is a scheduler that fills a per-block event buffer from the transport
//!   (RENDER-12), and owns the notes it turned on (RENDER-26a). `poly` and plugins read the
//!   buffer; a voice's gate opens at its note's On and closes at its Off (RENDER-13).
//! - A `poly` voice changes note at the exact sample the note starts: the voice graph
//!   is processed up to that sample with the old note, reset, then continues with the
//!   new one. Where a voice graph is split is decided by note data alone.
//! - A voice ends once its note is off, its output is quiet (-80 dB) for a block and
//!   no kernel in it reports `active` (an envelope still releasing, echoes in a delay).
//! - Every kernel instance starts from `init` and is reset when its voice is reused, on panic,
//!   and before an offline replay. A jump (seek, wrap, stop) resets nothing (RENDER-10).
//!   Rendering from sample `s` offline is defined as rendering from 0 and dropping what comes
//!   before `s`; [`Renderer::seek`] does exactly that (RENDER-1).
//! - Given the same project, packages, sample rate and this runtime, output is bit-
//!   identical. A NaN or infinity is an error in strict mode (the default) and is
//!   silenced in live mode.
//! - The audio thread's paths never allocate or lock (RENDER-28): buffers are sized when a
//!   renderer is built, and every kernel is reset once while it is built, so a later reset
//!   never grows its memory.
//! - Every constant with an origin is a cell ([`cells`]): written into kernel input memory once,
//!   and again only when a value arrives through its slot, at a block line (RENDER-22).
//! - Kernel input memory is the host's: a kernel never writes it, and a reset never changes it.
//!   Every port has its own memory, checked when an instance is built; debug builds also check
//!   that input memory is unchanged by every call into a kernel (ABI-1, RENDER-24).

pub mod audio;
pub mod cells;
pub mod data;
pub mod engine;
pub mod event;
mod fade;
mod keep;
pub mod packages;
mod plan;
pub mod plugins;
pub mod sched;
pub mod trace;
pub mod transport;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use m2_ir::{FNode, FOp, Flat, Input, Kind, NoteField, PortRef, TimeOf};
use m2_song::{Arrangement, DataSet, LaneData, TempoMap};
use wasmtime::{Instance, Memory, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};

pub use cells::{CellAt, CellId, CellInfo, CellMap, CellSlots, CellTarget};
pub use data::{decls, lane_kind};
pub use engine::{Applied, CallbackStamp, Engine, Envelope, Heard, Msg, Retired};
pub use event::{Event, Events};
pub use packages::{Package, PackageSet};
pub use plan::{Basis, Continues, Count, Keep, LaneInfo, Leftovers, PlanInfo, PluginAt, Shape, Stats};
pub use plugins::PluginInstance;
pub use sched::{Caps, LaneSwap, NotesData};
pub use trace::{TraceEntry, TraceKind};
pub use transport::{Cmd, Jump, Loop, MAX_RUNS, Run, Runs, Transport};

use crate::cells::{Cells, Write};
use crate::fade::xfade;
use crate::keep::{Edge, Wiring, used};
use crate::plan::{Entry, LanePlan, LaneSlot, Lanes, Plan};
use crate::trace::Trace;

pub const BLOCK: usize = 128;

/// Bytes of one kernel port buffer: `PORT_CHANNELS` (2) channels of [`BLOCK`] floats.
const PORT_BYTES: usize = 2 * BLOCK * 4;

/// A node's version when its output must be copied into the kernels it feeds at every call: it
/// isn't a constant, or it is a cell that ramps this block.
const COPY: u64 = u64::MAX;

/// Memory one kernel instance may use.
const KERNEL_MEMORY: usize = 32 << 20;

/// Output below this is "quiet" when deciding whether a voice has finished (-80 dB).
const QUIET: f32 = 1e-4;

/// What a run's plugins are told of the arrangement (HOST-6).
#[derive(Clone, Copy, Default)]
struct RunInfo {
    bpm: f64,
    tsig: (u16, u16),
    bar_start: f64,
    bar: i32,
}

/// Per-block timing shared by every node. Blocks are always [`BLOCK`] samples.
struct Block {
    /// Engine clock of the block's first frame.
    engine: u64,
    /// The transport over the block's frames.
    runs: Runs,
    info: [RunInfo; MAX_RUNS],
    /// The loop, in beats and seconds, for plugins.
    looping: Option<((f64, f64), (f64, f64))>,
    beats: [f32; BLOCK],
    seconds: [f32; BLOCK],
    /// Beat of each frame, in full precision, for lane lookups.
    beats64: [f64; BLOCK],
    sample_rate: f64,
    strict: bool,
    /// An activation's fade window while it runs: the engine clock it began at, and its frames.
    window: Option<(u64, u32)>,
}

impl Block {
    /// Song time of the block's frame `f`, for messages.
    fn time(&self, f: usize) -> f64 {
        self.runs.frame(f as u32) as f64 / self.sample_rate
    }
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

/// One kernel instance, in a wasmtime `Store` of its own (RENDER-32): a box a plan activation can
/// move into the next plan, whose memory goes when it drops.
struct KernelInst {
    store: Store<StoreLimits>,
    /// The hash of the shape it was made for: what an activation checks before moving it.
    shape: u64,
    name: String,
    memory: Memory,
    reset: TypedFunc<(), ()>,
    process: TypedFunc<u32, ()>,
    active: TypedFunc<(), u32>,
    in_ptrs: Vec<usize>,
    out_ptrs: Vec<usize>,
    out_channels: Vec<u8>,
    /// Debug builds: input memory before a call into the kernel, to compare after (ABI-1), and
    /// the kernel id and its inputs' names, for the message.
    #[cfg(debug_assertions)]
    before: Vec<u8>,
    #[cfg(debug_assertions)]
    kernel: String,
    #[cfg(debug_assertions)]
    in_names: Vec<String>,
}

/// Kernel instances alive in the process: what a churn of activations must give back (§7.6).
static LIVE_KERNELS: AtomicUsize = AtomicUsize::new(0);

/// Kernel instances alive in the process now.
pub fn live_kernels() -> usize {
    LIVE_KERNELS.load(Ordering::Relaxed)
}

impl Drop for KernelInst {
    fn drop(&mut self) {
        LIVE_KERNELS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A kernel node: what feeds it, and the instance it holds.
struct KernelNode {
    inputs: Vec<Input>,
    /// For each input, the version of its source constant last written into its memory
    /// ([`COPY`] when it was copied for one segment): a steady constant is written once
    /// (RENDER-22).
    seen: Vec<u64>,
    /// Empty only while its instance moves between plans: it plays silence then.
    inst: Option<Box<KernelInst>>,
}

impl KernelInst {
    /// Instantiate kernel `id` for node `n` in a store of its own, initialise it and check its ports;
    /// constant inputs are written into its memory once.
    fn new(packages: &PackageSet, n: &FNode, id: &str, channels: u8, inputs: &[Input], sample_rate: f64, shape: u64) -> Result<KernelInst> {
        let (module, prefix) = packages.module_for(id).with_context(|| format!("{}: kernel `{id}` is not loaded", n.name))?;
        let limits = StoreLimitsBuilder::new().memory_size(KERNEL_MEMORY).instances(1).memories(1).tables(1).build();
        let mut store = Store::new(&packages.engine, limits);
        store.limiter(|l| l);
        let instance = Instance::new(&mut store, module, &[]).wctx(|| format!("{}: instantiating `{id}`", n.name))?;
        let f = |name: &str| format!("{prefix}_{name}");
        let init = instance.get_typed_func::<(f32, u32, u32), ()>(&mut store, &f("init"))?;
        let ports = instance.get_typed_func::<(), u32>(&mut store, &f("ports"))?;
        let port = instance.get_typed_func::<u32, u32>(&mut store, &f("port"))?;
        let reset = instance.get_typed_func::<(), ()>(&mut store, &f("reset"))?;
        let process = instance.get_typed_func::<u32, ()>(&mut store, &f("process"))?;
        let active = instance.get_typed_func::<(), u32>(&mut store, &f("active"))?;
        let memory = instance.get_memory(&mut store, "memory").context("kernel has no memory")?;
        init.call(&mut store, (sample_rate as f32, BLOCK as u32, channels as u32))?;

        let (n_in, n_out) = (inputs.len(), n.outputs.len());
        let declared = ports.call(&mut store, ())?;
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
            let p = port.call(&mut store, i)? as usize;
            if p == 0 || !p.is_multiple_of(4) || p + PORT_BYTES > memory.data_size(&store) {
                bail!("{}: `{id}` returned a bad buffer for port {i}", n.name);
            }
            ptrs.push(p);
        }
        // ABI-1, in every build: a port that shared memory with another would let one write the
        // other, so an input written once could change behind the host's back.
        let sig = m2_ir::Kernels::kernel(packages, id).with_context(|| format!("{}: kernel `{id}` has no signature", n.name))?;
        let names: Vec<&str> = sig.inputs.iter().chain(&sig.outputs).map(|p| p.name.as_str()).collect();
        let mut by_start: Vec<(usize, usize)> = ptrs.iter().enumerate().map(|(i, &p)| (p, i)).collect();
        by_start.sort_unstable();
        if let Some(w) = by_start.windows(2).find(|w| w[0].0 + PORT_BYTES > w[1].0) {
            let name = |i: usize| names.get(i).copied().unwrap_or("?");
            bail!(
                "{}: `{id}` returned overlapping buffers for ports `{}` and `{}` (ABI-1: each port has its own memory)",
                n.name,
                name(w[0].1),
                name(w[1].1)
            );
        }
        let out_ptrs = ptrs.split_off(n_in);
        // Unconnected and constant inputs are written once; kernels never write inputs.
        let data = memory.data_mut(&mut store);
        for (inp, &ptr) in inputs.iter().zip(&ptrs) {
            if let Input::Const(v) = inp {
                floats(data, ptr, 2 * BLOCK).fill(*v as f32);
            }
        }
        LIVE_KERNELS.fetch_add(1, Ordering::Relaxed);
        Ok(KernelInst {
            store,
            shape,
            name: n.name.clone(),
            memory,
            reset,
            process,
            active,
            in_ptrs: ptrs,
            out_ptrs,
            out_channels: n.outputs.iter().map(|k| k.channels()).collect(),
            #[cfg(debug_assertions)]
            before: vec![0; n_in * PORT_BYTES],
            #[cfg(debug_assertions)]
            kernel: id.to_string(),
            #[cfg(debug_assertions)]
            in_names: (0..n_in).map(|i| names.get(i).copied().unwrap_or("?").to_string()).collect(),
        })
    }

    /// `k_process` for `frames` frames. Debug builds check it left its inputs alone (ABI-1).
    fn process(&mut self, frames: usize) -> Result<()> {
        #[cfg(debug_assertions)]
        self.keep_inputs();
        self.process.call(&mut self.store, frames as u32).wctx(|| format!("{} crashed", self.name))?;
        #[cfg(debug_assertions)]
        self.check_inputs(false)?;
        Ok(())
    }

    /// `k_reset`. Debug builds check it left its inputs alone (ABI-1).
    fn reset(&mut self) -> Result<()> {
        #[cfg(debug_assertions)]
        self.keep_inputs();
        self.reset.call(&mut self.store, ())?;
        #[cfg(debug_assertions)]
        self.check_inputs(true)?;
        Ok(())
    }

    /// Whether it still holds sound that will come out later.
    fn active(&mut self) -> Result<bool> {
        Ok(self.active.call(&mut self.store, ())? != 0)
    }

    #[cfg(debug_assertions)]
    fn keep_inputs(&mut self) {
        let data = self.memory.data(&self.store);
        for (k, &ptr) in self.in_ptrs.iter().enumerate() {
            self.before[k * PORT_BYTES..(k + 1) * PORT_BYTES].copy_from_slice(&data[ptr..ptr + PORT_BYTES]);
        }
    }

    #[cfg(debug_assertions)]
    fn check_inputs(&self, reset: bool) -> Result<()> {
        let data = self.memory.data(&self.store);
        for (k, &ptr) in self.in_ptrs.iter().enumerate() {
            if data[ptr..ptr + PORT_BYTES] != self.before[k * PORT_BYTES..(k + 1) * PORT_BYTES] {
                let (kernel, port) = (&self.kernel, &self.in_names[k]);
                let what = if reset {
                    format!("`{kernel}`'s reset changed its input `{port}`")
                } else {
                    format!("`{kernel}` wrote its input `{port}`")
                };
                bail!("{}: {what} (ABI-1: kernel input memory is read-only)", self.name);
            }
        }
        Ok(())
    }
}

impl KernelNode {
    /// Frames `seg` of block `b`: its inputs copied into its memory as they change, the call, and
    /// its outputs copied out. Never allocates.
    fn process(&mut self, b: &Block, seg: Seg, bufs: &[Vec<Buf>], versions: &[u64], out: &mut [Buf]) -> Result<()> {
        let n = seg.n;
        let Some(k) = &mut self.inst else {
            for o in out.iter_mut() {
                for c in 0..o.len() / BLOCK {
                    o[seg.range(c)].fill(0.0);
                }
            }
            return Ok(());
        };
        let data = k.memory.data_mut(&mut k.store);
        for ((inp, &ptr), seen) in self.inputs.iter().zip(&k.in_ptrs).zip(&mut self.seen) {
            if let Input::Port { src, src_channels, channels } = inp {
                let s = &bufs[src.node][src.port];
                // A steady constant is written over the whole block, once per value, so every
                // later segment finds it in place; anything else is copied for this segment.
                let version = versions[src.node];
                if version != COPY && *seen == version {
                    continue;
                }
                let (off, len) = if version == COPY { (seg.off, n) } else { (0, BLOCK) };
                for c in 0..*channels as usize {
                    let sc = if c < *src_channels as usize { c } else { 0 };
                    floats(data, ptr + c * BLOCK * 4, len).copy_from_slice(&s[sc * BLOCK + off..sc * BLOCK + off + len]);
                }
                *seen = version;
            }
        }
        k.process(n)?;
        let (out_ptrs, out_channels) = (&k.out_ptrs, &k.out_channels);
        let data = k.memory.data_mut(&mut k.store);
        for (o, (&ptr, &ch)) in out.iter_mut().zip(out_ptrs.iter().zip(out_channels)) {
            for c in 0..ch as usize {
                let dst = &mut o[seg.range(c)];
                dst.copy_from_slice(floats(data, ptr + c * BLOCK * 4, n));
                if b.strict
                    && let Some(at) = dst.iter().position(|x| !x.is_finite())
                {
                    bail!("{} produced {} at {:.3}s", k.name, dst[at], b.time(seg.off + at));
                }
            }
        }
        Ok(())
    }
}

/// The note a voice plays. `off` is the engine sample of its Off, `u64::MAX` until the Off
/// arrives: the gate is open before it (RENDER-13).
#[derive(Clone, Copy, Debug)]
struct VoiceNote {
    id: u32,
    pitch: f32,
    velocity: f32,
    off: u64,
}

struct VoiceSlot {
    graph: Graph,
    note: Option<VoiceNote>,
    /// When the voice last started a note, for stealing the oldest.
    born: u64,
    /// Whether the graph has run since its last reset.
    dirty: bool,
    /// The voice's peak over its last block, for a note whose Off lands on the next block
    /// line: it is freed there, exactly where it would have been at the end of the block.
    last_peak: f32,
    /// Given a note this block.
    taken: bool,
}

/// A note given to a voice this block, at `frame`.
#[derive(Clone, Copy)]
struct Planned {
    voice: u16,
    frame: u16,
    note: VoiceNote,
}

/// A `poly`'s voices, with their graphs and notes: one box, which a plan activation moves whole
/// (RENDER-16).
struct VoicePool {
    voices: Vec<VoiceSlot>,
    born: u64,
    shape: u64,
}

struct PolyNode {
    /// The notes lane it plays.
    slot: usize,
    /// The voice graph's output.
    out: PortRef,
    /// What its voices' `Capture` nodes read, by index: outputs of the graph around it. Kept out of
    /// the voices, so a pool moved into another plan reads that plan's nodes.
    captures: Vec<PortRef>,
    /// This block's Ons, by voice: room for two runs of the lane's Ons.
    plan: Vec<Planned>,
    /// Empty only while it moves between plans: it plays silence then.
    pool: Option<Box<VoicePool>>,
    /// A replaced poly playing out an activation's window: it reads no notes.
    ghost: bool,
}

/// A hosted plugin and what feeds it. It reaches its instance only through the host (P11).
struct PluginNode {
    name: String,
    /// Empty only while it moves between plans: it plays silence then.
    inst: Option<Arc<PluginInstance>>,
    audio_in: Option<Input>,
    /// The notes lane it plays, if any.
    notes: Option<usize>,
    /// The key of each note sounding into it, by note id.
    keys: Vec<(u32, i16)>,
    /// The notes it was given before a reset (a panic), ended at its next call: a plugin may keep
    /// its voices through a reset, and nothing else would end them (RENDER-10, RENDER-26a). As
    /// big as `keys`.
    ended: Vec<(u32, i16)>,
    /// Notes its instance held that this node lets go (a NoteOff, not a choke) at frame 0 of its
    /// next call: it replaced a node at an activation (a ghost), or was rewired to another lane
    /// (RENDER-26a). As big as `keys`, or as the node it took the instance from could hold.
    release: Vec<(u32, i16)>,
    /// (parameter id, input, value last sent).
    params: Vec<(u32, Input, Option<f32>)>,
    /// One call's events in and out, sized when built (RENDER-28). What comes out (parameter
    /// gestures, HOST-4) is kept for one call: nothing consumes it before plugin windows.
    events: m2_plugins::Events,
    out: m2_plugins::Events,
    /// What its last call reported: whether the engine may idle (RENDER-6a).
    status: m2_plugins::Status,
    /// NoteOffs sent to it since it was built, and the engine sample of the last (gates).
    offs: u64,
    last_off: Option<u64>,
    /// An export's borrowed instance: reset at its first call, so it starts from silence as a
    /// fresh one would (RENDER-1).
    reset_first: bool,
}

/// A top-level plugin node, as the gates and tests see it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginView {
    pub handle: m2_plugins::Handle,
    /// The notes sounding into it: `(note id, key)`.
    pub notes: Vec<(u32, i16)>,
    /// NoteOffs sent to it since it was built, and the engine sample of the last.
    pub offs: u64,
    pub last_off: Option<u64>,
    pub status: m2_plugins::Status,
}

/// How often automation is sampled for plugins, in samples.
const PARAM_STEP: usize = 16;

/// Events a plugin may output in one call (HOST-4); more are counted.
const OUT_EVENTS: usize = 256;

/// One call's events for a plugin, for the block's frames `lo..hi`: first a choke and a NoteOff
/// at its first frame for each of `ended`, and a NoteOff for each of `release`, then the lane's
/// note events (already in order) from `*next` on, merged with `params` sampled every
/// [`PARAM_STEP`] frames. At one frame parameters come after notes, the order a stable sort by
/// time gave before. An On becomes a NoteOn (and a Tuning between keys), an Off a NoteOff for the
/// key `keys` holds for it. Returns how many NoteOffs it sent, and the time of the last. Never
/// allocates: `out` and `keys` keep their room (RENDER-28).
#[allow(clippy::too_many_arguments)]
fn plugin_events(
    ended: &[(u32, i16)],
    release: &[(u32, i16)],
    notes: &[Event],
    next: &mut usize,
    params: &mut [(u32, Input, Option<f32>)],
    bufs: &[Vec<Buf>],
    (lo, hi): (usize, usize),
    keys: &mut Vec<(u32, i16)>,
    out: &mut m2_plugins::Events,
) -> (u32, Option<u32>) {
    use m2_plugins::Event as P;
    out.clear();
    let (mut offs, mut last) = (0, None);
    // A choke ends a note at once where the plugin knows how; the NoteOff releases it where not.
    for &(id, key) in ended {
        out.push(P::NoteChoke { time: 0, id: id as i32, key });
        out.push(P::NoteOff { time: 0, id: id as i32, key });
        (offs, last) = (offs + 1, Some(0));
    }
    // Let go, with their release: the edit only stopped them (RENDER-26a).
    for &(id, key) in release {
        out.push(P::NoteOff { time: 0, id: id as i32, key });
        (offs, last) = (offs + 1, Some(0));
    }
    let mut note = |e: Event, out: &mut m2_plugins::Events| match e {
        Event::On { frame, id, pitch, velocity, .. } => {
            let key = pitch.round().clamp(0.0, 127.0) as i16;
            let time = frame - lo as u32;
            out.push(P::NoteOn { time, id: id as i32, key, velocity: velocity as f64 });
            let tune = pitch as f64 - key as f64;
            if tune.abs() > 1e-6 {
                out.push(P::Tuning { time, id: id as i32, key, semitones: tune });
            }
            if keys.len() < keys.capacity() {
                keys.push((id, key));
            } else {
                debug_assert!(false, "more notes sound into a plugin than its lane holds");
            }
        }
        Event::Off { frame, id, .. } => {
            if let Some(i) = keys.iter().position(|k| k.0 == id) {
                let (_, key) = keys.swap_remove(i);
                out.push(P::NoteOff { time: frame - lo as u32, id: id as i32, key });
                (offs, last) = (offs + 1, Some(frame - lo as u32));
            }
        }
        _ => {}
    };
    let mut f = lo;
    loop {
        while *next < notes.len() && (notes[*next].frame() as usize) < hi && notes[*next].frame() as usize <= f {
            note(notes[*next], out);
            *next += 1;
        }
        if f >= hi {
            break;
        }
        for (id, input, last) in params.iter_mut() {
            match input {
                // Sent whenever it differs from the last value sent, as a port's sample is: an
                // instance that outlives its plan may have been sent another (HOST-5).
                Input::Const(v) => {
                    if *last != Some(*v as f32) {
                        out.push(P::Param { time: (f - lo) as u32, id: *id, value: *v });
                        *last = Some(*v as f32);
                    }
                }
                Input::Unset => {}
                Input::Port { src, .. } => {
                    let s = bufs[src.node][src.port][f];
                    if *last != Some(s) {
                        out.push(P::Param { time: (f - lo) as u32, id: *id, value: s as f64 });
                        *last = Some(s);
                    }
                }
            }
        }
        f += PARAM_STEP;
    }
    (offs, last)
}

impl PluginNode {
    fn process(&mut self, b: &Block, bufs: &[Vec<Buf>], out: &mut Buf, lanes: &[LaneSlot]) -> Result<()> {
        let Some(inst) = &self.inst else {
            out.fill(0.0);
            return Ok(());
        };
        // One that failed live plays silence and is never called again: the session replaces it.
        if inst.dead() {
            out.fill(0.0);
            self.status = m2_plugins::Status::Sleep;
            return Ok(());
        }
        let notes = self.notes.map_or(&[][..], |s| lanes[s].events());
        let mut next = 0;
        let channels = out.len() / BLOCK;
        let (host, h) = (inst.host(), inst.handle());
        // Its state was loaded: every parameter code owns goes again (HOME-4), at its next two
        // calls, as a plugin may apply the state at the first, over what that call brings.
        if inst.take_reloaded() {
            for p in &mut self.params {
                p.2 = None;
            }
        }
        if std::mem::take(&mut self.reset_first) {
            host.reset(h);
        }
        // One call per run: a call never spans a jump, and each gets its own transport (RENDER-9).
        for (ri, run) in b.runs.as_slice().iter().enumerate() {
            let (lo, hi) = (run.off as usize, (run.off + run.n) as usize);
            let (ended, release) = if ri == 0 { (&self.ended[..], &self.release[..]) } else { (&[][..], &[][..]) };
            let (offs, last) =
                plugin_events(ended, release, notes, &mut next, &mut self.params, bufs, (lo, hi), &mut self.keys, &mut self.events);
            self.offs += offs as u64;
            if let Some(t) = last {
                self.last_off = Some(b.engine + (lo as u32 + t) as u64);
            }
            debug_assert_eq!(self.events.dropped(), 0, "a plugin's events outgrew their room");
            let empty: &[f32] = &[];
            let mut ins = [empty; 2];
            let mut n_in = 0;
            if let Some(Input::Port { src, src_channels, .. }) = &self.audio_in {
                n_in = (*src_channels as usize).min(2);
                for (c, x) in ins.iter_mut().enumerate().take(n_in) {
                    *x = &bufs[src.node][src.port][c * BLOCK + lo..c * BLOCK + hi];
                }
            }
            let split = BLOCK.min(out.len());
            let (left, right) = out.split_at_mut(split);
            let outputs: [&mut [f32]; 2] = [&mut left[lo..hi], if channels > 1 { &mut right[lo..hi] } else { &mut [] }];
            let info = b.info[ri];
            let transport = m2_plugins::Transport {
                beats: b.beats64[lo],
                seconds: b.seconds[lo] as f64,
                bpm: info.bpm,
                playing: run.moving,
                looping: b.looping,
                tsig: info.tsig,
                bar_start: info.bar_start,
                bar: info.bar,
                steady: (b.engine + lo as u64) as i64,
            };
            let mut io = m2_plugins::SharedBuffers { inputs: ins, n_in, outputs, n_out: channels.min(2), frames: run.n };
            self.out.clear();
            self.status = host.process(h, &mut io, &self.events, &mut self.out, &transport);
            if self.status == m2_plugins::Status::Error {
                if b.strict {
                    bail!("{} reported a processing error", self.name);
                }
                // Live, only this plugin goes silent, never the song: the session replaces it from
                // its committed state (HOST-5). An atomic store: nothing here allocates.
                inst.kill();
                out.fill(0.0);
                self.status = m2_plugins::Status::Sleep;
                return Ok(());
            }
            if ri == 0 {
                self.ended.clear();
                self.release.clear();
            }
        }
        if b.strict
            && let Some(at) = out.iter().position(|x| !x.is_finite())
        {
            bail!("{} produced {} at {:.3}s", self.name, out[at], b.time(at % BLOCK));
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        let Some(inst) = &self.inst else { return Ok(()) };
        if !inst.dead() {
            inst.host().reset(inst.handle());
        }
        // Within its room (`ended` is as big as `keys`): never allocates on the audio thread.
        for k in self.keys.drain(..) {
            if self.ended.len() < self.ended.capacity() {
                self.ended.push(k);
            }
        }
        for p in &mut self.params {
            p.2 = None;
        }
        Ok(())
    }
}

/// A lane a renderer plays, by module key and lane key. A renderer's lanes are numbered in the
/// order [`lane_slots`] gives; `SwapLane` addresses them by that number, so no name reaches the
/// audio thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneRef {
    pub module: String,
    pub lane: String,
    pub kind: Kind,
}

/// The distinct lanes the nodes in `flat`'s processing order read, in node order.
pub fn lane_slots(flat: &Flat) -> Vec<LaneRef> {
    let used: HashSet<usize> = flat.order.iter().copied().collect();
    let mut out: Vec<LaneRef> = vec![];
    for (i, n) in flat.nodes.iter().enumerate() {
        if let FOp::Lane { module, lane, kind, .. } = &n.op
            && used.contains(&i)
            && !out.iter().any(|r| r.module == *module && r.lane == *lane)
        {
            out.push(LaneRef { module: module.clone(), lane: lane.clone(), kind: *kind });
        }
    }
    out
}

/// A notes lane's data at `sample_rate` in `tempo`; none when it has no data.
pub fn notes_data(data: &DataSet, module: &str, lane: &str, sample_rate: f64) -> NotesData {
    match data.lane(module, lane).map(|d| &**d) {
        Some(LaneData::Notes(clips)) => NotesData::build(clips, data.tempo(), sample_rate),
        _ => NotesData::empty(),
    }
}

enum Exec {
    Control {
        slot: usize,
        default: f32,
    },
    /// An audio lane: its player fills once per block before the graph, and this copies it.
    Audio {
        slot: usize,
    },
    /// A notes lane: its scheduler runs before the graph, once per block.
    Notes,
    Time(TimeOf),
    Const(f32),
    Kernel(KernelNode),
    Plugin(Box<PluginNode>),
    /// A plugin that isn't installed (FMT-18a): its audio input passed through, or silence.
    Missing(Option<Input>),
    Poly(Box<PolyNode>),
    /// A delay of exactly `BLOCK` samples; `ring` is indexed by engine sample mod `BLOCK`.
    Feedback {
        input: PortRef,
        ring: Buf,
    },
    NoteField(NoteField),
    /// Voice graphs only: the output its `poly` captures at this index ([`PolyNode::captures`]).
    Capture(usize),
    /// A changed edge into a kept node, or the song's output, or a new node's input from a node
    /// that plays on, during and after an activation's window: after `wait` frames `old`
    /// crossfades to `new` (RENDER-25a), then it copies `new` exactly.
    Fade {
        old: Input,
        new: Input,
        wait: u32,
    },
}

#[derive(Default)]
struct Graph {
    exec: Vec<Exec>,
    order: Vec<usize>,
    /// `bufs[node][port]`.
    bufs: Vec<Vec<Buf>>,
    /// Each node's version: a constant's changes when its value does, and every other node's is
    /// [`COPY`]. The kernels a constant feeds rewrite their input only when it changes (RENDER-22).
    versions: Vec<u64>,
    /// The last version given out.
    version: u64,
    /// Whole-graph resets since it was built (a voice's graph counts its own).
    resets: u64,
    /// Live, by top-level node holding a box: its output had a non-finite sample in its last
    /// block, or it trapped. An activation resets such a box as it moves it (RENDER-25).
    poisoned: Vec<bool>,
}

/// Builds a graph's nodes, and the instances of those that get new ones.
struct Builder<'a> {
    packages: &'a PackageSet,
    /// The revision's data: lanes, the tempo, and where assets and verified state blobs are.
    data: &'a DataSet,
    sample_rate: f64,
    /// Every lane slot, ghost lanes included, and the room each notes lane will have.
    slots: &'a [LaneRef],
    caps: &'a [Caps],
    /// Who the instances it creates report to.
    owner: &'a m2_plugins::Owner,
    /// What mode the plugin instances it creates are set to: realtime live, offline otherwise.
    mode: m2_plugins::RenderMode,
}

/// What a node's box is made of.
enum Make {
    /// A new instance, for a node of this shape (its hash).
    New(u64),
    /// None: an activation moves one in.
    Moved,
    /// The live engine's plugin instance, lent to an export (RENDER-1).
    Lent(Arc<PluginInstance>),
}

/// What each node of a graph is built with, by node.
type Boxed<'a> = &'a dyn Fn(usize) -> Make;

impl Builder<'_> {
    fn slot(&self, module: &str, lane: &str) -> Result<usize> {
        self.slots.iter().position(|r| r.module == module && r.lane == lane).with_context(|| format!("lane {module}/{lane} has no slot"))
    }

    fn caps(&self, slot: usize) -> Caps {
        self.caps.get(slot).copied().unwrap_or_default()
    }

    /// The graph of `nodes` in `order`. `room` is the room some plugin nodes need beyond their lane's:
    /// what the node they take an instance from could hold.
    fn graph(&mut self, nodes: &[FNode], order: &[usize], in_voice: bool, boxed: Boxed, room: &HashMap<usize, Caps>) -> Result<Graph> {
        let mut g = Graph { exec: Vec::with_capacity(nodes.len()), bufs: Vec::with_capacity(nodes.len()), ..Graph::default() };
        let used: HashSet<usize> = order.iter().copied().collect();
        // A voice graph's captures are numbered in node order (`PolyNode::captures`).
        let mut captures = 0;
        for (i, n) in nodes.iter().enumerate() {
            let capture = captures;
            captures += matches!(n.op, FOp::Capture(_)) as usize;
            let e = match used.contains(&i) {
                true => self.node(nodes, n, in_voice, boxed(i), false, room.get(&i).copied(), capture)?,
                false => Exec::Const(0.0),
            };
            g.push(e, &n.outputs);
        }
        g.order = order.to_vec();
        Ok(g)
    }

    /// One node. A ghost, a replaced node that plays out an activation's window, reads no notes.
    #[allow(clippy::too_many_arguments)]
    fn node(&mut self, nodes: &[FNode], n: &FNode, in_voice: bool, boxed: Make, ghost: bool, room: Option<Caps>, capture: usize) -> Result<Exec> {
        Ok(match &n.op {
            FOp::Lane { module, lane, kind: Kind::Control(_), default } => Exec::Control { slot: self.slot(module, lane)?, default: *default as f32 },
            FOp::Lane { kind: Kind::Events(_), .. } => Exec::Notes,
            FOp::Lane { module, lane, kind: Kind::Audio(_), .. } => Exec::Audio { slot: self.slot(module, lane)? },
            FOp::Time(t) => Exec::Time(*t),
            FOp::Const(v) => Exec::Const(*v as f32),
            FOp::Kernel { kernel, channels, inputs } => {
                let inst = match boxed {
                    Make::New(shape) => Some(Box::new(KernelInst::new(self.packages, n, kernel, *channels, inputs, self.sample_rate, shape)?)),
                    _ => None,
                };
                Exec::Kernel(KernelNode { inputs: inputs.to_vec(), seen: vec![COPY; inputs.len()], inst })
            }
            FOp::Plugin { plugin, inputs, .. } if self.packages.plugin_missing(plugin) => {
                // Typed with `in` first (see `PackageSet::bind_plugins`).
                Exec::Missing(inputs.first().filter(|i| matches!(i, Input::Port { .. })).cloned())
            }
            FOp::Plugin { plugin, inputs, .. } => Exec::Plugin(Box::new(self.plugin(nodes, n, plugin, inputs, boxed, ghost, room)?)),
            FOp::Poly { lane, voices, voice, .. } => {
                if in_voice {
                    bail!("{}: poly inside poly", n.name);
                }
                let slot = match ghost {
                    true => 0,
                    false => {
                        let FOp::Lane { module, lane: lane_id, .. } = &nodes[*lane].op else { bail!("{}: poly needs a notes lane", n.name) };
                        self.slot(module, lane_id)?
                    }
                };
                let pool = match boxed {
                    Make::New(shape) => {
                        let mut slots = Vec::with_capacity(*voices as usize);
                        for _ in 0..*voices {
                            let graph = self.graph(&voice.nodes, &voice.order, true, &|_| Make::New(0), &HashMap::new()).with_context(|| format!("{}: voice", n.name))?;
                            slots.push(VoiceSlot { graph, note: None, born: 0, dirty: false, last_peak: 0.0, taken: false });
                        }
                        Some(Box::new(VoicePool { voices: slots, born: 0, shape }))
                    }
                    _ => None,
                };
                let captures = voice.nodes.iter().filter_map(|n| if let FOp::Capture(p) = n.op { Some(p) } else { None }).collect();
                let plan = Vec::with_capacity(if ghost { 0 } else { 2 * self.caps(slot).ons });
                Exec::Poly(Box::new(PolyNode { slot, out: voice.out, captures, plan, pool, ghost }))
            }
            FOp::Feedback { input, channels } => Exec::Feedback { input: *input, ring: vec![0.0; *channels as usize * BLOCK] },
            FOp::NoteField(f) => Exec::NoteField(*f),
            FOp::Capture(_) => Exec::Capture(capture),
        })
    }

    /// A plugin node, and with `boxed` its instance: created through the host, its verified state
    /// loaded, and activated, on the plugin main thread while the plan before plays (RENDER-25);
    /// or one lent by the live engine, which it resets at its first call (RENDER-1).
    #[allow(clippy::too_many_arguments)]
    fn plugin(
        &mut self,
        nodes: &[FNode],
        n: &FNode,
        id: &str,
        inputs: &[Input],
        boxed: Make,
        ghost: bool,
        room: Option<Caps>,
    ) -> Result<PluginNode> {
        let info = self.packages.plugin_info(id).with_context(|| format!("{}: plugin `{id}` was not described", n.name))?;
        let rate = self.sample_rate as u32;
        let reset_first = matches!(boxed, Make::Lent(_));
        let inst = match boxed {
            Make::Lent(inst) => Some(inst),
            Make::New(shape) => {
                let blob = state_blob(self.data, n)?;
                let host = self.packages.host();
                Some(PluginInstance::create(host, &info.found, blob, self.owner, rate, shape, self.mode).with_context(|| n.name.clone())?)
            }
            Make::Moved => None,
        };
        // Inputs come in the order of the plugin's ports; a parameter is bound by the id in its
        // address, never by its place in the list (FMT-18).
        let sig = m2_ir::Kernels::plugin(self.packages, id).with_context(|| format!("{}: plugin `{id}` was not described", n.name))?;
        let (mut audio_in, mut notes, mut params) = (None, None, vec![]);
        for (port, input) in sig.inputs.iter().zip(inputs) {
            match (port.name.as_str(), input) {
                ("in", _) => audio_in = Some(input.clone()),
                ("notes", Input::Port { src, .. }) if !ghost => {
                    if let FOp::Lane { module, lane, .. } = &nodes[src.node].op {
                        notes = Some(self.slot(module, lane)?);
                    }
                }
                ("notes", _) | (_, Input::Unset) => {}
                (name, _) => {
                    let (_, pid) = packages::address(name).with_context(|| format!("{}: `{name}` isn't a parameter address", n.name))?;
                    params.push((pid, input.clone(), None));
                }
            }
        }
        // Room for one run's notes, a Tuning for each On, every parameter sampled, and a choke and
        // a NoteOff for every note it holds, a NoteOff for every note it lets go, as many as its
        // lane's room holds or the node it took its instance from held.
        let caps = notes.map_or(Caps::default(), |s| self.caps(s));
        let held = |c: Caps| c.sounding + 2 * c.ons + 8;
        let held = held(caps).max(room.map_or(0, held));
        let events = caps.events() + caps.ons + (BLOCK / PARAM_STEP) * params.len() + 8 + 3 * held;
        Ok(PluginNode {
            name: n.name.clone(),
            inst,
            audio_in,
            notes,
            keys: Vec::with_capacity(held),
            ended: Vec::with_capacity(held),
            release: Vec::with_capacity(held),
            params,
            events: m2_plugins::Events::with_capacity(events),
            out: m2_plugins::Events::with_capacity(OUT_EVENTS),
            status: m2_plugins::Status::Continue,
            offs: 0,
            last_off: None,
            reset_first,
        })
    }
}

/// The verified state blob plugin node `n` loads, read: only a blob whose sidecar names its plugin
/// (FMT-17). Without one it plays its defaults, and the data's diagnostics say why.
pub fn state_blob(data: &DataSet, n: &FNode) -> Result<Option<Arc<m2_plugins::Blob>>> {
    let FOp::Plugin { state: Some(keys), .. } = &n.op else { return Ok(None) };
    let Some(path) = data.state(&n.module, &keys.iter().map(String::as_str).collect::<Vec<_>>()) else { return Ok(None) };
    let bytes = std::fs::read(&path).with_context(|| format!("{}: reading plugin state {}", n.name, path.display()))?;
    Ok(Some(Arc::new(m2_plugins::Blob::new(bytes))))
}

/// Whether plugin node `new`, which keeps `old`'s instance, stops setting a parameter `old` set in
/// code (its literal or cable deleted): the instance keeps the value it was last sent (HOME-5a).
fn releases(old: &FNode, new: &FNode, packages: &PackageSet) -> bool {
    let (FOp::Plugin { plugin, inputs: was, .. }, FOp::Plugin { inputs: now, .. }) = (&old.op, &new.op) else { return false };
    let Some(sig) = m2_ir::Kernels::plugin(packages, plugin) else { return false };
    let param = |name: &str| !matches!(name, "in" | "notes");
    sig.inputs.iter().zip(was.iter().zip(now)).any(|(port, (a, b))| param(&port.name) && *a != Input::Unset && *b == Input::Unset)
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

/// A fade node's block (RENDER-25a): during the window, each frame crossfades from `old` to `new`;
/// after it, `new` is copied only when its version changes, and the fade takes that version, so a
/// steady cell behind it still costs nothing per block (RENDER-22). `had` is its version before;
/// returns its version now. Never allocates.
fn fade(b: &Block, (old, new, wait): (&Input, &Input, u32), bufs: &[Vec<Buf>], versions: &[u64], had: u64, out: &mut Buf) -> u64 {
    // Channel `c` of an input, a mono one in every channel.
    let read = |i: &Input, c: usize, f: usize| match i {
        Input::Port { src, .. } => {
            let s = &bufs[src.node][src.port];
            s[c.min(s.len() / BLOCK - 1) * BLOCK + f]
        }
        Input::Const(v) => *v as f32,
        Input::Unset => 0.0,
    };
    let channels = out.len() / BLOCK;
    match b.window {
        Some((from, len)) if b.engine < from + (wait + len) as u64 => {
            for c in 0..channels {
                for f in 0..BLOCK {
                    let k = (b.engine + f as u64 - from) as u32;
                    let o = read(old, c, f);
                    out[c * BLOCK + f] = if k < wait { o } else { xfade(o, read(new, c, f), k - wait, len) };
                }
            }
            COPY
        }
        _ => {
            let v = match new {
                Input::Port { src, .. } => versions[src.node],
                _ => 0,
            };
            if v == COPY || v != had {
                for c in 0..channels {
                    for f in 0..BLOCK {
                        out[c * BLOCK + f] = read(new, c, f);
                    }
                }
            }
            v
        }
    }
}

impl Graph {
    /// Append node `e`, with outputs of `outputs`' kinds.
    fn push(&mut self, e: Exec, outputs: &[Kind]) {
        let mut bufs: Vec<Buf> =
            outputs.iter().map(|k| vec![0.0; if matches!(k, Kind::Events(_)) { 0 } else { k.channels() as usize * BLOCK }]).collect();
        // Constants change only through their cells: filled once.
        if let Exec::Const(v) = e {
            bufs.iter_mut().for_each(|b| b.fill(v));
        }
        self.versions.push(if matches!(e, Exec::Const(_)) { 0 } else { COPY });
        self.exec.push(e);
        self.bufs.push(bufs);
        self.poisoned.push(false);
    }

    /// Process frames `seg` of block `b`. Buffers are indexed by position in the block. A voice graph
    /// gets the outputs around its `poly` and what its captures read in them.
    fn process(
        &mut self,
        b: &Block,
        seg: Seg,
        outer: Option<(&[Vec<Buf>], &[PortRef])>,
        note: Option<VoiceNote>,
        lanes: &[LaneSlot],
    ) -> Result<()> {
        let n = seg.n;
        for oi in 0..self.order.len() {
            let i = self.order[oi];
            let mut out = std::mem::take(&mut self.bufs[i]);
            let (bufs, versions) = (&self.bufs, &self.versions);
            let mut version = None;
            let done = match &mut self.exec[i] {
                Exec::Control { slot, default } => {
                    let o = &mut out[0][seg.range(0)];
                    match &lanes[*slot] {
                        LaneSlot::Control(Some(lane)) if !lane.is_empty() => {
                            for (x, &beat) in o.iter_mut().zip(&b.beats64[seg.off..]) {
                                *x = lane.value_at(beat, *default as f64) as f32;
                            }
                        }
                        _ => o.fill(*default),
                    }
                    Ok(())
                }
                Exec::Audio { slot } => {
                    debug_assert_eq!((seg.off, seg.n), (0, BLOCK), "audio lanes only play at the top level");
                    match &lanes[*slot] {
                        LaneSlot::Audio(Some(p)) => {
                            let n = out[0].len().min(p.out().len());
                            out[0][..n].copy_from_slice(&p.out()[..n]);
                        }
                        _ => out[0].fill(0.0),
                    }
                    Ok(())
                }
                Exec::Notes | Exec::Const(_) => Ok(()),
                Exec::Time(TimeOf::Beats) => {
                    out[0][seg.range(0)].copy_from_slice(&b.beats[seg.off..seg.off + n]);
                    Ok(())
                }
                Exec::Time(TimeOf::Seconds) => {
                    out[0][seg.range(0)].copy_from_slice(&b.seconds[seg.off..seg.off + n]);
                    Ok(())
                }
                Exec::Kernel(k) => k.process(b, seg, bufs, versions, &mut out),
                Exec::Poly(p) => p.process(b, bufs, &mut out[0], lanes),
                Exec::Plugin(p) => p.process(b, bufs, &mut out[0], lanes),
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
                    Ok(())
                }
                Exec::Feedback { ring, .. } => {
                    for c in 0..out[0].len() / BLOCK {
                        for f in 0..n {
                            let at = (b.engine as usize + seg.off + f) % BLOCK;
                            out[0][c * BLOCK + seg.off + f] = ring[c * BLOCK + at];
                        }
                    }
                    Ok(())
                }
                Exec::NoteField(field) => {
                    let note = note.expect("note fields only run in voices");
                    let o = &mut out[0][seg.range(0)];
                    match field {
                        NoteField::Pitch => o.fill(note.pitch),
                        NoteField::Velocity => o.fill(note.velocity),
                        NoteField::Pressure | NoteField::Timbre => o.fill(0.0),
                        NoteField::Gate => {
                            // Open from the note's On until its Off, on the engine clock (RENDER-13).
                            for (k, x) in o.iter_mut().enumerate() {
                                *x = ((b.engine + (seg.off + k) as u64) < note.off) as u8 as f32;
                            }
                        }
                    }
                    Ok(())
                }
                Exec::Capture(k) => {
                    let (outer, captures) = outer.expect("captures only run in voices");
                    let p = captures[*k];
                    let src = &outer[p.node][p.port];
                    for c in 0..out[0].len() / BLOCK {
                        out[0][seg.range(c)].copy_from_slice(&src[seg.range(c.min(src.len() / BLOCK - 1))]);
                    }
                    Ok(())
                }
                Exec::Fade { old, new, wait } => {
                    version = Some(fade(b, (old, new, *wait), bufs, versions, versions[i], &mut out[0]));
                    Ok(())
                }
            };
            // RENDER-25: live, a box whose output went non-finite, or that trapped, is marked; an
            // activation resets it as it moves it. A finite block clears the mark.
            if outer.is_none() && !b.strict && matches!(self.exec[i], Exec::Kernel(_) | Exec::Poly(_) | Exec::Plugin(_) | Exec::Feedback { .. }) {
                self.poisoned[i] = done.is_err() || out.iter().any(|o| o.iter().fold(false, |a, x| a | !x.is_finite()));
            }
            self.bufs[i] = out;
            if let Some(v) = version {
                self.versions[i] = v;
            }
            done?;
        }
        // Feedback stores its input for `BLOCK` samples later.
        for i in 0..self.exec.len() {
            if let Exec::Feedback { input, ring } = &mut self.exec[i] {
                let src = &self.bufs[input.node][input.port];
                let src_ch = src.len() / BLOCK;
                for c in 0..ring.len() / BLOCK {
                    let sc = if c < src_ch { c } else { 0 };
                    for f in 0..n {
                        let at = (b.engine as usize + seg.off + f) % BLOCK;
                        ring[c * BLOCK + at] = src[sc * BLOCK + seg.off + f];
                    }
                }
            }
        }
        Ok(())
    }

    /// Back to the state right after building, every mark cleared (RENDER-10).
    fn reset(&mut self) -> Result<()> {
        // Counted here, so no path resets the graph unseen (RENDER-10).
        self.resets += 1;
        self.poisoned.iter_mut().for_each(|p| *p = false);
        for i in 0..self.exec.len() {
            self.reset_node(i)?;
        }
        Ok(())
    }

    /// Node `i`'s box back to the state right after building, as a panic resets it: a kernel's
    /// `k_reset`, a pool's voices with their notes dropped, a plugin's `reset` with its notes to
    /// end, a ring zeroed. Never allocates.
    fn reset_node(&mut self, i: usize) -> Result<()> {
        match &mut self.exec[i] {
            Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.reset()?,
            Exec::Plugin(p) => p.reset()?,
            Exec::Feedback { ring, .. } => ring.fill(0.0),
            Exec::Poly(p) => {
                p.plan.clear();
                if let Some(pool) = &mut p.pool {
                    pool.born = 0;
                    for v in &mut pool.voices {
                        if v.dirty {
                            v.graph.reset()?;
                        }
                        (v.note, v.born, v.dirty, v.last_peak, v.taken) = (None, 0, false, 0.0, false);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The notes sounding into node `i` end now, at engine sample `clock`: a poly's voices go into
    /// their release, a plugin's notes get a NoteOff at its next call (RENDER-26a). Never allocates.
    fn release(&mut self, i: usize, clock: u64) {
        match &mut self.exec[i] {
            Exec::Poly(p) => {
                for n in p.pool.iter_mut().flat_map(|pool| &mut pool.voices).filter_map(|v| v.note.as_mut()) {
                    n.off = n.off.min(clock);
                }
            }
            Exec::Plugin(p) => {
                let PluginNode { keys, release, .. } = &mut **p;
                for k in keys.drain(..) {
                    if release.len() < release.capacity() {
                        release.push(k);
                    }
                }
            }
            _ => {}
        }
    }

    /// Node `i`'s input `edge` reads fade node `f` from now on (RENDER-25a).
    fn rewire(&mut self, i: usize, edge: Edge, f: usize) {
        let ch = (self.bufs[f][0].len() / BLOCK) as u8;
        let src = PortRef { node: f, port: 0 };
        // A constant filled both channels; a port keeps the channels it copied.
        let reads = |was: Option<&Input>| {
            let channels = if let Some(Input::Port { channels, .. }) = was { *channels } else { 2 };
            Input::Port { src, src_channels: ch, channels }
        };
        match (&mut self.exec[i], edge) {
            (Exec::Kernel(k), Edge::Input(p)) => k.inputs[p] = reads(k.inputs.get(p)),
            (Exec::Plugin(p), Edge::Input(_)) => p.audio_in = Some(reads(p.audio_in.as_ref())),
            (Exec::Missing(m), Edge::Input(_)) => *m = Some(Input::Port { src, src_channels: ch, channels: ch }),
            (Exec::Poly(p), Edge::Capture(k)) => p.captures[k] = src,
            (Exec::Feedback { input, .. }, Edge::Feedback) => *input = src,
            _ => debug_assert!(false, "node {i} has no input {edge:?}"),
        }
    }

    /// What constant `node` of poly `poly`'s first voice holds.
    fn voice_const(&self, poly: usize, node: usize) -> Option<f32> {
        let Exec::Poly(p) = self.exec.get(poly)? else { return None };
        match p.pool.as_ref()?.voices.first()?.graph.exec.get(node)? {
            Exec::Const(v) => Some(*v),
            _ => None,
        }
    }

    /// Reset every kernel once, in voices too: a kernel's first reset may grow its memory
    /// (it builds the new state before dropping the old), so it happens here, off the audio
    /// thread, and never again later. A reset is the same as init, so nothing sounds different.
    fn prewarm(&mut self) -> Result<()> {
        for e in &mut self.exec {
            match e {
                Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.reset()?,
                Exec::Poly(p) => {
                    for v in p.pool.iter_mut().flat_map(|pool| &mut pool.voices) {
                        v.graph.prewarm()?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The memory every kernel instance holds, in bytes.
    fn kernel_memory(&self) -> usize {
        self.exec
            .iter()
            .map(|e| match e {
                Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.memory.data_size(&k.store),
                Exec::Poly(p) => p.pool.iter().flat_map(|pool| &pool.voices).map(|v| v.graph.kernel_memory()).sum(),
                _ => 0,
            })
            .sum()
    }

    /// Write a cell's new value into constant `node`'s buffer. A step gives it a new version, so
    /// the kernels it feeds rewrite their input once; a ramp is copied at every call until the
    /// step that ends it (RENDER-22a). Never allocates.
    fn write_const(&mut self, node: usize, w: Write) {
        let Exec::Const(v) = &mut self.exec[node] else {
            debug_assert!(false, "cell {node} isn't a constant");
            return;
        };
        let (Write::Step(to) | Write::Ramp(_, to)) = w;
        *v = to;
        for b in &mut self.bufs[node] {
            for ch in b.chunks_mut(BLOCK) {
                w.fill(ch);
            }
        }
        self.versions[node] = match w {
            Write::Step(_) => {
                self.version += 1;
                self.version
            }
            Write::Ramp(..) => COPY,
        };
    }

    /// Whether any kernel still holds sound that will come out later.
    fn active(&mut self) -> Result<bool> {
        for e in &mut self.exec {
            let active = match e {
                Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.active()?,
                Exec::Feedback { ring, .. } => ring.iter().any(|x| x.abs() > QUIET),
                _ => false,
            };
            if active {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl PolyNode {
    /// One block. The lane's events say which notes start and end; voices are given notes in
    /// event order, and a voice's graph is split where its note changes. Never allocates.
    fn process(&mut self, b: &Block, outer: &[Vec<Buf>], out: &mut Buf, lanes: &[LaneSlot]) -> Result<()> {
        let (start, end) = (b.engine, b.engine + BLOCK as u64);
        let events = if self.ghost { &[] } else { lanes[self.slot].events() };
        let PolyNode { out: voice_out, captures, plan, pool, .. } = self;
        let Some(pool) = pool else {
            out.fill(0.0);
            return Ok(());
        };
        let VoicePool { voices, born, .. } = &mut **pool;
        // Offs for notes already in voices. A stolen note's Off finds nothing.
        for e in events {
            if let Event::Off { frame, id, .. } = *e
                && let Some(n) = voices.iter_mut().find_map(|v| v.note.as_mut().filter(|n| n.id == id))
            {
                n.off = start + frame as u64;
            }
        }
        // A note whose Off landed on this block line ends where the last block would have ended
        // it: same peak, same `active`, as nothing ran in between.
        for v in voices.iter_mut() {
            if let Some(n) = v.note
                && n.off == start
                && v.last_peak < QUIET
                && !v.graph.active()?
            {
                v.note = None;
            }
        }
        // Give this block's notes voices, in event order: the first free voice, else the oldest.
        plan.clear();
        for e in events {
            if let Event::On { frame, id, pitch, velocity, .. } = *e {
                let slot = match voices.iter().position(|v| v.note.is_none() && !v.taken) {
                    Some(i) => i,
                    None => (0..voices.len()).min_by_key(|&i| voices[i].born).expect("at least one voice"),
                };
                if plan.len() == plan.capacity() {
                    debug_assert!(false, "more notes start than the lane holds");
                    continue;
                }
                *born += 1;
                voices[slot].born = *born;
                voices[slot].taken = true;
                plan.push(Planned { voice: slot as u16, frame: frame as u16, note: VoiceNote { id, pitch, velocity, off: u64::MAX } });
            }
        }
        // Offs for notes that started this block.
        for e in events {
            if let Event::Off { frame, id, .. } = *e
                && let Some(p) = plan.iter_mut().find(|p| p.note.id == id)
            {
                p.note.off = start + frame as u64;
            }
        }

        out.fill(0.0);
        let out_ch = out.len() / BLOCK;
        let outer = (outer, &captures[..]);
        for (vi, v) in voices.iter_mut().enumerate() {
            let taken = std::mem::take(&mut v.taken);
            if v.note.is_none() && !taken {
                continue;
            }
            // Split the block wherever this voice starts a new note.
            let mut at = 0;
            let mut current = v.note;
            let mut peak = 0.0f32;
            let mut segment = |v: &mut VoiceSlot, seg: Seg, note: VoiceNote, peak: &mut f32| -> Result<()> {
                v.graph.process(b, seg, Some(outer), Some(note), &[])?;
                let vo = &v.graph.bufs[voice_out.node][voice_out.port];
                let v_ch = vo.len() / BLOCK;
                for c in 0..out_ch {
                    let sc = if c < v_ch { c } else { 0 };
                    for (o, x) in out[seg.range(c)].iter_mut().zip(&vo[seg.range(sc)]) {
                        *o += x;
                        *peak = peak.max(x.abs());
                    }
                }
                Ok(())
            };
            for p in plan.iter().filter(|p| p.voice as usize == vi) {
                let off = p.frame as usize;
                if let Some(c) = current
                    && off > at
                {
                    segment(v, Seg { off: at, n: off - at }, c, &mut peak)?;
                }
                if v.dirty {
                    v.graph.reset()?;
                }
                v.dirty = true;
                v.note = Some(p.note);
                peak = 0.0;
                current = Some(p.note);
                at = off;
            }
            if let Some(c) = current {
                segment(v, Seg { off: at, n: BLOCK - at }, c, &mut peak)?;
            }
            if let Some(n) = v.note {
                if n.off < end {
                    if peak < QUIET && !v.graph.active()? {
                        v.note = None;
                    }
                } else {
                    v.last_peak = peak;
                }
            }
        }
        Ok(())
    }
}

/// Frames rendered but not yet handed out, with the transport position after each: at most
/// one block, so it never grows (RENDER-28).
pub(crate) struct Pending {
    frames: [[f32; 2]; BLOCK],
    next: [u64; BLOCK],
    read: usize,
    len: usize,
}

impl Pending {
    pub(crate) const EMPTY: Pending = Pending { frames: [[0.0; 2]; BLOCK], next: [0; BLOCK], read: 0, len: 0 };

    pub(crate) fn is_empty(&self) -> bool {
        self.read == self.len
    }

    /// Take a block's frames; `runs` gives each its transport position, and `after` is where the
    /// transport is after the block. The position after a run's last frame is where the next frame
    /// plays: a loop's start after its end, never the end itself.
    pub(crate) fn load(&mut self, frames: &[[f32; 2]; BLOCK], runs: &Runs, after: u64) {
        self.frames = *frames;
        let runs = runs.as_slice();
        for (i, r) in runs.iter().enumerate() {
            for f in r.off..r.off + r.n {
                self.next[f as usize] = r.frame(f) + r.moving as u64;
            }
            if r.moving {
                self.next[(r.off + r.n - 1) as usize] = runs.get(i + 1).map_or(after, |n| n.at);
            }
        }
        (self.read, self.len) = (0, BLOCK);
    }

    /// Silence at a held position (no renderer, or idle).
    pub(crate) fn silence(&mut self, pos: u64) {
        self.frames = [[0.0; 2]; BLOCK];
        self.next = [pos; BLOCK];
        (self.read, self.len) = (0, BLOCK);
    }

    /// The next frame, and the transport position after it.
    pub(crate) fn pop(&mut self) -> ([f32; 2], u64) {
        let i = self.read;
        self.read += 1;
        (self.frames[i], self.next[i])
    }

    pub(crate) fn clear(&mut self) {
        (self.read, self.len) = (0, 0);
    }
}

/// Unique per renderer: what `SwapLane` checks it reaches the renderer it was made for.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Renders a checked song: one plan, the lanes its nodes read, and the song position.
pub struct Renderer {
    plan: Plan,
    lanes: Lanes,
    tempo: TempoMap,
    arrangement: Arc<Arrangement>,
    sample_rate: f64,
    transport: Transport,
    /// The engine clock of the next block.
    clock: u64,
    /// The id the next note gets, across every lane: unique for the engine's life, as the
    /// activation that replaces this renderer takes it over (RENDER-26a).
    next_note: u32,
    /// The last block, and its transport.
    frames: [[f32; 2]; BLOCK],
    runs: Runs,
    pending: Pending,
    delivered: u64,
    strict: bool,
    non_finite: u64,
    /// A kernel crashed live: it renders silence from then on.
    broken: bool,
    trace: Option<Trace>,
    dropped: u32,
}

impl Renderer {
    /// A renderer for a checked graph and the data it plays: a revision's [`DataSet`], which
    /// also says where its assets and verified plugin states are ([`DataSet::empty`] for none).
    /// Every instance is new. It plays from sample 0, offline, its plugins in offline mode
    /// (RENDER-1).
    pub fn new(flat: &Flat, data: &DataSet, packages: &PackageSet, sample_rate: u32) -> Result<Renderer> {
        let (cache, owner) = (&mut audio::Cache::default(), &m2_plugins::Owner::discard());
        Renderer::build(flat, data, packages, sample_rate, cache, owner, Basis::Fresh, m2_plugins::RenderMode::Offline)
    }

    /// A renderer for the live engine, sharing decoded assets and clip renders with earlier ones
    /// through `cache` (a live session's), its new plugin instances reporting to `owner`. Against
    /// [`Basis::Keep`] it is a plan the engine activates in place of the one that plays: every
    /// instance that continues and has the same shape moves into it at the block line, and the
    /// rest are made here, plugins created and activated on the plugin main thread while the old
    /// plan plays (RENDER-25). A prepare that fails drops what it made, here; the one call it makes
    /// into what plays comes last: a kept plugin whose committed state changed gets it loaded, on
    /// the plugin main thread (RENDER-26b). [`Basis::Lend`] is an offline render sharing the live
    /// engine's plugin instances (RENDER-1). Its new plugin instances run in realtime mode: it
    /// plays live.
    pub fn prepare(
        flat: &Flat,
        data: &DataSet,
        packages: &PackageSet,
        sample_rate: u32,
        cache: &mut audio::Cache,
        owner: &m2_plugins::Owner,
        basis: Basis,
    ) -> Result<Renderer> {
        Renderer::build(flat, data, packages, sample_rate, cache, owner, basis, m2_plugins::RenderMode::Realtime)
    }

    /// [`Renderer::prepare`], its new plugin instances set to `mode`: realtime for the live
    /// engine, offline for every other render (RENDER-1).
    #[allow(clippy::too_many_arguments)]
    fn build(
        flat: &Flat,
        data: &DataSet,
        packages: &PackageSet,
        sample_rate: u32,
        cache: &mut audio::Cache,
        owner: &m2_plugins::Owner,
        basis: Basis,
        mode: m2_plugins::RenderMode,
    ) -> Result<Renderer> {
        let tempo = data.tempo().clone();
        let rate = sample_rate as f64;
        let keep = match &basis {
            Basis::Keep(k) if k.info.rate != sample_rate => bail!("a plan is prepared against one at {} Hz, not {sample_rate}", k.info.rate),
            Basis::Keep(k) => Some(k),
            Basis::Fresh | Basis::Lend(_) => None,
        };
        let lent = if let Basis::Lend(l) = &basis { Some(*l) } else { None };
        let mut lanes = LanePlan::new(flat, data, cache, &tempo, rate, keep);
        let used = used(flat);
        let shapes: Vec<Option<Shape>> = flat.nodes.iter().zip(&used).map(|(n, &u)| if u { Shape::of(n, packages) } else { None }).collect();
        let blobs: Vec<Option<m2_song::Sha>> = flat.nodes.iter().map(|n| plan::blob(n, data)).collect();
        let w = match keep {
            Some(k) => keep::wire(k, flat, &shapes, &blobs, packages, &mut lanes),
            None => Wiring::fresh(flat),
        };
        let mut b = Builder { packages, data, sample_rate: rate, slots: &lanes.lanes.refs, caps: &lanes.caps, owner, mode };
        let boxed = |i: usize| match (w.kept[i], lent.and_then(|l| l.get(&i))) {
            (true, _) => Make::Moved,
            (false, Some(inst)) => Make::Lent(inst.clone()),
            (false, None) => Make::New(shapes[i].as_ref().map_or(0, Shape::hash64)),
        };
        let mut graph = b.graph(&flat.nodes, &flat.order, false, &boxed, &w.room)?;
        graph.prewarm()?;
        // Ghosts, then fade nodes, after the new graph's nodes (RENDER-25a).
        let n = flat.nodes.len();
        for (g, (_, node)) in w.ghosts.iter().enumerate() {
            let e = b.node(&flat.nodes, node, false, Make::Moved, true, w.room.get(&(n + g)).copied(), 0)?;
            graph.push(e, &node.outputs);
        }
        let len = (0.005 * rate).round().max(1.0) as u32;
        for f in &w.fades {
            let wait = if f.waits { len } else { 0 };
            graph.push(Exec::Fade { old: f.old.clone(), new: f.new.clone(), wait }, &[Kind::Audio(f.channels)]);
        }
        for &(i, edge, f) in &w.rewire {
            graph.rewire(i, edge, f);
        }
        let ghosts = n..n + w.ghosts.len();
        let order_after: Vec<usize> = w.order.iter().copied().filter(|i| !ghosts.contains(i)).collect();
        graph.order = w.order.clone();
        let mut cells = Cells::new(&cells::cells(flat));
        // What moves in, and what it keeps and makes.
        let mut stats = Stats { ghosts: w.ghosts.len(), fades: w.fades.len(), ..Stats::default() };
        for (i, s) in shapes.iter().enumerate() {
            match (s, w.kept[i]) {
                (Some(s), true) => stats.kept.add(s),
                (Some(s), false) => {
                    stats.fresh.add(s);
                    stats.fresh_nodes.push(i);
                }
                _ => {}
            }
        }
        let plugins: Vec<PluginAt> = (0..n)
            .filter_map(|i| {
                let Exec::Plugin(p) = &graph.exec[i] else { return None };
                let (handle, inst) = match (&p.inst, keep, w.same[i]) {
                    (Some(inst), ..) => (inst.handle(), Arc::downgrade(inst)),
                    (None, Some(k), Some(j)) => k.info.plugin(j).map(|at| (at.handle, at.inst.clone()))?,
                    _ => return None,
                };
                Some(PluginAt { node: i, handle, inst, blob: blobs[i] })
            })
            .collect();
        // State loads into kept instances, last, once nothing else can fail (RENDER-26b): each
        // kept plugin whose committed blob isn't the one it was last given, compared node by node.
        // The host skips a blob the instance holds already (HOME-6); after a load its node sends
        // the parameters code owns again (HOME-4).
        if let Some(k) = keep {
            let (mut loads, mut released) = (vec![], vec![]);
            for (at, p) in plugins.iter().enumerate() {
                let Some(j) = w.same[p.node].filter(|_| w.kept[p.node]) else { continue };
                if releases(&k.old.nodes[j], &flat.nodes[p.node], packages) {
                    released.extend(p.inst.upgrade());
                }
                if k.info.plugin(j).is_some_and(|o| o.blob != p.blob)
                    && let Some(blob) = state_blob(data, &flat.nodes[p.node])?
                {
                    loads.push((at, blob));
                }
            }
            // A parameter the code stopped setting keeps its value (HOME-5a), which no file records.
            released.iter().for_each(|inst| inst.release());
            let host = packages.host();
            for (at, blob) in loads {
                let p = &plugins[at];
                let loaded = host.load_state(p.handle, blob);
                let inst = p.inst.upgrade();
                // Its code-owned parameters go again whatever the plugin made of the bytes.
                if let Some(inst) = &inst
                    && !matches!(loaded, Ok(false))
                {
                    inst.mark_reloaded();
                }
                match loaded {
                    Ok(true) => {
                        stats.loads += 1;
                        inst.iter().for_each(|i| i.loaded());
                        // Compensation isn't built, and the check refuses latency at default
                        // state (RENDER-21): a patch that adds some plays uncompensated, and says so.
                        if let Ok(Ok(frames)) = host.latency(p.handle).recv()
                            && frames > 0
                        {
                            stats.latent.push((p.node, frames));
                        }
                    }
                    Ok(false) => {}
                    // Never fails the prepare: loads before it have landed in instances that play.
                    // It plays on as the plugin left it. The plan says it was given this blob, so
                    // the next plan doesn't try it again, nor say so again, until another is
                    // committed; then that one loads whatever this left (HOME-6).
                    Err(e) => stats.unloaded.push((p.node, format!("{e:#}"))),
                }
            }
        }
        let swaps = plan::audio_swaps(&lanes.moves);
        let (lane_info, ghost_lanes) = (lanes.info(), plan::ghost_lanes(&lanes.lanes));
        let windowed = !w.ghosts.is_empty() || !w.fades.is_empty() || swaps > 0;
        let entry = keep.map(|k| {
            // Continued cells, carried at entry (RENDER-22a).
            let old: HashMap<usize, CellId> = cells::cells(k.old)
                .iter()
                .enumerate()
                .filter_map(|(c, info)| if let CellAt::Top(j) = info.at { Some((j, CellId(c as u32))) } else { None })
                .collect();
            let infos = cells::cells(flat);
            let mut pairs = vec![];
            let mut voice_cells = vec![];
            for (c, info) in infos.iter().enumerate() {
                match info.at {
                    CellAt::Top(i) => pairs.extend(w.same[i].and_then(|j| old.get(&j)).map(|&o| (CellId(c as u32), o))),
                    CellAt::Voice { poly, .. } if w.kept[poly] => voice_cells.push(CellId(c as u32)),
                    CellAt::Voice { .. } => {}
                }
            }
            cells.reserve_carry(pairs.len() + voice_cells.len());
            Box::new(Entry { after: k.info.id, moves: w.moves, lanes: lanes.moves, cells: pairs, voice_cells, release: w.release })
        });
        let id = GENERATION.fetch_add(1, Ordering::Relaxed);
        let info = Arc::new(PlanInfo {
            id,
            rate: sample_rate,
            shapes,
            plugins,
            lanes: lane_info,
            cell_slots: cells.slots().clone(),
            cell_map: cells.map().clone(),
        });
        let plan = Plan {
            id,
            plugins: graph.exec.iter().any(|e| matches!(e, Exec::Plugin(_))),
            graph,
            nodes: n,
            cells,
            out: w.out,
            entry,
            leftovers: windowed.then(|| Plan::leftovers(ghosts.len(), ghost_lanes, swaps)),
            ghosts,
            order_after,
            fade: if windowed { len } else { 0 },
            wait: if w.fades.iter().any(|f| f.waits) { len } else { 0 },
            faded_from: None,
            stats,
            info,
        };
        Ok(Renderer {
            plan,
            lanes: lanes.lanes,
            tempo,
            arrangement: data.arrangement.clone(),
            sample_rate: rate,
            transport: Transport::new(0, true),
            clock: 0,
            next_note: 1,
            frames: [[0.0; 2]; BLOCK],
            runs: Runs::one(Run { off: 0, n: BLOCK as u32, at: 0, moving: true, jump: Jump::None }),
            pending: Pending::EMPTY,
            delivered: 0,
            strict: true,
            non_finite: 0,
            broken: false,
            trace: None,
            dropped: 0,
        })
    }

    /// An offline render from 0, as [`Renderer::new`] makes one, playing the live engine's plugin
    /// instances that `lent` names by node ([`PlanInfo::lend`], RENDER-1): each is reset at its
    /// first call. The live engine must be held while it renders.
    pub fn lending(
        flat: &Flat,
        data: &DataSet,
        packages: &PackageSet,
        sample_rate: u32,
        lent: &HashMap<usize, Arc<PluginInstance>>,
    ) -> Result<Renderer> {
        let (cache, owner) = (&mut audio::Cache::default(), &m2_plugins::Owner::discard());
        Renderer::build(flat, data, packages, sample_rate, cache, owner, Basis::Lend(lent), m2_plugins::RenderMode::Offline)
    }

    /// Play `inst`, a new instance, at top-level node `node` in place of the live one lent to this
    /// offline render ([`Renderer::lending`], RENDER-1): taking that one offline failed, or the
    /// live engine didn't hold. Only before the render's first block. False when the node plays
    /// no lent instance.
    pub fn unlend(&mut self, node: usize, inst: Arc<PluginInstance>) -> bool {
        match self.plan.graph.exec.get_mut(node) {
            Some(Exec::Plugin(p)) if p.reset_first => {
                (p.inst, p.reset_first) = (Some(inst), false);
                true
            }
            _ => false,
        }
    }

    /// A range render (RENDER-4): fresh from init at engine time 0, with the transport starting
    /// at `t0` and jump semantics, so nothing sounding at `t0` is chased. From 0 it is exactly
    /// [`Renderer::new`].
    pub fn starting_at(flat: &Flat, data: &DataSet, packages: &PackageSet, sample_rate: u32, t0: u64) -> Result<Renderer> {
        let mut r = Renderer::new(flat, data, packages, sample_rate)?;
        r.transport = Transport::starting(t0);
        Ok(r)
    }

    /// Strict (the default): a NaN or infinity from any kernel is an error naming it.
    /// Live: such samples are silenced at the output and counted in [`Self::non_finite`].
    pub fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    fn block(&mut self, clock: u64) -> Block {
        let runs = self.runs;
        let mut b = Block {
            engine: clock,
            runs,
            info: [RunInfo::default(); MAX_RUNS],
            looping: None,
            beats: [0.0; BLOCK],
            seconds: [0.0; BLOCK],
            beats64: [0.0; BLOCK],
            sample_rate: self.sample_rate,
            strict: self.strict,
            window: self.plan.faded_from.map(|f| (f, self.plan.fade)),
        };
        for r in runs.as_slice() {
            for f in r.off..r.off + r.n {
                let s = r.frame(f) as f64 / self.sample_rate;
                let beat = self.tempo.beat_at(s);
                b.seconds[f as usize] = s as f32;
                b.beats[f as usize] = beat as f32;
                b.beats64[f as usize] = beat;
            }
        }
        if self.plan.plugins {
            for (i, r) in runs.as_slice().iter().enumerate() {
                let beat = b.beats64[r.off as usize];
                let meter = self.arrangement.meter_at(beat);
                let (bar, into) = self.arrangement.bar_at(beat);
                b.info[i] = RunInfo {
                    bpm: self.tempo.bpm_at(beat),
                    tsig: (meter.beats as u16, meter.unit as u16),
                    bar_start: beat - into,
                    bar: bar as i32,
                };
            }
            b.looping = self.transport.looping().map(|l| {
                let s = |beat: f64| self.tempo.seconds_at(beat);
                ((l.start_beat, l.end_beat), (s(l.start_beat), s(l.end_beat)))
            });
        }
        b
    }

    /// Render the next block at engine clock `clock`, advancing the transport. The live
    /// engine's step, and the offline one's. Never allocates while it succeeds (RENDER-28).
    pub fn render_block(&mut self, clock: u64) -> Result<&[[f32; 2]; BLOCK]> {
        self.runs = self.transport.advance();
        self.clock = clock + BLOCK as u64;
        // Cell values written since the last block line (RENDER-22, RENDER-28); offline renders
        // never get any.
        self.plan.cells.apply(&mut self.plan.graph);
        if self.broken {
            self.frames = [[0.0; 2]; BLOCK];
            return Ok(&self.frames);
        }
        // Lanes play before the graph, once per block: schedulers fill their events, in slot
        // order, and audio players their block, which every node on the lane copies.
        let runs = self.runs;
        for l in &mut self.lanes.slots {
            match l {
                LaneSlot::Notes(Some(s)) => s.run(&runs, &mut self.next_note),
                LaneSlot::Audio(Some(p)) => p.fill(&runs),
                _ => {}
            }
        }
        if self.trace.is_some() {
            self.record(clock);
        }
        let b = self.block(clock);
        if let Err(e) = self.plan.graph.process(&b, Seg::WHOLE, None, None, &self.lanes.slots) {
            self.broken = !self.strict;
            return Err(e);
        }
        let o = &self.plan.graph.bufs[self.plan.out.node][self.plan.out.port];
        let stereo = o.len() >= 2 * BLOCK;
        for f in 0..BLOCK {
            let mut frame = [o[f], if stereo { o[BLOCK + f] } else { o[f] }];
            for x in &mut frame {
                if !x.is_finite() {
                    if self.strict {
                        bail!("the song's output became {x} at {:.3}s", b.time(f));
                    }
                    *x = 0.0;
                    self.non_finite += 1;
                }
            }
            self.frames[f] = frame;
        }
        Ok(&self.frames)
    }

    /// Put this block's jumps and notes in the trace.
    fn record(&mut self, clock: u64) {
        let Some(t) = &mut self.trace else { return };
        let runs = self.runs;
        for r in runs.as_slice().iter().filter(|r| r.jump != Jump::None) {
            t.push(TraceEntry { engine: clock + r.off as u64, transport: r.at, slot: u32::MAX, id: 0, kind: TraceKind::Jump(r.jump) });
        }
        let mut dropped = 0;
        for (slot, l) in self.lanes.slots.iter().enumerate() {
            let LaneSlot::Notes(Some(s)) = l else { continue };
            dropped += s.events().dropped_ons();
            for e in s.events().as_slice() {
                let (id, kind) = match *e {
                    Event::On { id, pitch, .. } => (id, TraceKind::On { pitch }),
                    Event::Off { id, .. } => (id, TraceKind::Off),
                    _ => continue,
                };
                let f = e.frame();
                t.push(TraceEntry { engine: clock + f as u64, transport: runs.frame(f), slot: slot as u32, id, kind });
            }
        }
        if dropped != self.dropped {
            self.dropped = dropped;
            t.push(TraceEntry { engine: clock, transport: runs.first().at, slot: u32::MAX, id: 0, kind: TraceKind::Dropped(dropped) });
        }
    }

    /// Render the next `out.len()` frames, stereo.
    pub fn process(&mut self, out: &mut [[f32; 2]]) -> Result<()> {
        for frame in out.iter_mut() {
            if self.pending.is_empty() {
                self.render_block(self.clock)?;
                self.pending.load(&self.frames, &self.runs, self.transport.pos());
            }
            *frame = self.pending.pop().0;
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

    /// Back to the state right after building, transport aside.
    fn reset(&mut self) -> Result<()> {
        self.plan.graph.reset()?;
        for l in &mut self.lanes.slots {
            match l {
                LaneSlot::Notes(Some(s)) => s.reset(),
                LaneSlot::Audio(Some(p)) => p.reset(),
                _ => {}
            }
        }
        Ok(())
    }

    /// Continue from sample `to`, exactly as if everything before it had been rendered.
    /// Implemented by resetting and replaying from 0 (RENDER-1): offline only.
    pub fn seek(&mut self, to: u64) -> Result<()> {
        self.reset()?;
        self.transport = Transport::new(0, true);
        self.clock = 0;
        self.pending.clear();
        self.delivered = 0;
        let mut scratch = vec![[0.0; 2]; 4096];
        while self.delivered < to {
            let n = (to - self.delivered).min(scratch.len() as u64) as usize;
            self.process(&mut scratch[..n])?;
        }
        Ok(())
    }

    /// Frames handed out so far by [`Self::process`] (the offline playback position).
    pub fn position(&self) -> u64 {
        self.delivered
    }

    /// The rate it renders at: the project's (RENDER-31).
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate as u32
    }

    /// The engine clock of the next block.
    pub fn clock(&self) -> u64 {
        self.clock
    }

    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    pub fn set_transport(&mut self, t: Transport) {
        self.transport = t;
    }

    /// Apply a transport command at the next block line (RENDER-9). Never allocates.
    pub fn transport_cmd(&mut self, cmd: Cmd) {
        self.transport.apply(cmd, &self.tempo, self.sample_rate);
    }

    pub fn tempo(&self) -> &TempoMap {
        &self.tempo
    }

    /// Take over from `old`, which may play in its own tempo map and at its own rate: the same
    /// beat, play state and loop (RENDER-25a, no jump back), a first block that seeks without
    /// chasing, and the next note id, so no id repeats into an instance that goes on (RENDER-26a).
    pub fn enter_from(&mut self, old: &Renderer) {
        let mut t = old.transport;
        t.retime(&old.tempo, old.sample_rate, &self.tempo, self.sample_rate);
        t.enter();
        self.transport = t;
        self.next_note = old.next_note;
    }

    /// The lanes it plays, by slot.
    pub fn lane_slots(&self) -> &[LaneRef] {
        &self.lanes.refs[..self.lanes.live]
    }

    /// Replace one lane's data at the next block line, touching no kernel (RENDER-26). Returns
    /// the data replaced, to retire, or `data` itself when it doesn't fit the slot (another
    /// kind, more notes than its scheduler holds, or audio renders, which an activation brings).
    /// Never allocates.
    pub fn swap_lane(&mut self, slot: usize, data: LaneSwap) -> Result<LaneSwap, LaneSwap> {
        let swapped = match (self.lanes.slots[..self.lanes.live].get_mut(slot), data) {
            (Some(LaneSlot::Notes(Some(s))), LaneSwap::Notes(d)) => s.swap(d).map(LaneSwap::Notes).map_err(LaneSwap::Notes),
            (Some(LaneSlot::Control(Some(c))), LaneSwap::Control(d)) => Ok(LaneSwap::Control(std::mem::replace(c, d))),
            (_, d) => Err(d),
        };
        if swapped.is_ok()
            && let Some(t) = &mut self.trace
        {
            t.push(TraceEntry { engine: self.clock, transport: self.transport.pos(), slot: slot as u32, id: 0, kind: TraceKind::Swap });
        }
        swapped
    }

    /// Its cells' slots: where values for its cells go while it plays, from any thread
    /// (RENDER-28). They live as long as the renderer, and longer while anyone holds them.
    pub fn cell_slots(&self) -> Arc<CellSlots> {
        self.plan.cells.slots().clone()
    }

    /// Which of its cells hold each value written in the code.
    pub fn cell_map(&self) -> Arc<CellMap> {
        self.plan.cells.map().clone()
    }

    /// What cell `id` holds now.
    pub fn cell_value(&self, id: CellId) -> Option<f32> {
        self.plan.cells.value(id)
    }

    /// Set `cells` to `value` at once, a step, off the audio thread: a renderer not yet sent, so
    /// that it plays a gesture's value from its first block. `value` is in the constant's own units
    /// (the source number times its scale).
    pub fn set_cells(&mut self, cells: &[CellId], value: f64) {
        self.plan.cells.set_now(&mut self.plan.graph, cells, value as f32);
    }

    /// Every cell write up to `seq` is in this renderer already (it was built with the values, or
    /// they were set before it was sent): what it reports as heard starts there.
    pub fn hear_from(&mut self, seq: u64) {
        self.plan.cells.hear_from(seq);
    }

    /// `(from, to)` when its blocks since the last call applied every cell write up to `to`: the
    /// engine's stamps of when writes were heard. Never allocates.
    pub fn take_heard(&mut self) -> Option<(u64, u64)> {
        self.plan.cells.take_heard()
    }

    /// Voices currently sounding, across every `poly`.
    pub fn active_voices(&self) -> usize {
        self.voices().filter(|(_, v)| v.note.is_some()).count()
    }

    /// Notes still held: voices whose gate is open, and notes sounding into plugins. After a
    /// stop or a release this is 0, or a note is stuck (RENDER-26a).
    pub fn held_notes(&self) -> usize {
        let clock = self.clock;
        let voices = self.voices().filter(|(_, v)| v.note.is_some_and(|n| n.off >= clock)).count();
        let plugins = self.top().iter().map(|e| match e {
            Exec::Plugin(p) => p.keys.len() + p.release.len(),
            _ => 0,
        });
        voices + plugins.sum::<usize>()
    }

    /// Notes the schedulers hold: turned on and not yet off.
    pub fn sounding_notes(&self) -> usize {
        self.lanes.slots.iter().map(|l| if let LaneSlot::Notes(Some(s)) = l { s.held() } else { 0 }).sum()
    }

    /// Whether every voice with an open gate plays a note its scheduler holds: a gated voice
    /// nobody will turn off is a stuck note.
    pub fn gates_are_owned(&self) -> bool {
        let clock = self.clock;
        self.voices().filter_map(|(slot, v)| Some((slot, v.note.filter(|n| n.off >= clock)?))).all(|(slot, n)| {
            match &self.lanes.slots[slot] {
                LaneSlot::Notes(Some(s)) => s.is_sounding(n.id),
                _ => false,
            }
        })
    }

    /// Its top-level nodes, without the ghosts and fades of an activation's window.
    fn top(&self) -> &[Exec] {
        &self.plan.graph.exec[..self.plan.nodes]
    }

    /// Every top-level `poly`'s voices, with the lane slot each plays. Never allocates.
    fn voices(&self) -> impl Iterator<Item = (usize, &VoiceSlot)> {
        let polys = self.top().iter().filter_map(|e| if let Exec::Poly(p) = e { Some(p) } else { None });
        polys.flat_map(|p| p.pool.iter().flat_map(|pool| &pool.voices).map(move |v| (p.slot, v)))
    }

    /// Panic (RENDER-10): every note off, every instance reset. The next block seeks without
    /// chasing.
    pub fn panic(&mut self) -> Result<()> {
        self.reset()?;
        self.transport.enter();
        Ok(())
    }

    /// Whether the engine may stop processing this renderer while stopped (RENDER-6a): no fade
    /// window runs, no voice sounds, no kernel at the top reports `active`, and every plugin at the
    /// top said it sleeps, or that it may stop once quiet and its output is (a plugin's tail is its
    /// own to report).
    pub fn can_idle(&mut self) -> bool {
        !self.in_window() && self.active_voices() == 0 && self.plugins_sleep() && !self.plan.graph.active().unwrap_or(true)
    }

    fn plugins_sleep(&self) -> bool {
        use m2_plugins::Status as S;
        let graph = &self.plan.graph;
        graph.exec.iter().enumerate().all(|(i, e)| match e {
            Exec::Plugin(p) => match p.status {
                S::Sleep => true,
                S::ContinueIfNotQuiet => graph.bufs[i].first().is_none_or(|b| b.iter().all(|x| x.abs() < QUIET)),
                _ => false,
            },
            _ => true,
        })
    }

    /// While the engine idles: whether a cell was written, or a plugin that slept asked to be
    /// processed (CLAP's `request_process`), either of which wakes it within one block, as a
    /// message does. Atomic loads and swaps only.
    pub fn woken(&self) -> bool {
        self.plan.cells.slots().pending()
            || self.plan.graph.exec.iter().any(|e| match e {
                Exec::Plugin(p) if p.status == m2_plugins::Status::Sleep => {
                    p.inst.as_ref().is_some_and(|i| !i.dead() && i.host().take_process_request(i.handle()))
                }
                _ => false,
            })
    }

    pub fn hosts_plugins(&self) -> bool {
        self.plan.plugins
    }

    /// The instances of its top-level plugin nodes, in node order (gates, tests; off the audio
    /// thread).
    pub fn plugin_handles(&self) -> Vec<m2_plugins::Handle> {
        self.top().iter().filter_map(|e| if let Exec::Plugin(p) = e { Some(p.inst.as_ref()?.handle()) } else { None }).collect()
    }

    /// Its top-level plugin nodes, in node order: the notes each holds, what it was sent and what
    /// it said (gates, tests; off the audio thread).
    pub fn plugins(&self) -> Vec<PluginView> {
        self.top()
            .iter()
            .filter_map(|e| match e {
                Exec::Plugin(p) => {
                    Some(PluginView { handle: p.inst.as_ref()?.handle(), notes: p.keys.clone(), offs: p.offs, last_off: p.last_off, status: p.status })
                }
                _ => None,
            })
            .collect()
    }

    /// Events plugins output in their last calls, and those that didn't fit (HOST-4: collected,
    /// not yet consumed).
    pub fn plugin_out_events(&self) -> (usize, u32) {
        self.plan.graph.exec.iter().fold((0, 0), |(n, d), e| match e {
            Exec::Plugin(p) => (n + p.out.len(), d + p.out.dropped()),
            _ => (n, d),
        })
    }

    /// The memory every kernel instance holds: unchanged by playing, or a reset grew it on the
    /// audio thread. Off the audio thread only.
    pub fn kernel_memory_bytes(&self) -> usize {
        self.plan.graph.kernel_memory()
    }

    /// Unique to this renderer.
    pub fn generation(&self) -> u64 {
        self.plan.id
    }

    /// Keep the last `cap` scheduler events, jumps and swaps.
    pub fn trace(&mut self, cap: usize) {
        self.trace = Some(Trace::new(cap));
    }

    /// The trace, oldest first (empty unless [`Self::trace`] was called).
    pub fn trace_entries(&self) -> Vec<TraceEntry> {
        self.trace.as_ref().map_or(vec![], Trace::entries)
    }

    /// Ons dropped by full event buffers (never offline, nor live in step 3a).
    pub fn dropped_ons(&self) -> u32 {
        self.lanes.slots.iter().map(|l| if let LaneSlot::Notes(Some(s)) = l { s.events().dropped_ons() } else { 0 }).sum()
    }

    /// Audio lane fades let go before they ended for want of room: never at a project rate.
    pub fn dropped_taps(&self) -> u32 {
        self.lanes.slots.iter().map(|l| if let LaneSlot::Audio(Some(p)) = l { p.dropped_taps() } else { 0 }).sum()
    }

    /// Times every kernel was reset since it was built: a panic, and offline each [`Self::seek`].
    /// A jump, a swap or an activation never moves it (RENDER-10); an activation resets only the
    /// boxes that went non-finite or trapped, counted in its [`Stats`].
    pub fn resets(&self) -> u64 {
        self.plan.graph.resets
    }

    /// Samples silenced in live mode because they were NaN or infinite.
    pub fn non_finite(&self) -> u64 {
        self.non_finite
    }

    /// The last block's runs.
    pub fn last_runs(&self) -> &Runs {
        &self.runs
    }

    /// Whether a kernel crashed live, so it renders silence.
    pub fn broken(&self) -> bool {
        self.broken
    }
}

#[cfg(test)]
mod tests;
