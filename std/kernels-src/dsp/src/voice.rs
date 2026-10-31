//! Building blocks of the instrument voices (`@m2/synths`' `synth` and `fm`, `@m2/drums`' `drum`,
//! and std's Sampler): a fast sine, a band-limited step for oscillators, a 2× decimator, a ladder
//! filter, envelopes, a soft clipper and noise.
//!
//! The house rules are the crate's: audio that isn't finite counts as silence, controls are
//! clamped (NaN takes the low end) and recursive state is flushed far below hearing. A
//! voice also keeps a control clock of its own, a tick every [`TICK`] samples counted from
//! its first sample (never from the host's blocks), so its output is the same however the
//! host splits its blocks. State is fixed-size: a voice's reset is a struct write.

use std::f32::consts::{PI, SQRT_2};

pub use crate::{Coef, Filter, Glide, clampf, flush, prewarp};

/// Samples between control ticks. Values read at a tick are interpolated to the next.
pub const TICK: u32 = 16;

/// Where an envelope ends, at -90 dB: a decay that reaches -60 dB in its time `t` gets
/// here at 1.5 `t`.
pub const END: f32 = 3.162_278e-5;

/// The decay rate that reaches -60 dB in one unit of time (ln 1000).
pub const T60: f32 = 6.907_755;

/// A pitch in semitones that is safe to turn into a frequency: NaN is middle C.
pub fn pitch_of(p: f32) -> f32 {
    if p.is_nan() { 60.0 } else { clampf(p, -48.0, 160.0) }
}

/// Frequency of a pitch in semitones (69 = A 440).
pub fn hz(pitch: f32) -> f32 {
    440.0 * ((pitch - 69.0) / 12.0).exp2()
}

// ---------------------------------------------------------------- maths

/// `sin(2π p)` for a phase `p` in cycles: an odd 9th-order polynomial after folding to a
/// quarter cycle, within 2e-7 of the true sine.
// The first coefficient is fitted, so it is close to 2π but not 2π.
#[allow(clippy::approx_constant)]
pub fn sin2pi(p: f32) -> f32 {
    let x = p - (p + 0.5).floor();
    let x = if x > 0.25 {
        0.5 - x
    } else if x < -0.25 {
        -0.5 - x
    } else {
        x
    };
    let z = x * x;
    x * (6.283_185 + z * (-41.341_656 + z * (81.601_006 + z * (-76.549_78 + z * 39.536_705))))
}

/// A tanh-shaped soft clip (a clamped rational): about `x` for small `x`, exactly ±1 from
/// |x| = 3 on, where its slope reaches 0.
pub fn soft(x: f32) -> f32 {
    let x = clampf(x, -3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// Linear up to |x| = 0.5, then bending smoothly (with [`soft`]'s curve) into ±1 at
/// |x| = 2: sounds at ordinary levels pass untouched, and only peaks saturate.
pub fn knee(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.5 { x } else { (0.5 + 0.5 * soft(2.0 * (a - 0.5))).copysign(x) }
}

/// A hash of two words, for seeds and start phases that sound random but are the same
/// every time.
pub fn hash(a: u32, b: u32) -> u32 {
    let mut x = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_add(0x7F4A_7C15).wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^ (x >> 16)
}

/// White noise, uniform in -1..1 (xorshift32).
#[derive(Clone, Copy)]
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Noise(seed.max(1))
    }

    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x as i32 as f32 * (1.0 / 2_147_483_648.0)
    }
}

/// A function's last result, so a tick only works out again what has moved.
#[derive(Clone, Copy)]
pub struct Memo {
    at: f32,
    value: f32,
}

impl Memo {
    pub fn new() -> Self {
        Memo { at: f32::NAN, value: 0.0 }
    }

    pub fn get(&mut self, x: f32, f: impl FnOnce(f32) -> f32) -> f32 {
        if x != self.at {
            self.at = x;
            self.value = f(x);
        }
        self.value
    }
}

/// A value that moves linearly from tick to tick: set a target at each tick, step once a
/// sample.
#[derive(Clone, Copy, Default)]
pub struct Ramp {
    pub value: f32,
    step: f32,
}

impl Ramp {
    /// Head for `target` over the next tick, or be there at once (a note from silence).
    pub fn to(&mut self, target: f32, jump: bool) {
        if jump {
            self.value = target;
            self.step = 0.0;
        } else {
            self.step = (target - self.value) / TICK as f32;
        }
    }

    pub fn next(&mut self) -> f32 {
        self.value += self.step;
        self.value
    }
}

// ---------------------------------------------------------------- oscillators

/// The four corrections that band-limit a unit step which happened `d` samples (0..1)
/// before the sample it was first seen at: for the two samples before that one, that
/// sample, and the one after. The step is smoothed by a cubic B-spline (an integrated
/// 4-point polyBLEP), which at 2× leaves aliases about 100 dB down.
pub fn blep4(d: f32) -> [f32; 4] {
    let e = 1.0 - d;
    let (d2, e2) = (d * d, e * e);
    let (d4, e4) = (d2 * d2, e2 * e2);
    let late = -0.5 + d * (2.0 / 3.0) - d2 * d / 3.0 + d4 / 8.0;
    let early = 0.5 - e * (2.0 / 3.0) + e2 * e / 3.0 - e4 / 8.0;
    [d4 / 24.0, early, late, -e4 / 24.0]
}

/// An oscillator bus that band-limits its steps: each sample is held back two samples, so
/// a step can still correct the two samples before the one it was seen at.
#[derive(Clone, Copy, Default)]
pub struct Steps {
    buf: [[f32; 4]; 2],
    at: usize,
}

impl Steps {
    /// A step of `h[c]` on each of the first `CH` channels, `d` samples (0..1) before the
    /// current sample.
    pub fn step<const CH: usize>(&mut self, d: f32, h: [f32; 2]) {
        let r = blep4(clampf(d, 0.0, 1.0));
        for (c, &h) in h.iter().enumerate().take(CH) {
            for (k, r) in r.iter().enumerate() {
                self.buf[c][(self.at + 2 + k) & 3] += h * r;
            }
        }
    }

    /// Add to the current sample's first `CH` channels.
    pub fn add<const CH: usize>(&mut self, x: [f32; 2]) {
        for (b, x) in self.buf.iter_mut().zip(x).take(CH) {
            b[self.at] += x;
        }
    }

    /// The sample from two samples ago, now final; the current sample moves on.
    pub fn next<const CH: usize>(&mut self) -> [f32; 2] {
        let old = (self.at + 2) & 3;
        let mut y = [0.0; 2];
        for (y, b) in y.iter_mut().zip(&mut self.buf).take(CH) {
            *y = std::mem::take(&mut b[old]);
        }
        self.at = (self.at + 1) & 3;
        y
    }

    /// Make the right channel a copy of the left (a mono sound turning stereo).
    pub fn copy_left(&mut self) {
        self.buf[1] = self.buf[0];
    }
}

/// A 50% square at `dt` cycles a sample, band-limited through `bus`, with gain `g` on the
/// first channel. Its phase starts at 0, where it rises.
#[derive(Clone, Copy, Default)]
pub struct Square {
    pub phase: f32,
}

impl Square {
    pub fn tick(&mut self, dt: f32, g: f32, bus: &mut Steps) -> f32 {
        let was_high = self.phase < 0.5;
        let mut t = self.phase + dt;
        if was_high && t >= 0.5 {
            bus.step::<1>((t - 0.5) / dt, [-2.0 * g, 0.0]);
        }
        if t >= 1.0 {
            t -= 1.0;
            bus.step::<1>(t / dt, [2.0 * g, 0.0]);
            if t >= 0.5 {
                bus.step::<1>((t - 0.5) / dt, [-2.0 * g, 0.0]);
            }
        }
        self.phase = t;
        if t < 0.5 { g } else { -g }
    }
}

// ---------------------------------------------------------------- filters

/// Coefficients of the 2× halfband below (Laurent de Soras' polyphase IIR design, 7
/// coefficients, transition 0.04): flat to 20 kHz (within 1e-8 dB) and at least 86 dB
/// down from 26 kHz, for 48 kHz out.
const HALFBAND: [f32; 7] = [0.051_802_01, 0.187_978_45, 0.365_053_7, 0.542_327_4, 0.697_778_1, 0.828_226_6, 0.943_126_7];

/// 2× decimator: two chains of first-order allpasses, one per phase of the input.
#[derive(Clone, Copy, Default)]
pub struct Halfband {
    x: [f32; 7],
    y: [f32; 7],
}

impl Halfband {
    /// One output sample from two input samples, `a` then `b`.
    pub fn tick(&mut self, a: f32, b: f32) -> f32 {
        let mut p = [b, a];
        for (i, &c) in HALFBAND.iter().enumerate() {
            let s = &mut p[i & 1];
            let y = (*s - self.y[i]) * c + self.x[i];
            self.x[i] = *s;
            self.y[i] = y;
            *s = y;
        }
        0.5 * (p[0] + p[1])
    }

    /// Zero any state far below hearing. Called once a tick rather than every sample, which
    /// would lengthen the recursion each sample waits on.
    pub fn flush(&mut self) {
        self.x = self.x.map(flush);
        self.y = self.y.map(flush);
    }
}

/// The ladder's saturation: [`knee`] at 3× the level, so a full-scale sound passes it
/// untouched (up to 1.5) and a resonance or a hard drive runs into it (±3 at most).
fn saturate(u: f32) -> f32 {
    if u.abs() <= 1.5 { u } else { 3.0 * knee(u * (1.0 / 3.0)) }
}

/// Four-pole ladder lowpass (zero-delay-feedback, after Zavalishin), with a soft knee on
/// the input after the feedback is summed in: clean at ordinary levels, it limits a
/// resonance (or a hard drive) as an analog ladder does.
#[derive(Clone, Copy, Default)]
pub struct Ladder {
    s: [f32; 4],
}

/// One sample's coefficients for every channel's ladder.
#[derive(Clone, Copy)]
pub struct LadderCoefs {
    /// Each stage's G = g / (1 + g), and 1 − G.
    g: f32,
    h: f32,
    /// The input's gain into the loop, and the stages' state's: the feedback `k` solved for
    /// (by 1 / (1 + k G⁴)), the input with the makeup that restores some of the bass a
    /// resonant ladder loses.
    input: f32,
    state: f32,
}

impl LadderCoefs {
    /// From each stage's g / (1 + g) and the feedback (4 is where it starts to self-oscillate).
    pub fn new(g: f32, k: f32) -> Self {
        let g2 = g * g;
        let solve = 1.0 / (1.0 + k * g2 * g2);
        LadderCoefs { g, h: 1.0 - g, input: (1.0 + 0.5 * k) * solve, state: k * (1.0 - g) * solve }
    }
}

impl Ladder {
    /// A whisper (-80 dB) in the first stage, so feedback past 4 sings with no input.
    pub fn nudge(&mut self) {
        self.s[0] = 1e-4;
    }

    pub fn tick(&mut self, x: f32, c: &LadderCoefs) -> f32 {
        let [s0, s1, s2, s3] = self.s;
        let g = c.g;
        // The output is y4 = G⁴ u + (1 − G)(G³ s1 + G² s2 + G s3 + s4): solved for the input
        // u = x − k y4, then saturated. The last stage's state goes in last, and each stage is
        // o = G·in + (1 − G)·s, then s = 2o − s, so each sample waits on as little as it can.
        let rest = x * c.input - ((s0 * g + s1) * g + s2) * g * c.state;
        let mut y = saturate(rest - s3 * c.state);
        for st in &mut self.s {
            let o = g * y + c.h * *st;
            *st = 2.0 * o - *st;
            y = o;
        }
        y
    }

    /// Zero any state far below hearing (once a tick).
    pub fn flush(&mut self) {
        self.s = self.s.map(flush);
    }
}

/// A one-pole TPT stage's g / (1 + g) at `f` Hz.
pub fn one_pole(f: f32, sr: f32) -> f32 {
    let g = prewarp(f, sr);
    g / (1.0 + g)
}

/// A TPT state-variable filter with fixed settings: lowpass, unit-peak bandpass or highpass.
#[derive(Clone, Copy)]
pub struct Svf {
    f: Filter,
    s: [f32; 2],
}

impl Svf {
    pub fn lowpass(sr: f32, f: f32, q: f32) -> Self {
        Svf { f: Filter::new(prewarp(f, sr), 1.0 / q, [0.0, 0.0, 1.0]), s: [0.0; 2] }
    }

    pub fn bandpass(sr: f32, f: f32, q: f32) -> Self {
        Svf { f: Filter::new(prewarp(f, sr), 1.0 / q, [0.0, 1.0 / q, 0.0]), s: [0.0; 2] }
    }

    pub fn highpass(sr: f32, f: f32, q: f32) -> Self {
        Svf { f: Filter::new(prewarp(f, sr), 1.0 / q, [1.0, -1.0 / q, -1.0]), s: [0.0; 2] }
    }

    pub fn tick(&mut self, x: f32) -> f32 {
        self.f.tick(&mut self.s, x)
    }
}

/// DC blocker: a one-pole highpass at 5 Hz.
#[derive(Clone, Copy, Default)]
pub struct DcBlock {
    x: f32,
    y: f32,
}

impl DcBlock {
    /// The pole for a sample rate.
    pub fn pole(sr: f32) -> f32 {
        (-2.0 * PI * 5.0 / sr).exp()
    }

    pub fn tick(&mut self, x: f32, pole: f32) -> f32 {
        let y = x - self.x + pole * self.y;
        self.x = x;
        self.y = y;
        y
    }

    /// Zero any state far below hearing (once a tick).
    pub fn flush(&mut self) {
        self.x = flush(self.x);
        self.y = flush(self.y);
    }
}

// ---------------------------------------------------------------- envelopes

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

/// How far past 1 the attack aims: an RC charge toward 1.3 that stops at 1, the curve of an
/// analog envelope. It gets there after ln(1.3 / 0.3) time constants.
const OVERSHOOT: f32 = 1.3;
const REACH: f32 = 1.466_337;

/// An analog-style ADSR. The attack is an RC curve that reaches 1 in its time; decay and
/// release are exponential, reaching -60 dB of the way in their time, and the sustain level
/// is approached at the decay's rate. A gate edge carries on from the current level, so a
/// retrigger never clicks. It ends in exact 0 at -90 dB.
pub struct Env {
    pub stage: Stage,
    pub value: f32,
    sustain: f32,
    attack: Coef,
    decay: Coef,
    release: Coef,
    /// The per-step coefficients for the current times.
    a: f32,
    d: f32,
    r: f32,
}

impl Env {
    pub fn new() -> Self {
        Env {
            stage: Stage::Idle,
            value: 0.0,
            sustain: 0.0,
            attack: Coef::new(),
            decay: Coef::new(),
            release: Coef::new(),
            a: 0.0,
            d: 0.0,
            r: 0.0,
        }
    }

    /// Times in seconds, at `rate` steps a second (a sample rate, or a tick rate).
    pub fn set(&mut self, attack: f32, decay: f32, sustain: f32, release: f32, rate: f32) {
        self.a = 1.0 - self.attack.get(clampf(attack, 0.0005, 30.0) / REACH, rate);
        self.d = self.decay.get(clampf(decay, 0.001, 60.0) / T60, rate);
        self.r = self.release.get(clampf(release, 0.003, 60.0) / T60, rate);
        let s = clampf(sustain, 0.0, 1.0);
        self.sustain = if s < END { 0.0 } else { s };
    }

    pub fn on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.value += (OVERSHOOT - self.value) * self.a;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.value = self.sustain + (self.value - self.sustain) * self.d;
                if self.value < END && self.sustain == 0.0 {
                    self.value = 0.0;
                }
            }
            Stage::Release => {
                self.value *= self.r;
                if self.value < END {
                    self.value = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.value
    }
}

/// Exponential decay from 1 that reaches -60 dB in `time` seconds and ends in exact 0 at
/// -90 dB: a one-shot's envelope.
#[derive(Clone, Copy, Default)]
pub struct Decay {
    pub value: f32,
    coef: f32,
}

impl Decay {
    pub fn new(time: f32, sr: f32) -> Self {
        Decay { value: 1.0, coef: (-T60 / (clampf(time, 0.001, 60.0) * sr)).exp() }
    }

    /// From `level` rather than 1.
    pub fn from(level: f32, time: f32, sr: f32) -> Self {
        Decay { value: level, ..Decay::new(time, sr) }
    }

    pub fn next(&mut self) -> f32 {
        let v = self.value;
        self.value = if v * self.coef < END { 0.0 } else { v * self.coef };
        v
    }
}

// ---------------------------------------------------------------- saturation

/// The cubic soft clip `1.5x − 0.5x³`, ±1 from |x| = 1 on.
fn cubic(x: f64) -> f64 {
    if x.abs() <= 1.0 { x * (1.5 - 0.5 * x * x) } else { x.signum() }
}

/// The cubic clip's antiderivative.
fn cubic_area(x: f64) -> f64 {
    let a = x.abs();
    if a <= 1.0 { x * x * (0.75 - 0.125 * x * x) } else { a - 0.375 }
}

/// The cubic soft clip with first-order antiderivative anti-aliasing: each output is the
/// clip's mean over the straight line from the last input to this one, which takes most of
/// the aliasing out of a hard drive (and delays the sound by half a sample).
#[derive(Clone, Copy, Default)]
pub struct Clip {
    last: f32,
}

impl Clip {
    pub fn tick(&mut self, x: f32) -> f32 {
        let (a, b) = (self.last, x);
        self.last = x;
        if a.abs() <= 1.0 && b.abs() <= 1.0 {
            // (F(b) − F(a)) / (b − a), factored so it never divides.
            (a + b) * (0.75 - 0.125 * (a * a + b * b))
        } else if a >= 1.0 && b >= 1.0 {
            1.0
        } else if a <= -1.0 && b <= -1.0 {
            -1.0
        } else {
            let (a, b) = (a as f64, b as f64);
            let d = b - a;
            (if d.abs() < 1e-9 { cubic(0.5 * (a + b)) } else { (cubic_area(b) - cubic_area(a)) / d }) as f32
        }
    }
}

/// How hard [`Drive`] pushes the clip at 0 dB: there, peaks of a full-scale sound reach a
/// quarter of the clip's knee.
const DRIVE_IN: f32 = 0.125;

/// A soft-clipping drive with makeup gain: 0 dB is clean, and turning it up to 24 dB adds
/// harmonics without changing the level of a full-scale sound by more than about 3 dB.
#[derive(Clone, Copy)]
pub struct Drive {
    /// Gain into the clip, and after it.
    pub into: f32,
    pub out: f32,
}

impl Drive {
    pub fn new(db: f32) -> Self {
        let g = 10f32.powf(clampf(db, 0.0, 36.0) / 20.0);
        // A full-scale saw's RMS through the clip, relative to clean: about g / (1 + (g/9.24)^2.5)^0.4.
        let level = (1.0 + (g / 9.24).powf(2.5)).powf(0.4);
        Drive { into: DRIVE_IN * g, out: level / (1.5 * DRIVE_IN * g) }
    }
}

/// Equal-power pan gains for a mono sound, unity at the centre (std's pan law): `p` from
/// -1 (left) to 1.
pub fn pan(p: f32) -> [f32; 2] {
    let a = (clampf(p, -1.0, 1.0) + 1.0) * 0.125;
    [sin2pi(a + 0.25) * SQRT_2, sin2pi(a) * SQRT_2]
}

#[cfg(test)]
mod tests;
