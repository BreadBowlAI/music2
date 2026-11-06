//! A radix-2 FFT of one size, its turns made at birth: `pitch` runs it while it plays, so it takes
//! no memory then.

use std::f64::consts::PI;

pub struct Fft {
    n: usize,
    cos: Vec<f64>,
    sin: Vec<f64>,
}

impl Fft {
    /// A transform of `n` points, a power of two.
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && n >= 2, "an FFT's length is a power of two");
        let (cos, sin) = (0..n / 2).map(|k| 2.0 * PI * k as f64 / n as f64).map(|a| (a.cos(), -a.sin())).unzip();
        Fft { n, cos, sin }
    }

    pub fn size(&self) -> usize {
        self.n
    }

    /// `(re, im)` transformed in place, unscaled: forward (`e^(-2πi…)`), or `inverse` (`e^(+2πi…)`).
    pub fn run(&self, re: &mut [f64], im: &mut [f64], inverse: bool) {
        let n = self.n;
        debug_assert!(re.len() == n && im.len() == n);
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
            let stride = n / len;
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
