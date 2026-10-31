//! What effects are made of, shared by std's effects and those of music2's own packs (`@m2/effects`):
//! a clamp that maps NaN to its low end, audio made safe to enter an effect, a flush to zero far
//! below hearing, control smoothing, a TPT filter, a delay line read between samples, and an LFO
//! phase that follows the song.
//!
//! Each is safe to feed anything: audio that isn't finite counts as silence and absurd levels are
//! capped before they reach any state; controls are clamped to their useful range (NaN takes the
//! low end); recursive state is flushed to zero far below hearing, so tails end in exact silence
//! instead of crawling through denormals.

use std::f32::consts::PI;

/// `clamp` that maps NaN to `lo` (`f32::clamp` passes NaN through, and a NaN in a
/// feedback path never leaves).
pub fn clampf(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// Audio about to enter an effect: non-finite samples become silence, and anything
/// beyond +80 dBFS is capped so no state can overflow.
pub fn sane(x: f32) -> f32 {
    if x.is_finite() { x.clamp(-1e4, 1e4) } else { 0.0 }
}

/// Zero for values far below hearing (-300 dB), so decaying state never goes denormal
/// and a tail ends in exact silence.
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-15 { 0.0 } else { x }
}

/// One-pole smoother for controls. It starts at its first target and lands exactly on a
/// target once it is close, so a settled control costs nothing to compare against.
pub struct Glide {
    value: f32,
    coef: f32,
    primed: bool,
}

impl Glide {
    pub fn new(time: f32, sr: f32) -> Self {
        Glide { value: 0.0, coef: (-1.0 / (time * sr)).exp(), primed: false }
    }

    pub fn next(&mut self, target: f32) -> f32 {
        if !self.primed {
            self.primed = true;
            self.value = target;
        } else if self.value != target {
            self.value = target + (self.value - target) * self.coef;
            if (self.value - target).abs() <= 1e-6 * (1.0 + target.abs()) {
                self.value = target;
            }
        }
        self.value
    }
}

/// `exp(-1 / (time × sr))`, the one-pole coefficient for a time constant, recomputed
/// only when the time changes.
pub struct Coef {
    time: f32,
    coef: f32,
}

impl Coef {
    pub fn new() -> Self {
        Coef { time: f32::NAN, coef: 0.0 }
    }

    pub fn get(&mut self, time: f32, sr: f32) -> f32 {
        if time != self.time {
            self.time = time;
            self.coef = (-1.0 / (time * sr)).exp();
        }
        self.coef
    }
}

/// `tan(π f / sr)`, the prewarped gain of a TPT filter at `f` Hz, kept below Nyquist.
pub fn prewarp(f: f32, sr: f32) -> f32 {
    (PI * clampf(f, 10.0, 0.45 * sr) / sr).tan()
}

/// A TPT state-variable filter (Cytomic's), mixed into a highpass, a shelf or a bell.
/// Unlike a direct-form biquad it stays clean while its settings move.
#[derive(Clone, Copy)]
pub struct Filter {
    a1: f32,
    a2: f32,
    a3: f32,
    /// How much of the input, the bandpass and the lowpass make the output.
    m: [f32; 3],
}

impl Filter {
    pub fn new(g: f32, k: f32, m: [f32; 3]) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Filter { a1, a2, a3: g * a2, m }
    }

    pub fn highpass(sr: f32, f: f32, q: f32) -> Self {
        let k = 1.0 / q;
        Filter::new(prewarp(f, sr), k, [1.0, -k, -1.0])
    }

    pub fn bell(sr: f32, f: f32, q: f32, db: f32) -> Self {
        let a = 10f32.powf(db / 40.0);
        let k = 1.0 / (q * a);
        Filter::new(prewarp(f, sr), k, [1.0, k * (a * a - 1.0), 0.0])
    }

    pub fn low_shelf(sr: f32, f: f32, q: f32, db: f32) -> Self {
        let a = 10f32.powf(db / 40.0);
        let k = 1.0 / q;
        Filter::new(prewarp(f, sr) / a.sqrt(), k, [1.0, k * (a - 1.0), a * a - 1.0])
    }

    pub fn high_shelf(sr: f32, f: f32, q: f32, db: f32) -> Self {
        let a = 10f32.powf(db / 40.0);
        let k = 1.0 / q;
        Filter::new(prewarp(f, sr) * a.sqrt(), k, [a * a, k * (1.0 - a) * a, 1.0 - a * a])
    }

    /// One sample through the filter; `s` is one channel's state.
    pub fn tick(&self, s: &mut [f32; 2], x: f32) -> f32 {
        let v3 = x - s[1];
        let v1 = self.a1 * s[0] + self.a2 * v3;
        let v2 = s[1] + self.a2 * s[0] + self.a3 * v3;
        s[0] = flush(2.0 * v1 - s[0]);
        s[1] = flush(2.0 * v2 - s[1]);
        self.m[0] * x + self.m[1] * v1 + self.m[2] * v2
    }
}

/// A delay line read at fractional delays with 4-point Hermite interpolation, which keeps
/// the top end of a modulated delay that linear interpolation would dull.
#[derive(Default)]
pub struct Line {
    buf: Vec<f32>,
    pos: usize,
}

impl Line {
    pub fn new(len: usize) -> Self {
        Line { buf: vec![0.0; len.max(4)], pos: 0 }
    }

    /// As `new` left it, in place.
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }

    pub fn push(&mut self, x: f32) {
        self.pos = if self.pos + 1 == self.buf.len() { 0 } else { self.pos + 1 };
        self.buf[self.pos] = x;
    }

    /// The sample pushed `back` pushes ago (0 is the newest).
    pub fn at(&self, back: usize) -> f32 {
        let n = self.buf.len();
        self.buf[(self.pos + n - back % n) % n]
    }

    /// The signal `d` samples ago, between samples.
    pub fn read(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, (self.buf.len() - 3) as f32);
        let i = d as usize;
        let t = d - i as f32;
        let (y0, y1, y2, y3) = (self.at(i - 1), self.at(i), self.at(i + 1), self.at(i + 2));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }
}

/// An LFO phase, 0..1, advanced by a rate in Hz on each sample the song plays. Stopped
/// (`playing` 0, or NaN) it waits where it is, so it carries on at Play as if never stopped.
pub struct Phase {
    pub sr: f32,
    pub at: f32,
}

impl Phase {
    pub fn next(&mut self, rate: f32, max: f32, playing: f32) -> f32 {
        let now = self.at;
        if playing > 0.5 {
            self.at += clampf(rate, 0.0, max) / self.sr;
            if self.at >= 1.0 {
                self.at -= 1.0;
            }
        }
        now
    }
}
