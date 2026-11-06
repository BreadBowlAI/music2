//! A look's spectrum (TAP-13): bands a twelfth of an octave wide, measured finely below 500 Hz.

use super::*;

/// `frames` of a sine at `hz` and amplitude `amp`, at 48 kHz, on both sides.
fn sine(hz: f64, amp: f64, frames: usize) -> Vec<[f32; 2]> {
    (0..frames).map(|t| (amp * (std::f64::consts::TAU * hz * t as f64 / 48_000.0).sin()) as f32).map(|x| [x, x]).collect()
}

fn loudest(bands: &[f64]) -> usize {
    (0..bands.len()).max_by(|&a, &b| bands[a].total_cmp(&bands[b])).unwrap()
}

// TAP-13: band k is centred on 20 × 2^(k/12) Hz, up to the last below 20 kHz and below half the rate.
#[test]
fn bands_go_from_20_hz_to_under_20_khz_and_half_the_rate() {
    let c = centres(48_000);
    assert_eq!(c.len(), 120);
    assert!((c[0] - 20.0).abs() < 1e-12 && (c[12] - 40.0).abs() < 1e-9 && (c[119] - 20.0 * 2f64.powf(119.0 / 12.0)).abs() < 1e-6);
    assert_eq!(centres(44_100).len(), 120);
    assert_eq!(centres(32_000).len(), 116, "under 16 kHz at 32 kHz");
}

// TAP-13: a sine reads its level in its band, a sine at full scale 0 dB, and almost nothing far from
// it; silence reads −120 dB everywhere.
#[test]
fn a_sine_reads_its_level_in_its_band() {
    let b = bands(&sine(1000.0, 0.5, 16_256), 2, 48_000);
    assert_eq!(b.len(), 120);
    let at = loudest(&b);
    assert!((centres(48_000)[at] / 1000.0).log2().abs() < 1.0 / 24.0, "band {at}");
    assert!((b[at] + 6.0).abs() < 0.5, "half of full scale reads −6 dB: {}", b[at]);
    assert!(b[at - 12] < -60.0 && b[at + 12] < -60.0, "an octave away, almost nothing: {} {}", b[at - 12], b[at + 12]);
    assert!(b.iter().all(|x| (x * 10.0).round() == x * 10.0), "rounded to 0.1 dB");
    assert!(bands(&vec![[0.0; 2]; 16_256], 2, 48_000).iter().all(|&x| x == -120.0), "silence");
    let full = bands(&sine(4000.0, 1.0, 16_256), 1, 48_000);
    assert!(full[loudest(&full)].abs() < 0.5, "a sine at full scale reads 0 dB: {}", full[loudest(&full)]);
}

// TAP-13: below 500 Hz the bins are 2.9 Hz apart, so an 80 Hz sine and one a fifth above stand
// apart, each loudest in its own band.
#[test]
fn the_lows_are_measured_finely() {
    let (a, b) = (sine(80.0, 0.5, 16_256), sine(120.0, 0.5, 16_256));
    let both: Vec<[f32; 2]> = a.iter().zip(&b).map(|(x, y)| [x[0] + y[0], x[1] + y[1]]).collect();
    let s = bands(&both, 1, 48_000);
    let at80 = loudest(&s[..30]);
    assert_eq!(at80, 24, "80 Hz is band 24");
    assert!((s[24] + 6.0).abs() < 1.5, "at its level: {}", s[24]);
    let between = s[27];
    assert!(between < s[24] - 20.0, "between them, a dip: {between} against {}", s[24]);
    assert!((s[31] + 6.0).abs() < 2.0, "120 Hz, near band 31, at its level: {}", s[31]);
}
