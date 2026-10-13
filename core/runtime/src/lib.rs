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

pub mod audio;
pub mod data;
pub mod engine;
pub mod event;
pub mod packages;
pub mod plugins;
pub mod sched;
pub mod trace;
pub mod transport;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use m2_ir::{FNode, FOp, Flat, Input, Kind, NoteField, Origin, PortRef, TimeOf};
use m2_song::{Arrangement, ControlLane, DataSet, LaneData, TempoMap};
use wasmtime::{Instance, Memory, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};

pub use data::{decls, lane_kind};
pub use engine::{Applied, CallbackStamp, Engine, Envelope, Msg, Retired};
pub use event::{Event, Events};
pub use packages::{Effect, Package, PackageSet};
pub use plugins::{Carry, CarryKey, Fresh, InstanceSource, PluginInstance};
pub use sched::{Caps, LaneSwap, NotesData};
pub use trace::{TraceEntry, TraceKind};
pub use transport::{Cmd, Jump, Loop, MAX_RUNS, Run, Runs, Transport};

use crate::sched::NoteScheduler;
use crate::trace::Trace;

pub const BLOCK: usize = 128;

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

struct PolyNode {
    /// The notes lane it plays.
    slot: usize,
    voices: Vec<VoiceSlot>,
    out: PortRef,
    born: u64,
    /// This block's Ons, by voice: room for two runs of the lane's Ons.
    plan: Vec<Planned>,
}

/// A hosted plugin and what feeds it. It reaches its instance only through the host (P11).
struct PluginNode {
    name: String,
    inst: Arc<PluginInstance>,
    audio_in: Option<Input>,
    /// The notes lane it plays, if any.
    notes: Option<usize>,
    /// The key of each note sounding into it, by note id.
    keys: Vec<(u32, i16)>,
    /// The notes it was given before a reset (a panic), ended at its next call: a plugin may keep
    /// its voices through a reset, and nothing else would end them (RENDER-10, RENDER-26a). As
    /// big as `keys`.
    ended: Vec<(u32, i16)>,
    /// Notes a carried instance held that this node's scheduler didn't adopt: each let go (a
    /// NoteOff, not a choke) at frame 0 of its first call (§5.3). As big as any node that used the
    /// instance could hold.
    release: Vec<(u32, i16)>,
    /// (parameter id, input, value last sent).
    params: Vec<(u32, Input, Option<f32>)>,
    /// One call's events in and out, sized when built (RENDER-28). What comes out (parameter
    /// gestures, HOST-4) is kept for one call: nothing consumes it before plugin windows.
    events: m2_plugins::Events,
    out: m2_plugins::Events,
    /// During an activation's crossfade, the node of the new renderer its instance was carried
    /// into: that node processes the instance, and this one only copies its output.
    lent: Option<usize>,
    /// What its last call reported: whether the engine may idle (RENDER-6a).
    status: m2_plugins::Status,
    /// NoteOffs sent to it since it was built, and the engine sample of the last (gates).
    offs: u64,
    last_off: Option<u64>,
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
    /// Lent to the renderer that replaced this one, for the crossfade.
    pub lent: bool,
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
                Input::Const(v) => {
                    if last.is_none() {
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
        if self.lent.is_some() {
            // Its output was copied in from the node it was carried into (`plugins::lend`).
            return Ok(());
        }
        let notes = self.notes.map_or(&[][..], |s| lanes[s].events());
        let mut next = 0;
        let channels = out.len() / BLOCK;
        let host = self.inst.host();
        let h = self.inst.handle();
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
                bail!("{} reported a processing error", self.name);
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
        if self.lent.is_some() {
            // The node it was carried into resets the instance.
            return Ok(());
        }
        self.inst.host().reset(self.inst.handle());
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

/// A lane the renderer plays: what its nodes read, by slot.
enum LaneSlot {
    Notes(NoteScheduler),
    Control(Arc<ControlLane>),
    Audio(audio::AudioLane),
}

impl LaneSlot {
    /// This block's note events (none for other lanes).
    fn events(&self) -> &[Event] {
        match self {
            LaneSlot::Notes(s) => s.events().as_slice(),
            _ => &[],
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
    Audio(Box<audio::AudioNode>),
    /// A notes lane: its scheduler runs before the graph, once per block.
    Notes,
    Time(TimeOf),
    Const(f32),
    Kernel(Box<KernelNode>),
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
    Capture(PortRef),
}

struct Graph {
    exec: Vec<Exec>,
    /// Where each constant was written, for live value changes.
    origins: Vec<Option<Origin>>,
    order: Vec<usize>,
    /// `bufs[node][port]`.
    bufs: Vec<Vec<Buf>>,
    /// Whole-graph resets since it was built (a voice's graph counts its own).
    resets: u64,
}

/// Builds instances for a graph.
struct Builder<'a> {
    store: &'a mut Store<StoreLimits>,
    packages: &'a PackageSet,
    /// The revision's data: lanes, the tempo, and where assets and verified state blobs are.
    data: &'a DataSet,
    sample_rate: f64,
    slots: &'a [LaneRef],
    lanes: &'a [LaneSlot],
    /// Where top-level plugin instances come from before any is created (HOST-5).
    source: &'a mut dyn InstanceSource,
    /// Who the instances it creates report to.
    owner: &'a m2_plugins::Owner,
    /// Top-level plugin nodes so far, by module, plugin and state path: each one's ordinal.
    ordinals: HashMap<(String, String, Option<Vec<String>>), u32>,
}

impl Builder<'_> {
    fn slot(&self, module: &str, lane: &str) -> Result<usize> {
        self.slots.iter().position(|r| r.module == module && r.lane == lane).with_context(|| format!("lane {module}/{lane} has no slot"))
    }

    fn caps(&self, slot: usize) -> Caps {
        match &self.lanes[slot] {
            LaneSlot::Notes(s) => s.caps(),
            _ => Caps::default(),
        }
    }

    fn graph(&mut self, nodes: &[FNode], order: &[usize], in_voice: bool) -> Result<Graph> {
        let mut exec = Vec::with_capacity(nodes.len());
        let mut bufs: Vec<Vec<Buf>> = Vec::with_capacity(nodes.len());
        let used: HashSet<usize> = order.iter().copied().collect();
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
                    Exec::Control { slot: self.slot(module, lane)?, default: *default as f32 }
                }
                FOp::Lane { kind: Kind::Events(_), .. } => Exec::Notes,
                FOp::Lane { module, lane, kind: Kind::Audio(_), .. } => {
                    Exec::Audio(Box::new(audio::AudioNode::new(self.slot(module, lane)?, self.sample_rate)))
                }
                FOp::Time(t) => Exec::Time(*t),
                FOp::Const(v) => Exec::Const(*v as f32),
                FOp::Kernel { kernel, channels, inputs } => Exec::Kernel(Box::new(self.kernel(n, kernel, *channels, inputs)?)),
                FOp::Plugin { plugin, inputs, .. } if self.packages.plugin_missing(plugin) => {
                    // Typed with `in` first (see `PackageSet::bind_plugins`).
                    Exec::Missing(inputs.first().filter(|i| matches!(i, Input::Port { .. })).cloned())
                }
                FOp::Plugin { plugin, inputs, state, .. } => {
                    Exec::Plugin(Box::new(self.plugin(nodes, n, plugin, inputs, state.as_deref(), in_voice)?))
                }
                FOp::Poly { lane, voices, voice, .. } => {
                    if in_voice {
                        bail!("{}: poly inside poly", n.name);
                    }
                    let FOp::Lane { module, lane: lane_id, .. } = &nodes[*lane].op else { bail!("{}: poly needs a notes lane", n.name) };
                    let slot = self.slot(module, lane_id)?;
                    let mut slots = Vec::with_capacity(*voices as usize);
                    for _ in 0..*voices {
                        let graph = self.graph(&voice.nodes, &voice.order, true).with_context(|| format!("{}: voice", n.name))?;
                        slots.push(VoiceSlot { graph, note: None, born: 0, dirty: false, last_peak: 0.0, taken: false });
                    }
                    let plan = Vec::with_capacity(2 * self.caps(slot).ons);
                    Exec::Poly(Box::new(PolyNode { slot, voices: slots, out: voice.out, born: 0, plan }))
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
        Ok(Graph { exec, origins: nodes.iter().map(|n| n.origin.clone()).collect(), order: order.to_vec(), bufs, resets: 0 })
    }

    /// A plugin node: its instance carried from the source when one matches (top level only), or
    /// created through the host, its verified state loaded, and activated.
    fn plugin(
        &mut self,
        nodes: &[FNode],
        n: &FNode,
        id: &str,
        inputs: &[Input],
        state: Option<&[String]>,
        in_voice: bool,
    ) -> Result<PluginNode> {
        let info = self.packages.plugin_info(id).with_context(|| format!("{}: plugin `{id}` was not described", n.name))?;
        let keys: Vec<&str> = state.unwrap_or_default().iter().map(String::as_str).collect();
        let state_path = state.map(<[String]>::to_vec);
        let ordinal = if in_voice {
            u32::MAX
        } else {
            let o = self.ordinals.entry((n.module.clone(), id.to_string(), state_path.clone())).or_insert(0);
            *o += 1;
            *o - 1
        };
        let key = CarryKey {
            module: n.module.clone(),
            plugin: info.found.id.clone(),
            version: info.found.version.clone(),
            state_path,
            ordinal,
            state: state.and_then(|_| self.data.state_hash(&n.module, &keys)),
        };
        let rate = self.sample_rate as u32;
        // Plugins inside voices aren't carried: a voice resets its instances on every retrigger.
        let carried = if in_voice { None } else { self.source.take(&key, rate) };
        let inst = match carried {
            Some(inst) => inst,
            None => {
                // Only a blob whose sidecar names this plugin is loaded (FMT-17); without one the
                // plugin plays its defaults, and the data's diagnostics say why.
                let blob = match state.and_then(|_| self.data.state(&n.module, &keys)) {
                    Some(path) => {
                        let bytes = std::fs::read(&path).with_context(|| format!("{}: reading plugin state {}", n.name, path.display()))?;
                        Some(Arc::new(m2_plugins::Blob::new(bytes)))
                    }
                    None => None,
                };
                let host = self.packages.host();
                let (h, _) = host.create(&info.found, blob, self.owner).with_context(|| n.name.clone())?;
                // Owned from here: dropped on an error, it is destroyed.
                let inst = Arc::new(PluginInstance::new(host.clone(), h, key, rate));
                host.activate(h, rate, BLOCK as u32).with_context(|| n.name.clone())?;
                inst
            }
        };
        // Inputs come in the order of the plugin's ports; a parameter is bound by the id in its
        // address, never by its place in the list (FMT-18).
        let sig = m2_ir::Kernels::plugin(self.packages, id).with_context(|| format!("{}: plugin `{id}` was not described", n.name))?;
        let (mut audio_in, mut notes, mut params) = (None, None, vec![]);
        for (port, input) in sig.inputs.iter().zip(inputs) {
            match (port.name.as_str(), input) {
                ("in", _) => audio_in = Some(input.clone()),
                ("notes", Input::Port { src, .. }) => {
                    if let FOp::Lane { module, lane, .. } = &nodes[src.node].op {
                        notes = Some(self.slot(module, lane)?);
                    }
                }
                (_, Input::Unset) => {}
                (name, _) => {
                    let (_, pid) = packages::address(name).with_context(|| format!("{}: `{name}` isn't a parameter address", n.name))?;
                    params.push((pid, input.clone(), None));
                }
            }
        }
        // Room for one run's notes, a Tuning for each On, every parameter sampled, a choke and a
        // NoteOff for every note it held when it was reset, and a NoteOff for every note a carried
        // instance held that this node lets go.
        let caps = notes.map_or(Caps::default(), |s| self.caps(s));
        let held = caps.sounding + 2 * caps.ons + 8;
        let release = inst.hold(held);
        let events = caps.events() + caps.ons + (BLOCK / PARAM_STEP) * params.len() + 8 + 2 * held + release;
        Ok(PluginNode {
            name: n.name.clone(),
            inst,
            audio_in,
            notes,
            keys: Vec::with_capacity(held),
            ended: Vec::with_capacity(held),
            release: Vec::with_capacity(release),
            params,
            events: m2_plugins::Events::with_capacity(events),
            out: m2_plugins::Events::with_capacity(OUT_EVENTS),
            lent: None,
            status: m2_plugins::Status::Continue,
            offs: 0,
            last_off: None,
        })
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
        note: Option<VoiceNote>,
        lanes: &[LaneSlot],
    ) -> Result<()> {
        let n = seg.n;
        for oi in 0..self.order.len() {
            let i = self.order[oi];
            let mut out = std::mem::take(&mut self.bufs[i]);
            let bufs = &self.bufs;
            match &mut self.exec[i] {
                Exec::Control { slot, default } => {
                    let LaneSlot::Control(lane) = &lanes[*slot] else { unreachable!("a control lane's slot holds control data") };
                    let o = &mut out[0][seg.range(0)];
                    if lane.is_empty() {
                        o.fill(*default);
                    } else {
                        for (x, &beat) in o.iter_mut().zip(&b.beats64[seg.off..]) {
                            *x = lane.value_at(beat, *default as f64) as f32;
                        }
                    }
                }
                Exec::Audio(a) => {
                    debug_assert_eq!((seg.off, seg.n), (0, BLOCK), "audio lanes only play at the top level");
                    let LaneSlot::Audio(lane) = &lanes[a.slot] else { unreachable!("an audio lane's slot holds audio") };
                    a.process(lane, &b.runs, &mut out[0]);
                }
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
                                bail!("{} produced {} at {:.3}s", k.name, dst[at], b.time(seg.off + at));
                            }
                        }
                    }
                }
                Exec::Poly(p) => p.process(store, b, bufs, &mut out[0], lanes)?,
                Exec::Plugin(p) => p.process(b, bufs, &mut out[0], lanes)?,
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
                            let at = (b.engine as usize + seg.off + f) % BLOCK;
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
                            // Open from the note's On until its Off, on the engine clock (RENDER-13).
                            for (k, x) in o.iter_mut().enumerate() {
                                *x = ((b.engine + (seg.off + k) as u64) < note.off) as u8 as f32;
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
                        let at = (b.engine as usize + seg.off + f) % BLOCK;
                        ring[c * BLOCK + at] = src[sc * BLOCK + seg.off + f];
                    }
                }
            }
        }
        Ok(())
    }

    /// Back to the state right after building.
    fn reset(&mut self, store: &mut Store<StoreLimits>) -> Result<()> {
        // Counted here, so no path resets the graph unseen (RENDER-10).
        self.resets += 1;
        for e in &mut self.exec {
            match e {
                Exec::Kernel(k) => k.reset.call(&mut *store, ())?,
                Exec::Plugin(p) => p.reset()?,
                Exec::Feedback { ring, .. } => ring.fill(0.0),
                Exec::Audio(a) => a.reset(),
                Exec::Poly(p) => {
                    p.born = 0;
                    p.plan.clear();
                    for v in &mut p.voices {
                        if v.dirty {
                            v.graph.reset(store)?;
                        }
                        (v.note, v.born, v.dirty, v.last_peak, v.taken) = (None, 0, false, 0.0, false);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Reset every kernel once, in voices too: a kernel's first reset may grow its memory
    /// (it builds the new state before dropping the old), so it happens here, off the audio
    /// thread, and never again later. A reset is the same as init, so nothing sounds different.
    fn prewarm(&mut self, store: &mut Store<StoreLimits>) -> Result<()> {
        for e in &mut self.exec {
            match e {
                Exec::Kernel(k) => k.reset.call(&mut *store, ())?,
                Exec::Poly(p) => {
                    for v in &mut p.voices {
                        v.graph.prewarm(store)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The memory every kernel instance holds, in bytes.
    fn kernel_memory(&self, store: &Store<StoreLimits>) -> usize {
        self.exec
            .iter()
            .map(|e| match e {
                Exec::Kernel(k) => k.memory.data_size(store),
                Exec::Poly(p) => p.voices.iter().map(|v| v.graph.kernel_memory(store)).sum(),
                _ => 0,
            })
            .sum()
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
    /// One block. The lane's events say which notes start and end; voices are given notes in
    /// event order, and a voice's graph is split where its note changes. Never allocates.
    fn process(&mut self, store: &mut Store<StoreLimits>, b: &Block, outer: &[Vec<Buf>], out: &mut Buf, lanes: &[LaneSlot]) -> Result<()> {
        let (start, end) = (b.engine, b.engine + BLOCK as u64);
        let events = lanes[self.slot].events();
        let PolyNode { voices, out: voice_out, born, plan, .. } = self;
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
                && !v.graph.active(store)?
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
        for (vi, v) in voices.iter_mut().enumerate() {
            let taken = std::mem::take(&mut v.taken);
            if v.note.is_none() && !taken {
                continue;
            }
            // Split the block wherever this voice starts a new note.
            let mut at = 0;
            let mut current = v.note;
            let mut peak = 0.0f32;
            let mut segment =
                |v: &mut VoiceSlot, store: &mut Store<StoreLimits>, seg: Seg, note: VoiceNote, peak: &mut f32| -> Result<()> {
                    v.graph.process(store, b, seg, Some(outer), Some(note), &[])?;
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
                    segment(v, store, Seg { off: at, n: off - at }, c, &mut peak)?;
                }
                if v.dirty {
                    v.graph.reset(store)?;
                }
                v.dirty = true;
                v.note = Some(p.note);
                peak = 0.0;
                current = Some(p.note);
                at = off;
            }
            if let Some(c) = current {
                segment(v, store, Seg { off: at, n: BLOCK - at }, c, &mut peak)?;
            }
            if let Some(n) = v.note {
                if n.off < end {
                    if peak < QUIET && !v.graph.active(store)? {
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

/// Renders a checked song.
pub struct Renderer {
    store: Store<StoreLimits>,
    graph: Graph,
    lanes: Vec<LaneSlot>,
    slots: Vec<LaneRef>,
    out: PortRef,
    tempo: TempoMap,
    arrangement: Arc<Arrangement>,
    sample_rate: f64,
    transport: Transport,
    /// The engine clock of the next block.
    clock: u64,
    /// The last block, and its transport.
    frames: [[f32; 2]; BLOCK],
    runs: Runs,
    pending: Pending,
    delivered: u64,
    strict: bool,
    non_finite: u64,
    generation: u64,
    plugins: bool,
    /// A kernel crashed live: it renders silence from then on.
    broken: bool,
    trace: Option<Trace>,
    dropped: u32,
}

impl Renderer {
    /// A renderer for a checked graph and the data it plays: a revision's [`DataSet`], which
    /// also says where its assets and verified plugin states are ([`DataSet::empty`] for none).
    /// It plays from sample 0, offline.
    pub fn new(flat: &Flat, data: &DataSet, packages: &PackageSet, sample_rate: u32) -> Result<Renderer> {
        Renderer::build_with(flat, data, packages, sample_rate, &mut audio::Cache::default())
    }

    /// [`Renderer::new`], sharing decoded assets and clip renders with earlier builds through
    /// `cache` (a live session's). Its plugin instances are all new, reporting to no one.
    pub fn build_with(flat: &Flat, data: &DataSet, packages: &PackageSet, sample_rate: u32, cache: &mut audio::Cache) -> Result<Renderer> {
        Renderer::build_live(flat, data, packages, sample_rate, cache, &m2_plugins::Owner::discard(), &mut Fresh)
    }

    /// [`Renderer::build_with`] for the live engine: top-level plugin instances come from `source`
    /// when one matches (HOST-5, a [`Carry`] of the renderer it will replace), and the rest are
    /// created through the host, reporting to `owner`. Instances are created and activated on the
    /// plugin main thread, while the old renderer plays (RENDER-25).
    pub fn build_live(
        flat: &Flat,
        data: &DataSet,
        packages: &PackageSet,
        sample_rate: u32,
        cache: &mut audio::Cache,
        owner: &m2_plugins::Owner,
        source: &mut dyn InstanceSource,
    ) -> Result<Renderer> {
        let limits =
            StoreLimitsBuilder::new().memory_size(KERNEL_MEMORY).instances(1_000_000).memories(1_000_000).tables(1_000_000).build();
        let mut store = Store::new(&packages.engine, limits);
        store.limiter(|l| l);
        let tempo = data.tempo().clone();
        let rate = sample_rate as f64;
        let slots = lane_slots(flat);
        let mut lanes = Vec::with_capacity(slots.len());
        for s in &slots {
            lanes.push(match s.kind {
                Kind::Events(_) => LaneSlot::Notes(NoteScheduler::new(Arc::new(notes_data(data, &s.module, &s.lane, rate)))),
                Kind::Control(_) => LaneSlot::Control(Arc::new(data.control(&s.module, &s.lane))),
                Kind::Audio(ch) => {
                    LaneSlot::Audio(audio::AudioLane::build(data.audio(&s.module, &s.lane), data, cache, &tempo, rate, ch as usize))
                }
            });
        }
        let mut graph = Builder {
            store: &mut store,
            packages,
            data,
            sample_rate: rate,
            slots: &slots,
            lanes: &lanes,
            source,
            owner,
            ordinals: HashMap::new(),
        }
        .graph(&flat.nodes, &flat.order, false)?;
        graph.prewarm(&mut store)?;
        let plugins = graph.exec.iter().any(|e| matches!(e, Exec::Plugin(_)));
        Ok(Renderer {
            store,
            graph,
            lanes,
            slots,
            out: flat.out,
            tempo,
            arrangement: data.arrangement.clone(),
            sample_rate: rate,
            transport: Transport::new(0, true),
            clock: 0,
            frames: [[0.0; 2]; BLOCK],
            runs: Runs::one(Run { off: 0, n: BLOCK as u32, at: 0, moving: true, jump: Jump::None }),
            pending: Pending::EMPTY,
            delivered: 0,
            strict: true,
            non_finite: 0,
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            plugins,
            broken: false,
            trace: None,
            dropped: 0,
        })
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
        if self.plugins {
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
    /// engine's step, and the offline one's. `release_all`: this renderer is being replaced,
    /// so every note sounding into it ends now (RENDER-26a). Never allocates while it
    /// succeeds (RENDER-28).
    pub fn render_block(&mut self, clock: u64, release_all: bool) -> Result<&[[f32; 2]; BLOCK]> {
        self.render_block_lent(clock, release_all, None)
    }

    /// [`Self::render_block`] for a renderer fading out after an activation that carried some of
    /// its plugin instances into `current`, which rendered this block first: each lent node takes
    /// the output its instance made there, so an instance is processed once per block (§5.3).
    pub fn render_block_lent(&mut self, clock: u64, release_all: bool, current: Option<&Renderer>) -> Result<&[[f32; 2]; BLOCK]> {
        self.runs = self.transport.advance();
        self.clock = clock + BLOCK as u64;
        if self.broken {
            self.frames = [[0.0; 2]; BLOCK];
            return Ok(&self.frames);
        }
        let runs = self.runs;
        for l in &mut self.lanes {
            if let LaneSlot::Notes(s) = l {
                s.run(&runs, release_all);
            }
        }
        if self.trace.is_some() {
            self.record(clock);
        }
        let b = self.block(clock);
        if let Some(c) = current {
            plugins::lend(&mut self.graph, &c.graph);
        }
        if let Err(e) = self.graph.process(&mut self.store, &b, Seg::WHOLE, None, None, &self.lanes) {
            self.broken = !self.strict;
            return Err(e);
        }
        let o = &self.graph.bufs[self.out.node][self.out.port];
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
        for (slot, l) in self.lanes.iter().enumerate() {
            let LaneSlot::Notes(s) = l else { continue };
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
                self.render_block(self.clock, false)?;
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
        self.graph.reset(&mut self.store)?;
        for l in &mut self.lanes {
            if let LaneSlot::Notes(s) = l {
                s.reset();
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

    /// Take over from a renderer whose transport is `from`, in its tempo map and rate: the same
    /// beat, play state and loop (RENDER-25a, no jump back), and a first block that seeks
    /// without chasing.
    pub fn enter_from(&mut self, from: &Transport, tempo: &TempoMap, rate: f64) {
        let mut t = *from;
        t.retime(tempo, rate, &self.tempo, self.sample_rate);
        t.enter();
        self.transport = t;
    }

    /// The lanes it plays, by slot.
    pub fn lane_slots(&self) -> &[LaneRef] {
        &self.slots
    }

    /// The room a notes lane's scheduler has, by slot.
    pub fn caps(&self, slot: usize) -> Option<Caps> {
        match self.lanes.get(slot)? {
            LaneSlot::Notes(s) => Some(s.caps()),
            _ => None,
        }
    }

    /// Replace one lane's data at the next block line, touching no kernel (RENDER-26). Returns
    /// the data replaced, to retire, or `data` itself when it doesn't fit the slot (another
    /// kind, or more notes than its scheduler holds). Never allocates.
    pub fn swap_lane(&mut self, slot: usize, data: LaneSwap) -> Result<LaneSwap, LaneSwap> {
        let swapped = match (self.lanes.get_mut(slot), data) {
            (Some(LaneSlot::Notes(s)), LaneSwap::Notes(d)) => s.swap(d).map(LaneSwap::Notes).map_err(LaneSwap::Notes),
            (Some(LaneSlot::Control(c)), LaneSwap::Control(d)) => Ok(LaneSwap::Control(std::mem::replace(c, d))),
            (_, d) => Err(d),
        };
        if swapped.is_ok()
            && let Some(t) = &mut self.trace
        {
            t.push(TraceEntry { engine: self.clock, transport: self.transport.pos(), slot: slot as u32, id: 0, kind: TraceKind::Swap });
        }
        swapped
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

    /// Notes still held: voices whose gate is open, and notes sounding into plugins. After a
    /// stop or a release this is 0, or a note is stuck (RENDER-26a).
    pub fn held_notes(&self) -> usize {
        let clock = self.clock;
        self.graph
            .exec
            .iter()
            .map(|e| match e {
                Exec::Poly(p) => p.voices.iter().filter(|v| v.note.is_some_and(|n| n.off >= clock)).count(),
                // A lent node's notes went with its instance.
                Exec::Plugin(p) if p.lent.is_none() => p.keys.len() + p.release.len(),
                _ => 0,
            })
            .sum()
    }

    /// Notes the schedulers hold: turned on and not yet off.
    pub fn sounding_notes(&self) -> usize {
        self.lanes.iter().map(|l| if let LaneSlot::Notes(s) = l { s.held() } else { 0 }).sum()
    }

    /// Whether every voice with an open gate plays a note its scheduler holds: a gated voice
    /// nobody will turn off is a stuck note.
    pub fn gates_are_owned(&self) -> bool {
        let clock = self.clock;
        self.graph.exec.iter().all(|e| match e {
            Exec::Poly(p) => p.voices.iter().filter_map(|v| v.note.filter(|n| n.off >= clock)).all(|n| match &self.lanes[p.slot] {
                LaneSlot::Notes(s) => s.is_sounding(n.id),
                _ => false,
            }),
            _ => true,
        })
    }

    /// Panic (RENDER-10): every note off, every instance reset. The next block seeks without
    /// chasing.
    pub fn panic(&mut self) -> Result<()> {
        self.reset()?;
        self.transport.enter();
        Ok(())
    }

    /// Whether the engine may stop processing this renderer while stopped (RENDER-6a): no voice
    /// sounds, no kernel at the top reports `active`, and every plugin at the top said it sleeps,
    /// or that it may stop once quiet and its output is (a plugin's tail is its own to report).
    pub fn can_idle(&mut self) -> bool {
        self.active_voices() == 0 && self.plugins_sleep() && !self.graph.active(&mut self.store).unwrap_or(true)
    }

    fn plugins_sleep(&self) -> bool {
        use m2_plugins::Status as S;
        self.graph.exec.iter().enumerate().all(|(i, e)| match e {
            Exec::Plugin(p) => match p.status {
                S::Sleep => true,
                S::ContinueIfNotQuiet => self.graph.bufs[i].first().is_none_or(|b| b.iter().all(|x| x.abs() < QUIET)),
                _ => false,
            },
            _ => true,
        })
    }

    /// While the engine idles: whether a plugin that slept asked to be processed (CLAP's
    /// `request_process`), which wakes it within one block. Atomic swaps only.
    pub fn woken(&self) -> bool {
        self.graph.exec.iter().any(|e| match e {
            Exec::Plugin(p) if p.status == m2_plugins::Status::Sleep => p.inst.host().take_process_request(p.inst.handle()),
            _ => false,
        })
    }

    pub fn hosts_plugins(&self) -> bool {
        self.plugins
    }

    /// Take over the plugin instances this renderer shares with `old`, which it replaces now: the
    /// notes sounding into them go on, and `old` lends them for the crossfade (`plugins`,
    /// RENDER-25). The engine calls it at activation. Never allocates.
    pub fn carry_from(&mut self, old: &mut Renderer) {
        plugins::carry_from(self, old);
    }

    /// The instances of its top-level plugin nodes, in node order (gates, tests; off the audio
    /// thread).
    pub fn plugin_handles(&self) -> Vec<m2_plugins::Handle> {
        self.graph.exec.iter().filter_map(|e| if let Exec::Plugin(p) = e { Some(p.inst.handle()) } else { None }).collect()
    }

    /// Its top-level plugin nodes, in node order: the notes each holds, what it was sent and what
    /// it said (gates, tests; off the audio thread).
    pub fn plugins(&self) -> Vec<PluginView> {
        self.graph
            .exec
            .iter()
            .filter_map(|e| match e {
                Exec::Plugin(p) => Some(PluginView {
                    handle: p.inst.handle(),
                    notes: p.keys.clone(),
                    offs: p.offs,
                    last_off: p.last_off,
                    lent: p.lent.is_some(),
                    status: p.status,
                }),
                _ => None,
            })
            .collect()
    }

    /// Events plugins output in their last calls, and those that didn't fit (HOST-4: collected,
    /// not yet consumed).
    pub fn plugin_out_events(&self) -> (usize, u32) {
        self.graph.exec.iter().fold((0, 0), |(n, d), e| match e {
            Exec::Plugin(p) => (n + p.out.len(), d + p.out.dropped()),
            _ => (n, d),
        })
    }

    /// The memory every kernel instance holds: unchanged by playing, or a reset grew it on the
    /// audio thread. Off the audio thread only.
    pub fn kernel_memory_bytes(&self) -> usize {
        self.graph.kernel_memory(&self.store)
    }

    /// Unique to this renderer.
    pub fn generation(&self) -> u64 {
        self.generation
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
        self.lanes.iter().map(|l| if let LaneSlot::Notes(s) = l { s.events().dropped_ons() } else { 0 }).sum()
    }

    /// Audio lane fades let go before they ended for want of room: never at a project rate.
    pub fn dropped_taps(&self) -> u32 {
        self.graph.exec.iter().map(|e| if let Exec::Audio(a) = e { a.dropped_taps() } else { 0 }).sum()
    }

    /// Times every kernel was reset since it was built: a panic, and offline each [`Self::seek`].
    /// A jump, a swap or an activation never moves it (RENDER-10).
    pub fn resets(&self) -> u64 {
        self.graph.resets
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
