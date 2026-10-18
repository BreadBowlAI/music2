//! What the instrument kernels' native tests share: a host that plays a kernel in blocks
//! of any size, the manifest's ports, and a spectrum for measuring aliasing.

use m2_kernel::{Host, Kernel, PORT_CHANNELS};

pub(crate) const SR: f32 = 48_000.0;
pub(crate) const BLOCK: usize = 128;

/// `input(port, channel, t)`: every input sample a kernel sees.
pub(crate) type Input<'a> = &'a dyn Fn(usize, usize, usize) -> f32;

/// What a kernel played: every output port's samples (both channels), and after each
/// sample whether the kernel said it was still active at the end of that call.
pub(crate) struct Played {
    pub(crate) out: Vec<Vec<[f32; 2]>>,
    pub(crate) active: Vec<bool>,
}

impl Played {
    /// Output port 0.
    pub(crate) fn main(&self) -> &[[f32; 2]] {
        &self.out[0]
    }

    /// Output port 0, left channel.
    pub(crate) fn left(&self) -> Vec<f32> {
        self.out[0].iter().map(|f| f[0]).collect()
    }
}

/// Kernel `K` over `frames` samples, processed `split` frames at a time, at `sr`.
pub(crate) fn play_at<K: Kernel>(sr: f32, channels: usize, frames: usize, split: usize, input: Input<'_>) -> Played {
    play_cfg::<K>(m2_kernel::Cfg::new(sr, BLOCK, channels), frames, split, input)
}

/// Kernel `K` built from `cfg` (its values and assets included), as [`play_at`] plays it.
pub(crate) fn play_cfg<K: Kernel>(cfg: m2_kernel::Cfg, frames: usize, split: usize, input: Input<'_>) -> Played {
    let mut host = Host::<K>::of(cfg);
    let mut out = vec![Vec::with_capacity(frames); K::OUTPUTS];
    let mut active = Vec::with_capacity(frames);
    let mut t = 0;
    while t < frames {
        let n = split.clamp(1, BLOCK).min(frames - t);
        for p in 0..K::INPUTS {
            let buf = host.port_mut(p);
            for c in 0..PORT_CHANNELS {
                for k in 0..n {
                    buf[c * BLOCK + k] = input(p, c, t + k);
                }
            }
        }
        host.process(n as u32);
        for (o, port) in out.iter_mut().enumerate() {
            let b = host.port_mut(K::INPUTS + o);
            port.extend((0..n).map(|k| [b[k], b[BLOCK + k]]));
        }
        let a = host.active();
        active.extend((0..n).map(|_| a));
        t += n;
    }
    Played { out, active }
}

pub(crate) fn play<K: Kernel>(frames: usize, split: usize, input: Input<'_>) -> Played {
    play_at::<K>(SR, 2, frames, split, input)
}

/// Plays `K` for `frames` with constant `inputs`, resets it, and plays again: the same output
/// both times means a reset (a reused voice) starts exactly where a new kernel does.
pub(crate) fn assert_reset_starts_over<K: Kernel>(inputs: &[f32], frames: usize) {
    let mut host = Host::<K>::new(SR, BLOCK as u32, 2);
    let take = |host: &mut Host<K>| -> Vec<f32> {
        let mut got = vec![];
        for _ in 0..frames.div_ceil(BLOCK) {
            for (port, &v) in inputs.iter().enumerate() {
                host.port_mut(port).fill(v);
            }
            host.process(BLOCK as u32);
            for o in 0..K::OUTPUTS {
                got.extend_from_slice(host.port_mut(K::INPUTS + o));
            }
        }
        got
    };
    let first = take(&mut host);
    host.reset();
    assert!(take(&mut host) == first, "a reset doesn't start over");
    assert!(first.iter().all(|v| v.is_finite()));
}

/// Seconds to samples at 48 kHz.
pub(crate) fn s(seconds: f32) -> usize {
    (seconds * SR) as usize
}

/// A kernel's ports from `std/m2pkg.toml`: `(name, default)` for its inputs, then its
/// outputs' names. The tests play every kernel with the manifest's own defaults, so a port
/// order or default that drifts from the code fails here.
pub(crate) fn manifest(kernel: &str) -> (Vec<(String, Option<f32>)>, Vec<String>) {
    let toml = include_str!("../../m2pkg.toml");
    let at = toml.find(&format!("name = \"{kernel}\"")).unwrap_or_else(|| panic!("no kernel {kernel} in m2pkg.toml"));
    let rest = &toml[at..];
    let list = |key: &str| -> Vec<String> {
        let from = rest.find(&format!("{key} = [")).expect(key) + key.len() + 4;
        let body = &rest[from..from + rest[from..].find(']').expect("]")];
        body.split('"').skip(1).step_by(2).map(str::to_string).collect()
    };
    let inputs = list("inputs")
        .iter()
        .map(|port| {
            let (name, kind) = port.split_once(':').expect(port);
            let default = kind.split_once('=').map(|(_, d)| d.split(" in ").next().unwrap().trim().parse::<f32>().expect(port));
            (name.trim().to_string(), default)
        })
        .collect();
    let outputs = list("outputs").iter().map(|port| port.split(':').next().unwrap().trim().to_string()).collect();
    (inputs, outputs)
}

/// The manifest's defaults for `kernel`, by port, with `names` checked against the
/// manifest's order (an input without a default reads 0).
pub(crate) fn defaults(kernel: &str, names: &[&str]) -> Vec<f32> {
    let (inputs, _) = manifest(kernel);
    let got: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(got, names, "{kernel}'s ports in m2pkg.toml");
    inputs.iter().map(|(_, d)| d.unwrap_or(0.0)).collect()
}

pub(crate) fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

pub(crate) fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len().max(1) as f64).sqrt() as f32
}

pub(crate) fn mean(x: &[f32]) -> f64 {
    x.iter().map(|&v| v as f64).sum::<f64>() / x.len().max(1) as f64
}

/// DC: the mean under a Hann window, so the part-cycles at the window's ends don't count.
pub(crate) fn dc(x: &[f32]) -> f64 {
    let n = x.len() as f64;
    let w = |i: usize| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n).cos();
    x.iter().enumerate().map(|(i, &v)| w(i) * v as f64).sum::<f64>() / (0..x.len()).map(w).sum::<f64>()
}

/// Power above 8 kHz in the 5 ms from `at`, against the 20 ms before, in dB (per sample):
/// how much a click adds to the top end. The top end before counts as at least -60 dB
/// of the whole sound, so a pure tone's clean top doesn't make any change look huge.
pub(crate) fn high_rise(x: &[f32], at: usize) -> f32 {
    // A 4th-order difference: a highpass that keeps little below 8 kHz.
    let hp = |i: usize| x[i] - 4.0 * x[i - 1] + 6.0 * x[i - 2] - 4.0 * x[i - 3] + x[i - 4];
    let power =
        |f: &dyn Fn(usize) -> f32, from: usize, to: usize| (from..to).map(|i| (f(i) as f64).powi(2)).sum::<f64>() / (to - from) as f64;
    let floor = 1e-6 * power(&|i| x[i], at - s(0.02), at);
    let (before, after) = (power(&hp, at - s(0.02), at).max(floor), power(&hp, at, at + s(0.005)));
    10.0 * (after.max(1e-30) / before.max(1e-30)).log10() as f32
}

pub(crate) fn db(x: f32) -> f32 {
    20.0 * x.max(1e-30).log10()
}

/// Index of the last sample that isn't exactly 0.
pub(crate) fn last_sound(x: &[f32]) -> Option<usize> {
    x.iter().rposition(|&v| v != 0.0)
}

// ---------------------------------------------------------------- spectra

/// In-place radix-2 FFT of `(re, im)`; the length must be a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
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
        let a = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((a * k as f64).cos(), (a * k as f64).sin());
                let (i, j) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[j] * wr - im[j] * wi, re[j] * wi + im[j] * wr);
                re[j] = re[i] - xr;
                im[j] = im[i] - xi;
                re[i] += xr;
                im[i] += xi;
            }
        }
        len <<= 1;
    }
}

/// Modified Bessel function I0, for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..200 {
        term *= (x / (2.0 * k as f64)).powi(2);
        sum += term;
        if term < 1e-17 * sum {
            break;
        }
    }
    sum
}

/// Power spectrum of `x` (its first power-of-two samples) under a Kaiser window (β 20,
/// sidelobes far below -150 dB), with each bin's frequency.
pub(crate) fn spectrum(x: &[f32], sr: f32) -> Vec<(f64, f64)> {
    let n = 1usize << (usize::BITS - 1 - x.len().leading_zeros());
    let beta = 20.0;
    let i0b = bessel_i0(beta);
    let mut re: Vec<f64> = (0..n)
        .map(|i| {
            let r = 2.0 * i as f64 / (n - 1) as f64 - 1.0;
            x[i] as f64 * bessel_i0(beta * (1.0 - r * r).sqrt()) / i0b
        })
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    (0..n / 2).map(|k| (k as f64 * sr as f64 / n as f64, re[k] * re[k] + im[k] * im[k])).collect()
}

/// A-weighting as a power gain (0 dB near 1 kHz).
fn a_weight(f: f64) -> f64 {
    let f2 = f * f;
    let ra = 12194f64.powi(2) * f2 * f2
        / ((f2 + 20.6f64.powi(2)) * ((f2 + 107.7f64.powi(2)) * (f2 + 737.9f64.powi(2))).sqrt() * (f2 + 12194f64.powi(2)));
    (ra * 1.2589).powi(2)
}

/// Aliasing of a periodic sound at `f0`: A-weighted power between 20 Hz and 16 kHz away
/// from the harmonics, in dB relative to the harmonics' power there. Bins within 10 of a
/// harmonic count as that harmonic (the Kaiser window's main lobe).
pub(crate) fn alias_db(x: &[f32], f0: f32, sr: f32) -> f64 {
    let spec = spectrum(x, sr);
    let bin = spec[1].0;
    let (mut harm, mut alias) = (0.0, 0.0);
    for &(f, p) in &spec {
        if !(20.0..=16_000.0).contains(&f) {
            continue;
        }
        let n = (f / f0 as f64).round();
        let w = p * a_weight(f);
        if n >= 1.0 && (f - n * f0 as f64).abs() <= 10.0 * bin {
            harm += w;
        } else {
            alias += w;
        }
    }
    10.0 * (alias.max(1e-300) / harm).log10()
}

/// Spectral centroid in Hz.
pub(crate) fn centroid(x: &[f32], sr: f32) -> f64 {
    let spec = spectrum(x, sr);
    let total: f64 = spec.iter().map(|s| s.1).sum();
    spec.iter().map(|s| s.0 * s.1).sum::<f64>() / total
}

/// The strongest frequency between `lo` and `hi` Hz, refined between bins.
pub(crate) fn peak_hz(x: &[f32], sr: f32, lo: f64, hi: f64) -> f64 {
    let spec = spectrum(x, sr);
    let (k, _) =
        spec.iter().enumerate().filter(|(_, s)| s.0 >= lo && s.0 <= hi).fold((0, 0.0), |m, (k, s)| if s.1 > m.1 { (k, s.1) } else { m });
    let (a, b, c) = (spec[k - 1].1.ln(), spec[k].1.ln(), spec[k + 1].1.ln());
    let shift = 0.5 * (a - c) / (a - 2.0 * b + c);
    (k as f64 + shift) * spec[1].0
}

/// Values no control should break on.
pub(crate) const EXTREMES: [f32; 11] = [f32::NAN, f32::NEG_INFINITY, f32::MIN, -1e9, -1.0, 0.0, 1e-9, 0.5, 1e9, f32::MAX, f32::INFINITY];

/// Deterministic pseudo-random picks.
pub(crate) struct Lcg(pub(crate) u32);

impl Lcg {
    pub(crate) fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 16
    }
}

/// Control settings to torture a kernel with: all controls at each extreme together, each
/// alone at each extreme with the rest as given, and pseudo-random mixes of extremes. Ports
/// in `keep` (pitch, gate, signals) stay as given.
pub(crate) fn torture(base: &[f32], keep: &[usize]) -> Vec<Vec<f32>> {
    let free: Vec<usize> = (0..base.len()).filter(|p| !keep.contains(p)).collect();
    let mut sets = vec![];
    for &v in &EXTREMES {
        let mut s = base.to_vec();
        for &p in &free {
            s[p] = v;
        }
        sets.push(s);
    }
    for &p in &free {
        for &v in &EXTREMES {
            let mut s = base.to_vec();
            s[p] = v;
            sets.push(s);
        }
    }
    let mut rng = Lcg(1);
    for _ in 0..40 {
        let mut s = base.to_vec();
        for &p in &free {
            s[p] = EXTREMES[rng.next() as usize % EXTREMES.len()];
        }
        sets.push(s);
    }
    sets
}

/// The biggest sample-to-sample step in `x[range]`.
pub(crate) fn max_step(x: &[f32], from: usize, to: usize) -> f32 {
    x[from.max(1)..to.min(x.len())].iter().zip(&x[from.max(1) - 1..]).fold(0.0, |m, (a, b)| m.max((a - b).abs()))
}
