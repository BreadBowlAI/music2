//! The synth voice: what every voice guarantees (silence, tails, no NaN, the same output
//! for any block split), then how it sounds (levels, aliasing, unison, filter, pitch).

use super::*;
use crate::testkit::*;

const NAMES: [&str; INPUTS] = [
    "pitch",
    "gate",
    "velocity",
    "saw",
    "square",
    "width",
    "sub",
    "sub_octave",
    "noise",
    "unison",
    "detune",
    "spread",
    "drop",
    "drop_time",
    "vibrato",
    "cutoff",
    "resonance",
    "keytrack",
    "env",
    "f_attack",
    "f_decay",
    "f_sustain",
    "attack",
    "decay",
    "sustain",
    "release",
    "drive",
    "vel_amount",
    "level",
    "seed",
    "wheel",
    "shape",
];

/// The manifest's defaults, with `changes` applied.
fn patch(changes: &[(usize, f32)]) -> Vec<f32> {
    let mut p = defaults("synth", &NAMES);
    for &(port, v) in changes {
        p[port] = v;
    }
    p
}

/// A note: pitch, velocity, first sample, sample after its last.
type Note = (f32, f32, usize, usize);

/// The synth playing `notes` one at a time (the latest wins, as std's mono feeds it): the
/// gate is on while any note is, and drops for one sample where a note starts on another.
fn synth(patch: &[f32], notes: &[Note], frames: usize, split: usize) -> Played {
    let input = |p: usize, _c: usize, t: usize| -> f32 {
        let now = notes.iter().rfind(|n| n.2 <= t);
        match p {
            PITCH => now.map_or(60.0, |n| n.0),
            VELOCITY => now.map_or(patch[VELOCITY], |n| n.1),
            GATE => {
                let held = notes.iter().any(|n| n.2 <= t && t < n.3);
                let retrigger = notes.iter().any(|n| n.2 == t + 1) && notes.iter().any(|n| n.2 <= t && t < n.3);
                (held && !retrigger) as i32 as f32
            }
            _ => patch[p],
        }
    };
    play::<Synth>(frames, split, &input)
}

fn held(patch: &[f32], pitch: f32, seconds: f32) -> Played {
    synth(patch, &[(pitch, 0.8, 0, s(seconds))], s(seconds), BLOCK)
}

// ---------------------------------------------------------------- every voice

#[test]
fn silence_until_a_note() {
    let out = synth(&patch(&[]), &[], s(1.0), BLOCK);
    assert!(out.main().iter().all(|f| f[0] == 0.0 && f[1] == 0.0));
    assert!(out.active.iter().all(|a| !a));
}

#[test]
fn tails_end_in_exact_silence() {
    for release in [0.003, 0.05, 0.3, 2.0] {
        for (unison, res) in [(1.0, 0.1), (7.0, 1.0)] {
            let p = patch(&[(RELEASE, release), (UNISON, unison), (RESONANCE, res), (SUSTAIN, 1.0)]);
            let off = s(0.3);
            let out = synth(&p, &[(57.0, 1.0, 0, off)], off + s(release * 1.5 + 0.5), BLOCK);
            let left = out.left();
            let last = last_sound(&left).unwrap();
            let limit = off + s(release * 1.5 + 0.02);
            assert!(last < limit, "release {release}: sound until {:.3} s after the note", (last - off) as f32 / SR);
            assert!(!out.active[limit] && out.active[off + 1], "release {release}: active after its tail");
            // Exactly 0, never a crawl through denormals.
            assert!(left.iter().all(|&v| v == 0.0 || v.abs() >= f32::MIN_POSITIVE));
        }
    }
}

#[test]
fn extreme_settings_never_make_nan() {
    let base = patch(&[(UNISON, 7.0), (SQUARE, 0.5), (SUB, 0.5), (NOISE, 0.2), (DRIVE, 12.0)]);
    for (n, set) in torture(&base, &[PITCH, GATE]).iter().enumerate() {
        for pitch in [0.0, 60.0, 127.0, f32::NAN] {
            // The gate flips every 3 samples, notes change pitch on each.
            let input = |p: usize, _c: usize, t: usize| match p {
                GATE => ((t / 3) % 2) as f32,
                PITCH => pitch + (t / 6 % 5) as f32,
                _ => set[p],
            };
            let out = play::<Synth>(2_000, BLOCK, &input);
            let bad = out.main().iter().position(|f| !f[0].is_finite() || !f[1].is_finite() || f[0].abs() > 4.0 || f[1].abs() > 4.0);
            assert_eq!(bad, None, "set {n} {set:?} at pitch {pitch}: {:?}", bad.map(|b| out.main()[b]));
        }
    }
}

#[test]
fn one_default_voice_stays_under_full_scale() {
    for pitch in [24.0, 48.0, 72.0, 96.0] {
        let p = patch(&[(VELOCITY, 1.0)]);
        let out = synth(&p, &[(pitch, 1.0, 0, s(0.5))], s(1.0), BLOCK);
        assert!(peak(&out.left()) <= 1.0, "pitch {pitch}: {}", peak(&out.left()));
    }
}

#[test]
fn the_same_whatever_the_block_size() {
    let p = patch(&[(UNISON, 5.0), (SQUARE, 0.5), (NOISE, 0.1), (SUB, 0.3), (DRIVE, 6.0), (ENV, 2.0), (VIBRATO, 0.3), (DROP, 12.0)]);
    // Cutoff and width move every 1000 samples, to exercise the smoothing and the ticks.
    let notes = [(48.0, 0.9, 10, 9_000), (55.0, 0.5, 9_000, 15_000), (62.0, 1.0, 17_003, 20_000)];
    let run = |split| {
        let input = |port: usize, _c: usize, t: usize| match port {
            CUTOFF => 400.0 + 900.0 * ((t / 1_000) % 4) as f32,
            WIDTH => 0.2 + 0.2 * ((t / 1_300) % 3) as f32,
            GATE | PITCH | VELOCITY => {
                let now = notes.iter().rfind(|n| n.2 <= t);
                match port {
                    PITCH => now.map_or(60.0, |n| n.0),
                    VELOCITY => now.map_or(0.8, |n| n.1),
                    _ => notes.iter().any(|n| n.2 <= t && t < n.3 && n.2 != t + 1) as i32 as f32,
                }
            }
            _ => p[port],
        };
        play::<Synth>(24_000, split, &input).out.remove(0)
    };
    let reference = run(BLOCK);
    assert!(reference.iter().any(|f| f[0] != 0.0));
    assert!(run(BLOCK) == reference, "differs between runs");
    for split in [1, 7, 64] {
        let out = run(split);
        assert_eq!(out.iter().zip(&reference).position(|(a, b)| a != b), None, "blocks of {split} differ");
    }
}

#[test]
fn a_reset_starts_over() {
    let mut p = patch(&[(UNISON, 5.0), (SQUARE, 0.5), (NOISE, 0.1), (DRIVE, 6.0), (DROP, 5.0)]);
    p[GATE] = 1.0;
    assert_reset_starts_over::<Synth>(&p, 5_000);
}

#[test]
fn oscillators_are_in_tune() {
    // Saw, pulse and sub, from the bottom of the keyboard to the top: within 0.5 cents.
    for (changes, octave) in
        [(vec![], 0.0), (vec![(SAW, 0.0), (SQUARE, 1.0), (WIDTH, 0.3)], 0.0), (vec![(SAW, 0.0), (SUB, 1.0), (SUB_OCTAVE, -1.0)], -1.0)]
    {
        for pitch in [24.0, 45.0, 69.0, 93.0] {
            let mut c = changes.clone();
            c.extend([(CUTOFF, 20_000.0), (KEYTRACK, 0.0), (SUSTAIN, 1.0)]);
            let out = held(&patch(&c), pitch, 1.5);
            let want = hz(pitch + 12.0 * octave) as f64;
            let f = peak_hz(&out.left()[s(0.2)..], SR, want * 0.8, want * 1.2);
            let cents = 1200.0 * (f / want).log2();
            assert!(cents.abs() < 0.5, "{changes:?} at {pitch}: {f:.3} Hz, {cents:.2} cents off");
        }
    }
}

#[test]
fn a_long_tail_never_goes_subnormal() {
    let p = patch(&[(RELEASE, 6.0), (SUSTAIN, 1.0), (RESONANCE, 0.9)]);
    let out = synth(&p, &[(40.0, 1.0, 0, s(0.2))], s(10.0), BLOCK);
    let x = out.left();
    assert!(x.iter().all(|&v| v == 0.0 || v.abs() >= f32::MIN_POSITIVE));
    assert!(last_sound(&x).unwrap() < s(0.2 + 9.0 + 0.02));
}

#[test]
fn no_dc_at_any_setting() {
    for changes in [
        vec![],
        vec![(SAW, 0.0), (SQUARE, 1.0), (WIDTH, 0.05)],
        vec![(SAW, 0.0), (SQUARE, 1.0), (WIDTH, 0.95)],
        vec![(SQUARE, 1.0), (WIDTH, 0.3), (DRIVE, 24.0)],
        vec![(UNISON, 7.0), (DETUNE, 1.0), (SPREAD, 1.0), (DRIVE, 24.0)],
        vec![(SAW, 0.0), (SUB, 1.0), (SUB_OCTAVE, -2.0), (DRIVE, 24.0)],
        vec![(NOISE, 1.0), (RESONANCE, 1.0)],
    ] {
        let mut c = changes.clone();
        c.extend([(SUSTAIN, 1.0), (CUTOFF, 20_000.0)]);
        let out = held(&patch(&c), 45.0, 2.0);
        for ch in 0..2 {
            let x: Vec<f32> = out.main()[s(1.0)..].iter().map(|f| f[ch]).collect();
            let m = dc(&x);
            assert!(m.abs() <= 1e-4, "{changes:?}: mean {m:e}");
        }
    }
}

// ---------------------------------------------------------------- how it sounds

#[test]
fn default_voice_peaks_near_minus_12_db() {
    for pitch in [48.0, 60.0, 72.0] {
        let out = held(&patch(&[]), pitch, 1.0);
        let p = db(peak(&out.left()));
        assert!((-14.0..=-10.0).contains(&p), "pitch {pitch}: peak {p:.1} dBFS");
    }
}

/// Aliasing of a held note with the filter wide open.
fn aliasing(pitch: f32, changes: &[(usize, f32)]) -> f64 {
    let mut c = vec![(CUTOFF, 20_000.0), (KEYTRACK, 0.0), (RESONANCE, 0.0), (SUSTAIN, 1.0)];
    c.extend_from_slice(changes);
    let out = held(&patch(&c), pitch, 2.0);
    alias_db(&out.left()[s(0.5)..], hz(pitch), SR)
}

#[test]
fn saws_alias_below_the_targets() {
    // A-weighted aliases relative to the harmonics, 20 Hz to 16 kHz.
    for (pitch, limit) in [(72.0, -70.0), (84.0, -70.0), (96.0, -55.0)] {
        let a = aliasing(pitch, &[]);
        println!("saw at {pitch}: {a:.1} dB");
        assert!(a <= limit, "saw at {pitch}: {a:.1} dB");
    }
    let a = aliasing(84.0, &[(DRIVE, 18.0)]);
    println!("saw at C6, 18 dB drive: {a:.1} dB");
    assert!(a <= -60.0, "18 dB of drive at C6: {a:.1} dB");
    let a = aliasing(84.0, &[(SAW, 0.0), (SQUARE, 1.0), (WIDTH, 0.3)]);
    println!("pulse at C6: {a:.1} dB");
    assert!(a <= -70.0, "pulse at C6: {a:.1} dB");
}

#[test]
fn mixes_that_reach_the_ladders_knee_dont_alias() {
    // A narrow pulse peaks at 1.8 once its offset is gone, and a saw with a sub near 2: the
    // ladder's saturation shapes them, at 2×, so nothing folds back.
    for (name, changes, pitch, limit) in [
        ("10% pulse at C7", vec![(SAW, 0.0), (SQUARE, 1.0), (WIDTH, 0.1)], 96.0, -75.0),
        ("10% pulse at C6", vec![(SAW, 0.0), (SQUARE, 1.0), (WIDTH, 0.1)], 84.0, -80.0),
        ("saw and pulse at C7", vec![(SQUARE, 1.0)], 96.0, -75.0),
        ("saw and sub at C6", vec![(SUB, 1.0), (SUB_OCTAVE, 0.0)], 84.0, -75.0),
    ] {
        let a = aliasing(pitch, &changes);
        println!("{name}: {a:.1} dB");
        assert!(a <= limit, "{name}: {a:.1} dB");
    }
}

#[test]
fn one_voice_is_mono_and_a_spread_stack_is_wide() {
    let mono = held(&patch(&[]), 57.0, 0.5);
    assert!(mono.main().iter().all(|f| f[0] == f[1]));
    let narrow = held(&patch(&[(UNISON, 7.0), (SPREAD, 0.0)]), 57.0, 0.5);
    assert!(narrow.main().iter().all(|f| f[0] == f[1]), "spread 0 isn't mono");
    // The supersaw's settings: wide, but not hollow in mono.
    let wide = held(&patch(&[(UNISON, 7.0), (DETUNE, 0.35), (SPREAD, 0.85), (SUSTAIN, 1.0)]), 57.0, 2.0);
    let (l, r): (Vec<f32>, Vec<f32>) = wide.main()[s(0.5)..].iter().map(|f| (f[0], f[1])).unzip();
    let corr = l.iter().zip(&r).map(|(a, b)| (a * b) as f64).sum::<f64>() / (rms(&l) as f64 * rms(&r) as f64 * l.len() as f64);
    let fold: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
    let loss = db(rms(&fold) / (0.5 * (rms(&l) + rms(&r))));
    println!("supersaw: correlation {corr:.2}, mono fold-down {loss:.2} dB");
    assert!((0.1..=0.5).contains(&corr), "correlation {corr:.2}");
    assert!(loss >= -3.0, "the mono fold-down loses {loss:.2} dB");
}

#[test]
fn unison_keeps_its_level_and_detune_widens_it() {
    let level = |u: f32, d: f32| rms(&held(&patch(&[(UNISON, u), (DETUNE, d), (SPREAD, 0.0), (SUSTAIN, 1.0)]), 57.0, 1.5).left()[s(0.5)..]);
    let one = level(1.0, 0.2);
    for u in [3.0, 5.0, 7.0] {
        let change = db(level(u, 0.5) / one);
        assert!(change.abs() < 2.0, "unison {u} changes the level by {change:.1} dB");
    }
    // Detune spreads the stack's energy around each harmonic.
    let spread_of = |d: f32| {
        let out = held(&patch(&[(UNISON, 7.0), (DETUNE, d), (SPREAD, 0.0), (SUSTAIN, 1.0)]), 57.0, 1.5);
        let spec = spectrum(&out.left()[s(0.5)..], SR);
        let f0 = hz(57.0) as f64;
        let near = |w: f64| spec.iter().filter(|x| (x.0 - f0).abs() < w).map(|x| x.1).sum::<f64>();
        near(f0 * 0.05) / near(f0 * 0.002).max(1e-30)
    };
    assert!(spread_of(0.6) > 2.0 * spread_of(0.05), "{} vs {}", spread_of(0.6), spread_of(0.05));
}

/// The frequency a self-oscillating filter sings at, with no oscillators.
fn singing(pitch: f32, cutoff: f32, keytrack: f32) -> f64 {
    let p = patch(&[(SAW, 0.0), (RESONANCE, 1.0), (CUTOFF, cutoff), (KEYTRACK, keytrack), (SUSTAIN, 1.0)]);
    let out = held(&p, pitch, 2.0);
    peak_hz(&out.left()[s(1.0)..], SR, 20.0, 20_000.0)
}

#[test]
fn resonance_1_sings_at_the_cutoff() {
    for cutoff in [220.0, 1_000.0, 4_000.0] {
        let f = singing(60.0, cutoff, 0.0);
        assert!((f / cutoff as f64 - 1.0).abs() < 0.02, "cutoff {cutoff}: sings at {f:.1}");
    }
}

#[test]
fn keytrack_1_moves_the_cutoff_an_octave_an_octave() {
    let (lo, hi) = (singing(48.0, 500.0, 1.0), singing(60.0, 500.0, 1.0));
    assert!((hi / lo - 2.0).abs() < 0.04, "{lo:.1} Hz → {hi:.1} Hz");
    assert!((hi / 500.0 - 1.0).abs() < 0.02);
}

/// The share of `x`'s power above `f` Hz.
fn above(x: &[f32], f: f64) -> f64 {
    let spec = spectrum(x, SR);
    spec.iter().filter(|s| s.0 > f).map(|s| s.1).sum::<f64>() / spec.iter().map(|s| s.1).sum::<f64>()
}

#[test]
fn the_filter_envelope_opens_the_sound_and_closes_it() {
    let out = |env: f32| {
        synth(&patch(&[(CUTOFF, 300.0), (ENV, env), (F_DECAY, 0.3), (SUSTAIN, 1.0)]), &[(48.0, 0.8, 0, s(1.5))], s(1.5), BLOCK).left()
    };
    let (shut, open) = (out(0.0), out(4.0));
    let (a, b) = (above(&shut[..s(0.1)], 1_000.0), above(&open[..s(0.1)], 1_000.0));
    assert!(b > 30.0 * a, "4 octaves of envelope: {b:.2e} of the power above 1 kHz, against {a:.2e}");
    // With no filter sustain it closes again.
    let late = above(&open[s(1.0)..], 1_000.0);
    assert!(late < 2.0 * a, "{late:.2e} late in the note");
}

#[test]
fn velocity_0_is_silent_and_sensitivity_sets_the_range() {
    let level = |v: f32, sens: f32| {
        let p = patch(&[(VEL_AMOUNT, sens)]);
        peak(&synth(&p, &[(60.0, v, 0, s(0.5))], s(0.5), BLOCK).left())
    };
    assert_eq!(level(0.0, 0.5), 0.0);
    assert_eq!(level(0.0, 1.0), 0.0);
    // At 0.5 the level follows velocity: 0.3 is 10.5 dB down; at 0 every note is the same.
    let range = db(level(1.0, 0.5)) - db(level(0.3, 0.5));
    assert!((range - 10.5).abs() < 1.5, "0.3 against 1 at sensitivity 0.5: {range:.1} dB");
    assert!((db(level(1.0, 0.0)) - db(level(0.3, 0.0))).abs() < 0.1);
    assert!(db(level(1.0, 1.0)) - db(level(0.3, 1.0)) > 18.0);
}

#[test]
fn a_legato_note_brings_its_own_velocity() {
    // One gate, the velocity changing under it, as std's mono plays a legato note.
    let p = patch(&[(SUSTAIN, 1.0)]);
    let input = |port: usize, _c: usize, t: usize| match port {
        GATE => (t < s(1.0)) as i32 as f32,
        VELOCITY => {
            if t < s(0.5) {
                0.3
            } else {
                1.0
            }
        }
        PITCH => 60.0,
        _ => p[port],
    };
    let x = play::<Synth>(s(1.0), BLOCK, &input).left();
    let fresh = synth(&p, &[(60.0, 1.0, 0, s(1.0))], s(1.0), BLOCK).left();
    let (soft, legato, hard) = (rms(&x[s(0.3)..s(0.5)]), rms(&x[s(0.7)..s(0.9)]), rms(&fresh[s(0.7)..s(0.9)]));
    assert!((db(legato) - db(hard)).abs() < 0.5, "legato {:.1} dB, struck {:.1}", db(legato), db(hard));
    assert!(db(legato) - db(soft) > 8.0);
    // The change is smoothed, not stepped: over the first cycle after it (C4's is 3.8 ms) the
    // level is still well short of where it settles.
    let (first, later) = (peak(&x[s(0.5)..s(0.504)]), peak(&x[s(0.53)..s(0.534)]));
    assert!(db(later) - db(first) > 3.0, "the first cycle peaks {:.1} dB below the settled level", db(later) - db(first));
}

#[test]
fn a_sub_at_the_note_reinforces_the_saw_on_every_note() {
    // Locked to the saw, a sub at the note's own pitch adds to its fundamental by the same
    // amount whatever the note (each note's saw starts at a phase of its own).
    let fundamental = |pitch: f32| {
        let p = patch(&[(SUB, 0.6), (SUB_OCTAVE, 0.0), (CUTOFF, 20_000.0), (KEYTRACK, 0.0), (SUSTAIN, 1.0)]);
        let x = held(&p, pitch, 1.0).left();
        let f0 = hz(pitch) as f64;
        let spec = spectrum(&x[s(0.3)..], SR);
        let band = |f: f64| spec.iter().filter(|b| (b.0 - f).abs() < 0.05 * f0).map(|b| b.1).sum::<f64>();
        10.0 * (band(f0) / band(2.0 * f0)).log10()
    };
    let all: Vec<f64> = [33.0, 40.0, 45.0, 52.0, 57.0].map(fundamental).to_vec();
    let (lo, hi) = all.iter().fold((f64::MAX, f64::MIN), |m, &v| (m.0.min(v), m.1.max(v)));
    assert!(hi - lo < 0.5, "the fundamental against the 2nd harmonic: {all:?}");
    assert!(lo > 6.0 + 3.0, "the sub adds to it: {all:?}");
}

#[test]
fn velocity_raises_level_and_brightness() {
    let mut last = (0.0, 0.0);
    for v in [0.2, 0.5, 0.8, 1.0] {
        let p = patch(&[(CUTOFF, 600.0), (ENV, 3.0), (F_DECAY, 0.4)]);
        let out = synth(&p, &[(48.0, v, 0, s(0.6))], s(0.6), BLOCK);
        let x = &out.left()[..s(0.3)];
        let now = (rms(x), centroid(x, SR));
        assert!(now.0 > last.0 && now.1 > last.1, "velocity {v}: {now:?} after {last:?}");
        last = now;
    }
}

#[test]
fn drop_and_vibrato_bend_the_pitch() {
    // An 808-style drop: two octaves above, falling to the note in about drop_time.
    let p = patch(&[(SAW, 0.0), (SUB, 1.0), (SUB_OCTAVE, 0.0), (DROP, 24.0), (DROP_TIME, 0.05), (SUSTAIN, 1.0), (CUTOFF, 20_000.0)]);
    let out = held(&p, 36.0, 1.0);
    let start = peak_hz(&out.left()[..1_024], SR, 20.0, 2_000.0);
    let settled = peak_hz(&out.left()[s(0.5)..], SR, 20.0, 2_000.0);
    assert!(start > 2.0 * hz(36.0) as f64, "starts at {start:.1}");
    assert!((settled / hz(36.0) as f64 - 1.0).abs() < 0.01, "settles at {settled:.1}");
    // Vibrato: none at the start, ±depth once faded in.
    let p = patch(&[(SAW, 0.0), (SUB, 1.0), (SUB_OCTAVE, 0.0), (VIBRATO, 1.0), (SUSTAIN, 1.0), (CUTOFF, 20_000.0)]);
    let out = held(&p, 69.0, 2.0);
    let crossings = |x: &[f32]| {
        // Upward zero crossings, placed between samples.
        let ups: Vec<f64> =
            (1..x.len()).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).map(|i| i as f64 - x[i] as f64 / (x[i] - x[i - 1]) as f64).collect();
        let periods: Vec<f64> = ups.windows(2).map(|w| w[1] - w[0]).collect();
        let f: Vec<f64> = periods.iter().map(|p| SR as f64 / p).collect();
        (f.iter().cloned().fold(f64::MAX, f64::min), f.iter().cloned().fold(0.0, f64::max))
    };
    let (lo, hi) = crossings(&out.left()[s(1.0)..s(2.0)]);
    let depth = 12.0 * (hi / lo).log2() / 2.0;
    assert!((depth - 1.0).abs() < 0.15, "vibrato ±{depth:.2} semitones");
    let (lo, hi) = crossings(&out.left()[..s(0.03)]);
    assert!(12.0 * (hi / lo).log2() < 0.2, "vibrato at the start");
}

/// The vibrato's depth over `x`, in semitones each way: from the highest and lowest frequency
/// between upward zero crossings.
fn vibrato_depth(x: &[f32]) -> f64 {
    let ups: Vec<f64> =
        (1..x.len()).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).map(|i| i as f64 - x[i] as f64 / (x[i] - x[i - 1]) as f64).collect();
    let f: Vec<f64> = ups.windows(2).map(|w| SR as f64 / (w[1] - w[0])).collect();
    let (lo, hi) = (f.iter().cloned().fold(f64::MAX, f64::min), f.iter().cloned().fold(0.0, f64::max));
    12.0 * (hi / lo).log2() / 2.0
}

// The mod wheel (RENDER-14's Midi, D8 of track pm) deepens the vibrato by up to half a semitone,
// at once, not faded in as the vibrato is; on top of a vibrato, the two add. At 0 it is no
// vibrato at all.
#[test]
fn the_wheel_deepens_the_synths_vibrato() {
    let sine = [(SAW, 0.0), (SUB, 1.0), (SUB_OCTAVE, 0.0), (SUSTAIN, 1.0), (CUTOFF, 20_000.0)];
    let with = |more: &[(usize, f32)]| patch(&[&sine[..], more].concat());
    let depth = vibrato_depth(&held(&with(&[(WHEEL, 1.0)]), 69.0, 1.0).left()[s(0.05)..s(0.5)]);
    assert!((depth - 0.5).abs() < 0.05, "the wheel alone: ±{depth:.3} semitones, from the start");
    let depth = vibrato_depth(&held(&with(&[(WHEEL, 1.0), (VIBRATO, 1.0)]), 69.0, 2.0).left()[s(1.0)..s(2.0)]);
    assert!((depth - 1.5).abs() < 0.15, "with a vibrato of 1: ±{depth:.2}");
    let depth = vibrato_depth(&held(&with(&[(WHEEL, 0.0)]), 69.0, 1.0).left()[s(0.05)..s(0.5)]);
    assert!(depth < 0.01, "at 0: ±{depth:.4}");
}

#[test]
fn retriggers_and_note_offs_dont_click() {
    // A held note, then another starting on it: the gate drops for one sample and both
    // envelopes start again from where they are. The biggest step in the 5 ms after is no
    // bigger than 1.5 times the biggest in the 20 ms before.
    let at = s(0.5);
    // (No drop here: a drop starting again is a designed jump in pitch.)
    let patches =
        [patch(&[]), patch(&[(SAW, 0.0), (SUB, 1.0), (SUB_OCTAVE, 0.0), (DRIVE, 6.0)]), patch(&[(SQUARE, 1.0), (ATTACK, 0.0005)])];
    for (n, p) in patches.iter().enumerate() {
        let out = synth(p, &[(48.0, 0.8, 0, at), (48.0, 0.8, at, s(1.0))], s(1.0), BLOCK);
        let x = out.left();
        let (before, after) = (max_step(&x, at - s(0.02), at), max_step(&x, at, at + s(0.005)));
        assert!(after <= 1.5 * before, "patch {n} retrigger: {after} after vs {before} before");
        assert!(high_rise(&x, at) < 6.0, "patch {n} retrigger: {:.1} dB more above 8 kHz", high_rise(&x, at));
        let out = synth(p, &[(48.0, 0.8, 0, at)], s(1.0), BLOCK);
        let x = out.left();
        let (before, after) = (max_step(&x, at - s(0.02), at), max_step(&x, at, at + s(0.005)));
        assert!(after <= 1.5 * before, "patch {n} note-off: {after} after vs {before} before");
    }
    // A unison stack's edges beat, so its biggest step wanders: there, what counts is that
    // the retrigger adds no top end.
    let p = patch(&[(UNISON, 3.0)]);
    let x = synth(&p, &[(48.0, 0.8, 0, at), (48.0, 0.8, at, s(1.0))], s(1.0), BLOCK).left();
    assert!(high_rise(&x, at) < 6.0, "unison retrigger: {:.1} dB more above 8 kHz", high_rise(&x, at));
}

#[test]
fn noise_and_sub_are_there() {
    let noise = held(&patch(&[(SAW, 0.0), (NOISE, 1.0), (CUTOFF, 20_000.0), (KEYTRACK, 0.0)]), 60.0, 1.0);
    let c = centroid(&noise.left()[s(0.2)..], SR);
    assert!(c > 5_000.0, "noise centroid {c:.0}");
    let sub = held(&patch(&[(SAW, 0.0), (SUB, 1.0), (SUB_OCTAVE, -1.0)]), 60.0, 1.0);
    let f = peak_hz(&sub.left()[s(0.2)..], SR, 20.0, 2_000.0);
    assert!((f / (hz(60.0) / 2.0) as f64 - 1.0).abs() < 0.005, "sub at {f:.1}");
}

/// The power of `x` within 8 Hz of `f`.
fn band(x: &[f32], f: f64) -> f64 {
    spectrum(x, SR).iter().filter(|s| (s.0 - f).abs() < 8.0).map(|s| s.1).sum()
}

// CARD-12, wave 3: `shape`, a choice, says what the stack's main wave is. The saw is its default,
// the synth as it always was; a square at width 0.5 has no even harmonics; a triangle's harmonics
// fall faster than the saw's; a sine has almost none. Each plays the note's pitch, and a change of
// shape while a note plays glides over a tick: no click.
#[test]
fn the_shape_picks_the_stacks_wave() {
    let open = [(CUTOFF, 20_000.0), (KEYTRACK, 0.0), (SUSTAIN, 1.0), (RESONANCE, 0.0)];
    let wave = |shape: f32| held(&patch(&[&open[..], &[(SHAPE, shape)]].concat()), 57.0, 1.0).left()[s(0.2)..].to_vec();
    let [saw, square, triangle, sine] = [0.0, 1.0, 2.0, 3.0].map(wave);
    assert_eq!(saw, held(&patch(&open), 57.0, 1.0).left()[s(0.2)..].to_vec(), "the saw is the default");
    let f0 = hz(57.0) as f64;
    // Each harmonic's power against the fundamental's.
    let ratio = |x: &[f32], k: f64| band(x, k * f0) / band(x, f0);
    for (name, x) in [("saw", &saw), ("square", &square), ("triangle", &triangle), ("sine", &sine)] {
        let f = peak_hz(x, SR, 100.0, 400.0);
        assert!((f / f0 - 1.0).abs() < 0.005, "{name} plays at {f:.1} Hz");
    }
    assert!(ratio(&saw, 2.0) > 0.1, "a saw has its second harmonic: {}", ratio(&saw, 2.0));
    assert!(
        ratio(&square, 2.0) < 1e-3 && ratio(&square, 3.0) > 0.05,
        "a square has odd harmonics only: {} {}",
        ratio(&square, 2.0),
        ratio(&square, 3.0)
    );
    assert!(
        ratio(&triangle, 3.0) < ratio(&saw, 3.0) / 4.0,
        "a triangle's fall faster: {} against {}",
        ratio(&triangle, 3.0),
        ratio(&saw, 3.0)
    );
    assert!(ratio(&sine, 2.0) < 1e-4 && ratio(&sine, 3.0) < 1e-4, "a sine has almost none: {} {}", ratio(&sine, 2.0), ratio(&sine, 3.0));
    // A change while the note plays glides: the biggest step is no bigger than the saw's own.
    let base = patch(&open);
    let changing = play::<Synth>(s(1.0), BLOCK, &|p, _c, t| match p {
        PITCH => 57.0,
        GATE => 1.0,
        SHAPE => [0.0, 3.0][(t >= s(0.5)) as usize],
        _ => base[p],
    });
    let x = changing.left();
    assert!(max_step(&x, s(0.49), s(0.52)) <= 1.2 * max_step(&saw, 0, s(0.3)), "a change of shape doesn't click");
}
