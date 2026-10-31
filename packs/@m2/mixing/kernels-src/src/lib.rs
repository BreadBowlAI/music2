//! The kernels of the pack `@m2/mixing`, its own sound code (PACK-21, PACK-26): each is called by
//! the pack's id, `kernel("@m2/mixing:eq", …)`. Port order must match the pack's `m2pkg.toml`.
//! Written with std's kit and the blocks std's kernels share (`m2_dsp`), built with std's kernels by
//! `std/build.sh` into `packs/@m2/mixing/kernels/mixing.wasm` (ABI-10); its crate's manifest is
//! `std/kernels-src/packs/@m2/mixing/Cargo.toml`. This folder is no part of the pack (PACK-20): only
//! the module built from it is.
//!
//! | Kernel | Module | What it is |
//! |---|---|---|
//! | `eq` | [`parametric`] | eight bands of RBJ's biquads, run as TPT state-variable filters |
//! | `gate` | [`dynamics`] | a gate with hysteresis, keyed by its input or another sound |
//! | `transient` | [`dynamics`] | a transient shaper: two followers of the level |
//! | `deesser` | [`dynamics`] | a band around a frequency, turned down while it is too loud |
//! | `multiband` | [`bands`] | three compressors on Linkwitz-Riley bands |
//!
//! Each is safe to feed anything, as std's effects are: audio that isn't finite counts as silence
//! and absurd levels are capped before they reach any state; controls are clamped to their useful
//! range (NaN takes the low end), and smoothed where a step would click. Controls are read every
//! sample and nothing depends on block boundaries, so the output is the same however the host
//! splits its blocks. None takes memory but its state, and none allocates while it plays (ABI-5).
//! Silence brings each back to the state it was born in, exactly, so a song played after the
//! engine sat stopped plays its export.

use m2_kernel::{abi, export};

pub mod bands;
pub mod dynamics;
pub mod parametric;

pub use bands::Multiband;
pub use dynamics::{Deesser, Gate, Transient};
pub use parametric::Eq;

abi!();

export!(eq, Eq);
export!(gate, Gate);
export!(transient, Transient);
export!(deesser, Deesser);
export!(multiband, Multiband);

/// One sample of `value` moving toward `target` by `coef` (a one-pole's), landing on it once it is
/// within a millionth, or once a step no longer moves it (a float one-pole stalls short of its
/// target, a few hundred-thousandths away when it is slow): so a follower that has caught up holds
/// its target exactly, and a ratio of two that agree is exactly 1.
fn toward(value: f32, target: f32, coef: f32) -> f32 {
    let v = target + (value - target) * coef;
    if v == value || (v - target).abs() <= 1e-6 * target.abs() { target } else { m2_dsp::fx::flush(v) }
}

/// A value that glides to each new target, a one-pole's way, starting on its first: a control
/// that would click if it stepped, or a filter's coefficient. It lands as [`toward`] does, where
/// `m2_dsp::fx::Glide` (std's, whose bits must not change) may stall just short.
struct Slide {
    value: f32,
    coef: f32,
    primed: bool,
}

impl Slide {
    fn new(time: f32, sr: f32) -> Slide {
        Slide { value: 0.0, coef: pole(time, sr), primed: false }
    }

    fn next(&mut self, target: f32) -> f32 {
        if !self.primed {
            self.primed = true;
            self.value = target;
        } else if self.value != target {
            self.value = toward(self.value, target, self.coef);
        }
        self.value
    }
}

/// The one-pole coefficient for a time constant of `time` seconds.
fn pole(time: f32, sr: f32) -> f32 {
    (-1.0 / (time * sr)).exp()
}

/// Gain from dB, exactly 1 at 0 dB.
fn gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests;
