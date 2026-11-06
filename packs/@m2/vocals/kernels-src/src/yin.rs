//! `@m2/vocals:pitch`: a voice's pitch, how sure it is, and its level (VOX-21).
//!
//! YIN (de Cheveigné and Kawahara, 2002), written from the paper. Every [`HOP`] samples, counted
//! from the state's birth, it looks at the last `window + longest + 2` samples of the mono sum of
//! its channels: `window` and `longest` are the longest period it finds, 1/70 s. For each lag τ the
//! difference between the sound and itself τ later, `d(τ) = e(0) + e(τ) − 2·r(τ)`, its two energies
//! from running sums and the correlation `r` through one FFT. Then each `d(τ)` over the mean of those
//! before it (the cumulative mean normalised difference, `d′`), the first lag from 1/1200 s on where
//! `d′` dips under 0.15, followed down to its lowest, else the lowest of all; a parabola through it
//! for the exact period. `hz` is the rate over that period; `clarity` is `1 − d′` there. A dip
//! deeper than [`UNCLEAR`] never counts: no pitch is `hz` 0 and `clarity` 0, and so is silence.
//! `level` is the root mean square of the last `window` samples. Each holds until the next hop.
//!
//! It looks only at what has come in, so it adds no latency: its reading at a moment is of the
//! `window` plus a period before it, centred 8 to 14 ms back, about where `shift`, 15 ms late,
//! plays what it reads it for.

use m2_dsp::fx::sane;
use m2_kernel::{Cfg, Io, Kernel};

use crate::fft::Fft;

/// Samples between two readings, at every rate.
pub const HOP: usize = 256;
/// The lowest and highest pitch it finds, in Hz.
pub const LOWEST: f32 = 70.0;
pub const HIGHEST: f32 = 1200.0;
/// A dip of `d′` under this is a period (YIN's absolute threshold).
const THRESHOLD: f64 = 0.15;
/// A dip no deeper than this is no pitch at all.
const UNCLEAR: f64 = 0.6;
/// Energy a sample under which the window is silence.
const SILENT: f64 = 1e-12;

pub struct Pitch {
    sr: f32,
    /// The shortest and longest period it looks for, in samples.
    shortest: usize,
    longest: usize,
    /// The samples each `d(τ)` sums over.
    window: usize,
    /// The mono sum's last `span` samples, a ring; `at` is where the next goes.
    ring: Vec<f32>,
    at: usize,
    /// Samples since the last reading.
    count: usize,
    fft: Fft,
    /// The transform's room: the sound, the window, and their spectra.
    re: Vec<f64>,
    im: Vec<f64>,
    wre: Vec<f64>,
    wim: Vec<f64>,
    /// Running sums of the squares, and `d′`.
    sq: Vec<f64>,
    dn: Vec<f64>,
    hz: f32,
    clarity: f32,
    level: f32,
}

impl Pitch {
    fn span(&self) -> usize {
        self.ring.len()
    }

    /// One reading, from the ring.
    fn read(&mut self) {
        let (span, n, w, longest) = (self.span(), self.fft.size(), self.window, self.longest);
        // Newest first, so the window is the newest `window` samples, each compared with those
        // a lag before it (YIN is the same either way round in time).
        for i in 0..n {
            let v = if i < span { self.ring[(self.at + span - 1 - i) % span] as f64 } else { 0.0 };
            self.re[i] = v;
            self.im[i] = 0.0;
            self.wre[i] = if i < w { v } else { 0.0 };
            self.wim[i] = 0.0;
        }
        // Running sums of the squares: e(τ) is the energy of the `window` samples from τ.
        self.sq[0] = 0.0;
        for i in 0..span {
            self.sq[i + 1] = self.sq[i] + self.re[i] * self.re[i];
        }
        let energy = |sq: &[f64], from: usize| sq[from + w] - sq[from];
        let e0 = energy(&self.sq, 0);
        self.level = ((e0 / w as f64).sqrt() as f32).min(1e4);
        if e0 < SILENT * w as f64 {
            (self.hz, self.clarity) = (0.0, 0.0);
            return;
        }
        // r(τ) = Σ w[j]·x[j + τ]: the sound's spectrum times the window's, conjugated, back.
        self.fft.run(&mut self.re, &mut self.im, false);
        self.fft.run(&mut self.wre, &mut self.wim, false);
        for k in 0..n {
            let (a, b, c, d) = (self.re[k], self.im[k], self.wre[k], -self.wim[k]);
            self.re[k] = a * c - b * d;
            self.im[k] = a * d + b * c;
        }
        self.fft.run(&mut self.re, &mut self.im, true);
        let scale = 1.0 / n as f64;
        // d′(τ), τ from 0 to longest + 1.
        self.dn[0] = 1.0;
        let mut sum = 0.0;
        for tau in 1..=longest + 1 {
            let d = (e0 + energy(&self.sq, tau) - 2.0 * self.re[tau] * scale).max(0.0);
            sum += d;
            self.dn[tau] = if sum > 0.0 { d * tau as f64 / sum } else { 1.0 };
        }
        let dn = &self.dn;
        let mut found = None;
        let mut tau = self.shortest;
        while tau <= longest {
            if dn[tau] < THRESHOLD {
                while tau < longest && dn[tau + 1] < dn[tau] {
                    tau += 1;
                }
                found = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = found.unwrap_or_else(|| (self.shortest..=longest).min_by(|&a, &b| dn[a].total_cmp(&dn[b])).unwrap_or(longest));
        if dn[tau] > UNCLEAR {
            (self.hz, self.clarity) = (0.0, 0.0);
            return;
        }
        let (l, c, r) = (dn[tau - 1], dn[tau], dn[tau + 1]);
        let bend = l - 2.0 * c + r;
        let exact = tau as f64 + if bend > 0.0 { (0.5 * (l - r) / bend).clamp(-1.0, 1.0) } else { 0.0 };
        let hz = (self.sr as f64 / exact) as f32;
        (self.hz, self.clarity) = match (LOWEST..=HIGHEST).contains(&hz) {
            true => (hz, (1.0 - c).clamp(0.0, 1.0) as f32),
            false => (0.0, 0.0),
        };
    }
}

impl Kernel for Pitch {
    const INPUTS: usize = 1;
    const OUTPUTS: usize = 3;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(8_000.0);
        let longest = (sr / LOWEST).ceil() as usize;
        let shortest = ((sr / HIGHEST).floor() as usize).max(2);
        let window = longest;
        let span = window + longest + 2;
        let fft = Fft::new(span.next_power_of_two());
        let n = fft.size();
        Pitch {
            sr,
            shortest,
            longest,
            window,
            ring: vec![0.0; span],
            at: 0,
            count: 0,
            fft,
            re: vec![0.0; n],
            im: vec![0.0; n],
            wre: vec![0.0; n],
            wim: vec![0.0; n],
            sq: vec![0.0; span + 1],
            dn: vec![0.0; longest + 2],
            hz: 0.0,
            clarity: 0.0,
            level: 0.0,
        }
    }

    /// As `new` leaves it, cleared in place: a reset takes no memory.
    fn reset(&mut self, _: &Cfg) {
        self.ring.fill(0.0);
        (self.at, self.count, self.hz, self.clarity, self.level) = (0, 0, 0.0, 0.0, 0.0);
    }

    fn process(&mut self, io: &mut Io) {
        let stereo = io.channels == 2;
        for t in 0..io.frames {
            let x = match stereo {
                true => 0.5 * (sane(io.input_ch(0, 0)[t]) + sane(io.input_ch(0, 1)[t])),
                false => sane(io.input(0)[t]),
            };
            let span = self.span();
            self.ring[self.at] = x;
            self.at = (self.at + 1) % span;
            self.count += 1;
            if self.count == HOP {
                self.count = 0;
                self.read();
            }
            io.output(0)[t] = self.hz;
            io.output(1)[t] = self.clarity;
            io.output(2)[t] = self.level;
        }
    }
}
