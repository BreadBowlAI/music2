//! Taps (TAP-1 to TAP-4, TAP-8): looking at a live plan without touching it. A tap is a point of
//! the plan ([`TapAt`]: the song's output, a top-level node's first output, or an overlay site)
//! whose block is read after the graph ran, never by the graph. A tap gives one of two things:
//!
//! - **Peaks**: each channel's largest `|x|` since the server last took them, raised with
//!   `fetch_max` into an atomic slot (a non-negative `f32` orders as its bits do). The meters: the
//!   song's output, and each meter the server lays out ([`crate::Renderer::tap`], TAP-5), the
//!   louder of its points channel by channel. Raised only while watched ([`TapSlots::watch`]).
//! - **Frames**: a ring of the last frames a point gave, two channels, for a scope or the master's
//!   loudness. A plan has [`RINGS`] of them, made while it is prepared; each copies only while
//!   the server points it somewhere ([`TapSlots::point`]), and the audio thread takes a new point
//!   at its next block. Before a block is written, the frames it may overwrite are marked; after,
//!   its head moves: so a reader ([`TapSlots::frames`]) drops what was overwritten while it read.
//!
//! No lock, no allocation and no wait on the audio thread (RENDER-28, TAP-3). Taps read buffers
//! only, so no render changes. Offline renders have none (TAP-2).
//!
//! Copy taps (AGENT-7) are an offline render's instead, for an agent's listen: after each block,
//! from the one that holds a given transport frame on, a copy of each tapped buffer ([`Copies`]),
//! which the analysis reads and clears between chunks. They read the same points, as the meters
//! do, so no render changes. Only a [`Copying`] has them: it owns its renderer and never gives it
//! up, and a [`crate::Renderer`], which the live engine takes, holds none, so no live plan can
//! have them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};

use anyhow::{Result, bail};

use crate::{BLOCK, Renderer, Runner};

/// The frame rings of a live plan: one for the master's loudness and four for scopes (TAP-8).
pub const RINGS: usize = 5;
/// The frames a ring holds: 341 ms at 48 kHz, 85 ms at 192 kHz, so a reader every 33 ms keeps up.
pub const RING: usize = 1 << 14;
/// A ring's channels: a wider point gives its first two, a mono one the same on both.
const SIDES: usize = 2;
/// A ring that points nowhere, and one that points at the song's output; any other point is an exec
/// node.
const OFF: u64 = u64::MAX;
const OUT: u64 = u64::MAX - 1;

/// A live plan's taps, as the server reads them: the peaks since they were last taken, the frame
/// rings, and whether anyone looks (TAP-4).
#[derive(Debug)]
pub struct TapSlots {
    /// While false the audio thread raises no peak.
    watched: AtomicBool,
    /// The output's two channels, then each meter's.
    peaks: Box<[AtomicU32]>,
    /// Each meter: its first slot and its channels.
    meters: Vec<(usize, usize)>,
    rings: Box<[Ring]>,
    /// What a point may be: each top-level node's first output's audio channels (0: no sound), and
    /// each overlay site's exec node and channels.
    sound: Box<[u8]>,
    sites: Box<[(usize, u8)]>,
}

/// What [`TapSlots::take`] read: each channel's largest `|x|`, linear (1.0 is 0 dBFS). A meter's
/// NaN is passed over and an infinity reads as `f32::MAX`, so every peak is a number JSON can
/// carry. `out` reads the block as played: its samples that weren't finite are 0 already.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peaks {
    pub out: [f32; 2],
    /// Each meter's channels, in the order [`crate::Renderer::tap`] was given them.
    pub meters: Vec<Vec<f32>>,
}

/// One ring of frames (TAP-3): two channels of [`RING`] frames each, one after the other.
#[derive(Debug)]
struct Ring {
    /// What the server asks it to copy: [`OFF`], [`OUT`] or an exec node.
    asked: AtomicU64,
    /// What the audio thread copies, and the frame it began that at.
    took: AtomicU64,
    since: AtomicU64,
    /// The end of the block being written: frames before `started - RING` are gone.
    started: AtomicU64,
    /// The frames written.
    head: AtomicU64,
    frames: Box<[AtomicU32]>,
}

impl Ring {
    fn new() -> Ring {
        let frames = (0..SIDES * RING).map(|_| AtomicU32::new(0)).collect();
        let zero = || AtomicU64::new(0);
        Ring { asked: AtomicU64::new(OFF), took: AtomicU64::new(OFF), since: zero(), started: zero(), head: zero(), frames }
    }

    /// After a block: take a new point, then copy the block from it. `took` is the audio thread's
    /// own copy of [`Ring::took`], so a ring that points nowhere costs one atomic load. `bufs` are
    /// the graph's, by exec node and port; `out` is the block the song gave. Never allocates.
    fn write(&self, took: &mut u64, bufs: &[Vec<Vec<f32>>], out: &[[f32; 2]; BLOCK]) {
        let asked = self.asked.load(Ordering::Acquire);
        if asked == OFF && *took == OFF {
            return;
        }
        let head = self.head.load(Ordering::Relaxed);
        if asked != *took {
            *took = asked;
            self.since.store(head, Ordering::Relaxed);
            self.took.store(asked, Ordering::Release);
        }
        // A point is checked when it is asked for; a node index past this plan's is never read.
        if asked == OFF || (asked != OUT && bufs.get(asked as usize).and_then(|b| b.first()).is_none_or(|x| x.len() < BLOCK)) {
            return;
        }
        // What this block overwrites is marked before any of it is written.
        self.started.store(head + BLOCK as u64, Ordering::Relaxed);
        fence(Ordering::Release);
        let at = (head % RING as u64) as usize;
        for c in 0..SIDES {
            let to = &self.frames[c * RING + at..c * RING + at + BLOCK];
            match asked {
                OUT => to.iter().zip(out).for_each(|(t, f)| t.store(f[c].to_bits(), Ordering::Relaxed)),
                node => {
                    let x = side(&bufs[node as usize][0], c);
                    to.iter().zip(x).for_each(|(t, v)| t.store(v.to_bits(), Ordering::Relaxed));
                }
            }
        }
        self.head.store(head + BLOCK as u64, Ordering::Release);
    }
}

/// Channel `c` of a node's block, channel-major: a node narrower than that gives its last channel,
/// as a mono signal is heard on both sides.
fn side(b: &[f32], c: usize) -> &[f32] {
    let have = (b.len() / BLOCK).max(1);
    let s = c.min(have - 1) * BLOCK;
    &b[s..s + BLOCK]
}

impl TapSlots {
    /// A live plan's taps, with no meter yet: what may be a point, by node and by site.
    pub(crate) fn new(sound: Box<[u8]>, sites: Box<[(usize, u8)]>) -> TapSlots {
        TapSlots {
            watched: AtomicBool::new(false),
            peaks: (0..2).map(|_| AtomicU32::new(0)).collect(),
            meters: vec![],
            rings: (0..RINGS).map(|_| Ring::new()).collect(),
            sound,
            sites,
        }
    }

    /// The exec node and channels of `at`; [`OUT`] for the output.
    fn resolve(&self, at: TapAt) -> Result<(u64, usize)> {
        Ok(match at {
            TapAt::Out => (OUT, 2),
            TapAt::Node(i) => match self.sound.get(i) {
                Some(&c) if c > 0 => (i as u64, c as usize),
                Some(_) => bail!("node {i} makes no sound to tap"),
                None => bail!("there's no node {i}"),
            },
            TapAt::Site(k) => match self.sites.get(k) {
                Some(&(node, c)) => (node as u64, c as usize),
                None => bail!("there's no overlay site {k}"),
            },
        })
    }

    /// Lay out `meters`, each the louder of its points channel by channel, its channels its widest
    /// point's: their slots, and each point's (exec node, first slot, channels) for the audio
    /// thread. The output is [`Peaks::out`] already, so it is no meter's point.
    pub(crate) fn lay_out(&mut self, meters: &[Vec<TapAt>]) -> Result<Vec<(usize, usize, usize)>> {
        let mut points = vec![];
        let mut layout = vec![];
        let mut next = 2;
        for m in meters {
            let mut channels = 1;
            let mut nodes = vec![];
            for &at in m {
                let (node, c) = self.resolve(at)?;
                if node == OUT {
                    bail!("the song's output is metered already: it is no meter's point");
                }
                channels = channels.max(c);
                nodes.push(node as usize);
            }
            points.extend(nodes.into_iter().map(|node| (node, next, channels)));
            layout.push((next, channels));
            next += channels;
        }
        self.peaks = (0..next).map(|_| AtomicU32::new(0)).collect();
        self.meters = layout;
        Ok(points)
    }

    /// While someone looks at the meters (TAP-4): the peaks are raised only then. The rings copy
    /// while they point somewhere, whatever this says.
    pub fn watch(&self, on: bool) {
        self.watched.store(on, Ordering::Relaxed);
    }

    /// Whether the meters are watched.
    pub fn watched(&self) -> bool {
        self.watched.load(Ordering::Relaxed)
    }

    /// How many meters it has.
    pub fn meters(&self) -> usize {
        self.meters.len()
    }

    /// The peaks since the last take, each slot set back to 0.
    pub fn take(&self) -> Peaks {
        let take = |i: usize| f32::from_bits(self.peaks[i].swap(0, Ordering::Relaxed));
        Peaks { out: [take(0), take(1)], meters: self.meters.iter().map(|&(at, n)| (at..at + n).map(take).collect()).collect() }
    }

    fn raise(&self, slot: usize, peak: f32) {
        self.peaks[slot].fetch_max(peak.to_bits(), Ordering::Relaxed);
    }

    /// Point ring `ring` at `at`, or nowhere (None): the audio thread takes it at its next block
    /// (TAP-3). The point's channels.
    pub fn point(&self, ring: usize, at: Option<TapAt>) -> Result<usize> {
        let Some(r) = self.rings.get(ring) else { bail!("there's no ring {ring}: a plan has {RINGS}") };
        let (code, channels) = match at {
            Some(at) => self.resolve(at)?,
            None => (OFF, 0),
        };
        r.asked.store(code, Ordering::Release);
        Ok(channels)
    }

    /// Ring `ring`'s frames from frame `from` of it on, the last `most` at most, into `out`, which
    /// is cleared first: both sides of each. Where the first frame is, by the ring's count; None
    /// while the audio thread hasn't taken the ring's point yet, or it points nowhere. Frames from
    /// before its point was taken, or that the audio thread overwrote while they were read, are
    /// left out: what is returned is always the point's, frame after frame.
    pub fn frames(&self, ring: usize, from: u64, most: usize, out: &mut Vec<[f32; 2]>) -> Option<u64> {
        out.clear();
        let r = self.rings.get(ring)?;
        let asked = r.asked.load(Ordering::Acquire);
        if asked == OFF || r.took.load(Ordering::Acquire) != asked {
            return None;
        }
        let since = r.since.load(Ordering::Relaxed);
        let head = r.head.load(Ordering::Acquire);
        // The block being written may be overwriting the oldest block's room already.
        let kept = (head + BLOCK as u64).saturating_sub(RING as u64);
        let first = from.max(since).max(kept).max(head.saturating_sub(most as u64));
        let load = |c: usize, f: u64| f32::from_bits(r.frames[c * RING + (f % RING as u64) as usize].load(Ordering::Relaxed));
        out.extend((first..head).map(|f| [load(0, f), load(1, f)]));
        // Whatever the audio thread overwrote meanwhile goes.
        fence(Ordering::Acquire);
        let gone = r.started.load(Ordering::Relaxed).saturating_sub(RING as u64);
        if gone > first {
            let lost = ((gone - first) as usize).min(out.len());
            out.drain(..lost);
            return Some(first + lost as u64);
        }
        Some(first)
    }
}

/// The audio thread's side of a plan's taps: which exec node raises which peak slots, and what
/// each ring copies.
pub(crate) struct Taps {
    slots: Arc<TapSlots>,
    /// Each meter's points: exec node, its meter's first slot, and its meter's channels.
    points: Vec<(usize, usize, usize)>,
    /// Each ring's point as the audio thread took it.
    took: [u64; RINGS],
}

impl Taps {
    pub(crate) fn new(slots: TapSlots) -> Taps {
        Taps { slots: Arc::new(slots), points: vec![], took: [OFF; RINGS] }
    }

    pub(crate) fn slots(&self) -> &Arc<TapSlots> {
        &self.slots
    }

    /// Lay out `meters` ([`TapSlots::lay_out`]): only before anyone else holds the slots, as
    /// a plan not yet read.
    pub(crate) fn lay_out(&mut self, meters: &[Vec<TapAt>]) -> Result<()> {
        let Some(slots) = Arc::get_mut(&mut self.slots) else {
            bail!("a plan's meters are laid out before anything reads its taps (TAP-9)");
        };
        self.points = slots.lay_out(meters)?;
        Ok(())
    }

    /// After a block: raise each peak slot to its block's peak while watched, and write each ring
    /// that points somewhere. `bufs` are the graph's, by exec node and port; `out` is the block the
    /// song gave. Never allocates (TAP-3); unwatched, one atomic load, and one per ring (TAP-4).
    pub(crate) fn write(&mut self, bufs: &[Vec<Vec<f32>>], out: &[[f32; 2]; BLOCK]) {
        let s = &*self.slots;
        if s.watched.load(Ordering::Relaxed) {
            for c in 0..2 {
                s.raise(c, peak(out.iter().map(|f| f[c])));
            }
            for &(node, at, channels) in &self.points {
                for c in 0..channels {
                    s.raise(at + c, peak(side(&bufs[node][0], c).iter().copied()));
                }
            }
        }
        for (r, took) in s.rings.iter().zip(&mut self.took) {
            r.write(took, bufs, out);
        }
    }
}

/// The largest `|x|`: a NaN is passed over, and an infinity reads as `f32::MAX`.
fn peak(x: impl Iterator<Item = f32>) -> f32 {
    x.fold(0.0f32, |m, v| m.max(v.abs())).min(f32::MAX)
}

/// Where a tap reads (TAP-1), live or a copy tap's (AGENT-7): the song's output, a checked node's
/// first output, or an overlay site's (a track where its audio enters another module, MON-1), by
/// its place in `Flat::overlays`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapAt {
    Out,
    Node(usize),
    Site(usize),
}

/// What an offline render's copy taps copied (AGENT-7): each tapped buffer, block by block, since
/// they were last cleared.
#[derive(Debug, Default)]
pub struct Copies {
    taps: Vec<Copied>,
    /// Blocks that end before this transport frame aren't copied.
    from: u64,
    /// Each block copied: the transport frame its first frame is at.
    blocks: Vec<u64>,
    /// Each tap's blocks, one after another.
    data: Vec<Vec<f32>>,
}

/// One copy tap: the exec node whose first output it reads (None: the song's output), its
/// channels, and the frames its samples come after the song time they carry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Copied {
    pub(crate) node: Option<usize>,
    pub(crate) channels: usize,
    pub(crate) late: u32,
}

impl Copies {
    pub(crate) fn new(taps: Vec<Copied>, from: u64) -> Copies {
        let data = vec![vec![]; taps.len()];
        Copies { taps, from, blocks: vec![], data }
    }

    /// How many taps it has.
    pub fn taps(&self) -> usize {
        self.taps.len()
    }

    /// Tap `tap`'s channels.
    pub fn channels(&self, tap: usize) -> usize {
        self.taps[tap].channels
    }

    /// The frames tap `tap`'s samples come after the song time they carry (RENDER-19): a block
    /// at transport frame `at` holds song time `at - late` on.
    pub fn late(&self, tap: usize) -> u32 {
        self.taps[tap].late
    }

    /// Each block copied: the transport frame its first frame is at.
    pub fn blocks(&self) -> &[u64] {
        &self.blocks
    }

    /// Tap `tap`'s copy of block `k`: [`BLOCK`] frames of each channel, one channel after another.
    pub fn block(&self, tap: usize, k: usize) -> &[f32] {
        let n = self.taps[tap].channels * BLOCK;
        &self.data[tap][k * n..(k + 1) * n]
    }

    /// Forget what was copied, keeping the room.
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.data.iter_mut().for_each(Vec::clear);
    }

    /// After the block whose first frame is at transport frame `at`: copy each tap. `bufs` are the
    /// graph's, by exec node and port; `out` is the block the song gave. A node narrower than its
    /// tap gives each channel it lacks its last one, as the meters read it.
    pub(crate) fn copy(&mut self, at: u64, bufs: &[Vec<Vec<f32>>], out: &[[f32; 2]; BLOCK]) {
        if at + (BLOCK as u64) <= self.from {
            return;
        }
        self.blocks.push(at);
        for (t, d) in self.taps.iter().zip(&mut self.data) {
            match t.node {
                None => (0..2).for_each(|c| d.extend(out.iter().map(|f| f[c]))),
                Some(node) => (0..t.channels).for_each(|c| d.extend_from_slice(side(&bufs[node][0], c))),
            }
        }
    }
}

/// An offline render that copies its taps (AGENT-7): its renderer, which it owns and never gives
/// up, and what it copied. The live engine takes a [`Renderer`], which holds no copy taps, so none
/// ride into it.
pub struct Copying {
    r: Renderer,
    copies: Copies,
}

impl Copying {
    /// `r`, with copy taps at `at`: after each block from transport frame `from` on, a copy of each
    /// tapped buffer. Refused for a renderer the live engine plays, or one prepared for it.
    pub fn new(r: Renderer, at: &[TapAt], from: u64) -> Result<Copying> {
        let taps = r.copied(at)?;
        Ok(Copying { r, copies: Copies::new(taps, from) })
    }

    /// Render the next `out.len()` frames on `runner`'s threads, as [`Renderer::process_on`] does,
    /// copying each block's taps.
    pub fn process_on(&mut self, out: &mut [[f32; 2]], runner: &mut Runner) -> Result<()> {
        self.r.process_into(out, runner, Some(&mut self.copies))
    }

    /// What it copied since it was last cleared.
    pub fn copies(&mut self) -> &mut Copies {
        &mut self.copies
    }

    /// Its renderer, to read.
    pub fn renderer(&self) -> &Renderer {
        &self.r
    }
}

#[cfg(test)]
mod tests {
    // `Peaks` says it: a NaN is passed over, and an infinity reads as `f32::MAX`.
    #[test]
    fn a_nan_is_passed_over_and_an_infinity_reads_as_max() {
        assert_eq!(super::peak([f32::NAN, f32::NAN].into_iter()), 0.0);
        assert_eq!(super::peak([0.5, f32::NAN, -0.25].into_iter()), 0.5);
        assert_eq!(super::peak([0.5, f32::NEG_INFINITY, f32::NAN].into_iter()), f32::MAX);
    }
}
