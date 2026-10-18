//! The planner's delays (RENDER-18), and what a plugin late by its offset is told (HOST-6).
//!
//! A delay node holds its line in a box that an activation moves into the next plan when the same
//! delay is still there: the same consumer, input and length ([`crate::latency::Delay`]). Its ring is
//! indexed by the engine clock, which an activation hands on, so a moved line plays on unbroken.
//! Nothing here allocates on the audio thread (RENDER-28): every line is sized when it is built.

use m2_ir::PortRef;
use m2_song::{Arrangement, TempoMap};

use crate::event::{Event, Events};
use crate::transport::{Jump, Run, Runs};
use crate::{BLOCK, Buf, QUIET, RunInfo};

/// An audio or control signal `frames` late: what a delay node's box holds.
pub(crate) struct Line {
    /// `channels × frames` samples, channel-major, by engine sample mod `frames`.
    ring: Buf,
    frames: usize,
    /// A control signal: before the first block it holds that block's first value, as a lane or
    /// `time` held before the song started, never 0. Audio starts from silence.
    prime: bool,
    primed: bool,
    /// Its shape's hash, checked before an activation moves it (RENDER-25).
    pub(crate) shape: u64,
}

impl Line {
    pub(crate) fn new(channels: u8, frames: u32, prime: bool, shape: u64) -> Line {
        Line { ring: vec![0.0; channels as usize * frames as usize], frames: frames as usize, prime, primed: false, shape }
    }

    /// One block: `out` is `src` as it was `frames` engine samples ago. Never allocates.
    pub(crate) fn process(&mut self, engine: u64, src: &[f32], out: &mut [f32]) {
        let (frames, channels) = (self.frames, out.len() / BLOCK);
        let have = (src.len() / BLOCK).max(1);
        if self.prime && !self.primed {
            for c in 0..channels {
                let first = src[c.min(have - 1) * BLOCK];
                self.ring[c * frames..(c + 1) * frames].fill(first);
            }
        }
        self.primed = true;
        let start = (engine % frames as u64) as usize;
        for c in 0..channels {
            let (ring, s) = (&mut self.ring[c * frames..(c + 1) * frames], c.min(have - 1) * BLOCK);
            let mut at = start;
            for f in 0..BLOCK {
                out[c * BLOCK + f] = std::mem::replace(&mut ring[at], src[s + f]);
                at += 1;
                if at == frames {
                    at = 0;
                }
            }
        }
    }

    /// Back to silence, as built (RENDER-10).
    pub(crate) fn reset(&mut self) {
        self.ring.fill(0.0);
        self.primed = false;
    }

    /// Whether it holds sound still to come out: an audio line with something above quiet.
    pub(crate) fn active(&self) -> bool {
        !self.prime && self.ring.iter().any(|x| x.abs() > QUIET)
    }
}

/// A delay node: what it reads, and its line (moved between plans).
pub(crate) struct DelayNode {
    pub(crate) input: PortRef,
    pub(crate) line: Box<Line>,
}

/// Note events `frames` late: what a note delay node's box holds. A ring of the events in flight,
/// each with the engine sample it is due at, in order.
pub(crate) struct NoteLine {
    pending: Vec<(u64, Event)>,
    /// The oldest in `pending`, and how many there are.
    head: usize,
    len: usize,
    frames: u64,
    pub(crate) shape: u64,
}

impl NoteLine {
    /// Room for `per_block` events in each block the delay holds, and one more.
    pub(crate) fn new(frames: u32, per_block: usize, shape: u64) -> NoteLine {
        let room = per_block * (frames as usize).div_ceil(BLOCK) + per_block;
        NoteLine { pending: vec![(0, Event::Midi { frame: 0, bytes: [0; 3] }); room], head: 0, len: 0, frames: frames as u64, shape }
    }

    /// The lane's events of the block at `engine` go in; those due in it come out into `out`, in
    /// order. Never allocates: an event past its room is dropped, an On counted.
    pub(crate) fn process(&mut self, engine: u64, events: &[Event], out: &mut Events) {
        let room = self.pending.len();
        for e in events {
            if self.len == room {
                if matches!(e, Event::On { .. }) {
                    out.drop_on();
                }
                debug_assert!(matches!(e, Event::On { .. }), "a note delay's room is sized from its lane's");
                continue;
            }
            self.pending[(self.head + self.len) % room] = (engine + e.frame() as u64 + self.frames, *e);
            self.len += 1;
        }
        out.clear();
        let end = engine + BLOCK as u64;
        while self.len > 0 {
            let (due, e) = self.pending[self.head];
            if due >= end {
                break;
            }
            out.push(at_frame(e, due.saturating_sub(engine) as u32));
            (self.head, self.len) = ((self.head + 1) % room, self.len - 1);
        }
    }

    /// Nothing in flight, as built (RENDER-10).
    pub(crate) fn reset(&mut self) {
        (self.head, self.len) = (0, 0);
    }

    /// Whether notes are still to come out.
    pub(crate) fn active(&self) -> bool {
        self.len > 0
    }
}

/// `e` at block frame `frame`.
fn at_frame(e: Event, frame: u32) -> Event {
    match e {
        Event::On { id, pitch, velocity, from_pitch, .. } => Event::On { frame, id, pitch, velocity, from_pitch },
        Event::Off { id, velocity, .. } => Event::Off { frame, id, velocity },
        Event::NoteExpression { id, kind, value, .. } => Event::NoteExpression { frame, id, kind, value },
        Event::Param { id, value, .. } => Event::Param { frame, id, value },
        Event::Midi { bytes, .. } => Event::Midi { frame, bytes },
    }
}

/// A note delay node: the notes lane it delays (its slot), its line (moved between plans), and the
/// events due in this block, which its consumer reads.
pub(crate) struct NoteDelayNode {
    pub(crate) slot: usize,
    pub(crate) line: Box<NoteLine>,
    pub(crate) out: Events,
}

/// Runs a told transport has at most: two blocks' runs, each at most [`crate::MAX_RUNS`].
pub(crate) const TOLD_RUNS: usize = 2 * crate::MAX_RUNS;

/// What a plugin `offset` frames late is told in one block (HOST-6): the runs of the transport
/// `offset` frames earlier, laid on this block's frames, each with what the arrangement says at its
/// first frame.
#[derive(Clone, Copy)]
pub(crate) struct Told {
    pub(crate) runs: [Run; TOLD_RUNS],
    pub(crate) len: usize,
    pub(crate) info: [RunInfo; TOLD_RUNS],
    pub(crate) beats: [f64; TOLD_RUNS],
    pub(crate) seconds: [f64; TOLD_RUNS],
}

impl Default for Told {
    fn default() -> Told {
        let still = Run { off: 0, n: BLOCK as u32, at: 0, moving: false, jump: Jump::None };
        Told { runs: [still; TOLD_RUNS], len: 1, info: [RunInfo::default(); TOLD_RUNS], beats: [0.0; TOLD_RUNS], seconds: [0.0; TOLD_RUNS] }
    }
}

/// The transport's runs over the last blocks, by the engine sample each started at: what a plugin
/// late by its offset is told from. An activation hands it on with the clocks.
#[derive(Clone)]
pub(crate) struct Past {
    blocks: Vec<(u64, Runs)>,
}

/// Blocks every renderer remembers at least, so a plan that adds latency at an activation finds
/// the transport before it.
const PAST_MIN: usize = 64;

impl Past {
    /// Room for an offset of `frames`.
    pub(crate) fn new(frames: u32) -> Past {
        let n = ((frames as usize).div_ceil(BLOCK) + 2).max(PAST_MIN);
        Past { blocks: vec![(u64::MAX, Runs::one(Run { off: 0, n: BLOCK as u32, at: 0, moving: false, jump: Jump::None })); n] }
    }

    /// The block at engine sample `engine` ran `runs`. Never allocates.
    pub(crate) fn push(&mut self, engine: u64, runs: Runs) {
        let n = self.blocks.len();
        self.blocks[(engine / BLOCK as u64) as usize % n] = (engine, runs);
    }

    /// What `old` remembers, as far as it reaches. Never allocates.
    pub(crate) fn take_from(&mut self, old: &Past) {
        for &(engine, runs) in old.blocks.iter().filter(|b| b.0 != u64::MAX) {
            let n = self.blocks.len();
            let slot = &mut self.blocks[(engine / BLOCK as u64) as usize % n];
            if slot.0 == u64::MAX || slot.0 < engine {
                *slot = (engine, runs);
            }
        }
    }

    fn get(&self, block: u64) -> Option<&Runs> {
        let b = &self.blocks[(block / BLOCK as u64) as usize % self.blocks.len()];
        (b.0 == block).then_some(&b.1)
    }

    /// The transport position at engine sample `t`, if it is remembered.
    pub(crate) fn position_at(&self, t: u64) -> Option<u64> {
        let block = t - t % BLOCK as u64;
        let f = (t - block) as u32;
        Some(self.get(block)?.frame(f))
    }

    /// What a plugin `offset` frames late is told in the block at engine sample `engine`. Before
    /// what is remembered (before the engine started), the transport stood at the first position
    /// remembered after it, stopped. Never allocates.
    pub(crate) fn told(&self, offset: u32, engine: u64, tempo: &TempoMap, arrangement: &Arrangement, rate: f64) -> Told {
        let mut t = Told { len: 0, ..Told::default() };
        let from = engine as i64 - offset as i64;
        let b = BLOCK as i64;
        // Fragments of the runs over `from..from + BLOCK`, each block's in turn. A block not
        // remembered stood still where the last run left off, or where the next starts.
        let mut s = from;
        while s < from + b {
            let block = s.div_euclid(b) * b;
            let (lo, hi) = ((s - block) as u32, ((from + b).min(block + b) - block) as u32);
            let off = (s - from) as u32;
            match (block >= 0).then(|| self.get(block as u64)).flatten() {
                Some(runs) => {
                    for r in runs.as_slice().iter().filter(|r| r.off < hi && r.off + r.n > lo) {
                        let a = r.off.max(lo);
                        let jump = if a == r.off { r.jump } else { Jump::None };
                        add(&mut t, Run { off: off + (a - lo), n: (r.off + r.n).min(hi) - a, at: r.frame(a), moving: r.moving, jump });
                    }
                }
                None => {
                    let at =
                        t.len.checked_sub(1).map_or(u64::MAX, |l| t.runs[l].at + if t.runs[l].moving { t.runs[l].n as u64 } else { 0 });
                    add(&mut t, Run { off, n: hi - lo, at, moving: false, jump: Jump::None });
                }
            }
            s = block + b;
        }
        if t.runs[0].at == u64::MAX {
            t.runs[0].at = if t.len > 1 { t.runs[1].at } else { 0 };
        }
        for i in 0..t.len {
            let s = t.runs[i].at as f64 / rate;
            let beat = tempo.beat_at(s);
            let meter = arrangement.meter_at(beat);
            let (bar, into) = arrangement.bar_at(beat);
            t.info[i] =
                RunInfo { bpm: tempo.bpm_at(beat), tsig: (meter.beats as u16, meter.unit as u16), bar_start: beat - into, bar: bar as i32 };
            (t.beats[i], t.seconds[i]) = (beat, s);
        }
        t
    }
}

/// Add `r` after the told runs: it carries the last one on when nothing jumps between them.
fn add(t: &mut Told, r: Run) {
    if t.len > 0 {
        let last = &mut t.runs[t.len - 1];
        let next = last.at + if last.moving { last.n as u64 } else { 0 };
        if r.jump == Jump::None && r.moving == last.moving && r.at == next {
            last.n += r.n;
            return;
        }
    }
    if t.len < TOLD_RUNS {
        t.runs[t.len] = r;
        t.len += 1;
    } else {
        t.runs[t.len - 1].n += r.n;
    }
}
