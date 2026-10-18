//! What a renderer plays, split as the contract's engine shape splits it (step 7): a [`Plan`], the
//! structure built from the checked graph with the instances its nodes hold, and the [`Lanes`] its
//! nodes read, built from the revision's data.
//!
//! The arena is the boxes in the playing plan's stateful nodes: a kernel instance with its own
//! `Store`, a `poly`'s whole voice pool, a plugin instance, a feedback ring. An activation moves
//! them into the next plan by a move list computed off the audio thread ([`crate::keep`]), at a
//! block line, each by a swap; lanes move the same way. So nothing is allocated, freed or rewired on
//! the audio thread, and whatever a move displaces leaves with the plan before (RENDER-25,
//! RENDER-28). Every move is verified before any is made: an activation applies whole or not at
//! all.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Weak};

use m2_ir::{FNode, FOp, Flat, Kind, PortRef, Voice};
use m2_song::{DataSet, LaneData, Sha, TempoMap};

use crate::audio::{self, AudioLane, AudioPlayer};
use crate::cells::{CellId, CellMap, CellSlots, Cells};
use crate::event::Event;
use crate::sched::{Caps, LaneSwap, NoteScheduler};
use crate::stopped::{Stopped, Why};
use crate::taps::{TapSlots, Taps};
use crate::{Exec, Graph, LaneRef, PackageSet, PluginInstance, Renderer, fpenv, lane_slots, notes_data};

/// What a box must be to move into a node (RENDER-25): an instance is kept only for a node of the
/// same shape. Compared whole when a plan is prepared; its [`Shape::hash64`] is stored in the box,
/// and the audio thread compares that before it moves anything.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    /// The kernel, the SHA-256 of its package's wasm module (which fixes its state ABI), and its
    /// channel count, its one setting in ABI v0.
    Kernel {
        id: String,
        wasm: Sha,
        channels: u8,
    },
    /// The plugin, its version and its channels. Not its state: a new committed blob is loaded
    /// into the instance (RENDER-26b), and a state path is the same for every pair the path pass
    /// makes.
    Plugin {
        id: String,
        version: String,
        channels: u8,
    },
    /// Its voices and channels, and its voice graph's structure with the cells' values, origins and
    /// names left out, hashed with the wasm modules its kernels run (RENDER-16).
    Poly {
        voices: u32,
        channels: u8,
        voice: Sha,
    },
    Feedback {
        channels: u8,
    },
    /// The planner's delay (RENDER-18): its length and channels, and whether it is a control
    /// signal's. Kept by its consumer, input and length ([`crate::latency::Delay`]).
    Delay {
        channels: u8,
        frames: u32,
        control: bool,
    },
    /// A notes lane's delay: its length and its room, per block, from its lane's.
    NoteDelay {
        frames: u32,
        room: usize,
    },
}

impl Shape {
    /// The shape of the box node `n` holds; None for a node that holds none.
    pub(crate) fn of(n: &FNode, packages: &PackageSet) -> Option<Shape> {
        match &n.op {
            FOp::Kernel { kernel, channels, .. } => {
                Some(Shape::Kernel { id: kernel.clone(), wasm: packages.module_sha(kernel)?, channels: *channels })
            }
            FOp::Plugin { plugin, .. } if packages.plugin_missing(plugin) => None,
            FOp::Plugin { plugin, channels, .. } => {
                let info = packages.plugin_info(plugin)?;
                Some(Shape::Plugin { id: info.found.id.clone(), version: info.found.version.clone(), channels: *channels })
            }
            FOp::Poly { voices, voice, channels, .. } => {
                Some(Shape::Poly { voices: *voices, channels: *channels, voice: blank(voice, packages) })
            }
            FOp::Feedback { channels, .. } => Some(Shape::Feedback { channels: *channels }),
            FOp::Delay { frames, .. } => {
                let kind = n.outputs.first().copied().unwrap_or(Kind::Control(None));
                Some(Shape::Delay { channels: kind.channels(), frames: *frames, control: matches!(kind, Kind::Control(_)) })
            }
            // Its room is its lane's: the plan sets it ([`Shape::with_room`]).
            FOp::NoteDelay { frames, .. } => Some(Shape::NoteDelay { frames: *frames, room: 0 }),
            _ => None,
        }
    }

    /// A note delay's shape, with the room its lane gives it.
    pub(crate) fn with_room(self, per_block: usize) -> Shape {
        match self {
            Shape::NoteDelay { frames, .. } => Shape::NoteDelay { frames, room: per_block },
            s => s,
        }
    }

    pub fn hash64(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish()
    }
}

/// A voice graph's structure: its nodes with every cell's value, every origin, source and name,
/// and every capture's target cleared (captures are the plan's, `PolyNode::captures`), with the
/// wasm modules its kernels run. A plain number written inside the voice stays: it is in the
/// kernels' memory, so editing it replaces the instrument; a declared control is a cell, carried.
fn blank(voice: &Voice, packages: &PackageSet) -> Sha {
    let mut v = voice.clone();
    let mut wasm = vec![];
    for n in &mut v.nodes {
        match &mut n.op {
            FOp::Const(x) if n.origin.is_some() => *x = 0.0,
            FOp::Capture(p) => *p = PortRef { node: 0, port: 0 },
            FOp::Kernel { kernel, .. } => wasm.extend(packages.module_sha(kernel)),
            _ => {}
        }
        (n.name, n.src, n.origin) = (String::new(), None, None);
    }
    wasm.sort_unstable();
    wasm.dedup();
    Sha::of(format!("{v:?} {wasm:?}").as_bytes())
}

/// For each top-level node of a new graph, the node of the graph that plays that it continues
/// (ID-15): what an activation may keep. Made by the session (`m2_session::tx::continues`), from
/// the names of what changed between the two revisions; shapes are the runtime's to check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Continues {
    pub top: Vec<Option<u32>>,
}

impl Continues {
    /// The same code: node i continues node i.
    pub fn identity(flat: &Flat) -> Continues {
        Continues { top: (0..flat.nodes.len() as u32).map(Some).collect() }
    }

    /// Nothing continues: every instance new, the whole old graph a ghost for one window (the
    /// gates' negative reference).
    pub fn nothing(flat: &Flat) -> Continues {
        Continues { top: vec![None; flat.nodes.len()] }
    }

    /// By the checked nodes' names (`<module>/<node id>`): for graphs written as IR, whose ids are
    /// stable, as tests and tools write them. Code's ids aren't, so the session names its nodes.
    pub fn named(old: &Flat, new: &Flat) -> Continues {
        let at: std::collections::HashMap<&str, u32> = old.nodes.iter().enumerate().map(|(i, n)| (n.name.as_str(), i as u32)).collect();
        Continues { top: new.nodes.iter().map(|n| at.get(n.name.as_str()).copied()).collect() }
    }
}

/// What a renderer's plugins, lanes and cells are, for the next plan prepared against it and for
/// the server: never read on the audio thread, and fixed once the renderer is prepared.
pub struct PlanInfo {
    pub id: u64,
    pub rate: u32,
    /// By top-level node: the shape of the box it holds.
    pub shapes: Vec<Option<Shape>>,
    /// Its top-level plugin nodes, with their instances.
    pub plugins: Vec<PluginAt>,
    /// Its lanes by slot, as `SwapLane` addresses them.
    pub lanes: Vec<LaneInfo>,
    pub cell_slots: Arc<CellSlots>,
    pub cell_map: Arc<CellMap>,
    /// Its overlays' cells, one per site of its graph in order (`Flat.overlays`): the track whose
    /// audio it carries, the module that reads it, and the cell (MON-1, RENDER-23). Their ids come
    /// after every value cell's.
    pub overlays: Vec<(String, String, CellId)>,
    /// What its meters read (8b.8): the song's output and each track, after its mute. Only a live
    /// plan has them.
    pub taps: Option<Arc<TapSlots>>,
    /// Its nodes that stopped while it played (RENDER-25), for the session to tell.
    pub stopped: Arc<Stopped>,
    /// Its latencies (RENDER-17 to RENDER-20): each checked node's offset, the delays, the song's
    /// latency `L`, and the nodes that make a feedback loop longer (`latency.in-feedback`).
    pub latency: crate::latency::Latency,
    /// The graph it plays when it has delays: the checked one with them (RENDER-18). The next
    /// plan is wired against it.
    pub played: Option<Arc<Flat>>,
}

/// One plugin node of a plan, as the next plan prepared against it, and an export, see it.
#[derive(Clone, Debug)]
pub struct PluginAt {
    pub node: usize,
    pub handle: m2_plugins::Handle,
    /// The instance, while a plan holds it: what an export borrows (RENDER-1).
    pub inst: Weak<PluginInstance>,
    /// The hash of the committed state blob it was last given: the one it was created from or
    /// last loaded, None for its defaults. The next plan loads another (RENDER-26b). A blob it
    /// refused counts too: it isn't tried again until another is committed.
    pub blob: Option<Sha>,
    /// The latency the plan compensates for it (RENDER-17): what its instance had then.
    pub latency: u32,
}

/// One lane a plan plays.
pub struct LaneInfo {
    pub lane: LaneRef,
    /// A notes lane's room: data that needs more takes an activation, which grows it.
    pub caps: Option<Caps>,
    /// An audio lane's renders.
    audio: Option<Arc<AudioLane>>,
}

impl PlanInfo {
    /// Top-level node `node`'s plugin, if it hosts one.
    pub fn plugin(&self, node: usize) -> Option<&PluginAt> {
        self.plugins.iter().find(|p| p.node == node)
    }

    /// The instance of top-level node `node`, if it hosts one.
    pub fn handle(&self, node: usize) -> Option<m2_plugins::Handle> {
        self.plugin(node).map(|p| p.handle)
    }

    /// The live instances an offline render of `flat` and `data` may share, by node (RENDER-1):
    /// each installed plugin node that continues one of this plan's (`cont`) with the same shape,
    /// each instance once. Only one that holds the committed blob `data` gives its node: none
    /// whose blob changed since this plan was prepared (a build that failed since), or is gone
    /// (CLAP has no "load defaults"), so an export never changes what an instance holds; none
    /// that holds a parameter value the code released (HOME-5a); none that failed. Each of those
    /// nodes plays a new instance from the committed blob in the render, as `m2 render`'s do.
    pub fn lend(&self, flat: &Flat, data: &DataSet, packages: &PackageSet, cont: &Continues) -> HashMap<usize, Arc<PluginInstance>> {
        let mut lent: HashMap<usize, Arc<PluginInstance>> = HashMap::new();
        for &i in &flat.order {
            let n = &flat.nodes[i];
            let Some(shape @ Shape::Plugin { .. }) = Shape::of(n, packages) else { continue };
            let Some(j) = cont.top.get(i).copied().flatten().map(|j| j as usize) else { continue };
            let Some(at) = self.plugin(j).filter(|_| self.shapes.get(j) == Some(&Some(shape))) else { continue };
            if blob(n, data) != at.blob {
                continue;
            }
            if let Some(inst) = at.inst.upgrade().filter(|i| i.lendable())
                && !lent.values().any(|x| Arc::ptr_eq(x, &inst))
            {
                lent.insert(i, inst);
            }
        }
        lent
    }
}

/// The hash of the verified state blob node `n` loads, if it is a plugin with one.
pub(crate) fn blob(n: &FNode, data: &DataSet) -> Option<Sha> {
    let FOp::Plugin { state: Some(path), .. } = &n.op else { return None };
    data.state_hash(&n.module, &path.iter().map(String::as_str).collect::<Vec<_>>())
}

/// What a plan is prepared against.
pub enum Basis<'a> {
    /// Every instance new: an offline render, a new engine, a rate change.
    Fresh,
    /// Move whatever continues from the plan the engine will be playing when this one arrives.
    Keep(Keep<'a>),
    /// An offline render that shares these live plugin instances, by node ([`PlanInfo::lend`]),
    /// each reset at its first call; every other instance new (RENDER-1). The live engine must
    /// be held while it renders.
    Lend(&'a HashMap<usize, Arc<PluginInstance>>),
}

/// The plan that plays, as a new one is prepared against it.
pub struct Keep<'a> {
    /// The graph it was built from, and its data as it plays (lane swaps included).
    pub old: &'a Flat,
    pub old_data: &'a DataSet,
    pub info: &'a PlanInfo,
    pub cont: &'a Continues,
    /// Plugin instances that asked to be restarted: never kept (HOST-5).
    pub restarts: &'a std::collections::HashSet<m2_plugins::Handle>,
}

/// One box moved at entry: from the old plan's top-level node to this plan's exec node.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Move {
    pub(crate) from: u32,
    pub(crate) to: u32,
    pub(crate) shape: u64,
}

/// One lane moved at entry, with what it plays from then on when that changes.
pub(crate) struct LaneMove {
    pub(crate) from: u32,
    pub(crate) to: u32,
    pub(crate) swap: Option<LaneSwap>,
    /// A scheduler sized for new notes the moved one has no room for.
    pub(crate) grow: Option<NoteScheduler>,
}

/// What a plan's activation does, all of it sized off the audio thread.
pub(crate) struct Entry {
    /// The plan it was prepared against.
    pub(crate) after: u64,
    /// Kept nodes, then ghosts.
    pub(crate) moves: Vec<Move>,
    pub(crate) lanes: Vec<LaneMove>,
    /// Top-level cells that continue, (new, old).
    pub(crate) cells: Vec<(CellId, CellId)>,
    /// The cells inside kept polys.
    pub(crate) voice_cells: Vec<CellId>,
    /// Exec nodes whose notes end at entry: ghosts, and kept consumers rewired to another lane.
    pub(crate) release: Vec<u32>,
}

/// Instances by kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    pub kernels: usize,
    pub polys: usize,
    pub plugins: usize,
    pub rings: usize,
}

impl Count {
    pub fn total(&self) -> usize {
        self.kernels + self.polys + self.plugins + self.rings
    }

    pub(crate) fn add(&mut self, s: &Shape) {
        match s {
            Shape::Kernel { .. } => self.kernels += 1,
            Shape::Poly { .. } => self.polys += 1,
            Shape::Plugin { .. } => self.plugins += 1,
            // A delay's line is a ring too.
            Shape::Feedback { .. } | Shape::Delay { .. } | Shape::NoteDelay { .. } => self.rings += 1,
        }
    }
}

/// What a plan's activation kept and made (gates, tests).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub kept: Count,
    pub fresh: Count,
    pub ghosts: usize,
    pub fades: usize,
    /// Moved boxes reset at entry: their output went non-finite, or they trapped.
    pub resets: usize,
    /// Committed state blobs loaded into kept plugin instances while it was prepared (RENDER-26b);
    /// a load the host skipped, as the instance held that state (HOME-6), isn't counted.
    pub loads: usize,
    /// Kept plugins whose latency such a load changed, by top-level node, and how many frames now:
    /// this plan compensates what each had before, and the next activation the new (RENDER-17).
    pub relatent: Vec<(usize, u32)>,
    /// The delays it inserted (RENDER-18).
    pub delays: usize,
    /// Kept plugins whose new state didn't load, by top-level node, and why: each plays on as the
    /// plugin left it, and no plan tries that blob again; another committed blob loads (RENDER-26b).
    pub unloaded: Vec<(usize, String)>,
    /// The top-level nodes with new instances.
    pub fresh_nodes: Vec<usize>,
}

/// Where a plan's ghosts, ghost lanes and replaced renders go when its window ends: room for every
/// one, made when the plan is, so the audio thread only swaps them in.
pub struct Leftovers {
    pub(crate) exec: Vec<Exec>,
    pub(crate) lanes: Vec<LaneSlot>,
    pub(crate) renders: Vec<Arc<AudioLane>>,
}

/// The structure a renderer plays: its graph, with every instance it holds, its cells and its
/// output, and how it is entered.
pub(crate) struct Plan {
    /// Unique per plan, from one process-wide counter: what `SwapLane` and an activation check.
    pub(crate) id: u64,
    pub(crate) graph: Graph,
    /// The new graph's top-level nodes: `graph.exec[..nodes]`; ghosts and fades follow.
    pub(crate) nodes: usize,
    /// Its editable constants and their slots (RENDER-22, RENDER-23).
    pub(crate) cells: Cells,
    pub(crate) out: PortRef,
    /// Whether any node hosts a plugin.
    pub(crate) plugins: bool,
    /// What its activation does; None for a fresh plan, and once it took over.
    pub(crate) entry: Option<Box<Entry>>,
    /// The exec nodes of its ghosts: replaced nodes playing out the fade window.
    pub(crate) ghosts: Range<usize>,
    /// The exec nodes of the window's own overlays, which run while the ghosts and old sides they
    /// carry do.
    pub(crate) windowed: Range<usize>,
    /// Its partitions and the tasks that run them (8a): one schedule for the plan's whole life, the
    /// window's nodes included.
    pub(crate) schedule: crate::parts::Schedule,
    /// The fade window, in frames: 5 ms when anything fades, else 0.
    pub(crate) fade: u32,
    /// Frames its crossfades wait for new nodes' inputs to fade in: one fade when any do, else 0.
    pub(crate) wait: u32,
    /// The engine clock at entry, while its window runs.
    pub(crate) faded_from: Option<u64>,
    pub(crate) leftovers: Option<Box<Leftovers>>,
    pub(crate) stats: Stats,
    pub(crate) info: Arc<PlanInfo>,
    /// The window's own overlays (no cell), each with the site whose value it takes.
    pub(crate) window_overlays: Vec<(usize, usize)>,
    /// The window's own overlays with no site to follow, each with the old plan's site whose gain
    /// it keeps from the block line on (MON-1).
    pub(crate) window_held: Vec<(usize, usize)>,
    /// Its taps, written after each block: a live plan's only.
    pub(crate) taps: Option<Taps>,
    /// Its top-level nodes that stopped live (RENDER-25), shared with [`PlanInfo::stopped`].
    pub(crate) stopped: Arc<Stopped>,
    /// Its plugin nodes whose inputs arrive late: told the transport that much earlier (HOST-6).
    pub(crate) late_plugins: Vec<usize>,
}

/// A lane a plan's nodes read. Empty only while it moves between plans: it plays nothing then.
pub(crate) enum LaneSlot {
    Notes(Option<NoteScheduler>),
    Control(Option<Arc<m2_song::ControlLane>>),
    Audio(Option<Box<AudioPlayer>>),
}

impl LaneSlot {
    /// This block's note events (none for other lanes).
    pub(crate) fn events(&self) -> &[Event] {
        match self {
            LaneSlot::Notes(Some(s)) => s.events().as_slice(),
            _ => &[],
        }
    }

    /// A notes lane's scheduler, while it is here.
    pub(crate) fn scheduler(&self) -> Option<&NoteScheduler> {
        match self {
            LaneSlot::Notes(Some(s)) => Some(s),
            _ => None,
        }
    }

    fn empty(kind: Kind) -> LaneSlot {
        match kind {
            Kind::Events(_) => LaneSlot::Notes(None),
            Kind::Control(_) => LaneSlot::Control(None),
            Kind::Audio(_) => LaneSlot::Audio(None),
        }
    }

    fn filled(&self) -> bool {
        match self {
            LaneSlot::Notes(s) => s.is_some(),
            LaneSlot::Control(c) => c.is_some(),
            LaneSlot::Audio(p) => p.is_some(),
        }
    }

    fn same_kind(&self, o: &LaneSlot) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(o)
    }
}

/// The lanes a plan's nodes read, by slot, in the order [`lane_slots`] gives; then the lanes only
/// its ghosts read, which leave when its window ends.
pub(crate) struct Lanes {
    pub(crate) slots: Vec<LaneSlot>,
    pub(crate) refs: Vec<LaneRef>,
    /// `slots[live..]` are read only by ghosts.
    pub(crate) live: usize,
}

/// A plan's lanes as prepared: what moves in, and the room each will have.
pub(crate) struct LanePlan {
    pub(crate) lanes: Lanes,
    /// By slot: the room a notes lane will have.
    pub(crate) caps: Vec<Caps>,
    pub(crate) moves: Vec<LaneMove>,
    /// By live slot: an audio lane's renders.
    pub(crate) audio: Vec<Option<Arc<AudioLane>>>,
}

/// Whether two revisions' data for a lane play alike.
fn same_data(a: Option<&Arc<LaneData>>, b: Option<&Arc<LaneData>>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => Arc::ptr_eq(x, y) || x == y,
        (None, None) => true,
        _ => false,
    }
}

impl LanePlan {
    /// The lanes `flat` reads, from `data` in `tempo` at `rate`. Against a plan that plays, each lane
    /// it has moves in, with new data when its data or the tempo map changed (RENDER-27): notes
    /// rebuilt in the new map, in a bigger scheduler when they outgrew the old one, and every
    /// audio lane rendered through `cache` (which renders only what it doesn't hold), swapped in
    /// when its renders differ. Other lanes are made here.
    pub(crate) fn new(flat: &Flat, data: &DataSet, cache: &mut audio::Cache, tempo: &TempoMap, rate: f64, keep: Option<&Keep>) -> LanePlan {
        let refs = lane_slots(flat);
        let mut p = LanePlan { lanes: Lanes { slots: vec![], refs: vec![], live: refs.len() }, caps: vec![], moves: vec![], audio: vec![] };
        for (s, r) in refs.iter().enumerate() {
            let (m, l) = (r.module.as_str(), r.lane.as_str());
            let from = keep.and_then(|k| Some((k, k.info.lanes.iter().position(|x| x.lane == *r)?)));
            let changed = from.is_some_and(|(k, _)| !same_data(k.old_data.lane(m, l), data.lane(m, l)));
            let mut caps = Caps::default();
            let mut audio = None;
            let slot = match (r.kind, from) {
                (Kind::Events(_), Some((k, o))) => {
                    caps = k.info.lanes[o].caps.unwrap_or_default();
                    let (mut swap, mut grow) = (None, None);
                    if changed || k.old_data.tempo() != tempo {
                        let d = Arc::new(notes_data(data, m, l, rate));
                        if caps.fits(d.needs()) {
                            swap = Some(LaneSwap::Notes(d));
                        } else {
                            let g = NoteScheduler::new(d);
                            caps = g.caps();
                            grow = Some(g);
                        }
                    }
                    p.moves.push(LaneMove { from: o as u32, to: s as u32, swap, grow });
                    LaneSlot::Notes(None)
                }
                (Kind::Events(_), None) => {
                    let sched = NoteScheduler::new(Arc::new(notes_data(data, m, l, rate)));
                    caps = sched.caps();
                    LaneSlot::Notes(Some(sched))
                }
                (Kind::Control(_), Some((_, o))) => {
                    let swap = changed.then(|| LaneSwap::Control(Arc::new(data.control(m, l))));
                    p.moves.push(LaneMove { from: o as u32, to: s as u32, swap, grow: None });
                    LaneSlot::Control(None)
                }
                (Kind::Control(_), None) => LaneSlot::Control(Some(Arc::new(data.control(m, l)))),
                (Kind::Audio(ch), from) => {
                    let lane = Arc::new(AudioLane::build(data.audio(m, l), data, cache, tempo, rate, ch as usize));
                    match from.map(|(k, o)| (o, k.info.lanes[o].audio.clone())) {
                        Some((o, Some(was))) if was.same(&lane) => {
                            audio = Some(was);
                            p.moves.push(LaneMove { from: o as u32, to: s as u32, swap: None, grow: None });
                            LaneSlot::Audio(None)
                        }
                        Some((o, _)) => {
                            audio = Some(lane.clone());
                            p.moves.push(LaneMove { from: o as u32, to: s as u32, swap: Some(LaneSwap::Audio(lane)), grow: None });
                            LaneSlot::Audio(None)
                        }
                        None => {
                            audio = Some(lane.clone());
                            LaneSlot::Audio(Some(Box::new(AudioPlayer::new(lane, rate))))
                        }
                    }
                }
            };
            p.lanes.slots.push(slot);
            p.caps.push(caps);
            p.audio.push(audio);
        }
        p.lanes.refs = refs;
        p
    }

    /// The slot a ghost reads lane `r` through: the live one when the new plan has it, else the
    /// old plan's lane, moved in for the window.
    pub(crate) fn ghost(&mut self, r: &LaneRef, keep: &Keep) -> Option<usize> {
        if let Some(s) = self.lanes.refs.iter().position(|x| x == r) {
            return Some(s);
        }
        let from = keep.info.lanes.iter().position(|x| x.lane == *r)?;
        let to = self.lanes.slots.len();
        self.moves.push(LaneMove { from: from as u32, to: to as u32, swap: None, grow: None });
        self.lanes.slots.push(LaneSlot::empty(r.kind));
        self.lanes.refs.push(r.clone());
        self.caps.push(Caps::default());
        Some(to)
    }

    /// The plan's lanes as [`PlanInfo`] describes them.
    pub(crate) fn info(&self) -> Vec<LaneInfo> {
        let live = self.lanes.live;
        let refs = self.lanes.refs[..live].iter().zip(&self.caps).zip(&self.audio);
        refs.map(|((lane, caps), audio)| LaneInfo {
            lane: lane.clone(),
            caps: matches!(lane.kind, Kind::Events(_)).then_some(*caps),
            audio: audio.clone(),
        })
        .collect()
    }
}

impl Plan {
    /// Where its window's leftovers will go: room for each ghost, ghost lane and audio swap.
    pub(crate) fn leftovers(ghosts: usize, lanes: usize, swaps: usize) -> Box<Leftovers> {
        Box::new(Leftovers { exec: Vec::with_capacity(ghosts), lanes: Vec::with_capacity(lanes), renders: Vec::with_capacity(swaps) })
    }
}

impl Renderer {
    /// Whether its activation's fade window runs.
    pub(crate) fn in_window(&self) -> bool {
        self.plan.faded_from.is_some()
    }

    /// Whether its window is over at engine clock `clock`.
    pub(crate) fn window_over(&self, clock: u64) -> bool {
        self.plan.faded_from.is_some_and(|f| clock >= f + (self.plan.fade + self.plan.wait) as u64)
    }

    /// Whether it was prepared against a plan, so it can only take over from that one.
    pub(crate) fn has_entry(&self) -> bool {
        self.plan.entry.is_some()
    }

    /// Whether every move of its entry fits `old`: the plan it was prepared against, every box
    /// there and of the shape prepared for, every slot here empty, every lane of its kind and
    /// with room for its data. Changes nothing.
    fn verify(&self, old: &Renderer) -> bool {
        let Some(e) = &self.plan.entry else { return false };
        let (og, ng) = (&old.plan.graph, &self.plan.graph);
        let moves = e.moves.iter().all(|m| {
            let (Some(o), Some(n)) = (og.exec.get(m.from as usize), ng.exec.get(m.to as usize)) else { return false };
            match (o, n) {
                (Exec::Kernel(o), Exec::Kernel(n)) => n.inst.is_none() && o.inst.as_ref().is_some_and(|k| k.shape == m.shape),
                (Exec::Poly(o), Exec::Poly(n)) => n.pool.is_none() && o.pool.as_ref().is_some_and(|p| p.shape == m.shape),
                (Exec::Plugin(o), Exec::Plugin(n)) => n.inst.is_none() && o.inst.as_ref().is_some_and(|i| i.shape == m.shape),
                (Exec::Feedback { ring: o, .. }, Exec::Feedback { ring: n, .. }) => o.len() == n.len(),
                (Exec::Delay(o), Exec::Delay(_)) => o.line.shape == m.shape,
                (Exec::NoteDelay(o), Exec::NoteDelay(_)) => o.line.shape == m.shape,
                _ => false,
            }
        });
        let lanes = e.lanes.iter().all(|lm| {
            let (Some(o), Some(n)) = (old.lanes.slots.get(lm.from as usize), self.lanes.slots.get(lm.to as usize)) else { return false };
            let room = match (o, &lm.swap) {
                (LaneSlot::Notes(Some(s)), Some(LaneSwap::Notes(d))) => s.caps().fits(d.needs()),
                (LaneSlot::Audio(Some(p)), Some(LaneSwap::Audio(_))) => !p.fading(),
                (LaneSlot::Control(_), Some(LaneSwap::Control(_))) | (_, None) => true,
                _ => false,
            };
            o.filled() && !n.filled() && o.same_kind(n) && room
        });
        e.after == old.plan.id && old.plan.entry.is_none() && moves && lanes
    }

    /// Take over from `old`, the renderer the engine plays, at a block line at engine clock
    /// `clock` (RENDER-25): verify every move, then make them, all by swap, so nothing is allocated
    /// or freed here and whatever a move displaces leaves with `old`. Returns false, with nothing
    /// changed, when this plan wasn't prepared against `old` or a box isn't what it was prepared for.
    pub(crate) fn take_over(&mut self, old: &mut Renderer, clock: u64) -> bool {
        // A reset below runs a kernel or a plugin, between blocks (RENDER-2a).
        let _mode = fpenv::Canon::enter();
        if !self.verify(old) {
            return false;
        }
        let Some(mut e) = self.plan.entry.take() else { return false };
        // Ramps written at the last block line end first, as the next one would have ended them:
        // a ramp moved with a voice pool would be copied every block (RENDER-22a).
        old.plan.cells.end_ramps(&mut old.plan.graph);
        for m in &e.moves {
            take_from(&mut self.plan.graph, m.to as usize, &mut old.plan.graph, m.from as usize);
        }
        // A box whose output went non-finite, or that trapped or stopped, starts again (RENDER-25,
        // RENDER-10). One whose reset fails stops at once, alone (its error allocated: this path
        // only).
        for m in &e.moves {
            let to = m.to as usize;
            if std::mem::take(&mut self.plan.graph.poisoned[to]) {
                self.plan.stats.resets += 1;
                if let Err(e) = self.plan.graph.reset_node(to) {
                    self.plan.graph.stop(to);
                    self.plan.stopped.stop(to, Why::of(&e));
                }
            }
        }
        // The song position: the same beat in this plan's map, and the engine's clock and note ids.
        // The engine's own clock, not `old`'s: an idle engine renders nothing, so `old`'s stopped,
        // and a window started there would be over at once (a new drone in at full level).
        self.enter_from(old);
        self.clock = clock;
        let shift = old.transport.pos() as i64 - self.transport.pos() as i64;
        for lm in &mut e.lanes {
            let (to, from) = (lm.to as usize, lm.from as usize);
            std::mem::swap(&mut self.lanes.slots[to], &mut old.lanes.slots[from]);
            match (&mut self.lanes.slots[to], lm.swap.take()) {
                // What it replaces leaves with the spent entry.
                (LaneSlot::Notes(Some(s)), Some(LaneSwap::Notes(d))) => lm.swap = Some(LaneSwap::Notes(s.swap(d).unwrap_or_else(|d| d))),
                (LaneSlot::Control(Some(c)), Some(LaneSwap::Control(d))) => lm.swap = Some(LaneSwap::Control(std::mem::replace(c, d))),
                (LaneSlot::Audio(Some(p)), Some(LaneSwap::Audio(l))) => p.swap(l, shift),
                (_, d) => lm.swap = d,
            }
            if let Some(mut g) = lm.grow.take() {
                if let LaneSlot::Notes(Some(small)) = &self.lanes.slots[to] {
                    g.take_state(small);
                }
                let mut slot = LaneSlot::Notes(Some(g));
                std::mem::swap(&mut self.lanes.slots[to], &mut slot);
                // The small one goes back with `old`, into the empty slot the first swap left.
                std::mem::swap(&mut old.lanes.slots[from], &mut slot);
            }
        }
        self.plan.cells.carry(&self.plan.graph, &old.plan.cells, &e.cells, &e.voice_cells);
        // A window overlay with no new site keeps its old site's gain: a track muted, then deleted
        // or read elsewhere, stays silent through the window.
        for &(node, k) in &self.plan.window_held {
            if let Some(gain) = old.plan.cells.overlay_node(k).and_then(|n| old.plan.graph.overlay_gain(n)) {
                self.plan.graph.hold_overlay(node, gain);
            }
        }
        for &r in &e.release {
            self.plan.graph.release(r as usize, self.clock);
        }
        if self.plan.fade > 0 {
            self.plan.faded_from = Some(self.clock);
        }
        old.plan.entry = Some(e);
        true
    }

    /// At a block line once the window is over: its ghosts, ghost lanes and replaced renders go
    /// into the leftovers, to leave the audio thread, and the window's own overlays are dropped (they
    /// hold no memory). Each slot left is a constant 0 that nothing reads, so the one schedule plays
    /// on. None when it has nothing to retire. Never allocates or frees.
    pub(crate) fn end_fade(&mut self) -> Option<Box<Leftovers>> {
        self.plan.faded_from = None;
        let l = self.plan.leftovers.as_mut()?;
        for g in self.plan.ghosts.clone() {
            l.exec.push(std::mem::replace(&mut self.plan.graph.exec[g], Exec::Const(0.0)));
        }
        for o in self.plan.windowed.clone() {
            let gone = std::mem::replace(&mut self.plan.graph.exec[o], Exec::Const(0.0));
            debug_assert!(matches!(gone, Exec::Overlay { .. }), "{o} is a window's overlay");
        }
        let live = self.lanes.live;
        for s in &mut self.lanes.slots[live..] {
            let kind = match s {
                LaneSlot::Notes(_) => Kind::Events(m2_ir::EventType::Note),
                LaneSlot::Control(_) => Kind::Control(None),
                LaneSlot::Audio(_) => Kind::Audio(1),
            };
            l.lanes.push(std::mem::replace(s, LaneSlot::empty(kind)));
        }
        for s in &mut self.lanes.slots[..live] {
            if let LaneSlot::Audio(Some(p)) = s
                && let Some(r) = p.take_prev()
            {
                l.renders.push(r);
            }
        }
        self.plan.leftovers.take()
    }

    /// What its activation kept, made and reset.
    pub fn stats(&self) -> &Stats {
        &self.plan.stats
    }

    /// Its plugins, lanes and cells, for the next plan prepared against it.
    pub fn info(&self) -> Arc<PlanInfo> {
        self.plan.info.clone()
    }

    /// The last block's output of top-level node `node`, channel-major (gates' taps). Off the
    /// audio thread.
    pub fn output_of(&self, node: usize) -> &[f32] {
        self.plan.graph.bufs.get(node).and_then(|b| b.first()).map_or(&[], Vec::as_slice)
    }
}

/// Move the box of `old`'s node `from` into `new`'s node `to`, by swap. A plugin node's notes and
/// what it last sent come with its instance, within the new node's room (§5.1). Verified first.
fn take_from(new: &mut Graph, to: usize, old: &mut Graph, from: usize) {
    match (&mut new.exec[to], &mut old.exec[from]) {
        (Exec::Kernel(n), Exec::Kernel(o)) => std::mem::swap(&mut n.inst, &mut o.inst),
        (Exec::Poly(n), Exec::Poly(o)) => std::mem::swap(&mut n.pool, &mut o.pool),
        (Exec::Feedback { ring: n, .. }, Exec::Feedback { ring: o, .. }) => std::mem::swap(n, o),
        (Exec::Delay(n), Exec::Delay(o)) => std::mem::swap(&mut n.line, &mut o.line),
        (Exec::NoteDelay(n), Exec::NoteDelay(o)) => std::mem::swap(&mut n.line, &mut o.line),
        (Exec::Plugin(n), Exec::Plugin(o)) => {
            std::mem::swap(&mut n.inst, &mut o.inst);
            for (to, from) in [(&mut n.keys, &o.keys), (&mut n.ended, &o.ended)] {
                to.clear();
                let room = to.capacity();
                debug_assert!(from.len() <= room, "a kept plugin node with less room than the old one");
                to.extend(from.iter().take(room));
            }
            (n.status, n.offs, n.last_off, n.again) = (o.status, o.offs, o.last_off, o.again);
            // Last-sent values by parameter id: a constant is sent again only when it changed.
            for p in &mut n.params {
                p.2 = o.params.iter().find(|q| q.0 == p.0).and_then(|q| q.2);
            }
        }
        _ => debug_assert!(false, "verified: the same kinds"),
    }
    new.poisoned[to] = old.poisoned[from];
}

/// How many audio lanes an entry swaps.
pub(crate) fn audio_swaps(moves: &[LaneMove]) -> usize {
    moves.iter().filter(|m| matches!(m.swap, Some(LaneSwap::Audio(_)))).count()
}

/// The slots its ghost lanes take.
pub(crate) fn ghost_lanes(l: &Lanes) -> usize {
    l.slots.len() - l.live
}
