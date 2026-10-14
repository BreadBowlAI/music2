//! The FM voice: what every voice guarantees, then brightness, key scaling and aliasing.

use super::*;
use crate::testkit::*;

const NAMES: [&str; INPUTS] = [
    "pitch",
    "gate",
    "velocity",
    "ratio_a",
    "mod_a",
    "index_a",
    "fall_a",
    "ratio_b",
    "mod_b",
    "index_b",
    "fall_b",
    "mix_b",
    "feedback",
    "detune",
    "tone",
    "attack",
    "decay",
    "sustain",
    "release",
    "vel_amount",
    "level",
    "drop",
    "drop_time",
];

fn patch(changes: &[(usize, f32)]) -> Vec<f32> {
    let mut p = defaults("fm", &NAMES);
    for &(port, v) in changes {
        p[port] = v;
    }
    p
}

/// std/fm's electric piano and bell, as plan values.
fn keys() -> Vec<f32> {
    patch(&[
        (INDEX_A, 1.8),
        (FALL_A, 1.2),
        (MOD_B, 14.0),
        (INDEX_B, 0.9),
        (FALL_B, 0.07),
        (MIX_B, 0.35),
        (DETUNE, 3.0),
        (VEL_AMOUNT, 0.8),
        (DECAY, 3.5),
        (RELEASE, 0.35),
    ])
}

fn bell() -> Vec<f32> {
    patch(&[
        (MOD_A, 3.5),
        (INDEX_A, 2.0),
        (FALL_A, 2.5),
        (RATIO_B, 3.0),
        (MOD_B, 1.0),
        (INDEX_B, 0.6),
        (FALL_B, 1.5),
        (MIX_B, 0.45),
        (DECAY, 4.0),
        (RELEASE, 0.6),
    ])
}

/// Notes `(pitch, velocity, start, end)`, played together (as `poly` would, summed).
fn fm(patch: &[f32], notes: &[(f32, f32, usize, usize)], frames: usize) -> Vec<f32> {
    let mut sum = vec![0.0; frames];
    for &(pitch, velocity, on, off) in notes {
        let input = |p: usize, _c: usize, t: usize| match p {
            PITCH => pitch,
            VELOCITY => velocity,
            GATE => (t >= on && t < off) as i32 as f32,
            _ => patch[p],
        };
        for (s, x) in sum.iter_mut().zip(play::<Fm>(frames, BLOCK, &input).left()) {
            *s += x;
        }
    }
    sum
}

#[test]
fn silence_until_a_note() {
    let input = |p: usize, _c: usize, _t: usize| if p == GATE { 0.0 } else { patch(&[])[p] };
    let out = play::<Fm>(s(0.5), BLOCK, &input);
    assert!(out.left().iter().all(|&v| v == 0.0) && out.active.iter().all(|a| !a));
}

#[test]
fn tails_end_in_exact_silence() {
    for release in [0.003, 0.4, 2.0] {
        let p = patch(&[(RELEASE, release), (SUSTAIN, 1.0)]);
        let off = s(0.3);
        let input = |port: usize, _c: usize, t: usize| match port {
            PITCH => 60.0,
            GATE => (t < off) as i32 as f32,
            _ => p[port],
        };
        let out = play::<Fm>(off + s(release * 1.5 + 0.5), BLOCK, &input);
        let x = out.left();
        let limit = off + s(release * 1.5 + 0.02);
        assert!(last_sound(&x).unwrap() < limit, "release {release}");
        assert!(!out.active[limit] && out.active[off + 1]);
        assert!(x.iter().all(|&v| v == 0.0 || v.abs() >= f32::MIN_POSITIVE));
    }
}

#[test]
fn extreme_settings_never_make_nan() {
    for set in torture(&keys(), &[PITCH, GATE]) {
        for pitch in [0.0, 60.0, 127.0, f32::NAN] {
            let input = |p: usize, _c: usize, t: usize| match p {
                GATE => ((t / 5) % 2) as f32,
                PITCH => pitch,
                _ => set[p],
            };
            let x = play::<Fm>(2_000, BLOCK, &input).left();
            let bad = x.iter().position(|v| !v.is_finite() || v.abs() > 4.0);
            assert_eq!(bad, None, "{set:?} at {pitch}: {:?}", bad.map(|b| x[b]));
        }
    }
}

#[test]
fn the_same_whatever_the_block_size() {
    let p = keys();
    let input = |port: usize, _c: usize, t: usize| match port {
        PITCH => 60.0 + ((t / 7_000) % 3) as f32 * 5.0,
        GATE => (t % 7_000 > 10 && t % 7_000 < 5_000) as i32 as f32,
        TONE => 0.3 + 0.2 * ((t / 1_100) % 3) as f32,
        _ => p[port],
    };
    let reference = play::<Fm>(21_000, BLOCK, &input).left();
    for split in [1, 7, 64] {
        assert!(play::<Fm>(21_000, split, &input).left() == reference, "blocks of {split}");
    }
}

#[test]
fn a_reset_starts_over() {
    let mut p = keys();
    p[GATE] = 1.0;
    p[PITCH] = 64.0;
    p[FEEDBACK] = 0.5;
    assert_reset_starts_over::<Fm>(&p, 5_000);
}

#[test]
fn carriers_are_in_tune() {
    // A carrier alone (no index), from the bottom of the keyboard to the top: within 0.5 cents.
    for pitch in [24.0, 45.0, 69.0, 93.0] {
        let p = patch(&[(INDEX_A, 0.0), (MIX_B, 0.0), (SUSTAIN, 1.0)]);
        let x = fm(&p, &[(pitch, 0.8, 0, s(1.5))], s(1.5));
        let want = hz(pitch) as f64;
        let f = peak_hz(&x[s(0.2)..], SR, want * 0.8, want * 1.2);
        assert!((1200.0 * (f / want).log2()).abs() < 0.5, "{pitch}: {f:.3} Hz");
    }
}

#[test]
fn no_dc_at_any_setting() {
    for changes in [
        vec![],
        vec![(FEEDBACK, 1.0)],
        vec![(INDEX_A, 8.0), (INDEX_B, 8.0), (FEEDBACK, 1.0), (TONE, 1.0)],
        vec![(RATIO_A, 0.5), (MOD_A, 1.0)],
    ] {
        let mut p = keys();
        for &(port, v) in &changes {
            p[port] = v;
        }
        p[SUSTAIN] = 1.0;
        let x = fm(&p, &[(45.0, 1.0, 0, s(2.0))], s(2.0));
        let m = dc(&x[s(1.0)..]);
        assert!(m.abs() <= 1e-4, "{changes:?}: {m:e}");
    }
}

#[test]
fn levels_suit_a_mix() {
    let one = db(peak(&fm(&patch(&[]), &[(60.0, 0.8, 0, s(1.0))], s(1.0))));
    assert!((-16.0..=-12.0).contains(&one), "one default note peaks at {one:.1} dBFS");
    for (name, p) in [("keys", keys()), ("bell", bell())] {
        let chord = fm(&p, &[(57.0, 0.8, 0, s(1.0)), (60.0, 0.8, 0, s(1.0)), (64.0, 0.8, 0, s(1.0)), (67.0, 0.8, 0, s(1.0))], s(1.5));
        let pk = db(peak(&chord));
        println!("{name}: a 4-note chord peaks at {pk:.1} dBFS");
        assert!((-12.0..=-4.0).contains(&pk), "{name}: a chord peaks at {pk:.1} dBFS");
    }
}

#[test]
fn a_chords_notes_dont_start_on_a_peak_together() {
    // Each note starts its operators a pitch-set fraction of a period in, so eight struck at
    // once add up like unrelated sounds (about +9 dB of level), not like one wave (+18).
    for (name, p) in [("keys", keys()), ("bell", bell())] {
        let voicing = [48.0, 55.0, 60.0, 64.0, 67.0, 71.0, 74.0, 79.0];
        let one = peak(&fm(&p, &[(48.0, 1.0, 0, s(1.0))], s(1.0)));
        let notes: Vec<_> = voicing.iter().map(|&n| (n, 1.0, 0, s(1.0))).collect();
        let eight = peak(&fm(&p, &notes, s(1.0)));
        let growth = db(eight) - db(one);
        println!("{name}: 8 notes peak {growth:.1} dB above one");
        assert!(growth < 14.0, "{name}: 8 notes peak {growth:.1} dB above one");
    }
}

#[test]
fn velocity_0_is_silent_and_the_keys_are_dynamic() {
    let level = |p: &[f32], v: f32| peak(&fm(p, &[(60.0, v, 0, s(0.5))], s(0.5)));
    assert_eq!(level(&keys(), 0.0), 0.0);
    let range = db(level(&keys(), 1.0)) - db(level(&keys(), 0.3));
    assert!(range > 14.0, "keys: 0.3 against 1 is {range:.1} dB");
}

#[test]
fn a_legato_note_brings_its_own_velocity() {
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
    let x = play::<Fm>(s(1.0), BLOCK, &input).left();
    let fresh = fm(&p, &[(60.0, 1.0, 0, s(1.0))], s(1.0));
    let (soft, legato) = (rms(&x[s(0.3)..s(0.5)]), rms(&x[s(0.7)..s(0.9)]));
    assert!(db(legato) - db(soft) > 8.0, "legato {:.1} dB, soft {:.1}", db(legato), db(soft));
    // As loud as a struck note at its velocity, give or take the brightness it started with.
    assert!((db(legato) - db(rms(&fresh[s(0.7)..s(0.9)]))).abs() < 1.5);
}

#[test]
fn the_drop_bends_each_note_down_into_its_pitch() {
    let p = patch(&[(MIX_B, 0.0), (INDEX_A, 0.0), (SUSTAIN, 1.0), (DROP, 12.0), (DROP_TIME, 0.05)]);
    let x = fm(&p, &[(57.0, 0.8, 0, s(1.0))], s(1.0));
    let start = peak_hz(&x[..1_024], SR, 50.0, 2_000.0);
    let settled = peak_hz(&x[s(0.5)..], SR, 50.0, 2_000.0);
    assert!(start > 1.3 * 220.0, "the first 21 ms average {start:.1} Hz");
    assert!((settled / 220.0 - 1.0).abs() < 0.005, "settles at {settled:.1} Hz");
}

#[test]
fn velocity_and_tone_brighten() {
    let bright = |p: &[f32], v: f32| centroid(&fm(p, &[(60.0, v, 0, s(0.5))], s(0.5))[..s(0.2)], SR);
    let k = keys();
    let (soft, hard) = (bright(&k, 0.3), bright(&k, 1.0));
    assert!(hard > 2.0 * soft, "keys: centroid {soft:.0} Hz at 0.3, {hard:.0} at 1.0");
    let mut last = 0.0;
    for v in [0.2, 0.5, 0.8, 1.0] {
        let x = fm(&k, &[(60.0, v, 0, s(0.5))], s(0.5));
        let now = rms(&x[..s(0.2)]);
        assert!(now > last, "velocity {v}: {now} after {last}");
        last = now;
    }
    let mut dull = k.clone();
    dull[TONE] = 0.0;
    let mut sharp = k.clone();
    sharp[TONE] = 1.0;
    assert!(bright(&sharp, 0.8) > bright(&k, 0.8) && bright(&k, 0.8) > bright(&dull, 0.8));
}

#[test]
fn higher_notes_decay_sooner() {
    let length = |pitch: f32| {
        let x = fm(&keys(), &[(pitch, 0.8, 0, s(6.0))], s(6.0));
        let p = peak(&x);
        (0..x.len()).rfind(|&i| x[i].abs() > p * 0.01).unwrap()
    };
    assert!(length(84.0) < length(60.0) && length(60.0) < length(36.0));
}

#[test]
fn keys_and_bells_dont_alias_even_bright_and_high() {
    // At 2×, with the index held where its sidebands fold into the decimator's stopband.
    for (name, p, base) in [("keys", keys(), 1.0), ("bell", bell(), 0.5)] {
        for (tone, pitch, limit) in [(0.5, 84.0, -80.0), (0.5, 96.0, -75.0), (1.0, 91.0, -75.0), (1.0, 96.0, -75.0)] {
            let mut p = p.clone();
            p[TONE] = tone;
            p[SUSTAIN] = 1.0;
            p[FALL_A] = 60.0;
            p[FALL_B] = 60.0;
            p[DETUNE] = 0.0;
            let x = fm(&p, &[(pitch, 1.0, 0, s(2.0))], s(2.0));
            let a = alias_db(&x[s(0.5)..], hz(pitch) * base, SR);
            println!("{name}, tone {tone}, at {pitch}: {a:.1} dB");
            assert!(a <= limit, "{name}, tone {tone}, at {pitch}: {a:.1} dB");
        }
    }
    // The taper holds even at the top of every index, from C6 to C7, and on a modulator at 7×.
    for (ratio, pitch) in [(1.0, 84.0), (1.0, 96.0), (7.0, 84.0), (7.0, 91.0)] {
        let mut p = keys();
        for (port, v) in
            [(MOD_A, ratio), (INDEX_A, 8.0), (INDEX_B, 8.0), (TONE, 1.0), (SUSTAIN, 1.0), (FALL_A, 60.0), (FALL_B, 60.0), (DETUNE, 0.0)]
        {
            p[port] = v;
        }
        let x = fm(&p, &[(pitch, 1.0, 0, s(2.0))], s(2.0));
        let a = alias_db(&x[s(0.5)..], hz(pitch), SR);
        println!("index 8, modulator {ratio}x, at {pitch}: {a:.1} dB");
        assert!(a <= -65.0, "index 8, modulator {ratio}x, at {pitch}: {a:.1} dB");
    }
}

#[test]
fn feedback_brightens_the_first_pair() {
    // The share of power above 1 kHz.
    let bright = |fb: f32| {
        let p = patch(&[(FEEDBACK, fb), (MIX_B, 0.0), (SUSTAIN, 1.0)]);
        let spec = spectrum(&fm(&p, &[(48.0, 0.8, 0, s(1.0))], s(1.0))[s(0.2)..], SR);
        spec.iter().filter(|s| s.0 > 1_000.0).map(|s| s.1).sum::<f64>() / spec.iter().map(|s| s.1).sum::<f64>()
    };
    assert!(bright(1.0) > 3.0 * bright(0.0), "{} vs {}", bright(1.0), bright(0.0));
}
