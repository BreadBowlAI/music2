//! std's effects: the chorus (which `@m2/effects` and `@m2/synths`' strings both play), and what
//! mixing needs: the compressor and the three-band EQ (PKG-17). Port order must match
//! `std/m2pkg.toml`. Reverb, phaser, tremolo and widen are `@m2/effects`' own kernels since std 0.9,
//! the bitcrusher since std 0.8 (`packs/@m2/effects/kernels-src`, PACK-21).
//!
//! Each is safe to feed anything, by `m2_dsp`'s house rules: audio that isn't finite counts as
//! silence and absurd levels are capped before they reach any state; controls are clamped to their
//! useful range (NaN takes the low end); recursive state is flushed to zero far below hearing, so
//! tails end in exact silence instead of crawling through denormals; and controls that shape the
//! sound are smoothed, so turning a knob doesn't click. Controls are read every sample and nothing
//! depends on block boundaries, so the output is the same however the host splits its blocks.
//!
//! The chorus follows the song, not the engine: its last input, `playing`, is 1 while the song
//! plays (@m2/effects passes `time.playing`; left out it is 1). Stopped, its LFO waits where it is.
//! So after the engine sat stopped it plays the export, bit for bit (RENDER-33).

use std::f32::consts::TAU;

use m2_dsp::{Coef, Filter, Glide, Line, Phase, clampf, flush, sane};
use m2_kernel::{Cfg, Io, Kernel, db_to_gain, export};

// ---------------------------------------------------------------- modulation

/// Each chorus tap sweeps between `CHORUS_BASE` and `CHORUS_BASE + depth × CHORUS_SWEEP`
/// seconds: at full depth and 1 Hz that bends pitch by about ±30 cents.
const CHORUS_BASE: f32 = 0.008;
const CHORUS_SWEEP: f32 = 0.006;

/// Chorus: two copies of the sound on slowly sweeping delays, swinging in opposite
/// directions. On a stereo input the right side sweeps a quarter cycle behind the left,
/// which spreads even a centred sound wide. Inputs: in, rate (Hz), depth, mix, playing.
pub struct Chorus {
    sr: f32,
    lines: [Line; 2],
    lfo: Phase,
    depth: Glide,
    mix: Glide,
}

impl Chorus {
    /// The state `new` makes, around zeroed lines.
    fn around(sr: f32, lines: [Line; 2]) -> Self {
        Chorus { sr, lines, lfo: Phase { sr, at: 0.0 }, depth: Glide::new(0.05, sr), mix: Glide::new(0.02, sr) }
    }
}

impl Kernel for Chorus {
    const INPUTS: usize = 5;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        let len = ((CHORUS_BASE + CHORUS_SWEEP) * sr) as usize + 8;
        Chorus::around(sr, [Line::new(len), Line::new(len)])
    }

    /// As `new` leaves it, its lines cleared in place: a reset takes no memory.
    fn reset(&mut self, cfg: &Cfg) {
        let mut lines = std::mem::take(&mut self.lines);
        lines.iter_mut().for_each(Line::clear);
        *self = Chorus::around(cfg.sample_rate.max(1.0), lines);
    }

    fn process(&mut self, io: &mut Io) {
        let sides = io.channels.min(2);
        let ([l, r, rate, depth, mix, playing], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0), (4, 0)], [(0, 0), (0, 1)]);
        let base = CHORUS_BASE * self.sr;
        for i in 0..ol.len() {
            let x = [sane(l[i]), if sides == 2 { sane(r[i]) } else { 0.0 }];
            let sweep = self.depth.next(clampf(depth[i], 0.0, 1.0)) * CHORUS_SWEEP * self.sr;
            let m = self.mix.next(clampf(mix[i], 0.0, 1.0));
            let (sin, cos) = (TAU * self.lfo.next(rate[i], 20.0, playing[i])).sin_cos();
            let mut out = [0.0; 2];
            for (c, lfo) in [(0, sin), (1, cos)].into_iter().take(sides) {
                let line = &mut self.lines[c];
                line.push(x[c]);
                let wet = 0.5 * (line.read(base + sweep * (0.5 + 0.5 * lfo)) + line.read(base + sweep * (0.5 - 0.5 * lfo)));
                out[c] = x[c] * (1.0 - m) + wet * m;
            }
            ol[i] = out[0];
            or[i] = out[1];
        }
    }
}

export!(chorus, Chorus);

// ---------------------------------------------------------------- dynamics

/// Width of the compressor's soft knee, in dB around the threshold.
const KNEE: f32 = 6.0;

/// Feed-forward compressor, stereo-linked, with a soft knee. The gain reduction (in dB)
/// follows the peak level with `attack` and `release`. Inputs: in, threshold (dB),
/// ratio (1 = off), attack (s), release (s), makeup (dB).
pub struct Compress {
    sr: f32,
    /// Gain reduction now, in dB (0 or more).
    env: f32,
    attack: Coef,
    release: Coef,
    makeup: Glide,
}

impl Kernel for Compress {
    const INPUTS: usize = 6;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        Compress { sr, env: 0.0, attack: Coef::new(), release: Coef::new(), makeup: Glide::new(0.02, sr) }
    }

    fn process(&mut self, io: &mut Io) {
        let stereo = io.channels > 1;
        let ([l, r, threshold, ratio, attack, release, makeup], [ol, or]) =
            io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)], [(0, 0), (0, 1)]);
        for i in 0..ol.len() {
            let (xl, xr) = (sane(l[i]), if stereo { sane(r[i]) } else { 0.0 });
            let level = 20.0 * xl.abs().max(xr.abs()).max(1e-6).log10();
            let over = level - clampf(threshold[i], -80.0, 12.0);
            let slope = 1.0 - 1.0 / clampf(ratio[i], 1.0, 50.0);
            let target = if over <= -KNEE / 2.0 {
                0.0
            } else if over < KNEE / 2.0 {
                slope * (over + KNEE / 2.0) * (over + KNEE / 2.0) / (2.0 * KNEE)
            } else {
                slope * over
            };
            let coef = if target > self.env {
                self.attack.get(clampf(attack[i], 0.0001, 1.0), self.sr)
            } else {
                self.release.get(clampf(release[i], 0.001, 5.0), self.sr)
            };
            self.env = flush(target + (self.env - target) * coef);
            let g = db_to_gain(self.makeup.next(clampf(makeup[i], -60.0, 24.0)) - self.env);
            ol[i] = xl * g;
            or[i] = xr * g;
        }
    }
}

export!(compress, Compress);

// ---------------------------------------------------------------- EQ and colour

/// The shelves are maximally flat; the mid bell is broad and musical.
const SHELF_Q: f32 = 0.707;
const BELL_Q: f32 = 0.7;

/// Three-band EQ: a low shelf, a mid bell and a high shelf, each from -60 dB (a DJ-style
/// kill) to +24 dB, flat (and exactly transparent) at 0 dB. Inputs: in, low, mid, high
/// (dB), low_freq, mid_freq, high_freq (Hz).
pub struct Eq3 {
    sr: f32,
    params: [Glide; 6],
    /// The smoothed parameters `bands` were computed for.
    at: [f32; 6],
    bands: [Filter; 3],
    state: [[[f32; 2]; 3]; 2],
}

impl Kernel for Eq3 {
    const INPUTS: usize = 7;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        let flat = Filter::bell(sr, 1000.0, BELL_Q, 0.0);
        Eq3 { sr, params: std::array::from_fn(|_| Glide::new(0.03, sr)), at: [f32::NAN; 6], bands: [flat; 3], state: [[[0.0; 2]; 3]; 2] }
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.min(2);
        let ([l, r, low, mid, high, low_f, mid_f, high_f], [ol, or]) =
            io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0)], [(0, 0), (0, 1)]);
        let (ins, outs) = ([l, r], [ol, or]);
        let nyquist = 0.45 * self.sr;
        for i in 0..ins[0].len() {
            let target = [
                clampf(low[i], -60.0, 24.0),
                clampf(mid[i], -60.0, 24.0),
                clampf(high[i], -60.0, 24.0),
                clampf(low_f[i], 20.0, nyquist),
                clampf(mid_f[i], 20.0, nyquist),
                clampf(high_f[i], 20.0, nyquist),
            ];
            let p: [f32; 6] = std::array::from_fn(|k| self.params[k].next(target[k]));
            if p != self.at {
                self.at = p;
                self.bands = [
                    Filter::low_shelf(self.sr, p[3], SHELF_Q, p[0]),
                    Filter::bell(self.sr, p[4], BELL_Q, p[1]),
                    Filter::high_shelf(self.sr, p[5], SHELF_Q, p[2]),
                ];
            }
            for c in 0..2 {
                if c >= channels {
                    outs[c][i] = 0.0;
                    continue;
                }
                let mut y = sane(ins[c][i]);
                for (band, s) in self.bands.iter().zip(&mut self.state[c]) {
                    y = band.tick(s, y);
                }
                outs[c][i] = y;
            }
        }
    }
}

export!(eq3, Eq3);

#[cfg(test)]
mod tests;
