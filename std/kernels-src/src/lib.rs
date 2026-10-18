//! The kernels of the `std` package. Port order must match `std/m2pkg.toml`.
//!
//! No kernel allocates in `process`.

use std::f32::consts::{PI, SQRT_2, TAU};

use m2_kernel::{Cfg, Io, Kernel, abi, db_to_gain, export};

abi!();

// ---------------------------------------------------------------- oscillators

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Band-limited (PolyBLEP) oscillator. `W`: 0 sine, 1 saw, 2 square, 3 triangle.
/// Inputs: freq [, width for square]. Output: out.
pub struct Osc<const W: u8> {
    sr: f32,
    phase: f32,
}

impl<const W: u8> Osc<W> {
    fn run(&mut self, freq: &[f32], width: Option<&[f32]>, out: &mut [f32]) {
        for i in 0..out.len() {
            let dt = (freq[i] / self.sr).clamp(0.0, 0.49);
            let t = self.phase;
            out[i] = match W {
                0 => (t * TAU).sin(),
                1 => 2.0 * t - 1.0 - poly_blep(t, dt),
                2 => {
                    let w = width.map_or(0.5, |w| w[i].clamp(0.05, 0.95));
                    let mut sq = if t < w { 1.0 } else { -1.0 };
                    sq += poly_blep(t, dt);
                    sq -= poly_blep((t + 1.0 - w).fract(), dt);
                    sq
                }
                _ => 1.0 - 4.0 * (t - 0.5).abs(),
            };
            self.phase += dt;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

impl<const W: u8> Kernel for Osc<W> {
    const INPUTS: usize = if W == 2 { 2 } else { 1 };
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        Osc { sr: sample_rate, phase: 0.0 }
    }

    fn process(&mut self, io: &mut Io) {
        if W == 2 {
            let ([freq, width], [out]) = io.ports([(0, 0), (1, 0)], [(0, 0)]);
            self.run(freq, Some(width), out);
        } else {
            let ([freq], [out]) = io.ports([(0, 0)], [(0, 0)]);
            self.run(freq, None, out);
        }
    }
}

export!(sine, Osc<0>);
export!(saw, Osc<1>);
export!(square, Osc<2>);
export!(tri, Osc<3>);

/// White noise (xorshift32). Inputs: seed (read once). Output: out.
pub struct Noise(Option<u32>);

impl Kernel for Noise {
    const INPUTS: usize = 1;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Noise(None)
    }

    fn process(&mut self, io: &mut Io) {
        let mut x = *self.0.get_or_insert((io.first(0) as u32).max(1));
        for o in io.output(0) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *o = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
        }
        self.0 = Some(x);
    }
}

export!(noise, Noise);

// ---------------------------------------------------------------- envelopes

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

/// ADSR driven by a gate. Inputs: gate, attack, decay, sustain, release. Output: out (0..1).
pub struct Adsr {
    sr: f32,
    stage: Stage,
    value: f32,
    gate: bool,
}

fn decay_coef(time: f32, sr: f32) -> f32 {
    // Reach about -60 dB in `time` seconds.
    (-6.9 / (time.max(0.0005) * sr)).exp()
}

impl Kernel for Adsr {
    const INPUTS: usize = 5;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        Adsr { sr: sample_rate, stage: Stage::Idle, value: 0.0, gate: false }
    }

    fn process(&mut self, io: &mut Io) {
        let ([gate, a, d, s, r], [out]) = io.ports([(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)], [(0, 0)]);
        for i in 0..out.len() {
            let on = gate[i] > 0.5;
            if on != self.gate {
                self.gate = on;
                self.stage = if on { Stage::Attack } else { Stage::Release };
            }
            let sustain = s[i].clamp(0.0, 1.0);
            match self.stage {
                Stage::Idle => self.value = 0.0,
                Stage::Attack => {
                    self.value += 1.0 / (a[i].max(0.0005) * self.sr);
                    if self.value >= 1.0 {
                        self.value = 1.0;
                        self.stage = Stage::Decay;
                    }
                }
                Stage::Decay => self.value = sustain + (self.value - sustain) * decay_coef(d[i], self.sr),
                Stage::Release => {
                    self.value *= decay_coef(r[i], self.sr);
                    if self.value < 1e-5 {
                        self.value = 0.0;
                        self.stage = Stage::Idle;
                    }
                }
            }
            out[i] = self.value;
        }
    }

    /// Only the release holds sound back: attack and decay need the gate, which is off
    /// once a voice's note is over.
    fn active(&self) -> bool {
        self.stage == Stage::Release
    }
}

export!(adsr, Adsr);

/// Envelope follower over all channels. Inputs: in, attack, release. Output: out (0..1).
pub struct Follow {
    sr: f32,
    env: f32,
}

impl Kernel for Follow {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        Follow { sr: sample_rate, env: 0.0 }
    }

    fn process(&mut self, io: &mut Io) {
        let ca = (-1.0 / (io.first(1).max(0.0001) * self.sr)).exp();
        let cr = (-1.0 / (io.first(2).max(0.0001) * self.sr)).exp();
        let stereo = io.channels > 1;
        let ([l, r], [out]) = io.ports([(0, 0), (0, if stereo { 1 } else { 0 })], [(0, 0)]);
        for i in 0..out.len() {
            let peak = l[i].abs().max(r[i].abs());
            let c = if peak > self.env { ca } else { cr };
            self.env = peak + (self.env - peak) * c;
            out[i] = self.env.min(1.0);
        }
    }
}

export!(follow, Follow);

/// Beat-synced LFO. Inputs: beats, period, shape, phase. Output: out (0..1).
/// Shapes: 0 sine, 1 triangle, 2 saw, 3 square.
pub struct Lfo;

impl Kernel for Lfo {
    const INPUTS: usize = 4;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Lfo
    }

    fn process(&mut self, io: &mut Io) {
        let ([beats, period, shape, phase], [out]) = io.ports([(0, 0), (1, 0), (2, 0), (3, 0)], [(0, 0)]);
        for i in 0..out.len() {
            let p = (beats[i] as f64 / period[i].max(1e-4) as f64 + phase[i] as f64).rem_euclid(1.0) as f32;
            out[i] = match shape[i].round() as i32 {
                1 => 1.0 - (2.0 * p - 1.0).abs(),
                2 => p,
                3 => (p < 0.5) as i32 as f32,
                _ => 0.5 - 0.5 * (p * TAU).cos(),
            };
        }
    }
}

export!(lfo, Lfo);

// ---------------------------------------------------------------- math

/// `in × by`, generic over audio and control. Inputs: in, by.
pub struct Scale;

impl Kernel for Scale {
    const INPUTS: usize = 2;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Scale
    }

    fn process(&mut self, io: &mut Io) {
        for ch in 0..io.channels {
            let ([x, by], [out]) = io.ports([(0, ch), (1, 0)], [(0, ch)]);
            for i in 0..out.len() {
                out[i] = x[i] * by[i];
            }
        }
    }
}

export!(scale, Scale);

/// `a + b`, generic over audio and control.
pub struct Add;

impl Kernel for Add {
    const INPUTS: usize = 2;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Add
    }

    fn process(&mut self, io: &mut Io) {
        for ch in 0..io.channels {
            let ([a, b], [out]) = io.ports([(0, ch), (1, ch)], [(0, ch)]);
            for i in 0..out.len() {
                out[i] = a[i] + b[i];
            }
        }
    }
}

export!(add, Add);

/// Crossfade, generic over audio and control: `a` at m = 0, `b` at m = 1. Inputs: a, b, m.
pub struct Blend;

impl Kernel for Blend {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Blend
    }

    fn process(&mut self, io: &mut Io) {
        for ch in 0..io.channels {
            let ([a, b, m], [out]) = io.ports([(0, ch), (1, ch), (2, 0)], [(0, ch)]);
            for i in 0..out.len() {
                out[i] = a[i] + (b[i] - a[i]) * m[i].clamp(0.0, 1.0);
            }
        }
    }
}

export!(blend, Blend);

/// Pitch in semitones (60 = middle C) to frequency.
pub struct Mtof;

impl Kernel for Mtof {
    const INPUTS: usize = 1;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Mtof
    }

    fn process(&mut self, io: &mut Io) {
        let ([p], [out]) = io.ports([(0, 0)], [(0, 0)]);
        for i in 0..out.len() {
            out[i] = 440.0 * ((p[i] - 69.0) / 12.0).exp2();
        }
    }
}

export!(mtof, Mtof);

/// Map 0..1 onto lo..hi; `EXP` maps exponentially (for frequencies and times).
/// Inputs: x, lo, hi.
pub struct Range<const EXP: bool>;

impl<const EXP: bool> Kernel for Range<EXP> {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Range
    }

    fn process(&mut self, io: &mut Io) {
        let ([x, lo, hi], [out]) = io.ports([(0, 0), (1, 0), (2, 0)], [(0, 0)]);
        for i in 0..out.len() {
            let t = x[i].clamp(0.0, 1.0);
            out[i] = if EXP && lo[i] > 0.0 && hi[i] > 0.0 { lo[i] * (hi[i] / lo[i]).powf(t) } else { lo[i] + (hi[i] - lo[i]) * t };
        }
    }
}

export!(range, Range<false>);
export!(xrange, Range<true>);

// ---------------------------------------------------------------- mixing

/// One-pole smoother so gain changes don't click. Starts at its first target.
struct Smooth {
    value: f32,
    coef: f32,
    primed: bool,
}

impl Smooth {
    fn new(time: f32, sr: f32) -> Self {
        Smooth { value: 0.0, coef: (-1.0 / (time * sr)).exp(), primed: false }
    }

    fn next(&mut self, target: f32) -> f32 {
        if !self.primed {
            self.primed = true;
            self.value = target;
        }
        self.value = target + (self.value - target) * self.coef;
        self.value
    }

    /// Whether a step toward `target` gives back the value it holds, to the bit: it has settled.
    /// Rounding can stop it a little short of `target`, so this checks the step itself.
    fn settled(&self, target: f32) -> bool {
        self.primed && (target + (self.value - target) * self.coef).to_bits() == self.value.to_bits()
    }
}

/// Gain in dB, smoothed. Inputs: in, db.
///
/// Every mixer channel has one, its fader, whose dB mostly holds still. So a block whose dB holds
/// still, once the smoother has settled on it, is multiplied by the one gain the smoother holds:
/// the bits the per-sample loop gives, without a `powf` and a smoother step on every sample.
pub struct Gain {
    smooth: Smooth,
    /// The last dB read, and its gain: `db_to_gain` runs again only when the dB changes.
    db: f32,
    gain: f32,
}

impl Gain {
    /// The gain of `db`, kept from the last time when it is the same dB.
    fn target(&mut self, db: f32) -> f32 {
        if db.to_bits() != self.db.to_bits() {
            self.db = db;
            self.gain = db_to_gain(db);
        }
        self.gain
    }

    /// The one gain every sample of this block gets, if there is one: the same dB all block,
    /// and the smoother settled on its gain.
    fn steady(&mut self, db: &[f32]) -> Option<f32> {
        let first = *db.first()?;
        if db.iter().any(|d| d.to_bits() != first.to_bits()) {
            return None;
        }
        let target = self.target(first);
        self.smooth.settled(target).then_some(self.smooth.value)
    }

    /// One block: `l` and `r` times the smoothed gain of `db`. Mono (`r` None) leaves `or` silent.
    fn block(&mut self, l: &[f32], r: Option<&[f32]>, db: &[f32], ol: &mut [f32], or: &mut [f32]) {
        if let Some(g) = self.steady(db) {
            for (o, x) in ol.iter_mut().zip(l) {
                *o = x * g;
            }
            match r {
                Some(r) => or.iter_mut().zip(r).for_each(|(o, x)| *o = x * g),
                None => or.fill(0.0),
            }
            return;
        }
        for i in 0..ol.len() {
            let target = self.target(db[i]);
            let g = self.smooth.next(target);
            ol[i] = l[i] * g;
            or[i] = r.map_or(0.0, |r| r[i] * g);
        }
    }
}

impl Kernel for Gain {
    const INPUTS: usize = 2;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        // The dB kept starts as NaN, with NaN's own gain, so it always has its gain beside it.
        Gain { smooth: Smooth::new(0.005, sample_rate), db: f32::NAN, gain: db_to_gain(f32::NAN) }
    }

    fn process(&mut self, io: &mut Io) {
        let stereo = io.channels > 1;
        let ([l, r, db], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0)], [(0, 0), (0, 1)]);
        self.block(l, stereo.then_some(r), db, ol, or);
    }
}

export!(gain, Gain);

#[cfg(test)]
mod tests;

/// Pan, stereo out. Inputs: in, pan (-1..1). A mono input is panned with equal power; a
/// stereo one (a reverb, a widened sound) is balanced: the far side fades out with equal
/// power, the near side stays as it is, so the centre leaves it unchanged.
pub struct Pan;

impl Kernel for Pan {
    const INPUTS: usize = 2;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Pan
    }

    fn process(&mut self, io: &mut Io) {
        if io.channels == 1 {
            let ([x, pan], [l, r]) = io.ports([(0, 0), (1, 0)], [(0, 0), (0, 1)]);
            for i in 0..x.len() {
                let a = (pan[i].clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
                l[i] = x[i] * a.cos() * SQRT_2;
                r[i] = x[i] * a.sin() * SQRT_2;
            }
            return;
        }
        let ([xl, xr, pan], [l, r]) = io.ports([(0, 0), (0, 1), (1, 0)], [(0, 0), (0, 1)]);
        for i in 0..l.len() {
            let p = pan[i].clamp(-1.0, 1.0);
            l[i] = xl[i] * if p > 0.0 { (p * 0.5 * PI).cos() } else { 1.0 };
            r[i] = xr[i] * if p < 0.0 { (p * 0.5 * PI).cos() } else { 1.0 };
        }
    }
}

export!(pan, Pan);

/// Stereo out, unchanged. Input: in, a stereo port, so a mono signal arrives in both channels.
/// What follows it is stereo from the start: std's master puts it first, so a mono mix that
/// turns stereo (its first pan) keeps the master's gain and limiter (design-8b §5.1).
pub struct Stereo;

impl Kernel for Stereo {
    const INPUTS: usize = 1;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Stereo
    }

    fn process(&mut self, io: &mut Io) {
        let ([l, r], [ol, or]) = io.ports([(0, 0), (0, 1)], [(0, 0), (0, 1)]);
        ol.copy_from_slice(l);
        or.copy_from_slice(r);
    }
}

export!(stereo, Stereo);

// ---------------------------------------------------------------- filters

/// TPT state-variable filter (Cytomic). Inputs: in, cutoff, q. Outputs: lp, bp, hp.
pub struct Svf {
    sr: f32,
    ic1: [f32; 2],
    ic2: [f32; 2],
}

impl Kernel for Svf {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 3;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        Svf { sr: sample_rate, ic1: [0.0; 2], ic2: [0.0; 2] }
    }

    fn process(&mut self, io: &mut Io) {
        for ch in 0..io.channels {
            let ([x, cutoff, q], [lp, bp, hp]) = io.ports([(0, ch), (1, 0), (2, 0)], [(0, ch), (1, ch), (2, ch)]);
            for i in 0..x.len() {
                let fc = cutoff[i].clamp(16.0, self.sr * 0.45);
                let g = (PI * fc / self.sr).tan();
                let k = 1.0 / q[i].max(0.3);
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;
                let v3 = x[i] - self.ic2[ch];
                let v1 = a1 * self.ic1[ch] + a2 * v3;
                let v2 = self.ic2[ch] + a2 * self.ic1[ch] + a3 * v3;
                self.ic1[ch] = 2.0 * v1 - self.ic1[ch];
                self.ic2[ch] = 2.0 * v2 - self.ic2[ch];
                lp[i] = v2;
                bp[i] = v1;
                hp[i] = x[i] - k * v1 - v2;
            }
        }
    }
}

export!(svf, Svf);

// ---------------------------------------------------------------- time effects

/// Longest delay time, in seconds.
const MAX_DELAY: f32 = 2.0;

/// Feedback delay with a dry/wet mix. Inputs: in, time, feedback, mix.
pub struct Delay {
    sr: f32,
    lines: Vec<Vec<f32>>,
    pos: usize,
    /// Samples since anything audible was written into a line.
    quiet: usize,
}

impl Kernel for Delay {
    const INPUTS: usize = 4;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let (sample_rate, channels) = (cfg.sample_rate, cfg.channels);
        let len = (MAX_DELAY * sample_rate) as usize + 2;
        Delay { sr: sample_rate, lines: vec![vec![0.0; len]; channels], pos: 0, quiet: usize::MAX }
    }

    /// As `new` leaves it, its lines cleared in place: a reset takes no memory.
    fn reset(&mut self, _: &Cfg) {
        self.lines.iter_mut().for_each(|l| l.fill(0.0));
        (self.pos, self.quiet) = (0, usize::MAX);
    }

    fn process(&mut self, io: &mut Io) {
        let len = self.lines[0].len();
        let start = self.pos;
        let mut quietest = usize::MAX;
        for ch in 0..self.lines.len() {
            let ([x, time, fb, mix], [out]) = io.ports([(0, ch), (1, 0), (2, 0), (3, 0)], [(0, ch)]);
            let line = &mut self.lines[ch];
            let mut pos = start;
            let mut quiet = self.quiet;
            for i in 0..out.len() {
                let d = (time[i].clamp(0.0, MAX_DELAY) * self.sr).max(1.0);
                let back = d.floor() as usize;
                let frac = d - back as f32;
                let a = line[(pos + len - back) % len];
                let b = line[(pos + len - back - 1) % len];
                let wet = a + (b - a) * frac;
                let w = x[i] + wet * fb[i].clamp(0.0, 0.95);
                line[pos] = w;
                quiet = if w.abs() > 1e-5 { 0 } else { quiet.saturating_add(1) };
                let m = mix[i].clamp(0.0, 1.0);
                out[i] = x[i] * (1.0 - m) + wet * m;
                pos = (pos + 1) % len;
            }
            quietest = quietest.min(quiet);
            if ch + 1 == self.lines.len() {
                self.pos = pos;
            }
        }
        self.quiet = quietest;
    }

    fn active(&self) -> bool {
        self.quiet < self.lines[0].len()
    }
}

export!(delay, Delay);

// ---------------------------------------------------------------- dynamics and colour

/// Soft saturation, peak-normalised. Inputs: in, amount (dB of drive).
pub struct Drive;

impl Kernel for Drive {
    const INPUTS: usize = 2;
    const OUTPUTS: usize = 1;

    fn new(_: &Cfg) -> Self {
        Drive
    }

    fn process(&mut self, io: &mut Io) {
        for ch in 0..io.channels {
            let ([x, amount], [out]) = io.ports([(0, ch), (1, 0)], [(0, ch)]);
            for i in 0..out.len() {
                let g = db_to_gain(amount[i].max(0.0));
                out[i] = (x[i] * g).tanh() / g.tanh();
            }
        }
    }
}

export!(drive, Drive);

/// Sidechain ducking. Inputs: in, side (0..1, e.g. a follower), depth (dB at side = 1).
pub struct Duck(Smooth);

impl Kernel for Duck {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        Duck(Smooth::new(0.001, sample_rate))
    }

    fn process(&mut self, io: &mut Io) {
        let stereo = io.channels > 1;
        let ([l, r, side, depth], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0), (2, 0)], [(0, 0), (0, 1)]);
        for i in 0..ol.len() {
            let g = self.0.next(db_to_gain(depth[i].min(0.0) * side[i].clamp(0.0, 1.0)));
            ol[i] = l[i] * g;
            or[i] = if stereo { r[i] * g } else { 0.0 };
        }
    }
}

export!(duck, Duck);

/// Peak limiter with instant attack and a hard ceiling. Inputs: in, ceiling (dB).
pub struct Limit {
    sr: f32,
    env: f32,
}

impl Kernel for Limit {
    const INPUTS: usize = 2;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        Limit { sr: sample_rate, env: 0.0 }
    }

    fn process(&mut self, io: &mut Io) {
        let release = (-1.0 / (0.08 * self.sr)).exp();
        let stereo = io.channels > 1;
        let ceiling = db_to_gain(io.first(1).min(0.0));
        let ([l, r], [ol, or]) = io.ports([(0, 0), (0, 1)], [(0, 0), (0, 1)]);
        for i in 0..ol.len() {
            let peak = if stereo { l[i].abs().max(r[i].abs()) } else { l[i].abs() };
            self.env = if peak > self.env { peak } else { peak + (self.env - peak) * release };
            let g = if self.env > ceiling { ceiling / self.env } else { 1.0 };
            ol[i] = (l[i] * g).clamp(-ceiling, ceiling);
            or[i] = if stereo { (r[i] * g).clamp(-ceiling, ceiling) } else { 0.0 };
        }
    }
}

export!(limit, Limit);

// ---------------------------------------------------------------- effects

mod fx;

pub use fx::{Bitcrush, Chorus, Compress, Eq3, Phaser, Reverb, Tremolo, Widen};

// ---------------------------------------------------------------- instruments

mod drums;
mod fm;
mod notes;
mod synth;
#[cfg(test)]
mod testkit;
mod voice;

pub use drums::Drum;
pub use fm::Fm;
pub use notes::{DrumHit, HitCount, Mono, NoteCv};
pub use synth::Synth;
