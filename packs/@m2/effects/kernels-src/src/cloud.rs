//! Grains (PACK-36): the sound broken into tiny pieces, its grains, played back overlapped.
//!
//! - **Its buffer**: the last [`BUFFER`] seconds of its input (3 s), each channel; Freeze on, it
//!   stops taking in sound and holds what it last heard.
//! - **Its grains.** One starts on average every `1 / density` seconds, each gap from half to one
//!   and a half of that, at random. Each is `size` long, in a Hann window, and reads the buffer from
//!   `back + u × spread` seconds before the newest sound (`u` at random) at `2^(pitch / 12)` times
//!   its speed; `back` keeps it behind the newest sound until it ends (`Grains::start`), so it reads
//!   only the past. At most [`MOST`] play at once: one more lets the oldest go, fading it out in
//!   [`LET_GO`].
//! - **Its level.** They are summed and turned down by `1 / √(3/8 × n)`, `n = size × density` (16
//!   at most), when that is over 1: grains read from places far apart add up in power.
//!
//! Its clock and its random numbers (one 32-bit xorshift from [`SEED`]) move only while `playing` is
//! 1, so it follows the song: no grain starts while it is stopped, and after the engine sat stopped
//! its grains start where an export's do. Where a grain reads is counted from the frames written
//! since it started, never from its birth, so a grain plays the same bits whenever it starts. Every
//! buffer is made at its birth: nothing is taken while it plays (RENDER-28). Its meter `reads` is how
//! far back the newest grain reads now, in seconds, which its card's picture draws (CARD-129).

use m2_dsp::fx::{Glide, clampf, sane};
use m2_kernel::{Cfg, Io, Kernel};

/// How much of its input it keeps, in seconds: enough for a grain an octave up and 500 ms long to
/// start 2 s back and still stay behind the newest sound, frozen too.
pub const BUFFER: f32 = 3.0;
/// The most grains playing at once.
pub const MOST: usize = 16;
/// How long a grain let go takes to fade out, in seconds: shorter than the shortest gap between two
/// grains (half of 1/50 s), so one is never let go while another still fades.
pub const LET_GO: f32 = 0.005;
/// How many frames a grain stays behind the newest sound, at least: its read between samples (four
/// points, Hermite) reaches one frame nearer.
pub const MARGIN: f64 = 2.0;
/// Where its random numbers start, at its birth and at a reset.
pub const SEED: u32 = 0x9E37_79B9;
/// A grain's length, in seconds, at its shortest and its longest. [copied: src/pictures.ts `zone`, CARD-129]
const SIZES: (f32, f32) = (0.01, 0.5);
/// Grains a second, at the fewest and the most.
const DENSITIES: (f32, f32) = (1.0, 50.0);
/// How far back a grain may start past `back`, in seconds, at the most.
const SPREAD: f32 = 2.0;
/// Pitch, in semitones, either way.
const PITCH: f32 = 12.0;
/// The window, a Hann window from silence to silence, in this many steps, read between them.
const TABLE: usize = 1024;

/// One grain.
#[derive(Clone, Copy)]
struct Grain {
    on: bool,
    /// How many grains started before it: its age, for letting the oldest go.
    born: u64,
    /// The frames written when it started, and how far back from the newest sound it read then, in
    /// frames.
    at: u64,
    back: f64,
    /// How far it reads each frame: `2^(pitch / 12)`.
    step: f64,
    /// Frames it has played, and its length.
    n: u32,
    len: u32,
    /// The window's steps a frame.
    scale: f32,
    /// Frames left of its fade once let go; 0 while it isn't.
    fade: u32,
}

impl Grain {
    const NONE: Grain = Grain { on: false, born: 0, at: 0, back: 0.0, step: 1.0, n: 0, len: 2, scale: 0.0, fade: 0 };
}

/// Grains. Inputs: in, size (s), density (a second), spread (s), pitch (semitones), mix, freeze,
/// playing. Outputs: out, and its meter `reads`.
pub struct Grains {
    sr: f32,
    stereo: bool,
    rings: [Vec<f32>; 2],
    /// Each ring's length: the buffer, and room for the read between samples.
    len: usize,
    /// Frames written into the rings since its birth.
    written: u64,
    window: [f32; TABLE + 1],
    grains: [Grain; MOST + 1],
    /// Grains started since its birth.
    started: u64,
    rng: u32,
    /// Grains' worth of time since the last started, and the gap to the next, in grains.
    clock: f32,
    gap: f32,
    /// Frames written since the last that wasn't silence.
    since_sound: usize,
    mix: Glide,
    level: Glide,
    /// A let go grain's fade, in frames.
    let_go: u32,
}

impl Grains {
    /// The state `new` makes, around zeroed rings.
    fn around(sr: f32, stereo: bool, rings: [Vec<f32>; 2], len: usize) -> Self {
        let mut window = [0.0; TABLE + 1];
        for (k, w) in window.iter_mut().enumerate() {
            *w = (0.5 - 0.5 * (std::f64::consts::TAU * k as f64 / TABLE as f64).cos()) as f32;
        }
        (window[0], window[TABLE]) = (0.0, 0.0);
        Grains {
            sr,
            stereo,
            rings,
            len,
            written: 0,
            window,
            grains: [Grain::NONE; MOST + 1],
            started: 0,
            rng: SEED,
            // The first grain starts as the song does.
            clock: 0.0,
            gap: 0.0,
            since_sound: usize::MAX,
            mix: Glide::new(0.02, sr),
            level: Glide::new(0.05, sr),
            let_go: ((LET_GO * sr) as u32).max(1),
        }
    }

    /// A number from 0 to 1, at random: the next of its xorshift.
    fn unit(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A grain starts: `size` seconds long, from `back + u × spread` seconds back at `pitch`
    /// semitones, `back` keeping it behind the newest sound until it ends: `(ratio − 1) × size` when
    /// that is over 0 (it reads faster than the sound comes), `ratio × size` frozen (no sound comes),
    /// and `MARGIN` frames. With `MOST` playing, the oldest is let go first.
    fn start(&mut self, size: f32, spread: f32, pitch: f32, frozen: bool) {
        let u = self.unit();
        if self.grains.iter().filter(|g| g.on && g.fade == 0).count() >= MOST
            && let Some(oldest) = self.grains.iter_mut().filter(|g| g.on && g.fade == 0).min_by_key(|g| g.born)
        {
            oldest.fade = self.let_go;
        }
        let len = ((size * self.sr).round() as u32).max(2);
        let step = 2f64.powf(pitch as f64 / 12.0);
        let reach = if frozen { step * len as f64 } else { ((step - 1.0) * len as f64).max(0.0) };
        let grain = Grain {
            on: true,
            born: self.started,
            at: self.written,
            back: reach + MARGIN + u as f64 * (spread * self.sr) as f64,
            step,
            n: 0,
            len,
            scale: TABLE as f32 / len as f32,
            fade: 0,
        };
        // Fades are shorter than the gaps, so one slot is always free.
        if let Some(free) = self.grains.iter_mut().find(|g| !g.on) {
            *free = grain;
        }
        self.started += 1;
    }

    /// How many grains play (not counting those let go), how many are let go and fading, when the
    /// oldest playing started, when the youngest let go started (in grains started), and how many
    /// have started: for the tests.
    #[cfg(test)]
    pub(crate) fn census(&self) -> (usize, usize, Option<u64>, Option<u64>, u64) {
        let on = self.grains.iter().filter(|g| g.on);
        (
            on.clone().filter(|g| g.fade == 0).count(),
            on.clone().filter(|g| g.fade > 0).count(),
            on.clone().filter(|g| g.fade == 0).map(|g| g.born).min(),
            on.filter(|g| g.fade > 0).map(|g| g.born).max(),
            self.started,
        )
    }
}

impl Kernel for Grains {
    const INPUTS: usize = 8;
    const OUTPUTS: usize = 2;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(1.0);
        let len = (BUFFER * sr).ceil() as usize + 8;
        let stereo = cfg.channels > 1;
        Grains::around(sr, stereo, [vec![0.0; len], if stereo { vec![0.0; len] } else { vec![] }], len)
    }

    /// As `new` leaves it, its rings cleared in place: a reset takes no memory.
    fn reset(&mut self, cfg: &Cfg) {
        let mut rings = std::mem::take(&mut self.rings);
        rings.iter_mut().for_each(|r| r.fill(0.0));
        let (len, stereo) = (self.len, self.stereo);
        *self = Grains::around(cfg.sample_rate.max(1.0), stereo, rings, len);
    }

    fn process(&mut self, io: &mut Io) {
        let ([l, r, size, density, spread, pitch, mix, freeze, playing], [ol, or]) =
            io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0), (7, 0)], [(0, 0), (0, 1)]);
        let sides = if self.stereo { 2 } else { 1 };
        let furthest = self.len as f64 - 3.0;
        let mut reads = 0.0f64;
        for i in 0..ol.len() {
            let x = [sane(l[i]), if self.stereo { sane(r[i]) } else { 0.0 }];
            let frozen = freeze[i] > 0.5;
            if !frozen {
                let at = (self.written % self.len as u64) as usize;
                for (ring, x) in self.rings.iter_mut().zip(x).take(sides) {
                    ring[at] = x;
                }
                self.written += 1;
                self.since_sound = if x != [0.0; 2] { 0 } else { self.since_sound.saturating_add(1) };
            }
            let (size, density) = (clampf(size[i], SIZES.0, SIZES.1), clampf(density[i], DENSITIES.0, DENSITIES.1));
            if playing[i] > 0.5 {
                self.clock += density / self.sr;
                if self.clock >= self.gap {
                    self.clock -= self.gap;
                    self.start(size, clampf(spread[i], 0.0, SPREAD), clampf(pitch[i], -PITCH, PITCH), frozen);
                    self.gap = 0.5 + self.unit();
                }
            }
            let g = self.level.next(1.0 / (0.375 * (size * density).min(MOST as f32)).max(1.0).sqrt());
            let m = self.mix.next(clampf(mix[i], 0.0, 1.0));
            // Where the newest sound is in the rings. Before anything was written, a ring holds the
            // silence before the song: a frame `d` back is at `newest − d`, round the ring.
            let newest = ((self.written + self.len as u64 - 1) % self.len as u64) as usize;
            let mut wet = [0.0f32; 2];
            let mut youngest: Option<(u64, f64)> = None;
            for grain in self.grains.iter_mut().filter(|g| g.on) {
                // How far back from the newest sound it reads now, in frames.
                let back = ((self.written - grain.at) as f64 + grain.back - grain.n as f64 * grain.step).clamp(MARGIN, furthest);
                if youngest.is_none_or(|(born, _)| grain.born > born) {
                    youngest = Some((grain.born, back));
                }
                let pos = grain.n as f32 * grain.scale;
                let k = (pos as usize).min(TABLE - 1);
                let mut w = self.window[k] + (self.window[k + 1] - self.window[k]) * (pos - k as f32);
                if grain.fade > 0 {
                    w *= grain.fade as f32 / self.let_go as f32;
                    grain.fade -= 1;
                    grain.on = grain.fade > 0;
                }
                let whole = back.floor();
                let (t, j) = ((back - whole) as f32, whole as usize);
                for (c, ring) in self.rings.iter().enumerate().take(sides) {
                    // The sample `d` frames before the newest (`d` under the ring's length).
                    let at = |d: usize| ring[(newest + self.len - d) % self.len];
                    let (y0, y1, y2, y3) = (at(j - 1), at(j), at(j + 1), at(j + 2));
                    let c1 = 0.5 * (y2 - y0);
                    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
                    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
                    wet[c] += w * (((c3 * t + c2) * t + c1) * t + y1);
                }
                grain.n += 1;
                if grain.n >= grain.len {
                    grain.on = false;
                }
            }
            reads = youngest.map_or(0.0, |(_, back)| back / self.sr as f64);
            // At Mix 0, the input as it is, its sign of zero too.
            let out = |c: usize| if m == 0.0 { x[c] } else { x[c] * (1.0 - m) + m * g * wet[c] };
            (ol[i], or[i]) = (out(0), if self.stereo { out(1) } else { 0.0 });
        }
        io.meter(1, reads as f32);
    }

    /// What it took in can come out for as long as its buffer holds it: until a whole buffer of
    /// silence has gone in, or, frozen, for as long as it is frozen.
    fn active(&self) -> bool {
        self.since_sound < self.len
    }
}
