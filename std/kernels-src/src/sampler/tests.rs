//! The sampler voice: a file plays its own samples at its root and every second one an octave up,
//! a note picks its file by its range, a one-shot plays past the note's end, and a missing file is
//! silence. Each plays in blocks of an odd size, so a read across blocks is checked too.
//!
//! Then `std:regions` (PKG-16): regions by key and velocity, layers summed, turns from the count,
//! release regions, loops held then played out, tune, volume, pan and offset, any block split, and
//! no value or control that breaks it.

use m2_kernel::native;

use super::*;
use crate::testkit::*;

const NAMES: [&str; 9] = ["pitch", "gate", "velocity", "tune", "level", "attack", "release", "mode", "veltrack"];
const REGION_NAMES: [&str; 10] = ["pitch", "gate", "velocity", "tune", "level", "attack", "release", "mode", "veltrack", "count"];

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
    play_cfg::<Sampler<false>>(cfg, frames, 37, &|p, _, t| match p {
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
    for set in torture(&base, &[PITCH, GATE]).iter().chain(&[vec![f32::NAN; NAMES.len()]]) {
        let changes: Vec<(usize, f32)> = set.iter().copied().enumerate().collect();
        for pitch in [f32::NAN, -1e9, 0.0, 60.0, 127.0, 1e9] {
            let got = play_note(&[asset], &[], pitch, 300, 600, &changes);
            assert!(got.main().iter().all(|f| f[0].is_finite() && f[1].is_finite() && f[0].abs() <= 32.0), "{set:?} at {pitch}");
        }
    }
}

// ---------------------------------------------------------------- velocity, both kernels

#[test]
fn the_velocity_control_sets_how_much_velocity_moves_the_level() {
    let x = file(500, 10);
    let asset = native::asset(SR, vec![x.clone()]);
    let at = |velocity: f32, veltrack: f32| play_note(&[asset], &[], 60.0, 600, 600, &[(VELOCITY, velocity), (VELTRACK, veltrack)]).left();
    // Its default, 1: the level follows velocity, as before it was a control (velocity × level).
    let full = at(0.3, 1.0);
    assert!((0..500).all(|t| full[t].to_bits() == (x[t] * 0.3).to_bits()), "veltrack 1 is velocity × level, to the bit");
    // 0: every velocity plays at the level.
    assert!(at(0.3, 0.0)[..500] == x[..], "veltrack 0 ignores velocity");
    // Between: 1 - v + v × velocity.
    let half = at(0.3, 0.5);
    assert!((0..500).all(|t| half[t] == x[t] * (0.5 + 0.5 * 0.3)), "veltrack 0.5 at velocity 0.3 is 0.65");
}

// ---------------------------------------------------------------- regions

/// One region of `std:regions`, as std/sampler lays it out (PKG-15's fields, defaults as there).
#[derive(Clone, Copy, Debug)]
struct R {
    file: f64,
    root: f64,
    keys: [f64; 2],
    velocity: [f64; 2],
    turn: [f64; 2],
    release: bool,
    looped: [f64; 2],
    tune: f64,
    volume: f64,
    pan: f64,
    offset: f64,
}

impl R {
    /// Asset `file`, on every note and velocity, at root 60.
    fn of(file: usize) -> R {
        R {
            file: file as f64,
            root: 60.0,
            keys: [0.0, 127.0],
            velocity: [1.0, 127.0],
            turn: [1.0, 1.0],
            release: false,
            looped: [-1.0, -1.0],
            tune: 0.0,
            volume: 0.0,
            pan: 0.0,
            offset: 0.0,
        }
    }

    fn values(&self) -> [f64; FIELDS] {
        let [k0, k1] = self.keys;
        let [v0, v1] = self.velocity;
        let [p, of] = self.turn;
        let [l0, l1] = self.looped;
        let trigger = if self.release { 1.0 } else { 0.0 };
        [self.file, self.root, k0, k1, v0, v1, p, of, trigger, l0, l1, self.tune, self.volume, self.pan, self.offset]
    }
}

/// `regions`' values: how many each region takes, then each region's.
fn layout(regions: &[R]) -> Vec<f64> {
    std::iter::once(FIELDS as f64).chain(regions.iter().flat_map(|r| r.values())).collect()
}

/// A note `std:regions` plays: its pitch, its velocity (0..1), the samples its gate is held, and
/// the count it starts with.
#[derive(Clone, Copy)]
struct Note {
    pitch: f32,
    velocity: f32,
    held: usize,
    count: f32,
}

const NOTE: Note = Note { pitch: 60.0, velocity: 1.0, held: 100_000, count: 1.0 };

/// `std:regions` with `assets` and `regions`, playing `note` over `frames` in blocks of `split`, at
/// the manifest's defaults but `changes`. Velocity moves no level (`veltrack` 0) unless a change
/// says so, so a region at its root plays its file's own samples.
fn play_regions(assets: &[Asset], regions: &[R], note: Note, frames: usize, split: usize, changes: &[(usize, f32)]) -> Played {
    play_layout(assets, &layout(regions), note, frames, split, changes)
}

fn play_layout(assets: &[Asset], values: &[f64], note: Note, frames: usize, split: usize, changes: &[(usize, f32)]) -> Played {
    let mut patch = defaults("regions", &REGION_NAMES);
    patch[VELOCITY] = note.velocity;
    patch[VELTRACK] = 0.0;
    patch[COUNT] = note.count;
    for &(port, v) in changes {
        patch[port] = v;
    }
    let cfg = Cfg { values: Vec::leak(values.to_vec()), assets: Vec::leak(assets.to_vec()), ..Cfg::new(SR, BLOCK, 2) };
    play_cfg::<Sampler<true>>(cfg, frames, split, &|p, _, t| match p {
        PITCH => note.pitch,
        GATE => (t < note.held) as u8 as f32,
        _ => patch[p],
    })
}

/// Files `n` frames long, each its own.
fn files(n: usize, how_many: usize) -> (Vec<Vec<f32>>, Vec<Asset>) {
    let xs: Vec<Vec<f32>> = (0..how_many).map(|i| file(n, 100 + i as u32)).collect();
    let assets = xs.iter().map(|x| native::asset(SR, vec![x.clone()])).collect();
    (xs, assets)
}

#[test]
fn a_region_plays_by_its_keys_and_velocity() {
    let (x, assets) = files(400, 3);
    // Soft and hard layers on 60 to 72 (split at 64), and a low file on every note below.
    let soft = R { keys: [60.0, 72.0], velocity: [1.0, 63.0], ..R::of(0) };
    let hard = R { keys: [60.0, 72.0], velocity: [64.0, 127.0], ..R::of(1) };
    let low = R { root: 48.0, keys: [0.0, 59.0], ..R::of(2) };
    let regions = [soft, hard, low];
    let at = |pitch: f32, velocity: f32| play_regions(&assets, &regions, Note { pitch, velocity, ..NOTE }, 500, 37, &[]);
    let plays = |pitch: f32, velocity: f32, want: &[f32]| {
        let got = at(pitch, velocity).left();
        assert!(got[..want.len()] == want[..] && got[want.len()..].iter().all(|&v| v == 0.0), "{pitch} at {velocity}");
    };
    // velocity × 127, rounded: 63/127 is 63, 64/127 is 64, and 0.5 is 63.5, which rounds up to 64.
    plays(60.0, 63.0 / 127.0, &x[0]);
    plays(60.0, 64.0 / 127.0, &x[1]);
    plays(60.0, 0.5, &x[1]);
    // Velocity 0 is 1, the softest: never no layer.
    plays(60.0, 0.0, &x[0]);
    plays(60.0, 1.0, &x[1]);
    plays(48.0, 1.0, &x[2]);
    // A note no region holds is silence, and never active.
    let none = at(80.0, 1.0);
    assert!(none.left().iter().all(|&v| v == 0.0) && !none.active.iter().any(|&a| a), "80 is in no region");
}

#[test]
fn every_region_that_matches_plays_summed() {
    let (x, assets) = files(300, 3);
    let regions = [R::of(0), R { velocity: [100.0, 127.0], ..R::of(1) }, R::of(2)];
    let got = play_regions(&assets, &regions, NOTE, 400, 37, &[]).left();
    assert!((0..300).all(|t| got[t] == x[0][t] + x[1][t] + x[2][t]), "three layers, summed in order");
    let soft = play_regions(&assets, &regions, Note { velocity: 0.5, ..NOTE }, 400, 37, &[]).left();
    assert!((0..300).all(|t| soft[t] == x[0][t] + x[2][t]), "the loud layer is left out");
    // The same file in two regions plays twice.
    let twice = play_regions(&assets[..1], &[R::of(0), R { volume: -120.0, ..R::of(0) }, R::of(0)], NOTE, 400, 37, &[]).left();
    assert!((0..300).all(|t| twice[t] == x[0][t] + x[0][t] * db_to_gain(-120.0) + x[0][t]));
}

#[test]
fn round_robin_takes_turns_by_the_count() {
    let (x, assets) = files(200, 5);
    // A snare of two takes, a hat of three, and a layer with no turns that always plays.
    let regions = [
        R { turn: [1.0, 2.0], keys: [60.0, 60.0], ..R::of(0) },
        R { turn: [2.0, 2.0], keys: [60.0, 60.0], ..R::of(1) },
        R { turn: [1.0, 3.0], root: 42.0, keys: [42.0, 42.0], ..R::of(2) },
        R { turn: [2.0, 3.0], root: 42.0, keys: [42.0, 42.0], ..R::of(3) },
        R { turn: [3.0, 3.0], root: 42.0, keys: [42.0, 42.0], ..R::of(4) },
    ];
    let take = |pitch: f32, count: f32| {
        let got = play_regions(&assets, &regions, Note { pitch, count, ..NOTE }, 200, 37, &[]).left();
        let which: Vec<usize> = (0..5).filter(|&i| got[..50].iter().zip(&x[i]).all(|(a, b)| a == b)).collect();
        assert_eq!(which.len(), 1, "{pitch} at count {count}: one take");
        which[0]
    };
    let snare: Vec<usize> = [1.0, 2.0, 3.0, 4.0, 5.0].iter().map(|&c| take(60.0, c)).collect();
    assert_eq!(snare, [0, 1, 0, 1, 0], "the nth start plays take ((n - 1) mod 2) + 1");
    let hat: Vec<usize> = [1.0, 2.0, 3.0, 4.0, 7.0].iter().map(|&c| take(42.0, c)).collect();
    assert_eq!(hat, [2, 3, 4, 2, 2], "and of 3, ((n - 1) mod 3) + 1");
    // No count (0, NaN, or below) is the first turn; the count's wrap at 2^24 is an even number.
    for c in [0.0, -5.0, f32::NAN] {
        assert_eq!(take(60.0, c), 0, "count {c}");
    }
    assert_eq!(take(60.0, 16_777_215.0), 0, "2^24 - 1: an odd count, the first take of two");
}

#[test]
fn release_regions_start_when_the_note_ends() {
    let (x, assets) = files(1_000, 3);
    // A long attack, a short release noise for loud notes, and one for other keys.
    let regions =
        [R::of(0), R { release: true, velocity: [100.0, 127.0], ..R::of(1) }, R { release: true, keys: [70.0, 80.0], ..R::of(2) }];
    let note = Note { held: 300, ..NOTE };
    let got = play_regions(&assets, &regions, note, 2_000, 37, &[]);
    let l = got.left();
    assert!(l[..300] == x[0][..300], "the attack alone while held");
    // One-shot: the attack plays on, and the release region starts with the note's end.
    assert!((300..1_000).all(|t| l[t] == x[0][t] + x[1][t - 300]), "attack and release, summed");
    assert!((1_000..1_300).all(|t| l[t] == x[1][t - 300]), "the release plays to its end");
    assert!(l[1_300..].iter().all(|&v| v == 0.0) && got.active[1_290] && !got.active[1_999]);
    // A softer note matches no loud release noise.
    let quieter = play_regions(&assets, &regions, Note { velocity: 0.5, ..note }, 2_000, 37, &[]).left();
    assert!((300..1_000).all(|t| quieter[t] == x[0][t]), "a soft note has no release noise");
    // Gate mode with no release time: the attack stops with the note, the release plays on.
    let gated = play_regions(&assets, &regions, note, 2_000, 37, &[(MODE, 1.0), (RELEASE, 0.0)]).left();
    assert!((300..1_000).all(|t| gated[t] == x[1][t - 300]), "gated: the release region alone");
}

// A poly frees a voice whose Off lands on a block line before the voice runs that block, when its
// last block was silent and it says it isn't active (core/runtime's poly). A release region starts
// sound at the gate's fall, so a held note it matches is active, though its attack has ended or it
// has none: else its release would be lost whenever the note ends on a block line.
#[test]
fn a_held_note_with_a_release_region_is_active() {
    let (x, assets) = files(200, 3);
    // Below 70, a short attack and a release noise for loud notes; on 70 to 80, a release alone.
    let regions = [
        R { keys: [0.0, 69.0], ..R::of(0) },
        R { release: true, keys: [0.0, 69.0], velocity: [100.0, 127.0], ..R::of(1) },
        R { release: true, root: 75.0, keys: [70.0, 80.0], ..R::of(2) },
    ];
    let note = Note { held: 1_000, ..NOTE };
    let loud = play_regions(&assets, &regions, note, 1_400, 37, &[]);
    assert!(loud.left()[300..1_000].iter().all(|&v| v == 0.0), "the attack has ended while the note is held");
    assert!(loud.active[500] && loud.active[999], "held, with a release to come: active");
    assert!((1_000..1_200).all(|t| loud.left()[t] == x[1][t - 1_000]) && !loud.active[1_399], "the release plays, then it is over");
    // A release-only key: no attack at all, active until its release has played.
    let only = play_regions(&assets, &regions, Note { pitch: 75.0, ..note }, 1_400, 37, &[]);
    assert!(only.left()[..1_000].iter().all(|&v| v == 0.0) && only.active[0] && only.active[999]);
    assert!((1_000..1_200).all(|t| only.left()[t] == x[2][t - 1_000]) && !only.active[1_399]);
    // No release region matches a soft note: once its attack ends it is silent, and not active.
    let soft = play_regions(&assets, &regions, Note { velocity: 0.5, ..note }, 1_400, 37, &[]);
    assert!(soft.active[100] && !soft.active[500] && soft.left()[200..].iter().all(|&v| v == 0.0));
}

/// A sine of `period` samples, `n` long.
fn sine(n: usize, period: f32) -> Vec<f32> {
    (0..n).map(|t| (std::f32::consts::TAU * t as f32 / period).sin() * 0.8).collect()
}

#[test]
fn a_loop_repeats_while_held_then_plays_on_to_the_end() {
    let x = file(1_000, 20);
    let asset = [native::asset(SR, vec![x.clone()])];
    // Frames 100 to 499 loop (400 of them), crossfaded over min(5 ms = 240, 100, 200) = 100.
    let lp = R { looped: [100.0, 499.0], ..R::of(0) };
    // Held until 2050: in the crossfade from 2000 to 2100.
    let got = play_regions(&asset, &[lp], Note { held: 2_050, ..NOTE }, 3_200, 37, &[]);
    let l = got.left();
    assert!(l[..400] == x[..400], "up to the crossfade, the file");
    for t in 400..500 {
        let fade = (t - 400) as f32 / 100.0;
        let want = x[t] + (x[t - 400] - x[t]) * fade;
        assert!((l[t] - want).abs() <= 1e-6, "{t}: {} against {want}", l[t]);
    }
    assert!((500..800).all(|t| l[t] == x[t - 400]), "after the end comes the start");
    assert!((900..1_200).all(|t| l[t] == x[t - 800]) && (1_700..2_000).all(|t| l[t] == x[t - 1_600]), "and again");
    // Let go mid-crossfade: that crossfade finishes, and the next pass plays on to the file's end.
    assert!((2_100..2_400).all(|t| l[t] == x[t - 2_000]), "the crossfade let go in finishes");
    assert!((2_400..3_000).all(|t| l[t] == x[t - 2_000]), "then on past the loop to the end");
    assert!(l[3_000..].iter().all(|&v| v == 0.0) && !got.active[3_199], "and silence after it");
    // Gate mode: it fades out over the release as it plays on.
    let gated = play_regions(&asset, &[lp], Note { held: 2_050, ..NOTE }, 3_200, 37, &[(MODE, 1.0), (RELEASE, 0.002)]).left();
    assert!(gated[2_049] != 0.0 && gated[2_050 + 100..].iter().all(|&v| v == 0.0), "gated: 2 ms of release");
    // Let go before the loop: it never loops.
    let short = play_regions(&asset, &[lp], Note { held: 50, ..NOTE }, 1_200, 37, &[]).left();
    assert!(short[..1_000] == x[..] && short[1_000..].iter().all(|&v| v == 0.0), "let go early: the file once");
}

// The loop SFZ files most often have ends at the file's last frame (and the one a WAV's `smpl`
// chunk gives). It loops while held like any other: the pass that reaches the end goes back to the
// start, never ends at the file's end. So does a loop whose end is past the file (the build can't
// know the file's length: it is taken as the last frame), and one that ends within a step of the
// file's end, played above its root.
#[test]
fn a_loop_to_the_files_last_frame_loops_while_held() {
    let x = file(1_000, 21);
    let asset = [native::asset(SR, vec![x.clone()])];
    let held = |looped: [f64; 2], pitch: f32| {
        play_regions(&asset, &[R { looped, ..R::of(0) }], Note { pitch, held: 20_000, ..NOTE }, 21_000, 37, &[])
    };
    for (looped, pitch) in
        [([300.0, 999.0], 60.0), ([300.0, 1e9], 60.0), ([300.0, 999.0], 61.0), ([300.0, 998.0], 61.0), ([300.0, 998.0], 60.0)]
    {
        let got = held(looped, pitch);
        let l = got.left();
        assert!(l[19_000..20_000].iter().any(|&v| v != 0.0) && got.active[19_999], "{looped:?} at {pitch}: still looping when let go");
    }
    // At its root, exactly: frames 300 to 999 loop (700 of them), crossfaded over min(240, 300, 350).
    let l = held([300.0, 999.0], 60.0).left();
    assert!(l[..760] == x[..760], "up to the crossfade, the file");
    for t in 760..1_000 {
        let want = x[t] + (x[t - 700] - x[t]) * (t - 760) as f32 / 240.0;
        assert!((l[t] - want).abs() <= 1e-6, "{t}: {} against {want}", l[t]);
    }
    assert!((1_000..1_460).all(|t| l[t] == x[t - 700]), "after the file's last frame comes the loop's start");
    // Let go, it plays on to the file's end, then it is over.
    let out = play_regions(&asset, &[R { looped: [300.0, 999.0], ..R::of(0) }], Note { held: 1_100, ..NOTE }, 3_000, 37, &[]);
    let l = out.left();
    assert!((1_460..1_700).all(|t| l[t] == x[t - 700]), "the pass let go in plays on");
    assert!(l[1_700..].iter().all(|&v| v == 0.0) && !out.active[2_999], "to the file's end, then silence");
}

#[test]
fn a_loops_crossfade_bounds_the_step() {
    // A sine of 96 samples, looped over 985 frames: its end is at the sine's peak, its start a quarter
    // of a period away, so a jump from the end to the start steps by the sine's whole height.
    let x = sine(4_000, 96.0);
    let asset = [native::asset(SR, vec![x.clone()])];
    let own = max_step(&x, 1, 4_000);
    let looped = |start: f64| {
        let r = R { looped: [start, start + 984.0], ..R::of(0) };
        play_regions(&asset, &[r], Note { held: 20_000, ..NOTE }, 6_000, 37, &[]).left()
    };
    // From frame 0 there is nothing before the start to crossfade with: the jump shows.
    let hard = looped(0.0);
    assert!(max_step(&hard, 1, 6_000) > 0.5, "a loop from frame 0 jumps: {}", max_step(&hard, 1, 6_000));
    // From frame 500, 5 ms (240 frames) of crossfade: the step stays near the sine's own.
    let soft = looped(500.0);
    let step = max_step(&soft, 1, 6_000);
    assert!(step < 1.5 * own, "crossfaded, the biggest step is {step}, against the sine's own {own}");
}

#[test]
fn tune_volume_pan_and_offset_are_each_regions_own() {
    let x = file(2_001, 30);
    let asset = [native::asset(SR, vec![x.clone()])];
    let one = |r: R| play_regions(&asset, &[r], NOTE, 1_200, 37, &[]);
    let up = one(R { tune: 1_200.0, ..R::of(0) }).left();
    assert!((0..=1_000).all(|t| up[t] == x[2 * t]), "1200 cents up: every second sample");
    let loud = one(R { volume: 20.0, ..R::of(0) }).left();
    assert!((0..1_200).all(|t| loud[t] == x[t] * db_to_gain(20.0)), "20 dB: ten times");
    let right = one(R { pan: 1.0, ..R::of(0) });
    assert!(right.main()[..1_200].iter().zip(&x).all(|(f, &x)| f[0] == 0.0 && f[1] == x), "pan 1: the right side alone");
    let left = one(R { pan: -0.5, ..R::of(0) });
    let side = (0.25 * std::f32::consts::PI).sin();
    assert!(left.main()[..1_200].iter().zip(&x).all(|(f, &x)| f[0] == x && f[1] == x * side), "pan -0.5: the right side lower");
    let later = one(R { offset: 100.0, ..R::of(0) }).left();
    assert!((0..1_200).all(|t| later[t] == x[t + 100]), "offset 100: from frame 100");
    // Every field at its default is the file as it is, both sides.
    let plain = one(R::of(0));
    assert!(plain.main()[..1_200].iter().zip(&x).all(|(f, &x)| f[0] == x && f[1] == x));
}

/// A small instrument with everything: two velocity layers, two takes, a release noise and a loop.
fn instrument() -> (Vec<Asset>, Vec<R>) {
    let x = |n, seed| native::asset(SR, vec![file(n, seed)]);
    let assets = vec![x(3_000, 40), x(3_000, 41), x(3_000, 42), x(800, 43), native::asset(44_100.0, vec![sine(5_000, 101.0)])];
    let regions = vec![
        R { keys: [48.0, 72.0], velocity: [1.0, 80.0], turn: [1.0, 2.0], ..R::of(0) },
        R { keys: [48.0, 72.0], velocity: [1.0, 80.0], turn: [2.0, 2.0], ..R::of(1) },
        R { keys: [48.0, 72.0], velocity: [81.0, 127.0], tune: -30.0, pan: 0.3, ..R::of(2) },
        R { release: true, volume: -6.0, ..R::of(3) },
        R { looped: [1_000.0, 2_999.0], volume: -12.0, offset: 10.0, ..R::of(4) },
    ];
    (assets, regions)
}

#[test]
fn any_block_split_plays_the_same() {
    let (assets, regions) = instrument();
    for note in [Note { pitch: 62.3, velocity: 0.4, held: 4_000, count: 2.0 }, Note { pitch: 55.0, velocity: 0.9, held: 900, count: 1.0 }] {
        let at = |split: usize| play_regions(&assets, &regions, note, 9_000, split, &[(VELTRACK, 1.0), (ATTACK, 0.003), (MODE, 1.0)]).out;
        let whole = at(BLOCK);
        assert!(whole[0].iter().any(|f| f[0] != 0.0));
        for split in [1, 7, 37, 100] {
            assert!(at(split) == whole, "blocks of {split}");
        }
    }
}

#[test]
fn a_reset_plays_regions_from_the_start_again() {
    let (assets, regions) = instrument();
    let values = layout(&regions);
    let cfg = Cfg { values: Vec::leak(values), assets: Vec::leak(assets), ..Cfg::new(SR, BLOCK, 2) };
    let mut host = m2_kernel::Host::<Sampler<true>>::of(cfg);
    let take = |host: &mut m2_kernel::Host<Sampler<true>>| -> Vec<f32> {
        let mut got = vec![];
        for b in 0..40 {
            let mut patch = defaults("regions", &REGION_NAMES);
            patch[PITCH] = 60.0;
            patch[GATE] = (b < 20) as u8 as f32;
            patch[COUNT] = 2.0;
            for (p, v) in patch.iter().enumerate() {
                host.port_mut(p).fill(*v);
            }
            host.process(BLOCK as u32);
            got.extend_from_slice(host.port_mut(REGION_NAMES.len()));
        }
        got
    };
    let first = take(&mut host);
    assert!(first.iter().any(|&v| v != 0.0));
    host.reset();
    assert!(take(&mut host) == first, "a reset starts over");
}

#[test]
fn no_control_or_value_breaks_regions() {
    let (assets, regions) = instrument();
    let mut base = defaults("regions", &REGION_NAMES);
    base[VELOCITY] = 1.0;
    for set in torture(&base, &[PITCH, GATE]).iter().take(60) {
        let changes: Vec<(usize, f32)> = set.iter().copied().enumerate().collect();
        for pitch in [f32::NAN, -1e9, 60.0, 1e9] {
            let got = play_regions(&assets, &regions, Note { pitch, held: 300, ..NOTE }, 900, 37, &changes);
            assert!(got.main().iter().all(|f| f[0].is_finite() && f[1].is_finite() && f[0].abs() <= 1e4), "{set:?} at {pitch}");
        }
    }
    // Values no layout holds: wrong counts, files that aren't there, ranges upside down, huge loops.
    let mut rng = Lcg(7);
    let odd = [-1e300, -1e9, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0, 3.0, 15.0, 127.0, 128.0, 1e9, 1e300];
    for len in [0, 1, 2, 15, 16, 31, 45, 200] {
        for _ in 0..20 {
            let mut values: Vec<f64> = (0..len).map(|_| odd[rng.next() as usize % odd.len()]).collect();
            if len > 0 && rng.next().is_multiple_of(2) {
                values[0] = FIELDS as f64;
            }
            for note in [NOTE, Note { held: 50, count: 3.0, ..NOTE }] {
                let got = play_layout(&assets, &values, note, 600, 37, &[]);
                assert!(got.main().iter().all(|f| f[0].is_finite() && f[1].is_finite()), "{values:?}");
            }
        }
    }
}
