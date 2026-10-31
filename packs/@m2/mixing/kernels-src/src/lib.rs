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

/// A follower of a level that moves (the transient shaper's), a one-pole's way: it lands on its
/// level only once the level holds still, when within a millionth of it or when a step no longer
/// moves it (a slow float one-pole stalls short). Chasing a level that moves, it is never snapped
/// onto it, however small the gap: a slow follower snapped onto a slowly falling level would never
/// hang over it (review Z1). Controls glide with `m2_dsp::fx::Glide`, std's, which lands within a
/// millionth of any target, moving or not: right for a control, wrong for this.
struct Follow {
    value: f32,
    /// The level it was given last.
    last: f32,
}

impl Follow {
    fn new() -> Follow {
        Follow { value: 0.0, last: 0.0 }
    }

    /// A sample toward `level` by `coef` (a one-pole's).
    fn next(&mut self, level: f32, coef: f32) -> f32 {
        let still = level == self.last;
        self.last = level;
        if self.value != level {
            let v = level + (self.value - level) * coef;
            let landed = still && (v == self.value || (v - level).abs() <= 1e-6 * level.abs());
            self.value = if landed { level } else { m2_dsp::fx::flush(v) };
        }
        self.value
    }

    /// At `level` at once.
    fn set(&mut self, level: f32) -> f32 {
        (self.value, self.last) = (level, level);
        level
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
