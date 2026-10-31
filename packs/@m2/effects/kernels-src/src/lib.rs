//! The kernels of the pack `@m2/effects`, its own sound code (PACK-21): each is called by the
//! pack's id, `kernel("@m2/effects:bitcrush", …)`. Port order must match the pack's `m2pkg.toml`.
//! Written with std's kit and the blocks std's kernels share (`m2_dsp`), built with std's kernels by
//! `std/build.sh` into `packs/@m2/effects/kernels/effects.wasm` (ABI-10); its crate's manifest is
//! `std/kernels-src/packs/@m2/effects/Cargo.toml`. This folder is no part of the pack (PACK-20): only
//! the module built from it is.
//!
//! Each is safe to feed anything, as std's effects are: audio that isn't finite counts as silence
//! and absurd levels are capped before they reach any state; controls are clamped to their useful
//! range (NaN takes the low end), and controls that shape the sound are smoothed, so turning a
//! knob doesn't click. Controls are read every sample and nothing depends on block boundaries, so
//! the output is the same however the host splits its blocks.
//!
//! The bitcrusher follows the song, not the engine: its last input, `playing`, is 1 while the song
//! plays (the pack's `bitcrush` passes `time.playing`; left out it is 1). Its clock is put back at
//! Play to where it was at Stop, so after the engine sat stopped it plays the export, bit for bit
//! (RENDER-33). It was std's `std:bitcrush` until std 0.8, and plays the same bits.

use m2_dsp::fx::{Glide, clampf, sane};
use m2_kernel::{Cfg, Io, Kernel, abi, export};

abi!();

// ---------------------------------------------------------------- lo-fi

/// Bitcrusher: holds samples at a lower `rate` (Hz) and rounds them to `bits` of
/// resolution (fractional bits sweep smoothly). Inputs: in, bits, rate, mix, playing.
///
/// Its clock runs on while the song is stopped, so a tail is still crushed; it can't wait, as a
/// held sample would freeze into a DC level. Instead it is put back at Play to where it was at
/// Stop, or to its start, 1, if the song has not played: where the export's clock is.
pub struct Bitcrush {
    sr: f32,
    /// Fraction of a held sample elapsed; a new sample is taken when it reaches 1.
    clock: f32,
    /// The clock at the last Stop, put back at the next Play.
    kept: f32,
    /// Whether the song played at the last sample.
    playing: bool,
    held: [f32; 2],
    bits: f32,
    /// Quantisation steps per unit for `bits`.
    steps: f32,
    mix: Glide,
}

impl Kernel for Bitcrush {
    const INPUTS: usize = 5;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        Bitcrush { sr, clock: 1.0, kept: 1.0, playing: false, held: [0.0; 2], bits: f32::NAN, steps: 1.0, mix: Glide::new(0.02, sr) }
    }

    fn process(&mut self, io: &mut Io) {
        let stereo = io.channels > 1;
        let ([l, r, bits, rate, mix, playing], [ol, or]) = io.ports([(0, 0), (0, 1), (1, 0), (2, 0), (3, 0), (4, 0)], [(0, 0), (0, 1)]);
        for i in 0..ol.len() {
            let now = playing[i] > 0.5;
            if now != self.playing {
                if now {
                    self.clock = self.kept;
                } else {
                    self.kept = self.clock;
                }
                self.playing = now;
            }
            let b = clampf(bits[i], 1.0, 24.0);
            if b != self.bits {
                self.bits = b;
                self.steps = (b - 1.0).exp2();
            }
            let x = [sane(l[i]), if stereo { sane(r[i]) } else { 0.0 }];
            if self.clock >= 1.0 {
                self.clock -= 1.0;
                self.held = x.map(|x| (x * self.steps).round() / self.steps);
            }
            self.clock += clampf(rate[i], 20.0, self.sr) / self.sr;
            let m = self.mix.next(clampf(mix[i], 0.0, 1.0));
            ol[i] = x[0] * (1.0 - m) + self.held[0] * m;
            or[i] = x[1] * (1.0 - m) + self.held[1] * m;
        }
    }
}

export!(bitcrush, Bitcrush);

#[cfg(test)]
mod tests;
