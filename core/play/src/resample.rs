//! Rate conversion at the device boundary (RENDER-31): the engine renders at the project's
//! rate, and when the device runs at another, its output is converted here, after the engine
//! and before the device. Offline renders never pass through it.
//!
//! A windowed-sinc interpolator with a fixed ratio. The position between input frames is kept
//! as an exact fraction, so it never drifts, and the filter's cutoff sits below the lower of
//! the two rates' Nyquist frequencies, so downsampling doesn't alias.

use anyhow::Result;

/// Taps on each side of the interpolated point.
const HALF: usize = 16;
const TAPS: usize = 2 * HALF;
/// Filter phases between two input frames; the ones between are interpolated.
const PHASES: usize = 256;
/// Input frames pulled from the engine at a time.
const CHUNK: usize = 64;
/// How much input is kept before it is dropped from the front.
const KEEP: usize = 4096;

/// Converts stereo frames from the engine's rate to the device's.
pub struct Resampler {
    from: u64,
    to: u64,
    /// Input frames; the next output frame's taps start at `start`.
    input: Vec<[f32; 2]>,
    start: usize,
    /// Where the next output frame falls past the frame at `start + HALF - 1`, as `acc / to`.
    acc: u64,
    /// `PHASES + 1` rows of `TAPS` coefficients, each row summing to 1.
    table: Vec<f32>,
}

impl Resampler {
    /// A converter from `from` to `to` frames per second. Made off the audio thread: it builds
    /// its filter and reserves all the memory it uses.
    pub fn new(from: u32, to: u32) -> Resampler {
        // Below the lower Nyquist frequency, with room for the filter's transition.
        let cutoff = 0.92 * (to as f64 / from as f64).min(1.0);
        let mut table = Vec::with_capacity((PHASES + 1) * TAPS);
        for p in 0..=PHASES {
            let frac = p as f64 / PHASES as f64;
            let row: Vec<f64> = (0..TAPS)
                .map(|k| {
                    let x = k as f64 - (HALF - 1) as f64 - frac;
                    // libm, as on the render path: the same table on every OS (PLAT-4).
                    let sinc =
                        if x == 0.0 { 1.0 } else { libm::sin(std::f64::consts::PI * cutoff * x) / (std::f64::consts::PI * cutoff * x) };
                    // Blackman, over the taps' whole span.
                    let w = (x + HALF as f64) / TAPS as f64;
                    let window = 0.42 - 0.5 * libm::cos(2.0 * std::f64::consts::PI * w) + 0.08 * libm::cos(4.0 * std::f64::consts::PI * w);
                    sinc * window.max(0.0)
                })
                .collect();
            let sum: f64 = row.iter().sum();
            table.extend(row.iter().map(|c| (c / sum) as f32));
        }
        // Silence before the first frame, so output frame 0 is input frame 0.
        let mut input = Vec::with_capacity(KEEP + TAPS + CHUNK);
        input.resize(HALF - 1, [0.0; 2]);
        Resampler { from: from as u64, to: to as u64, input, start: 0, acc: 0, table }
    }

    /// Fill `out` with frames at the device's rate, pulling engine frames from `pull` as they
    /// are needed. It pulls up to `HALF` frames ahead of what it outputs.
    pub fn fill(&mut self, out: &mut [[f32; 2]], pull: &mut dyn FnMut(&mut [[f32; 2]]) -> Result<()>) -> Result<()> {
        for o in out {
            while self.input.len() < self.start + TAPS {
                let n = self.input.len();
                self.input.resize(n + CHUNK, [0.0; 2]);
                pull(&mut self.input[n..])?;
            }
            let at = (self.acc * PHASES as u64) as f64 / self.to as f64;
            let p = at as usize;
            let w = (at - p as f64) as f32;
            let (a, b) = (&self.table[p * TAPS..(p + 1) * TAPS], &self.table[(p + 1).min(PHASES) * TAPS..][..TAPS]);
            let x = &self.input[self.start..self.start + TAPS];
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for k in 0..TAPS {
                let h = a[k] + (b[k] - a[k]) * w;
                l += x[k][0] * h;
                r += x[k][1] * h;
            }
            *o = [l, r];
            self.acc += self.from;
            while self.acc >= self.to {
                self.acc -= self.to;
                self.start += 1;
            }
            if self.start >= KEEP {
                // Within the memory reserved: draining never reallocates.
                self.input.drain(..self.start);
                self.start = 0;
            }
        }
        Ok(())
    }
}
