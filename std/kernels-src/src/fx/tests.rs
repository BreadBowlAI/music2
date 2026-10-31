//! Native tests for std's effects: first what every effect guarantees (m2-dsp's harness, shared
//! with the effects of music2's own packs), then what each one is for.

use m2_dsp::fx::{Phase, clampf};
use m2_dsp::testing::effects::{self, BLOCK, Fx, Input, SR, db, noise, rms, run, saw, with};

use super::*;

/// Every effect at the settings the effect menu starts it with (the EQ gets some boost and
/// cut, since flat it does nothing), and playing: the last of chorus's is `playing`, 1. Delay,
/// reverb, phaser, tremolo, widen and the bitcrusher are `@m2/effects`' own
/// (`packs/@m2/effects/kernels-src`), tested there by the same harness.
const ALL: [Fx; 3] = [
    Fx { name: "chorus", run: run::<Chorus>, controls: &[0.8, 0.5, 0.5, 1.0] },
    Fx { name: "compress", run: run::<Compress>, controls: &[-18.0, 3.0, 0.01, 0.15, 4.0] },
    Fx { name: "eq3", run: run::<Eq3>, controls: &[4.0, -3.0, 5.0, 200.0, 1000.0, 4000.0] },
];

// ---------------------------------------------------------------- every effect

#[test]
fn silence_in_silence_out() {
    effects::silence_in_silence_out(&ALL);
}

#[test]
fn tails_end_in_exact_silence() {
    effects::tails_end_in_exact_silence(&ALL);
}

#[test]
fn extreme_settings_and_broken_input_never_make_nan() {
    effects::extreme_settings_and_broken_input_never_make_nan(&ALL);
}

#[test]
fn output_stays_bounded() {
    effects::output_stays_bounded(&ALL);
}

#[test]
fn deterministic_whatever_the_block_size() {
    effects::deterministic_whatever_the_block_size(&ALL);
}

#[test]
fn mono_stays_mono() {
    effects::mono_stays_mono(&ALL);
}

#[test]
fn every_effect_changes_the_sound() {
    effects::every_effect_changes_the_sound(&ALL);
}

// ---------------------------------------------------------------- each effect

/// Gain in dB of a mono sine at `freq` through `fx`, once settled (by RMS: sampled peaks
/// of high frequencies miss the true peak).
fn sine_gain(fx: fn(usize, usize, usize, Input<'_>) -> Vec<[f32; 2]>, controls: &[f32], freq: f32, amp: f32) -> f32 {
    let sine = |_, t: usize| amp * (TAU * (t as f64 * freq as f64 / SR as f64).fract() as f32).sin();
    let out = fx(1, 48_000, BLOCK, &with(&sine, controls));
    db(rms(&out[24_000..], 0) / rms(&(24_000..48_000).map(|t| [sine(0, t), 0.0]).collect::<Vec<_>>(), 0))
}

#[test]
fn chorus_spreads_a_centred_sound_at_the_same_level() {
    let mono = |_, t| 0.5 * saw(0, t);
    let out = (ALL[0].run)(2, 48_000, BLOCK, &with(&mono, ALL[0].controls));
    let width = out.iter().map(|f| (f[0] - f[1]).abs()).fold(0.0, f32::max);
    assert!(width > 0.05, "left and right only differ by {width}");
    let dry = rms(&(0..48_000).map(|t| [mono(0, t), 0.0]).collect::<Vec<_>>(), 0);
    let level = db(rms(&out, 0) / dry);
    assert!(level.abs() < 3.0, "chorus changes the level by {level:.1} dB");
}

#[test]
fn compressor_follows_its_curve() {
    let fx = ALL[1].run;
    // threshold, ratio, attack, release, makeup
    let at = |ratio: f32, makeup: f32, amp: f32| sine_gain(fx, &[-20.0, ratio, 0.001, 0.05, makeup], 1_000.0, amp) + db(amp);
    let loud = at(4.0, 0.0, 1.0);
    assert!((loud + 15.0).abs() < 0.5, "0 dB in at 4:1 over -20 dB comes out at {loud:.2} dB, not -15");
    let quiet = at(4.0, 0.0, 0.01);
    assert!((quiet + 40.0).abs() < 0.05, "-40 dB in comes out at {quiet:.2} dB");
    let made_up = at(4.0, 6.0, 0.01);
    assert!((made_up + 34.0).abs() < 0.05, "makeup: {made_up:.2} dB");
    let off = at(1.0, 0.0, 1.0);
    assert!(off.abs() < 0.05, "ratio 1 still compresses: {off:.2} dB");
}

#[test]
fn eq3_is_transparent_when_flat_and_boosts_its_bands() {
    let fx = ALL[2].run;
    let flat = [0.0, 0.0, 0.0, 200.0, 1000.0, 4000.0];
    let out = fx(2, 12_000, BLOCK, &with(&noise, &flat));
    assert!(out.iter().enumerate().all(|(t, f)| f[0] == noise(0, t) && f[1] == noise(1, t)), "flat EQ changes the sound");
    let band = |k: usize, gain: f32, freq: f32| {
        let mut c = flat;
        c[k] = gain;
        sine_gain(fx, &c, freq, 0.1)
    };
    for (k, gain, freq, want) in [
        (0, 12.0, 40.0, 12.0),
        (0, 12.0, 8_000.0, 0.0),
        (0, -12.0, 40.0, -12.0),
        (1, 12.0, 1_000.0, 12.0),
        (1, 12.0, 30.0, 0.0),
        (1, -9.0, 1_000.0, -9.0),
        (1, -60.0, 1_000.0, -60.0),
        (2, 12.0, 16_000.0, 12.0),
        (2, 12.0, 100.0, 0.0),
    ] {
        let got = band(k, gain, freq);
        assert!((got - want).abs() < 0.6, "band {k} at {gain} dB: {freq} Hz comes out {got:.2} dB, want {want}");
    }
}

#[test]
fn pan_places_a_mono_sound_and_balances_a_stereo_one() {
    use std::f32::consts::{PI, SQRT_2};

    use crate::Pan;
    let pan = |channels: usize, at: f32| run::<Pan>(channels, 512, BLOCK, &with(&saw, &[at]));
    let x = |c: usize, t: usize| saw(c, t);
    // Mono: equal power, unity in the middle, all of it (+3 dB) at one side.
    let centre = pan(1, 0.0);
    assert!(centre.iter().enumerate().all(|(t, f)| (f[0] - x(0, t)).abs() < 1e-6 && (f[1] - x(0, t)).abs() < 1e-6));
    let left = pan(1, -1.0);
    assert!(left.iter().enumerate().all(|(t, f)| (f[0] - x(0, t) * SQRT_2).abs() < 1e-5 && f[1].abs() < 1e-6));
    // Stereo: the middle leaves both sides as they are; to one side, the other fades out.
    assert!(pan(2, 0.0).iter().enumerate().all(|(t, f)| f[0] == x(0, t) && f[1] == x(1, t)));
    let right = pan(2, 1.0);
    assert!(right.iter().enumerate().all(|(t, f)| f[0].abs() < 1e-6 && f[1] == x(1, t)));
    let half = pan(2, -0.5);
    assert!(half.iter().enumerate().all(|(t, f)| f[0] == x(0, t) && (f[1] - x(1, t) * (0.25 * PI).cos()).abs() < 1e-6));
}

// ---------------------------------------------------------------- following the song

/// The effects whose last input is `playing`.
const FOLLOW: [&str; 1] = ["chorus"];

/// The LFO phase before it followed the song, kept here as the oracle: what [`Phase`] must still
/// do while the song plays.
fn old_phase(at: &mut f32, sr: f32, rate: f32, max: f32) -> f32 {
    let now = *at;
    *at += clampf(rate, 0.0, max) / sr;
    if *at >= 1.0 {
        *at -= 1.0;
    }
    now
}

// While the song plays, the LFO steps exactly as it always did, at any rate: no render changes.
#[test]
fn playing_the_lfo_steps_as_it_always_did() {
    let rates = [0.8, 5.0, 19.9, 0.05, 0.0, -1.0, f32::NAN, f32::INFINITY, 1e9, 7.3];
    for (max, sr) in [(20.0, SR), (40.0, SR), (20.0, 44_100.0), (40.0, 384_000.0)] {
        let (mut phase, mut old) = (Phase { sr, at: 0.0 }, 0.0f32);
        for t in 0..400_000 {
            let rate = rates[t / 9_000 % rates.len()];
            let (now, was) = (phase.next(rate, max, 1.0), old_phase(&mut old, sr, rate, max));
            assert_eq!(now.to_bits(), was.to_bits(), "sample {t} at {rate} Hz, {sr} Hz");
        }
    }
}

// Stopped (`playing` 0, or NaN), each LFO waits where it is: played on, the effect is what it is
// with no stop at all, bit for bit. A fresh one that sat stopped plays as one made at Play.
#[test]
fn stopped_the_effects_wait_and_play_on_where_they_were() {
    effects::stopped_they_wait_and_play_on_where_they_were(ALL.iter().filter(|fx| FOLLOW.contains(&fx.name)));
}
