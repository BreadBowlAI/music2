//! The parametric EQ: eight bands, each one of RBJ's cookbook biquads (Robert Bristow-Johnson's
//! "Cookbook formulae for audio EQ biquad filter coefficients": bilinear, prewarped at the band's
//! frequency), run as a TPT state-variable filter (Cytomic's, `m2_dsp::fx::Filter`, as std's `eq3`
//! runs its bands). The SVF has the same response as the direct-form biquad.
//!
//! A band never glides through the settings between two: a resonant band moved through them booms
//! (the filters halfway can be louder than both ends, and a filter whose resonance moves faster than
//! it rings rings up), and there is nothing between two shapes. Instead a band that is asked for a
//! new setting starts a second filter there, from the first one's state; lets it settle while the
//! first still plays (eight times its ringing time, how long its resonance takes to fall by 1/e; a
//! second at most); then crossfades from the first to the second (20 ms). A crossfade of two settled
//! filters is never louder than the louder of them, so no change booms. The settling and the fade
//! scale with how far the setting moves (all of them for a new shape, 3 dB of gain, or half an
//! octave of frequency or Q), so a small move, a knob's drag or a lane's line, is followed at once, as
//! std's `eq3` follows it. On and off crossfade the band with its input. The output is the input
//! plus the band's part times its weight (0 off, 1 on), so a band that is off, or a bell or a shelf
//! at 0 dB, is its input bit for bit: the EQ at its start changes no bit.

use m2_dsp::fx::{Filter, clampf, prewarp, sane};
use m2_kernel::{Cfg, Io, Kernel};

/// Each band's inputs: shape, freq, gain, q, on.
const FIELDS: usize = 5;
const BANDS: usize = 8;
/// How long a band takes to fade in or out, and to crossfade to a new setting a whole move away.
const FADE: f32 = 0.02;
/// A new setting a whole move away settles for this many of its ringing times before the
/// crossfade, at most `MOST_SETTLE` seconds.
const RING_SETTLES: f32 = 8.0;
const MOST_SETTLE: f32 = 1.0;
/// A whole move: a gain this many dB away, or a frequency or Q this many octaves away (or a new
/// shape).
const WHOLE_DB: f32 = 3.0;
const WHOLE_OCTAVES: f32 = 0.5;
/// The loudest the EQ gives: just under +80 dBFS, the most any effect gives (m2-dsp's harness of
/// every effect). Eight bands boosting one frequency could give far more.
const LOUDEST: f32 = 9_999.0;

/// The shapes, by their place in the menu.
#[derive(Clone, Copy, PartialEq, Debug)]
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

/// What a band plays: its shape, frequency (Hz), gain (dB) and Q.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Setting {
    shape: Shape,
    hz: f32,
    db: f32,
    q: f32,
}

impl Setting {
    /// RBJ's response for this setting, as an SVF's `g` (its frequency), `k` (its damping) and
    /// the mix of its input, band-pass and low-pass.
    fn filter(self, sr: f32) -> Filter {
        let g = prewarp(self.hz, sr);
        let a = 10f32.powf(self.db / 40.0);
        let k = 1.0 / self.q;
        match self.shape {
            Shape::LowCut => Filter::new(g, k, [1.0, -k, -1.0]),
            Shape::LowShelf => Filter::new(g / a.sqrt(), k, [1.0, k * (a - 1.0), a * a - 1.0]),
            Shape::Bell => Filter::new(g, k / a, [1.0, k / a * (a * a - 1.0), 0.0]),
            Shape::HighShelf => Filter::new(g * a.sqrt(), k, [a * a, k * (1.0 - a) * a, 1.0 - a * a]),
            Shape::HighCut => Filter::new(g, k, [0.0, 0.0, 1.0]),
        }
    }

    /// How long its resonance takes to fall by 1/e, in seconds: its poles' Q over π times their
    /// frequency. A bell's poles' Q is its Q times its gain as a level's square root; a shelf's poles
    /// sit a gain's quarter power below (low) or above (high) its frequency.
    fn ring(self, sr: f32) -> f32 {
        let a = 10f32.powf(self.db / 40.0);
        let (q, hz) = match self.shape {
            Shape::Bell => (self.q * a, self.hz),
            Shape::LowShelf => (self.q, self.hz / a.sqrt()),
            Shape::HighShelf => (self.q, self.hz * a.sqrt()),
            Shape::LowCut | Shape::HighCut => (self.q, self.hz),
        };
        q / (std::f32::consts::PI * clampf(hz, 10.0, 0.45 * sr))
    }

    /// How far it is to `other`, from 0 to 1: 1 for another shape, and a whole move or more.
    fn apart(self, other: Setting) -> f32 {
        if self.shape != other.shape {
            return 1.0;
        }
        let octaves = |a: f32, b: f32| (a / b).log2().abs() / WHOLE_OCTAVES;
        ((self.db - other.db).abs() / WHOLE_DB).max(octaves(self.hz, other.hz)).max(octaves(self.q, other.q)).min(1.0)
    }
}

/// A setting's filter, and its state for each channel.
#[derive(Clone, Copy)]
struct Side {
    setting: Setting,
    filter: Filter,
    state: [[f32; 2]; 2],
}

/// One band: the setting it is asked for, the filter it plays, and the one it is moving to.
struct Band {
    /// The controls last read, the setting they ask for, and its switch.
    read: [f32; FIELDS],
    want: Setting,
    on: bool,
    /// What it plays, and while it moves, what it moves to: `next` settles for `settle` samples,
    /// then is crossfaded in by `step` a sample, its weight `toward` going from 0 to 1.
    now: Side,
    next: Side,
    moving: bool,
    settle: u32,
    toward: f32,
    step: f32,
    /// How much of the band is heard: 0 off, 1 on.
    heard: f32,
    born: bool,
}

impl Band {
    fn new() -> Band {
        let flat = Setting { shape: Shape::Bell, hz: 1000.0, db: 0.0, q: 1.0 };
        let side = Side { setting: flat, filter: Filter::new(1.0, 1.0, [1.0, 0.0, 0.0]), state: [[0.0; 2]; 2] };
        Band {
            read: [f32::NAN; FIELDS],
            want: flat,
            on: false,
            now: side,
            next: side,
            moving: false,
            settle: 0,
            toward: 0.0,
            step: 1.0,
            heard: 0.0,
            born: false,
        }
    }

    /// Reads this sample's controls, and moves the band a sample toward them.
    fn set(&mut self, sr: f32, read: [f32; FIELDS]) {
        if read != self.read {
            self.read = read;
            let [shape, hz, db, q, on] = read;
            self.want =
                Setting { shape: Shape::at(shape), hz: clampf(hz, 10.0, 0.45 * sr), db: clampf(db, -30.0, 30.0), q: clampf(q, 0.05, 50.0) };
            self.on = on > 0.5;
        }
        if !self.born {
            // Born where its controls are: no settling, no fade.
            self.born = true;
            self.now = Side { setting: self.want, filter: self.want.filter(sr), state: [[0.0; 2]; 2] };
            self.heard = if self.on { 1.0 } else { 0.0 };
        }
        if self.moving && self.toward >= 1.0 {
            (self.now, self.moving) = (self.next, false);
        }
        if !self.moving && self.want != self.now.setting {
            let apart = self.now.setting.apart(self.want);
            self.next = Side { setting: self.want, filter: self.want.filter(sr), state: self.now.state };
            self.settle = ((apart * RING_SETTLES * self.want.ring(sr)).min(MOST_SETTLE) * sr) as u32;
            (self.moving, self.toward, self.step) = (true, 0.0, (1.0 / (apart * FADE * sr)).min(1.0));
        }
        if self.moving {
            if self.settle > 0 {
                self.settle -= 1;
            } else {
                self.toward = (self.toward + self.step).min(1.0);
            }
        }
        let step = 1.0 / (FADE * sr);
        self.heard = if self.on { (self.heard + step).min(1.0) } else { (self.heard - step).max(0.0) };
    }

    /// One sample of channel `c` through the band.
    fn tick(&mut self, c: usize, x: f32) -> f32 {
        let mut y = self.now.filter.tick(&mut self.now.state[c], x);
        if self.moving {
            let next = self.next.filter.tick(&mut self.next.state[c], x);
            if self.toward > 0.0 {
                y += self.toward * (next - y);
            }
        }
        if self.heard == 1.0 {
            y
        } else if self.heard == 0.0 {
            x
        } else {
            x + self.heard * (y - x)
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
        Eq { sr, bands: std::array::from_fn(|_| Band::new()) }
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
                    y = band.tick(c, y);
                }
                out[i] = y.clamp(-LOUDEST, LOUDEST);
            }
        }
    }
}
