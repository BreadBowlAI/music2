//! `@m2/vocals:shift`: a voice moved by `semitones`, its colour kept (VOX-23). TD-PSOLA (Moulines
//! and Charpentier, 1990), written from the paper.
//!
//! **Pulses.** A voice is a train of pulses, one each period, each ringing through the throat and
//! mouth: the ring is the vowel. Each pulse is marked in the mono sum of the channels, low-passed
//! at 1 kHz: while `clarity` says a pitch is sung ([`VOICED`], `hz` in range), the next mark is the
//! largest peak within a quarter period of where the last mark plus `hz`'s period says, so marks
//! stay steady from period to period; a period that disagrees with the one the marks followed by a
//! factor near 2 (an octave error of the pitch) is taken as the one nearer it. A peak at or under 0 ends the
//! marks, and so does a sound with no clear pitch.
//!
//! **Grains.** The output is grains laid over each other, each a raised cosine around its centre `s`
//! read from around a source point `a`, and weighed: each sample out is the sum of the grains over
//! it divided by the sum of their windows, so grains never add up louder than what came in. A
//! *voiced* grain is read around a mark, two periods long (less where the look-ahead is short: a
//! low voice), and the next one is laid the mark's period over `2^(semitones/12)` later: closer for
//! a higher voice, further for a lower one, each grain keeping its shape, so the vowel stays (D5).
//! It is read `2^(formant/12)` faster (and lasts a period over that), which moves the colour alone. Each grain's source is the mark
//! nearest its centre. Where no mark is near (an "s", a breath, a mark that can't be found) grains of
//! [`PASS`] are read where they lie: the sound comes through exactly, so a switch between the two is
//! a crossfade over where their grains overlap, and never moves time. A voiced run starts on a mark,
//! read where it lies, so with no move the voice comes out exactly as it went in.
//!
//! **Look-ahead.** A grain needs the sound up to its source's end before its second half plays, so
//! the output is [`LATENCY_S`] late, the manifest's `latency_s`, in frames by the kit's one rule: the
//! engine delays the rest of the song to match (RENDER-18). Every buffer is sized at birth from the
//! rate: nothing is taken while it plays. Both channels use the same marks and grains.

use m2_dsp::fx::sane;
use m2_kernel::{Cfg, Io, Kernel, latency_frames};

use crate::aim::VOICED;
use crate::yin::{HIGHEST, LOWEST};

/// How late the output is, in seconds: the manifest's `latency_s`.
pub const LATENCY_S: f64 = 0.015;
/// How far `semitones` and `formant` reach, each way.
const MOST: f32 = 12.0;
/// A grain that passes the sound through: half of it, in seconds.
pub const PASS: f32 = 0.010;
/// Marks in a row the pitch may say an octave off before the voice is taken to have leapt.
const LEAP: u8 = 8;
/// Marks kept.
const MARKS: usize = 64;
/// Grains whose second half is still to come.
const PENDING: usize = 32;
/// Points of the half window's table.
const TABLE: usize = 1024;
/// What a sample weighs where no grain is over it: the input, so a gap is never silence.
const EPS: f32 = 1e-4;

const IN: usize = 0;
const SEMITONES: usize = 1;
const HZ: usize = 2;
const CLARITY: usize = 3;
const FORMANT: usize = 4;

#[derive(Clone, Copy, Default)]
struct Mark {
    at: i64,
    /// Frames from the mark before (the period `hz` said, for a chain's first).
    period: f32,
    /// The period the chain followed to find it, octave errors put right, and how many marks in
    /// a row have been put right so.
    said: f32,
    odd: u8,
}

#[derive(Clone, Copy, Default)]
struct Grain {
    s: i64,
    a: f64,
    half: usize,
    q: f32,
}

/// The grain laid last: none yet, one that passes the sound, or a voiced one and its mark's place.
#[derive(Clone, Copy)]
enum Last {
    Pass { s: i64, half: usize },
    Voiced { s: f64, mark: usize },
}

/// A two-pole low-pass (RBJ), its state in f64.
#[derive(Clone, Copy)]
struct Lowpass {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Lowpass {
    fn new(sr: f64, f: f64) -> Self {
        let w = std::f64::consts::TAU * f / sr;
        let alpha = w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let c = w.cos();
        Lowpass {
            b: [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn tick(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        let y = if y.abs() < 1e-30 { 0.0 } else { y };
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y as f32
    }

    fn clear(&mut self) {
        (self.x, self.y) = ([0.0; 2], [0.0; 2]);
    }
}

pub struct Shift {
    sr: f32,
    /// Frames late: `latency_s` at this rate.
    late: usize,
    /// Half of a grain that passes the sound, in frames.
    pass: usize,
    /// The longest period marked, in frames.
    longest: usize,
    mask: usize,
    /// The input, by frame: both channels, and the mono sum low-passed.
    x: [Vec<f32>; 2],
    lp: Vec<f32>,
    filter: Lowpass,
    /// The output being laid, by frame, and its windows' sum.
    acc: [Vec<f32>; 2],
    wsum: Vec<f32>,
    /// The half window, 0 to 1 by [`TABLE`] steps.
    hann: Vec<f32>,
    /// The frame the next input is.
    n: i64,
    marks: [Mark; MARKS],
    /// Marks found so far: mark `j` is `marks[j % MARKS]`.
    count: usize,
    /// The mark the chain goes on from, while one is followed.
    chain: Option<Mark>,
    last: Last,
    pending: [Grain; PENDING],
    /// The pending grains: `head` is the oldest, `len` of them.
    head: usize,
    len: usize,
    /// Frames since the input was last other than 0.
    quiet: usize,
}

fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

impl Shift {
    fn mark(&self, j: usize) -> Option<Mark> {
        (j < self.count && j + MARKS >= self.count).then(|| self.marks[j % MARKS])
    }

    fn push(&mut self, m: Mark) {
        self.marks[self.count % MARKS] = m;
        self.count += 1;
        self.chain = Some(m);
    }

    /// The largest peak of the low-passed sum over frames `from..=to`, and its value.
    fn peak(&self, from: i64, to: i64) -> (i64, f32) {
        let mut best = (from, f32::NEG_INFINITY);
        for i in from..=to {
            let v = self.lp[(i as usize) & self.mask];
            if v > best.1 {
                best = (i, v);
            }
        }
        best
    }

    /// Follow the pulses at frame `self.n`, the newest, given the pitch said there.
    fn track(&mut self, hz: f32, clarity: f32) {
        let n = self.n;
        if !(clarity >= VOICED && (LOWEST..=HIGHEST).contains(&hz)) {
            self.chain = None;
            return;
        }
        let said = self.sr / hz;
        // The newest mark: a chain begins at least half a period after it.
        let newest = self.mark(self.count.wrapping_sub(1)).map(|m| m.at);
        match self.chain {
            None => {
                let (from, to) = ((n as f32 - 1.25 * said).round() as i64, (n as f32 - 0.25 * said).round() as i64);
                let from = newest.map_or(from, |m| from.max(m + (0.5 * said) as i64));
                if from >= 0 && from <= to {
                    let (at, v) = self.peak(from, to);
                    if v > 0.0 {
                        self.push(Mark { at, period: said, said, odd: 0 });
                    }
                }
            }
            Some(last) => {
                // An octave error of the pitch is taken as the period nearer the one followed,
                // until the pitch has said so for [`LEAP`] marks: then the voice leapt an octave.
                let ratio = said / last.said;
                let (period, odd) = match ratio {
                    r if (1.7..2.3).contains(&r) && last.odd < LEAP => (said / 2.0, last.odd + 1),
                    r if (0.43..0.59).contains(&r) && last.odd < LEAP => (said * 2.0, last.odd + 1),
                    _ => (said, 0),
                };
                let expect = last.at as f32 + period;
                let to = (expect + 0.25 * period).round() as i64;
                if n < to {
                    return;
                }
                let from = ((expect - 0.25 * period).round() as i64).max(last.at + 1);
                let (at, v) = self.peak(from, to);
                match v > 0.0 {
                    true => self.push(Mark { at, period: (at - last.at) as f32, said: period, odd }),
                    false => self.chain = None,
                }
            }
        }
    }

    /// The window at `j` frames from a grain's centre, `half` frames to its edge.
    fn window(&self, j: i64, half: usize) -> f32 {
        let u = (j.unsigned_abs() as f32 / half as f32).min(1.0) * TABLE as f32;
        let i = (u as usize).min(TABLE - 1);
        let f = u - i as f32;
        self.hann[i] + f * (self.hann[i + 1] - self.hann[i])
    }

    /// Lay frames `from..to` (from the grain's centre) of grain `g` on the output.
    fn lay(&mut self, g: Grain, from: i64, to: i64, channels: usize) {
        let mask = self.mask;
        for j in from..to {
            let w = self.window(j, g.half);
            let z = ((g.s + j) as usize) & mask;
            let src = g.a + j as f64 * g.q as f64;
            for c in 0..channels {
                let x = &self.x[c];
                let v = match g.q == 1.0 {
                    true => x[(src as i64 as usize) & mask],
                    false => {
                        let i = src.floor();
                        let f = (src - i) as f32;
                        let i = i as i64 as usize;
                        x[i & mask] + f * (x[(i + 1) & mask] - x[i & mask])
                    }
                };
                self.acc[c][z] += w * v;
            }
            self.wsum[z] += w;
        }
    }

    /// The mark nearest frame `s` among those known, by its place.
    fn nearest(&self, s: f64) -> Option<(usize, Mark)> {
        let mut best: Option<(usize, Mark)> = None;
        let mut j = self.count;
        while j > 0 && j + MARKS > self.count {
            j -= 1;
            let m = self.marks[j % MARKS];
            if best.is_none_or(|(_, b)| (m.at as f64 - s).abs() < (b.at as f64 - s).abs()) {
                best = Some((j, m));
            }
            if (m.at as f64) < s - 2.0 * self.longest as f64 {
                break;
            }
        }
        best
    }

    /// The period around mark `j`: to the mark after it once that is known, else its own.
    fn period_at(&self, j: usize) -> Option<f64> {
        let here = self.mark(j)?;
        Some(self.mark(j + 1).map_or(here.period as f64, |n| (n.at - here.at) as f64))
    }

    /// Lay the grains whose first half starts by output frame `k`, each as late as it can be, so
    /// it moves by the newest `semitones` (`ratio`) and finds the most marks.
    fn place(&mut self, k: i64, ratio: f64, q: f32, channels: usize) {
        loop {
            match self.last {
                Last::Pass { s, half } => {
                    let next = s + half as i64;
                    // A voiced run starts on a mark, read where it lies: the first after this grain
                    // and by the next, whose centre is still to come.
                    let mut on = None;
                    let mut j = self.count;
                    while j > 0 && j + MARKS > self.count {
                        j -= 1;
                        let m = self.marks[j % MARKS];
                        if m.at <= s.max(k) {
                            break;
                        }
                        if m.at <= next {
                            on = Some((j, m));
                        }
                    }
                    if let Some((j, m)) = on {
                        let period = self.period_at(j).unwrap_or(m.period as f64);
                        if m.at - (period as i64).min(self.pass as i64) > k {
                            return;
                        }
                        self.voiced(m.at as f64, j, m, k, q, channels);
                    } else if next - self.pass as i64 > k {
                        return;
                    } else {
                        self.pass(next, k, channels);
                    }
                }
                Last::Voiced { s, mark } => {
                    let period = self.period_at(mark).unwrap_or(self.pass as f64);
                    let s = s + (period / ratio).max(1.0);
                    if s.round() as i64 - (period as i64).clamp(1, self.pass as i64) > k {
                        return;
                    }
                    match self.nearest(s) {
                        Some((j, m)) if (m.at as f64 - s).abs() <= 0.5 * m.period as f64 + 1.0 => self.voiced(s, j, m, k, q, channels),
                        // No later mark known yet while the chain goes on: the last, within a period.
                        Some((j, m)) if j + 1 == self.count && self.chain.is_some() && s - (m.at as f64) <= m.period as f64 => {
                            self.voiced(s, j, m, k, q, channels)
                        }
                        _ => self.pass(s.round() as i64, k, channels),
                    }
                }
            }
        }
    }

    /// A voiced grain centred at `s`, read around mark `j` (`m`).
    fn voiced(&mut self, s: f64, j: usize, m: Mark, k: i64, q: f32, channels: usize) {
        let at = s.round() as i64;
        let room = (at - k).max(1) as usize;
        // It reads one pulse: a period each side of its mark, read `q` times as fast, so it lasts
        // a period over `q`; its second half's source must have come in by the time it plays.
        let ahead = (at + self.late as i64 - m.at).max(1) as f32;
        let read = m.period.max(2.0).min(ahead).min(room as f32 * q).min(self.pass as f32 * q);
        let half = ((read / q) as usize).max(1);
        self.grain(Grain { s: at, a: m.at as f64, half, q }, channels);
        self.last = Last::Voiced { s, mark: j };
    }

    /// A grain centred at `at` that passes the sound through.
    fn pass(&mut self, at: i64, k: i64, channels: usize) {
        let half = self.pass.min((at - k).max(1) as usize);
        self.grain(Grain { s: at, a: at as f64, half, q: 1.0 }, channels);
        self.last = Last::Pass { s: at, half };
    }

    /// Lay a grain's first half now, and keep it for its second.
    fn grain(&mut self, g: Grain, channels: usize) {
        self.lay(g, -(g.half as i64), 0, channels);
        if self.len == PENDING {
            // Never so many at once (grains come at least a frame apart, and last at most `pass`
            // past their centre); were it so, the oldest plays out now.
            let old = self.pending[self.head];
            self.lay(old, 0, old.half as i64, channels);
            self.head = (self.head + 1) % PENDING;
            self.len -= 1;
        }
        self.pending[(self.head + self.len) % PENDING] = g;
        self.len += 1;
    }
}

impl Kernel for Shift {
    const INPUTS: usize = 5;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(8_000.0);
        let late = latency_frames(LATENCY_S, sr as f64) as usize;
        let longest = (sr / LOWEST).ceil() as usize;
        let pass = ((PASS * sr) as usize).min(late * 2 / 3).max(1);
        let size = (late + 4 * longest + 2 * late + 64).next_power_of_two();
        let hann = (0..=TABLE).map(|i| 0.5 + 0.5 * (std::f64::consts::PI * i as f64 / TABLE as f64).cos()).map(|w| w as f32).collect();
        Shift {
            sr,
            late,
            pass,
            longest,
            mask: size - 1,
            x: [vec![0.0; size], vec![0.0; size]],
            lp: vec![0.0; size],
            filter: Lowpass::new(sr as f64, 1000.0),
            acc: [vec![0.0; size], vec![0.0; size]],
            wsum: vec![0.0; size],
            hann,
            n: 0,
            marks: [Mark::default(); MARKS],
            count: 0,
            chain: None,
            last: Last::Pass { s: -(late as i64), half: pass },
            pending: [Grain::default(); PENDING],
            head: 0,
            len: 0,
            quiet: usize::MAX,
        }
    }

    /// As `new` leaves it, cleared in place: a reset takes no memory.
    fn reset(&mut self, _: &Cfg) {
        for v in self.x.iter_mut().chain(self.acc.iter_mut()) {
            v.fill(0.0);
        }
        self.lp.fill(0.0);
        self.wsum.fill(0.0);
        self.filter.clear();
        (self.n, self.count, self.chain, self.head, self.len, self.quiet) = (0, 0, None, 0, 0, usize::MAX);
        self.last = Last::Pass { s: -(self.late as i64), half: self.pass };
    }

    fn active(&self) -> bool {
        self.quiet <= self.mask + 1
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.clamp(1, 2);
        let mask = self.mask;
        for t in 0..io.frames {
            let x0 = sane(io.input_ch(IN, 0)[t]);
            let x1 = if channels == 2 { sane(io.input_ch(IN, 1)[t]) } else { x0 };
            let i = (self.n as usize) & mask;
            self.x[0][i] = x0;
            self.x[1][i] = x1;
            self.lp[i] = self.filter.tick(if channels == 2 { 0.5 * (x0 + x1) } else { x0 });
            self.quiet = if x0 != 0.0 || x1 != 0.0 { 0 } else { self.quiet.saturating_add(1) };
            self.track(io.input(HZ)[t], io.input(CLARITY)[t]);
            let semitones = finite(io.input(SEMITONES)[t]).clamp(-MOST, MOST);
            let formant = finite(io.input(FORMANT)[t]).clamp(-MOST, MOST);
            let ratio = 2f64.powf(semitones as f64 / 12.0);
            let q = if formant == 0.0 { 1.0 } else { 2f32.powf(formant / 12.0) };
            let k = self.n - self.late as i64;
            self.place(k, ratio, q, channels);
            // Second halves that start here.
            while self.len > 0 && self.pending[self.head].s <= k {
                let g = self.pending[self.head];
                self.lay(g, 0, g.half as i64, channels);
                self.head = (self.head + 1) % PENDING;
                self.len -= 1;
            }
            let z = (k as usize) & mask;
            let w = self.wsum[z];
            for c in 0..2 {
                let ch = c.min(channels - 1);
                let v = (self.acc[ch][z] + EPS * self.x[ch][z]) / (w + EPS);
                io.output_ch(0, c)[t] = if k < 0 { 0.0 } else { v };
            }
            for a in &mut self.acc {
                a[z] = 0.0;
            }
            self.wsum[z] = 0.0;
            self.n += 1;
        }
    }
}
