//! A look's spectrum (TAP-13): how loud a sound is at each pitch, worked out on the server from a
//! ring's frames, off the audio thread and never under the control plane's lock.
//!
//! - **Two FFTs.** For the lows, one of [`LOWS`] frames (the ring's last 16,256, the rest silence:
//!   2.9 Hz apart at 48 kHz, so a kick and a bass are really measured); for the highs, one of
//!   [`HIGHS`] frames, a quarter of the work. Each of the channels' mean, through a Hann window, by a
//!   real FFT in f32 (an FFT of half the length, of pairs of frames), with tables made once.
//! - **Bands.** Band `k` is centred on 20 × 2^(k/12) Hz and a twelfth of an octave wide, up to the
//!   last below 20 kHz and below half the rate ([`centres`]). Each sums the power of the bins in it;
//!   a band narrower than three bins sums the three nearest its centre. Above that, a bin's power
//!   goes to the bands by how much of it each covers. Below [`JOIN`]`.0` a band is
//!   the low FFT's, from [`JOIN`]`.1` up the high one's, and between them it mixes the two (by where
//!   it is between them, on a log scale), so no power is lost where they meet.
//! - **dB.** A sine at full scale reads 0 dB in its band; each band is rounded to 0.1 dB, and is
//!   never under −120.
//! - **A look's own** ([`Spectrum`]): its buffers, and the lows it last had, worked out at every
//!   fourth read (their window is 340 ms long, so they change slowly) and kept while a new plan's
//!   ring fills.

use std::f32::consts::TAU;
use std::sync::OnceLock;

/// The FFT's frames for the lows, and for the highs.
pub const LOWS: usize = 1 << 14;
pub const HIGHS: usize = 1 << 12;
/// The frames the low FFT reads at most: a ring's, less the block being written.
pub const LOW_FRAMES: usize = LOWS - 128;
/// Where the bands go from the low FFT's to the high one's, in Hz.
pub const JOIN: (f64, f64) = (630.0, 1000.0);
/// How often the lows are worked out again: at every this many reads.
pub const LOWS_EVERY: u32 = 4;
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
/// [`centres`], worked out whole: [`Spectrum::bands`] with nothing kept.
pub fn bands(frames: &[[f32; 2]], channels: u8, rate: u32) -> Vec<f64> {
    Spectrum::default().bands(frames, channels, rate)
}

/// A look's spectrum, between its reads: the lows it last worked out, at what rate, and how many
/// reads ago.
#[derive(Default)]
pub struct Spectrum {
    lows: Vec<f32>,
    rate: u32,
    reads: u32,
    mono: Vec<f32>,
}

impl Spectrum {
    /// The spectrum of `frames` (the latest of a ring; their first `channels` sides, at `rate`), in
    /// dB for each band of [`centres`]. The lows are worked out again once [`LOWS_EVERY`] reads
    /// have gone by since they last were, at the first with [`LOW_FRAMES`] frames or more, and
    /// whenever there are none yet or the rate changed; otherwise they are the ones it last had (a
    /// new plan's ring filling).
    pub fn bands(&mut self, frames: &[[f32; 2]], channels: u8, rate: u32) -> Vec<f64> {
        let both = channels >= 2;
        self.mono.clear();
        self.mono.extend(frames.iter().map(|f| if both { (f[0] + f[1]) * 0.5 } else { f[0] }));
        let fresh = self.lows.is_empty() || self.rate != rate;
        if fresh || (self.reads >= LOWS_EVERY && self.mono.len() >= LOW_FRAMES) {
            self.lows = power(&self.mono[self.mono.len().saturating_sub(LOW_FRAMES)..], LOWS);
            (self.rate, self.reads) = (rate, 0);
        }
        self.reads = self.reads.wrapping_add(1);
        let highs = power(&self.mono[self.mono.len().saturating_sub(HIGHS)..], HIGHS);
        let (lw, hw) = (rate as f64 / LOWS as f64, rate as f64 / HIGHS as f64);
        let span = (JOIN.1 / JOIN.0).log2();
        centres(rate)
            .into_iter()
            .map(|c| {
                let high = ((c / JOIN.0).log2() / span).clamp(0.0, 1.0);
                let low = if high < 1.0 { band(&self.lows, lw, c) } else { 0.0 };
                let up = if high > 0.0 { band(&highs, hw, c) } else { 0.0 };
                let db = 10.0 * ((low * (1.0 - high) + up * high) / 0.5).log10();
                (db.max(FLOOR) * 10.0).round() / 10.0
            })
            .collect()
    }
}

/// The power of the band around `centre` from bins `width` Hz apart: each bin's power for the part
/// of it the band covers (bin `k` covers `(k ± ½) × width`), so the bands share every bin's power
/// out; or, for a band narrower than three bins, the three nearest its centre.
fn band(p: &[f32], width: f64, centre: f64) -> f64 {
    let (lo, hi) = (centre * 2f64.powf(-1.0 / 24.0), centre * 2f64.powf(1.0 / 24.0));
    if hi - lo < 3.0 * width {
        let at = (centre / width).round() as usize;
        return p[at.saturating_sub(1).min(p.len())..(at + 2).min(p.len())].iter().map(|&x| x as f64).sum();
    }
    let (first, last) = ((lo / width + 0.5).floor() as usize, ((hi / width + 0.5).ceil() as usize).min(p.len()));
    (first..last)
        .map(|k| {
            let (a, b) = ((k as f64 - 0.5) * width, (k as f64 + 0.5) * width);
            p[k] as f64 * ((hi.min(b) - lo.max(a)) / width).max(0.0)
        })
        .sum()
}

/// Each bin's share of the sound's mean square, from 0 Hz to half the rate: `x` through a Hann
/// window of its length, then silence to `n` frames. A sine of amplitude `A` sums to `A²/2` over
/// its bins.
fn power(x: &[f32], n: usize) -> Vec<f32> {
    let t = Tables::of(n);
    let (m, half) = (x.len(), n / 2);
    let made;
    let w: &[f32] = match m {
        LOW_FRAMES | HIGHS => window(m),
        _ => {
            made = hann(m);
            &made
        }
    };
    let (mut re, mut im) = (vec![0.0f32; half], vec![0.0f32; half]);
    // Pairs of frames as one complex number each: an FFT of half the length.
    for (i, (&v, &wi)) in x.iter().zip(w).enumerate() {
        if i % 2 == 0 {
            re[i / 2] = v * wi;
        } else {
            im[i / 2] = v * wi;
        }
    }
    let energy: f64 = w.iter().map(|&x| (x * x) as f64).sum();
    fft(&mut re, &mut im, t);
    let scale = if energy > 0.0 { (2.0 / (n as f64 * energy)) as f32 } else { 0.0 };
    // Undo the pairing: bin k of the real input from bins k and half - k.
    (0..half)
        .map(|k| {
            let j = (half - k) % half;
            let (ar, ai, br, bi) = (re[k], im[k], re[j], -im[j]);
            let (er, ei) = ((ar + br) * 0.5, (ai + bi) * 0.5);
            let (or, oi) = ((ai - bi) * 0.5, (br - ar) * 0.5);
            let (c, s) = t.real[k];
            let (xr, xi) = (er + or * c - oi * s, ei + or * s + oi * c);
            (xr * xr + xi * xi) * scale
        })
        .collect()
}

/// A Hann window of `m` frames.
fn hann(m: usize) -> Vec<f32> {
    (0..m).map(|i| 0.5 - 0.5 * (TAU * i as f32 / m.max(2) as f32).cos()).collect()
}

/// A Hann window of [`LOW_FRAMES`] or [`HIGHS`] frames, made once.
fn window(m: usize) -> &'static [f32] {
    static W: [OnceLock<Vec<f32>>; 2] = [OnceLock::new(), OnceLock::new()];
    W[usize::from(m == HIGHS)].get_or_init(|| hann(m))
}

/// What a real FFT of [`LOWS`] or [`HIGHS`] frames needs, made once: the bit reversal and the
/// twiddles of its half, and the turns that undo the pairing.
struct Tables {
    reverse: Vec<u32>,
    twiddle: Vec<(f32, f32)>,
    real: Vec<(f32, f32)>,
}

impl Tables {
    fn of(n: usize) -> &'static Tables {
        static T: [OnceLock<Tables>; 2] = [OnceLock::new(), OnceLock::new()];
        debug_assert!(n == LOWS || n == HIGHS);
        T[usize::from(n == HIGHS)].get_or_init(|| {
            let half = n / 2;
            let bits = half.trailing_zeros();
            let turn = |k: usize, of: usize| {
                let a = -std::f64::consts::TAU * k as f64 / of as f64;
                (a.cos() as f32, a.sin() as f32)
            };
            Tables {
                reverse: (0..half as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect(),
                twiddle: (0..half / 2).map(|k| turn(k, half)).collect(),
                real: (0..half).map(|k| turn(k, n)).collect(),
            }
        })
    }
}

/// An FFT in place, of a power of two, by `t`'s tables: each stage block by block, its butterflies
/// inside, so the arrays are walked in order.
fn fft(re: &mut [f32], im: &mut [f32], t: &Tables) {
    let n = re.len();
    for i in 0..n {
        let j = t.reverse[i] as usize;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let (half, stride) = (len / 2, n / len);
        for start in (0..n).step_by(len) {
            for k in 0..half {
                let (wr, wi) = t.twiddle[k * stride];
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
