//! Notes as signals: drum hits and their count. (The mono decoder's rules are the `poly` op's
//! now: `core/runtime/src/tests/poly.rs`.)

use super::*;
use crate::testkit::*;

#[test]
fn ports_match_the_manifest() {
    defaults("drumhit", &["pitch", "model"]);
    defaults("hitcount", &["hits"]);
}

#[test]
fn a_reset_starts_over() {
    assert_reset_starts_over::<DrumHit>(&[42.0, -1.0], 1_000);
    // A hit on every sample: the count climbs, and starts again from 0 after a reset.
    assert_reset_starts_over::<HitCount>(&[HIT_FIELD as f32], 1_000);
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
