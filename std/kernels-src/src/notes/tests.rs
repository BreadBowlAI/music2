//! Notes as signals: codes, the mono decoder's legato rules and note memory, drum hits and
//! their count.

use super::*;
use crate::testkit::*;

#[test]
fn ports_match_the_manifest() {
    defaults("notecv", &["pitch", "gate", "velocity"]);
    defaults("mono", &["last", "held", "glide"]);
    defaults("drumhit", &["pitch", "model"]);
    defaults("hitcount", &["hits"]);
    assert_eq!(manifest("mono").1, ["gate", "pitch", "velocity"]);
}

#[test]
fn a_reset_starts_over() {
    assert_reset_starts_over::<NoteCv>(&[64.0, 1.0, 0.7], 1_000);
    assert_reset_starts_over::<Mono>(&[1.0, 2.0, 0.01], 1_000);
    assert_reset_starts_over::<DrumHit>(&[42.0, -1.0], 1_000);
    // A hit on every sample: the count climbs, and starts again from 0 after a reset.
    assert_reset_starts_over::<HitCount>(&[HIT_FIELD as f32], 1_000);
}

#[test]
fn codes_round_trip_exactly() {
    for p in 0..(128 * 64) {
        for v in 0..=127 {
            let (pitch, velocity) = (p as f32 / 64.0, v as f32 / 127.0);
            let code = note_code(pitch, velocity);
            assert!((1.0..16_777_216.0).contains(&code) && code == code.floor());
            let (dp, dv) = decode(code).unwrap();
            assert!(dp == pitch && (dv - velocity).abs() < 1e-6, "{pitch} {velocity}: {dp} {dv}");
        }
    }
    // Out of range and broken values still make codes, and nothing else decodes.
    for x in [f32::NAN, f32::INFINITY, -5.0, 1e9] {
        assert!(decode(note_code(x, x)).is_some());
    }
    for x in [0.0, -1.0, 0.5, 1.5, f32::NAN, f32::INFINITY, 1e9, 2.5e6 + 0.5] {
        assert_eq!(decode(x), None, "{x}");
    }
}

#[test]
fn notecv_marks_the_start_and_the_end() {
    let input = |p: usize, _c: usize, t: usize| match p {
        0 => 64.5,
        1 => (t < 300) as i32 as f32,
        _ => 0.7,
    };
    let out = play::<NoteCv>(1_000, 7, &input);
    let x = out.main();
    let code = note_code(64.5, 0.7);
    assert_eq!((x[0][1], x[300][1]), (code, -code));
    assert!(x.iter().enumerate().all(|(t, f)| t == 0 || t == 300 || f[1] == 0.0));
    assert!(x.iter().enumerate().all(|(t, f)| f[0] == (t < 300) as i32 as f32));
    assert!(out.active.iter().all(|a| !a));
}

/// Notes `(pitch, velocity, start, end)`, in order of their starts, as std's `mono()` feeds
/// the decoder: `last` is a one-voice notecv poly (the latest note: a new note steals it, so
/// a stolen note never says it ended), `held` an eight-voice one (its left side counts the
/// notes held, its right side sums their starts and ends).
fn decoder(notes: &[(f32, f32, usize, usize)], glide: f32, frames: usize) -> Played {
    let code = |n: &(f32, f32, usize, usize)| note_code(n.0, n.1);
    let input = |p: usize, c: usize, t: usize| -> f32 {
        let latest = notes.iter().rfind(|n| n.2 <= t);
        match (p, c) {
            (0, 0) => latest.is_some_and(|n| t < n.3) as i32 as f32,
            (0, 1) => match latest {
                Some(n) if n.2 == t => code(n),
                Some(n) if n.3 == t => -code(n),
                _ => 0.0,
            },
            (1, 0) => notes.iter().filter(|n| n.2 <= t && t < n.3).count() as f32,
            (1, 1) => notes
                .iter()
                .map(|n| {
                    if n.2 == t {
                        code(n)
                    } else if n.3 == t {
                        -code(n)
                    } else {
                        0.0
                    }
                })
                .sum(),
            _ => glide,
        }
    };
    play::<Mono>(frames, 13, &input)
}

/// The velocity a code carries.
fn vel(v: f32) -> f32 {
    (v * 127.0).round() / 127.0
}

#[test]
fn touching_notes_jump_and_retrigger() {
    let out = decoder(&[(48.0, 0.8, 100, 1_000), (55.0, 0.5, 1_000, 2_000)], 0.05, 3_000);
    let (gate, pitch, velocity) = (&out.out[0], &out.out[1], &out.out[2]);
    assert_eq!(gate[99][0], 0.0);
    assert!(gate[100..1_000].iter().all(|g| g[0] == 1.0));
    // One sample of gate off where the second note starts, then on again.
    assert_eq!(gate[1_000][0], 0.0);
    assert!(gate[1_001..2_000].iter().all(|g| g[0] == 1.0));
    assert!(gate[2_000..].iter().all(|g| g[0] == 0.0));
    assert_eq!((pitch[999][0], pitch[1_000][0]), (48.0, 55.0));
    assert_eq!((velocity[999][0], velocity[1_000][0]), (vel(0.8), vel(0.5)));
    // Released, the pitch holds for the tail.
    assert!(pitch[2_000..].iter().all(|p| p[0] == 55.0));
}

#[test]
fn overlapping_notes_slide_without_retriggering() {
    let glide = 0.01;
    let out = decoder(&[(48.0, 0.8, 100, 1_100), (60.0, 0.8, 1_000, 3_000)], glide, 4_000);
    let (gate, pitch) = (&out.out[0], &out.out[1]);
    assert!(gate[100..3_000].iter().all(|g| g[0] == 1.0), "legato drops the gate");
    let n = (glide * SR) as usize;
    assert_eq!(pitch[1_000][0], 48.0);
    assert_eq!(pitch[1_000 + n][0], 60.0);
    // Linear on the way.
    for k in 1..n {
        let want = 48.0 + 12.0 * k as f32 / n as f32;
        assert!((pitch[1_000 + k][0] - want).abs() < 1e-3, "{k}: {} vs {want}", pitch[1_000 + k][0]);
    }
    // No glide: a legato jump.
    let out = decoder(&[(48.0, 0.8, 100, 1_100), (60.0, 0.8, 1_000, 3_000)], 0.0, 4_000);
    assert_eq!(out.out[1][1_000][0], 60.0);
    assert!(out.out[0][100..3_000].iter().all(|g| g[0] == 1.0));
}

/// The pitch glides linearly from `from` at `at` to `to` over `n` samples.
fn assert_glides(pitch: &[[f32; 2]], at: usize, n: usize, from: f32, to: f32) {
    assert_eq!(pitch[at][0], from, "where the glide starts");
    for k in 1..n {
        let want = from + (to - from) * k as f32 / n as f32;
        assert!((pitch[at + k][0] - want).abs() < 1e-3, "{k}: {} vs {want}", pitch[at + k][0]);
    }
    assert_eq!(pitch[at + n][0], to, "where the glide ends");
}

#[test]
fn a_held_note_sounds_again_when_a_short_one_over_it_ends() {
    // An 808 slide up and back: A1 held, A2 over it for a moment.
    let glide = 0.01;
    let n = (glide * SR) as usize;
    let out = decoder(&[(33.0, 0.8, 100, 4_000), (45.0, 0.5, 1_000, 2_000)], glide, 5_000);
    let (gate, pitch, velocity) = (&out.out[0], &out.out[1], &out.out[2]);
    assert!(gate[100..4_000].iter().all(|g| g[0] == 1.0), "the gate stays on while A1 is held");
    assert!(gate[4_000..].iter().all(|g| g[0] == 0.0));
    assert_glides(pitch, 1_000, n, 33.0, 45.0);
    assert_glides(pitch, 2_000, n, 45.0, 33.0);
    assert_eq!((velocity[1_500][0], velocity[2_000][0]), (vel(0.5), vel(0.8)), "back to the held note's velocity");
    assert!(pitch[4_000..].iter().all(|p| p[0] == 33.0), "released, the pitch holds for the tail");
}

#[test]
fn a_trill_over_a_held_note_glides_each_way() {
    let glide = 0.005;
    let n = (glide * SR) as usize;
    let notes = [(60.0, 0.8, 100, 6_000), (62.0, 0.8, 1_000, 1_500), (64.0, 0.8, 2_000, 2_500), (67.0, 0.8, 3_000, 3_500)];
    let out = decoder(&notes, glide, 7_000);
    let (gate, pitch) = (&out.out[0], &out.out[1]);
    assert!(gate[100..6_000].iter().all(|g| g[0] == 1.0), "legato all the way");
    for (start, to) in [(1_000, 62.0), (2_000, 64.0), (3_000, 67.0)] {
        assert_glides(pitch, start, n, 60.0, to);
        assert_glides(pitch, start + 500, n, to, 60.0);
    }
}

#[test]
fn notes_that_end_together_return_to_the_one_still_held() {
    let notes = [(48.0, 0.8, 100, 5_000), (55.0, 0.6, 1_000, 3_000), (60.0, 0.7, 2_000, 3_000)];
    let out = decoder(&notes, 0.0, 6_000);
    let (gate, pitch, velocity) = (&out.out[0], &out.out[1], &out.out[2]);
    assert!(gate[100..5_000].iter().all(|g| g[0] == 1.0));
    assert_eq!((pitch[1_500][0], pitch[2_500][0], pitch[3_000][0]), (55.0, 60.0, 48.0));
    assert_eq!(velocity[3_000][0], vel(0.8));
    // The older of two still held, once the newer ends too.
    let notes = [(48.0, 0.8, 100, 5_000), (55.0, 0.6, 1_000, 4_000), (60.0, 0.7, 2_000, 3_000)];
    let out = decoder(&notes, 0.0, 6_000);
    assert_eq!((out.out[1][3_000][0], out.out[1][4_000][0]), (55.0, 48.0));
    assert!(out.out[0][100..5_000].iter().all(|g| g[0] == 1.0));
}

#[test]
fn a_note_that_ends_under_the_one_sounding_changes_nothing() {
    let out = decoder(&[(48.0, 0.8, 100, 2_000), (60.0, 0.8, 1_000, 3_000)], 0.0, 4_000);
    assert!(out.out[1][1_000..3_000].iter().all(|p| p[0] == 60.0));
    assert!(out.out[0][100..3_000].iter().all(|g| g[0] == 1.0));
    assert!(out.out[0][3_000..].iter().all(|g| g[0] == 0.0));
}

#[test]
fn a_chord_from_silence_plays_its_last_note() {
    // A one-voice poly keeps the last of the notes that start together; eight held count 2.
    let input = |p: usize, c: usize, t: usize| -> f32 {
        match (p, c) {
            (0, 0) => (50..500).contains(&t) as i32 as f32,
            (0, 1) => {
                if t == 50 {
                    note_code(67.0, 1.0)
                } else {
                    0.0
                }
            }
            (1, 0) => {
                if (50..500).contains(&t) {
                    2.0
                } else {
                    0.0
                }
            }
            (1, 1) => match t {
                50 => note_code(60.0, 1.0) + note_code(67.0, 1.0),
                500 => -note_code(60.0, 1.0) - note_code(67.0, 1.0),
                _ => 0.0,
            },
            _ => 0.1,
        }
    };
    let out = play::<Mono>(1_000, BLOCK, &input);
    assert_eq!(out.out[1][50][0], 67.0, "a note from silence doesn't slide");
    assert_eq!(out.out[0][50][0], 1.0);
}

#[test]
fn the_decoder_ignores_what_isnt_a_code() {
    let saw = |p: usize, _c: usize, t: usize| if p == 2 { 0.1 } else { 2.0 * (t as f32 * 0.0123).fract() - 1.0 };
    let out = play::<Mono>(4_800, BLOCK, &saw);
    assert!(out.out[1].iter().all(|p| p[0] == 60.0));
    assert!(out.out.iter().all(|port| port.iter().all(|f| f[0].is_finite())));
}

#[test]
fn drumhit_marks_every_hit_and_its_choke_group() {
    let hit = |model: f32, pitch: f32| {
        let input = |p: usize, _c: usize, _t: usize| if p == 0 { pitch } else { model };
        play::<DrumHit>(300, 7, &input).left()
    };
    let h = HIT_FIELD as f32;
    for (pitch, want) in [
        (36.0, h + 1.0),
        (35.0, h + 1.0),
        (38.0, h + 16.0),
        (39.0, h + 256.0),
        (42.0, h + 4096.0),
        (46.0, h + 4096.0),
        (49.0, h + 65536.0),
        (45.0, h),
        (51.0, h),
        (60.0, h),
    ] {
        let x = hit(-1.0, pitch);
        assert_eq!(x[0], want, "pitch {pitch}");
        assert!(x[1..].iter().all(|&v| v == 0.0));
    }
    assert_eq!(hit(1.0, 60.0)[0], h + 16.0, "a snare alone");
    assert_eq!(hit(4.0, 60.0)[0], h, "a tom alone");
    assert_eq!(hit(f32::NAN, f32::NAN)[0], h, "a silent note is still a hit");
    // Fifteen hits on one sample, each in every group, still sum to an exact integer that
    // carries into no other field.
    let most = 15.0 * (h + 1.0 + 16.0 + 256.0 + 4096.0 + 65536.0);
    assert_eq!(most, 16_777_215.0);
    assert_eq!(hits_in(most), 15);
}

/// `hitcount` on `hits`, a sum of marks at sample t, in blocks of `split`.
fn count(frames: usize, split: usize, hits: &dyn Fn(usize) -> f32) -> Vec<f32> {
    let input = |_p: usize, _c: usize, t: usize| hits(t);
    play::<HitCount>(frames, split, &input).left()
}

#[test]
fn hitcount_counts_hits_never_samples() {
    // Hits at samples 10 and 20, two at 30 (a kick and a hat, say), none after: the count
    // includes the sample's own hits, and holds through silence however long.
    let h = HIT_FIELD as f32;
    let hits = |t: usize| match t {
        10 => h + 1.0,
        20 => h + 4096.0,
        30 => 2.0 * h + 1.0 + 4096.0,
        _ => 0.0,
    };
    let x = count(100_000, BLOCK, &hits);
    let want = |t: usize| match t {
        0..10 => 0.0,
        10..20 => 1.0,
        20..30 => 2.0,
        _ => 4.0,
    };
    assert!(x.iter().enumerate().all(|(t, &v)| v == want(t)), "{:?}", &x[..40]);
    // The n-th hit sees the same count however long the kernel ran before it: what an engine
    // sitting stopped does before playback starts.
    for idle in [1, 127, 128, 96_000] {
        let late = count(idle + 100, 37, &|t| if t >= idle { hits(t - idle) } else { 0.0 });
        assert_eq!(&late[idle..], &x[..100], "after {idle} samples of silence");
    }
    // The same whatever the block split.
    for split in [1, 7, 64] {
        assert_eq!(count(1_000, split, &hits), x[..1_000], "in blocks of {split}");
    }
}

#[test]
fn hitcount_ignores_what_isnt_a_mark_and_wraps_exactly() {
    // The choke fields, fractions, negatives, NaN and huge values count no hits.
    let h = HIT_FIELD as f32;
    let noise = [1.0, 16.0 + 4096.0, 65_536.0 * 15.0, 0.5, h + 0.5, -h, f32::NAN, f32::INFINITY, -f32::INFINITY, 16_777_216.0, 1e30];
    let x = count(noise.len(), 1, &|t| noise[t]);
    assert!(x.iter().all(|&v| v == 0.0), "{x:?}");
    // Fifteen hits on every sample, past the wrap: every count is an exact integer below 2^24,
    // 15 on from the one before.
    let frames = (COUNT_WRAP / 15) as usize + 10;
    let x = count(frames, BLOCK, &|_| 15.0 * h);
    assert_eq!(x[0], 15.0);
    for w in x.windows(2) {
        assert!(w[1] < COUNT_WRAP as f32 && w[1] == w[1].floor());
        assert_eq!((w[0] as u32 + 15) % COUNT_WRAP, w[1] as u32);
    }
    assert!(x[frames - 1] < 200.0, "it wrapped");
}
