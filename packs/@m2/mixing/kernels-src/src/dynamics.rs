//! The tools that follow a sound's level: the gate, the transient shaper and the de-esser.

use m2_dsp::fx::{Filter, Glide, clampf, flush, prewarp, sane};
use m2_kernel::{Cfg, Io, Kernel};

use crate::{Follow, gain, pole};

/// A level that counts as silence, so a ratio of two silences is 1.
const FLOOR: f32 = 1e-6;

/// `20 log10(x)`, silence at the floor.
fn db(x: f32) -> f32 {
    20.0 * x.max(FLOOR).log10()
}

// ---------------------------------------------------------------- the gate

/// How far under the threshold the key must go before the gate may close: its hysteresis.
const HYSTERESIS_DB: f32 = 6.0;
/// The shortest hold: half a wave of 20 Hz is 25 ms, so the gaps between a low note's waves never
/// close the gate.
const MIN_HOLD: f32 = 0.025;

/// Gate. Inputs: in, key, threshold (dB), attack, hold and release (seconds), range (dB).
///
/// It opens when the key reaches the threshold, and starts to close once the key has stayed 6 dB
/// under it for `hold` (at least 25 ms). It opens over `attack`, a straight line in level from
/// `range` to 0 dB, and closes over `release`, a straight line in dB down to `range`. The key is
/// its own input when the component is given no other (`gate(x)` passes `x`).
pub struct Gate {
    sr: f32,
    open: bool,
    /// Samples since the key was last at or above the closing level.
    quiet: u32,
    /// The gain now; NaN until the first sample, which finds it closed.
    g: f32,
    /// The controls the rest was worked out from.
    read: [f32; 5],
    opens: f32,
    closes: f32,
    hold: u32,
    /// The gain closed, what opening adds each sample, and what closing multiplies by.
    floor: f32,
    up: f32,
    down: f32,
}

impl Kernel for Gate {
    const INPUTS: usize = 7;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(1.0);
        Gate {
            sr,
            open: false,
            quiet: u32::MAX,
            g: f32::NAN,
            read: [f32::NAN; 5],
            opens: 1.0,
            closes: 1.0,
            hold: 0,
            floor: 1.0,
            up: 1.0,
            down: 1.0,
        }
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.min(2);
        let ([l, r, kl, kr, threshold, attack, hold, release, range], [ol, or]) =
            io.ports([(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0)], [(0, 0), (0, 1)]);
        let (ins, mut outs) = ([l, r], [ol, or]);
        for i in 0..outs[0].len() {
            let read = [threshold[i], attack[i], hold[i], release[i], range[i]];
            if read != self.read {
                self.read = read;
                let threshold = clampf(read[0], -120.0, 0.0);
                self.opens = gain(threshold);
                self.closes = gain(threshold - HYSTERESIS_DB);
                self.hold = (clampf(read[2], MIN_HOLD, 10.0) * self.sr).round() as u32;
                self.floor = gain(clampf(read[4], -120.0, 0.0));
                self.up = (1.0 - self.floor).max(1e-6) / (clampf(read[1], 1e-5, 1.0) * self.sr);
                self.down = (self.floor.ln() / (clampf(read[3], 0.001, 10.0) * self.sr)).exp();
            }
            let key = sane(kl[i]).abs().max(sane(kr[i]).abs());
            self.quiet = if key >= self.closes { 0 } else { self.quiet.saturating_add(1) };
            if key >= self.opens {
                self.open = true;
            } else if self.open && self.quiet > self.hold {
                self.open = false;
            }
            if self.g.is_nan() {
                self.g = self.floor;
            }
            let target = if self.open { 1.0 } else { self.floor };
            if self.g < target {
                self.g = (self.g + self.up).min(target);
            } else if self.g > target {
                self.g = (self.g * self.down).max(target);
            }
            for (c, out) in outs.iter_mut().enumerate() {
                out[i] = if c < channels { sane(ins[c][i]) * self.g } else { 0.0 };
            }
        }
    }
}

// ---------------------------------------------------------------- the transient shaper

/// The level the transient shaper follows: the loudest sample of the last 25 to 26 ms, kept as
/// the loudest of each 1 ms piece. A low note's waves read as one steady level, and a sound that
/// fades reads as a level that only falls. Silence after its last piece empties brings it back to
/// its birth.
struct Window {
    pieces: [f32; Window::PIECES],
    /// Where the next finished piece goes.
    at: usize,
    /// The loudest of the finished pieces.
    finished: f32,
    /// The piece being filled: its loudest sample so far, and the samples it still takes.
    now: f32,
    left: u32,
    len: u32,
}

impl Window {
    const PIECES: usize = 25;

    fn new(sr: f32) -> Window {
        let len = (sr / 1000.0).round().max(1.0) as u32;
        Window { pieces: [0.0; Window::PIECES], at: 0, finished: 0.0, now: 0.0, left: len, len }
    }

    fn next(&mut self, x: f32) -> f32 {
        // Empty, and silent: back where it was born, so the next sound's pieces start at its first
        // sample, as a fresh window's would.
        if x == 0.0 && self.now == 0.0 && self.finished == 0.0 {
            (self.at, self.left) = (0, self.len);
            return 0.0;
        }
        self.now = self.now.max(x);
        let level = self.now.max(self.finished);
        self.left -= 1;
        if self.left == 0 {
            self.pieces[self.at] = self.now;
            self.at = (self.at + 1) % Window::PIECES;
            self.finished = self.pieces.iter().fold(0.0, |m, &p| m.max(p));
            self.now = 0.0;
            self.left = self.len;
        }
        level
    }
}

/// How fast the level is smoothed, so each 1 ms piece's step is a glide.
const LEVEL_TIME: f32 = 0.002;
/// How slowly the attack follower rises, and the sustain follower falls.
const RISE_TIME: f32 = 0.02;
const FALL_TIME: f32 = 0.3;
/// How far the level must be from a follower for the whole of `attack` or `sustain`.
const FULL_DB: f32 = 6.0;
/// The most a transient shaper turns a sound up or down.
const MOST_DB: f32 = 24.0;

/// Transient shaper. Inputs: in, attack, sustain (dB, each −24 to 24).
///
/// Two followers watch the level: one rises slowly (`RISE_TIME`) and falls with it at once, so it
/// lags each hit's start; one falls slowly (`FALL_TIME`) and rises with it at once, so it hangs over
/// each tail. How far the level is above the first is a hit's start, and turns it by up to
/// `attack` dB; how far below the second is its tail, turned by up to `sustain` dB. Each in full
/// from 6 dB on. Where the level holds, or falls with `sustain` at 0, both terms are exactly 0, and
/// the sound comes out as it went in.
pub struct Transient {
    window: Window,
    level: Follow,
    rise: Follow,
    fall: Follow,
    /// One-pole coefficients: the level's glide, the riser's and the faller's.
    poles: [f32; 3],
    attack: Glide,
    sustain: Glide,
}

impl Kernel for Transient {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(1.0);
        Transient {
            window: Window::new(sr),
            level: Follow::new(),
            rise: Follow::new(),
            fall: Follow::new(),
            poles: [pole(LEVEL_TIME, sr), pole(RISE_TIME, sr), pole(FALL_TIME, sr)],
            attack: Glide::new(0.02, sr),
            sustain: Glide::new(0.02, sr),
        }
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.min(2);
        let ([l, r, attack, sustain], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0), (2, 0)], [(0, 0), (0, 1)]);
        let (ins, mut outs) = ([l, r], [ol, or]);
        for i in 0..outs[0].len() {
            let x = [sane(ins[0][i]), if channels > 1 { sane(ins[1][i]) } else { 0.0 }];
            let loudest = self.window.next(x[0].abs().max(x[1].abs()));
            let level = self.level.next(loudest, self.poles[0]);
            let rise = if level <= self.rise.value { self.rise.set(level) } else { self.rise.next(level, self.poles[1]) };
            let fall = if level >= self.fall.value { self.fall.set(level) } else { self.fall.next(level, self.poles[2]) };
            let start = ((db(level) - db(rise)) / FULL_DB).min(1.0);
            let tail = ((db(fall) - db(level)) / FULL_DB).min(1.0);
            let a = self.attack.next(clampf(attack[i], -MOST_DB, MOST_DB));
            let s = self.sustain.next(clampf(sustain[i], -MOST_DB, MOST_DB));
            let g = gain(clampf(a * start + s * tail, -MOST_DB, MOST_DB));
            for (c, out) in outs.iter_mut().enumerate() {
                out[i] = if c < channels { x[c] * g } else { 0.0 };
            }
        }
    }
}

// ---------------------------------------------------------------- the de-esser

/// The width of the band a de-esser turns down: a bell of Q 1, about an octave and a half.
const DEESS_Q: f32 = 1.0;
/// How fast its detector grabs the band's level, and lets it go.
const DEESS_ATTACK: f32 = 0.0005;
const DEESS_RELEASE: f32 = 0.05;

/// De-esser. Inputs: in, freq (Hz), threshold (dB), amount (dB).
///
/// The band around `freq` (a band-pass of Q 1, at 0 dB at `freq`) is turned down while it is louder
/// than the threshold: held at the threshold, by at most `amount` dB. The output is the input plus
/// the band times (gain − 1), so at gain 1 it is the input, bit for bit; the band's gain is exact at
/// `freq`, and falls away an octave either side. The detector hears the band through a second
/// band-pass of its own, so a loud low note never sets it off.
pub struct Deesser {
    sr: f32,
    freq: Glide,
    /// The frequency the filters were made for.
    at: f32,
    band: Filter,
    /// Each channel's state of the band, and of the detector's second band-pass.
    state: [[[f32; 2]; 2]; 2],
    env: f32,
    poles: [f32; 2],
    threshold: Glide,
    amount: Glide,
}

impl Kernel for Deesser {
    const INPUTS: usize = 4;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(1.0);
        Deesser {
            sr,
            freq: Glide::new(0.03, sr),
            at: f32::NAN,
            band: Filter::new(1.0, 1.0, [0.0; 3]),
            state: [[[0.0; 2]; 2]; 2],
            env: 0.0,
            poles: [pole(DEESS_ATTACK, sr), pole(DEESS_RELEASE, sr)],
            threshold: Glide::new(0.02, sr),
            amount: Glide::new(0.02, sr),
        }
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.min(2);
        let ([l, r, freq, threshold, amount], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0)], [(0, 0), (0, 1)]);
        let (ins, mut outs) = ([l, r], [ol, or]);
        let k = 1.0 / DEESS_Q;
        for i in 0..outs[0].len() {
            let f = self.freq.next(clampf(freq[i], 1000.0, 0.45 * self.sr));
            if f != self.at {
                self.at = f;
                self.band = Filter::new(prewarp(f, self.sr), k, [0.0, k, 0.0]);
            }
            let x = [sane(ins[0][i]), if channels > 1 { sane(ins[1][i]) } else { 0.0 }];
            let mut band = [0.0; 2];
            let mut heard = 0.0f32;
            for c in 0..channels {
                band[c] = self.band.tick(&mut self.state[c][0], x[c]);
                heard = heard.max(self.band.tick(&mut self.state[c][1], band[c]).abs());
            }
            let pole = if heard > self.env { self.poles[0] } else { self.poles[1] };
            self.env = flush(heard + (self.env - heard) * pole);
            let threshold = self.threshold.next(clampf(threshold[i], -120.0, 0.0));
            let most = self.amount.next(clampf(amount[i], 0.0, 48.0));
            let g = gain(-clampf(db(self.env) - threshold, 0.0, most));
            for (c, out) in outs.iter_mut().enumerate() {
                out[i] = if c < channels { x[c] + (g - 1.0) * band[c] } else { 0.0 };
            }
        }
    }
}
