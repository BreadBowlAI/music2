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

/// Each band's power, linear, summed: what all the bands say of a sound together.
fn total(bands: &[f64]) -> f64 {
    10.0 * bands.iter().map(|b| 10f64.powf(b / 10.0)).sum::<f64>().log10()
}

// TAP-13 (review T3): a full-scale sine at every band's centre reads 0 dB in its band: within
// 0.5 dB from 80 Hz up, within 1 dB below it.
#[test]
fn a_sine_at_every_band_centre_reads_its_level() {
    for (k, c) in centres(48_000).into_iter().enumerate() {
        let b = bands(&sine(c, 1.0, LOW_FRAMES), 1, 48_000);
        let near = if c >= 80.0 { 0.5 } else { 1.0 };
        assert!(b[k].abs() <= near, "a sine at {c:.1} Hz reads {} dB in its band {k}", b[k]);
    }
}

// TAP-13 (review T3): across the join of the two FFTs, a sine loses no power: the bands' sum
// stays within 0.5 dB of 0 from 560 Hz to 1,120 Hz, between band centres too.
#[test]
fn a_sweep_across_the_join_loses_no_power() {
    let mut hz = 560.0;
    while hz <= 1120.0 {
        let sum = total(&bands(&sine(hz, 1.0, LOW_FRAMES), 1, 48_000));
        assert!(sum.abs() <= 0.5, "a sine at {hz} Hz sums to {sum} dB");
        hz += 3.0;
    }
}

// TAP-13 (review T3): white noise climbs 3 dB an octave in bands of a twelfth of an octave (a
// quarter of a dB a band), with no step where the two FFTs join.
#[test]
fn white_noise_has_no_step_at_the_join() {
    let mut seed = 1u64;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    };
    let runs = 96;
    let mut mean = vec![0.0; 120];
    for _ in 0..runs {
        let x: Vec<[f32; 2]> = (0..LOW_FRAMES).map(|_| noise()).map(|v| [v, v]).collect();
        for (m, b) in mean.iter_mut().zip(bands(&x, 1, 48_000)) {
            *m += 10f64.powf(b / 10.0) / runs as f64;
        }
    }
    let db: Vec<f64> = mean.iter().map(|p| 10.0 * p.log10()).collect();
    // The straight line nearest bands 44 to 90 (200 Hz to 3.6 kHz): it climbs 0.25 dB a band, and no
    // band is a dB off it. (Below about 150 Hz a band is narrower than three bins and sums the three
    // nearest, so noise reads loud there.)
    let ks: Vec<f64> = (44..=90).map(|k| k as f64).collect();
    let n = ks.len() as f64;
    let (mk, md) = (ks.iter().sum::<f64>() / n, (44..=90).map(|k| db[k]).sum::<f64>() / n);
    let slope = ks.iter().map(|&k| (k - mk) * (db[k as usize] - md)).sum::<f64>() / ks.iter().map(|&k| (k - mk).powi(2)).sum::<f64>();
    assert!((slope - 0.25).abs() < 0.03, "3 dB an octave: {slope} a band");
    for (k, (d, c)) in db.iter().zip(centres(48_000)).enumerate().take(91).skip(44) {
        let off = d - (md + slope * (k as f64 - mk));
        assert!(off.abs() < 1.0, "band {k} ({c:.0} Hz) is {off:.2} dB off the line");
    }
}

// TAP-13 (review T1, T3): a look works out its lows at every fourth read, and keeps them while a
// new plan's ring has fewer frames than they need; its highs each read.
#[test]
fn a_look_keeps_its_lows_between_their_reads() {
    let mut look = Spectrum::default();
    let (low, high) = (sine(80.0, 1.0, LOW_FRAMES), sine(centres(48_000)[80], 1.0, LOW_FRAMES));
    let first = look.bands(&low, 1, 48_000);
    assert!(first[24].abs() < 0.5, "the lows, worked out: {}", first[24]);
    for read in 1..LOWS_EVERY {
        let b = look.bands(&high, 1, 48_000);
        assert_eq!(b[24], first[24], "read {read}: the lows it last had");
        assert!(b[80].abs() < 0.6, "read {read}: the highs anew: {}", b[80]);
    }
    let b = look.bands(&high[..2000], 1, 48_000);
    assert_eq!(b[24], first[24], "a ring that holds too few frames for the lows: kept");
    let b = look.bands(&high, 1, 48_000);
    assert!(b[24] < -40.0, "four reads on, the first with frames enough: the lows anew: {}", b[24]);
}

// TAP-13 (review T1): 31 spectra, each working out its lows too, take under half the meters' 33 ms
// read. It judges speed, so it runs in the nightly tier (ci/nightly-tests.txt).
#[test]
fn thirty_one_spectra_fit_in_a_read_with_room() {
    let x = sine(440.0, 0.5, LOW_FRAMES);
    let mut looks: Vec<Spectrum> = (0..31).map(|_| Spectrum::default()).collect();
    looks.iter_mut().for_each(|l| drop(l.bands(&x, 2, 48_000)));
    let best = (0..5)
        .map(|_| {
            let t = std::time::Instant::now();
            for l in &mut looks {
                *l = Spectrum::default();
                std::hint::black_box(l.bands(&x, 2, 48_000));
            }
            t.elapsed()
        })
        .min()
        .unwrap();
    assert!(best < std::time::Duration::from_micros(16_500), "31 spectra took {best:?}");
}
