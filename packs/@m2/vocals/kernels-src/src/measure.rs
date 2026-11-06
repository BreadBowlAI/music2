//! `@m2/vocals:notes`: the sung notes of a file (VOX-42), a measure (VOX-41): the host runs it over
//! the whole file at the file's rate, and it writes, every [`HOP`] frames, the voice's `hz`,
//! `clarity` and `level` (the reading of `pitch`, [`crate::yin`], of the window centred on the hop),
//! `note` (0 between notes, then 1, 2, 3…) and `centre` (that note's sung pitch, a MIDI number).
//!
//! The rules are written by hand, so they can be read (the note's D11):
//! - a hop is **voiced** where `clarity` is [`VOICED`] or more; [`BRIDGE`] unvoiced hops or fewer
//!   between voiced ones count as voiced, so a dip of a few milliseconds doesn't cut a note;
//! - a **run** of voiced hops is cut into notes where the pitch moves more than [`MOVE`] semitones
//!   and stays (the middle of the next [`SIDE`] hops' pitch and of the last [`SIDE`] differ by more,
//!   at the hop where they differ most), and where it gets 6 dB louder (the middle of the next
//!   [`LOUD`] hops' level is twice the middle of the last [`LOUD`]);
//! - a note under [`SHORTEST`] seconds joins the note it touches (of two, the nearer in pitch); one
//!   that touches none is no note; a run is at most [`LONGEST`] seconds;
//! - a note's `centre` is the middle of its pitch, each hop weighed by its level.
//!
//! It reads ahead as far as a run goes, so it writes a note's numbers once the whole note is known.
//! Every buffer is sized at birth from the file's length.

use m2_kernel::{Cfg, Io, Kernel};

use crate::aim::VOICED;
use crate::file::File;
use crate::yin::Pitch;

/// Frames between two measures, of the file at its own rate: the manifest's `hop`.
pub const HOP: usize = 256;
/// Unvoiced hops a note bridges.
pub const BRIDGE: usize = 2;
/// Hops each side whose pitch is compared, and how far it must move to start a note.
pub const SIDE: usize = 6;
pub const MOVE: f32 = 0.7;
/// Hops each side whose level is compared, and how many times as loud starts a note (6 dB).
pub const LOUD: usize = 3;
const LOUDER: f32 = 2.0;
/// The shortest note, and the longest run, in seconds.
pub const SHORTEST: f64 = 0.060;
pub const LONGEST: f64 = 30.0;

#[derive(Clone, Copy, Default)]
struct Reading {
    hz: f32,
    clarity: f32,
    level: f32,
}

impl Reading {
    fn voiced(&self) -> bool {
        self.clarity >= VOICED && self.hz > 0.0
    }

    fn midi(&self) -> f32 {
        if self.voiced() { 69.0 + 12.0 * (self.hz / 440.0).log2() } else { f32::NAN }
    }
}

/// A note found: hops `from..to`, and its centre.
#[derive(Clone, Copy)]
struct Found {
    from: usize,
    to: usize,
    centre: f32,
}

pub struct Notes {
    file: File,
    pitch: Pitch,
    /// Hops in the file, and the readings made so far, by hop.
    hops: usize,
    readings: Vec<Reading>,
    found: Vec<Found>,
    /// Hops before `done` are known for good; `out` is the next written, `next` the first note
    /// that may hold it.
    done: usize,
    out: usize,
    next: usize,
    /// The shortest note and the longest run, in hops.
    shortest: usize,
    longest: usize,
    /// A run's room: its pitch and level by hop, where it is cut, its pieces, and a sorting space.
    p: Vec<f32>,
    level: Vec<f32>,
    cuts: Vec<usize>,
    pieces: Vec<(usize, usize)>,
    sorting: Vec<(f32, f32)>,
}

/// The middle of the numbers of `x` that are numbers, through `room`; NaN with none.
fn middle(x: &[f32], room: &mut Vec<(f32, f32)>) -> f32 {
    room.clear();
    room.extend(x.iter().filter(|v| v.is_finite()).map(|&v| (v, 1.0)));
    weighted(room)
}

/// The middle of `room`'s numbers, each weighed by its weight: NaN with none.
fn weighted(room: &mut [(f32, f32)]) -> f32 {
    if room.is_empty() {
        return f32::NAN;
    }
    room.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f32 = room.iter().map(|r| r.1).sum();
    let mut sum = 0.0;
    for r in room.iter() {
        sum += r.1;
        if sum >= 0.5 * total {
            return r.0;
        }
    }
    room[room.len() - 1].0
}

impl Notes {
    /// The reading of hop `k`: made in order, as far as asked.
    fn reading(&mut self, k: usize) -> Reading {
        if k >= self.hops {
            return Reading::default();
        }
        let (span, half) = (self.pitch.span() as i64, self.pitch.window() as i64 / 2);
        while self.readings.len() <= k {
            let end = (self.readings.len() * HOP) as i64 + half;
            self.file.hold(end - span, end);
            let (hz, clarity, level) = self.pitch.read_window(self.file.mono(end - span, end));
            self.readings.push(Reading { hz, clarity, level });
        }
        self.readings[k]
    }

    /// Know more hops for good: the unvoiced ones up to the next run, or the next run, cut into notes.
    fn advance(&mut self) {
        let mut s = self.done;
        while s < self.hops && !self.reading(s).voiced() {
            s += 1;
        }
        // Unvoiced hops are known at once: no note.
        if s > self.done {
            self.done = s;
            return;
        }
        let (mut e, mut gap, mut k) = (s + 1, 0, s + 1);
        while k < self.hops && k - s < self.longest {
            if self.reading(k).voiced() {
                (e, gap) = (k + 1, 0);
            } else {
                gap += 1;
                if gap > BRIDGE {
                    break;
                }
            }
            k += 1;
        }
        self.cut(s, e);
        self.done = e;
    }

    /// Cut the run of hops `s..e` into notes.
    fn cut(&mut self, s: usize, e: usize) {
        let n = e - s;
        self.p.clear();
        self.level.clear();
        for k in s..e {
            let r = self.reading(k);
            self.p.push(r.midi());
            self.level.push(r.level);
        }
        self.cuts.clear();
        // Where the pitch moves and stays, then where it gets louder: at the most of each stretch
        // over the line.
        for (side, louder) in [(SIDE, false), (LOUD, true)] {
            let mut best: Option<(usize, f32)> = None;
            // Where a stretch over the line ends, its cut: for the pitch, the first hop of it nearer
            // the pitch after than before, so the cut is where the voice moved.
            let cut_at = |me: &mut Notes, at: usize| -> usize {
                if louder {
                    return at;
                }
                let before = middle(&me.p[at - side..at], &mut me.sorting);
                let after = middle(&me.p[at..at + side], &mut me.sorting);
                (at - side..at + side).find(|&j| (me.p[j] - after).abs() < (me.p[j] - before).abs()).unwrap_or(at).max(1)
            };
            for i in side..=n.saturating_sub(side) {
                if i == 0 || i >= n {
                    continue;
                }
                let (before, after) = if louder {
                    (middle(&self.level[i - side..i], &mut self.sorting), middle(&self.level[i..i + side], &mut self.sorting))
                } else {
                    (middle(&self.p[i - side..i], &mut self.sorting), middle(&self.p[i..i + side], &mut self.sorting))
                };
                let d = if louder { after / before.max(1e-6) } else { (after - before).abs() };
                let over = d.is_finite() && if louder { d > LOUDER } else { d > MOVE };
                match (over, best) {
                    (true, Some((_, b))) if d <= b => {}
                    (true, _) => best = Some((i, d)),
                    (false, Some((at, _))) => {
                        let c = cut_at(self, at);
                        self.cuts.push(c);
                        best = None;
                    }
                    (false, None) => {}
                }
            }
            if let Some((at, _)) = best {
                let c = cut_at(self, at);
                self.cuts.push(c);
            }
        }
        self.cuts.sort_unstable();
        self.cuts.dedup();
        self.pieces.clear();
        let mut from = 0;
        for &c in &self.cuts {
            if c > from {
                self.pieces.push((from, c));
                from = c;
            }
        }
        self.pieces.push((from, n));
        // A piece too short joins the one it touches nearer in pitch; alone, it is no note.
        loop {
            let short = (0..self.pieces.len()).filter(|&i| self.pieces[i].1 - self.pieces[i].0 < self.shortest).min_by_key(|&i| self.pieces[i].1 - self.pieces[i].0);
            let Some(i) = short else { break };
            if self.pieces.len() == 1 {
                self.pieces.clear();
                break;
            }
            let centre = |me: &mut Notes, j: usize| {
                let (a, b) = me.pieces[j];
                middle(&me.p[a..b], &mut me.sorting)
            };
            let here = centre(self, i);
            let left = (i > 0).then(|| (centre(self, i - 1) - here).abs());
            let right = (i + 1 < self.pieces.len()).then(|| (centre(self, i + 1) - here).abs());
            let into_left = match (left, right) {
                (Some(l), Some(r)) => l.is_finite() && (!r.is_finite() || l <= r),
                (Some(_), None) => true,
                _ => false,
            };
            if into_left {
                self.pieces[i - 1].1 = self.pieces[i].1;
            } else {
                self.pieces[i + 1].0 = self.pieces[i].0;
            }
            self.pieces.remove(i);
        }
        for j in 0..self.pieces.len() {
            let (a, b) = self.pieces[j];
            self.sorting.clear();
            for i in a..b {
                if self.p[i].is_finite() {
                    self.sorting.push((self.p[i], self.level[i].max(1e-9)));
                }
            }
            let centre = weighted(&mut self.sorting);
            if centre.is_finite() && self.found.len() < self.found.capacity() {
                self.found.push(Found { from: s + a, to: s + b, centre });
            }
        }
    }
}

impl Kernel for Notes {
    const INPUTS: usize = 0;
    const OUTPUTS: usize = 5;

    fn new(cfg: &Cfg) -> Self {
        let asset = cfg.assets.first().copied().unwrap_or_default();
        let rate = if asset.sample_rate > 0.0 { asset.sample_rate } else { cfg.sample_rate }.clamp(8_000.0, 384_000.0);
        let pitch = Pitch::new(&Cfg::new(rate, cfg.max_block, 1));
        let file = File::new(asset, 4 * pitch.span() + 2 * HOP, pitch.span() + HOP);
        let hops = (file.frames() as usize).div_ceil(HOP);
        let shortest = (SHORTEST * rate as f64 / HOP as f64).ceil() as usize;
        let longest = (LONGEST * rate as f64 / HOP as f64).ceil() as usize;
        Notes {
            file,
            pitch,
            hops,
            readings: Vec::with_capacity(hops),
            found: Vec::with_capacity(hops / shortest.max(1) + 1),
            done: 0,
            out: 0,
            next: 0,
            shortest,
            longest,
            p: Vec::with_capacity(longest + 1),
            level: Vec::with_capacity(longest + 1),
            cuts: Vec::with_capacity(longest + 1),
            pieces: Vec::with_capacity(longest + 2),
            sorting: Vec::with_capacity(longest + 1),
        }
    }

    /// As `new` leaves it, its readings forgotten: they are made again, the same.
    fn reset(&mut self, _: &Cfg) {
        self.readings.clear();
        self.found.clear();
        (self.done, self.out, self.next) = (0, 0, 0);
    }

    fn process(&mut self, io: &mut Io) {
        for t in 0..io.frames {
            let k = self.out;
            while self.done <= k && self.done < self.hops {
                self.advance();
            }
            let r = self.reading(k);
            while self.next < self.found.len() && self.found[self.next].to <= k {
                self.next += 1;
            }
            let (note, centre) = match self.found.get(self.next) {
                Some(f) if f.from <= k => ((self.next + 1) as f32, f.centre),
                _ => (0.0, 0.0),
            };
            for (o, v) in [r.hz, r.clarity, r.level, note, centre].into_iter().enumerate() {
                io.output(o)[t] = v;
            }
            self.out += 1;
        }
    }
}
