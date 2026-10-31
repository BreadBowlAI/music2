//! The parametric EQ: eight bands, each one of RBJ's cookbook biquads (Robert Bristow-Johnson's
//! "Cookbook formulae for audio EQ biquad filter coefficients": bilinear, prewarped at the band's
//! frequency), run as a TPT state-variable filter (Cytomic's, `m2_dsp::fx::Filter`). The SVF has the
//! same response as the direct-form biquad, and stays clean while its values move: its state is
//! the same for every shape, so a band's shape can change while it plays.
//!
//! A band is the SVF's coefficients `g` (its frequency), `k` (its damping) and the mix `m` of its
//! input, band-pass and low-pass. Each value a band reads (shape, frequency, gain, Q, on) sets
//! where those five go; they glide there over about 20 ms, so no move clicks. A band that is off
//! is the mix (1, 0, 0), its input as it is; a bell or a shelf at 0 dB is the same, so the EQ at its
//! start changes no bit.

use m2_dsp::fx::{Filter, clampf, prewarp, sane};
use m2_kernel::{Cfg, Io, Kernel};

use crate::Slide;

/// Each band's inputs: shape, freq, gain, q, on.
const FIELDS: usize = 5;
const BANDS: usize = 8;
/// How long a band's coefficients take to reach a new value (a one-pole's time constant).
const GLIDE: f32 = 0.02;

/// The shapes, by their place in the menu.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    LowCut,
    LowShelf,
    Bell,
    HighShelf,
    HighCut,
}

impl Shape {
    /// The shape a choice's place picks; anything else, the nearest.
    fn at(place: f32) -> Shape {
        match clampf(place, 0.0, 4.0).round() as u32 {
            0 => Shape::LowCut,
            1 => Shape::LowShelf,
            2 => Shape::Bell,
            3 => Shape::HighShelf,
            _ => Shape::HighCut,
        }
    }
}

/// One band: what its coefficients glide to, and where they are.
struct Band {
    /// The values the target was worked out from.
    read: [f32; FIELDS],
    /// `g`, `k` and the mix `m`, as RBJ's shape gives them.
    target: [f32; 5],
    glide: [Slide; 5],
    /// The coefficients `filter` was made from.
    now: [f32; 5],
    filter: Filter,
    /// Each channel's state.
    state: [[f32; 2]; 2],
}

impl Band {
    fn new(sr: f32) -> Band {
        let flat = [prewarp(1000.0, sr), 1.0, 1.0, 0.0, 0.0];
        Band {
            read: [f32::NAN; FIELDS],
            target: flat,
            glide: std::array::from_fn(|_| Slide::new(GLIDE, sr)),
            now: [f32::NAN; 5],
            filter: Filter::new(flat[0], flat[1], [1.0, 0.0, 0.0]),
            state: [[0.0; 2]; 2],
        }
    }

    /// Where the coefficients go for `shape` at `hz`, `db` and `q`: RBJ's responses as an SVF's.
    fn coefficients(sr: f32, shape: Shape, hz: f32, db: f32, q: f32, on: bool) -> [f32; 5] {
        let g = prewarp(hz, sr);
        let a = 10f32.powf(db / 40.0);
        let k = 1.0 / q;
        let [g, k, m0, m1, m2] = match shape {
            Shape::LowCut => [g, k, 1.0, -k, -1.0],
            Shape::LowShelf => [g / a.sqrt(), k, 1.0, k * (a - 1.0), a * a - 1.0],
            Shape::Bell => [g, k / a, 1.0, k / a * (a * a - 1.0), 0.0],
            Shape::HighShelf => [g * a.sqrt(), k, a * a, k * (1.0 - a) * a, 1.0 - a * a],
            Shape::HighCut => [g, k, 0.0, 0.0, 1.0],
        };
        // Off, the state runs on at the band's own frequency, so turning it on again glides from
        // a state that is ready.
        if on { [g, k, m0, m1, m2] } else { [g, k, 1.0, 0.0, 0.0] }
    }

    /// Reads this sample's values, and moves the coefficients a sample toward theirs.
    fn set(&mut self, sr: f32, read: [f32; FIELDS]) {
        if read != self.read {
            self.read = read;
            let [shape, hz, db, q, on] = read;
            self.target = Band::coefficients(
                sr,
                Shape::at(shape),
                clampf(hz, 10.0, 0.45 * sr),
                clampf(db, -30.0, 30.0),
                clampf(q, 0.05, 50.0),
                on > 0.5,
            );
        }
        let c: [f32; 5] = std::array::from_fn(|i| self.glide[i].next(self.target[i]));
        if c != self.now {
            self.now = c;
            self.filter = Filter::new(c[0], c[1], [c[2], c[3], c[4]]);
        }
    }
}

/// The EQ. Inputs: in, then for each band 1 to 8 its shape, freq, gain, q and on.
pub struct Eq {
    sr: f32,
    bands: [Band; BANDS],
}

impl Kernel for Eq {
    const INPUTS: usize = 1 + BANDS * FIELDS;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sr = cfg.sample_rate.max(1.0);
        Eq { sr, bands: std::array::from_fn(|_| Band::new(sr)) }
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.min(2);
        let ins: [(usize, usize); 2 + BANDS * FIELDS] = std::array::from_fn(|i| if i < 2 { (0, i) } else { (i - 1, 0) });
        let (ins, [ol, or]) = io.ports(ins, [(0, 0), (0, 1)]);
        let (audio, controls) = ins.split_at(2);
        let mut outs = [ol, or];
        for i in 0..outs[0].len() {
            for (b, band) in self.bands.iter_mut().enumerate() {
                band.set(self.sr, std::array::from_fn(|f| controls[b * FIELDS + f][i]));
            }
            for (c, out) in outs.iter_mut().enumerate() {
                if c >= channels {
                    out[i] = 0.0;
                    continue;
                }
                let mut y = sane(audio[c][i]);
                for band in &mut self.bands {
                    y = band.filter.tick(&mut band.state[c], y);
                }
                out[i] = y;
            }
        }
    }
}
