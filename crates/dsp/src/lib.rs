//! L1 — DSP primitives. Per-sample building blocks; `m2-graph` wraps them into
//! block-processing nodes. Everything is allocation-free after construction.

use std::f32::consts::{PI, TAU};

pub type Frame = [f32; 2];

pub fn db_to_gain(db: f32) -> f32 {
    if db <= -96.0 { 0.0 } else { 10f32.powf(db / 20.0) }
}

pub fn gain_to_db(g: f32) -> f32 {
    if g <= 1e-6 { -120.0 } else { 20.0 * g.log10() }
}

/// Equal-power pan, `pan` in [-1, 1].
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
    (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Saw,
    Square,
    Triangle,
}

impl Wave {
    pub fn parse(s: &str) -> Wave {
        match s {
            "saw" | "sawtooth" => Wave::Saw,
            "square" | "pulse" => Wave::Square,
            "triangle" | "tri" => Wave::Triangle,
            _ => Wave::Sine,
        }
    }
}

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

/// Band-limited (PolyBLEP) oscillator.
#[derive(Debug, Clone)]
pub struct Osc {
    pub phase: f32,
}

impl Osc {
    pub fn new(phase: f32) -> Self {
        Self { phase: phase.fract() }
    }

    pub fn next(&mut self, wave: Wave, freq: f32, sr: f32) -> f32 {
        let dt = (freq / sr).clamp(0.0, 0.49);
        let t = self.phase;
        let out = match wave {
            Wave::Sine => (t * TAU).sin(),
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Wave::Square => {
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t + 0.5).fract(), dt);
                sq
            }
            // Triangle harmonics fall off at 12 dB/oct, so naive aliasing is mild.
            Wave::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// Cheap white noise (xorshift32).
#[derive(Debug, Clone)]
pub struct Noise(u32);

#[allow(clippy::should_implement_trait)]
impl Noise {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear-attack, exponential decay/release ADSR. Times in seconds.
#[derive(Debug, Clone)]
pub struct Adsr {
    stage: Stage,
    value: f32,
    pub a: f32,
    pub d: f32,
    pub s: f32,
    pub r: f32,
}

impl Adsr {
    pub fn new(a: f32, d: f32, s: f32, r: f32) -> Self {
        Self { stage: Stage::Idle, value: 0.0, a, d, s, r }
    }

    pub fn gate(&mut self, on: bool) {
        if on {
            self.stage = Stage::Attack;
        } else if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    fn coef(time: f32, sr: f32) -> f32 {
        // Reach ~-60 dB in `time` seconds.
        (-6.9 / (time.max(0.0005) * sr)).exp()
    }

    pub fn next(&mut self, sr: f32) -> f32 {
        match self.stage {
            Stage::Idle => self.value = 0.0,
            Stage::Attack => {
                self.value += 1.0 / (self.a.max(0.0005) * sr);
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let c = Self::coef(self.d, sr);
                self.value = self.s + (self.value - self.s) * c;
                if (self.value - self.s).abs() < 1e-4 {
                    self.value = self.s;
                    self.stage = Stage::Sustain;
                }
                if self.s <= 0.0 && self.value < 1e-4 {
                    self.stage = Stage::Idle;
                }
            }
            Stage::Sustain => self.value = self.s,
            Stage::Release => {
                self.value *= Self::coef(self.r, sr);
                if self.value < 1e-4 {
                    self.value = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
}

impl FilterMode {
    pub fn parse(s: &str) -> FilterMode {
        match s {
            "hp" | "highpass" => FilterMode::Highpass,
            "bp" | "bandpass" => FilterMode::Bandpass,
            _ => FilterMode::Lowpass,
        }
    }
}

/// Topology-preserving-transform state-variable filter (Cytomic). Stable under fast
/// cutoff modulation, which is what synth filters need.
#[derive(Debug, Clone, Default)]
pub struct Svf {
    ic1: [f32; 2],
    ic2: [f32; 2],
}

impl Svf {
    pub fn process(&mut self, x: Frame, mode: FilterMode, cutoff: f32, q: f32, sr: f32) -> Frame {
        let fc = cutoff.clamp(16.0, sr * 0.45);
        let g = (PI * fc / sr).tan();
        let k = 1.0 / q.max(0.3);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let mut out = [0.0; 2];
        for c in 0..2 {
            let v3 = x[c] - self.ic2[c];
            let v1 = a1 * self.ic1[c] + a2 * v3;
            let v2 = self.ic2[c] + a2 * self.ic1[c] + a3 * v3;
            self.ic1[c] = 2.0 * v1 - self.ic1[c];
            self.ic2[c] = 2.0 * v2 - self.ic2[c];
            out[c] = match mode {
                FilterMode::Lowpass => v2,
                FilterMode::Bandpass => v1,
                FilterMode::Highpass => x[c] - k * v1 - v2,
            };
        }
        out
    }
}

/// RBJ biquad, stereo. Used for EQ where coefficients change rarely.
#[derive(Debug, Clone)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
}

#[derive(Debug, Clone, Copy)]
pub enum BiquadKind {
    LowShelf,
    HighShelf,
    Peak,
    Highpass,
}

impl Biquad {
    pub fn new(kind: BiquadKind, freq: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        let mut s = Self { b: [1.0, 0.0, 0.0], a: [0.0, 0.0], z: [[0.0; 2]; 2] };
        s.set(kind, freq, q, gain_db, sr);
        s
    }

    pub fn set(&mut self, kind: BiquadKind, freq: f32, q: f32, gain_db: f32, sr: f32) {
        let w0 = TAU * freq.clamp(10.0, sr * 0.45) / sr;
        let (sn, cs) = w0.sin_cos();
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = sn / (2.0 * q.max(0.1));
        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Peak => (1.0 + alpha * a, -2.0 * cs, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cs, 1.0 - alpha / a),
            BiquadKind::LowShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cs + sq),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                    a * ((a + 1.0) - (a - 1.0) * cs - sq),
                    (a + 1.0) + (a - 1.0) * cs + sq,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                    (a + 1.0) + (a - 1.0) * cs - sq,
                )
            }
            BiquadKind::HighShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cs + sq),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                    a * ((a + 1.0) + (a - 1.0) * cs - sq),
                    (a + 1.0) - (a - 1.0) * cs + sq,
                    2.0 * ((a - 1.0) - (a + 1.0) * cs),
                    (a + 1.0) - (a - 1.0) * cs - sq,
                )
            }
            BiquadKind::Highpass => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
        };
        self.b = [b0 / a0, b1 / a0, b2 / a0];
        self.a = [a1 / a0, a2 / a0];
    }

    pub fn process(&mut self, x: Frame) -> Frame {
        let mut y = [0.0; 2];
        for c in 0..2 {
            // Transposed direct form II.
            let out = self.b[0] * x[c] + self.z[0][c];
            self.z[0][c] = self.b[1] * x[c] - self.a[0] * out + self.z[1][c];
            self.z[1][c] = self.b[2] * x[c] - self.a[1] * out;
            y[c] = out;
        }
        y
    }
}

/// Stereo delay line with fractional (linear) reads.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buf: Vec<Frame>,
    pos: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> Self {
        Self { buf: vec![[0.0; 2]; max_samples.max(4)], pos: 0 }
    }

    pub fn read(&self, delay: f32) -> Frame {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 2) as f32);
        let i = d.floor() as usize;
        let f = d - i as f32;
        let a = self.buf[(self.pos + len - i) % len];
        let b = self.buf[(self.pos + len - i - 1) % len];
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
    }

    pub fn write(&mut self, x: Frame) {
        self.pos = (self.pos + 1) % self.buf.len();
        self.buf[self.pos] = x;
    }
}

#[derive(Debug, Clone)]
struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.pos] = x + self.store * feedback;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.pos];
        let out = b - x;
        self.buf[self.pos] = x + b * 0.5;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

/// Freeverb-style stereo reverb.
#[derive(Debug, Clone)]
pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
        let scale = sr / 44_100.0;
        let mk = |spread: usize| {
            (
                COMBS
                    .iter()
                    .map(|l| Comb { buf: vec![0.0; ((l + spread) as f32 * scale) as usize], pos: 0, store: 0.0 })
                    .collect::<Vec<_>>(),
                ALLPASSES.iter().map(|l| Allpass { buf: vec![0.0; ((l + spread) as f32 * scale) as usize], pos: 0 }).collect::<Vec<_>>(),
            )
        };
        let (cl, al) = mk(0);
        let (cr, ar) = mk(23);
        Self { combs: [cl, cr], allpasses: [al, ar] }
    }

    /// Returns the wet signal only. `size` and `damp` in [0, 1].
    pub fn process(&mut self, x: Frame, size: f32, damp: f32) -> Frame {
        let input = (x[0] + x[1]) * 0.015;
        let feedback = 0.7 + size.clamp(0.0, 1.0) * 0.28;
        let damp = damp.clamp(0.0, 1.0) * 0.4;
        let mut out = [0.0; 2];
        #[allow(clippy::needless_range_loop)]
        for c in 0..2 {
            let mut acc = 0.0;
            for comb in &mut self.combs[c] {
                acc += comb.process(input, feedback, damp);
            }
            for ap in &mut self.allpasses[c] {
                acc = ap.process(acc);
            }
            out[c] = acc;
        }
        out
    }
}

/// Feed-forward compressor with a smoothed peak detector. Returns the gain to apply.
#[derive(Debug, Clone, Default)]
pub struct Compressor {
    env: f32,
}

impl Compressor {
    pub fn gain(&mut self, level: f32, threshold_db: f32, ratio: f32, attack: f32, release: f32, sr: f32) -> f32 {
        let coef = |t: f32| (-1.0 / (t.max(0.0001) * sr)).exp();
        let c = if level > self.env { coef(attack) } else { coef(release) };
        self.env = level + (self.env - level) * c;
        let over = gain_to_db(self.env) - threshold_db;
        if over <= 0.0 { 1.0 } else { db_to_gain(-over * (1.0 - 1.0 / ratio.max(1.0))) }
    }
}

/// One-pole smoother for parameter changes (avoids zipper noise).
#[derive(Debug, Clone)]
pub struct Smooth {
    pub value: f32,
    coef: f32,
}

impl Smooth {
    pub fn new(value: f32, time: f32, sr: f32) -> Self {
        Self { value, coef: (-1.0 / (time.max(0.0001) * sr)).exp() }
    }

    pub fn next(&mut self, target: f32) -> f32 {
        self.value = target + (self.value - target) * self.coef;
        self.value
    }
}

/// Soft clip that is transparent at low levels.
pub fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adsr_finishes() {
        let mut e = Adsr::new(0.01, 0.1, 0.5, 0.05);
        e.gate(true);
        for _ in 0..4800 {
            e.next(48_000.0);
        }
        assert!((e.next(48_000.0) - 0.5).abs() < 0.01);
        e.gate(false);
        for _ in 0..48_000 {
            e.next(48_000.0);
        }
        assert!(e.is_idle());
    }

    #[test]
    fn osc_is_bounded() {
        for w in [Wave::Sine, Wave::Saw, Wave::Square, Wave::Triangle] {
            let mut o = Osc::new(0.0);
            for _ in 0..48_000 {
                let s = o.next(w, 440.0, 48_000.0);
                assert!(s.abs() < 1.6, "{w:?} {s}");
            }
        }
    }

    #[test]
    fn filter_and_reverb_are_stable() {
        let mut f = Svf::default();
        let mut r = Reverb::new(48_000.0);
        let mut n = Noise::new(1);
        for i in 0..48_000 {
            let x = n.next();
            let y = f.process([x, x], FilterMode::Lowpass, 200.0 + (i % 1000) as f32 * 10.0, 4.0, 48_000.0);
            let w = r.process(y, 0.9, 0.5);
            assert!(y[0].is_finite() && w[0].abs() < 10.0);
        }
    }
}
