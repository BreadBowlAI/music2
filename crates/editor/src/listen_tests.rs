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

/// `parts` (Hz, dBFS) of sines summed, `seconds` long.
fn sines(parts: &[(f64, f64)], seconds: f64) -> Vec<f32> {
    let mut x = vec![0.0f32; (seconds * SR as f64).round() as usize];
    for &(hz, db) in parts {
        for (s, v) in x.iter_mut().zip(sine(hz, db, seconds, 0.0)) {
            *s += v;
        }
    }
    x
}

/// A signal's moments, the same on both sides: a track as it reaches the mix. The meter is warmed
/// with `warm` first, as a listen warms it with what comes before its range.
fn warmed(warm: &[f32], x: &[f32]) -> Vec<Moment> {
    let mut m = Meter::new(SR, Wants { bands: true, moments: true, ..Wants::default() });
    m.warm(warm, warm);
    for piece in x.chunks(4096) {
        m.push(piece, piece);
    }
    m.moments().expect("moments").to_vec()
}

/// A signal's moments, from silence.
fn moments(x: &[f32]) -> Vec<Moment> {
    warmed(&[], x)
}

/// A track of `parts` (Hz, dBFS) of sines that sound through 4 s, moment by moment.
fn track(parts: &[(f64, f64)]) -> Vec<Moment> {
    let x = sines(parts, 5.0);
    let (warm, range) = x.split_at(SR as usize);
    warmed(warm, range)
}

/// [`masking`] of tracks by key.
fn masked(tracks: &[(&str, &Vec<Moment>)]) -> Vec<Masked> {
    let heard: Vec<(&str, &[Moment])> = tracks.iter().map(|(k, m)| (*k, m.as_slice())).collect();
    masking(&heard)
}

// Masking: in a band, the tracks as they reach the mix within 6 dB of that band's loudest track
// are named, the loudest first. The bass and the kick share 63 Hz; the hats are alone at 8 kHz, and
// what leaks into a band beside a track's own is never its masking.
#[test]
fn two_tracks_in_one_band_are_named_as_masking() {
    let (bass, kick, hats) = (track(&[(63.0, -12.0)]), track(&[(58.0, -15.0)]), track(&[(8_000.0, -20.0)]));
    let found = masked(&[("hats", &hats), ("kick", &kick), ("bass", &bass)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].hz, &found[0].tracks), (63.0, &vec!["bass".to_string(), "kick".to_string()]), "{found:?}");
    assert!(found[0].db[0] > found[0].db[1] && found[0].db[0] - found[0].db[1] <= 6.0, "{found:?}");
    // They sound together all the time.
    assert!(found[0].share > 0.99, "{found:?}");
}

// A track turned down in the mix, more than 6 dB under the band's loudest, masks nothing there.
#[test]
fn a_track_turned_down_in_the_mix_masks_nothing() {
    let (bass, kick) = (track(&[(63.0, -12.0)]), track(&[(58.0, -24.0)]));
    assert_eq!(masked(&[("kick", &kick), ("bass", &bass)]), vec![]);
    // Nor does a band no track is heard in: two near-silent tracks.
    assert_eq!(masked(&[("a", &track(&[(1000.0, -80.0)])), ("b", &track(&[(1000.0, -81.0)]))]), vec![]);
}

// The review of ag-listen, F3: a loud track's leak into the band beside its own never decides that
// band's loudest. A kick at 60 Hz is loud at 63 Hz and leaks into 125 Hz; a bass at 125 Hz and keys
// at 135 Hz, level with each other there, mask each other.
#[test]
fn a_loud_tracks_leak_never_hides_a_clash_beside_it() {
    let (kick, bass, keys) = (track(&[(60.0, -3.0)]), track(&[(125.0, -25.0)]), track(&[(135.0, -25.5)]));
    let found = masked(&[("kick", &kick), ("bass", &bass), ("keys", &keys)]);
    let at = |hz: f64| found.iter().filter(|m| m.hz == hz).map(|m| m.tracks.clone()).collect::<Vec<_>>();
    assert_eq!(at(125.0), vec![vec!["bass".to_string(), "keys".to_string()]], "{found:?}");
    assert!(found.iter().all(|m| !m.tracks.contains(&"kick".to_string())), "the kick is alone at 63 Hz: {found:?}");
}

// The review of ag-listen, F4: two tracks that take turns, a second each, never sound at once, so
// they never mask each other, though their bands over the whole range are level. Where they do
// sound together they mask each other for as long as they do: here half the range.
#[test]
fn tracks_that_take_turns_never_mask_each_other() {
    let tone = sines(&[(63.0, -12.0), (126.0, -18.0)], 1.0);
    let quiet = vec![0.0f32; tone.len()];
    let turns =
        |first: bool| -> Vec<f32> { (0..8).flat_map(|i| if (i % 2 == 0) == first { tone.clone() } else { quiet.clone() }).collect() };
    let (a, b) = (moments(&turns(true)), moments(&turns(false)));
    assert_eq!(masked(&[("a", &a), ("b", &b)]), vec![], "they take turns");
    // b also sounds in a's first two turns: they meet a quarter of the range.
    let mut met = turns(false);
    met[..tone.len()].copy_from_slice(&tone);
    met[2 * tone.len()..3 * tone.len()].copy_from_slice(&tone);
    let found = masked(&[("a", &a), ("b", &moments(&met))]);
    let low = found.iter().find(|m| m.hz == 63.0).unwrap_or_else(|| panic!("at 63 Hz: {found:?}"));
    assert!((low.share - 0.25).abs() < 0.03, "a quarter of the range: {found:?}");
}

// The review of ag-listen, F4: a bass ducked under each kick hit, the musician's fix for a kick and
// a bass in one band, masks it only where they meet: not while the kick hits and the bass is
// ducked, nor once the kick has died away. Over the whole range their 63 Hz bands are level (the
// kick -17 dBFS, the bass -19), and in 400 ms they would meet in every beat.
#[test]
fn a_bass_ducked_under_each_kick_masks_it_only_where_they_meet() {
    let (beat, n) = (SR as usize / 2, 16);
    let mut kick = vec![0.0f32; beat * n];
    let mut bass = vec![0.0f32; beat * n];
    let (k, b) = (sine(60.0, 0.0, 0.5, 0.0), sine(55.0, -15.0, 8.0, 0.0));
    for i in 0..beat * n {
        let t = (i % beat) as f64 / SR as f64;
        // The kick dies away over its beat; the bass is ducked 15 dB while the kick is loud.
        kick[i] = k[i % beat] * (-t / 0.04).exp() as f32;
        let duck = if t < 0.12 { 10f64.powf(-15.0 / 20.0) } else { 1.0 };
        bass[i] = b[i] * duck as f32;
    }
    let (kick, bass) = (moments(&kick), moments(&bass));
    let level = |m: &[Moment]| 10.0 * (m.iter().map(|x| x.bands[1] as f64).sum::<f64>() / m.len() as f64).log10();
    assert!((level(&kick) - level(&bass)).abs() < 6.0, "level over the range: {} and {}", level(&kick), level(&bass));
    let found = masked(&[("kick", &kick), ("bass", &bass)]);
    let low = found.iter().find(|m| m.hz == 63.0).map_or(0.0, |m| m.share);
    assert!(low < 0.3, "they meet only as the duck lets go: {found:?}");
}

// The review of ag-listen, F10: what a track has in a band is its masking there, however far under
// its loudest band: a dark bass's low-mid harmonics mask a pad level with them at 250 Hz.
#[test]
fn a_dark_bass_and_a_pad_level_in_the_low_mids_mask_each_other() {
    let n = 4 * SR as usize;
    let tone: Vec<f64> = (0..n)
        .map(|i| (1..=40).map(|k| (std::f64::consts::TAU * 55.0 * k as f64 * i as f64 / SR as f64).sin() / (k * k) as f64).sum())
        .collect();
    let top = tone.iter().fold(0.0f64, |m, s| m.max(s.abs()));
    let bass: Vec<f32> = tone.iter().map(|s| (s / top * 0.5) as f32).collect();
    let bass = moments(&bass);
    // The bass's level at 250 Hz, and a pad's sine there as loud.
    let at250 = 10.0 * (bass.iter().map(|m| m.bands[3] as f64).sum::<f64>() / bass.len() as f64).log10();
    let pad = track(&[(250.0, at250 + 3.01)]);
    let found = masked(&[("bass", &bass), ("pad", &pad)]);
    assert!(found.iter().any(|m| m.hz == 250.0), "bass and pad at 250 Hz: {found:?}");
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

// The review of ag-listen, F1: a range that starts while a sound is loud reads it as it is, not as
// a step from silence. A meter warmed with the second before the range reads a sine at -1 dBFS,
// read from its peak on, at -1.0 dBTP, as the whole signal reads; cold, its first samples are a
// step whose peaks between samples aren't the sine's.
#[test]
fn a_meter_warmed_before_the_range_reads_no_step_from_silence() {
    for (hz, dbfs) in [(1_000.0, -1.0), (60.0, -1.0), (1_000.0, -0.3)] {
        let x = sine(hz, dbfs, 3.0, std::f64::consts::FRAC_PI_2);
        let (before, range) = x.split_at(SR as usize);
        let mut m = Meter::new(SR, Wants { bands: true, stereo: true, moments: true, ..Wants::default() });
        m.warm(before, before);
        for piece in range.chunks(4096) {
            m.push(piece, piece);
        }
        let warm = m.measures();
        let whole = both(&faded(x.clone()));
        near(warm.true_peak, dbfs, 0.02, &format!("{hz} Hz at {dbfs} dBFS, its true peak"));
        near(warm.lufs, whole.lufs.unwrap(), 0.05, &format!("{hz} Hz, its loudness"));
        // Its band, as a steady sine has it all along: its mean square, 3 dB under its peak.
        let band = warm.bands.unwrap()[if hz < 100.0 { 1 } else { 5 }];
        near(band, dbfs - 3.01, 0.05, &format!("{hz} Hz, its band"));
        // Nothing of the warming is counted: the moments are the range's.
        assert_eq!(m.moments().unwrap().len(), (2.0 / MOMENT).round() as usize, "the range's moments");
    }
}

// The review of ag-listen, F14: a sample that isn't a number (NaN, infinity) is counted, each side,
// and read as 0, as the render's output reads one: everything else measures what a 0 there gives,
// and one bad sample in 5 s changes the loudness by nothing to speak of.
#[test]
fn a_sample_that_isnt_a_number_is_counted_and_read_as_zero() {
    let clean = sine(1000.0, -6.0, 5.0, 0.0);
    let wants = Wants { bands: true, stereo: true, picture: false, moments: true };
    let mut zeroed = clean.clone();
    zeroed[1000] = 0.0;
    let want = measure(&zeroed, &zeroed, wants);
    near(want.lufs, measure(&clean, &clean, wants).lufs.unwrap(), 0.01, "one 0 in 5 s");
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut x = clean.clone();
        x[1000] = bad;
        let m = measure(&x, &x, wants);
        assert_eq!(m.non_finite, 2, "{bad}: each side");
        assert_eq!(Measures { non_finite: 0, ..m }, want, "{bad}: read as 0");
    }
    assert_eq!(want.non_finite, 0);
}
