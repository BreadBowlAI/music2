//! The Clipper (PACK-34): turns the sound up by `gain`, then cuts every peak over `ceiling` flat,
//! at four times the song's rate, so its cut folds nothing back into hearing; then cuts any sample
//! still over the ceiling, so none ever is.
//!
//! A cut at the song's rate makes harmonics far above half the rate, which fold back as harsh,
//! out-of-tune tones (aliases). So the sound is brought up to four times the rate by two half-band
//! filters, cut there, and what the cut took off is brought back down by the same two, which leave
//! out all of it that would fold back. That is taken from the sound, which goes round the filters,
//! as late as they are: where nothing is cut, the output is the input times the gain, bit for bit.
//!
//! The filters are windowed sincs, linear in phase, so they turn no part of a sound against
//! another, and a sound under the ceiling keeps its shape (all-pass half-bands, which add no
//! latency, lifted a bright sound's peaks by up to 2 dB: PACK-34's why). Brought back down, a cut
//! rings a little past the ceiling (the sound is band-limited again), so the last step cuts each
//! sample still over it, at the song's rate.
//!
//! Every buffer is a fixed array of the state: nothing is taken while it plays (RENDER-28).

use m2_dsp::fx::{Glide, clampf, sane};
use m2_kernel::{Cfg, Io, Kernel, db_to_gain};

/// How late the Clipper is, in frames at every rate (its manifest's `latency`): the first
/// half-band there and back (39 frames), the second (13 samples at twice the rate), and two
/// samples at four times the rate, so the whole is frames.
pub const LATENCY: usize = 46;

/// The first half-band, at twice the song's rate: a sinc under Kaiser's window (β 10.06, for 100
/// dB), 79 taps: those one side of its centre at odd offsets, from the outermost (39) in. Its
/// centre is ½, and its taps at even offsets are 0.
pub const H0: [f32; 20] = [
    -2.7349847e-6,
    1.6305754e-5,
    -5.0468843e-5,
    1.2184385e-4,
    -2.5460613e-4,
    4.8199776e-4,
    -8.4773067e-4,
    1.4073347e-3,
    -2.229698e-3,
    3.3993882e-3,
    -5.020941e-3,
    7.2274627e-3,
    -1.0198276e-2,
    1.4195934e-2,
    -1.9647503e-2,
    2.7338613e-2,
    -3.8944587e-2,
    5.8839753e-2,
    -1.0314409e-1,
    3.1731227e-1,
];

/// The second half-band, at four times the rate: the same window, 27 taps (offsets 13 to 1).
pub const H1: [f32; 7] = [8.204954e-6, -3.6553154e-4, 2.5431921e-3, -1.0198165e-2, 3.0594826e-2, -8.2015835e-2, 3.0943227e-1];

/// What the meter says for silence, in dB, and the least it says.
const FLOOR: f32 = -100.0;

/// Once the cut has taken nothing off for this many frames, every sample the filters that bring it
/// down still read is 0 (the two reach back under 30 frames), so they give exactly nothing: left out.
const QUIET: usize = LATENCY;

/// The last `N` samples of a stream (`N` a power of two), the newest at `back(0)`.
#[derive(Clone, Copy)]
struct Ring<const N: usize> {
    buf: [f32; N],
    at: usize,
}

impl<const N: usize> Ring<N> {
    const NEW: Self = Ring { buf: [0.0; N], at: 0 };

    fn push(&mut self, x: f32) {
        self.at = (self.at + 1) & (N - 1);
        self.buf[self.at] = x;
    }

    fn back(&self, k: usize) -> f32 {
        self.buf[self.at.wrapping_sub(k) & (N - 1)]
    }
}

/// Half-band `taps` (one side of its centre, `m` = 2 × their count − 1 its reach) up: each sample
/// in gives two out, at twice its rate, as late as `m / 2` samples in: first the sinc's, between
/// this sample and the one before; then the sample `(m − 1) / 2` back.
fn up<const N: usize>(x: &mut Ring<N>, taps: &[f32], v: f32) -> [f32; 2] {
    let m = 2 * taps.len() - 1;
    x.push(v);
    let mut acc = 0.0;
    for (q, &h) in taps.iter().enumerate() {
        acc += h * (x.back(q) + x.back(m - q));
    }
    // Zeros put between the samples halve their level: the sinc's twice.
    [2.0 * acc, x.back((m - 1) / 2)]
}

/// Half-band `taps` down: two samples in (`a` then `b`), one out at half their rate, as late as
/// `m / 2` samples out.
fn down<const N: usize, const C: usize>(a: &mut Ring<N>, b: &mut Ring<C>, taps: &[f32], va: f32, vb: f32) -> f32 {
    let m = 2 * taps.len() - 1;
    a.push(va);
    b.push(vb);
    let mut acc = 0.5 * b.back(m.div_ceil(2));
    for (q, &h) in taps.iter().enumerate() {
        acc += h * (a.back(q) + a.back(m - q));
    }
    acc
}

/// One channel's state.
#[derive(Clone, Copy)]
struct Side {
    /// The sound times the gain as it came in: what comes out where nothing is cut.
    dry: Ring<64>,
    /// What each half-band up has heard: at the song's rate, and at twice it.
    up0: Ring<64>,
    up1: Ring<16>,
    /// What the cut took off, coming down: at four times the rate in pairs (`a` the earlier), then
    /// at twice it.
    down1: (Ring<16>, Ring<8>),
    down0: (Ring<64>, Ring<32>),
    /// The last two samples the cut took off, at four times the rate: the two that make the whole
    /// whole frames late.
    held: [f32; 2],
}

impl Side {
    const NEW: Self = Side {
        dry: Ring::NEW,
        up0: Ring::NEW,
        up1: Ring::NEW,
        down1: (Ring::NEW, Ring::NEW),
        down0: (Ring::NEW, Ring::NEW),
        held: [0.0; 2],
    };

    /// One frame of `v`, the sound times the gain, under `c`: what comes out, the last step left
    /// out; whether the cut took anything off; and the loudest of the frame at four times the rate.
    fn frame(&mut self, v: f32, c: f32, cutting: bool) -> (f32, bool, f32) {
        self.dry.push(v);
        let [a, b] = up(&mut self.up0, &H0, v);
        let [u0, u1] = up(&mut self.up1, &H1, a);
        let [u2, u3] = up(&mut self.up1, &H1, b);
        let u = [u0, u1, u2, u3];
        let r = u.map(|u| clampf(u, -c, c) - u);
        let loudest = u.iter().fold(0.0f32, |m, u| m.max(u.abs()));
        let cut = r.iter().any(|&r| r != 0.0);
        let mut y = self.dry.back(LATENCY);
        if cut || cutting {
            let p = [self.held[0], self.held[1], r[0], r[1]];
            self.held = [r[2], r[3]];
            let w0 = down(&mut self.down1.0, &mut self.down1.1, &H1, p[0], p[1]);
            let w1 = down(&mut self.down1.0, &mut self.down1.1, &H1, p[2], p[3]);
            let off = down(&mut self.down0.0, &mut self.down0.1, &H0, w0, w1);
            // Exactly nothing leaves the sound as it is, its sign of zero too.
            if off != 0.0 {
                y += off;
            }
        }
        (y, cut, loudest)
    }
}

/// Clipper. Inputs: in, gain (dB, 0 to 24), ceiling (dB, −24 to 0). Outputs: out, and its meter
/// `pushed`: the loudest it heard after its gain, at four times the rate, in dB, the most of the
/// call's frames.
pub struct Clip {
    sides: [Side; 2],
    stereo: bool,
    gain: Glide,
    ceiling: Glide,
    /// The gain and the ceiling last asked for, in dB, and each as a gain: worked out again only
    /// when they move.
    asked: [f32; 2],
    gains: [f32; 2],
    /// Frames since the cut last took anything off.
    since_cut: usize,
    /// Frames since anything but silence came in.
    since_sound: usize,
}

impl Clip {
    /// One call: `cut` false leaves out the last step, for the tests.
    pub(crate) fn run(&mut self, io: &mut Io, cut: bool) {
        let ([l, r, gain, ceiling], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0), (2, 0)], [(0, 0), (0, 1)]);
        let mut loudest = 0.0f32;
        for i in 0..ol.len() {
            let asked = [clampf(gain[i], 0.0, 24.0), clampf(ceiling[i], -24.0, 0.0)];
            for ((now, was), as_gain) in asked.into_iter().zip(&mut self.asked).zip(&mut self.gains) {
                if now != *was {
                    (*was, *as_gain) = (now, db_to_gain(now));
                }
            }
            let (g, c) = (self.gain.next(self.gains[0]), self.ceiling.next(self.gains[1]));
            let cutting = self.since_cut < QUIET;
            let (vl, vr) = (sane(l[i]) * g, if self.stereo { sane(r[i]) * g } else { 0.0 });
            let (yl, took_l, most_l) = self.sides[0].frame(vl, c, cutting);
            let (yr, took_r, most_r) = if self.stereo { self.sides[1].frame(vr, c, cutting) } else { (0.0, false, 0.0) };
            self.since_cut = if took_l || took_r { 0 } else { self.since_cut.saturating_add(1) };
            self.since_sound = if vl != 0.0 || vr != 0.0 { 0 } else { self.since_sound.saturating_add(1) };
            loudest = loudest.max(most_l).max(most_r);
            (ol[i], or[i]) = if cut { (clampf(yl, -c, c), clampf(yr, -c, c)) } else { (yl, yr) };
        }
        io.meter(1, if loudest > 0.0 { (20.0 * loudest.log10()).max(FLOOR) } else { FLOOR });
    }
}

impl Kernel for Clip {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 2;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(1.0);
        Clip {
            sides: [Side::NEW; 2],
            stereo: cfg.channels > 1,
            gain: Glide::new(0.02, sr),
            ceiling: Glide::new(0.02, sr),
            asked: [f32::NAN; 2],
            gains: [1.0; 2],
            since_cut: QUIET,
            since_sound: usize::MAX,
        }
    }

    fn process(&mut self, io: &mut Io) {
        self.run(io, true);
    }

    /// What came in comes out `LATENCY` frames later, and what the cut took off over as long again.
    fn active(&self) -> bool {
        self.since_sound < 2 * LATENCY
    }
}
