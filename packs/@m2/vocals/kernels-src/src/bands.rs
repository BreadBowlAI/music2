//! `@m2/vocals:vocoder` (VOX-29): a sound, the carrier (a synth), shaped by a voice, so the synth
//! talks. The classic channel vocoder (Homer Dudley's, 1939), in [`BANDS`] bands:
//!
//! - **The voice's bands.** The mono sum of the voice goes through a bank of band-passes, each a
//!   third of an octave wide, their centres a third of an octave apart from [`LOWEST`] (100 Hz) to
//!   8.06 kHz: the ISO third-octave bands. A follower on each says how loud the voice is there:
//!   its power rises in [`ATTACK`] and falls in half of `smooth`, so its level falls by 1/e in
//!   `smooth` once the voice stops there.
//! - **The carrier's bands.** The same bank on each channel of the carrier, its centres moved by
//!   `formant` semitones. Each band is played as loud as the voice is in the same band, against the
//!   voice's loudest band: `level / (loudest + KNEE)`. So the band where the voice is loudest plays
//!   the carrier at up to its own level, every other band as much quieter as the voice is there,
//!   and a voice quieter than [`KNEE`] (−20 dB) plays it quieter still: no band is ever louder
//!   than the carrier is there. The bands are summed, every other one's sign turned over, so that,
//!   all open, they give the carrier back within 1.6 dB.
//! - **Words.** An S, a T and a breath have no pitch, and a synth has little that high: the voice's
//!   own sound over [`HIGHS`] (5 kHz), each channel, is added, times `clarity`.
//!
//! Each band is two state-variable band-passes in a row (m2-dsp's `Filter`), placed in Hz at the
//! song's rate: no look-ahead, so no latency. A silent voice gives silence out: every follower
//! reads 0, so every band is shut. With `listen` 0 (the pack's `vocoder(x)` with no voice) it plays
//! `in` as it is, bit for bit. Every buffer is a fixed array of the state: nothing is taken while it
//! plays (RENDER-28), and each frame is worked out from frames counted from its birth, so the output
//! is the same however its blocks split.

use m2_dsp::fx::{Coef, Filter, Glide, clampf, flush, prewarp, sane};
use m2_kernel::{Cfg, Io, Kernel};

/// How many bands.
pub const BANDS: usize = 20;
/// The lowest band's centre, in Hz: the others are each a third of an octave higher.
pub const LOWEST: f32 = 100.0;
/// Each band's two sections' Q: the pair is 3 dB down a sixth of an octave each side of its centre,
/// where the next band's is, so a third of an octave wide.
pub const Q: f32 = 2.77;
/// How fast a band's follower rises, in seconds.
pub const ATTACK: f32 = 0.002;
/// The level, as a band's amplitude, under which a voice plays the carrier quieter than its own: −20 dB.
pub const KNEE: f32 = 0.1;
/// Where the voice's own highs start, in Hz: a 4th-order Butterworth high-pass.
pub const HIGHS: f32 = 5000.0;
/// How often the carrier's bands are placed again while `formant` moves, in frames from birth.
const EVERY: u32 = 16;

const IN: usize = 0;
const VOICE: usize = 1;
const CLARITY: usize = 2;
const SMOOTH: usize = 3;
const FORMANT: usize = 4;
const LISTEN: usize = 5;

/// Band `k`'s centre, in Hz.
pub fn centre(k: usize) -> f32 {
    LOWEST * (k as f32 / 3.0).exp2()
}

/// A band-pass of 0 dB at `f` Hz, one section: the state-variable filter's band, times 1/Q.
fn section(sr: f32, f: f32) -> Filter {
    Filter::new(prewarp(f, sr), 1.0 / Q, [0.0, 1.0 / Q, 0.0])
}

/// One band's two sections, and one channel's states of them.
type Pair = [Filter; 2];
type States = [[f32; 2]; 2];

fn tick(f: &Pair, s: &mut States, x: f32) -> f32 {
    let a = f[0].tick(&mut s[0], x);
    f[1].tick(&mut s[1], a)
}

/// Vocoder. Inputs: in (the carrier), voice, clarity (0 to 1), smooth (seconds, 0.01 to 0.2),
/// formant (semitones, −12 to 12), listen (a switch). Output: out.
pub struct Vocoder {
    sr: f32,
    stereo: bool,
    /// The voice's bands, and their states (of its mono sum).
    voice: [Pair; BANDS],
    heard: [States; BANDS],
    /// Each band's power, as its follower has it.
    power: [f32; BANDS],
    /// The carrier's bands, moved by `formant`, and their states, each channel.
    carrier: [Pair; BANDS],
    played: [[States; BANDS]; 2],
    /// The voice's highs, and their states, each channel.
    highs: Pair,
    high: [States; 2],
    /// The followers' rise, and their fall (worked out again only when `smooth` moves).
    attack: f32,
    release: Coef,
    clarity: Glide,
    formant: Glide,
    /// The `formant` the carrier's bands were last placed for.
    placed: f32,
    /// Frames since birth.
    n: u32,
}

impl Vocoder {
    /// The carrier's bands, placed `formant` semitones from the voice's.
    fn place(&mut self, formant: f32) {
        let by = (formant / 12.0).exp2();
        for (k, pair) in self.carrier.iter_mut().enumerate() {
            *pair = [section(self.sr, centre(k) * by); 2];
        }
        self.placed = formant;
    }
}

impl Kernel for Vocoder {
    const INPUTS: usize = 6;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(8_000.0);
        let pairs = |f: &dyn Fn(usize) -> f32| -> [Pair; BANDS] { std::array::from_fn(|k| [section(sr, f(k)); 2]) };
        // A 4th-order Butterworth high-pass: two sections, Q 0.54 and 1.31.
        let highs = [Filter::highpass(sr, HIGHS, 0.541_196_1), Filter::highpass(sr, HIGHS, 1.306_563)];
        Vocoder {
            sr,
            stereo: cfg.channels > 1,
            voice: pairs(&centre),
            heard: [[[0.0; 2]; 2]; BANDS],
            power: [0.0; BANDS],
            carrier: pairs(&centre),
            played: [[[[0.0; 2]; 2]; BANDS]; 2],
            highs,
            high: [[[0.0; 2]; 2]; 2],
            attack: (-1.0 / (ATTACK * sr)).exp(),
            release: Coef::new(),
            clarity: Glide::new(0.02, sr),
            formant: Glide::new(0.02, sr),
            placed: f32::NAN,
            n: 0,
        }
    }

    fn process(&mut self, io: &mut Io) {
        let ([l, r, vl, vr, clarity, smooth, formant, listen], [ol, or]) =
            io.ports([(IN, 0), (IN, 1), (VOICE, 0), (VOICE, 1), (CLARITY, 0), (SMOOTH, 0), (FORMANT, 0), (LISTEN, 0)], [(0, 0), (0, 1)]);
        let stereo = self.stereo;
        for i in 0..ol.len() {
            let n = self.n;
            self.n = n.wrapping_add(1);
            // With nothing to listen to (`listen` 0, or not a number), the carrier as it is: the
            // pack's `vocoder(x)`.
            let listening = listen[i] >= 0.5;
            if !listening {
                (ol[i], or[i]) = (l[i], if stereo { r[i] } else { 0.0 });
                continue;
            }
            let at = self.formant.next(clampf(formant[i], -12.0, 12.0));
            if (n.is_multiple_of(EVERY) || self.placed.is_nan()) && at.to_bits() != self.placed.to_bits() {
                self.place(at);
            }
            let fall = self.release.get(0.5 * clampf(smooth[i], 0.01, 0.2), self.sr);
            let v = [sane(vl[i]), if stereo { sane(vr[i]) } else { 0.0 }];
            let mono = if stereo { 0.5 * (v[0] + v[1]) } else { v[0] };
            let mut loudest = 0.0f32;
            for ((band, state), power) in self.voice.iter().zip(&mut self.heard).zip(&mut self.power) {
                let a = tick(band, state, mono);
                let p = a * a;
                let c = if p > *power { self.attack } else { fall };
                *power = flush(p + (*power - p) * c);
                loudest = loudest.max(*power);
            }
            // Every other band's sign turned over: where two bands meet, their phases are 130° apart,
            // and summed as they are they would cut a notch there; turned over, 50°.
            let by = 1.0 / (loudest.sqrt() + KNEE);
            let gains: [f32; BANDS] = std::array::from_fn(|k| if k % 2 == 0 { by } else { -by } * self.power[k].sqrt());
            let clear = self.clarity.next(clampf(clarity[i], 0.0, 1.0));
            let x = [sane(l[i]), if stereo { sane(r[i]) } else { 0.0 }];
            let mut y = [0.0f32; 2];
            for c in 0..if stereo { 2 } else { 1 } {
                let mut sum = 0.0;
                for ((band, state), g) in self.carrier.iter().zip(&mut self.played[c]).zip(gains) {
                    sum += tick(band, state, x[c]) * g;
                }
                y[c] = sum + clear * tick(&self.highs, &mut self.high[c], v[c]);
            }
            (ol[i], or[i]) = (y[0], y[1]);
        }
    }

    /// What the voice said still shapes what comes out until every band's follower has fallen to 0.
    fn active(&self) -> bool {
        self.power.iter().any(|&p| p > 0.0)
    }
}
