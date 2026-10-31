//! The voice building blocks against their specifications.

use std::f64::consts::TAU;

use super::*;
use crate::testkit::{SR, db, peak, peak_hz, rms};

#[test]
fn sine_is_within_2e7_everywhere() {
    let mut worst = 0.0f64;
    for i in -400_000..400_000 {
        let p = i as f32 / 100_000.0;
        worst = worst.max((sin2pi(p) as f64 - (TAU * p as f64).sin()).abs());
    }
    assert!(worst < 2.5e-7, "error {worst}");
    assert_eq!(sin2pi(0.0), 0.0);
    assert!((sin2pi(0.25) - 1.0).abs() < 1e-6 && (sin2pi(-0.25) + 1.0).abs() < 1e-6);
}

#[test]
fn soft_clip_is_smooth_and_bounded() {
    assert_eq!(soft(0.0), 0.0);
    assert_eq!(soft(3.0), 1.0);
    assert_eq!(soft(-50.0), -1.0);
    assert_eq!(soft(f32::NAN), -1.0);
    assert!((soft(0.01) - 0.01).abs() < 1e-6);
    // Slope 0 where it reaches ±1, so the knee doesn't click.
    assert!((soft(2.999) - 1.0).abs() < 1e-6);
}

#[test]
fn blep_corrections_make_a_centred_step() {
    for d in [0.0, 0.1, 0.5, 0.9, 0.999] {
        let r = blep4(d);
        // Summed, the corrections move the step's centre from the sample it was seen at to
        // where it really happened.
        assert!((r.iter().sum::<f32>() - (d - 0.5)).abs() < 1e-6, "d {d}: {r:?}");
        assert!(r[0] >= 0.0 && r[3] <= 0.0);
    }
    // Continuous at the ends: a step at the very end of a sample is one at the start of the next.
    let (a, b) = (blep4(0.0), blep4(1.0));
    // (The sample at the step itself differs by the step: one view counts it as after the step.)
    assert!((a[1] - b[0]).abs() < 1e-6 && (a[2] + 1.0 - b[1]).abs() < 1e-6 && (a[3] - b[2]).abs() < 1e-6 && b[3].abs() < 1e-6);
}

/// Gain of the halfband for a sine at `f` Hz into 2× SR.
fn halfband_gain(f: f64) -> f32 {
    let mut hb = Halfband::default();
    let n = 48_000;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = |k: usize| (TAU * f * k as f64 / (2.0 * SR as f64)).sin() as f32;
        out.push(hb.tick(x(2 * i), x(2 * i + 1)));
    }
    rms(&out[n / 2..]) / std::f32::consts::FRAC_1_SQRT_2
}

#[test]
fn halfband_is_flat_to_20k_and_stops_from_26k() {
    for f in [50.0, 1_000.0, 10_000.0, 18_000.0, 20_000.0] {
        let g = db(halfband_gain(f));
        assert!(g.abs() < 0.05, "{f} Hz passes at {g:.3} dB");
    }
    for f in [26_000.0, 30_000.0, 40_000.0, 47_000.0] {
        let g = db(halfband_gain(f));
        assert!(g < -80.0, "{f} Hz is only {g:.1} dB down");
    }
}

/// The ladder at `fc` Hz and feedback `k`, fed `input(t)`.
fn ladder(fc: f32, k: f32, n: usize, input: impl Fn(usize) -> f32) -> Vec<f32> {
    let c = LadderCoefs::new(one_pole(fc, SR), k);
    let mut l = Ladder::default();
    (0..n).map(|t| l.tick(input(t), &c)).collect()
}

#[test]
fn ladder_self_oscillates_at_its_cutoff() {
    for fc in [110.0, 440.0, 1_000.0, 3_000.0, 8_000.0] {
        // A tiny kick and nothing else: past k = 4 it rings up and holds.
        let y = ladder(fc, 4.6, 96_000, |t| if t == 0 { 1e-3 } else { 0.0 });
        let tail = &y[48_000..];
        let f = peak_hz(tail, SR, 20.0, 20_000.0);
        assert!((f / fc as f64 - 1.0).abs() < 0.02, "at {fc} Hz it sings at {f:.1} Hz");
        assert!(peak(tail) > 0.3 && peak(tail) < 2.0, "amplitude {}", peak(tail));
    }
    // Below 4 it rings and dies away.
    let y = ladder(1_000.0, 3.6, 48_000, |t| if t == 0 { 1.0 } else { 0.0 });
    assert!(peak(&y[24_000..]) < 1e-6);
}

#[test]
fn ladder_is_a_24db_lowpass() {
    let gain = |f: f64, fc: f32| {
        let y = ladder(fc, 0.0, 48_000, |t| 0.1 * (TAU * f * t as f64 / SR as f64).sin() as f32);
        db(rms(&y[24_000..]) / (0.1 * std::f32::consts::FRAC_1_SQRT_2))
    };
    assert!(gain(50.0, 1_000.0).abs() < 0.1);
    let octave_up = gain(4_000.0, 1_000.0) - gain(8_000.0, 1_000.0);
    assert!((octave_up - 24.0).abs() < 3.0, "{octave_up:.1} dB an octave");
    assert!((gain(1_000.0, 1_000.0) + 12.0).abs() < 0.5, "at the cutoff: {:.2} dB", gain(1_000.0, 1_000.0));
}

#[test]
fn ladder_survives_anything() {
    for &k in &[0.0, 4.0, 4.6, 1e9] {
        let k = clampf(k, 0.0, 4.6);
        let y = ladder(20_000.0, k, 4_800, |t| [1e4, -1e4, 0.0][t % 3]);
        assert!(y.iter().all(|v| v.is_finite() && v.abs() < 10.0), "k {k}");
    }
}

#[test]
fn envelope_times_and_curves() {
    let mut e = Env::new();
    e.set(0.01, 0.5, 0.5, 0.3, SR);
    e.on();
    let mut n = 0;
    while e.next() < 1.0 {
        n += 1;
    }
    assert!((n as f32 / SR - 0.01).abs() < 0.0002, "attack took {n} samples");
    // The attack is RC-shaped: past halfway by a third of its time.
    let mut e = Env::new();
    e.set(0.03, 0.5, 0.5, 0.3, SR);
    e.on();
    let v = (0..480).map(|_| e.next()).last().unwrap();
    assert!(v > 0.5, "{v} after a third of the attack");
    // Decay: -60 dB of the way to sustain in its time.
    for _ in 0..(1440 - 480 + (0.5 * SR) as usize) {
        e.next();
    }
    assert!((e.value - 0.5).abs() < 0.5 * 1e-3 * 1.2, "{}", e.value);
    // Release: -60 dB in its time, then exactly 0 at -90 dB, by 1.5 times it.
    e.off();
    let tail: Vec<f32> = (0..(0.5 * SR) as usize).map(|_| e.next()).collect();
    let at = |s: f32| tail[(s * SR) as usize];
    assert!((db(at(0.3) / 0.5) + 60.0).abs() < 0.5, "{} dB at the release time", db(at(0.3) / 0.5));
    let end = tail.iter().position(|&v| v == 0.0).unwrap();
    assert!(end as f32 / SR <= 0.45 && end as f32 / SR > 0.4, "ends at {}", end as f32 / SR);
    assert_eq!(e.stage, Stage::Idle);
}

#[test]
fn a_retrigger_carries_on_from_the_level_it_is_at() {
    let mut e = Env::new();
    e.set(0.005, 0.2, 0.6, 0.3, SR);
    e.on();
    let mut last = 0.0;
    for t in 0..48_000 {
        if t == 20_000 {
            e.off();
        }
        if t == 22_000 {
            e.on();
        }
        let v = e.next();
        assert!((v - last).abs() < 0.01, "jumps by {} at {t}", v - last);
        last = v;
    }
}

#[test]
fn a_decay_ends_in_exact_silence() {
    let mut d = Decay::new(0.2, SR);
    let v: Vec<f32> = (0..48_000).map(|_| d.next()).collect();
    assert_eq!(v[0], 1.0);
    assert!((db(v[(0.2 * SR) as usize]) + 60.0).abs() < 0.1);
    let end = v.iter().position(|&x| x == 0.0).unwrap();
    assert!((end as f32 / SR - 0.3).abs() < 0.002, "ends at {}", end as f32 / SR);
    assert!(v[end..].iter().all(|&x| x == 0.0));
}

#[test]
fn clip_is_continuous_through_its_knee_and_bounded() {
    let mut c = Clip::default();
    let mut last = 0.0;
    for i in 0..40_000 {
        let x = 3.0 * (TAU * i as f64 / 40_000.0).sin() as f32;
        let y = c.tick(x);
        assert!(y.abs() <= 1.0 + 1e-6 && (y - last).abs() < 0.01, "{x} → {y}");
        last = y;
    }
    // Small signals pass at 1.5× (the clip's slope at 0), half a sample late.
    let mut c = Clip::default();
    c.tick(0.01);
    assert!((c.tick(0.02) - 1.5 * 0.015).abs() < 1e-5);
}

#[test]
fn drive_keeps_the_level_within_3db() {
    let saw = |t: usize| 2.0 * (t as f32 * 220.0 / SR).fract() - 1.0;
    let clean = rms(&(0..48_000).map(saw).collect::<Vec<_>>());
    for d in [0.0, 6.0, 12.0, 18.0, 24.0] {
        let drive = Drive::new(d);
        let mut c = Clip::default();
        let y: Vec<f32> = (0..48_000).map(|t| c.tick(saw(t) * drive.into) * drive.out).collect();
        let change = db(rms(&y) / clean);
        assert!(change.abs() < 3.0, "{d} dB of drive changes the level by {change:.2} dB");
    }
}

#[test]
fn pan_is_std_pan() {
    assert!((pan(0.0)[0] - 1.0).abs() < 1e-6 && (pan(0.0)[1] - 1.0).abs() < 1e-6);
    assert!((pan(-1.0)[0] - SQRT_2).abs() < 1e-6 && pan(-1.0)[1].abs() < 1e-6);
    let p = pan(0.4);
    assert!((p[0] * p[0] + p[1] * p[1] - 2.0).abs() < 1e-5);
}

#[test]
fn noise_is_uniform_and_seeded() {
    let mut n = Noise::new(hash(1, 2));
    let x: Vec<f32> = (0..100_000).map(|_| n.next()).collect();
    assert!(x.iter().all(|v| (-1.0..1.0).contains(v)));
    assert!(crate::testkit::mean(&x).abs() < 0.01 && (rms(&x) - 0.577).abs() < 0.01);
    let mut m = Noise::new(hash(1, 2));
    assert!(x.iter().all(|&v| v == m.next()));
}
