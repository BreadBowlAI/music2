//! The multiband compressor: three bands split by Linkwitz-Riley crossovers, each compressed on
//! its own, as std's `compress` compresses. Of its two split points, whichever is lower splits the
//! low band from the mid, so the mid band is never empty.
//!
//! A Linkwitz-Riley crossover of 24 dB per octave is two Butterworth filters of 12 dB per octave in
//! a row (Q 1/√2), a low-pass for the band below and a high-pass for the band above. The two sum to
//! an all-pass: flat in level, turned in phase. The low band goes through the upper crossover's
//! all-pass too, so all three sum to the two all-passes one after the other: at ratio 1 and gain 0
//! in every band, the input through two all-passes. Each filter is an SVF (`m2_dsp::fx::Filter`),
//! whose responses are RBJ's.

use std::f32::consts::SQRT_2;

use m2_dsp::fx::{Coef, Filter, Glide, clampf, flush, prewarp, sane};
use m2_kernel::{Cfg, Io, Kernel};

use crate::gain;

/// The compressors' soft knee, as std's `compress` has it.
const KNEE: f32 = 6.0;

/// One crossover's filters: the low-pass, the high-pass, and the all-pass they sum to.
#[derive(Clone, Copy)]
struct Crossover {
    low: Filter,
    high: Filter,
    all: Filter,
}

impl Crossover {
    fn at(hz: f32, sr: f32) -> Crossover {
        let (g, k) = (prewarp(hz, sr), SQRT_2);
        Crossover {
            low: Filter::new(g, k, [0.0, 0.0, 1.0]),
            high: Filter::new(g, k, [1.0, -k, -1.0]),
            all: Filter::new(g, k, [1.0, -2.0 * k, 0.0]),
        }
    }
}

/// One channel's states: each crossover's two low-passes and two high-passes, and the low band's
/// all-pass.
#[derive(Clone, Copy, Default)]
struct States {
    split: [[[f32; 2]; 4]; 2],
    all: [f32; 2],
}

/// One band's compressor: stereo-linked, soft-knee, std's `compress`'s gain computer line for line.
/// It is a copy because std's lives in std's crate, whose source wave 3 is moving: the two become
/// one in `m2-dsp` after it lands (review Z9).
struct Squeeze {
    /// Gain reduction now, in dB (0 or more).
    env: f32,
    attack: Coef,
    release: Coef,
    gain: Glide,
}

impl Squeeze {
    fn new(sr: f32) -> Squeeze {
        Squeeze { env: 0.0, attack: Coef::new(), release: Coef::new(), gain: Glide::new(0.02, sr) }
    }

    /// The gain for a band whose loudest channel is `peak` now, with its controls.
    fn next(&mut self, sr: f32, peak: f32, [threshold, ratio, attack, release, makeup]: [f32; 5]) -> f32 {
        let level = 20.0 * peak.max(1e-6).log10();
        let over = level - clampf(threshold, -80.0, 12.0);
        let slope = 1.0 - 1.0 / clampf(ratio, 1.0, 50.0);
        let target = if over <= -KNEE / 2.0 {
            0.0
        } else if over < KNEE / 2.0 {
            slope * (over + KNEE / 2.0) * (over + KNEE / 2.0) / (2.0 * KNEE)
        } else {
            slope * over
        };
        let coef = if target > self.env {
            self.attack.get(clampf(attack, 0.0001, 1.0), sr)
        } else {
            self.release.get(clampf(release, 0.001, 5.0), sr)
        };
        self.env = flush(target + (self.env - target) * coef);
        gain(self.gain.next(clampf(makeup, -60.0, 24.0)) - self.env)
    }
}

/// Multiband compressor. Inputs: in, low_mid and mid_high (Hz), then for the low, mid and high bands
/// each its threshold (dB), ratio, attack and release (seconds) and gain (dB).
pub struct Multiband {
    sr: f32,
    split: [Glide; 2],
    /// The frequencies the crossovers were made for.
    at: [f32; 2],
    crossovers: [Crossover; 2],
    states: [States; 2],
    bands: [Squeeze; 3],
}

impl Kernel for Multiband {
    const INPUTS: usize = 18;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(1.0);
        Multiband {
            sr,
            split: [Glide::new(0.03, sr), Glide::new(0.03, sr)],
            at: [f32::NAN; 2],
            crossovers: [Crossover::at(200.0, sr), Crossover::at(2500.0, sr)],
            states: [States::default(); 2],
            bands: std::array::from_fn(|_| Squeeze::new(sr)),
        }
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.min(2);
        let ins: [(usize, usize); 19] = std::array::from_fn(|i| if i < 2 { (0, i) } else { (i - 1, 0) });
        let (ins, [ol, or]) = io.ports(ins, [(0, 0), (0, 1)]);
        let mut outs = [ol, or];
        for i in 0..outs[0].len() {
            let nyquist = 0.45 * self.sr;
            let [a, b] = [self.split[0].next(clampf(ins[2][i], 10.0, nyquist)), self.split[1].next(clampf(ins[3][i], 10.0, nyquist))];
            // Split points that cross are put in order: the lower splits low from mid (review Z7).
            let hz = [a.min(b), a.max(b)];
            if hz != self.at {
                self.at = hz;
                self.crossovers = hz.map(|f| Crossover::at(f, self.sr));
            }
            let [lower, upper] = self.crossovers;
            let mut bands = [[0.0f32; 3]; 2];
            for c in 0..channels {
                let x = sane(ins[c][i]);
                let s = &mut self.states[c];
                // Each band through two filters of 12 dB per octave: Linkwitz-Riley's 24.
                let [a, b, c2, d] = &mut s.split[0];
                let low = lower.low.tick(b, lower.low.tick(a, x));
                let rest = lower.high.tick(d, lower.high.tick(c2, x));
                let [a, b, c2, d] = &mut s.split[1];
                let mid = upper.low.tick(b, upper.low.tick(a, rest));
                let high = upper.high.tick(d, upper.high.tick(c2, rest));
                bands[c] = [upper.all.tick(&mut s.all, low), mid, high];
            }
            let mut g = [0.0; 3];
            for (b, band) in self.bands.iter_mut().enumerate() {
                let peak = bands[0][b].abs().max(bands[1][b].abs());
                let controls = std::array::from_fn(|f| ins[4 + b * 5 + f][i]);
                g[b] = band.next(self.sr, peak, controls);
            }
            for (c, out) in outs.iter_mut().enumerate() {
                out[i] = if c < channels { bands[c][0] * g[0] + bands[c][1] * g[1] + bands[c][2] * g[2] } else { 0.0 };
            }
        }
    }
}
