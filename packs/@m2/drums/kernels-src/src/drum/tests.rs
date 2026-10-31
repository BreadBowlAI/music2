//! The drum voice: the General MIDI map, one-shot guarantees (an exact-zero start, an
//! exact-zero end, chokes), levels, and what each drum is made of.

use super::*;
use crate::testkit::*;

const NAMES: [&str; INPUTS] = [
    "pitch",
    "velocity",
    "hits",
    "count",
    "model",
    "kick_tune",
    "kick_decay",
    "punch",
    "snare_tune",
    "snap",
    "snare_decay",
    "clap_tone",
    "clap_decay",
    "hat_tone",
    "hat_decay",
    "open_decay",
    "tom_tune",
    "tom_decay",
    "rim_tone",
    "bell_tune",
    "cymbal_decay",
    "shake_decay",
    "drive",
    "kick_level",
    "snare_level",
    "hat_level",
    "tom_level",
    "perc_level",
];

fn patch(changes: &[(usize, f32)]) -> Vec<f32> {
    let mut p = defaults("drum", &NAMES);
    for &(port, v) in changes {
        p[port] = v;
    }
    p
}

/// One hit at `pitch` (the kit), with `hits` on the choke input.
fn hit(patch: &[f32], pitch: f32, velocity: f32, frames: usize, hits: &dyn Fn(usize) -> f32) -> Played {
    let input = |p: usize, _c: usize, t: usize| match p {
        PITCH => pitch,
        VELOCITY => velocity,
        HITS => hits(t),
        _ => patch[p],
    };
    play::<Drum>(frames, BLOCK, &input)
}

fn kit(pitch: f32) -> Vec<f32> {
    hit(&patch(&[]), pitch, 0.8, s(4.0), &|_| 0.0).left()
}

/// Every mapped General MIDI pitch: what it plays, and its peak window at velocity 0.8 (dBFS).
const KIT: [(f32, Kind, f32, f32); 26] = [
    (35.0, Kind::Kick, -8.0, -4.0),
    (36.0, Kind::Kick, -8.0, -4.0),
    (37.0, Kind::Rim, -15.0, -9.0),
    (38.0, Kind::Snare, -11.0, -7.0),
    (39.0, Kind::Clap, -11.0, -7.0),
    (40.0, Kind::Snare, -11.0, -7.0),
    (41.0, Kind::Tom, -11.0, -7.0),
    (42.0, Kind::ClosedHat, -19.0, -13.0),
    (43.0, Kind::Tom, -11.0, -7.0),
    (44.0, Kind::PedalHat, -23.0, -15.0),
    (45.0, Kind::Tom, -11.0, -7.0),
    (46.0, Kind::OpenHat, -19.0, -13.0),
    (47.0, Kind::Tom, -11.0, -7.0),
    (48.0, Kind::Tom, -11.0, -7.0),
    (49.0, Kind::Crash, -21.0, -15.0),
    (50.0, Kind::Tom, -11.0, -7.0),
    (51.0, Kind::Ride, -21.0, -15.0),
    (54.0, Kind::Tambourine, -23.0, -17.0),
    (56.0, Kind::Cowbell, -15.0, -9.0),
    (57.0, Kind::Crash, -21.0, -15.0),
    (59.0, Kind::Ride, -21.0, -15.0),
    (69.0, Kind::Shaker, -23.0, -17.0),
    (70.0, Kind::Shaker, -23.0, -17.0),
    (75.0, Kind::Claves, -15.0, -9.0),
    (0.0, Kind::Silent, 0.0, 0.0),
    (60.0, Kind::Silent, 0.0, 0.0),
];

#[test]
fn ports_match_the_manifest() {
    patch(&[]);
}

#[test]
fn levels() {
    for &(pitch, kind, _, _) in &KIT {
        let x = kit(pitch);
        let end = last_sound(&x).map_or(0.0, |e| e as f32 / SR);
        println!("{pitch:4} {kind:?}: peak {:6.1} dBFS, rms(50 ms) {:6.1}, ends {end:.3} s", db(peak(&x)), db(rms(&x[..s(0.05)])));
    }
}

#[test]
fn the_kit_follows_general_midi_at_mix_ready_levels() {
    for &(pitch, kind, lo, hi) in &KIT {
        assert_eq!(kind_of(-1.0, pitch).0, kind, "pitch {pitch}");
        let x = kit(pitch);
        if kind == Kind::Silent {
            assert!(x.iter().all(|&v| v == 0.0));
            continue;
        }
        let p = db(peak(&x));
        assert!((lo..=hi).contains(&p), "{kind:?} at {pitch} peaks at {p:.1} dBFS, not {lo}..{hi}");
    }
    // Every other pitch is silent.
    for pitch in 0..128 {
        if !KIT.iter().any(|k| k.0 == pitch as f32) {
            assert_eq!(kind_of(-1.0, pitch as f32).0, Kind::Silent, "pitch {pitch}");
        }
    }
    // Alone, a drum ignores pitch, except the hats (pedal, open) and the tom (chromatic).
    assert_eq!(kind_of(0.0, 80.0).0, Kind::Kick);
    assert_eq!(kind_of(3.0, 46.0).0, Kind::OpenHat);
    assert_eq!(kind_of(3.0, 60.0).0, Kind::ClosedHat);
    assert_eq!(kind_of(4.0, 57.0), (Kind::Tom, 12.0, false));
    assert_eq!(kind_of(9.0, 36.0).0, Kind::Silent);
    assert_eq!(kind_of(f32::NAN, 36.0).0, Kind::Kick);
}

/// The drum's decay setting, for the tail rule (a fixed one where it has none).
fn decay_of(kind: Kind) -> f32 {
    let p = patch(&[]);
    match kind {
        Kind::Kick => p[KICK_DECAY],
        Kind::Snare => p[SNARE_DECAY],
        Kind::Clap => 0.032 + p[CLAP_DECAY],
        Kind::ClosedHat => p[HAT_DECAY],
        Kind::PedalHat => 0.025,
        Kind::OpenHat => 0.015 + p[OPEN_DECAY],
        Kind::Crash => p[CYMBAL_DECAY],
        Kind::Ride => 0.7 * p[CYMBAL_DECAY],
        // The low tom rings longest: √2 the tom's decay.
        Kind::Tom => 1.42 * p[TOM_DECAY],
        Kind::Rim => 0.025,
        Kind::Claves => 0.06,
        Kind::Cowbell => 0.6,
        Kind::Shaker => p[SHAKE_DECAY],
        Kind::Tambourine => 2.5 * p[SHAKE_DECAY],
        Kind::Silent => 0.0,
    }
}

#[test]
fn hits_start_at_zero_and_end_in_exact_silence() {
    for &(pitch, kind, _, _) in &KIT {
        let out = hit(&patch(&[]), pitch, 1.0, s(5.0), &|_| 0.0);
        let x = out.left();
        assert_eq!(x[0], 0.0, "{kind:?} starts with a click");
        let end = last_sound(&x).map_or(0, |e| e + 1);
        let limit = s(decay_of(kind) * 1.5 + 0.02);
        assert!(end <= limit, "{kind:?} sounds until {:.3} s, past {:.3}", end as f32 / SR, limit as f32 / SR);
        if kind != Kind::Silent {
            assert!(out.active[0], "{kind:?} isn't active while it sounds");
        }
        assert!(!out.active[limit + BLOCK], "{kind:?} is still active");
        assert!(x.iter().all(|&v| v == 0.0 || v.abs() >= f32::MIN_POSITIVE));
    }
}

#[test]
fn extreme_settings_never_make_nan() {
    for &pitch in &[36.0, 38.0, 39.0, 42.0, 46.0, 45.0, 49.0, 51.0, 37.0, 75.0, 56.0, 54.0, 69.0, 0.0, 127.0, f32::NAN] {
        for (n, set) in torture(&patch(&[(DRIVE, 12.0)]), &[PITCH, HITS]).iter().enumerate() {
            let input = |p: usize, _c: usize, t: usize| match p {
                PITCH => pitch,
                HITS => [0.0, 1.0, f32::NAN, 4096.0, -1e30, 0.5][t % 6],
                _ => set[p],
            };
            let out = play::<Drum>(1_500, BLOCK, &input);
            let bad = out.main().iter().position(|f| !f[0].is_finite() || f[0].abs() > 4.0);
            assert_eq!(bad, None, "pitch {pitch}, set {n} {set:?}");
        }
    }
}

#[test]
fn the_same_whatever_the_block_size() {
    for &(pitch, ..) in &KIT {
        let p = patch(&[(DRIVE, 6.0)]);
        let hits = |t: usize| if t == 9_001 { 4096.0 } else { 0.0 };
        let input = |port: usize, _c: usize, t: usize| match port {
            PITCH => pitch,
            HITS => hits(t),
            VELOCITY => 0.9,
            _ => p[port],
        };
        let reference = play::<Drum>(20_000, BLOCK, &input).out.remove(0);
        for split in [1, 7, 64] {
            let out = play::<Drum>(20_000, split, &input).out.remove(0);
            assert!(out == reference, "pitch {pitch} in blocks of {split}");
        }
    }
}

/// Peak of `x` from `from` for `len` samples.
fn peak_in(x: &[f32], from: usize, len: usize) -> f32 {
    peak(&x[from..(from + len).min(x.len())])
}

#[test]
fn a_closed_hat_chokes_an_open_one() {
    let at = s(0.1);
    let x = hit(&patch(&[]), 46.0, 0.8, s(1.0), &|t| if t == at { 4096.0 } else { 0.0 }).left();
    let before = peak_in(&x, at - s(0.01), s(0.01));
    let after = peak_in(&x, at + s(0.006), s(0.5));
    assert!(before > 0.01 && after == 0.0, "{before} before, {after} after");
    // Smoothly: no step bigger than the hat's own.
    assert!(max_step(&x, at, at + s(0.006)) <= 1.5 * max_step(&x, at - s(0.02), at));
}

#[test]
fn chokes_stay_in_their_group() {
    let at = s(0.2);
    let ringing = |pitch: f32, hits: f32| {
        peak_in(&hit(&patch(&[]), pitch, 0.8, s(0.5), &|t| if t == at { hits } else { 0.0 }).left(), at + s(0.01), s(0.05))
    };
    let free = |pitch: f32| ringing(pitch, 0.0);
    // A kick chokes a kick (two at once count 2 in its field), quickly.
    assert_eq!(ringing(36.0, 1.0), 0.0);
    assert_eq!(ringing(36.0, 2.0), 0.0);
    // Two kicks, a clap, the hats and a crash don't choke a snare.
    assert_eq!(ringing(38.0, 2.0 + 256.0 + 4096.0 + 65_536.0), free(38.0));
    assert!(free(38.0) > 0.0);
    assert_eq!(ringing(38.0, 16.0), 0.0);
    // Toms, cowbells and rides ring through anything.
    for pitch in [45.0, 56.0, 51.0] {
        assert_eq!(ringing(pitch, 1_048_575.0), free(pitch), "pitch {pitch}");
    }
    // The field that marks every hit (for hitcount) chokes nothing: a tom's hit doesn't cut
    // a kick, and a kick's hit (its field and its group's) still does.
    let h = m2_dsp::gm::HIT_FIELD as f32;
    assert_eq!(ringing(36.0, h), free(36.0));
    assert_eq!(ringing(36.0, 8.0 * h), free(36.0));
    assert_eq!(ringing(36.0, h + 1.0), 0.0);
    assert_eq!(ringing(46.0, 2.0 * h + 4096.0), 0.0);
    // A hit on the drum's own first sample is its own, not a choke.
    let x = hit(&patch(&[]), 36.0, 0.8, s(0.3), &|t| if t == 0 { 1.0 } else { 0.0 }).left();
    assert!(peak(&x[s(0.1)..]) > 0.01);
}

#[test]
fn clap_bursts_fall_where_they_should() {
    let x = kit(39.0);
    let level = |ms: f32| rms(&x[s(ms * 0.001)..s((ms + 0.5) * 0.001)]);
    for onset in [10.0, 21.0, 31.0] {
        let (before, after) = (level(onset - 1.0), level(onset + 0.5));
        assert!(after > 4.0 * before, "no burst at {onset} ms: {before} then {after}");
    }
    assert!(level(0.5) > 4.0 * level(8.5));
}

#[test]
fn the_kick_lands_on_its_tune_and_decays_in_its_time() {
    for (tune, decay) in [(48.0, 0.45), (40.0, 1.2), (60.0, 0.3)] {
        let x = hit(&patch(&[(KICK_TUNE, tune), (KICK_DECAY, decay)]), 36.0, 0.8, s(3.0), &|_| 0.0).left();
        let f = peak_hz(&x[s(0.15)..s(0.15) + 16_384], SR, 20.0, 200.0);
        assert!((f / tune as f64 - 1.0).abs() < 0.03, "tune {tune}: the tail is at {f:.1} Hz");
        // T60: the time from the peak to 60 dB below it.
        let p = peak(&x);
        let t60 = (0..x.len()).rfind(|&i| x[i].abs() > p * 1e-3).unwrap() as f32 / SR;
        assert!((t60 / decay - 1.0).abs() < 0.15, "decay {decay}: T60 {t60:.3}");
    }
    // Punch starts it higher: its first half cycle is shorter.
    let first_half = |punch: f32| {
        let x = hit(&patch(&[(PUNCH, punch)]), 36.0, 0.8, s(0.5), &|_| 0.0).left();
        (2..x.len()).find(|&i| x[i - 1] > 0.0 && x[i] <= 0.0).unwrap()
    };
    let (a, b, c) = (first_half(0.0), first_half(0.5), first_half(1.0));
    assert!(a > b && b > c, "{a} {b} {c}");
}

#[test]
fn the_snare_is_tuned_and_snap_brings_the_noise() {
    let x = hit(&patch(&[(SNAP, 0.0)]), 38.0, 0.8, s(0.5), &|_| 0.0).left();
    let f = peak_hz(&x[..s(0.2)], SR, 100.0, 300.0);
    assert!((f / 190.0 - 1.0).abs() < 0.05, "body at {f:.1} Hz");
    let noise_share = |snap: f32| {
        let x = hit(&patch(&[(SNAP, snap)]), 38.0, 0.8, s(0.5), &|_| 0.0).left();
        let spec = spectrum(&x[..16_384], SR);
        spec.iter().filter(|s| s.0 > 1_000.0).map(|s| s.1).sum::<f64>() / spec.iter().map(|s| s.1).sum::<f64>()
    };
    let shares: Vec<f64> = [0.0, 0.3, 0.6, 1.0].iter().map(|&s| noise_share(s)).collect();
    assert!(shares.windows(2).all(|w| w[1] > w[0]), "{shares:?}");
}

/// The share of `x`'s power above `f` Hz.
fn above(x: &[f32], f: f64) -> f64 {
    let spec = spectrum(x, SR);
    spec.iter().filter(|s| s.0 > f).map(|s| s.1).sum::<f64>() / spec.iter().map(|s| s.1).sum::<f64>()
}

#[test]
fn hats_and_cymbals_are_bright_and_metallic() {
    for pitch in [42.0, 44.0, 46.0, 49.0, 51.0] {
        let x = kit(pitch);
        let share = above(&x[..16_384.min(x.len())], 5_000.0);
        let want = if pitch < 47.0 { 0.8 } else { 0.5 };
        assert!(share >= want, "pitch {pitch}: {:.0}% above 5 kHz", 100.0 * share);
    }
    // Harder hits are brighter.
    let bright = |v: f32| centroid(&hit(&patch(&[]), 42.0, v, s(0.2), &|_| 0.0).left(), SR);
    assert!(bright(1.0) > bright(0.3));
}

#[test]
fn the_cowbell_rings_at_its_two_notes() {
    let x = kit(56.0);
    let spec = spectrum(&x[..16_384], SR);
    let at = |f: f64| spec.iter().filter(|s| (s.0 - f).abs() < 15.0).map(|s| s.1).sum::<f64>();
    let between = at(670.0) + at(450.0) + at(1_000.0);
    assert!(at(540.0) > 30.0 * between && at(799.2) > 30.0 * between, "{} {} {}", at(540.0), at(799.2), between);
}

#[test]
fn toms_climb_the_general_midi_ladder() {
    let pitches = [41.0, 43.0, 45.0, 47.0, 48.0, 50.0];
    let f: Vec<f64> = pitches.iter().map(|&p| peak_hz(&kit(p)[s(0.05)..s(0.05) + 8_192], SR, 40.0, 600.0)).collect();
    for k in 0..pitches.len() {
        let want = 130.0 * ((pitches[k] - 45.0) * 1.5 / 12.0).exp2() as f64;
        assert!((f[k] / want - 1.0).abs() < 0.03, "GM {}: {:.1} Hz, want {want:.1}", pitches[k], f[k]);
    }
    // Alone, chromatic around A2.
    let x = hit(&patch(&[(MODEL, 4.0)]), 57.0, 0.8, s(0.5), &|_| 0.0).left();
    let f = peak_hz(&x[s(0.05)..s(0.05) + 8_192], SR, 40.0, 600.0);
    assert!((f / 260.0 - 1.0).abs() < 0.03, "{f}");
}

#[test]
fn a_reset_starts_over() {
    for pitch in [36.0, 38.0, 39.0, 46.0, 49.0, 45.0, 37.0, 56.0, 69.0] {
        let mut p = patch(&[(DRIVE, 6.0)]);
        p[PITCH] = pitch;
        assert_reset_starts_over::<Drum>(&p, 5_000);
    }
}

#[test]
fn velocity_sets_the_level() {
    for &(pitch, kind, ..) in KIT.iter().filter(|k| k.1 != Kind::Silent) {
        let level = |v: f32| peak(&hit(&patch(&[]), pitch, v, s(0.3), &|_| 0.0).left());
        let (soft, mid, hard) = (level(0.3), level(0.8), level(1.0));
        assert!(soft < mid && mid < hard, "{kind:?}: {soft} {mid} {hard}");
        let drop = db(soft / mid);
        assert!((-13.0..=-7.0).contains(&drop), "{kind:?}: velocity 0.3 is {drop:.1} dB from 0.8");
    }
}

#[test]
fn drive_crunches_without_getting_louder_than_full_scale() {
    for pitch in [36.0, 38.0, 42.0] {
        let clean = kit(pitch);
        let driven = hit(&patch(&[(DRIVE, 24.0)]), pitch, 1.0, s(1.0), &|_| 0.0).left();
        assert!(peak(&driven) < 1.0 && driven.iter().zip(&clean).any(|(a, b)| (a - b).abs() > 0.01), "pitch {pitch}");
    }
}

// A decaying sine that starts at phase 0 leaves an offset behind it: the kick, the snare and
// the toms are highpassed so a beat of them adds no DC to a mix.
#[test]
fn bodies_leave_no_offset() {
    for pitch in [35.0, 36.0, 38.0, 40.0, 41.0, 45.0, 50.0] {
        let x = kit(pitch);
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let mean = x.iter().map(|&v| v as f64).sum::<f64>() / x.len() as f64;
        // Without the highpass a kick leaves about 6e-4 of its peak here (over 4 s); with it,
        // what is left (under 1e-5) is the fade-in on its first samples.
        assert!(mean.abs() < 2e-5 * peak as f64, "{pitch}: mean {mean:e} for a peak of {peak}");
    }
}

/// One hit at `pitch` with `count` (from hitcount: the hits its lane has played) on the
/// count input, at `sr`.
fn counted(pitch: f32, count: f32, sr: f32, changes: &[(usize, f32)]) -> Vec<f32> {
    let p = patch(changes);
    let input = |port: usize, _c: usize, _t: usize| match port {
        PITCH => pitch,
        VELOCITY => 0.8,
        HITS => 0.0,
        COUNT => count,
        _ => p[port],
    };
    play_at::<Drum>(sr, 2, (sr * 0.5) as usize, BLOCK, &input).left()
}

#[test]
fn every_hit_is_its_own_and_every_render_the_same() {
    // The noise and the metal are seeded by the hit count: the next hit differs, and the same
    // count gives the same bits.
    let alike = |a: &[f32], b: &[f32]| {
        let n = s(0.03);
        let dot: f64 = a[..n].iter().zip(&b[..n]).map(|(x, y)| *x as f64 * *y as f64).sum();
        dot / (rms(&a[..n]) as f64 * rms(&b[..n]) as f64 * n as f64)
    };
    for pitch in [38.0, 39.0, 42.0, 46.0, 49.0, 69.0] {
        let (a, b) = (counted(pitch, 4.0, SR, &[]), counted(pitch, 5.0, SR, &[]));
        assert!(a == counted(pitch, 4.0, SR, &[]), "pitch {pitch}: the same count, the same hit");
        let c = alike(&a, &b);
        assert!(c < 0.9, "pitch {pitch}: one hit and the next correlate {c:.3}");
        assert!((db(rms(&a)) - db(rms(&b))).abs() < 1.5, "pitch {pitch}: and stay as loud");
    }
    // A kick is a kick: no noise in its body, so every hit is the same.
    let kick = counted(36.0, 4.0, SR, &[(PUNCH, 0.0)]);
    assert!(alike(&kick, &counted(36.0, 9.0, SR, &[(PUNCH, 0.0)])) > 0.999);
    // Any count makes a hit: huge, broken and negative ones too.
    for count in [16_777_215.0, 1e30, f32::NAN, f32::INFINITY, -3.0] {
        let x = counted(38.0, count, SR, &[]);
        assert!(x.iter().all(|v| v.is_finite()) && peak(&x) > 0.01, "count {count}");
    }
}

#[test]
fn levels_move_their_groups_only() {
    let pk = |pitch: f32, changes: &[(usize, f32)]| db(peak(&counted(pitch, 0.0, SR, changes)));
    for (pitch, level) in [
        (36.0, KICK_LEVEL),
        (38.0, SNARE_LEVEL),
        (39.0, SNARE_LEVEL),
        (37.0, SNARE_LEVEL),
        (42.0, HAT_LEVEL),
        (46.0, HAT_LEVEL),
        (45.0, TOM_LEVEL),
        (49.0, PERC_LEVEL),
        (56.0, PERC_LEVEL),
        (69.0, PERC_LEVEL),
    ] {
        let moved = pk(pitch, &[(level, -6.0)]) - pk(pitch, &[]);
        assert!((moved + 6.0).abs() < 0.01, "pitch {pitch}: {moved:.2} dB");
        for other in [KICK_LEVEL, SNARE_LEVEL, HAT_LEVEL, TOM_LEVEL, PERC_LEVEL].into_iter().filter(|&o| o != level) {
            assert_eq!(pk(pitch, &[(other, -6.0)]), pk(pitch, &[]), "pitch {pitch}: another group's level");
        }
    }
}

#[test]
fn noise_drums_are_as_loud_at_any_sample_rate() {
    // White noise is scaled so its level in hertz, what the filters pass, doesn't depend on
    // the rate. Measured below 20 kHz, the band both rates share; the brightest (a tambourine
    // at 11 kHz) keeps a little more top at 96 kHz, where its filter is further from Nyquist.
    for pitch in [38.0, 39.0, 69.0, 54.0] {
        let at = |sr: f32| {
            let x = counted(pitch, 0.0, sr, &[]);
            let spec = spectrum(&x[..(sr * 0.3) as usize], sr);
            // Parseval: the bins' power over n² is the mean square, both rates windowed alike.
            let n = (2 * spec.len()) as f64;
            10.0 * (spec.iter().filter(|b| b.0 < 20_000.0).map(|b| b.1).sum::<f64>() / (n * n)).log10()
        };
        let (a, b) = (at(48_000.0), at(96_000.0));
        assert!((a - b).abs() < 1.5, "pitch {pitch}: {a:.1} dB at 48 kHz, {b:.1} at 96");
    }
}
