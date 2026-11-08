//! `@m2/vocals:snap`: how many semitones to move a sung pitch (VOX-22). The only one of the three
//! made just for Tune.
//!
//! A pitch is voiced when its `clarity` is at least [`VOICED`] and its `hz` is one `pitch` finds.
//! Then its aim is the nearest note of the key: the key is `time.key`, a keyset (VOX-12), read only
//! as a whole number from 1 to 4095, else the last good one is kept (every note, at first). The
//! nearest is worked out between pitch classes, so a pitch heard an octave off gives the same move,
//! and the note it aimed at last is kept while it is no more than [`BIAS`] semitones further than
//! the nearest. Unvoiced, its aim is 0. The output eases toward `amount` × the aim with a time
//! constant of `glide` seconds (a one-pole), and at glide 0 is there at once: Robot.
//!
//! Its two meters (ABI-13) are Tune's pitch line (VOX-27), which nothing in a song reads, each in
//! semitones, + sharp and − flat, as it stands at the call's end: `off`, how far the pitch it hears
//! is from the note it aims at (−aim), and `left`, how far its move leaves it from that note (the
//! output less the aim: 0 once it has moved all the way). Unvoiced, both are 0.

use m2_dsp::fx::{clampf, flush};
use m2_kernel::{Cfg, Io, Kernel};

use crate::yin::{HIGHEST, LOWEST};

/// The clarity from which a pitch is a sung note (`shift` takes the same).
pub const VOICED: f32 = 0.5;
/// How much nearer another note must be before the current one is left, in semitones.
pub const BIAS: f64 = 0.2;
/// Every note allowed.
const EVERY: u16 = 4095;
/// The longest glide, in seconds: the knob's end.
const LONGEST: f32 = 0.4;

const HZ: usize = 0;
const CLARITY: usize = 1;
const KEY: usize = 2;
const GLIDE: usize = 3;
const AMOUNT: usize = 4;

pub struct Snap {
    sr: f32,
    key: u16,
    /// The pitch class it aimed at last, if any.
    note: Option<u8>,
    /// What it worked out last, by what it was given: `(hz, clarity, key)` and the aim.
    seen: Option<(f32, f32, u16)>,
    aim: f64,
    /// The glide's coefficient, by the glide it was made for.
    coef: (f32, f32),
    out: f32,
}

/// `x` wrapped into a pitch class's distance: −6 to 6 semitones.
fn wrap(x: f64) -> f64 {
    x - 12.0 * (x / 12.0).round()
}

impl Snap {
    /// The move from a sung `hz` to the nearest note of `key`, keeping the last note by the bias.
    fn aim(&mut self, hz: f32) -> f64 {
        let class = (69.0 + 12.0 * (hz as f64 / 440.0).log2()).rem_euclid(12.0);
        let allowed = |n: u8| self.key >> n & 1 == 1;
        let (mut best, mut by) = (0u8, f64::INFINITY);
        for n in 0..12u8 {
            let d = wrap(n as f64 - class);
            if allowed(n) && d.abs() < by.abs() {
                (best, by) = (n, d);
            }
        }
        if let Some(n) = self.note.filter(|&n| allowed(n)) {
            let d = wrap(n as f64 - class);
            if d.abs() <= by.abs() + BIAS {
                (best, by) = (n, d);
            }
        }
        self.note = Some(best);
        by
    }
}

impl Kernel for Snap {
    const INPUTS: usize = 5;
    const OUTPUTS: usize = 3;

    fn new(cfg: &Cfg) -> Self {
        Snap { sr: cfg.sample_rate.max(1.0), key: EVERY, note: None, seen: None, aim: 0.0, coef: (0.0, 1.0), out: 0.0 }
    }

    fn process(&mut self, io: &mut Io) {
        for t in 0..io.frames {
            let k = io.input(KEY)[t];
            if k.fract() == 0.0 && (1.0..=EVERY as f32).contains(&k) {
                self.key = k as u16;
            }
            let (hz, clarity) = (io.input(HZ)[t], io.input(CLARITY)[t]);
            let now = (hz, clarity, self.key);
            // A reading holds for a hop: worked out once for each.
            if self.seen.is_none_or(|s| s.0.to_bits() != now.0.to_bits() || s.1.to_bits() != now.1.to_bits() || s.2 != now.2) {
                self.seen = Some(now);
                let voiced = clarity >= VOICED && (LOWEST..=HIGHEST).contains(&hz);
                self.aim = if voiced { self.aim(hz) } else { 0.0 };
                if !voiced {
                    self.note = None;
                }
            }
            let glide = clampf(io.input(GLIDE)[t], 0.0, LONGEST);
            if glide.to_bits() != self.coef.0.to_bits() {
                self.coef = (glide, if glide > 0.0 { 1.0 - (-1.0 / (glide * self.sr)).exp() } else { 1.0 });
            }
            let target = clampf(io.input(AMOUNT)[t], 0.0, 1.0) * self.aim as f32;
            self.out = flush(self.out + self.coef.1 * (target - self.out));
            if (self.out - target).abs() < 1e-7 {
                self.out = target;
            }
            io.output(0)[t] = self.out;
        }
        // Its meters: the aim moves the heard pitch onto its note, so the voice is −aim from it,
        // and the move leaves it the output less the aim. Unvoiced (no note), both are 0, though the
        // move still eases back.
        let voiced = self.note.is_some();
        io.meter(1, if voiced { (0.0 - self.aim) as f32 } else { 0.0 });
        io.meter(2, if voiced { self.out - self.aim as f32 } else { 0.0 });
    }
}
