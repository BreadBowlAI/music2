//! What an agent's listen measures (AGENT-7), written from the standards, with no FFT and no crate
//! of its own:
//!
//! - **Loudness**, per ITU-R BS.1770-4, gated as EBU R128 (EBU Tech 3341's cases): each side
//!   K-weighted ([`k_weighting`]: a high shelf, then a high-pass), its mean square kept every
//!   10 ms. Integrated: the 400 ms blocks every 100 ms, gated at −70 LUFS, then at 10 LU under the
//!   loudness of what passed. Short-term maximum: the loudest 3 s window, every 10 ms. Under 3 s, or
//!   under the gate, there is none.
//! - **True peak**, per BS.1770 Annex 2: each side oversampled 4× by an interpolating filter, its
//!   largest |x|. A block whose samples can't make a peak over the one found is passed over: the
//!   filter's gain bounds what they can make.
//! - **Peak, RMS and crest**: the largest |sample|, the mean square of both sides, and peak minus
//!   RMS (a sine's crest is 3 dB). Clipped: samples over 0 dBFS. Silent: none louder than −90 dBFS.
//! - **Octave bands**: ten one-octave bands, 31.5 Hz to 16 kHz, each a 4th-order Butterworth
//!   band-pass (two biquads, by the bilinear transform with both edges pre-warped), its mean square
//!   over both sides, in dBFS (a full-scale sine is −3 dB).
//! - **Stereo**: the correlation of the two sides; the width, the side's energy against the mid's
//!   in dB; and whether the lows under 120 Hz are mono, their side 20 dB or more under their mid.
//! - **Masking** ([`masking`]), from the tracks' bands as they reach the mix.
//! - **The picture** ([`Picture`]): third-octave levels every 50 ms, which `m2 listen --png` draws.
//!
//! A [`Meter`] takes a signal a piece at a time, as a render hands it, and keeps only sums, so a
//! song's length costs no memory but the 10 ms steps and the picture's columns.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

mod hear;
pub mod png;

pub use hear::{Ask, Heard, Tapped, TrackHeard, difference, difference_words, hear, tapped};

/// The ten octave bands' centres, as they are named, in Hz: each is 1 kHz times a power of two
/// (31.25, 62.5…), named as ISO 266 names it.
pub const OCTAVES: [f64; 10] = [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

/// The picture's rows: 31 third-octave bands, 20 Hz to 20 kHz, as ISO 266 names them.
pub const THIRDS: [f64; 31] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0,
    1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
];

/// The picture's columns, in seconds.
pub const COLUMN: f64 = 0.05;

/// Quieter than this (−90 dBFS) is silence, as a render's tail counts it.
const SILENT: f64 = 3.2e-5;
/// BS.1770's −0.691 dB: a 1 kHz sine at 0 dBFS on both sides reads 0 LUFS.
const K_OFFSET: f64 = -0.691;
/// EBU R128's absolute gate, in LUFS, and its relative one, in LU under what passed it.
const ABSOLUTE_GATE: f64 = -70.0;
const RELATIVE_GATE: f64 = 10.0;
/// Loudness's steps: 10 ms; a gating block is 40 of them, every 10; a short-term window 300.
const STEPS_PER_BLOCK: usize = 40;
const BLOCK_EVERY: usize = 10;
const STEPS_PER_WINDOW: usize = 300;
/// The lows the stereo reading asks to be mono: under 120 Hz.
const LOWS: f64 = 120.0;

/// Masking: a track quieter than this in a band (dBFS) masks nothing there…
pub const AUDIBLE: f64 = -60.0;
/// …two tracks within this of a band's loudest track mask each other there…
pub const CLOSE: f64 = 6.0;
/// …when the band is one of each one's own: within this of its loudest band, not what leaks into
/// the band beside it.
pub const OWN: f64 = 10.0;

/// What a meter measures besides loudness and peaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wants {
    pub bands: bool,
    pub stereo: bool,
    /// The picture's third-octave columns.
    pub picture: bool,
}

/// What a meter read. A level of a signal that is all zeros is None, as is loudness under the gate
/// or of a signal too short to read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measures {
    /// Integrated loudness, in LUFS.
    pub lufs: Option<f64>,
    /// The loudest short-term (3 s) loudness, in LUFS.
    pub lufs_short_max: Option<f64>,
    /// dBTP.
    pub true_peak: Option<f64>,
    /// The loudest sample, in dBFS.
    pub peak: Option<f64>,
    /// The mean square of both sides, in dBFS.
    pub rms: Option<f64>,
    /// Peak minus RMS, in dB.
    pub crest: Option<f64>,
    /// Samples over 0 dBFS, both sides counted.
    pub clipped: u64,
    pub silent: bool,
    /// Each octave band's level, in dBFS ([`OCTAVES`]).
    pub bands: Option<[Option<f64>; 10]>,
    pub stereo: Option<Stereo>,
}

/// How the two sides relate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stereo {
    /// From −1 (one side inverted) through 0 to 1 (mono); None with a silent side.
    pub correlation: Option<f64>,
    /// The side's energy against the mid's, in dB, from −60 (mono) to +60 (all side).
    pub width: Option<f64>,
    /// The lows under 120 Hz: their side 20 dB or more under their mid, or no lows at all.
    pub lows_mono: bool,
}

/// Third-octave levels every 50 ms ([`THIRDS`], [`COLUMN`]), of the mid (both sides' mean).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picture {
    /// Each column's level in each row, in dBFS; None for silence.
    pub columns: Vec<[Option<f32>; 31]>,
}

/// A biquad, in transposed direct form II.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Biquad {
    pub(crate) b0: f64,
    pub(crate) b1: f64,
    pub(crate) b2: f64,
    pub(crate) a1: f64,
    pub(crate) a2: f64,
    s1: f64,
    s2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Biquad {
        Biquad { b0, b1, b2, a1, a2, s1: 0.0, s2: 0.0 }
    }

    /// `(b2 s² + b1 s + b0) / (a2 s² + a1 s + a0)` at `rate`, by the bilinear transform.
    fn analog([b2, b1, b0]: [f64; 3], [a2, a1, a0]: [f64; 3], rate: f64) -> Biquad {
        let k = 2.0 * rate;
        let d = a2 * k * k + a1 * k + a0;
        Biquad::new(
            (b2 * k * k + b1 * k + b0) / d,
            2.0 * (b0 - b2 * k * k) / d,
            (b2 * k * k - b1 * k + b0) / d,
            2.0 * (a0 - a2 * k * k) / d,
            (a2 * k * k - a1 * k + a0) / d,
        )
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// BS.1770's K-weighting at `rate`: its pre-filter, a high shelf of about +4 dB from 1.7 kHz, then
/// its high-pass (RLB) at about 38 Hz. Made from their analogue shapes, so any rate gets them; at
/// 48 kHz they are the standard's table.
pub(crate) fn k_weighting(rate: f64) -> [Biquad; 2] {
    let (f0, gain, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (PI * f0 / rate).tan();
    let vh = 10f64.powf(gain / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad::new(
        (vh + vb * k / q + k * k) / a0,
        2.0 * (k * k - vh) / a0,
        (vh - vb * k / q + k * k) / a0,
        2.0 * (k * k - 1.0) / a0,
        (1.0 - k / q + k * k) / a0,
    );
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let high_pass = Biquad::new(1.0, -2.0, 1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0);
    [shelf, high_pass]
}

/// A band-pass from `lo` to `hi` Hz at `rate`: a 2nd-order Butterworth low-pass made a band-pass
/// (4th order), its edges pre-warped, as two biquads. An edge past 0.45 of the rate is put there.
fn band(lo: f64, hi: f64, rate: f64) -> [Biquad; 2] {
    let warp = |f: f64| 2.0 * rate * (PI * f.min(0.45 * rate) / rate).tan();
    let (wl, wh) = (warp(lo), warp(hi));
    let (w0, bw) = (wl * wh, wh - wl);
    // The prototype's pole (−1 + j)/√2 gives two band-pass poles, the roots of s² − p·bw·s + w0²;
    // each makes a section with its conjugate (the prototype's other pole's).
    let pb = C(-FRAC_1_SQRT_2 * bw, FRAC_1_SQRT_2 * bw);
    let root = pb.mul(pb).sub(C(4.0 * w0, 0.0)).sqrt();
    let section = |s: C| Biquad::analog([0.0, bw, 0.0], [1.0, -2.0 * s.0, s.0 * s.0 + s.1 * s.1], rate);
    [section(pb.add(root).half()), section(pb.sub(root).half())]
}

/// `x` through two biquads, one after the other.
#[inline]
fn two(f: &mut [Biquad; 2], x: f64) -> f64 {
    let y = f[0].run(x);
    f[1].run(y)
}

/// A 4th-order Butterworth low-pass at `hz`: two biquads.
fn low_pass(hz: f64, rate: f64) -> [Biquad; 2] {
    let w = 2.0 * rate * (PI * hz / rate).tan();
    let section = |q: f64| Biquad::analog([0.0, 0.0, w * w], [1.0, w / q, w * w], rate);
    [section(0.541_196_100_146_197), section(1.306_562_964_876_376_6)]
}

/// A complex number, for [`band`]'s poles.
#[derive(Clone, Copy)]
struct C(f64, f64);

impl C {
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
    fn sub(self, o: C) -> C {
        C(self.0 - o.0, self.1 - o.1)
    }
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn half(self) -> C {
        C(self.0 / 2.0, self.1 / 2.0)
    }
    fn sqrt(self) -> C {
        let r = self.0.hypot(self.1);
        let re = ((r + self.0) / 2.0).sqrt();
        let im = ((r - self.0) / 2.0).sqrt();
        C(re, if self.1 < 0.0 { -im } else { im })
    }
}

/// Taps either side of the point a true-peak filter interpolates.
const HALF: usize = 8;
const TAPS: usize = 2 * HALF;

/// BS.1770 Annex 2's 4× oversampling, for one side: the three points between each two samples,
/// interpolated by a windowed sinc (Lanczos, 8 lobes a side), each phase's gain 1.
struct TruePeak {
    phases: [[f64; TAPS]; 3],
    /// What any phase can make of samples no louder than 1.
    gain: f64,
    /// The last samples, twice over, so the last `TAPS` are one slice.
    ring: [f64; 2 * TAPS],
    at: usize,
    /// The loudest point found between samples.
    max: f64,
}

impl TruePeak {
    fn new() -> TruePeak {
        let sinc = |x: f64| if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
        let mut phases = [[0.0; TAPS]; 3];
        for (p, h) in phases.iter_mut().enumerate() {
            // Point `p + 1` quarters past sample `HALF - 1` of the slice.
            let t = HALF as f64 - 1.0 + (p + 1) as f64 / 4.0;
            for (j, c) in h.iter_mut().enumerate() {
                let x = j as f64 - t;
                *c = sinc(x) * sinc(x / HALF as f64);
            }
            let sum: f64 = h.iter().sum();
            h.iter_mut().for_each(|c| *c /= sum);
        }
        let gain = phases.iter().map(|h| h.iter().map(|c| c.abs()).sum::<f64>()).fold(0.0, f64::max);
        TruePeak { phases, gain, ring: [0.0; 2 * TAPS], at: 0, max: 0.0 }
    }

    /// The next samples; `loudest` is their largest |x|.
    fn push(&mut self, x: &[f32], loudest: f64) {
        // Points between these samples (and the last ones before them, which the filter still
        // reads) can't pass `gain` times the loudest of them.
        let before = self.ring[..TAPS].iter().fold(0.0f64, |m, s| m.max(s.abs()));
        let skip = self.gain * loudest.max(before) <= self.max;
        for &s in x {
            let s = s as f64;
            self.ring[self.at] = s;
            self.ring[self.at + TAPS] = s;
            self.at = (self.at + 1) % TAPS;
            if !skip {
                let w = &self.ring[self.at..self.at + TAPS];
                for h in &self.phases {
                    let y: f64 = w.iter().zip(h).map(|(a, b)| a * b).sum();
                    self.max = self.max.max(y.abs());
                }
            }
        }
    }
}

/// One stereo signal's meter.
pub struct Meter {
    rate: f64,
    /// Each side's K-weighting.
    k: [[Biquad; 2]; 2],
    /// Loudness's 10 ms steps: each one's mean square, both sides summed; and the one being filled.
    step: usize,
    filled: usize,
    sum: f64,
    steps: Vec<f64>,
    peaks: [TruePeak; 2],
    peak: f64,
    squares: f64,
    frames: u64,
    clipped: u64,
    bands: Option<Box<Bands>>,
    stereo: Option<Box<Sums>>,
    picture: Option<Box<Columns>>,
}

/// The octave bands' filters, each side's, and their sums of squares.
struct Bands {
    filters: [[[Biquad; 2]; 10]; 2],
    sums: [f64; 10],
}

/// What the stereo reading sums.
#[derive(Default)]
struct Sums {
    ll: f64,
    rr: f64,
    lr: f64,
    lows: [[Biquad; 2]; 2],
    low_mid: f64,
    low_side: f64,
}

/// The picture's filters and the column being filled.
struct Columns {
    filters: [[Biquad; 2]; 31],
    len: usize,
    filled: usize,
    sums: [f64; 31],
    done: Vec<[Option<f32>; 31]>,
}

/// `x` in dB, None for 0.
fn db(power: f64) -> Option<f64> {
    (power > 0.0).then(|| 10.0 * power.log10())
}

impl Meter {
    pub fn new(rate: u32, wants: Wants) -> Meter {
        let rate = rate as f64;
        let k = k_weighting(rate);
        let octave = |c: usize| {
            let hz = 1000.0 * 2f64.powi(c as i32 - 5);
            band(hz * FRAC_1_SQRT_2, hz / FRAC_1_SQRT_2, rate)
        };
        let bands = wants.bands.then(|| {
            let one: [[Biquad; 2]; 10] = std::array::from_fn(octave);
            Box::new(Bands { filters: [one, one], sums: [0.0; 10] })
        });
        let stereo = wants.stereo.then(|| Box::new(Sums { lows: [low_pass(LOWS, rate); 2], ..Sums::default() }));
        let picture = wants.picture.then(|| {
            let third = |c: usize| {
                let hz = 1000.0 * 2f64.powf((c as f64 - 17.0) / 3.0);
                band(hz * 2f64.powf(-1.0 / 6.0), hz * 2f64.powf(1.0 / 6.0), rate)
            };
            let len = (COLUMN * rate).round().max(1.0) as usize;
            Box::new(Columns { filters: std::array::from_fn(third), len, filled: 0, sums: [0.0; 31], done: vec![] })
        });
        Meter {
            rate,
            k: [k, k],
            step: (rate / 100.0).round().max(1.0) as usize,
            filled: 0,
            sum: 0.0,
            steps: vec![],
            peaks: [TruePeak::new(), TruePeak::new()],
            peak: 0.0,
            squares: 0.0,
            frames: 0,
            clipped: 0,
            bands,
            stereo,
            picture,
        }
    }

    /// The next frames: `left` and `right`, as long as each other.
    pub fn push(&mut self, left: &[f32], right: &[f32]) {
        debug_assert_eq!(left.len(), right.len());
        let n = left.len().min(right.len());
        let (left, right) = (&left[..n], &right[..n]);
        for (&l, &r) in left.iter().zip(right) {
            let (l, r) = (l as f64, r as f64);
            let (kl, kr) = (two(&mut self.k[0], l), two(&mut self.k[1], r));
            self.sum += kl * kl + kr * kr;
            self.filled += 1;
            if self.filled == self.step {
                self.steps.push(self.sum / self.step as f64);
                (self.sum, self.filled) = (0.0, 0);
            }
            self.squares += l * l + r * r;
            self.clipped += (l.abs() > 1.0) as u64 + (r.abs() > 1.0) as u64;
        }
        self.frames += n as u64;
        for (side, x) in [left, right].into_iter().enumerate() {
            for piece in x.chunks(128) {
                let loudest = piece.iter().fold(0.0f64, |m, &s| m.max((s as f64).abs()));
                self.peak = self.peak.max(loudest);
                self.peaks[side].push(piece, loudest);
            }
        }
        if let Some(b) = &mut self.bands {
            let Bands { filters, sums } = &mut **b;
            for (side, x) in [left, right].into_iter().enumerate() {
                for &s in x {
                    let s = s as f64;
                    for (f, sum) in filters[side].iter_mut().zip(sums.iter_mut()) {
                        let y = two(f, s);
                        *sum += y * y;
                    }
                }
            }
        }
        if let Some(s) = &mut self.stereo {
            for (&l, &r) in left.iter().zip(right) {
                let (l, r) = (l as f64, r as f64);
                s.ll += l * l;
                s.rr += r * r;
                s.lr += l * r;
                let (a, b) = (two(&mut s.lows[0], l), two(&mut s.lows[1], r));
                s.low_mid += (a + b) * (a + b) / 4.0;
                s.low_side += (a - b) * (a - b) / 4.0;
            }
        }
        if let Some(c) = &mut self.picture {
            for (&l, &r) in left.iter().zip(right) {
                let mid = (l as f64 + r as f64) / 2.0;
                for (f, sum) in c.filters.iter_mut().zip(c.sums.iter_mut()) {
                    let y = two(f, mid);
                    *sum += y * y;
                }
                c.filled += 1;
                if c.filled == c.len {
                    let n = c.len as f64;
                    c.done.push(c.sums.map(|s| db(s / n).map(|d| d as f32)));
                    (c.sums, c.filled) = ([0.0; 31], 0);
                }
            }
        }
    }

    /// What it read so far.
    pub fn measures(&self) -> Measures {
        let frames = self.frames.max(1) as f64;
        let peak = db(self.peak * self.peak);
        let rms = db(self.squares / (2.0 * frames));
        let true_peak = self.peaks[0].max.max(self.peaks[1].max).max(self.peak);
        let (lufs, lufs_short_max) = self.loudness();
        Measures {
            lufs,
            lufs_short_max,
            true_peak: db(true_peak * true_peak),
            peak,
            rms,
            crest: peak.zip(rms).map(|(p, r)| p - r),
            clipped: self.clipped,
            silent: self.peak < SILENT,
            bands: self.bands.as_ref().map(|b| b.sums.map(|s| db(s / (2.0 * frames)))),
            stereo: self.stereo.as_ref().map(|s| {
                let (mid, side) = (self.squares + 2.0 * s.lr, self.squares - 2.0 * s.lr);
                let width = match (db(side), db(mid)) {
                    _ if mid <= 0.0 && side <= 0.0 => None,
                    (Some(s), Some(m)) => Some((s - m).clamp(-60.0, 60.0)),
                    (None, _) => Some(-60.0),
                    (_, None) => Some(60.0),
                };
                let correlation = (s.ll > 0.0 && s.rr > 0.0).then(|| (s.lr / (s.ll * s.rr).sqrt()).clamp(-1.0, 1.0));
                let quiet = (s.low_mid + s.low_side) / frames < 1e-6;
                Stereo { correlation, width, lows_mono: quiet || s.low_side <= 0.01 * s.low_mid }
            }),
        }
    }

    /// The picture's columns so far, when it was asked for.
    pub fn picture(&self) -> Option<Picture> {
        self.picture.as_ref().map(|c| Picture { columns: c.done.clone() })
    }

    /// Integrated loudness and the short-term maximum, from the 10 ms steps (BS.1770, EBU R128).
    fn loudness(&self) -> (Option<f64>, Option<f64>) {
        let lufs = |power: f64| db(power).map(|d| K_OFFSET + d);
        let mut sums = Vec::with_capacity(self.steps.len() + 1);
        sums.push(0.0);
        for s in &self.steps {
            sums.push(sums.last().copied().unwrap_or(0.0) + s);
        }
        let mean = |from: usize, n: usize| (sums[from + n] - sums[from]) / n as f64;
        let blocks: Vec<f64> = (0..)
            .map(|j| j * BLOCK_EVERY)
            .take_while(|&at| at + STEPS_PER_BLOCK <= self.steps.len())
            .map(|at| mean(at, STEPS_PER_BLOCK))
            .collect();
        let gated = |above: f64| {
            let passed: Vec<f64> = blocks.iter().copied().filter(|&z| lufs(z).is_some_and(|l| l > above)).collect();
            (!passed.is_empty()).then(|| passed.iter().sum::<f64>() / passed.len() as f64)
        };
        let integrated = gated(ABSOLUTE_GATE)
            .and_then(lufs)
            .and_then(|l| gated(ABSOLUTE_GATE.max(l - RELATIVE_GATE)))
            .and_then(lufs);
        let short = (0..=self.steps.len().saturating_sub(STEPS_PER_WINDOW))
            .filter(|_| self.steps.len() >= STEPS_PER_WINDOW)
            .map(|at| mean(at, STEPS_PER_WINDOW))
            .fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m| m.max(z))))
            .and_then(lufs)
            .filter(|&l| l > ABSOLUTE_GATE);
        (integrated, short)
    }

    /// The rate it measures at.
    pub fn rate(&self) -> f64 {
        self.rate
    }
}

/// Tracks that mask each other in one band: every two of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Masked {
    /// The band's centre, as it is named ([`OCTAVES`]).
    pub hz: f64,
    /// Two or more, the loudest first.
    pub tracks: Vec<String>,
    /// Their levels in the band, in dBFS.
    pub db: Vec<f64>,
}

/// Masking, from each track's octave bands as it reaches the mix (`heard`, by key): in each band,
/// the tracks within [`CLOSE`] dB of the band's loudest track, each louder there than [`AUDIBLE`]
/// and within [`OWN`] dB of its own loudest band. Every two of them mask each other there; a band
/// with one or none has no masking. Band by band, the loudest first.
pub fn masking(heard: &[(&str, [Option<f64>; 10])]) -> Vec<Masked> {
    let mut found = vec![];
    for (b, &hz) in OCTAVES.iter().enumerate() {
        let loudest = heard.iter().filter_map(|(_, x)| x[b]).fold(f64::NEG_INFINITY, f64::max);
        let mut here: Vec<(&str, f64)> = heard
            .iter()
            .filter_map(|&(key, x)| {
                let level = x[b]?;
                let own = x.iter().flatten().copied().fold(f64::NEG_INFINITY, f64::max);
                (level >= AUDIBLE && level >= loudest - CLOSE && level >= own - OWN).then_some((key, level))
            })
            .collect();
        here.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
        if here.len() >= 2 {
            found.push(Masked { hz, tracks: here.iter().map(|t| t.0.to_string()).collect(), db: here.iter().map(|t| t.1).collect() });
        }
    }
    found
}

#[cfg(test)]
#[path = "listen_tests.rs"]
mod tests;
