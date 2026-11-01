//! Native tests of the wavetable voice: its pitch, its position, its band-limiting, its unison, a
//! file as a table, the tables its voices share, then what every voice guarantees (no allocation
//! while it plays, the same bits however its blocks are split, a reset that starts over, nothing
//! broken by any control).

use m2_dsp::testing::*;
use m2_dsp::voice::hz;
use m2_kernel::{Asset, Cfg, Host, native};

use super::*;

const MANIFEST: &str = include_str!("../../m2pkg.toml");

const NAMES: [&str; INPUTS] = [
    "pitch",
    "gate",
    "velocity",
    "a_position",
    "a_pitch",
    "a_level",
    "b_position",
    "b_pitch",
    "b_level",
    "detune",
    "spread",
    "filter",
    "cutoff",
    "q",
    "keytrack",
    "env",
    "f_attack",
    "f_decay",
    "f_sustain",
    "f_release",
    "attack",
    "decay",
    "sustain",
    "release",
    "lfo_rate",
    "lfo_position",
    "lfo_cutoff",
    "vel_amount",
    "level",
];

// The basic table's frames, by position.
const SINE: f32 = 0.0;
const TRIANGLE: f32 = 1.0 / 3.0;
const SAW: f32 = 2.0 / 3.0;
const SQUARE: f32 = 1.0;

/// The manifest's defaults, with `changes` applied.
fn patch(changes: &[(usize, f32)]) -> Vec<f32> {
    let mut p = defaults_in(MANIFEST, "voice", &NAMES);
    for &(port, v) in changes {
        p[port] = v;
    }
    p
}

/// The settings a song's code gives the kernel: its values and its files.
fn cfg(sr: f32, values: &[f64], assets: &[Asset]) -> Cfg {
    Cfg { values: Box::leak(values.into()), assets: Box::leak(assets.into()), ..Cfg::new(sr, BLOCK, 2) }
}

/// One note held from the first sample, `frames` long, played `split` frames at a time.
fn note(cfg: Cfg, patch: &[f32], pitch: f32, frames: usize, split: usize) -> Played {
    let p = patch.to_vec();
    play_cfg::<Wavetable>(cfg, frames, split, &move |port, _, _| match port {
        PITCH => pitch,
        GATE => 1.0,
        _ => p[port],
    })
}

/// A held note's left channel from `from` on, `n` samples of it.
fn held(cfg: Cfg, patch: &[f32], pitch: f32, from: usize, n: usize) -> Vec<f32> {
    note(cfg, patch, pitch, from + n, BLOCK).left()[from..].to_vec()
}

/// The frequency of a sine, from the times it crosses zero going up (between samples, in a straight
/// line), over all of `x`.
fn measured_hz(x: &[f32], sr: f32) -> f64 {
    let ups: Vec<f64> = (1..x.len())
        .filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0)
        .map(|i| (i - 1) as f64 + x[i - 1] as f64 / (x[i - 1] - x[i]) as f64)
        .collect();
    assert!(ups.len() > 10, "too few cycles to measure");
    sr as f64 * (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0])
}

fn cents(f: f64, want: f64) -> f64 {
    1200.0 * (f / want).log2()
}

/// The power of a partial at `f`, in dB: all in the window's main lobe about it (8 bins each way),
/// which is the same wherever it falls between bins.
fn power_at(spec: &[(f64, f64)], f: f64) -> f64 {
    let bin = spec[1].0;
    let p: f64 = spec.iter().filter(|s| (s.0 - f).abs() <= 8.0 * bin).map(|s| s.1).sum();
    10.0 * p.max(1e-300).log10()
}

/// The harmonics `hs` of a note at `f0`, each in dB against the first.
fn harmonics(x: &[f32], sr: f32, f0: f64, hs: &[usize]) -> Vec<f64> {
    let spec = spectrum(x, sr);
    let one = power_at(&spec, f0);
    hs.iter().map(|&h| power_at(&spec, h as f64 * f0) - one).collect()
}

/// Frames of 2048 samples, each one cycle of a sine at the harmonic given for it.
fn sines(harmonics: &[usize]) -> Vec<f32> {
    harmonics.iter().flat_map(|&h| (0..2048).map(move |n| (2.0 * std::f64::consts::PI * (h * n) as f64 / 2048.0).sin() as f32)).collect()
}

// ---------------------------------------------------------------- pitch

// PACK-27: a note plays at its pitch, measured, at every rate and across the keyboard, and each
// oscillator's pitch moves it by exactly its semitones.
#[test]
fn a_note_plays_at_its_pitch() {
    for (pitch, sr) in [(33.0, 48_000.0), (45.0, 44_100.0), (69.0, 48_000.0), (69.0, 44_100.0), (93.0, 48_000.0), (105.0, 96_000.0)] {
        let x = held(cfg(sr, &[], &[]), &patch(&[(A_POSITION, SINE)]), pitch, (0.1 * sr) as usize, (2.0 * sr) as usize);
        let got = measured_hz(&x, sr);
        let c = cents(got, hz(pitch) as f64);
        assert!(c.abs() < 0.05, "note {pitch} at {sr} Hz: {got:.4} Hz, {c:+.4} cents off");
    }
    let fifth = held(cfg(SR, &[], &[]), &patch(&[(A_PITCH, 7.0)]), 57.0, s(0.1), s(2.0));
    let c = cents(measured_hz(&fifth, SR), hz(64.0) as f64);
    assert!(c.abs() < 0.05, "a's pitch 7: {c:+.4} cents off");
    // Oscillator B alone, an octave down.
    let b = held(cfg(SR, &[], &[]), &patch(&[(A_LEVEL, 0.0), (B_LEVEL, 1.0), (B_PITCH, -12.0)]), 69.0, s(0.1), s(2.0));
    let c = cents(measured_hz(&b, SR), 220.0);
    assert!(c.abs() < 0.05, "b an octave down: {c:+.4} cents off");
}

// ---------------------------------------------------------------- position

// PACK-27: position slides through the frames, crossfading the two it falls between. The basic
// table's frames are a sine, a triangle, a falling saw and a square (in that order, each starting
// upward), so between two the harmonics are the two frames' mixed in a straight line.
#[test]
fn position_morphs_between_frames_as_declared() {
    use std::f64::consts::PI;
    // Each frame's harmonics 1 to 3, as declared, in the same phase (sines).
    let tri = |h: usize| {
        if h % 2 == 1 { 8.0 / (PI * PI) * if h % 4 == 3 { -1.0 } else { 1.0 } / (h * h) as f64 } else { 0.0 }
    };
    let frames: [&dyn Fn(usize) -> f64; 4] = [&|h| if h == 1 { 1.0 } else { 0.0 }, &tri, &|h| 2.0 / (PI * h as f64), &|h| {
        if h % 2 == 1 { 4.0 / (PI * h as f64) } else { 0.0 }
    }];
    let f0 = hz(57.0) as f64;
    for position in [0.0f32, 1.0 / 6.0, TRIANGLE, 0.5, SAW, 0.8, SQUARE] {
        let at = position as f64 * 3.0;
        let j = (at as usize).min(2);
        let m = at - j as f64;
        let want = |h: usize| (frames[j](h) * (1.0 - m) + frames[j + 1](h) * m).abs();
        let got = harmonics(&held(cfg(SR, &[], &[]), &patch(&[(A_POSITION, position)]), 57.0, s(0.1), 1 << 16), SR, f0, &[2, 3]);
        for (h, g) in [2, 3].into_iter().zip(got) {
            let w = 20.0 * (want(h) / want(1)).log10();
            if w > -60.0 {
                assert!((g - w).abs() < 0.1, "position {position}: harmonic {h} at {g:.2} dB, declared {w:.2}");
            } else {
                assert!(g < -60.0, "position {position}: harmonic {h} at {g:.2} dB, declared none");
            }
        }
    }
}

// ---------------------------------------------------------------- aliasing

// PACK-27: no aliasing. A high note's spectrum holds nothing below its fundamental above -90 dB of
// it (the stated floor): every table's frames are band-limited, a mip level an octave, and the
// voice plays at twice the rate. Measured on the sharpest frames: the saw, the square, the narrowest
// pulse (whose harmonics are nearly all as loud as its fundamental, the worst case for the images
// a read between samples makes: -94 dB) and a digital wave, at C8, E7 and C7, at 48 and 44.1 kHz.
// A saw read without band-limiting at the same pitch is far above the floor, so the measure would
// see aliasing.
#[test]
fn a_high_note_folds_nothing_below_its_fundamental() {
    const FLOOR: f64 = -90.0;
    let below = |x: &[f32], sr: f32, f0: f64| -> f64 {
        let spec = spectrum(x, sr);
        let bin = spec[1].0;
        let low = spec.iter().filter(|s| s.0 >= 20.0 && s.0 < f0 - 12.0 * bin).fold(0.0f64, |m, s| m.max(s.1));
        10.0 * low.max(1e-300).log10() - power_at(&spec, f0)
    };
    for (table, position, what) in [(0.0, SAW, "saw"), (0.0, SQUARE, "square"), (1.0, 1.0, "narrowest pulse"), (4.0, 0.5, "digital")] {
        for (pitch, sr) in [(108.0, 48_000.0), (100.0, 44_100.0), (96.0, 48_000.0)] {
            let x = held(cfg(sr, &[table, table], &[]), &patch(&[(A_POSITION, position)]), pitch, (0.1 * sr) as usize, 1 << 16);
            let got = below(&x, sr, hz(pitch) as f64);
            assert!(got < FLOOR, "{what} at note {pitch}, {sr} Hz: {got:.1} dB below the fundamental");
        }
    }
    let f0 = hz(108.0) as f64;
    let naive: Vec<f32> = (0..1 << 16).map(|i| (1.0 - 2.0 * (i as f64 * f0 / SR as f64).fract()) as f32).collect();
    assert!(below(&naive, SR, f0) > FLOOR + 40.0, "the measure sees a saw's aliasing");
}

// PACK-27 (review B4, B7): across the keyboard, every note from A0 to C8, a note holds little but
// its harmonics: what isn't one, A-weighted from 20 Hz to 16 kHz (std's synths' measure), stays
// under -65 dB of them, for any table: the saw, the narrowest pulse, a vowel, a digital frame, and
// files whose every harmonic is as loud as the first (an impulse, white noise), the brightest a
// table can be. A level's copy holds 16 samples or more for its top harmonic, and a read draws a
// cubic through four samples: the images of a level's top harmonics, above hearing themselves,
// fold back into it otherwise (with 8 samples, an impulse held -48 dB; with 2048 samples for 1023
// harmonics read in a straight line, a saw at A1 held -45 dB).
#[test]
fn every_note_holds_little_but_its_harmonics() {
    const FLOOR: f64 = -65.0;
    let impulse: Vec<f32> = (0..2048).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();
    let mut rng = Lcg(7);
    let noise: Vec<f32> = (0..2048).map(|_| rng.next() as f32 / 32768.0 - 1.0).collect();
    let file = |x: &Vec<f32>| (vec![0.0, 0.0, 0.0, -1.0, 1.0], vec![native::asset(SR, vec![x.clone()])], 0.0);
    let built = |t: f64, position: f32| (vec![t, t], vec![], position);
    let mut loud = vec![];
    for (what, (values, assets, position)) in [
        ("saw", built(0.0, SAW)),
        ("narrowest pulse", built(1.0, 1.0)),
        ("vowel", built(2.0, 0.5)),
        ("digital", built(4.0, 0.5)),
        ("impulse", file(&impulse)),
        ("noise", file(&noise)),
    ] {
        let c = cfg(SR, &values, &assets);
        let worst = (21..=108)
            .map(|pitch| {
                let x = held(c, &patch(&[(A_POSITION, position), (SUSTAIN, 1.0)]), pitch as f32, s(0.1), 1 << 16);
                (alias_db(&x, hz(pitch as f32), SR), pitch)
            })
            .fold((f64::MIN, 0), |m, a| if a.0 > m.0 { a } else { m });
        if worst.0 >= FLOOR {
            loud.push(format!("{what}: {:.1} dB not its harmonics at note {}", worst.0, worst.1));
        }
    }
    assert!(loud.is_empty(), "{}", loud.join("\n"));
}

/// Frames of 4096 from `from`, half overlapping, while the pitch moves as `pitch(t)`: in each, the
/// strongest bin below its lowest pitch against the strongest about its middle pitch; the worst, dB.
fn worst_below_while_moving(x: &[f32], sr: f32, pitch: &dyn Fn(usize) -> f32, from: usize) -> f64 {
    let n = 4096;
    let mut worst = f64::MIN;
    let mut at = from;
    while at + n <= x.len() {
        let (lo, hi) = (0..=64).map(|k| pitch(at + k * n / 64)).fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p), h.max(p)));
        let (flo, fmid) = (hz(lo) as f64, hz(0.5 * (lo + hi)) as f64);
        let spec = spectrum(&x[at..at + n], sr);
        let bin = spec[1].0;
        let low = spec.iter().filter(|s| s.0 >= 20.0 && s.0 < flo - 14.0 * bin).fold(0.0f64, |m, s| m.max(s.1));
        let near = (hz(hi) - hz(lo)) as f64 + 20.0 * bin;
        let fund = spec.iter().filter(|s| (s.0 - fmid).abs() < near).fold(0.0f64, |m, s| m.max(s.1));
        worst = worst.max(10.0 * (low.max(1e-300) / fund).log10());
        at += n / 2;
    }
    worst
}

// PACK-27 (review B3): a pitch that moves across a level's line crossfades from the old level to
// the new through that tick, so it never clicks. A vibrato of half a semitone each way at 6 Hz
// across the lines at 750 Hz, 1.5 kHz and 3 kHz (at 48 kHz) holds nothing below its lowest pitch
// above -90 dB of its fundamental, on the saw, the narrowest pulse and a file of every harmonic (an
// impulse). Read with no crossfade they held -71, -47 and -40 dB.
#[test]
fn a_pitch_crossing_a_levels_line_never_clicks() {
    let mut loud = vec![];
    let impulse: Vec<f32> = (0..2048).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();
    for (what, values, assets, position) in [
        ("saw", vec![0.0, 0.0], vec![], SAW),
        ("narrowest pulse", vec![1.0, 1.0], vec![], 1.0),
        ("impulse", vec![0.0, 0.0, 0.0, -1.0, 1.0], vec![native::asset(SR, vec![impulse.clone()])], 0.0),
    ] {
        for centre in [78.24f32, 90.24, 102.24] {
            let pitch = move |t: usize| centre + 0.5 * (2.0 * std::f32::consts::PI * 6.0 * t as f32 / SR).sin();
            let p = patch(&[(A_POSITION, position), (SUSTAIN, 1.0)]);
            let x = play_cfg::<Wavetable>(cfg(SR, &values, &assets), s(2.0), BLOCK, &|port, _, t| match port {
                PITCH => pitch(t),
                GATE => 1.0,
                _ => p[port],
            })
            .left();
            let got = worst_below_while_moving(&x, SR, &pitch, 4800);
            if got >= -90.0 {
                loud.push(format!("{what}, a vibrato about note {centre}: {got:.1} dB below its pitch"));
            }
        }
    }
    assert!(loud.is_empty(), "{}", loud.join("\n"));
}

// ---------------------------------------------------------------- unison

// PACK-27: unison voices detune as declared: n voices spread evenly from `detune` semitones below
// the note to `detune` above.
#[test]
fn unison_voices_detune_as_declared() {
    let f0 = hz(69.0) as f64;
    for (voices, detune) in [(2.0, 0.3), (3.0, 0.5), (5.0, 1.0)] {
        let x = held(cfg(SR, &[0.0, 0.0, -1.0, -1.0, voices], &[]), &patch(&[(DETUNE, detune), (SPREAD, 0.0)]), 69.0, s(0.1), 1 << 18);
        let n = voices as usize;
        for k in 0..n {
            let semis = detune as f64 * (2.0 * k as f64 / (n - 1) as f64 - 1.0);
            let want = f0 * (semis / 12.0).exp2();
            let step = f0 * ((detune as f64 / (n - 1) as f64) / 12.0).exp2() - f0;
            let got = peak_hz(&x, SR, want - 0.4 * step, want + 0.4 * step);
            assert!(cents(got, want).abs() < 0.2, "{n} voices, detune {detune}: voice {k} at {got:.3} Hz, declared {want:.3}");
        }
    }
}

// ---------------------------------------------------------------- your own table

// PACK-27: a dropped WAV becomes a table: its samples cut into frames of 2048, position sliding
// through them as through a built-in table's.
#[test]
fn a_dropped_wav_becomes_a_table() {
    let f0 = hz(57.0) as f64;
    let file = native::asset(SR, vec![sines(&[1, 3, 5])]);
    let c = cfg(SR, &[0.0, 0.0, 0.0, -1.0, 1.0], &[file]);
    for (position, h) in [(0.0, 1), (0.5, 3), (1.0, 5)] {
        let x = held(c, &patch(&[(A_POSITION, position)]), 57.0, s(0.1), 1 << 16);
        let spec = spectrum(&x, SR);
        let top = power_at(&spec, h as f64 * f0);
        for other in [1, 3, 5].into_iter().filter(|&o| o != h) {
            let d = power_at(&spec, other as f64 * f0) - top;
            assert!(d < -80.0, "position {position}: harmonic {other} at {d:.1} dB against frame's {h}");
        }
    }
    // Oscillator B plays a file of its own, A the built-in sine: the file's place says whose it is.
    let x = held(
        cfg(SR, &[0.0, 0.0, -1.0, 0.0, 1.0], &[native::asset(SR, vec![sines(&[2])])]),
        &patch(&[(A_LEVEL, 0.0), (B_LEVEL, 1.0)]),
        57.0,
        s(0.1),
        1 << 16,
    );
    let got = harmonics(&x, SR, f0, &[2]);
    assert!(got[0] > 60.0, "b's file plays its 2nd harmonic: {got:?}");
}

// PACK-27: a file shorter than a frame is one cycle; a stereo file's channels are mixed; a file of
// more than 64 frames gives 64, spread evenly from its first to its last; a missing file plays the
// built-in table.
#[test]
fn every_kind_of_file_becomes_a_table() {
    use std::f64::consts::PI;
    let f0 = hz(57.0) as f64;
    let cycle: Vec<f32> =
        (0..600).map(|n| ((2.0 * PI * n as f64 / 600.0).sin() + 0.5 * (2.0 * PI * 2.0 * n as f64 / 600.0).sin()) as f32).collect();
    let x = held(cfg(SR, &[0.0, 0.0, 0.0, -1.0, 1.0], &[native::asset(SR, vec![cycle])]), &patch(&[]), 57.0, s(0.1), 1 << 16);
    let got = harmonics(&x, SR, f0, &[2, 3]);
    assert!((got[0] - 20.0 * 0.5f64.log10()).abs() < 0.1 && got[1] < -80.0, "a cycle of 600 samples: {got:?}");

    let stereo = native::asset(SR, vec![sines(&[1]), sines(&[2])]);
    let x = held(cfg(SR, &[0.0, 0.0, 0.0, -1.0, 1.0], &[stereo]), &patch(&[]), 57.0, s(0.1), 1 << 16);
    let got = harmonics(&x, SR, f0, &[2]);
    assert!(got[0].abs() < 0.1, "a stereo file's channels mixed: {got:?}");

    // 128 frames: the first a sine, the last its 2nd harmonic, the rest its 3rd.
    let many: Vec<usize> = (0..128)
        .map(|i| {
            if i == 0 {
                1
            } else if i == 127 {
                2
            } else {
                3
            }
        })
        .collect();
    let c = cfg(SR, &[0.0, 0.0, 0.0, -1.0, 1.0], &[native::asset(SR, vec![sines(&many)])]);
    for (position, h) in [(0.0, 1), (1.0 / 63.0, 3), (1.0, 2)] {
        let x = held(c, &patch(&[(A_POSITION, position)]), 57.0, s(0.1), 1 << 16);
        let spec = spectrum(&x, SR);
        let top = power_at(&spec, h as f64 * f0);
        let rest = [1, 2, 3].into_iter().filter(|&o| o != h).map(|o| power_at(&spec, o as f64 * f0) - top).fold(f64::MIN, f64::max);
        assert!(rest < -80.0, "64 of 128 frames, position {position}: frame's harmonic {h}, others at {rest:.1} dB");
    }

    let sine = held(cfg(SR, &[0.0, 0.0, -1.0, -1.0, 1.0], &[]), &patch(&[]), 57.0, s(0.1), 1 << 14);
    let missing = held(cfg(SR, &[0.0, 0.0, 0.0, -1.0, 1.0], &[native::missing()]), &patch(&[]), 57.0, s(0.1), 1 << 14);
    assert!(missing == sine, "a missing file plays the built-in table");
}

// ---------------------------------------------------------------- the shelf

// PACK-27: the voices of one sandbox share the tables they play, made once; a table the shelf has
// no room for is made in the voice's own memory, and plays the same bits.
#[test]
fn voices_share_their_tables() {
    let c = cfg(SR, &[1.0, 1.0, -1.0, -1.0, 1.0], &[]);
    let (a, b) = (Wavetable::new(&c), Wavetable::new(&c));
    assert!(!a.table_at(0).is_null());
    assert_eq!(a.table_at(0), b.table_at(0), "two voices of one sound share its table");
    assert_eq!(a.table_at(0), a.table_at(1), "both oscillators play the one pulse table");
    let other = Wavetable::new(&cfg(SR, &[2.0, 2.0, -1.0, -1.0, 1.0], &[]));
    assert_ne!(other.table_at(0), a.table_at(0), "another table is another");

    // Two files of 64 frames fill the shelf: a third is the voice's own.
    let full = |h: usize| native::asset(SR, vec![sines(&[h; 64])]);
    let first = Wavetable::new(&cfg(SR, &[0.0, 0.0, 0.0, 1.0, 1.0], &[full(1), full(2)]));
    let own = cfg(SR, &[0.0, 0.0, 0.0, -1.0, 1.0], &[full(3)]);
    let third = Wavetable::new(&own);
    assert!(third.table_at(0) != first.table_at(0) && third.table_at(0) != first.table_at(1));
    let here = held(own, &patch(&[]), 57.0, s(0.1), 1 << 14);
    let alone = std::thread::spawn(move || {
        let c = cfg(SR, &[0.0, 0.0, 0.0, -1.0, 1.0], &[full(3)]);
        held(c, &patch(&[]), 57.0, s(0.1), 1 << 14)
    })
    .join()
    .unwrap();
    assert!(here == alone, "a table made in the voice plays as one on the shelf");
}

// ---------------------------------------------------------------- filter

// The filter's shape is a choice: each place plays its shape. A saw at 110 Hz through each at
// 1 kHz (q 2), each harmonic's gain against the saw unfiltered: the lowpass keeps the lows and cuts
// the highs, the highpass the other way, the bandpass keeps only the cutoff's, the notch all but it.
#[test]
fn each_filter_shape_plays_its_place() {
    let f0 = hz(45.0) as f64;
    let bands = |changes: &[(usize, f32)]| -> [f64; 3] {
        let mut p = patch(&[(A_POSITION, SAW)]);
        changes.iter().for_each(|&(port, v)| p[port] = v);
        let spec = spectrum(&held(cfg(SR, &[], &[]), &p, 45.0, s(0.1), 1 << 16), SR);
        [power_at(&spec, f0), power_at(&spec, 9.0 * f0), power_at(&spec, 60.0 * f0)]
    };
    let open = bands(&[(CUTOFF, 20_000.0)]);
    let gain = |shape: f32| {
        let b = bands(&[(FILTER, shape), (CUTOFF, 1000.0), (Q, 2.0)]);
        [0, 1, 2].map(|i| b[i] - open[i])
    };
    let [low, band, high, notch] = [0.0, 1.0, 2.0, 3.0].map(gain);
    assert!(low[0] > -1.0 && low[2] < -25.0, "lowpass: {low:?}");
    assert!(high[0] < -30.0 && high[2] > -1.0, "highpass: {high:?}");
    assert!(band[1] > -1.0 && band[0] < -15.0 && band[2] < -15.0, "bandpass: {band:?}");
    assert!(notch[1] < -15.0 && notch[0] > -1.0 && notch[2] > -1.0, "notch: {notch:?}");
}

// The filter moves smoothly however fast it moves: its gain and damping ramp a sample at a time
// between the control ticks, so a sweep of 6 octaves in 50 ms (its envelope) makes nothing at the
// ticks' rate (3 kHz at 48 kHz) on a sine at 220 Hz: under -120 dB of the sine (-149 measured). A
// filter set once a tick steps there (-107 dB).
#[test]
fn a_fast_filter_sweep_never_steps() {
    let p = patch(&[(A_POSITION, SINE), (CUTOFF, 200.0), (ENV, 6.0), (F_ATTACK, 0.001), (F_DECAY, 0.05), (F_SUSTAIN, 0.0), (Q, 4.0)]);
    let x = held(cfg(SR, &[], &[]), &p, 57.0, s(0.005), 4096);
    let spec = spectrum(&x, SR);
    let sine = power_at(&spec, 220.0);
    let ticks = spec.iter().filter(|s| (2_600.0..3_400.0).contains(&s.0)).fold(0.0f64, |m, s| m.max(s.1));
    let got = 10.0 * ticks.max(1e-300).log10() - sine;
    assert!(got < -120.0, "the sweep steps at the ticks' rate: {got:.1} dB under the sine");
}

// PACK-27 (review B6): an oscillator is held under the song's own Nyquist, so at the lowest rate a
// song may have (8 kHz), a note as high as it goes (127, pitch A two octaves up) plays bounded for a
// whole minute: its phase never runs past a cycle. It grew past 1e15 in 10 s.
#[test]
fn a_note_too_high_for_a_low_rate_stays_bounded() {
    for sr in [8_000.0f32, 11_025.0] {
        let p = patch(&[(A_POSITION, SAW), (SUSTAIN, 1.0), (A_PITCH, 24.0), (CUTOFF, 20_000.0)]);
        let x = note(cfg(sr, &[0.0, 0.0], &[]), &p, 127.0, (60.0 * sr) as usize, BLOCK).left();
        assert!(x.iter().all(|v| v.is_finite()) && peak(&x) < 2.0, "{sr} Hz: peak {}", peak(&x));
    }
}

// ---------------------------------------------------------------- every voice

mod counting {
    //! A count of what this thread allocates and frees.
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static HERE: Cell<u64> = const { Cell::new(0) };
    }

    pub struct Counting;

    fn count() {
        let _ = HERE.try_with(|c| c.set(c.get() + 1));
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count();
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            count();
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            count();
            unsafe { System.realloc(ptr, layout, size) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    /// What this thread has allocated and freed so far.
    pub fn here() -> u64 {
        HERE.with(Cell::get)
    }
}

// PACK-27, ABI-5: a voice allocates nothing while it plays: every path (both oscillators, a file's
// table, eight unison voices, the LFO, each filter shape, legato, a release, a note again) once it
// is born.
#[test]
fn no_allocation_while_playing() {
    let file = native::asset(SR, vec![sines(&[1, 2, 3, 4])]);
    let c = cfg(SR, &[3.0, 0.0, 0.0, -1.0, 8.0], &[file]);
    let mut host = Host::<Wavetable>::of(c);
    let p = patch(&[(B_LEVEL, 0.5), (LFO_POSITION, 0.5), (LFO_CUTOFF, 2.0), (ENV, 3.0), (SPREAD, 1.0)]);
    let before = counting::here();
    for b in 0..400 {
        for (port, v) in p.iter().enumerate() {
            host.port_mut(port).fill(*v);
        }
        host.port_mut(GATE).fill(if b % 100 < 70 { 1.0 } else { 0.0 });
        host.port_mut(PITCH).fill(40.0 + (b % 37) as f32);
        host.port_mut(FILTER).fill((b / 100) as f32);
        host.process(BLOCK as u32);
        if b == 200 {
            host.reset();
        }
    }
    assert_eq!(counting::here() - before, 0, "it allocated or freed while it played");
}

#[test]
fn deterministic_whatever_the_block_size() {
    let c = cfg(SR, &[1.0, 2.0, -1.0, -1.0, 4.0], &[]);
    let p = patch(&[(B_LEVEL, 0.7), (LFO_POSITION, 0.4), (A_POSITION, 0.3), (ENV, 2.0), (LFO_CUTOFF, 1.0)]);
    let notes = |split: usize| {
        let p = p.clone();
        play_cfg::<Wavetable>(c, s(1.0), split, &move |port, _, t| match port {
            PITCH => {
                if t < s(0.4) {
                    48.0
                } else {
                    55.0
                }
            }
            GATE => {
                if t < s(0.7) && t != s(0.4) {
                    1.0
                } else {
                    0.0
                }
            }
            _ => p[port],
        })
        .main()
        .to_vec()
    };
    let whole = notes(BLOCK);
    assert!(peak(&whole.iter().map(|f| f[0]).collect::<Vec<_>>()) > 0.05);
    for split in [1, 7, 64] {
        assert!(notes(split) == whole, "split {split} differs");
    }
}

#[test]
fn a_reset_starts_over() {
    let mut p = patch(&[(B_LEVEL, 0.5), (LFO_POSITION, 0.3)]);
    p[PITCH] = 52.0;
    p[GATE] = 1.0;
    assert_reset_starts_over::<Wavetable>(&p, s(0.2));
}

#[test]
fn silent_without_a_note_and_its_release_ends_in_exact_silence() {
    let p = patch(&[]);
    let quiet = play_cfg::<Wavetable>(cfg(SR, &[], &[]), s(0.5), BLOCK, &|port, _, _| {
        if port == GATE { 0.0 } else { p[port] }
    })
    .left();
    assert!(quiet.iter().all(|&v| v == 0.0));
    let p = patch(&[(RELEASE, 0.1)]);
    let played = play_cfg::<Wavetable>(cfg(SR, &[], &[]), s(1.0), BLOCK, &|port, _, t| match port {
        GATE => {
            if t < s(0.2) {
                1.0
            } else {
                0.0
            }
        }
        PITCH => 60.0,
        _ => p[port],
    });
    let x = played.left();
    assert!(peak(&x[..s(0.2)]) > 0.05);
    let end = last_sound(&x).expect("it sounded");
    assert!(end < s(0.5), "the release ends: last sound at {end}");
    assert!(!played.active[s(0.9)], "not active once silent");
}

#[test]
fn extreme_controls_never_make_nan() {
    let base = patch(&[(B_LEVEL, 0.5)]);
    // At the lowest rate a song may have too (review B6): a note as high as can be stays under it.
    for sr in [SR, 8_000.0] {
        for set in torture(&base, &[PITCH, GATE]) {
            for pitch in [f32::NAN, -1e9, 0.0, 60.0, 200.0] {
                let x = note(cfg(sr, &[0.0, 4.0, -1.0, -1.0, 3.0], &[]), &set, pitch, (0.1 * sr) as usize, BLOCK);
                let l = x.left();
                assert!(l.iter().all(|v| v.is_finite()) && peak(&l) < 16.0, "{set:?} at {pitch}, {sr} Hz: peak {}", peak(&l));
            }
        }
    }
    // Absurd settings too.
    for values in [[f64::NAN; 5], [-7.0, 99.0, 5.0, 1e9, -3.0], [4.0, 4.0, -1.0, -1.0, 1e9]] {
        let x = note(cfg(SR, &values, &[]), &patch(&[]), 60.0, s(0.1), BLOCK).left();
        assert!(x.iter().all(|v| v.is_finite()) && peak(&x) > 0.01, "{values:?}");
    }
}
