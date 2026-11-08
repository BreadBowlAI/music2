//! The Flanger (PACK-35): the sound mixed with a copy of itself a little late, which an LFO sweeps
//! from [`SHORTEST`] (0.1 ms) to [`RANGE`] times that at full depth (10 ms), evenly in octaves; the
//! copy goes round again by `feedback`. The sum is a comb whose notches and peaks sweep up and down.
//!
//! - **The delay.** `SHORTEST × RANGE^(depth × (0.5 − 0.5 cos 2πφ))`, φ its LFO's phase: at its
//!   shortest at the song's start, at its longest half a cycle on. On a stereo sound the right side's
//!   LFO is a quarter cycle behind the left's. The copy is read between samples (m2-dsp's `Line`, four
//!   points, Hermite), whose gain is never over 1 at any frequency, so it sweeps with no steps, and
//!   the copy going round never grows.
//! - **The comb.** Its line takes in the sound plus the copy times `feedback`; out is
//!   `(1 − mix) × in + mix × s × copy`, `s` Feedback's sign (glided), so below 0 the copy is turned
//!   over too, and the comb's notches and peaks move half way: its hollow sound.
//!
//! It follows the song, as the chorus does: its LFO moves only while `playing` is 1, its lines run on.
//! Its phase is added up in 64-bit floats, so a cycle lasts 1/rate seconds to the sample however
//! long it plays (m2-dsp's `Phase`, in 32-bit floats, drifts by up to 0.1% of a cycle at slow rates).
//! Its meter `phase` is its LFO's phase at the call's last frame (the left side's), which its card's
//! picture draws its line at (CARD-128). Its lines are made at its birth for the longest delay at its
//! song's rate: nothing is taken while it plays (RENDER-28).

use std::f32::consts::TAU;

use m2_dsp::fx::{Glide, Line, clampf, flush, sane};
use m2_kernel::{Cfg, Io, Kernel};

/// How late the copy is at its shortest, in seconds: where its LFO starts. [copied: src/pictures.ts `notchAt`, CARD-128]
pub const SHORTEST: f32 = 0.0001;
/// How many times later than [`SHORTEST`] the copy is at the sweep's other end, at Depth 1.
pub const RANGE: f32 = 100.0;
/// The most feedback, either way: under 1, so the copy going round always dies away.
pub const MOST_FEEDBACK: f32 = 0.9;
/// The fastest sweep, in cycles a second.
pub const MOST_RATE: f32 = 10.0;
/// What the line holds at most, either way: far over anything a stable comb plays, so no state can
/// overflow whatever comes in.
const MOST: f32 = 1e5;

/// Flanger. Inputs: in, rate (Hz), depth (0 to 1), feedback (−0.9 to 0.9), mix (0 to 1), playing.
/// Outputs: out, and its meter `phase`.
pub struct Flanger {
    sr: f32,
    lines: [Line; 2],
    /// Its LFO's phase, 0 to 1, from 0 at its birth.
    phase: f64,
    depth: Glide,
    feedback: Glide,
    /// Feedback's sign: what the copy is turned by, gliding from one side to the other.
    sign: Glide,
    mix: Glide,
    /// Samples since anything but silence went into a line.
    quiet: usize,
    /// Each line's length.
    len: usize,
}

impl Flanger {
    /// The state `new` makes, around zeroed lines.
    fn around(sr: f32, lines: [Line; 2], len: usize) -> Self {
        Flanger {
            sr,
            lines,
            phase: 0.0,
            depth: Glide::new(0.05, sr),
            feedback: Glide::new(0.05, sr),
            sign: Glide::new(0.02, sr),
            mix: Glide::new(0.02, sr),
            quiet: usize::MAX,
            len,
        }
    }
}

impl Kernel for Flanger {
    const INPUTS: usize = 6;
    const OUTPUTS: usize = 2;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(1.0);
        let len = (SHORTEST * RANGE * sr) as usize + 8;
        Flanger::around(sr, [Line::new(len), Line::new(len)], len)
    }

    /// As `new` leaves it, its lines cleared in place: a reset takes no memory.
    fn reset(&mut self, cfg: &Cfg) {
        let mut lines = std::mem::take(&mut self.lines);
        lines.iter_mut().for_each(Line::clear);
        let len = self.len;
        *self = Flanger::around(cfg.sample_rate.max(1.0), lines, len);
    }

    fn process(&mut self, io: &mut Io) {
        let sides = io.channels.min(2);
        let ([l, r, rate, depth, feedback, mix, playing], [ol, or]) =
            io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)], [(0, 0), (0, 1)]);
        let (ins, outs) = ([l, r], [ol, or]);
        let (shortest, octaves) = (SHORTEST * self.sr, RANGE.ln());
        let mut phase = self.phase;
        for i in 0..ins[0].len() {
            let d = self.depth.next(clampf(depth[i], 0.0, 1.0));
            let asked = clampf(feedback[i], -MOST_FEEDBACK, MOST_FEEDBACK);
            let f = self.feedback.next(asked);
            let s = self.sign.next(if asked < 0.0 { -1.0 } else { 1.0 });
            let m = self.mix.next(clampf(mix[i], 0.0, 1.0));
            phase = self.phase;
            if playing[i] > 0.5 {
                self.phase += clampf(rate[i], 0.0, MOST_RATE) as f64 / self.sr as f64;
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
            }
            let (sin, cos) = (TAU * phase as f32).sin_cos();
            let mut heard = false;
            for c in 0..2 {
                if c >= sides {
                    outs[c][i] = 0.0;
                    continue;
                }
                // The right side a quarter cycle behind: cos(2π(φ − ¼)) is sin(2πφ).
                let lfo = if c == 0 { cos } else { sin };
                let late = shortest * (d * (0.5 - 0.5 * lfo) * octaves).exp();
                let line = &mut self.lines[c];
                // The newest sample in the line is a frame old, so the copy is `late − 1` back from it.
                let copy = line.read(late - 1.0);
                let x = sane(ins[c][i]);
                let w = flush(clampf(x + f * copy, -MOST, MOST));
                line.push(w);
                heard |= w != 0.0;
                // At Mix 0, the input as it is, its sign of zero too.
                outs[c][i] = if m == 0.0 { x } else { x * (1.0 - m) + s * m * copy };
            }
            self.quiet = if heard { 0 } else { self.quiet.saturating_add(1) };
        }
        io.meter(1, phase as f32);
    }

    /// The copy keeps sounding after the input stops, for as long as anything but silence is in its
    /// lines.
    fn active(&self) -> bool {
        self.quiet < self.len
    }
}
