//! The sampler voice: a file plays its own samples at its root and every second one an octave up,
//! a note picks its file by its range, a one-shot plays past the note's end, and a missing file is
//! silence. Each plays in blocks of an odd size, so a read across blocks is checked too.

use m2_kernel::native;

use super::*;
use crate::testkit::*;

const NAMES: [&str; INPUTS] = ["pitch", "gate", "velocity", "tune", "level", "attack", "release", "mode"];

/// A file of `n` frames that changes at every sample, so a sample played twice or skipped shows.
fn file(n: usize, seed: u32) -> Vec<f32> {
    let mut rng = Lcg(seed);
    (0..n).map(|_| (rng.next() as f32 / 32_768.0) - 1.0).collect()
}

/// The sampler with `assets` and `values`, at 48 kHz, playing `pitch` with the gate on for `held`
/// samples, over `frames`, at full velocity and the manifest's other defaults, `changes` aside.
fn play_note(assets: &[Asset], values: &[f64], pitch: f32, held: usize, frames: usize, changes: &[(usize, f32)]) -> Played {
    let mut patch = defaults("sampler", &NAMES);
    patch[VELOCITY] = 1.0;
    for &(port, v) in changes {
        patch[port] = v;
    }
    let cfg = Cfg { values: Vec::leak(values.to_vec()), assets: Vec::leak(assets.to_vec()), ..Cfg::new(SR, BLOCK, 2) };
    play_cfg::<Sampler>(cfg, frames, 37, &|p, _, t| match p {
        PITCH => pitch,
        GATE => (t < held) as u8 as f32,
        _ => patch[p],
    })
}

#[test]
fn at_its_root_a_sample_plays_its_own_samples() {
    let x = file(1_000, 1);
    let mono = native::asset(SR, vec![x.clone()]);
    let got = play_note(&[mono], &[], 60.0, 2_000, 2_000, &[]);
    for (t, f) in got.main().iter().enumerate() {
        let want = x.get(t).copied().unwrap_or(0.0);
        assert_eq!((f[0].to_bits(), f[1].to_bits()), (want.to_bits(), want.to_bits()), "sample {t}: {f:?} against {want}");
    }
    // A stereo file at root 48, played at 48: each channel its own.
    let (a, b) = (file(500, 2), file(500, 3));
    let stereo = native::asset(SR, vec![a.clone(), b.clone()]);
    let got = play_note(&[stereo], &[48.0], 48.0, 600, 600, &[]);
    assert!(
        got.main()[..500].iter().zip(a.iter().zip(&b)).all(|(f, (a, b))| f[0] == *a && f[1] == *b),
        "a stereo file plays both channels"
    );
    assert!(got.active[450] && !got.active[599], "active while the file plays, not after");
}

#[test]
fn an_octave_up_plays_every_second_sample() {
    let x = file(1_001, 4);
    let got = play_note(&[native::asset(SR, vec![x.clone()])], &[], 72.0, 2_000, 1_000, &[]);
    let left = got.left();
    for t in 0..=500 {
        assert_eq!(left[t].to_bits(), x[2 * t].to_bits(), "sample {t}");
    }
    assert!(left[501..].iter().all(|&v| v == 0.0), "it ends at the file's end");
    // The file's own rate counts: a file at 24 kHz at its root plays every sample twice as long.
    let slow = play_note(&[native::asset(SR / 2.0, vec![x.clone()])], &[], 72.0, 2_000, 1_000, &[]).left();
    assert!((0..1_000).all(|t| slow[t] == x[t]), "an octave up at half the rate: its own samples");
}

#[test]
fn a_note_picks_its_zone() {
    let (low, high) = (file(400, 5), file(400, 6));
    let zones = [native::asset(SR, vec![low.clone()]), native::asset(SR, vec![high.clone()])];
    // `low` from 0 to 37, root 36; `high` from 38 up, root 38.
    let values = [36.0, 0.0, 37.0, 38.0, 38.0, 127.0];
    let at = |pitch: f32| play_note(&zones, &values, pitch, 500, 500, &[]).left();
    assert!(at(36.0)[..400] == low[..], "36 plays its own zone at its root");
    assert!(at(38.0)[..400] == high[..], "38 plays the next zone at its root");
    let above = at(40.0);
    assert!(above[0] == high[0] && above[1] != high[1], "40 plays the high zone, sped up");
    assert!(at(30.0)[0] == low[0], "below every root, the lowest zone");
    // A note no range holds is silence.
    let gap = play_note(&zones, &[36.0, 36.0, 36.0, 38.0, 38.0, 38.0], 37.0, 500, 500, &[]);
    assert!(gap.left().iter().all(|&v| v == 0.0) && !gap.active.iter().any(|&a| a), "37 is in no zone");
}

#[test]
fn one_shot_plays_past_the_note_off() {
    let x = file(2_000, 7);
    let shot = play_note(&[native::asset(SR, vec![x.clone()])], &[], 60.0, 10, 3_000, &[]);
    assert!(shot.left()[..2_000] == x[..], "one-shot (the default) plays the whole file after a 10-sample note");
    assert!(shot.active[1_990] && !shot.active[2_999], "and is active until the file ends");
    // Gated, with no release: it stops when the note does.
    let gated = play_note(&[native::asset(SR, vec![x.clone()])], &[], 60.0, 10, 3_000, &[(MODE, 1.0), (RELEASE, 0.0)]).left();
    assert!(gated[..10] == x[..10] && gated[10..].iter().all(|&v| v == 0.0), "gated stops at the note off");
    // Gated, with a release: a straight fade to exact silence.
    let faded = play_note(&[native::asset(SR, vec![x.clone()])], &[], 60.0, 10, 3_000, &[(MODE, 1.0), (RELEASE, 0.01)]);
    let l = faded.left();
    assert!(l[10..480].iter().any(|&v| v != 0.0) && l[495..].iter().all(|&v| v == 0.0), "it fades out over 10 ms");
    assert!(!faded.active[2_999]);
    // An attack fades in from 0.
    let soft = play_note(&[native::asset(SR, vec![vec![1.0; 2_000]])], &[], 60.0, 3_000, 3_000, &[(ATTACK, 0.01)]).left();
    assert!(soft[0] == 0.0 && soft[240] > 0.4 && soft[240] < 0.6 && soft[600] == 1.0, "attack: {} {} {}", soft[0], soft[240], soft[600]);
}

#[test]
fn a_missing_sample_is_silence() {
    for assets in [vec![native::missing()], vec![]] {
        let got = play_note(&assets, &[], 60.0, 500, 1_000, &[]);
        assert!(got.left().iter().all(|&v| v == 0.0) && !got.active.iter().any(|&a| a), "{assets:?} is silent and never active");
    }
    // Missing, with a file after it: a note in both ranges plays the file.
    let x = file(100, 8);
    let got = play_note(&[native::missing(), native::asset(SR, vec![x.clone()])], &[], 60.0, 500, 200, &[]).left();
    assert!(got[..100] == x[..]);
}

#[test]
fn no_control_breaks_it() {
    let x = file(4_000, 9);
    let asset = native::asset(SR, vec![x]);
    let mut base = defaults("sampler", &NAMES);
    base[VELOCITY] = 1.0;
    for set in torture(&base, &[PITCH, GATE]).iter().chain(&[vec![f32::NAN; INPUTS]]) {
        let changes: Vec<(usize, f32)> = set.iter().copied().enumerate().collect();
        for pitch in [f32::NAN, -1e9, 0.0, 60.0, 127.0, 1e9] {
            let got = play_note(&[asset], &[], pitch, 300, 600, &changes);
            assert!(got.main().iter().all(|f| f[0].is_finite() && f[1].is_finite() && f[0].abs() <= 32.0), "{set:?} at {pitch}");
        }
    }
}
