//! A radix-2 FFT, for making tables while a voice is born: never while it plays.

use std::f64::consts::PI;

/// The longest transform: the longest level's copy of a frame.
pub const MAX: usize = 8192;

/// The turns a transform of up to [`MAX`] points takes: `e^(-2πik/MAX)` for k below `MAX / 2`.
pub struct Fft {
    cos: Vec<f64>,
    sin: Vec<f64>,
}

impl Fft {
    pub fn new() -> Self {
        let (cos, sin) = (0..MAX / 2).map(|k| 2.0 * PI * k as f64 / MAX as f64).map(|a| (a.cos(), -a.sin())).unzip();
        Fft { cos, sin }
    }

    /// `(re, im)` transformed in place, unscaled: forward (`e^(-2πi…)`), or `inverse` (`e^(+2πi…)`).
    /// The length is a power of two, at most [`MAX`].
    pub fn run(&self, re: &mut [f64], im: &mut [f64], inverse: bool) {
        let n = re.len();
        debug_assert!(n.is_power_of_two() && n <= MAX && im.len() == n);
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
        let sign = if inverse { -1.0 } else { 1.0 };
        let mut len = 2;
        while len <= n {
            let stride = MAX / len;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (wr, wi) = (self.cos[k * stride], sign * self.sin[k * stride]);
                    let (a, b) = (start + k, start + k + len / 2);
                    let (xr, xi) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                    re[b] = re[a] - xr;
                    im[b] = im[a] - xi;
                    re[a] += xr;
                    im[a] += xi;
                }
            }
            len <<= 1;
        }
    }
}
