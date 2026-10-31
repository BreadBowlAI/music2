//! The effects behind the editor's effect menu: reverb, chorus, phaser, tremolo,
//! compressor, three-band EQ and stereo widener. Port order must match `std/m2pkg.toml`. The
//! bitcrusher is `@m2/effects`' own kernel since std 0.8 (`packs/@m2/effects/kernels-src`).
//!
//! Each is safe to feed anything. Audio that isn't finite counts as silence and absurd
//! levels are capped before they reach any state; controls are clamped to their useful
//! range (NaN takes the low end); recursive state is flushed to zero far below hearing,
//! so tails end in exact silence instead of crawling through denormals; and controls
//! that shape the sound are smoothed, so turning a knob doesn't click. Controls are read
//! every sample and nothing depends on block boundaries, so the output is the same
//! however the host splits its blocks.
//!
//! Chorus, phaser and tremolo follow the song, not the engine: their last input, `playing`, is 1
//! while the song plays (@m2/effects passes `time.playing`; left out it is 1). Stopped, the LFOs
//! wait where they are. So after the engine sat stopped they play the export, bit for bit
//! (RENDER-33).

use std::f32::consts::{FRAC_PI_2, TAU};

use m2_dsp::fx::{Coef, Filter, Glide, Line, Phase, clampf, flush, prewarp, sane};
use m2_kernel::{Cfg, Io, Kernel, db_to_gain, export};

// ---------------------------------------------------------------- reverb

/// Freeverb's comb and allpass lengths at 44.1 kHz, scaled to the sample rate. The right
/// channel's lines are `SPREAD` samples longer so the two sides decorrelate.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;
/// Lows below this stay out of the reverb, so it doesn't turn bass into mud (and DC
/// never builds up in the combs).
const REVERB_CUT: f32 = 200.0;
/// Scales the tank's output so the fully wet sound is about as loud as the dry sound.
const REVERB_WET: f32 = 0.05;

struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    fn tick(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = flush(out * (1.0 - damp) + self.store * damp);
        self.buf[self.pos] = flush(x + self.store * feedback);
        self.pos = if self.pos + 1 == self.buf.len() { 0 } else { self.pos + 1 };
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    fn tick(&mut self, x: f32) -> f32 {
        let b = self.buf[self.pos];
        self.buf[self.pos] = flush(x + b * 0.5);
        self.pos = if self.pos + 1 == self.buf.len() { 0 } else { self.pos + 1 };
        b - x
    }
}

/// Freeverb-style reverb: eight damped combs and four allpasses per side, fed from the
/// mono sum. A stereo input gets a stereo tail (the sides decorrelated); a mono input
/// stays mono, so the reverb fits anywhere a mono sound does (a panned mixer channel).
/// Inputs: in, size, damping, mix (0..1, an equal-power crossfade, since the reverb is
/// uncorrelated with the dry sound).
pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    send: Filter,
    send_state: [f32; 2],
    size: Glide,
    damping: Glide,
    mix: Glide,
    /// Dry and wet gains for `gains_at`, the mix they were computed for.
    gains: (f32, f32),
    gains_at: f32,
    /// Samples it takes sound to pass through the longest comb and the allpasses.
    tail: usize,
    /// Samples since anything audible came out.
    quiet: usize,
}

impl Reverb {
    /// The state `new` makes, around zeroed lines of the lengths it gives them.
    fn around(sr: f32, combs: [Vec<Comb>; 2], allpasses: [Vec<Allpass>; 2]) -> Self {
        // The right side's longest comb and its allpasses.
        let tail = combs[1][7].buf.len() + allpasses[1].iter().map(|a| a.buf.len()).sum::<usize>();
        Reverb {
            combs,
            allpasses,
            send: Filter::highpass(sr, REVERB_CUT, 0.707),
            send_state: [0.0; 2],
            size: Glide::new(0.05, sr),
            damping: Glide::new(0.05, sr),
            mix: Glide::new(0.02, sr),
            gains: (1.0, 0.0),
            gains_at: f32::NAN,
            tail,
            quiet: usize::MAX,
        }
    }
}

impl Kernel for Reverb {
    const INPUTS: usize = 4;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        let len = |l: usize, spread: usize| (((l + spread) as f32 * sr / 44_100.0) as usize).max(1);
        let combs = |spread| -> Vec<Comb> { COMBS.iter().map(|&l| Comb { buf: vec![0.0; len(l, spread)], pos: 0, store: 0.0 }).collect() };
        let allpasses =
            |spread| -> Vec<Allpass> { ALLPASSES.iter().map(|&l| Allpass { buf: vec![0.0; len(l, spread)], pos: 0 }).collect() };
        Reverb::around(sr, [combs(0), combs(SPREAD)], [allpasses(0), allpasses(SPREAD)])
    }

    /// As `new` leaves it, its lines cleared in place: a reset takes no memory.
    fn reset(&mut self, cfg: &Cfg) {
        let (mut combs, mut allpasses) = (std::mem::take(&mut self.combs), std::mem::take(&mut self.allpasses));
        for c in combs.iter_mut().flatten() {
            c.buf.fill(0.0);
            (c.pos, c.store) = (0, 0.0);
        }
        for a in allpasses.iter_mut().flatten() {
            a.buf.fill(0.0);
            a.pos = 0;
        }
        *self = Reverb::around(cfg.sample_rate.max(1.0), combs, allpasses);
    }

    fn process(&mut self, io: &mut Io) {
        let sides = io.channels.min(2);
        let ([l, r, size, damping, mix], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0)], [(0, 0), (0, 1)]);
        for i in 0..ol.len() {
            let (xl, xr) = (sane(l[i]), if sides == 2 { sane(r[i]) } else { 0.0 });
            let feedback = 0.7 + 0.28 * self.size.next(clampf(size[i], 0.0, 1.0));
            let damp = 0.4 * self.damping.next(clampf(damping[i], 0.0, 1.0));
            let m = self.mix.next(clampf(mix[i], 0.0, 1.0));
            if m != self.gains_at {
                self.gains_at = m;
                self.gains = ((m * FRAC_PI_2).cos(), (m * FRAC_PI_2).sin());
            }
            let input = self.send.tick(&mut self.send_state, if sides == 2 { (xl + xr) * 0.5 } else { xl });
            let mut wet = [0.0; 2];
            for (c, w) in wet.iter_mut().enumerate().take(sides) {
                let mut acc = 0.0;
                for comb in &mut self.combs[c] {
                    acc += comb.tick(input, feedback, damp);
                }
                for ap in &mut self.allpasses[c] {
                    acc = ap.tick(acc);
                }
                *w = acc * REVERB_WET;
            }
            let (dry, wet_gain) = self.gains;
            ol[i] = xl * dry + wet[0] * wet_gain;
            or[i] = xr * dry + wet[1] * wet_gain;
            let loud = wet[0].abs().max(wet[1].abs()) > 1e-5 || input.abs() > 1e-5;
            self.quiet = if loud { 0 } else { self.quiet.saturating_add(1) };
        }
    }

    /// The tail keeps sounding after the input stops.
    fn active(&self) -> bool {
        self.quiet < self.tail
    }
}

export!(reverb, Reverb);

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

const PHASER_STAGES: usize = 6;
/// The sweep's centre, and how many octaves it moves either way at full depth.
const PHASER_CENTER: f32 = 640.0;
const PHASER_OCTAVES: f32 = 2.5;
/// Feedback at `feedback` = 1. Below 1 the loop always decays: peaks reach 10 dB at most.
const PHASER_MAX_FEEDBACK: f32 = 0.7;

/// Six first-order allpasses swept by a sine LFO, with feedback. Mixed with the dry
/// sound they cut moving notches; stereo sides sweep a quarter cycle apart.
/// Inputs: in, rate (Hz), depth, feedback, mix (0.5 gives the deepest notches), playing.
pub struct Phaser {
    sr: f32,
    lfo: Phase,
    stages: [[f32; PHASER_STAGES]; 2],
    last: [f32; 2],
    depth: Glide,
    feedback: Glide,
    mix: Glide,
}

impl Kernel for Phaser {
    const INPUTS: usize = 6;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        Phaser {
            sr,
            lfo: Phase { sr, at: 0.0 },
            stages: [[0.0; PHASER_STAGES]; 2],
            last: [0.0; 2],
            depth: Glide::new(0.05, sr),
            feedback: Glide::new(0.05, sr),
            mix: Glide::new(0.02, sr),
        }
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.min(2);
        let ([l, r, rate, depth, feedback, mix, playing], [ol, or]) =
            io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)], [(0, 0), (0, 1)]);
        let (ins, outs) = ([l, r], [ol, or]);
        for i in 0..ins[0].len() {
            let d = self.depth.next(clampf(depth[i], 0.0, 1.0));
            let fb = PHASER_MAX_FEEDBACK * self.feedback.next(clampf(feedback[i], 0.0, 1.0));
            let m = self.mix.next(clampf(mix[i], 0.0, 1.0));
            let (sin, cos) = (TAU * self.lfo.next(rate[i], 20.0, playing[i])).sin_cos();
            for c in 0..2 {
                if c >= channels {
                    outs[c][i] = 0.0;
                    continue;
                }
                let lfo = if c == 0 { sin } else { cos };
                let g = prewarp(PHASER_CENTER * (PHASER_OCTAVES * d * lfo).exp2(), self.sr);
                let a = (g - 1.0) / (g + 1.0);
                let x = sane(ins[c][i]);
                let mut y = x + fb * self.last[c];
                for s in &mut self.stages[c] {
                    let o = a * y + *s;
                    *s = flush(y - a * o);
                    y = o;
                }
                self.last[c] = flush(y);
                outs[c][i] = x * (1.0 - m) + y * m;
            }
        }
    }
}

export!(phaser, Phaser);

/// Tremolo: a sine LFO that dips the level by `depth` (1 = down to silence) and starts
/// at full level. Inputs: in, rate (Hz), depth, playing.
pub struct Tremolo {
    lfo: Phase,
    depth: Glide,
}

impl Kernel for Tremolo {
    const INPUTS: usize = 4;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        Tremolo { lfo: Phase { sr, at: 0.0 }, depth: Glide::new(0.02, sr) }
    }

    fn process(&mut self, io: &mut Io) {
        let stereo = io.channels > 1;
        let ([l, r, rate, depth, playing], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0)], [(0, 0), (0, 1)]);
        for i in 0..ol.len() {
            let d = self.depth.next(clampf(depth[i], 0.0, 1.0));
            let g = 1.0 - d * (0.5 - 0.5 * (TAU * self.lfo.next(rate[i], 40.0, playing[i])).cos());
            ol[i] = sane(l[i]) * g;
            or[i] = if stereo { sane(r[i]) * g } else { 0.0 };
        }
    }
}

export!(tremolo, Tremolo);

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

// ---------------------------------------------------------------- stereo

/// Past width 1, the widener adds a side signal made from the middle `WIDEN_DELAY`
/// seconds ago, highpassed at `WIDEN_CUT` Hz so bass stays centred, at up to
/// `WIDEN_SYNTH` (at width 2). That widens mono sounds too, and since it is pure side
/// signal it cancels exactly when the result is summed to mono.
const WIDEN_DELAY: f32 = 0.011;
const WIDEN_CUT: f32 = 300.0;
const WIDEN_SYNTH: f32 = 0.6;

/// Stereo width via mid/side: 0 is mono, 1 leaves the sound unchanged, 2 is twice as
/// wide. Inputs: in (stereo), width. Output: out (stereo).
pub struct Widen {
    line: Line,
    back: usize,
    /// A one-pole TPT highpass: its gain and state.
    cut: f32,
    cut_state: f32,
    width: Glide,
}

impl Widen {
    /// The state `new` makes, around a zeroed line.
    fn around(sr: f32, line: Line) -> Self {
        let back = ((WIDEN_DELAY * sr) as usize).max(1);
        let g = prewarp(WIDEN_CUT, sr);
        Widen { line, back, cut: g / (1.0 + g), cut_state: 0.0, width: Glide::new(0.02, sr) }
    }
}

impl Kernel for Widen {
    const INPUTS: usize = 2;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        Widen::around(sr, Line::new(((WIDEN_DELAY * sr) as usize).max(1) + 1))
    }

    /// As `new` leaves it, its line cleared in place: a reset takes no memory.
    fn reset(&mut self, cfg: &Cfg) {
        let mut line = std::mem::take(&mut self.line);
        line.clear();
        *self = Widen::around(cfg.sample_rate.max(1.0), line);
    }

    fn process(&mut self, io: &mut Io) {
        let ([l, r, width], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0)], [(0, 0), (0, 1)]);
        for i in 0..ol.len() {
            let (xl, xr) = (sane(l[i]), sane(r[i]));
            let (mid, side) = ((xl + xr) * 0.5, (xl - xr) * 0.5);
            self.line.push(mid);
            let late = self.line.at(self.back);
            let v = (late - self.cut_state) * self.cut;
            let low = v + self.cut_state;
            self.cut_state = flush(low + v);
            let w = self.width.next(clampf(width[i], 0.0, 2.0));
            let s = side * w + (w - 1.0).max(0.0) * WIDEN_SYNTH * (late - low);
            ol[i] = mid + s;
            or[i] = mid - s;
        }
    }
}

export!(widen, Widen);

#[cfg(test)]
mod tests;
