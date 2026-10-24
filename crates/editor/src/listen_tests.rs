//! The measures against known signals (AGENT-7): loudness as EBU Tech 3341's cases read it, true
//! peak as BS.1770 Annex 2 finds it, octave bands, stereo and masking.

use super::*;

const SR: u32 = 48_000;

/// A sine of `hz` with its peak at `dbfs`, `seconds` long, starting at `phase` (radians).
fn sine(hz: f64, dbfs: f64, seconds: f64, phase: f64) -> Vec<f32> {
    let a = 10f64.powf(dbfs / 20.0);
    let n = (seconds * SR as f64).round() as usize;
    (0..n).map(|i| (a * (std::f64::consts::TAU * hz * i as f64 / SR as f64 + phase).sin()) as f32).collect()
}

/// A part of a test signal: (dBFS, seconds) of a 1 kHz sine, or silence for `None`.
type Part = (Option<f64>, f64);

/// `parts` one after another.
fn tones(parts: &[Part]) -> Vec<f32> {
    let mut x = vec![];
    for &(db, s) in parts {
        match db {
            Some(db) => x.extend(sine(1000.0, db, s, 0.0)),
            None => x.extend(std::iter::repeat_n(0.0, (s * SR as f64).round() as usize)),
        }
    }
    x
}

/// `left` and `right` through a meter, a few thousand frames at a time, as a render hands them.
fn measure(left: &[f32], right: &[f32], wants: Wants) -> Measures {
    let mut m = Meter::new(SR, wants);
    for (l, r) in left.chunks(4096).zip(right.chunks(4096)) {
        m.push(l, r);
    }
    m.measures()
}

/// The same signal on both sides: a stereo signal as EBU Tech 3341's are.
fn both(x: &[f32]) -> Measures {
    measure(x, x, Wants::default())
}

fn near(got: Option<f64>, want: f64, within: f64, what: &str) {
    let got = got.unwrap_or_else(|| panic!("{what}: no reading"));
    assert!((got - want).abs() <= within, "{what}: {got:.3}, not {want} ± {within}");
}

// EBU Tech 3341, case 1: a stereo 1 kHz sine at −23 dBFS for 20 s reads −23.0 ± 0.1 LUFS,
// integrated and short-term.
#[test]
fn a_1_khz_sine_at_minus_23_dbfs_reads_minus_23_lufs() {
    let m = both(&sine(1000.0, -23.0, 20.0, 0.0));
    near(m.lufs, -23.0, 0.1, "integrated");
    near(m.lufs_short_max, -23.0, 0.1, "short-term max");
}

// EBU Tech 3341's other cases for a stereo meter: 2 to 5 (the gates), 9 (short-term over 3 s
// windows) and 10 (the short-term maximum of a 3 s burst, wherever it falls).
#[test]
fn the_other_ebu_tech_3341_cases_read_as_it_says() {
    let cases: [(&str, Vec<Part>, f64); 4] = [
        ("case 2", vec![(Some(-33.0), 20.0)], -33.0),
        ("case 3", vec![(Some(-36.0), 10.0), (Some(-23.0), 60.0), (Some(-36.0), 10.0)], -23.0),
        ("case 4", vec![(Some(-72.0), 10.0), (Some(-36.0), 10.0), (Some(-23.0), 60.0), (Some(-36.0), 10.0), (Some(-72.0), 10.0)], -23.0),
        ("case 5", vec![(Some(-26.0), 20.0), (Some(-20.0), 20.1), (Some(-26.0), 20.0)], -23.0),
    ];
    for (name, parts, want) in cases {
        near(both(&tones(&parts)).lufs, want, 0.1, name);
    }
    // Case 9: 1.34 s at −20 dBFS and 1.66 s at −30, five times: every 3 s window holds −23.0.
    let parts: Vec<Part> = (0..5).flat_map(|_| [(Some(-20.0), 1.34), (Some(-30.0), 1.66)]).collect();
    let m = both(&tones(&parts));
    near(m.lufs_short_max, -23.0, 0.1, "case 9, the short-term maximum");
    // Case 10: a 3 s burst at −23 dBFS between silences, at offsets that fall between any
    // meter's steps: its short-term maximum is −23.0.
    for lead in [0.0, 0.0137, 0.05, 0.111, 0.333, 1.071] {
        let m = both(&tones(&[(None, lead), (Some(-23.0), 3.0), (None, 2.0)]));
        near(m.lufs_short_max, -23.0, 0.1, &format!("case 10, the burst {lead} s in"));
    }
    // Silence has no loudness: under the absolute gate.
    assert_eq!((both(&tones(&[(None, 5.0)])).lufs, both(&tones(&[(Some(-80.0), 5.0)])).lufs), (None, None));
}

/// `x` faded in over its first 20 ms, as a sound starts: a step from silence has peaks of its own
/// between samples.
fn faded(mut x: Vec<f32>) -> Vec<f32> {
    let n = SR as usize / 50;
    for (i, s) in x.iter_mut().take(n).enumerate() {
        *s *= (0.5 - 0.5 * (std::f64::consts::PI * i as f64 / n as f64).cos()) as f32;
    }
    x
}

// BS.1770 Annex 2: a peak between samples is found by 4× oversampling. A 12 kHz sine whose samples
// fall 45° either side of its peaks has every sample at ±1.0 (0 dBFS) and its peaks at √2, +3.0
// dBTP (EBU Tech 3341's case 19, within its +0.2 / −0.4 dB); a half-scale sine reads −6.0 dBTP
// at any frequency and phase (its cases 15 to 18).
#[test]
fn true_peak_finds_the_peak_between_samples() {
    let x = faded(sine(12_000.0, 20.0 * 2f64.sqrt().log10(), 2.0, std::f64::consts::FRAC_PI_4));
    assert!(x[SR as usize / 50..].iter().all(|s| (s.abs() - 1.0).abs() < 1e-6), "every sample at ±1.0");
    let m = both(&x);
    near(m.peak, 0.0, 0.01, "the sample peak");
    let tp = m.true_peak.unwrap();
    assert!((3.01 - 0.4..=3.01 + 0.2).contains(&tp), "the true peak: {tp:.3} dBTP, not 3.0");
    for hz in [576.0, 1_152.0, 1_931.0, 3_882.0, 9_600.0] {
        for phase in [0.0, 0.3, 1.0] {
            let tp = both(&faded(sine(hz, -6.02, 1.0, phase))).true_peak.unwrap();
            assert!((-6.02 - 0.4..=-6.02 + 0.2).contains(&tp), "{hz} Hz at {phase}: {tp:.3} dBTP, not −6.0");
        }
    }
    let over = sine(1000.0, 3.0, 1.0, 0.0);
    assert_eq!(both(&over).clipped, 2 * over.iter().filter(|s| s.abs() > 1.0).count() as u64, "samples over 0 dBFS, each side");
}

/// White noise from a fixed seed, made pink by Paul Kellet's refined filter: equal power in every
/// octave, within a twentieth of a dB.
fn pink(seconds: f64) -> Vec<f32> {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut b = [0.0f64; 7];
    (0..(seconds * SR as f64) as usize)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let w = (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
            b[0] = 0.99886 * b[0] + w * 0.0555179;
            b[1] = 0.99332 * b[1] + w * 0.0750759;
            b[2] = 0.96900 * b[2] + w * 0.1538520;
            b[3] = 0.86650 * b[3] + w * 0.3104856;
            b[4] = 0.55000 * b[4] + w * 0.5329522;
            b[5] = -0.7616 * b[5] - w * 0.0168980;
            let p = b.iter().sum::<f64>() + w * 0.5362;
            b[6] = w * 0.115926;
            (p * 0.05) as f32
        })
        .collect()
}

// Pink noise has equal power in every octave, so its ten octave bands read alike: each within
// 1 dB of their mean.
#[test]
fn pink_noise_has_flat_octave_bands() {
    let x = pink(20.0);
    let m = measure(&x, &x, Wants { bands: true, ..Wants::default() });
    let bands = m.bands.expect("bands").map(|b| b.expect("a level"));
    let mean = bands.iter().sum::<f64>() / bands.len() as f64;
    for (hz, db) in OCTAVES.iter().zip(bands) {
        assert!((db - mean).abs() <= 1.0, "{hz} Hz: {db:.2} dB against {mean:.2}: {bands:?}");
    }
}

/// A track's octave bands, as it reaches the mix: `parts` (Hz, dBFS) of sines, the same on both
/// sides.
fn track(parts: &[(f64, f64)]) -> [Option<f64>; 10] {
    let mut x = vec![0.0f32; 4 * SR as usize];
    for &(hz, db) in parts {
        for (s, v) in x.iter_mut().zip(sine(hz, db, 4.0, 0.0)) {
            *s += v;
        }
    }
    measure(&x, &x, Wants { bands: true, ..Wants::default() }).bands.expect("bands")
}

// Masking: in a band, the tracks as they reach the mix within 6 dB of that band's loudest track
// are named, the loudest first. The bass and the kick share 63 Hz; the hats are alone at 8 kHz, and
// what leaks into a band beside a track's own is never its masking.
#[test]
fn two_tracks_in_one_band_are_named_as_masking() {
    let (bass, kick, hats) = (track(&[(63.0, -12.0)]), track(&[(58.0, -15.0)]), track(&[(8_000.0, -20.0)]));
    let found = masking(&[("hats", hats), ("kick", kick), ("bass", bass)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].hz, &found[0].tracks), (63.0, &vec!["bass".to_string(), "kick".to_string()]), "{found:?}");
    assert!(found[0].db[0] > found[0].db[1] && found[0].db[0] - found[0].db[1] <= 6.0, "{found:?}");
}

// A track turned down in the mix, more than 6 dB under the band's loudest, masks nothing there.
#[test]
fn a_track_turned_down_in_the_mix_masks_nothing() {
    let (bass, kick) = (track(&[(63.0, -12.0)]), track(&[(58.0, -24.0)]));
    assert_eq!(masking(&[("kick", kick), ("bass", bass)]), vec![]);
    // Nor does a band no track is heard in: two near-silent tracks.
    assert_eq!(masking(&[("a", track(&[(1000.0, -80.0)])), ("b", track(&[(1000.0, -81.0)]))]), vec![]);
}

// Stereo: the same signal on both sides correlates 1, with no side and mono lows; one side inverted
// correlates −1, all side, and its lows aren't mono. Silence has no correlation.
#[test]
fn mono_correlates_1_and_inverted_minus_1() {
    let mut x = sine(80.0, -12.0, 3.0, 0.0);
    for (s, v) in x.iter_mut().zip(sine(1000.0, -18.0, 3.0, 0.0)) {
        *s += v;
    }
    let inverted: Vec<f32> = x.iter().map(|s| -s).collect();
    let wants = Wants { stereo: true, ..Wants::default() };
    let mono = measure(&x, &x, wants).stereo.expect("stereo");
    near(mono.correlation, 1.0, 1e-6, "mono's correlation");
    assert!(mono.width.unwrap() <= -60.0 && mono.lows_mono, "mono: {mono:?}");
    let wide = measure(&x, &inverted, wants).stereo.expect("stereo");
    near(wide.correlation, -1.0, 1e-6, "inverted's correlation");
    assert!(wide.width.unwrap() >= 60.0 && !wide.lows_mono, "inverted: {wide:?}");
    let quiet = vec![0.0f32; SR as usize];
    let silent = measure(&quiet, &quiet, wants);
    assert_eq!((silent.stereo.unwrap().correlation, silent.silent), (None, true));
}

// BS.1770's K-weighting at 48 kHz is the standard's table: the pre-filter's and the high-pass's
// coefficients, made here from their analogue shapes at any rate.
#[test]
fn k_weighting_at_48_khz_is_bs_1770s_table() {
    let [pre, rlb] = k_weighting(48_000.0);
    let table = [
        ([1.53512485958697, -2.69169618940638, 1.19839281085285], [-1.69065929318241, 0.73248077421585]),
        ([1.0, -2.0, 1.0], [-1.99004745483398, 0.99007225036621]),
    ];
    for (f, (b, a)) in [pre, rlb].iter().zip(table) {
        for (got, want) in [f.b0, f.b1, f.b2, f.a1, f.a2].into_iter().zip(b.into_iter().chain(a)) {
            assert!((got - want).abs() < 1e-8, "{got} against the table's {want}");
        }
    }
}
