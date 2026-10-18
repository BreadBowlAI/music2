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
pub mod click;
pub mod data;
mod delay;
pub mod engine;
pub mod event;
mod fade;
pub mod fault;
pub mod fpenv;
#[doc(hidden)]
pub mod gauge;
mod keep;
pub mod latency;
mod offline;
mod overlay;
pub mod packages;
mod parts;
mod plan;
mod renders;
pub mod plugins;
mod poly;
mod sandbox;
pub mod sched;
pub mod stream;
mod taps;
pub mod threads;
pub mod trace;
pub mod transport;
mod view;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use m2_ir::{Alloc, FNode, FOp, Flat, Input, Kind, Mode, NoteField, PortRef, TimeOf};
use m2_song::{Arrangement, DataSet, LaneData, TempoMap};

pub use cells::{CellAt, CellId, CellInfo, CellMap, CellSlots, CellTarget};
pub use data::{decls, lane_kind};
pub use audio::LiveInput;
pub use engine::{Applied, CallbackStamp, Engine, Envelope, Heard, Msg, Retired};
pub use event::{Event, Events};
pub use m2_pool::{Counters, ENTRY_BUCKETS, Meter, Pool, Runner, Setup, Trip};
pub use packages::{Package, PackageSet};
pub use plan::{Basis, Continues, Count, Keep, LaneInfo, Leftovers, PlanInfo, PluginAt, Shape, Stats};
pub use plugins::PluginInstance;
pub use sched::{Caps, LaneSwap, LiveEv, NotesData};
pub use sandbox::{OutOfMemory, TooManySounds};
pub use taps::{Peaks, TapSlots};
pub use trace::{TraceEntry, TraceKind};
pub use transport::{Cmd, Jump, Loop, MAX_RUNS, Run, Runs, Transport};

use crate::cells::{Cells, Write};
use crate::fault::{Code, Reporter};
use crate::fade::xfade;
use crate::keep::{Edge, Wiring, used};
use crate::parts::Schedule;
use crate::plan::{Entry, LanePlan, LaneSlot, Lanes, Plan};
use crate::sandbox::{Sandbox, State};
use crate::taps::Taps;
use crate::trace::Trace;
use crate::view::{GraphView, Local, Own, Reads};

pub const BLOCK: usize = 128;

/// Bytes of one kernel port buffer: `PORT_CHANNELS` (2) channels of [`BLOCK`] floats.
pub(crate) const PORT_BYTES: usize = 2 * BLOCK * 4;

/// A node's version when its output must be copied into the kernels it feeds at every call: it
/// isn't a constant, or it is a cell that ramps this block.
const COPY: u64 = u64::MAX;

/// Memory one sandbox may take while it is born (its birth cap, RENDER-32); once sealed it takes no
/// more. std's largest state, a stereo delay at 384 kHz, takes 6.1 MB, so a voice pool of 12 such
/// voices fits.
pub(crate) const KERNEL_MEMORY: usize = 256 << 20;

/// The birth cap in MB, as the app tells it.
pub const SOUND_MEMORY_MB: usize = KERNEL_MEMORY >> 20;

/// The most sandboxes one renderer holds (RENDER-32): one for each kernel outside an instrument,
/// and one a package for each instrument's voices. Two renderers, the one that plays and the one
/// an edit prepares, stay within 25% of the 128 TiB a process may address when each memory takes
/// 4.097 GiB of it, as measured on the Mac (4.0625 on Linux): 16,384 GiB / 4.097 GiB = 3,999. The
/// same number on every OS, so a song either fits or doesn't, everywhere.
pub const SANDBOX_LIMIT: usize = 3_999;

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
    /// Played by the live [`Engine`]: a node that fails stops alone (RENDER-25).
    live: bool,
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

/// One kernel state (RENDER-32). Outside an instrument it is a box with a sandbox of its own, which
/// a plan activation can move into the next plan, and whose memory goes when it drops. In a voice,
/// its state is in its voice pool's sandbox for its package, [`VoicePool::sandboxes`]`[at]`.
struct KernelInst {
    own: Option<Sandbox>,
    at: usize,
    state: State,
    /// The hash of the shape it was made for: what an activation checks before moving it.
    shape: u64,
    /// Live, it failed (RENDER-25): it plays silence and isn't called again until it is reset, as
    /// an activation moves it on or a panic.
    stopped: bool,
    name: String,
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

/// One call's view of a kernel's memory: its bytes, the ports in them, and the instance's name.
struct Ports<'a> {
    data: &'a mut [u8],
    ins: &'a [usize],
    outs: &'a [usize],
    out_channels: &'a [u8],
    name: &'a str,
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

/// The channel count of each port of kernel `sig` in node `n`, inputs first, as its port table has
/// them: a connected input's own, else its kind's (`audio<*>` and `*` take the node's `channels`).
fn port_channels(sig: &m2_ir::KernelSig, n: &FNode, channels: u8, inputs: &[Input]) -> Vec<u32> {
    let input = |(p, i): (&m2_ir::PortSig, &Input)| match (i, &p.kind) {
        (Input::Port { channels, .. }, _) => *channels,
        (_, m2_ir::PortKind::Audio(Some(c))) => *c,
        (_, m2_ir::PortKind::Audio(None) | m2_ir::PortKind::Any) => channels,
        _ => 1,
    };
    sig.inputs.iter().zip(inputs).map(input).chain(n.outputs.iter().map(|k| k.channels())).map(|c| c.clamp(1, 2) as u32).collect()
}

impl KernelInst {
    /// Kernel `k` for node `n` outside an instrument, in a sandbox of its own: born, and sealed.
    fn new(packages: &PackageSet, n: &FNode, k: &Kernel, shape: u64) -> Result<KernelInst> {
        let id = k.id;
        let (module, prefix, _) = packages.module_for(id).with_context(|| format!("{}: kernel `{id}` is not loaded", n.name))?;
        let mut sandbox = Sandbox::new(&packages.engine, module).with_context(|| format!("{}: `{id}`", n.name))?;
        let mut k = KernelInst::born(&mut sandbox, 0, prefix, packages, n, k, shape)?;
        sandbox.seal().with_context(|| n.name.clone())?;
        k.own = Some(sandbox);
        Ok(k)
    }

    /// Kernel `k` for node `n`, its state born in `sandbox`, a voice pool's `at`th, its constant
    /// inputs written into its memory once.
    fn born(sandbox: &mut Sandbox, at: usize, prefix: &str, packages: &PackageSet, n: &FNode, k: &Kernel, shape: u64) -> Result<KernelInst> {
        let (id, inputs) = (k.id, k.inputs);
        let sig = m2_ir::Kernels::kernel(packages, id).with_context(|| format!("{}: kernel `{id}` has no signature", n.name))?;
        let (n_in, n_out) = (inputs.len(), n.outputs.len());
        let ports = port_channels(sig, n, k.channels, inputs);
        let spec = sandbox::Spec {
            inputs: n_in,
            outputs: n_out,
            channels: &ports,
            audio: k.channels,
            sample_rate: k.sample_rate,
            values: k.values,
            assets: k.assets,
            offline: None,
        };
        let state = match sandbox.state(prefix, &spec) {
            Ok(s) => s,
            Err(e) => {
                let e = sandbox.ran_out(&n.name, e);
                return Err(if e.is::<OutOfMemory>() { e } else { e.context(format!("{}: `{id}`", n.name)) });
            }
        };
        // Unconnected and constant inputs are written once; kernels never write inputs.
        let data = sandbox.data_born();
        for (inp, &ptr) in inputs.iter().zip(&state.ins) {
            if let Input::Const(v) = inp {
                floats(data, ptr, 2 * BLOCK).fill(*v as f32);
            }
        }
        #[cfg(debug_assertions)]
        let names: Vec<&str> = sig.inputs.iter().map(|p| p.name.as_str()).collect();
        LIVE_KERNELS.fetch_add(1, Ordering::Relaxed);
        Ok(KernelInst {
            own: None,
            at,
            state,
            shape,
            stopped: false,
            name: n.name.clone(),
            out_channels: n.outputs.iter().map(|k| k.channels()).collect(),
            #[cfg(debug_assertions)]
            before: vec![0; n_in * PORT_BYTES],
            #[cfg(debug_assertions)]
            kernel: id.to_string(),
            #[cfg(debug_assertions)]
            in_names: (0..n_in).map(|i| names.get(i).copied().unwrap_or("?").to_string()).collect(),
        })
    }

    /// Its sandbox, and its state: its own, or the pool's in `pool` (a voice's). For debug builds'
    /// input checks.
    #[cfg(debug_assertions)]
    fn sandbox<'a>(&'a mut self, pool: &'a mut [Sandbox]) -> (&'a mut Sandbox, &'a State) {
        match &mut self.own {
            Some(own) => (own, &self.state),
            None => (&mut pool[self.at], &self.state),
        }
    }

    /// `k_process` for `frames` frames. Debug builds check it left its inputs alone (ABI-1).
    fn process(&mut self, pool: &mut [Sandbox], frames: usize) -> Result<()> {
        #[cfg(debug_assertions)]
        self.keep_inputs(pool);
        let name = &self.name;
        let (sb, state) = match &mut self.own {
            Some(own) => (own, &self.state),
            None => (&mut pool[self.at], &self.state),
        };
        let done = state.process(sb, frames).wctx(|| format!("{name} crashed"));
        done.map_err(|e| sb.ran_out(name, e))?;
        #[cfg(debug_assertions)]
        self.check_inputs(pool, false)?;
        Ok(())
    }

    /// `k_reset`. Debug builds check it left its inputs alone (ABI-1).
    fn reset(&mut self, pool: &mut [Sandbox]) -> Result<()> {
        #[cfg(debug_assertions)]
        self.keep_inputs(pool);
        let name = &self.name;
        let (sb, state) = match &mut self.own {
            Some(own) => (own, &self.state),
            None => (&mut pool[self.at], &self.state),
        };
        state.reset(sb).map_err(|e| sb.ran_out(name, e))?;
        #[cfg(debug_assertions)]
        self.check_inputs(pool, true)?;
        Ok(())
    }

    /// Whether it still holds sound that will come out later.
    fn active(&mut self, pool: &mut [Sandbox]) -> Result<bool> {
        let name = &self.name;
        let (sb, state) = match &mut self.own {
            Some(own) => (own, &self.state),
            None => (&mut pool[self.at], &self.state),
        };
        state.active(sb).map_err(|e| sb.ran_out(name, e))
    }

    /// Its first reset, while it is built, after its sandbox is sealed: a reset that would take
    /// memory fails here, at the build, naming its node, and never later on the audio thread.
    fn prewarm(&mut self, pool: &mut [Sandbox]) -> Result<()> {
        self.reset(pool)
    }

    /// Its memory's bytes for one call's copies, with its ports and its name, without asking the
    /// store (RENDER-32).
    fn ports<'a>(&'a mut self, pool: &'a mut [Sandbox]) -> Ports<'a> {
        let (out_channels, name) = (&self.out_channels, &self.name);
        let (sb, s) = match &mut self.own {
            Some(own) => (own, &self.state),
            None => (&mut pool[self.at], &self.state),
        };
        Ports { data: sb.data(), ins: &s.ins, outs: &s.outs, out_channels, name }
    }

    #[cfg(debug_assertions)]
    fn keep_inputs(&mut self, pool: &mut [Sandbox]) {
        let mut before = std::mem::take(&mut self.before);
        let (sb, state) = self.sandbox(pool);
        let data = sb.bytes();
        for (k, &ptr) in state.ins.iter().enumerate() {
            before[k * PORT_BYTES..(k + 1) * PORT_BYTES].copy_from_slice(&data[ptr..ptr + PORT_BYTES]);
        }
        self.before = before;
    }

    #[cfg(debug_assertions)]
    fn check_inputs(&mut self, pool: &mut [Sandbox], reset: bool) -> Result<()> {
        let before = std::mem::take(&mut self.before);
        let (sb, state) = self.sandbox(pool);
        let data = sb.bytes();
        let changed = state.ins.iter().enumerate().position(|(k, &ptr)| data[ptr..ptr + PORT_BYTES] != before[k * PORT_BYTES..(k + 1) * PORT_BYTES]);
        self.before = before;
        if let Some(k) = changed {
            let (kernel, port) = (&self.kernel, &self.in_names[k]);
            let what = if reset {
                format!("`{kernel}`'s reset changed its input `{port}`")
            } else {
                format!("`{kernel}` wrote its input `{port}`")
            };
            bail!("{}: {what} (ABI-1: kernel input memory is read-only)", self.name);
        }
        Ok(())
    }
}

impl KernelNode {
    /// Frames `seg` of block `b`: its inputs copied into its memory as they change, the call, and
    /// its outputs copied out. Never allocates.
    fn process(&mut self, b: &Block, seg: Seg, r: &impl Reads, out: &mut [Buf], pool: &mut [Sandbox]) -> Result<()> {
        let n = seg.n;
        let Some(k) = self.inst.as_deref_mut().filter(|k| !k.stopped) else {
            for o in out.iter_mut() {
                for c in 0..o.len() / BLOCK {
                    o[seg.range(c)].fill(0.0);
                }
            }
            return Ok(());
        };
        let p = k.ports(pool);
        for ((inp, &ptr), seen) in self.inputs.iter().zip(p.ins).zip(&mut self.seen) {
            if let Input::Port { src, src_channels, channels } = inp {
                let s = r.buf(src.node, src.port);
                // A steady constant is written over the whole block, once per value, so every
                // later segment finds it in place; anything else is copied for this segment.
                let version = r.version(src.node);
                if version != COPY && *seen == version {
                    continue;
                }
                let (off, len) = if version == COPY { (seg.off, n) } else { (0, BLOCK) };
                for c in 0..*channels as usize {
                    let sc = if c < *src_channels as usize { c } else { 0 };
                    floats(p.data, ptr + c * BLOCK * 4, len).copy_from_slice(&s[sc * BLOCK + off..sc * BLOCK + off + len]);
                }
                *seen = version;
            }
        }
        k.process(pool, n)?;
        let p = k.ports(pool);
        for (o, (&ptr, &ch)) in out.iter_mut().zip(p.outs.iter().zip(p.out_channels)) {
            for c in 0..ch as usize {
                let dst = &mut o[seg.range(c)];
                dst.copy_from_slice(floats(p.data, ptr + c * BLOCK * 4, n));
                if b.strict
                    && let Some(at) = dst.iter().position(|x| !x.is_finite())
                {
                    bail!("{} produced {} at {:.3}s", p.name, dst[at], b.time(seg.off + at));
                }
            }
        }
        Ok(())
    }
}

/// The note a voice plays. Its gate is open from engine sample `on` until `off`, the sample of its
/// Off, `u64::MAX` until the Off arrives (RENDER-13). `on` is its On's sample, or the one after
/// when the voice carried on from a note whose gate was open (`retrigger: false`).
#[derive(Clone, Copy, Debug)]
struct VoiceNote {
    id: u32,
    pitch: f32,
    velocity: f32,
    on: u64,
    off: u64,
}

/// What a voice's note fields read: its note, or in a one-note poly (`"mono"`, `"legato"`) the
/// pool's pitch, gate and velocity at each frame of the block; and its lane's wheels.
#[derive(Clone, Copy)]
enum Voiced<'a> {
    Note(VoiceNote, poly::Wheels),
    Cv(&'a poly::Cv, poly::Wheels),
}

struct VoiceSlot {
    graph: Graph,
    note: Option<VoiceNote>,
    /// When the voice last started a note, for stealing the oldest.
    born: u64,
    /// Whether the graph has run since its last reset.
    dirty: bool,
    /// The voice's peak over the last block it ran (since its note started, when that was in the
    /// block): for a note whose Off lands on the next block line, which is freed there, exactly
    /// where it would have been at the end of the block, and for `steal: "quietest"`.
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
    /// How its voices are given notes (part of its shape).
    alloc: Alloc,
    /// `"mono"` and `"legato"`: the one note at a time its one voice plays.
    mono: Option<Box<poly::Mono>>,
    /// Its lane's pitch wheel and mod wheel, as the lane last sent them: reset with the pool.
    wheels: poly::Wheels,
    /// Its voices' kernel states, one sandbox for each package they use (RENDER-32): born with the
    /// pool, sealed, and dropped with it.
    sandboxes: Vec<Sandbox>,
    born: u64,
    shape: u64,
    /// Live, a voice failed: the pool plays silence and calls no voice, as a stopped kernel
    /// ([`KernelInst::stopped`]).
    stopped: bool,
}

struct PolyNode {
    /// The notes lane it plays.
    slot: usize,
    /// The note delay node it reads that lane's notes through, when it is late (RENDER-18).
    late: Option<usize>,
    /// The voice graph's output.
    out: PortRef,
    /// What its voices' `Capture` nodes read, by index: outputs of the graph around it. Kept out of
    /// the voices, so a pool moved into another plan reads that plan's nodes.
    captures: Vec<PortRef>,
    /// This block's Ons, by voice: room for two runs of the lane's Ons.
    plan: Vec<Planned>,
    /// What a slide's time is read from (RENDER-16): an output of the graph around it.
    glide: Option<PortRef>,
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
    /// The note delay node it reads that lane's notes through, when it is late (RENDER-18).
    late: Option<usize>,
    /// How late its inputs arrive (RENDER-18): it is told the transport this many frames earlier
    /// (HOST-6), as `told` holds it for this block.
    offset: u32,
    told: delay::Told,
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
    /// Beside each parameter: whether a lane or a cable drives it (a gesture latches it, MON-6),
    /// rather than a value in code (a gesture holds it for a Tx, HOME-5).
    driven: Vec<bool>,
    /// Beside each parameter: the value its window holds or latched it at, while it does (live
    /// only): what is sent in its place.
    over: Vec<Option<f64>>,
    /// Calls left in which a change outside a gesture is set back without a word: a plugin may
    /// say what a state it loaded set (HOME-5a's notice is for the person's own changes).
    quiet: u8,
    /// One call's events in and out, sized when built (RENDER-28). What comes out (parameter
    /// gestures and values, HOST-4) is read after each live call: a gesture holds or latches a
    /// code-owned parameter, a change outside one is set back, and one to a parameter code
    /// doesn't set marks the state changed.
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
    /// Its instance is back from an export, which ended its notes: at its next call, each note it
    /// holds that its lane still holds starts again, and the others are forgotten (RENDER-1).
    again: bool,
    /// Where it reports that its plugin failed (`plugin.failed`, RENDER-25): its plan's reporter
    /// and its node. A top-level node's, once its plan has its id.
    reports: Option<(Reporter, usize)>,
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
/// at its first frame for each of `ended`, and a NoteOff for each of `release`; with `again` (the
/// lane's scheduler, once an export gave the instance back), a NoteOn there for each of `keys` the
/// scheduler still holds, and the others forgotten; then the lane's note events (already in order)
/// from `*next` on, merged with `params` sampled every [`PARAM_STEP`] frames; one with a value in
/// `over` (its window holds or latched it) gets that value, only when a send is due (`None`
/// last sent), never its input's. At one frame
/// parameters come after notes, the order a stable sort by time gave before. An On becomes a
/// NoteOn (and a Tuning between keys), an Off a NoteOff for the key `keys` holds for it. Returns
/// how many NoteOffs it sent, and the time of the last. Never allocates: `out` and `keys` keep
/// their room (RENDER-28).
#[allow(clippy::too_many_arguments)]
fn plugin_events(
    ended: &[(u32, i16)],
    release: &[(u32, i16)],
    again: Option<&sched::NoteScheduler>,
    notes: &[Event],
    next: &mut usize,
    params: &mut [(u32, Input, Option<f32>)],
    over: &[Option<f64>],
    r: &impl Reads,
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
    // Back from an export, which ended them (RENDER-1): the notes the song still holds start
    // again, with their own ids, so their Offs end them. One the lane let go at this block line,
    // or lets go within it, isn't started: the plugin no longer holds it, so no Off is sent for it.
    if let Some(s) = again {
        keys.retain(|&(id, key)| {
            let Some((pitch, velocity)) = s.sounding_note(id) else { return false };
            out.push(P::NoteOn { time: 0, id: id as i32, key, velocity: velocity as f64 });
            let tune = pitch as f64 - key as f64;
            if tune.abs() > 1e-6 {
                out.push(P::Tuning { time: 0, id: id as i32, key, semitones: tune });
            }
            true
        });
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
        // The lane's wheels, as CLAP MIDI (D8 of track pm).
        Event::Midi { frame, bytes } => {
            out.push(P::Midi { time: frame - lo as u32, bytes });
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
        for (i, (id, input, last)) in params.iter_mut().enumerate() {
            if let Some(&Some(v)) = over.get(i) {
                if last.is_none() {
                    out.push(P::Param { time: (f - lo) as u32, id: *id, value: v });
                    *last = Some(v as f32);
                }
                continue;
            }
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
                    let s = r.buf(src.node, src.port)[f];
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
    fn process(&mut self, b: &Block, r: &impl Reads, out: &mut Buf, lanes: &[LaneSlot]) -> Result<()> {
        let Some(inst) = &self.inst else {
            out.fill(0.0);
            return Ok(());
        };
        // One that failed live plays silence and is never called again: the session replaces it.
        // Offline its render fails (RENDER-1): an export lent it, and it failed or its host ended.
        if inst.dead() {
            if !b.live {
                let code = if inst.host().gone(inst.handle()) { Code::PluginCrashed } else { Code::PluginFailed };
                return Err(fault::PluginStopped { code, name: self.name.clone() }.into());
            }
            out.fill(0.0);
            self.status = m2_plugins::Status::Sleep;
            return Ok(());
        }
        let notes = match self.late {
            Some(d) => r.events(d),
            None => self.notes.map_or(&[][..], |s| lanes[s].events()),
        };
        let mut next = 0;
        let channels = out.len() / BLOCK;
        let (host, h) = (inst.host(), inst.handle());
        // Its state was loaded: every parameter code owns goes again (HOME-4), at its next two
        // calls, as a plugin may apply the state at the first, over what that call brings.
        if inst.take_reloaded() {
            for p in &mut self.params {
                p.2 = None;
            }
            self.quiet = 3;
        }
        let quiet = self.quiet > 0;
        self.quiet = self.quiet.saturating_sub(1);
        if std::mem::take(&mut self.reset_first) {
            host.reset(h);
            // A plugin may leave the thread flushing subnormals (RENDER-2a).
            fpenv::canon();
            // With the export's call before it, that ended the notes playback left in it: its live
            // node starts the ones the song still holds again at the resume.
            inst.mark_lent();
        }
        let mut again = std::mem::take(&mut self.again).then(|| self.notes.and_then(|s| lanes[s].scheduler())).flatten();
        if b.live {
            taken_over(&mut self.params, &mut self.over, inst);
        }
        // One call per run: a call never spans a jump, and each gets its own transport (RENDER-9).
        // One late by its offset is told the transport that much earlier (HOST-6, RENDER-18).
        let told = (self.offset > 0).then_some(self.told);
        let runs = match &told {
            Some(t) => &t.runs[..t.len],
            None => b.runs.as_slice(),
        };
        for (ri, run) in runs.iter().enumerate() {
            let (lo, hi) = (run.off as usize, (run.off + run.n) as usize);
            let (ended, release) = if ri == 0 { (&self.ended[..], &self.release[..]) } else { (&[][..], &[][..]) };
            let (offs, last) = plugin_events(
                ended,
                release,
                again.take(),
                notes,
                &mut next,
                &mut self.params,
                &self.over,
                r,
                (lo, hi),
                &mut self.keys,
                &mut self.events,
            );
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
                    *x = &r.buf(src.node, src.port)[c * BLOCK + lo..c * BLOCK + hi];
                }
            }
            let split = BLOCK.min(out.len());
            let (left, right) = out.split_at_mut(split);
            let outputs: [&mut [f32]; 2] = [&mut left[lo..hi], if channels > 1 { &mut right[lo..hi] } else { &mut [] }];
            let (info, beats, seconds) = match &told {
                Some(t) => (t.info[ri], t.beats[ri], t.seconds[ri]),
                None => (b.info[ri], b.beats64[lo], b.seconds[lo] as f64),
            };
            let transport = m2_plugins::Transport {
                beats,
                seconds,
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
            // A plugin may leave the thread flushing subnormals (RENDER-2a).
            fpenv::canon();
            if self.status == m2_plugins::Status::Error {
                // Offline the render fails, naming it (RENDER-1), strict or not: an export never
                // leaves a plugin out.
                if !b.live {
                    let code = if host.gone(h) { Code::PluginCrashed } else { Code::PluginFailed };
                    return Err(fault::PluginStopped { code, name: self.name.clone() }.into());
                }
                // Live, only this plugin goes silent, never the song: the session replaces it from
                // its committed state (HOST-5), and tells it. Atomic stores: nothing here allocates.
                // One whose host process ended is the session's to tell, as `plugin.crashed`
                // (HOST-3).
                inst.kill();
                if let Some((report, node)) = &self.reports
                    && !host.gone(h)
                {
                    report.node(Code::PluginFailed, *node);
                }
                out.fill(0.0);
                self.status = m2_plugins::Status::Sleep;
                return Ok(());
            }
            if ri == 0 {
                self.ended.clear();
                self.release.clear();
            }
            if b.live {
                heard(&self.out, &mut self.params, &mut self.over, &self.driven, inst, quiet);
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
            fpenv::canon();
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

/// What its window holds or latched, read before a live call (HOME-5a, MON-6). One no longer
/// taken over gets its input's value again.
fn taken_over(params: &mut [(u32, Input, Option<f32>)], over: &mut [Option<f64>], inst: &PluginInstance) {
    let o = inst.overrides();
    for (i, p) in params.iter_mut().enumerate() {
        let now = o.get(p.0).filter(|(f, _)| f & (plugins::HELD | plugins::LATCHED) != 0).map(|(_, v)| v);
        if over[i].is_some() && now.is_none() {
            p.2 = None;
        }
        over[i] = now;
    }
}

/// What a live call output (HOST-4): a gesture on a parameter code owns holds it (a value in
/// code, HOME-5) or latches it (a lane or a cable, MON-6); a change outside one is set back at
/// the next call (HOME-5a); a change to one code doesn't set changed its state (HOME-6).
/// Atomics only: nothing here allocates or locks.
fn heard(
    out: &m2_plugins::Events,
    params: &mut [(u32, Input, Option<f32>)],
    over: &mut [Option<f64>],
    driven: &[bool],
    inst: &PluginInstance,
    quiet: bool,
) {
    use m2_plugins::Event as P;
    let o = inst.overrides();
    for e in out.iter() {
        let (id, at) = match e {
            P::Gesture { id, .. } | P::Param { id, .. } => (id, params.iter().position(|p| p.0 == id)),
            _ => continue,
        };
        match (e, at) {
            (P::Gesture { begin: true, .. }, Some(i)) => {
                let kind = if driven[i] { plugins::LATCHED } else { plugins::HELD };
                let was = params[i].2.map_or(0.0, f64::from);
                if o.begin(id, kind, was) {
                    over[i] = Some(was);
                }
            }
            (P::Gesture { begin: false, .. }, Some(_)) => o.end(id),
            (P::Gesture { begin: false, .. }, None) => inst.state_changed(),
            (P::Param { value, .. }, Some(i)) => match o.get(id) {
                Some(_) => {
                    o.value(id, value);
                    over[i] = Some(value);
                    params[i].2 = Some(value as f32);
                }
                None => {
                    params[i].2 = None;
                    if !quiet {
                        inst.reverted(id);
                    }
                }
            },
            (P::Param { .. }, None) => inst.state_changed(),
            _ => {}
        }
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
    /// A monitoring overlay (MON-1): `src` times a gain that moves toward `target` by `step` a
    /// frame, from `cur` ([`overlay::process`]). Its cell sets `target` (RENDER-22a).
    Overlay {
        src: PortRef,
        target: f32,
        cur: f32,
        step: f32,
    },
    /// The planner's delay of an audio or control signal (RENDER-18).
    Delay(delay::DelayNode),
    /// The planner's delay of a notes lane (RENDER-18): its consumer reads its events.
    NoteDelay(Box<delay::NoteDelayNode>),
}

impl Exec {
    /// Whether its box stopped live (RENDER-25): only a kernel's or a `poly`'s can.
    fn stopped(&self) -> bool {
        match self {
            Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.stopped,
            Exec::Poly(p) => p.pool.as_ref().is_some_and(|p| p.stopped),
            _ => false,
        }
    }

    fn set_stopped(&mut self, stopped: bool) {
        match self {
            Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.stopped = stopped,
            Exec::Poly(p) => p.pool.iter_mut().for_each(|p| p.stopped = stopped),
            _ => {}
        }
    }

    /// Each node whose output this node reads during a block (8a): what the schedule orders a
    /// partition after. Lanes, time and constants read only what the pre-pass froze; a feedback
    /// reads its input after the graph, into next block's ring. A node may be named more than once.
    fn reads(&self, mut f: impl FnMut(usize)) {
        let mut port = |i: &Input| {
            if let Input::Port { src, .. } = i {
                f(src.node);
            }
        };
        match self {
            Exec::Kernel(k) => k.inputs.iter().for_each(&mut port),
            Exec::Plugin(p) => {
                p.audio_in.iter().for_each(&mut port);
                p.params.iter().for_each(|(_, i, _)| port(i));
                p.late.iter().for_each(|&d| f(d));
            }
            Exec::Missing(input) => input.iter().for_each(&mut port),
            Exec::Poly(p) => {
                p.captures.iter().chain(&p.glide).for_each(|c| f(c.node));
                p.late.iter().for_each(|&d| f(d));
            }
            Exec::Delay(d) => f(d.input.node),
            Exec::Fade { old, new, .. } => {
                port(old);
                port(new);
            }
            Exec::Overlay { src, .. } => f(src.node),
            Exec::Feedback { .. }
            | Exec::Control { .. }
            | Exec::Audio { .. }
            | Exec::Notes
            | Exec::Time(_)
            | Exec::Const(_)
            | Exec::NoteField(_)
            | Exec::NoteDelay(_)
            | Exec::Capture(_) => {}
        }
    }

    /// Whether this node does nothing in a block: a constant, written only by its cells before the
    /// graph, or a notes lane, whose scheduler runs before the graph. It runs in no partition, and
    /// reading it waits for nothing (8a, R1).
    fn idle(&self) -> bool {
        matches!(self, Exec::Const(_) | Exec::Notes)
    }
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

/// The sandboxes a renderer of `flat` holds, its top-level nodes `used` (RENDER-32): one for each
/// kernel outside an instrument, and one for each package an instrument's voices run kernels of.
fn sandboxes(flat: &Flat, used: &[bool], packages: &PackageSet) -> usize {
    let one = |n: &FNode| match &n.op {
        FOp::Kernel { .. } => 1,
        FOp::Poly { voice, .. } => {
            let mut of: Vec<&str> = voice.nodes.iter().filter_map(|n| match &n.op {
                FOp::Kernel { kernel, .. } => packages.module_for(kernel).map(|(_, _, package)| package),
                _ => None,
            }).collect();
            of.sort_unstable();
            of.dedup();
            of.len()
        }
        _ => 0,
    };
    flat.nodes.iter().zip(used).filter(|(_, u)| **u).map(|(n, _)| one(n)).sum()
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
    /// The top-level node it is building, if it is one.
    at: Option<usize>,
    /// By top-level node: how late its inputs arrive (RENDER-18).
    offsets: &'a [u32],
    /// Plugin instances made before the graph, to measure their latency, by node (RENDER-17).
    made: HashMap<usize, Arc<PluginInstance>>,
    /// While a voice pool is built: its sandboxes so far, by package (RENDER-32).
    pool: Vec<(String, Sandbox)>,
    /// The assets its kernels read, decoded and shared.
    assets: &'a audio::KernelAssets,
}

/// A kernel node's kernel and settings, as its state is made from them.
struct Kernel<'a> {
    id: &'a str,
    channels: u8,
    inputs: &'a [Input],
    values: &'a [f64],
    assets: &'a [Option<sandbox::Held>],
    sample_rate: f64,
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

    /// A new instance for plugin node `n`, of shape `shape`: created with its committed state,
    /// activated, its latency measured (RENDER-17), on the plugin main thread.
    fn create(&self, n: &FNode, found: &m2_plugins::Found, shape: u64) -> Result<Arc<PluginInstance>> {
        make_plugin(self.packages, self.data, self.owner, self.sample_rate as u32, self.mode, n, found, shape)
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
            self.at = (!in_voice).then_some(i);
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
            FOp::Kernel { kernel, channels, inputs, assets, values } => {
                let read: Vec<Option<sandbox::Held>> = assets.iter().map(|a| self.assets.get(a).map(sandbox::Held::Whole)).collect();
                let k = Kernel { id: kernel, channels: *channels, inputs, values, assets: &read, sample_rate: self.sample_rate };
                let inst = match boxed {
                    Make::New(shape) if in_voice => Some(Box::new(self.in_pool(n, &k, shape)?)),
                    Make::New(shape) => Some(Box::new(KernelInst::new(self.packages, n, &k, shape)?)),
                    _ => None,
                };
                Exec::Kernel(KernelNode { inputs: inputs.to_vec(), seen: vec![COPY; inputs.len()], inst })
            }
            FOp::Plugin { plugin, inputs, .. } if self.packages.plugin_missing(plugin) => {
                // Typed with `in` first (see `PackageSet::bind_plugins`).
                Exec::Missing(inputs.first().filter(|i| matches!(i, Input::Port { .. })).cloned())
            }
            FOp::Plugin { plugin, inputs, .. } => Exec::Plugin(Box::new(self.plugin(nodes, n, plugin, inputs, boxed, ghost, room)?)),
            FOp::Poly { lane, voices, voice, alloc, glide, .. } => {
                if in_voice {
                    bail!("{}: poly inside poly", n.name);
                }
                let (slot, late) = match ghost {
                    true => (0, None),
                    false => {
                        // Through a note delay when it is late (RENDER-18).
                        let (src, late) = match nodes[*lane].op {
                            FOp::NoteDelay { lane: l, .. } => (l, Some(*lane)),
                            _ => (*lane, None),
                        };
                        let FOp::Lane { module, lane: lane_id, .. } = &nodes[src].op else { bail!("{}: poly needs a notes lane", n.name) };
                        (self.slot(module, lane_id)?, late)
                    }
                };
                let pool = match boxed {
                    Make::New(shape) => {
                        // Its voices' states are born in sandboxes of its own, sealed once all are born.
                        let outer = std::mem::take(&mut self.pool);
                        let mut built = || -> Result<Vec<VoiceSlot>> {
                            let mut slots = Vec::with_capacity(*voices as usize);
                            for _ in 0..*voices {
                                let graph = self.graph(&voice.nodes, &voice.order, true, &|_| Make::New(0), &HashMap::new());
                                let graph = graph.with_context(|| format!("{}: voice", n.name))?;
                                slots.push(VoiceSlot { graph, note: None, born: 0, dirty: false, last_peak: 0.0, taken: false });
                            }
                            Ok(slots)
                        };
                        let slots = built();
                        let mut sandboxes: Vec<Sandbox> = std::mem::replace(&mut self.pool, outer).into_iter().map(|(_, s)| s).collect();
                        let slots = slots?;
                        for sb in &mut sandboxes {
                            sb.seal().with_context(|| n.name.clone())?;
                        }
                        let mono = (alloc.mode != Mode::Poly).then(|| Box::new(poly::Mono::new(self.sample_rate)));
                        Some(Box::new(VoicePool { voices: slots, alloc: *alloc, mono, wheels: poly::Wheels::default(), sandboxes, born: 0, shape, stopped: false }))
                    }
                    _ => None,
                };
                let captures = voice.nodes.iter().filter_map(|n| if let FOp::Capture(p) = n.op { Some(p) } else { None }).collect();
                let plan = Vec::with_capacity(if ghost { 0 } else { 2 * self.caps(slot).ons + sched::LIVE_ROOM });
                Exec::Poly(Box::new(PolyNode { slot, late, out: voice.out, captures, plan, glide: *glide, pool, ghost }))
            }
            FOp::Feedback { input, channels } => Exec::Feedback { input: *input, ring: vec![0.0; *channels as usize * BLOCK] },
            FOp::NoteField(f) => Exec::NoteField(*f),
            FOp::Capture(_) => Exec::Capture(capture),
            FOp::Delay { input, frames } => {
                let kind = n.outputs.first().copied().unwrap_or(Kind::Control(None));
                let shape = match boxed {
                    Make::New(shape) => shape,
                    _ => 0,
                };
                let line = delay::Line::new(kind.channels(), *frames, matches!(kind, Kind::Control(_)), shape);
                Exec::Delay(delay::DelayNode { input: *input, line: Box::new(line) })
            }
            FOp::NoteDelay { lane, frames } => {
                let FOp::Lane { module, lane: id, .. } = &nodes[*lane].op else { bail!("{}: a note delay needs a notes lane", n.name) };
                let slot = self.slot(module, id)?;
                let shape = match boxed {
                    Make::New(shape) => shape,
                    _ => 0,
                };
                let per_block = note_delay_room(self.caps(slot));
                let line = Box::new(delay::NoteLine::new(*frames, per_block, shape));
                Exec::NoteDelay(Box::new(delay::NoteDelayNode { slot, line, out: Events::with_capacity(2 * per_block) }))
            }
        })
    }

    /// Kernel `k` for node `n` in a voice: its state born in the pool's sandbox for its package,
    /// made when the pool's first kernel of that package is.
    fn in_pool(&mut self, n: &FNode, k: &Kernel, shape: u64) -> Result<KernelInst> {
        let id = k.id;
        let (module, prefix, package) = self.packages.module_for(id).with_context(|| format!("{}: kernel `{id}` is not loaded", n.name))?;
        let at = match self.pool.iter().position(|(p, _)| p == package) {
            Some(at) => at,
            None => {
                let sb = Sandbox::new(&self.packages.engine, module).with_context(|| format!("{}: `{id}`", n.name))?;
                self.pool.push((package.to_string(), sb));
                self.pool.len() - 1
            }
        };
        KernelInst::born(&mut self.pool[at].1, at, prefix, self.packages, n, k, shape)
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
        let reset_first = matches!(boxed, Make::Lent(_));
        let made = self.at.and_then(|i| self.made.remove(&i));
        let inst = match boxed {
            Make::Lent(inst) => Some(inst),
            Make::New(shape) => Some(match made {
                Some(i) => i,
                None => self.create(n, &info.found, shape)?,
            }),
            Make::Moved => None,
        };
        // Inputs come in the order of the plugin's ports; a parameter is bound by the id in its
        // address, never by its place in the list (FMT-18).
        let sig = m2_ir::Kernels::plugin(self.packages, id).with_context(|| format!("{}: plugin `{id}` was not described", n.name))?;
        let (mut audio_in, mut notes, mut late, mut params, mut driven) = (None, None, None, vec![], vec![]);
        for (port, input) in sig.inputs.iter().zip(inputs) {
            match (port.name.as_str(), input) {
                ("in", _) => audio_in = Some(input.clone()),
                ("notes", Input::Port { src, .. }) if !ghost => {
                    // Through a note delay when it is late (RENDER-18).
                    let lane = match nodes[src.node].op {
                        FOp::NoteDelay { lane, .. } => {
                            late = Some(src.node);
                            lane
                        }
                        _ => src.node,
                    };
                    if let FOp::Lane { module, lane, .. } = &nodes[lane].op {
                        notes = Some(self.slot(module, lane)?);
                    }
                }
                ("notes", _) | (_, Input::Unset) => {}
                (name, _) => {
                    let (_, pid) = packages::address(name).with_context(|| format!("{}: `{name}` isn't a parameter address", n.name))?;
                    params.push((pid, input.clone(), None));
                    // A value in code is a constant (a cell, or folded); anything else drives it. A
                    // ghost's inputs name the plan before's nodes, not `nodes`: it only plays out.
                    let src_op = |src: &PortRef| nodes.get(src.node).map(|x| &x.op);
                    driven.push(!ghost && matches!(input, Input::Port { src, .. } if !matches!(src_op(src), Some(FOp::Const(_)))));
                }
            }
        }
        // Room for one run's notes, a Tuning for each On, every parameter sampled, and a choke and
        // a NoteOff for every note it holds, a NoteOff for every note it lets go, and a NoteOn and
        // a Tuning for every note it starts again after an export, as many as its lane's room holds
        // or the node it took its instance from held.
        // Live notes too (MON-7): as many more held, and a NoteOn and a Tuning for each struck; and
        // the lane's wheels.
        let caps = notes.map_or(Caps::default(), |s| self.caps(s));
        let held = |c: Caps| c.sounding + 2 * c.ons + 8 + sched::LIVE_ROOM;
        let held = held(caps).max(room.map_or(0, held));
        let events = caps.events() + caps.ons + 2 * sched::LIVE_ROOM + sched::WHEELS + (BLOCK / PARAM_STEP) * params.len() + 8 + 5 * held;
        // A call crosses to the plugin host process in shared memory, which can't grow (HOST-3).
        if events > m2_plugins::MAX_CALL_EVENTS {
            bail!(
                "{}: one call to this plugin may carry {events} events (notes and parameter changes), more than the {} a call can carry; \
                 fewer notes at once on its lane, or fewer parameters driven, make it fit",
                n.name,
                m2_plugins::MAX_CALL_EVENTS
            );
        }
        Ok(PluginNode {
            name: n.name.clone(),
            inst,
            audio_in,
            notes,
            late,
            offset: if ghost { 0 } else { self.at.and_then(|i| self.offsets.get(i)).copied().unwrap_or(0) },
            told: delay::Told::default(),
            keys: Vec::with_capacity(held),
            ended: Vec::with_capacity(held),
            release: Vec::with_capacity(held),
            over: vec![None; params.len()],
            quiet: 0,
            params,
            driven,
            events: m2_plugins::Events::with_capacity(events),
            out: m2_plugins::Events::with_capacity(OUT_EVENTS),
            status: m2_plugins::Status::Continue,
            offs: 0,
            last_off: None,
            reset_first,
            again: false,
            reports: None,
        })
    }
}

/// A new instance for plugin node `n` (of shape `shape`, its hash), reporting to `owner`: created with
/// its committed state, set to `mode`, activated at `rate` and its latency measured (RENDER-17), on
/// the plugin main thread.
#[allow(clippy::too_many_arguments)]
fn make_plugin(
    packages: &PackageSet,
    data: &DataSet,
    owner: &m2_plugins::Owner,
    rate: u32,
    mode: m2_plugins::RenderMode,
    n: &FNode,
    found: &m2_plugins::Found,
    shape: u64,
) -> Result<Arc<PluginInstance>> {
    let blob = state_blob(data, n)?;
    PluginInstance::create(packages.host(), found, blob, owner, rate, shape, mode).with_context(|| n.name.clone())
}

/// Each checked node's latency (RENDER-17): a kernel's as its package declares it, a plugin's as its
/// instance last measured it: the one it keeps, the one lent to it, or a new one made here, into
/// `made`, for its node to play.
#[allow(clippy::too_many_arguments)]
fn latencies(
    flat: &Flat,
    packages: &PackageSet,
    data: &DataSet,
    assets: &audio::KernelAssets,
    keep: Option<&Keep>,
    lent: Option<&HashMap<usize, Arc<PluginInstance>>>,
    made: &mut HashMap<usize, Arc<PluginInstance>>,
    mk: impl Fn(&FNode, &m2_plugins::Found, u64) -> Result<Arc<PluginInstance>>,
) -> Result<Vec<u32>> {
    let mut lat = vec![0; flat.nodes.len()];
    let used = used(flat);
    let shape = |i: usize| if used[i] { Shape::of(&flat.nodes[i], packages, assets) } else { None };
    let kept = keep.map(|k| {
        let shapes: Vec<Option<Shape>> = (0..flat.nodes.len()).map(shape).collect();
        let blobs: Vec<Option<m2_song::Sha>> = flat.nodes.iter().map(|n| plan::blob(n, data)).collect();
        (k, keep::matching(k, flat, &shapes, &blobs))
    });
    for &i in &flat.order {
        let n = &flat.nodes[i];
        lat[i] = match &n.op {
            FOp::Kernel { kernel, .. } => m2_ir::Kernels::kernel(packages, kernel).map_or(0, |s| s.latency),
            FOp::Plugin { plugin, .. } if !packages.plugin_missing(plugin) => {
                let had = match &kept {
                    Some((k, (kept, same))) if kept[i] => same[i].and_then(|j| k.info.plugin(j)).and_then(|p| p.inst.upgrade()),
                    _ => lent.and_then(|l| l.get(&i)).cloned(),
                };
                match had {
                    Some(inst) => inst.latency(),
                    None => {
                        let info = packages.plugin_info(plugin).with_context(|| format!("{}: plugin `{plugin}` was not described", n.name))?;
                        let inst = mk(n, &info.found, shape(i).as_ref().map_or(0, Shape::hash64))?;
                        let frames = inst.latency();
                        made.insert(i, inst);
                        frames
                    }
                }
            }
            _ => 0,
        };
    }
    Ok(lat)
}

/// `cont` for the graph that plays a checked graph of `checked` nodes with `l`'s delays: each delay
/// continues the one the plan that plays (`old`) has on its consumer's old node, on the same input
/// and as long (RENDER-25): its line moves, with what is in flight in it.
fn continue_delays(cont: &Continues, checked: usize, l: &latency::Latency, old: &PlanInfo) -> Continues {
    let mut top = cont.top.clone();
    top.resize(checked, None);
    let old_checked = old.played.as_ref().map_or(0, |p| p.nodes.len() - old.latency.delays.len());
    for d in &l.delays {
        let j = cont.top.get(d.consumer).copied().flatten().map(|j| j as usize);
        let k = j.and_then(|j| old.latency.delays.iter().position(|o| o.consumer == j && o.at == d.at && o.frames == d.frames));
        top.push(k.map(|k| (old_checked + k) as u32));
    }
    Continues { top }
}

/// Note events a note delay holds per block of its length: its lane's room, live notes too.
fn note_delay_room(caps: Caps) -> usize {
    caps.events() + sched::LIVE_ROOM + sched::WHEELS
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
fn fade(b: &Block, (old, new, wait): (&Input, &Input, u32), r: &impl Reads, had: u64, out: &mut Buf) -> u64 {
    // Channel `c` of an input, a mono one in every channel.
    let read = |i: &Input, c: usize, f: usize| match i {
        Input::Port { src, .. } => {
            let s = r.buf(src.node, src.port);
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
                Input::Port { src, .. } => r.version(src.node),
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

/// One node's frames `seg` of block `b`: what every graph's loop and every partition's task does for
/// each of its nodes. It writes only `own` and reads other nodes through `r`; a voice's captures
/// read the graph around its `poly` through `outer`. At the top level (no `outer`) a live block
/// marks a box whose output went non-finite, or that failed (RENDER-25). Never allocates while it
/// succeeds.
#[allow(clippy::too_many_arguments)]
fn step<R: Reads>(
    own: Own,
    r: &R,
    b: &Block,
    seg: Seg,
    outer: Option<(&dyn Reads, &[PortRef])>,
    note: Option<Voiced>,
    lanes: &[LaneSlot],
    pool: &mut [Sandbox],
) -> Result<()> {
    let Own { exec, out, version, poisoned } = own;
    let n = seg.n;
    let done = match &mut *exec {
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
        // From the block's runs, as plugins are told (HOST-6): 1 where the transport moves.
        Exec::Time(TimeOf::Playing) => {
            for run in b.runs.as_slice() {
                let (lo, hi) = ((run.off as usize).max(seg.off), ((run.off + run.n) as usize).min(seg.off + n));
                if lo < hi {
                    out[0][lo..hi].fill(if run.moving { 1.0 } else { 0.0 });
                }
            }
            Ok(())
        }
        Exec::Kernel(k) => k.process(b, seg, r, out, pool),
        Exec::Poly(p) => p.process(b, r, &mut out[0], lanes),
        Exec::Plugin(p) => p.process(b, r, &mut out[0], lanes),
        Exec::Missing(input) => {
            for c in 0..out[0].len() / BLOCK {
                match input {
                    Some(Input::Port { src, src_channels, .. }) => {
                        let sc = if c < *src_channels as usize { c } else { 0 };
                        out[0][seg.range(c)].copy_from_slice(&r.buf(src.node, src.port)[seg.range(sc)]);
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
            let o = &mut out[0][seg.range(0)];
            let voiced = note.expect("note fields only run in voices");
            let (Voiced::Cv(_, wheels) | Voiced::Note(_, wheels)) = voiced;
            match voiced {
                Voiced::Cv(cv, _) => {
                    let at = seg.off..seg.off + n;
                    match field {
                        NoteField::Pitch => o.copy_from_slice(&cv.pitch[at]),
                        NoteField::Gate => o.copy_from_slice(&cv.gate[at]),
                        NoteField::Velocity => o.copy_from_slice(&cv.velocity[at]),
                        NoteField::Pressure | NoteField::Timbre => o.fill(0.0),
                        NoteField::ModWheel => o.fill(wheels.wheel),
                    }
                }
                Voiced::Note(note, _) => match field {
                    NoteField::Pitch => o.fill(note.pitch),
                    NoteField::Velocity => o.fill(note.velocity),
                    NoteField::Pressure | NoteField::Timbre => o.fill(0.0),
                    NoteField::ModWheel => o.fill(wheels.wheel),
                    NoteField::Gate => {
                        // Open from the note's On until its Off, on the engine clock (RENDER-13).
                        for (k, x) in o.iter_mut().enumerate() {
                            let t = b.engine + (seg.off + k) as u64;
                            *x = (note.on <= t && t < note.off) as u8 as f32;
                        }
                    }
                },
            }
            // The lane's bend, on top of the note's pitch and its slide. Centred, nothing is added:
            // the pitch is the note's, bit for bit.
            if *field == NoteField::Pitch && wheels.bend != 0.0 {
                o.iter_mut().for_each(|x| *x += wheels.bend);
            }
            Ok(())
        }
        Exec::Capture(k) => {
            let (outer, captures) = outer.expect("captures only run in voices");
            let p = captures[*k];
            let src = outer.buf(p.node, p.port);
            for c in 0..out[0].len() / BLOCK {
                out[0][seg.range(c)].copy_from_slice(&src[seg.range(c.min(src.len() / BLOCK - 1))]);
            }
            Ok(())
        }
        Exec::Fade { old, new, wait } => {
            *version = fade(b, (old, new, *wait), r, *version, &mut out[0]);
            Ok(())
        }
        Exec::Overlay { src, target, cur, step } => {
            overlay::process(r.buf(src.node, src.port), &mut out[0], *target, cur, *step);
            Ok(())
        }
        Exec::Delay(d) => {
            debug_assert_eq!((seg.off, seg.n), (0, BLOCK), "delays only play at the top level");
            d.line.process(b.engine, r.buf(d.input.node, d.input.port), &mut out[0]);
            Ok(())
        }
        Exec::NoteDelay(d) => {
            let delay::NoteDelayNode { slot, line, out } = &mut **d;
            line.process(b.engine, lanes[*slot].events(), out);
            Ok(())
        }
    };
    // RENDER-25: live, a box whose output went non-finite, or that trapped or stopped, is marked;
    // an activation resets it as it moves it. A finite block clears the mark.
    if outer.is_none() && b.live && matches!(exec, Exec::Kernel(_) | Exec::Poly(_) | Exec::Plugin(_) | Exec::Feedback { .. }) {
        *poisoned = done.is_err() || exec.stopped() || out.iter().any(|o| o.iter().fold(false, |a, x| a | !x.is_finite()));
    }
    done
}

/// Block `b`'s graph: each partition of `sched` a task on `runner`, its nodes in plan order.
/// Offline, the error first in plan order, if a node failed: what reads a failed node is skipped, the
/// rest runs. Live, a node that fails stops alone, reported through `report`. A panic in a task is
/// raised again here, once the block is done.
fn run_graph(
    graph: &mut Graph,
    sched: &mut Schedule,
    lanes: &[LaneSlot],
    b: &Block,
    runner: &mut Runner,
    report: &Reporter,
) -> Option<anyhow::Error> {
    let Schedule { tasks, parts } = sched;
    let view = GraphView::new(graph);
    let shared = &*parts;
    let task = |p: u32| run_part(&view, shared, p, b, lanes, report);
    if let Err(payload) = runner.run(tasks, &task) {
        std::panic::resume_unwind(payload);
    }
    parts.take_error()
}

/// Partition `p`'s task: its nodes in plan order, in the one floating-point mode. Offline (an
/// export too, strict or not), false when a node failed, its error kept in `parts` as a
/// [`fault::Failed`] naming the node (RENDER-1). Live, played by
/// the [`Engine`], a node that fails stops alone (RENDER-25):
/// it is silent from then on, what reads it reads silence, `sound.stopped` is reported for the
/// session to tell, and the partition goes on.
fn run_part(view: &GraphView, parts: &parts::Parts, p: u32, b: &Block, lanes: &[LaneSlot], report: &Reporter) -> bool {
    // Whatever ran on this thread before, this partition computes in the one mode.
    fpenv::canon();
    for &i in parts.nodes(p) {
        let i = i as usize;
        // SAFETY: partition p's task alone runs its nodes this block, and every node it reads
        // finished before: earlier in p, or in a partition p waits for (the pool runs a task after
        // its inputs complete, with Acquire).
        let own = unsafe { view.own(i) };
        if let Err(e) = step(own, &view.reader(parts, p, i), b, Seg::WHOLE, None, None, lanes, &mut []) {
            if !b.live {
                let f = report.fault(Code::of(&e), fault::Subject::Node(i as u32));
                // SAFETY: this is partition p's task.
                unsafe { parts.fail(p, i, fault::Failed::new(f, Some(e)).into()) };
                return false;
            }
            // Its error allocated, and is dropped here: this path only.
            report.node(Code::of(&e), i);
            // SAFETY: as above; the step's own ended with it.
            let own = unsafe { view.own(i) };
            silence(own.exec, own.out);
        }
    }
    debug_assert!(fpenv::is_canon(), "partition {p} left the thread in another floating-point mode");
    true
}

/// A node that failed live stops (RENDER-25): a kernel or a `poly` plays silence from now on and
/// calls nothing; its outputs are silence at once. Never allocates.
fn silence(exec: &mut Exec, out: &mut [Buf]) {
    exec.set_stopped(true);
    out.iter_mut().for_each(|o| o.fill(0.0));
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

    /// Process frames `seg` of block `b`, every node in order on this thread: a voice graph's loop,
    /// and step 7's loop for the top level (the gates' old-loop switch). Buffers are indexed by
    /// position in the block. A voice graph gets the outputs around its `poly` and what its
    /// captures read in them.
    fn process(
        &mut self,
        b: &Block,
        seg: Seg,
        outer: Option<(&dyn Reads, &[PortRef])>,
        note: Option<Voiced>,
        lanes: &[LaneSlot],
    ) -> Result<()> {
        self.process_in(b, seg, outer, note, lanes, &mut [])
    }

    /// [`Graph::process`] for a voice's graph, whose kernels' states are in its pool's sandboxes.
    fn process_in(
        &mut self,
        b: &Block,
        seg: Seg,
        outer: Option<(&dyn Reads, &[PortRef])>,
        note: Option<Voiced>,
        lanes: &[LaneSlot],
        pool: &mut [Sandbox],
    ) -> Result<()> {
        for oi in 0..self.order.len() {
            let i = self.order[oi];
            let mut out = std::mem::take(&mut self.bufs[i]);
            let (mut version, mut poisoned) = (self.versions[i], self.poisoned[i]);
            let own = Own { exec: &mut self.exec[i], out: &mut out, version: &mut version, poisoned: &mut poisoned };
            let done = step(own, &Local { bufs: &self.bufs, versions: &self.versions }, b, seg, outer, note, lanes, pool);
            (self.bufs[i], self.versions[i], self.poisoned[i]) = (out, version, poisoned);
            done?;
        }
        self.feedback(b, seg);
        Ok(())
    }

    /// Feedback stores its input for `BLOCK` samples later: after every node ran.
    fn feedback(&mut self, b: &Block, seg: Seg) {
        for i in 0..self.exec.len() {
            if let Exec::Feedback { input, ring } = &mut self.exec[i] {
                let src = &self.bufs[input.node][input.port];
                let src_ch = src.len() / BLOCK;
                for c in 0..ring.len() / BLOCK {
                    let sc = if c < src_ch { c } else { 0 };
                    for f in 0..seg.n {
                        let at = (b.engine as usize + seg.off + f) % BLOCK;
                        ring[c * BLOCK + at] = src[sc * BLOCK + seg.off + f];
                    }
                }
            }
        }
    }

    /// Back to the state right after building, every mark cleared (RENDER-10).
    fn reset(&mut self) -> Result<()> {
        self.reset_in(&mut [])
    }

    /// [`Graph::reset`] for a voice's graph, whose kernels' states are in its pool's sandboxes.
    fn reset_in(&mut self, pool: &mut [Sandbox]) -> Result<()> {
        // Counted here, so no path resets the graph unseen (RENDER-10).
        self.resets += 1;
        self.poisoned.iter_mut().for_each(|p| *p = false);
        for i in 0..self.exec.len() {
            self.reset_node_in(i, pool)?;
        }
        Ok(())
    }

    /// [`Graph::reset`] for a panic (RENDER-10, RENDER-25): a node whose reset fails stops alone,
    /// reported through `report`, and the rest is reset all the same. Never allocates while every
    /// reset succeeds.
    fn panic(&mut self, report: &Reporter) {
        self.resets += 1;
        self.poisoned.iter_mut().for_each(|p| *p = false);
        for i in 0..self.exec.len() {
            // Its error allocated: this path only.
            if let Err(e) = self.reset_node(i) {
                self.stop(i);
                report.node(Code::of(&e), i);
            }
        }
    }

    /// Node `i`'s box back to the state right after building, as a panic resets it: a kernel's
    /// `k_reset`, a pool's voices with their notes dropped, a plugin's `reset` with its notes to
    /// end, a ring zeroed. A node that stopped plays again once reset; one whose reset fails again
    /// stays stopped, and the rest goes on (RENDER-25). Never allocates while it succeeds.
    fn reset_node(&mut self, i: usize) -> Result<()> {
        self.reset_node_in(i, &mut [])
    }

    /// [`Graph::reset_node`] in a voice's graph, whose kernels' states are in its pool's sandboxes.
    fn reset_node_in(&mut self, i: usize, pool: &mut [Sandbox]) -> Result<()> {
        let stopped = self.exec[i].stopped();
        let done = match &mut self.exec[i] {
            Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.reset(pool),
            Exec::Plugin(p) => p.reset(),
            Exec::Feedback { ring, .. } => {
                ring.fill(0.0);
                Ok(())
            }
            Exec::Poly(p) => p.reset(),
            Exec::Delay(d) => {
                d.line.reset();
                Ok(())
            }
            Exec::NoteDelay(d) => {
                d.line.reset();
                d.out.clear();
                Ok(())
            }
            _ => Ok(()),
        };
        match done {
            Ok(()) => {
                self.exec[i].set_stopped(false);
                Ok(())
            }
            // Its error allocated: this path only.
            Err(_) if stopped => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Node `i` stops (RENDER-25): it plays silence from now on, and calls nothing.
    fn stop(&mut self, i: usize) {
        silence(&mut self.exec[i], &mut self.bufs[i]);
    }

    /// The notes sounding into node `i` end now, at engine sample `clock`: a poly's voices go into
    /// their release, a plugin's notes get a NoteOff at its next call (RENDER-26a). Never allocates.
    fn release(&mut self, i: usize, clock: u64) {
        match &mut self.exec[i] {
            Exec::Poly(p) => {
                let Some(pool) = p.pool.as_deref_mut() else { return };
                for n in pool.voices.iter_mut().filter_map(|v| v.note.as_mut()) {
                    n.off = n.off.min(clock);
                }
                // A one-note pool's held notes are its own: dropped, so none sounds again.
                if let Some(m) = &mut pool.mono {
                    m.release();
                }
                // The old lane's wheels stay with it; the new lane sends its own (a ghost keeps
                // them through its release).
                if !p.ghost {
                    pool.wheels = poly::Wheels::default();
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
            (Exec::Delay(d), Edge::Input(_)) => d.input = src,
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

    /// Reset every kernel once, in voices too, after its sandbox is sealed: a reset that would take
    /// memory fails here, off the audio thread, naming its node (RENDER-28). A reset leaves a state
    /// as it was born, so nothing sounds different.
    fn prewarm(&mut self) -> Result<()> {
        self.prewarm_in(&mut [])
    }

    /// [`Graph::prewarm`] for a voice's graph, whose kernels' states are in its pool's sandboxes.
    fn prewarm_in(&mut self, pool: &mut [Sandbox]) -> Result<()> {
        for e in &mut self.exec {
            match e {
                Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.prewarm(pool)?,
                Exec::Poly(p) => {
                    if let Some(p) = p.pool.as_deref_mut() {
                        for v in &mut p.voices {
                            v.graph.prewarm_in(&mut p.sandboxes)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The kernel sandboxes it holds, in voices too (RENDER-32).
    fn sandboxes(&self) -> usize {
        self.exec
            .iter()
            .map(|e| match e {
                Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.own.is_some() as usize,
                Exec::Poly(p) => p.pool.iter().map(|pool| pool.sandboxes.len() + pool.voices.iter().map(|v| v.graph.sandboxes()).sum::<usize>()).sum(),
                _ => 0,
            })
            .sum()
    }

    /// The memory every kernel instance holds, in bytes.
    fn kernel_memory(&self) -> usize {
        self.exec
            .iter()
            .map(|e| match e {
                Exec::Kernel(KernelNode { inst: Some(k), .. }) => k.own.as_ref().map_or(0, Sandbox::size),
                Exec::Poly(p) => p.pool.iter().flat_map(|pool| &pool.sandboxes).map(Sandbox::size).sum(),
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

    /// Overlay `node`'s gain: where it is going, and where it is.
    fn overlay_gain(&self, node: usize) -> Option<(f32, f32)> {
        match self.exec.get(node) {
            Some(Exec::Overlay { target, cur, .. }) => Some((*target, *cur)),
            _ => None,
        }
    }

    /// Overlay `node`'s gain set to `gain` (going to, at), as [`Graph::overlay_gain`] gives it: it
    /// carries on as it was there. Never allocates.
    fn hold_overlay(&mut self, node: usize, gain: (f32, f32)) {
        if let Some(Exec::Overlay { target, cur, .. }) = self.exec.get_mut(node) {
            (*target, *cur) = gain;
        }
    }

    /// Overlay `node`'s gain moves to `value`: glides there (RENDER-22a), or with `now` is there at
    /// once, for a plan not yet sent. Never allocates.
    fn set_overlay(&mut self, node: usize, value: f32, now: bool) {
        let Some(Exec::Overlay { target, cur, .. }) = self.exec.get_mut(node) else {
            debug_assert!(false, "{node} isn't an overlay");
            return;
        };
        *target = value;
        if now {
            *cur = value;
        }
    }

    /// Whether any kernel still holds sound that will come out later.
    fn active(&mut self) -> Result<bool> {
        self.active_in(&mut [])
    }

    /// [`Graph::active`] for a voice's graph, whose kernels' states are in its pool's sandboxes.
    fn active_in(&mut self, pool: &mut [Sandbox]) -> Result<bool> {
        for e in &mut self.exec {
            let active = match e {
                // One that stopped is never called again, and holds nothing that will sound.
                Exec::Kernel(KernelNode { inst: Some(k), .. }) if !k.stopped => k.active(pool)?,
                Exec::Feedback { ring, .. } => ring.iter().any(|x| x.abs() > QUIET),
                Exec::Delay(d) => d.line.active(),
                Exec::NoteDelay(d) => d.line.active(),
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
    /// Its voices back to the state right after building, their notes dropped.
    fn reset(&mut self) -> Result<()> {
        self.plan.clear();
        if let Some(pool) = &mut self.pool {
            pool.born = 0;
            for v in &mut pool.voices {
                if v.dirty {
                    v.graph.reset_in(&mut pool.sandboxes)?;
                }
                (v.note, v.born, v.dirty, v.last_peak, v.taken) = (None, 0, false, 0.0, false);
            }
            if let Some(m) = &mut pool.mono {
                m.reset();
            }
            pool.wheels = poly::Wheels::default();
        }
        Ok(())
    }

    /// One block. The lane's events say which notes start and end; voices are given notes in
    /// event order, as the pool's `alloc` says, and a voice's graph is split where its note
    /// changes. Never allocates.
    fn process(&mut self, b: &Block, outer: &dyn Reads, out: &mut Buf, lanes: &[LaneSlot]) -> Result<()> {
        let (start, end) = (b.engine, b.engine + BLOCK as u64);
        let events = match (self.ghost, self.late) {
            (true, _) => &[],
            (false, Some(d)) => outer.events(d),
            (false, None) => lanes[self.slot].events(),
        };
        let PolyNode { out: voice_out, captures, plan, pool, glide, .. } = self;
        let Some(pool) = pool.as_deref_mut().filter(|p| !p.stopped) else {
            out.fill(0.0);
            return Ok(());
        };
        let VoicePool { voices, born, sandboxes, alloc, mono, wheels, .. } = pool;
        let alloc = *alloc;
        // The lane's wheels, for the whole block: they come at its first frame (D8).
        wheels.read(events);
        let wheels = *wheels;
        out.fill(0.0);
        let out_ch = out.len() / BLOCK;
        let glide = glide.map(|g| &outer.buf(g.node, g.port)[..BLOCK]);
        let outer = (outer, &captures[..]);
        // Sum the voice's frames `seg` into `out`, keeping their peak.
        let segment = |v: &mut VoiceSlot, seg: Seg, note: Voiced, peak: &mut f32, out: &mut Buf, sandboxes: &mut [Sandbox]| -> Result<()> {
            v.graph.process_in(b, seg, Some(outer), Some(note), &[], sandboxes)?;
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

        // One note at a time: the pool's pitch, gate and velocity, frame by frame. Once started, the
        // voice runs every block, reset where a note starts again when `retrigger` says so.
        if let Some(m) = mono.as_deref_mut() {
            let was = m.started;
            m.block(events, glide, alloc);
            if !m.started {
                return Ok(());
            }
            let v = &mut voices[0];
            // A voice starts at its first note.
            let (mut at, mut peak) = (if was { 0 } else { m.first as usize }, 0.0f32);
            for &f in &m.starts[..m.n_starts] {
                let f = f as usize;
                if f > at {
                    segment(v, Seg { off: at, n: f - at }, Voiced::Cv(&m.cv, wheels), &mut peak, out, sandboxes)?;
                    v.dirty = true;
                }
                if v.dirty {
                    v.graph.reset_in(sandboxes)?;
                    v.dirty = false;
                }
                at = f;
            }
            segment(v, Seg { off: at, n: BLOCK - at }, Voiced::Cv(&m.cv, wheels), &mut peak, out, sandboxes)?;
            v.dirty = true;
            v.last_peak = peak;
            // The voice's note, as a poly voice holds one: what a release and the stuck-note
            // checks read (RENDER-26a). Open while the gate is; then until it rings out.
            v.note = match (m.sounding(), v.note) {
                (Some((id, pitch, velocity)), n) => {
                    let on = n.filter(|n| n.id == id).map_or(start, |n| n.on);
                    Some(VoiceNote { id, pitch, velocity, on, off: u64::MAX })
                }
                (None, Some(n)) if peak >= QUIET || v.graph.active_in(sandboxes)? => Some(VoiceNote { off: n.off.min(start), ..n }),
                (None, _) => None,
            };
            return Ok(());
        }

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
                && !v.graph.active_in(sandboxes)?
            {
                v.note = None;
            }
        }
        // Give this block's notes voices, in event order: a free voice, else the one `steal` names.
        plan.clear();
        for e in events {
            if let Event::On { frame, id, pitch, velocity, .. } = *e {
                let Some(slot) = poly::pick(voices, alloc.steal) else { continue };
                if plan.len() == plan.capacity() {
                    debug_assert!(false, "more notes start than the lane holds");
                    continue;
                }
                *born += 1;
                voices[slot].born = *born;
                voices[slot].taken = true;
                let at = start + frame as u64;
                plan.push(Planned { voice: slot as u16, frame: frame as u16, note: VoiceNote { id, pitch, velocity, on: at, off: u64::MAX } });
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

        for (vi, v) in voices.iter_mut().enumerate() {
            let taken = std::mem::take(&mut v.taken);
            if v.note.is_none() && !taken {
                continue;
            }
            // Split the block wherever this voice starts a new note.
            let mut at = 0;
            let mut current = v.note;
            let mut peak = 0.0f32;
            for p in plan.iter().filter(|p| p.voice as usize == vi) {
                let off = p.frame as usize;
                let mut note = p.note;
                if let Some(c) = current
                    && off > at
                {
                    segment(v, Seg { off: at, n: off - at }, Voiced::Note(c, wheels), &mut peak, out, sandboxes)?;
                }
                if alloc.retrigger {
                    if v.dirty {
                        v.graph.reset_in(sandboxes)?;
                    }
                } else if let Some(c) = current
                    && c.on <= note.on
                    && note.on < c.off
                {
                    // Carried on from an open gate: it falls for one frame.
                    note.on += 1;
                }
                v.dirty = true;
                v.note = Some(note);
                peak = 0.0;
                current = Some(note);
                at = off;
            }
            if let Some(c) = current {
                segment(v, Seg { off: at, n: BLOCK - at }, Voiced::Note(c, wheels), &mut peak, out, sandboxes)?;
            }
            v.last_peak = peak;
            if let Some(n) = v.note
                && n.off < end
                && peak < QUIET
                && !v.graph.active_in(sandboxes)?
            {
                v.note = None;
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

    /// Frames not yet handed out.
    pub(crate) fn left(&self) -> usize {
        self.len - self.read
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

/// Each exec node's label, the module its partition is named by (8a, RENDER-2): a plan node its
/// module; a ghost its old module; an overlay the module of what it reads (a track's mute runs in
/// the track); a fade the node that reads it (a feedback's ring reads it after the graph, so a
/// fade only a feedback reads, and the output's fade, take their new side's, else their old
/// side's). One left without (nothing to follow) is labelled "".
///
/// Then, in `order`, a plan node whose inputs all come from one other label, idle nodes and those
/// outside the order aside, takes that label, and an overlay follows its source again (R1): a
/// mixer strip (a channel's pan and fader) runs with its track, not in the song's serial tail. A
/// node that reads a fade never moves: the fade took its label.
fn labels(
    flat: &Flat,
    w: &Wiring,
    exec: &[Exec],
    order: &[usize],
    fades: std::ops::Range<usize>,
    placed: &overlay::Placed,
) -> Vec<Box<str>> {
    let n = flat.nodes.len();
    let mut label: Vec<Option<&str>> = vec![None; exec.len()];
    for (i, node) in flat.nodes.iter().enumerate() {
        label[i] = Some(&node.module);
    }
    for (g, (_, node)) in w.ghosts.iter().enumerate() {
        label[n + g] = Some(&node.module);
    }
    for &(i, edge, f) in &w.rewire {
        if fades.contains(&f) && edge != Edge::Feedback {
            label[f] = label[i];
        }
    }
    let side = |i: &Input| if let Input::Port { src, .. } = i { Some(src.node) } else { None };
    let overlays = fades.end..fades.end + placed.nodes.len();
    // Sides and sources may be labelled only on a later pass: a fade's new side can be an overlay.
    for _ in 0..3 {
        for (k, o) in placed.nodes.iter().enumerate() {
            label[overlays.start + k] = label[overlays.start + k].or(label[o.src.node]);
        }
        for f in fades.clone() {
            if label[f].is_none()
                && let Exec::Fade { old, new, .. } = &exec[f]
            {
                label[f] = side(new).and_then(|j| label[j]).or_else(|| side(old).and_then(|j| label[j]));
            }
        }
    }
    let mut runs = vec![false; exec.len()];
    for &i in order {
        runs[i] = !exec[i].idle();
    }
    for &i in order {
        if overlays.contains(&i) {
            label[i] = label[placed.nodes[i - overlays.start].src.node];
            continue;
        }
        if i >= n {
            continue;
        }
        // The one label all its running inputs have, if there is one.
        let (mut from, mut one) = (None, true);
        exec[i].reads(|j| {
            if runs[j] {
                one &= from.is_none_or(|l| l == label[j]);
                from = Some(label[j]);
            }
        });
        if let Some(Some(l)) = from.filter(|_| one)
            && Some(l) != label[i]
        {
            debug_assert!(
                {
                    let mut fade = false;
                    exec[i].reads(|j| fade |= fades.contains(&j));
                    !fade
                },
                "node {i} reads a fade, so it can't move"
            );
            label[i] = Some(l);
        }
    }
    label.into_iter().map(|l| Box::from(l.unwrap_or(""))).collect()
}

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
    /// Played by the live [`Engine`], which marks every renderer it plays: a node that fails stops
    /// alone (RENDER-25). Offline, every render fails, naming the node.
    live: bool,
    /// Its audio lanes' rings are filled by a player's disk thread: marked before it is sent, so no
    /// ring is ever filled on the sound thread (RENDER-30, D11). Unmarked, it fills them itself
    /// before each block, as an offline render does (D10).
    disk: bool,
    /// Its ring is its own, an offline cache's: read before each block and after it, so the first
    /// fault found while it renders (a file it can't read, a stretch) fails it (RENDER-1).
    checks: bool,
    non_finite: u64,
    trace: Option<Trace>,
    dropped: u32,
    /// The whole order as one partition, made when a gate first asks ([`gauge`]).
    whole: Option<Box<Schedule>>,
    /// The song's latency `L` (RENDER-19): its output is that many frames late.
    latency: u32,
    /// Frames an offline render still drops from its start: `L`, so its sample 0 is song time 0.
    lead: u64,
    /// The transport over the last blocks, for plugins told it late (HOST-6).
    past: delay::Past,
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
        // Each kernel's first state is computed here, by `k_init` and a reset (RENDER-2a).
        let _mode = fpenv::Canon::enter();
        let tempo = data.tempo().clone();
        let rate = sample_rate as f64;
        let keep = match &basis {
            Basis::Keep(k) if k.info.rate != sample_rate => bail!("a plan is prepared against one at {} Hz, not {sample_rate}", k.info.rate),
            Basis::Keep(k) => Some(k),
            Basis::Fresh | Basis::Lend(_) => None,
        };
        let lent = if let Basis::Lend(l) = &basis { Some(*l) } else { None };
        // Latency (RENDER-17): each node's, its plugins' measured by their instances (new ones made
        // here for that); then the delays that align every input (RENDER-18). With none, the
        // checked graph plays as it is.
        let checked = flat;
        let mut made = HashMap::new();
        let mk = |n: &FNode, found: &m2_plugins::Found, shape: u64| make_plugin(packages, data, owner, sample_rate, mode, n, found, shape);
        // The assets its kernels read: the delays below add no kernel, so the checked graph's are
        // the played one's.
        // Its parts report to the cache's ring, as this plan: a live session's is the app's
        // (RENDER-25). Its build reports too: a file it can't read.
        let id = GENERATION.fetch_add(1, Ordering::Relaxed);
        let report = Reporter::new(cache.faults().clone(), id);
        let assets = audio::KernelAssets::of(checked, data, cache, &report);
        let lat = latencies(checked, packages, data, &assets, keep, lent, &mut made, mk)?;
        let l = latency::plan(checked, &lat);
        let played = (!l.delays.is_empty()).then(|| Arc::new(latency::insert(checked, &l)));
        let flat: &Flat = played.as_deref().unwrap_or(checked);
        // Wired against the graph the plan that plays plays, its delays kept by consumer, input and
        // length.
        let cont = keep.map(|k| continue_delays(k.cont, checked.nodes.len(), &l, k.info));
        let keep = keep.zip(cont.as_ref()).map(|(k, cont)| Keep {
            old: k.info.played.as_deref().unwrap_or(k.old),
            old_data: k.old_data,
            info: k.info,
            cont,
            restarts: k.restarts,
        });
        let keep = keep.as_ref();
        let stretch = offline::Kernel::of(packages, offline::STRETCH);
        let mut lanes = LanePlan::new(flat, data, cache, &tempo, rate, keep, stretch.as_ref(), &report)?;
        let used = used(flat);
        let mut shapes: Vec<Option<Shape>> =
            flat.nodes.iter().zip(&used).map(|(n, &u)| if u { Shape::of(n, packages, &assets) } else { None }).collect();
        // A note delay's room is its lane's.
        for (i, n) in flat.nodes.iter().enumerate() {
            if let FOp::NoteDelay { lane, .. } = n.op
                && let FOp::Lane { module, lane: id, .. } = &flat.nodes[lane].op
                && let Some(slot) = lanes.lanes.refs.iter().position(|r| r.module == *module && r.lane == *id)
            {
                let room = note_delay_room(lanes.caps.get(slot).copied().unwrap_or_default());
                shapes[i] = shapes[i].take().map(|s| s.with_room(room));
            }
        }
        let blobs: Vec<Option<m2_song::Sha>> = flat.nodes.iter().map(|n| plan::blob(n, data)).collect();
        let w = match keep {
            Some(k) => keep::wire(k, flat, &shapes, &blobs, packages, &mut lanes),
            None => Wiring::fresh(flat),
        };
        let mut b = Builder {
            packages,
            data,
            sample_rate: rate,
            slots: &lanes.lanes.refs,
            caps: &lanes.caps,
            owner,
            mode,
            at: None,
            offsets: &l.offset,
            made,
            pool: vec![],
            assets: &assets,
        };
        let boxed = |i: usize| match (w.kept[i], lent.and_then(|l| l.get(&i))) {
            (true, _) => Make::Moved,
            (false, Some(inst)) => Make::Lent(inst.clone()),
            (false, None) => Make::New(shapes[i].as_ref().map_or(0, Shape::hash64)),
        };
        let needed = sandboxes(flat, &used, packages);
        if needed > SANDBOX_LIMIT {
            return Err(TooManySounds { needed, limit: SANDBOX_LIMIT }.into());
        }
        let mut graph = b.graph(&flat.nodes, &flat.order, false, &boxed, &w.room)?;
        graph.prewarm()?;
        // Ghosts, then fade nodes, after the new graph's nodes (RENDER-25a).
        let n = flat.nodes.len();
        b.at = None;
        for (g, (_, node)) in w.ghosts.iter().enumerate() {
            let e = b.node(&flat.nodes, node, false, Make::Moved, true, w.room.get(&(n + g)).copied(), 0)?;
            graph.push(e, &node.outputs);
        }
        // Overlays after the fades (MON-1): wired first, as they read and feed fades' sides.
        let mut w = w;
        let (fades, base) = (n + w.ghosts.len(), n + w.ghosts.len() + w.fades.len());
        let placed = overlay::place(flat, keep.map(|k| k.old), &mut w, fades, base);
        let len = (0.005 * rate).round().max(1.0) as u32;
        for f in &w.fades {
            let wait = if f.waits { len } else { 0 };
            graph.push(Exec::Fade { old: f.old.clone(), new: f.new.clone(), wait }, &[Kind::Audio(f.channels)]);
        }
        debug_assert_eq!(graph.exec.len(), base);
        let step = overlay::step(rate);
        for o in &placed.nodes {
            graph.push(Exec::Overlay { src: o.src, target: o.value, cur: o.value, step }, &[Kind::Audio(o.channels)]);
        }
        for &(i, edge, f) in &w.rewire {
            graph.rewire(i, edge, f);
        }
        let ghosts = n..n + w.ghosts.len();
        // Window-only overlays run while the ghosts and old sides they carry do.
        let windowed = base + placed.sites..base + placed.nodes.len();
        graph.order = w.order.clone();
        let labels = labels(flat, &w, &graph.exec, &graph.order, fades..base, &placed);
        let schedule = Schedule::build(&graph.order, &graph.exec, &labels)?;
        let window_overlays: Vec<(usize, usize)> =
            placed.nodes.iter().enumerate().skip(placed.sites).filter_map(|(j, o)| Some((base + j, o.follows?))).collect();
        let window_held: Vec<(usize, usize)> =
            placed.nodes.iter().enumerate().skip(placed.sites).filter(|(_, o)| o.follows.is_none()).filter_map(|(j, o)| Some((base + j, o.was?))).collect();
        let sites: Vec<(usize, f32)> = placed.nodes[..placed.sites].iter().enumerate().map(|(j, o)| (base + j, o.value)).collect();
        let mut cells = Cells::new(&cells::cells(flat), &sites);
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
                Some(PluginAt { node: i, handle, inst, blob: blobs[i], latency: lat.get(i).copied().unwrap_or(0) })
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
                        // Measured again (RENDER-17): a new latency is compensated from the next
                        // activation on, which the session makes at once.
                        if let Ok(Ok(frames)) = host.latency(p.handle).recv() {
                            inst.iter().for_each(|i| i.set_latency(frames));
                            if frames != p.latency {
                                stats.relatent.push((p.node, frames));
                            }
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
        let window = !w.ghosts.is_empty() || !w.fades.is_empty() || swaps > 0;
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
        let overlays = flat.overlays.iter().enumerate().map(|(k, s)| (s.module.clone(), s.into.clone(), cells.overlay(k))).collect();
        // A live plan's meters (8b.8): its output, and each celled overlay by its track.
        let taps = (mode == m2_plugins::RenderMode::Realtime).then(|| {
            let sites: Vec<(usize, &str, u8)> =
                flat.overlays.iter().enumerate().map(|(k, s)| (base + k, s.module.as_str(), placed.nodes[k].channels)).collect();
            Taps::new(&sites)
        });
        for (i, e) in graph.exec.iter_mut().enumerate().take(checked.nodes.len()) {
            if let Exec::Plugin(p) = e {
                p.reports = Some((report.clone(), i));
            }
        }
        stats.delays = l.delays.len();
        let late_plugins: Vec<usize> =
            (0..checked.nodes.len()).filter(|&i| l.offset[i] > 0 && matches!(graph.exec[i], Exec::Plugin(_))).collect();
        // The transport is remembered as far back as a plugin is told it (HOST-6).
        let (total, reach) = (l.total, l.offset.iter().copied().max().unwrap_or(0));
        let info = Arc::new(PlanInfo {
            id,
            rate: sample_rate,
            shapes,
            plugins,
            lanes: lane_info,
            cell_slots: cells.slots().clone(),
            cell_map: cells.map().clone(),
            overlays,
            taps: taps.as_ref().map(|t| t.slots().clone()),
            faults: cache.faults().clone(),
            latency: l,
            played,
        });
        let plan = Plan {
            id,
            plugins: graph.exec.iter().any(|e| matches!(e, Exec::Plugin(_))),
            graph,
            nodes: n,
            cells,
            out: w.out,
            entry,
            leftovers: window.then(|| Plan::leftovers(ghosts.len(), ghost_lanes, swaps)),
            ghosts,
            windowed,
            schedule,
            fade: if window { len } else { 0 },
            wait: if w.fades.iter().any(|f| f.waits) { len } else { 0 },
            faded_from: None,
            stats,
            info,
            window_overlays,
            window_held,
            taps,
            report,
            late_plugins,
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
            live: false,
            disk: false,
            checks: !cache.is_live(),
            non_finite: 0,
            trace: None,
            dropped: 0,
            whole: None,
            latency: total,
            lead: total as u64,
            past: delay::Past::new(reach),
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

    /// Its audio lanes are filled by a disk thread from now on, never by its own blocks: a player
    /// marks every renderer before it sends it (RENDER-30, D11).
    pub fn from_disk(&mut self) {
        self.disk = true;
    }

    /// Whether a disk thread fills its audio lanes ([`Self::from_disk`]).
    pub fn is_from_disk(&self) -> bool {
        self.disk
    }

    /// Its audio lanes, with the module each plays in: what a disk thread keeps filled.
    pub fn audio_lanes(&self) -> Vec<(String, Arc<audio::AudioLane>)> {
        self.plan.info.lanes.iter().filter_map(|l| Some((l.lane.module.clone(), l.audio()?.clone()))).collect()
    }

    /// Strict (the default): a NaN or infinity from any kernel is an error naming it.
    /// Not strict (live, and an export): such samples are silenced at the output and counted in
    /// [`Self::non_finite`].
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
            live: self.live,
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

    /// Render the next block at engine clock `clock`, advancing the transport, on this thread
    /// alone. The live engine's step, and the offline one's. Never allocates while it succeeds
    /// (RENDER-28).
    pub fn render_block(&mut self, clock: u64) -> Result<&[[f32; 2]; BLOCK]> {
        self.render_block_on(clock, &mut Runner::one())
    }

    /// [`Renderer::render_block`] on `runner`'s threads (8a, RENDER-2): the same bits on any
    /// number. The pre-pass (cells, lanes, the trace, the block's times) runs here; then each
    /// partition runs as one task, after the partitions it reads; then the post-pass (feedback
    /// rings, the output, the meters) runs here. A plan that hosts a plugin runs every task here,
    /// whatever the runner: plugins on worker threads come later. A panic in a task is raised again
    /// here once the block is done, as a panic in a node always was. Never allocates while it
    /// succeeds (RENDER-28).
    pub fn render_block_on(&mut self, clock: u64, runner: &mut Runner) -> Result<&[[f32; 2]; BLOCK]> {
        // One floating-point mode for the block, and the caller's back after it (RENDER-2a).
        let _mode = fpenv::Canon::enter();
        let alone = &mut Runner::one();
        let runner = if self.plan.plugins { alone } else { runner };
        // A fault its build found fails it before its first block (RENDER-1).
        self.faulted()?;
        let b = self.pre(clock, runner);
        let failed = run_graph(&mut self.plan.graph, &mut self.plan.schedule, &self.lanes.slots, &b, runner, &self.plan.report);
        self.post(&b, failed)?;
        self.faulted()?;
        Ok(&self.frames)
    }

    /// Offline, with a ring of its own: the first fault in it, as the error that stops the render
    /// (RENDER-1). Live, the session reads the ring, and the part goes silent alone.
    fn faulted(&self) -> Result<()> {
        if self.checks
            && !self.live
            && let Some(f) = self.plan.report.faults().take()
        {
            return Err(fault::Failed::new(f, None).into());
        }
        Ok(())
    }

    /// A block's pre-pass, on this thread: the transport, cell values, lanes, the trace and the
    /// block's times.
    fn pre(&mut self, clock: u64, runner: &mut Runner) -> Block {
        self.runs = self.transport.advance();
        self.clock = clock + BLOCK as u64;
        // Cell values written since the last block line (RENDER-22, RENDER-28); offline renders
        // never get any.
        self.plan.cells.apply(&mut self.plan.graph);
        // Workers wake while the pre-pass runs.
        runner.wake(self.plan.schedule.tasks.width());
        // Lanes play before the graph, once per block: schedulers fill their events, in slot
        // order, and audio players their block, which every node on the lane copies.
        let runs = self.runs;
        for l in &mut self.lanes.slots {
            match l {
                LaneSlot::Notes(Some(s)) => s.run(&runs, &mut self.next_note),
                LaneSlot::Audio(Some(p)) => {
                    if !self.disk {
                        p.prefetch(&runs);
                    }
                    p.fill(&runs)
                }
                _ => {}
            }
        }
        if self.trace.is_some() {
            self.record(clock);
        }
        // What a plugin late by its offset is told (HOST-6, RENDER-18).
        self.past.push(clock, runs);
        for &i in &self.plan.late_plugins {
            if let Exec::Plugin(p) = &mut self.plan.graph.exec[i] {
                p.told = self.past.told(p.offset, clock, &self.tempo, &self.arrangement, self.sample_rate);
            }
        }
        self.block(clock)
    }

    /// A block's post-pass, on this thread, once the graph ran: feedback rings, the output, the
    /// meters. When a node `failed` (offline only: live, it stopped alone), its error, and nothing
    /// more is written for the block, as step 7's loop did.
    fn post(&mut self, b: &Block, failed: Option<anyhow::Error>) -> Result<()> {
        if let Some(e) = failed {
            return Err(e);
        }
        self.plan.graph.feedback(b, Seg::WHOLE);
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
        // The meters (8b.8): read after the graph ran, so they change no bit.
        if let Some(t) = &self.plan.taps {
            t.write(&self.plan.graph.bufs, &self.frames);
        }
        Ok(())
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
        self.process_on(out, &mut Runner::one())
    }

    /// [`Renderer::process`] on `runner`'s threads: the same bits.
    pub fn process_on(&mut self, out: &mut [[f32; 2]], runner: &mut Runner) -> Result<()> {
        for frame in out.iter_mut() {
            loop {
                if self.pending.is_empty() {
                    self.render_block_on(self.clock, runner)?;
                    self.pending.load(&self.frames, &self.runs, self.transport.pos());
                }
                // Offline, the song's first `L` frames are dropped: they come before song time 0
                // (RENDER-19).
                let f = self.pending.pop().0;
                if self.lead == 0 {
                    *frame = f;
                    break;
                }
                self.lead -= 1;
            }
        }
        self.delivered += out.len() as u64;
        Ok(())
    }

    /// Render `seconds` of audio from the current position.
    pub fn render(&mut self, seconds: f64) -> Result<Vec<[f32; 2]>> {
        self.render_on(seconds, &mut Runner::one())
    }

    /// [`Renderer::render`] on `runner`'s threads: the same bits.
    pub fn render_on(&mut self, seconds: f64, runner: &mut Runner) -> Result<Vec<[f32; 2]>> {
        let mut out = vec![[0.0; 2]; (seconds * self.sample_rate).round() as usize];
        self.process_on(&mut out, runner)?;
        Ok(out)
    }

    /// Back to the state right after building, transport aside.
    fn reset(&mut self) -> Result<()> {
        self.plan.graph.reset()?;
        self.reset_lanes();
        Ok(())
    }

    /// Every lane back to its start.
    fn reset_lanes(&mut self) {
        for l in &mut self.lanes.slots {
            match l {
                LaneSlot::Notes(Some(s)) => s.reset(),
                LaneSlot::Audio(Some(p)) => p.reset(),
                _ => {}
            }
        }
    }

    /// Continue from sample `to`, exactly as if everything before it had been rendered.
    /// Implemented by resetting and replaying from 0 (RENDER-1): offline only.
    pub fn seek(&mut self, to: u64) -> Result<()> {
        self.seek_on(to, &mut Runner::one())
    }

    /// [`Renderer::seek`], replaying on `runner`'s threads: the same bits.
    pub fn seek_on(&mut self, to: u64, runner: &mut Runner) -> Result<()> {
        // The reset runs every kernel and plugin, before any block's mode (RENDER-2a).
        let _mode = fpenv::Canon::enter();
        self.reset()?;
        self.transport = Transport::new(0, true);
        self.clock = 0;
        self.pending.clear();
        self.delivered = 0;
        self.lead = self.latency as u64;
        let mut scratch = vec![[0.0; 2]; 4096];
        while self.delivered < to {
            let n = (to - self.delivered).min(scratch.len() as u64) as usize;
            self.process_on(&mut scratch[..n], runner)?;
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

    /// The tempo and meters it plays in.
    pub fn arrangement(&self) -> &Arrangement {
        &self.arrangement
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
        self.past.take_from(&old.past);
    }

    /// The song's latency `L`, in frames (RENDER-19): its output is that late. Offline renders drop
    /// it from their start; live, the position heard is the transport's `L` frames before.
    pub fn latency(&self) -> u32 {
        self.latency
    }

    /// Its plugin nodes whose latency, measured now, isn't what the plan compensates (RENDER-17):
    /// an offline render keeps its `L`, so these played misaligned from their change on
    /// (`latency.changed`). Off the audio thread: it asks each plugin on the plugin main thread.
    pub fn latency_moved(&self) -> Vec<usize> {
        let info = &self.plan.info;
        info.plugins
            .iter()
            .filter(|at| {
                let Some(inst) = at.inst.upgrade().filter(|i| !i.dead()) else { return false };
                matches!(inst.host().latency(at.handle).recv(), Ok(Ok(f)) if f != at.latency)
            })
            .map(|at| at.node)
            .collect()
    }

    /// The transport position at engine sample `t`, while it is remembered: what was heard at
    /// `t + L` (RENDER-19).
    pub fn position_at(&self, t: u64) -> Option<u64> {
        self.past.position_at(t)
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

    /// Its overlays' values, one per site of its graph in order ([`PlanInfo::overlays`]), set at once
    /// off the audio thread: a renderer not yet sent starts where the plan before it was, with no
    /// glide at entry (MON-1). The window's own overlays take their site's value. Writes no slot,
    /// so what it reports as heard is unchanged.
    pub fn set_overlays(&mut self, values: &[f32]) {
        self.plan.cells.set_overlays(&mut self.plan.graph, values);
        for &(node, site) in &self.plan.window_overlays {
            if let Some(&v) = values.get(site) {
                self.plan.graph.set_overlay(node, v, true);
            }
        }
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

    /// A live note's On or Off, a wheel, or its source's release (MON-7), for lane `slot`: its
    /// consumers get it at the next block's first frame. A release reaches every lane, whatever its
    /// slot. False, and nothing taken, for a slot that isn't one of its notes lanes, a pitch outside
    /// 0 to 127 or a velocity outside 0 to 1 (R2), or bytes that aren't a wheel. Never allocates.
    pub fn live(&mut self, slot: usize, ev: LiveEv) -> bool {
        let (lanes, next) = (&mut self.lanes.slots[..self.lanes.live], &mut self.next_note);
        match ev {
            LiveEv::Release { .. } => {
                for l in lanes {
                    if let LaneSlot::Notes(Some(s)) = l {
                        s.live(ev, next);
                    }
                }
                true
            }
            LiveEv::On { pitch, velocity, .. } if !(0.0..=127.0).contains(&pitch) || !(0.0..=1.0).contains(&velocity) => false,
            LiveEv::Midi { bytes, .. } if !sched::is_wheel(bytes) => false,
            _ => match lanes.get_mut(slot) {
                Some(LaneSlot::Notes(Some(s))) => {
                    s.live(ev, next);
                    true
                }
                _ => false,
            },
        }
    }

    /// Hear `input` on audio lane `slot` (MON-8), or with None stop hearing on every lane: Ok with
    /// what was heard before, to retire; Err with `input` back for a slot that isn't one of its
    /// audio lanes. Only one lane hears at a time. Never allocates.
    #[allow(clippy::type_complexity)]
    pub fn hear(&mut self, slot: usize, input: Option<Arc<dyn LiveInput>>) -> Result<Option<Arc<dyn LiveInput>>, Option<Arc<dyn LiveInput>>> {
        let lanes = &mut self.lanes.slots[..self.lanes.live];
        if input.is_some() && !matches!(lanes.get(slot), Some(LaneSlot::Audio(Some(_)))) {
            return Err(input);
        }
        let mut old = None;
        for l in lanes.iter_mut() {
            if let LaneSlot::Audio(Some(p)) = l
                && p.hearing()
            {
                old = p.hear(None);
            }
        }
        if let Some(LaneSlot::Audio(Some(p))) = lanes.get_mut(slot).filter(|_| input.is_some()) {
            p.hear(input);
        }
        Ok(old)
    }

    /// Whether an audio lane hears a live input (MON-8).
    pub fn hearing(&self) -> bool {
        self.lanes.slots.iter().any(|l| matches!(l, LaneSlot::Audio(Some(p)) if p.hearing()))
    }

    /// Live notes held, across every lane (MON-7).
    pub fn live_notes(&self) -> usize {
        self.lanes.slots.iter().map(|l| if let LaneSlot::Notes(Some(s)) = l { s.live_held() } else { 0 }).sum()
    }

    /// Lanes whose pitch wheel or mod wheel live input left away from rest.
    pub fn live_wheels(&self) -> usize {
        self.lanes.slots.iter().filter(|l| matches!(l, LaneSlot::Notes(Some(s)) if s.wheels_moved())).count()
    }

    /// Lane `slot`'s events in the last block (tests).
    #[cfg(test)]
    pub(crate) fn lane_events(&self, slot: usize) -> &[Event] {
        self.lanes.slots[slot].events()
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
    /// chasing. A node that stopped plays again, reset; one whose reset fails stops alone, marked,
    /// and the rest is reset all the same (RENDER-25): Stop All Sound always works.
    pub fn panic(&mut self) {
        // Plugins are reset here, between blocks (RENDER-2a).
        let _mode = fpenv::Canon::enter();
        self.plan.graph.panic(&self.plan.report);
        self.reset_lanes();
        self.transport.enter();
    }

    /// The engine resumes after a hold (RENDER-1): each plugin node whose instance an export
    /// rendered with starts again, at its next call, the notes the song still holds. Kernels and
    /// voices were frozen, not reset, so they need nothing. Atomic swaps only.
    pub(crate) fn resumed(&mut self) {
        for e in &mut self.plan.graph.exec {
            if let Exec::Plugin(p) = e
                && p.inst.as_ref().is_some_and(|i| i.take_lent())
            {
                p.again = true;
            }
        }
        // An export put a borrowed plugin's wheels at rest: each lane's are sent again.
        for l in &mut self.lanes.slots {
            if let LaneSlot::Notes(Some(s)) = l {
                s.resend_wheels();
            }
        }
    }

    /// Whether the engine may stop processing this renderer while stopped (RENDER-6a): no fade
    /// window runs, no voice sounds, no kernel at the top reports `active`, and every plugin at the
    /// top said it sleeps, or that it may stop once quiet and its output is (a plugin's tail is its
    /// own to report).
    pub fn can_idle(&mut self) -> bool {
        // `k_active` runs a kernel, after the block's mode is gone (RENDER-2a).
        let _mode = fpenv::Canon::enter();
        !self.in_window()
            && self.active_voices() == 0
            && self.live_notes() == 0
            && !self.hearing()
            && self.plugins_sleep()
            && !self.plan.graph.active().unwrap_or(true)
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

    /// The kernel sandboxes it holds (RENDER-32).
    pub fn sandboxes(&self) -> usize {
        self.plan.graph.sandboxes()
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

    /// The faults reported to its ring (RENDER-25): G12 counts each as a crash.
    pub fn stops(&self) -> u64 {
        self.plan.report.faults().count()
    }

    /// The last block's runs.
    pub fn last_runs(&self) -> &Runs {
        &self.runs
    }
}

#[cfg(test)]
mod tests;
