//! A look's spectrum (TAP-13): how loud a sound is at each pitch, worked out on the server from a
//! ring's frames, off the audio thread.
//!
//! - **Two FFTs.** Below [`SPLIT`] (500 Hz), one of [`LOWS`] frames (the ring's last 16,256, the rest
//!   silence: 2.9 Hz apart at 48 kHz, so a kick and a bass are really measured); from it up, one of
//!   [`HIGHS`] frames, a quarter of the work. Each of the channels' mean, through a Hann window.
//! - **Bands.** Band `k` is centred on 20 × 2^(k/12) Hz and a twelfth of an octave wide, up to the
//!   last below 20 kHz and below half the rate ([`centres`]). Each sums the power of the bins in it;
//!   a band narrower than a bin takes the power the bins around it say there is at its centre, for
//!   its width.
//! - **dB.** A sine at full scale reads 0 dB in its band; each band is rounded to 0.1 dB, and is
//!   never under −120.

use std::f64::consts::{PI, TAU};

/// The FFT's frames below [`SPLIT`], and from it up.
pub const LOWS: usize = 1 << 14;
pub const HIGHS: usize = 1 << 12;
/// Where the high FFT takes over, in Hz.
pub const SPLIT: f64 = 500.0;
/// The lowest band's centre, and the top every band is under.
const LOWEST: f64 = 20.0;
const TOP: f64 = 20_000.0;
/// What a band says of no sound.
const FLOOR: f64 = -120.0;

/// Each band's centre at `rate`, in Hz.
pub fn centres(rate: u32) -> Vec<f64> {
    let top = TOP.min(rate as f64 / 2.0);
    (0..).map(|k| LOWEST * 2f64.powf(k as f64 / 12.0)).take_while(|&c| c < top).collect()
}

/// The spectrum of `frames` (their first `channels` sides, at `rate`), in dB for each band of
/// [`centres`]: the latest [`HIGHS`] frames from [`SPLIT`] up, the latest [`LOWS`] below it. Fewer
/// frames are as many as there are, the rest silence.
pub fn bands(frames: &[[f32; 2]], channels: u8, rate: u32) -> Vec<f64> {
    let both = channels >= 2;
    let mono: Vec<f64> = frames.iter().map(|f| if both { (f[0] as f64 + f[1] as f64) / 2.0 } else { f[0] as f64 }).collect();
    let lows = power(&mono[mono.len().saturating_sub(LOWS)..], LOWS);
    let highs = power(&mono[mono.len().saturating_sub(HIGHS)..], HIGHS);
    centres(rate)
        .into_iter()
        .map(|c| {
            let (p, n) = if c < SPLIT { (&lows, LOWS) } else { (&highs, HIGHS) };
            let db = 10.0 * (band(p, rate as f64 / n as f64, c) / 0.5).log10();
            (db.max(FLOOR) * 10.0).round() / 10.0
        })
        .collect()
}

/// The power of the band around `centre` from bins `width` Hz apart: the bins whose frequency is in
/// it, or, when none is, the power the two around its centre say there is there, for its width.
fn band(p: &[f64], width: f64, centre: f64) -> f64 {
    let (lo, hi) = (centre * 2f64.powf(-1.0 / 24.0), centre * 2f64.powf(1.0 / 24.0));
    let (first, last) = ((lo / width).ceil() as usize, ((hi / width).ceil() as usize).min(p.len()));
    if first < last {
        return p[first..last].iter().sum();
    }
    let at = centre / width;
    let (k, f) = (at.floor() as usize, at.fract());
    let (a, b) = (p.get(k).copied().unwrap_or(0.0), p.get(k + 1).copied().unwrap_or(0.0));
    (a + (b - a) * f) * (hi - lo) / width
}

/// Each bin's share of the sound's mean square, from 0 Hz to half the rate: `x` through a Hann
/// window of its length, then silence to `n` frames. A sine of amplitude `A` sums to `A²/2` over
/// its bins.
fn power(x: &[f64], n: usize) -> Vec<f64> {
    let m = x.len().max(2) as f64;
    let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
    let mut energy = 0.0;
    for (i, &v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (TAU * i as f64 / m).cos();
        re[i] = v * w;
        energy += w * w;
    }
    fft(&mut re, &mut im);
    let scale = if energy > 0.0 { 2.0 / (n as f64 * energy) } else { 0.0 };
    (0..n / 2).map(|k| (re[k] * re[k] + im[k] * im[k]) * scale).collect()
}

/// An FFT in place, of a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        for k in 0..half {
            let a = -PI * k as f64 / half as f64;
            let (wr, wi) = (a.cos(), a.sin());
            for start in (0..n).step_by(len) {
                let (x, y) = (start + k, start + k + half);
                let (tr, ti) = (re[y] * wr - im[y] * wi, re[y] * wi + im[y] * wr);
                (re[y], im[y]) = (re[x] - tr, im[x] - ti);
                re[x] += tr;
                im[x] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
#[path = "spectrum_tests.rs"]
mod tests;
