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
//! - **Octave bands**: ten one-octave bands, 31.5 Hz to 16 kHz, each an 8th-order Butterworth
//!   band-pass (four biquads, by the bilinear transform with both edges pre-warped), its mean square
//!   over both sides, in dBFS (a full-scale sine is −3 dB). A tone at the centre of the band beside
//!   one reads 26 dB down in it, which IEC 61260's class 1 octave filters ask for and more.
//! - **Stereo**: the correlation of the two sides; the width, the side's energy against the mid's
//!   in dB; and whether the lows under 120 Hz are mono, their side 20 dB or more under their mid.
//! - **Masking** ([`masking`]), from the tracks' octave bands as they reach the mix, moment by
//!   moment ([`MOMENT`]): two tracks mask each other only where they sound at once.
//! - **The picture** ([`Picture`]): third-octave levels every 50 ms, which `m2 listen --png` draws.
//!
//! A [`Meter`] takes a signal a piece at a time, as a render hands it, and keeps only sums, so a
//! song's length costs no memory but the 10 ms steps, the moments and the picture's columns. It is
//! warmed first with what came before the range ([`Meter::warm`]), so a range that starts while a
//! sound is loud reads it as it is, not as a step from silence. A sample that isn't a number is
//! counted and read as 0.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

mod hear;
pub mod png;

pub use hear::{Ask, Heard, Overlaid, Tapped, TrackHeard, difference, difference_words, hear, tapped};

/// The ten octave bands' centres, as they are named, in Hz: each is 1 kHz times a power of two
/// (31.25, 62.5…), named as ISO 266 names it.
pub const OCTAVES: [f64; 10] = [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

/// The picture's rows: 31 third-octave bands, 20 Hz to 20 kHz, as ISO 266 names them.
pub const THIRDS: [f64; 31] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0,
    2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
];

/// The picture's columns, in seconds.
pub const COLUMN: f64 = 0.05;

/// Masking's moments, in seconds: two tracks mask each other only where they sound at once.
pub const MOMENT: f64 = 0.05;

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

/// Masking, moment by moment: a track quieter than this in a band (dBFS) masks nothing there…
pub const AUDIBLE: f64 = -60.0;
/// …nor does one this far or more under a band beside it: that is the band-pass letting its
/// neighbour's sound through (a tone at the centre of the band beside reads 26 dB down, one a
/// quarter of an octave past their edge 15 dB down), not sound of its own…
pub const LEAK: f64 = 16.0;
/// …and two tracks within this of the loudest of the rest there mask each other.
pub const CLOSE: f64 = 6.0;

/// What a meter measures besides loudness and peaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wants {
    pub bands: bool,
    pub stereo: bool,
    /// The picture's third-octave columns.
    pub picture: bool,
    /// The octave bands moment by moment ([`MOMENT`]), for masking.
    pub moments: bool,
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
    /// Samples that weren't numbers (NaN or infinity), both sides counted: each read as 0.
    pub non_finite: u64,
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

/// One moment of a signal ([`MOMENT`]): its mean square over both sides, and in each octave band.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Moment {
    pub all: f32,
    pub bands: [f32; 10],
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

/// A Butterworth band-pass from `lo` to `hi` Hz at `rate`, of order `2 * N`: an `N`th-order
/// Butterworth low-pass made a band-pass, its edges pre-warped, as `N` biquads. An edge past 0.45 of
/// the rate is put there.
fn band<const N: usize>(lo: f64, hi: f64, rate: f64) -> [Biquad; N] {
    let warp = |f: f64| 2.0 * rate * (PI * f.min(0.45 * rate) / rate).tan();
    let (wl, wh) = (warp(lo), warp(hi));
    let (w0, bw) = (wl * wh, wh - wl);
    let section = |s: C| Biquad::analog([0.0, bw, 0.0], [1.0, -2.0 * s.0, s.0 * s.0 + s.1 * s.1], rate);
    // Each prototype pole above the axis, at angle π(2k + N − 1)/2N, gives two band-pass poles, the
    // roots of s² − p·bw·s + w0²; each makes a section with its conjugate (the prototype's other
    // pole's).
    let mut out = [Biquad::default(); N];
    for k in 0..N / 2 {
        let angle = PI * (2 * (k + 1) + N - 1) as f64 / (2 * N) as f64;
        let pb = C(bw * angle.cos(), bw * angle.sin());
        let root = pb.mul(pb).sub(C(4.0 * w0, 0.0)).sqrt();
        out[2 * k] = section(pb.add(root).half());
        out[2 * k + 1] = section(pb.sub(root).half());
    }
    out
}

/// `x` through biquads, one after another.
#[inline]
fn run<const N: usize>(f: &mut [Biquad; N], x: f64) -> f64 {
    f.iter_mut().fold(x, |y, b| b.run(y))
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

    /// The next samples; `loudest` is their largest |x|. Uncounted (`count` false), they only fill
    /// the history the filter reads.
    fn push(&mut self, x: &[f32], loudest: f64, count: bool) {
        // Points between these samples (and the last ones before them, which the filter still
        // reads) can't pass `gain` times the loudest of them.
        let before = self.ring[..TAPS].iter().fold(0.0f64, |m, s| m.max(s.abs()));
        let skip = !count || self.gain * loudest.max(before) <= self.max;
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
    non_finite: u64,
    /// The samples being read, each side's, with what isn't a number read as 0.
    clean: [Vec<f32>; 2],
    bands: Option<Box<Bands>>,
    stereo: Option<Box<Sums>>,
    picture: Option<Box<Columns>>,
}

/// The octave bands' filters, each side's, and their sums of squares.
struct Bands {
    filters: [[[Biquad; 4]; 10]; 2],
    sums: [f64; 10],
    /// Each whole moment, when asked for; and the one being filled: its sum of squares, and each
    /// band's.
    moments: Option<Vec<Moment>>,
    moment: usize,
    filled: usize,
    all: f64,
    now: [f64; 10],
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
            band::<4>(hz * FRAC_1_SQRT_2, hz / FRAC_1_SQRT_2, rate)
        };
        let bands = wants.bands.then(|| {
            let one: [[Biquad; 4]; 10] = std::array::from_fn(octave);
            let moment = (MOMENT * rate).round().max(1.0) as usize;
            Box::new(Bands {
                filters: [one, one],
                sums: [0.0; 10],
                moments: wants.moments.then(Vec::new),
                moment,
                filled: 0,
                all: 0.0,
                now: [0.0; 10],
            })
        });
        let stereo = wants.stereo.then(|| Box::new(Sums { lows: [low_pass(LOWS, rate); 2], ..Sums::default() }));
        let picture = wants.picture.then(|| {
            let third = |c: usize| {
                let hz = 1000.0 * 2f64.powf((c as f64 - 17.0) / 3.0);
                band::<2>(hz * 2f64.powf(-1.0 / 6.0), hz * 2f64.powf(1.0 / 6.0), rate)
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
            non_finite: 0,
            clean: [vec![], vec![]],
            bands,
            stereo,
            picture,
        }
    }

    /// The next frames of the range: `left` and `right`, as long as each other.
    pub fn push(&mut self, left: &[f32], right: &[f32]) {
        self.read(left, right, true);
    }

    /// Frames from before the range, which settle its filters and the true peak's history as the
    /// song has them there: nothing in them is counted.
    pub fn warm(&mut self, left: &[f32], right: &[f32]) {
        self.read(left, right, false);
    }

    /// `left` and `right` through every filter; counted (`count`) or only heard.
    fn read(&mut self, left: &[f32], right: &[f32], count: bool) {
        debug_assert_eq!(left.len(), right.len());
        let n = left.len().min(right.len());
        // What isn't a number is read as 0, and counted.
        let mut clean = std::mem::take(&mut self.clean);
        for (c, x) in clean.iter_mut().zip([&left[..n], &right[..n]]) {
            c.clear();
            c.extend(x.iter().map(|&s| if s.is_finite() { s } else { 0.0 }));
            if count {
                self.non_finite += x.iter().filter(|s| !s.is_finite()).count() as u64;
            }
        }
        let (left, right) = (&clean[0][..], &clean[1][..]);
        for (&l, &r) in left.iter().zip(right) {
            let (l, r) = (l as f64, r as f64);
            let (kl, kr) = (run(&mut self.k[0], l), run(&mut self.k[1], r));
            if !count {
                continue;
            }
            self.sum += kl * kl + kr * kr;
            self.filled += 1;
            if self.filled == self.step {
                self.steps.push(self.sum / self.step as f64);
                (self.sum, self.filled) = (0.0, 0);
            }
            self.squares += l * l + r * r;
            self.clipped += (l.abs() > 1.0) as u64 + (r.abs() > 1.0) as u64;
        }
        if count {
            self.frames += n as u64;
        }
        for (side, x) in [left, right].into_iter().enumerate() {
            for piece in x.chunks(128) {
                let loudest = piece.iter().fold(0.0f64, |m, &s| m.max((s as f64).abs()));
                if count {
                    self.peak = self.peak.max(loudest);
                }
                self.peaks[side].push(piece, loudest, count);
            }
        }
        if let Some(b) = &mut self.bands {
            let Bands { filters, sums, moments, moment, filled, all, now } = &mut **b;
            let [lf, rf] = filters;
            for (&l, &r) in left.iter().zip(right) {
                let (l, r) = (l as f64, r as f64);
                for (((fl, fr), sum), at) in lf.iter_mut().zip(rf.iter_mut()).zip(sums.iter_mut()).zip(now.iter_mut()) {
                    let (a, b) = (run(fl, l), run(fr, r));
                    if count {
                        let e = a * a + b * b;
                        *sum += e;
                        *at += e;
                    }
                }
                if !count {
                    continue;
                }
                *all += l * l + r * r;
                *filled += 1;
                if *filled == *moment {
                    if let Some(m) = moments {
                        let n = 2.0 * *moment as f64;
                        m.push(Moment { all: (*all / n) as f32, bands: now.map(|e| (e / n) as f32) });
                    }
                    (*all, *now, *filled) = (0.0, [0.0; 10], 0);
                }
            }
        }
        if let Some(s) = &mut self.stereo {
            for (&l, &r) in left.iter().zip(right) {
                let (l, r) = (l as f64, r as f64);
                let (a, b) = (run(&mut s.lows[0], l), run(&mut s.lows[1], r));
                if count {
                    s.ll += l * l;
                    s.rr += r * r;
                    s.lr += l * r;
                    s.low_mid += (a + b) * (a + b) / 4.0;
                    s.low_side += (a - b) * (a - b) / 4.0;
                }
            }
        }
        if let Some(c) = &mut self.picture {
            for (&l, &r) in left.iter().zip(right) {
                let mid = (l as f64 + r as f64) / 2.0;
                for (f, sum) in c.filters.iter_mut().zip(c.sums.iter_mut()) {
                    let y = run(f, mid);
                    if count {
                        *sum += y * y;
                    }
                }
                if !count {
                    continue;
                }
                c.filled += 1;
                if c.filled == c.len {
                    let n = c.len as f64;
                    c.done.push(c.sums.map(|s| db(s / n).map(|d| d as f32)));
                    (c.sums, c.filled) = ([0.0; 31], 0);
                }
            }
        }
        self.clean = clean;
    }

    /// Each whole moment of the range ([`MOMENT`]), when asked for: what [`masking`] reads.
    pub fn moments(&self) -> Option<&[Moment]> {
        self.bands.as_ref().and_then(|b| b.moments.as_deref())
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
            non_finite: self.non_finite,
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
        let integrated = gated(ABSOLUTE_GATE).and_then(lufs).and_then(|l| gated(ABSOLUTE_GATE.max(l - RELATIVE_GATE))).and_then(lufs);
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

/// Two tracks that mask each other in one band.
#[derive(Clone, Debug, PartialEq)]
pub struct Masked {
    /// The band's centre, as it is named ([`OCTAVES`]).
    pub hz: f64,
    /// The two, the louder first.
    pub tracks: Vec<String>,
    /// Their levels in the band in the moments they mask each other, in dBFS.
    pub db: Vec<f64>,
    /// How much of the range they mask each other for, from 0 to 1: their moments of it.
    pub share: f64,
}

/// Masking, from each track's moments as it reaches the mix (`heard`, by key: [`Meter::moments`],
/// the same moments of the range for each). In each moment, the tracks that sound in it (louder
/// than [`AUDIBLE`]); in each band, those of them louder there than [`AUDIBLE`] and less than
/// [`LEAK`] under a band beside it; any two of those within [`CLOSE`] dB of the loudest of them mask
/// each other in that moment. Two tracks that never sound at once never mask each other, though a
/// band-pass rings on a moment after a track stops. Each pair, band by band, the most of the range
/// first: how much of the range it holds, and their levels in the moments it does.
pub fn masking(heard: &[(&str, &[Moment])]) -> Vec<Masked> {
    let n = heard.iter().map(|h| h.1.len()).min().unwrap_or(0);
    // By band and pair (their places in `heard`): the moments they mask each other in, and each
    // one's power summed over them.
    let mut met: std::collections::BTreeMap<(usize, usize, usize), (usize, f64, f64)> = Default::default();
    let mut here: Vec<(usize, f64)> = vec![];
    for w in 0..n {
        for b in 0..OCTAVES.len() {
            here.clear();
            for (i, (_, m)) in heard.iter().enumerate() {
                if db(m[w].all as f64).is_none_or(|l| l < AUDIBLE) {
                    continue;
                }
                let x = &m[w].bands;
                let Some(level) = db(x[b] as f64) else { continue };
                let beside = [b.wrapping_sub(1), b + 1].into_iter().filter_map(|c| x.get(c).and_then(|&e| db(e as f64)));
                if level >= AUDIBLE && beside.fold(f64::NEG_INFINITY, f64::max) - LEAK < level {
                    here.push((i, level));
                }
            }
            let loudest = here.iter().map(|h| h.1).fold(f64::NEG_INFINITY, f64::max);
            here.retain(|h| h.1 >= loudest - CLOSE);
            for (k, &(i, _)) in here.iter().enumerate() {
                for &(j, _) in &here[k + 1..] {
                    let (i, j) = (i.min(j), i.max(j));
                    let e = met.entry((b, i, j)).or_default();
                    e.0 += 1;
                    e.1 += heard[i].1[w].bands[b] as f64;
                    e.2 += heard[j].1[w].bands[b] as f64;
                }
            }
        }
    }
    let mut found: Vec<(usize, Masked)> = met
        .into_iter()
        .map(|((b, i, j), (moments, ei, ej))| {
            let level = |e: f64| db(e / moments as f64).unwrap_or(f64::NEG_INFINITY);
            let (mut a, mut c) = ((heard[i].0, level(ei)), (heard[j].0, level(ej)));
            if c.1 > a.1 || (c.1 == a.1 && c.0 < a.0) {
                std::mem::swap(&mut a, &mut c);
            }
            let share = moments as f64 / n as f64;
            (b, Masked { hz: OCTAVES[b], tracks: vec![a.0.to_string(), c.0.to_string()], db: vec![a.1, c.1], share })
        })
        .collect();
    found.sort_by(|(b, x), (c, y)| b.cmp(c).then(y.share.total_cmp(&x.share)).then(x.tracks.cmp(&y.tracks)));
    found.into_iter().map(|f| f.1).collect()
}

#[cfg(test)]
#[path = "listen_tests.rs"]
mod tests;
