//! Cells (RENDER-22, RENDER-22a, RENDER-23, RENDER-28): every editable constant of a checked graph
//! is a cell, a const node with an origin. Its value is written into the node's buffer and from
//! there into the kernel inputs it feeds once, and again only when a new value arrives through its
//! slot: nothing is rebuilt, nothing is allocated, and a steady cell costs nothing per block.
//!
//! - A cell's [`CellId`] is its index in its renderer's table ([`cells`]), so it means nothing to
//!   any other renderer (RENDER-23): a [`CellTarget`] pairs ids with the slots they index.
//! - Values travel in [`CellSlots`]: the control side writes them, and the audio thread reads the
//!   cells marked changed at each block line. The latest value wins, and the queue is never used
//!   (RENDER-28).
//! - A cell whose control is declared smooth ramps linearly from its old value to the new one over
//!   one block, and the others step (RENDER-22a, PKG-14). Offline renders never receive a value, so
//!   they never ramp.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use m2_ir::{FOp, Flat, Origin};

use crate::{BLOCK, Exec, Graph};

/// A cell's index in one renderer's cell table. It addresses that renderer's cells only
/// (RENDER-23).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellId(pub u32);

/// Where a cell's constant is: a node of the top-level graph, or a node of a `poly`'s voice
/// graph, which is one cell for that node in every voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellAt {
    Top(usize),
    Voice { poly: usize, node: usize },
}

/// One cell, as a checked graph declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct CellInfo {
    pub at: CellAt,
    pub origin: Origin,
    /// Its value when built.
    pub value: f64,
    /// It ramps over one block on every change (RENDER-22a).
    pub smooth: bool,
}

/// Every cell of `flat`, in a fixed order: the const nodes with an origin that the processing
/// order reaches, top level first by node index, then each used `poly`'s voice graph by node index.
pub fn cells(flat: &Flat) -> Vec<CellInfo> {
    let used = |nodes: usize, order: &[usize]| {
        let mut used = vec![false; nodes];
        for &i in order {
            used[i] = true;
        }
        used
    };
    let cell = |at: CellAt, n: &m2_ir::FNode| match (&n.op, &n.origin) {
        (FOp::Const(v), Some(o)) => Some(CellInfo { at, origin: o.clone(), value: *v, smooth: smooth(o) }),
        _ => None,
    };
    let top = used(flat.nodes.len(), &flat.order);
    let mut out: Vec<CellInfo> =
        flat.nodes.iter().enumerate().filter(|(i, _)| top[*i]).filter_map(|(i, n)| cell(CellAt::Top(i), n)).collect();
    for (poly, n) in flat.nodes.iter().enumerate() {
        if let FOp::Poly { voice, .. } = &n.op
            && top[poly]
        {
            let inner = used(voice.nodes.len(), &voice.order);
            out.extend(
                voice.nodes.iter().enumerate().filter(|(i, _)| inner[*i]).filter_map(|(node, n)| cell(CellAt::Voice { poly, node }, n)),
            );
        }
    }
    out
}

/// Whether a constant's control was declared `.smooth()` (PKG-14).
fn smooth(o: &Origin) -> bool {
    o.control.as_ref().is_some_and(|c| c.smooth)
}

/// The cells of each value in the code ([`Origin::same_value`]): one written literal, wherever it
/// reaches, or one inherited control of one call. What a lease on a control writes.
#[derive(Debug, Default)]
pub struct CellMap {
    groups: Vec<(Origin, Vec<CellId>)>,
    /// The groups at each site: one value's origins share their site, so a lookup compares only
    /// these (thousands of cells at 50 tracks).
    by_site: HashMap<u32, Vec<usize>>,
}

impl CellMap {
    pub fn of(cells: &[CellInfo]) -> CellMap {
        let mut map = CellMap::default();
        for (i, c) in cells.iter().enumerate() {
            let id = CellId(i as u32);
            match map.group(&c.origin) {
                Some(g) => map.groups[g].1.push(id),
                None => {
                    map.by_site.entry(c.origin.site).or_default().push(map.groups.len());
                    map.groups.push((c.origin.clone(), vec![id]));
                }
            }
        }
        map
    }

    fn group(&self, origin: &Origin) -> Option<usize> {
        self.by_site.get(&origin.site)?.iter().copied().find(|&g| self.groups[g].0.same_value(origin))
    }

    /// The cells that hold the value written at `origin`: none when it reaches no cell here.
    pub fn cells(&self, origin: &Origin) -> &[CellId] {
        self.group(origin).map_or(&[], |g| &self.groups[g].1)
    }
}

/// Per-cell slots: the control side writes, and the audio thread reads the cells marked changed at
/// each block line. The latest value wins. The audio thread never locks, waits or allocates here.
///
/// Ordering. Writers are serialised by the caller (the player's cell lock), so seqs are published
/// in order. A write stores the values (Relaxed), sets the cells' dirty bits (`fetch_or`, Release),
/// publishes its seq (Release), then sets `any` (Release). The reader, at a block line, clears
/// `any` (`swap`, Acquire), and only if it was set loads the published seq (Acquire), then swaps
/// every dirty word to 0 (Acquire) and applies its cells' values.
/// - Every write up to the published seq it loaded set its bits before publishing, so the drain
///   that follows sees them: what it reports as heard is never early. A newer value read under an
///   older bit is applied now and reported later: late, never early.
/// - `any` is cleared before the drain, so a write that lands during it leaves `any` set and is
///   drained at the next block line. No write is stranded, and an idle engine's wake check sees it.
pub struct CellSlots {
    /// f32 bits.
    values: Box<[AtomicU32]>,
    /// One bit per cell.
    dirty: Box<[AtomicU64]>,
    /// The newest seq whose writes have all set their bits.
    published: AtomicU64,
    any: AtomicBool,
}

impl CellSlots {
    pub fn new(cells: usize) -> CellSlots {
        CellSlots {
            values: (0..cells).map(|_| AtomicU32::new(0)).collect(),
            dirty: (0..cells.div_ceil(64)).map(|_| AtomicU64::new(0)).collect(),
            published: AtomicU64::new(0),
            any: AtomicBool::new(false),
        }
    }

    /// How many cells it holds.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Write `value` into `cells` as write `seq`, for the next block line. Writers must be
    /// serialised and their seqs increase (see the type). A write that reaches no cell is still
    /// published, so it is heard as the others are. Never waits.
    pub fn set(&self, cells: &[CellId], value: f32, seq: u64) {
        for c in cells {
            let i = c.0 as usize;
            let Some(v) = self.values.get(i) else {
                debug_assert!(false, "cell {i} of {}", self.values.len());
                continue;
            };
            v.store(value.to_bits(), Ordering::Relaxed);
            self.dirty[i / 64].fetch_or(1 << (i % 64), Ordering::Release);
        }
        self.published.store(seq, Ordering::Release);
        self.any.store(true, Ordering::Release);
    }

    /// The audio thread, at a block line: every cell written since the last drain, with its
    /// latest value, then the seq every write up to which is now applied. None when nothing was
    /// written. Never allocates or waits.
    pub(crate) fn drain(&self, mut f: impl FnMut(CellId, f32)) -> Option<u64> {
        if !self.any.swap(false, Ordering::Acquire) {
            return None;
        }
        let published = self.published.load(Ordering::Acquire);
        for (w, word) in self.dirty.iter().enumerate() {
            let mut bits = word.swap(0, Ordering::Acquire);
            while bits != 0 {
                let i = w * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                f(CellId(i as u32), f32::from_bits(self.values[i].load(Ordering::Relaxed)));
            }
        }
        Some(published)
    }

    /// Whether a write waits for the next drain: one atomic load.
    pub fn pending(&self) -> bool {
        self.any.load(Ordering::Acquire)
    }
}

/// Where a gesture's values go: cells of one renderer, with that renderer's slots.
#[derive(Clone)]
pub struct CellTarget {
    pub slots: Arc<CellSlots>,
    pub cells: Arc<[CellId]>,
}

/// One cell as a renderer holds it.
struct Cell {
    at: CellAt,
    smooth: bool,
    value: f32,
}

/// A new value as written into a cell's buffers.
#[derive(Clone, Copy)]
pub(crate) enum Write {
    /// The value at every frame.
    Step(f32),
    /// From the first value to the second over the block, ending exactly on it (RENDER-22a).
    Ramp(f32, f32),
}

impl Write {
    /// Fill one channel of one block.
    pub(crate) fn fill(self, block: &mut [f32]) {
        match self {
            Write::Step(v) => block.fill(v),
            Write::Ramp(from, to) => {
                let d = to - from;
                for (i, x) in block.iter_mut().enumerate() {
                    *x = from + d * ((i + 1) as f32 / BLOCK as f32);
                }
                if let Some(last) = block.last_mut() {
                    *last = to;
                }
            }
        }
    }
}

/// A renderer's cells: what each holds, its slots, and how far writes were heard.
pub(crate) struct Cells {
    table: Vec<Cell>,
    /// Overlay cells (MON-1), after the value cells: each overlay node's exec index, and its value.
    /// A value goes to the node's target, and the node glides there itself (RENDER-22a).
    overlays: Vec<(usize, f32)>,
    slots: Arc<CellSlots>,
    map: Arc<CellMap>,
    /// Smooth cells ramping this block, which end on their value at the next block line. Room
    /// for every smooth cell, so it never grows.
    ramping: Vec<u32>,
    /// Values an activation carried in (RENDER-22a): applied at the next block line from the
    /// value the plan before played, as a slot write would be. Room sized when the plan is made.
    carried: Vec<(u32, f32)>,
    /// Every write up to this seq is applied.
    heard: u64,
    /// `heard` as [`Cells::take_heard`] last reported it.
    reported: u64,
}

impl Cells {
    /// The cells of `info`, then one for each overlay node of `overlays` (its node, its value).
    pub(crate) fn new(info: &[CellInfo], overlays: &[(usize, f32)]) -> Cells {
        Cells {
            table: info.iter().map(|c| Cell { at: c.at, smooth: c.smooth, value: c.value as f32 }).collect(),
            overlays: overlays.to_vec(),
            slots: Arc::new(CellSlots::new(info.len() + overlays.len())),
            map: Arc::new(CellMap::of(info)),
            ramping: Vec::with_capacity(info.iter().filter(|c| c.smooth).count()),
            carried: vec![],
            heard: 0,
            reported: 0,
        }
    }

    pub(crate) fn slots(&self) -> &Arc<CellSlots> {
        &self.slots
    }

    pub(crate) fn map(&self) -> &Arc<CellMap> {
        &self.map
    }

    pub(crate) fn value(&self, id: CellId) -> Option<f32> {
        let i = id.0 as usize;
        match self.table.get(i) {
            Some(c) => Some(c.value),
            None => self.overlays.get(i - self.table.len()).map(|o| o.1),
        }
    }

    /// The cell of overlay `k`, the `k`th site of its graph.
    pub(crate) fn overlay(&self, k: usize) -> CellId {
        CellId((self.table.len() + k) as u32)
    }

    /// Set every overlay at once, by site, off the audio thread: a plan not yet sent (MON-1). Its
    /// slots are left alone, so what it reports as heard is unchanged.
    pub(crate) fn set_overlays(&mut self, graph: &mut Graph, values: &[f32]) {
        for (o, &v) in self.overlays.iter_mut().zip(values) {
            o.1 = v;
            graph.set_overlay(o.0, v, true);
        }
    }

    /// At a block line, before the graph runs: ramps from the last block end on their value, then
    /// what an activation carried in and what the slots hold is applied (RENDER-22, RENDER-22a).
    /// Never allocates.
    pub(crate) fn apply(&mut self, graph: &mut Graph) {
        self.end_ramps(graph);
        let Cells { table, overlays, slots, ramping, carried, heard, .. } = self;
        let mut set = |id: u32, v: f32| {
            let Some(c) = table.get_mut(id as usize) else {
                // An overlay's node glides to its value itself: no host ramp (RENDER-22a).
                if let Some(o) = overlays.get_mut(id as usize - table.len())
                    && o.1.to_bits() != v.to_bits()
                {
                    o.1 = v;
                    graph.set_overlay(o.0, v, false);
                }
                return;
            };
            if c.value.to_bits() == v.to_bits() {
                return;
            }
            if c.smooth && ramping.len() < ramping.capacity() {
                write(graph, c.at, Write::Ramp(c.value, v));
                ramping.push(id);
            } else {
                write(graph, c.at, Write::Step(v));
            }
            c.value = v;
        };
        for (id, v) in carried.drain(..) {
            set(id, v);
        }
        if let Some(p) = slots.drain(|id, v| set(id.0, v)) {
            *heard = (*heard).max(p);
        }
    }

    /// Ramps written at the last block line end on their value, as the next block line would end
    /// them: before an activation moves voices whose constants a ramp is in. Never allocates.
    pub(crate) fn end_ramps(&mut self, graph: &mut Graph) {
        for &i in &self.ramping {
            let c = &self.table[i as usize];
            write(graph, c.at, Write::Step(c.value));
        }
        self.ramping.clear();
    }

    /// Room for `n` values an activation carries in.
    pub(crate) fn reserve_carry(&mut self, n: usize) {
        self.carried.reserve_exact(n);
    }

    /// At an activation, on the audio thread, once the boxes moved: each continued cell whose value
    /// differs from what the plan before played is applied at the next block line as a `SetCell`
    /// from that value (RENDER-22a): a smooth one ramps, where a fresh buffer would step. `pairs`
    /// are top-level cells, (new, old); `voices` the voice cells of kept polys, whose constants
    /// moved in with their voices and hold the old values. Never allocates.
    pub(crate) fn carry(&mut self, graph: &Graph, old: &Cells, pairs: &[(CellId, CellId)], voices: &[CellId]) {
        let room = |c: &Vec<(u32, f32)>| c.len() < c.capacity();
        for &(id, was) in pairs {
            let (Some(c), Some(v)) = (self.table.get_mut(id.0 as usize), old.value(was)) else { continue };
            // A step is in the fresh buffer already.
            if c.smooth && c.value.to_bits() != v.to_bits() && room(&self.carried) {
                self.carried.push((id.0, c.value));
                c.value = v;
            }
        }
        for &id in voices {
            let Some(c) = self.table.get_mut(id.0 as usize) else { continue };
            let CellAt::Voice { poly, node } = c.at else { continue };
            if let Some(held) = graph.voice_const(poly, node)
                && held.to_bits() != c.value.to_bits()
                && room(&self.carried)
            {
                self.carried.push((id.0, c.value));
                c.value = held;
            }
        }
    }

    /// Set `cells` to `value` at once, as a step: a renderer not yet sent.
    pub(crate) fn set_now(&mut self, graph: &mut Graph, cells: &[CellId], value: f32) {
        for id in cells {
            if let Some(c) = self.table.get_mut(id.0 as usize) {
                c.value = value;
                write(graph, c.at, Write::Step(value));
            }
        }
    }

    /// Every write up to `seq` is in this renderer already: what it reports as heard starts there.
    pub(crate) fn hear_from(&mut self, seq: u64) {
        self.heard = self.heard.max(seq);
    }

    /// `(from, to)` when writes up to `to` were applied since the last report.
    pub(crate) fn take_heard(&mut self) -> Option<(u64, u64)> {
        (self.heard > self.reported).then(|| (std::mem::replace(&mut self.reported, self.heard), self.heard))
    }
}

/// Write a cell's value into its constant: the node, or that node in every voice of its `poly`.
fn write(graph: &mut Graph, at: CellAt, w: Write) {
    match at {
        CellAt::Top(node) => graph.write_const(node, w),
        CellAt::Voice { poly, node } => {
            if let Exec::Poly(p) = &mut graph.exec[poly] {
                for v in p.pool.iter_mut().flat_map(|pool| &mut pool.voices) {
                    v.graph.write_const(node, w);
                }
            }
        }
    }
}
