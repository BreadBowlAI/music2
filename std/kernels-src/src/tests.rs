//! Tests of the kernels in `lib.rs`: std:gain gives the bits it gave before its fast path, on
//! every sample, and its smoother ends every block where it did.

use std::ops::Range;

use m2_kernel::{Kernel, db_to_gain};

use crate::testkit::{BLOCK, SR, play_at};
use crate::{Gain, Smooth};

/// std:gain's loop before its fast path (std.wasm at `85b069a`): `db_to_gain` and a smoother step
/// on every sample. The bits std:gain must keep.
fn per_sample(smooth: &mut Smooth, l: &[f32], r: Option<&[f32]>, db: &[f32], ol: &mut [f32], or: &mut [f32]) {
    for i in 0..ol.len() {
        let g = smooth.next(db_to_gain(db[i]));
        ol[i] = l[i] * g;
        or[i] = r.map_or(0.0, |r| r[i] * g);
    }
}

/// The audio both play: a sine, and now and then a value whose bits show any other multiply:
/// -0.0, a subnormal, a large value, infinity, NaN.
fn audio(channel: usize, t: usize) -> f32 {
    match t % 97 {
        13 => -0.0,
        41 => 1.0e-40,
        59 => f32::INFINITY,
        77 => -3.0e30,
        83 => f32::NAN,
        _ => (t as f32 * 0.0575 + channel as f32).sin() * 0.8,
    }
}

/// Which blocks [`compare`] played, from which sample, and whether each was one gain.
struct Seen(Vec<(Range<usize>, bool)>);

impl Seen {
    /// How many blocks inside `within` were one gain.
    fn steady_in(&self, within: Range<usize>) -> usize {
        self.0.iter().filter(|(b, steady)| *steady && b.start >= within.start && b.end <= within.end).count()
    }

    /// Whether the block holding sample `t` was one gain.
    fn steady_at(&self, t: usize) -> bool {
        self.0.iter().any(|(b, steady)| *steady && b.contains(&t))
    }

    fn blocks(&self) -> usize {
        self.0.len()
    }

    fn steady(&self) -> usize {
        self.0.iter().filter(|(_, steady)| *steady).count()
    }
}

/// Plays `db(t)` through std:gain and through [`per_sample`] side by side: `frames` samples, in
/// blocks of `sizes` taken in turn, stereo or mono. Every output sample, and the smoother after
/// every block, must be the same bits.
fn compare(what: &str, frames: usize, sizes: &[usize], stereo: bool, db: impl Fn(usize) -> f32) -> Seen {
    let mut gain = Gain::new(&m2_kernel::Cfg::new(SR, BLOCK, if stereo { 2 } else { 1 }));
    let mut before = Smooth::new(0.005, SR);
    let mut seen = Seen(vec![]);
    let mut t = 0;
    for &n in sizes.iter().cycle() {
        if t >= frames {
            break;
        }
        let n = n.min(frames - t);
        let at = |c: usize| (t..t + n).map(|t| audio(c, t)).collect::<Vec<f32>>();
        let (l, r) = (at(0), at(1));
        let r = stereo.then_some(&r[..]);
        let d: Vec<f32> = (t..t + n).map(&db).collect();
        // Asking changes nothing: it only keeps the dB's gain, which `block` asks for first anyway.
        seen.0.push((t..t + n, gain.steady(&d).is_some()));
        let (mut gl, mut gr, mut bl, mut br) = (vec![1.5; n], vec![1.5; n], vec![2.5; n], vec![2.5; n]);
        gain.block(&l, r, &d, &mut gl, &mut gr);
        per_sample(&mut before, &l, r, &d, &mut bl, &mut br);
        for i in 0..n {
            assert_eq!(gl[i].to_bits(), bl[i].to_bits(), "{what}: left at sample {}: {} against {}", t + i, gl[i], bl[i]);
            assert_eq!(gr[i].to_bits(), br[i].to_bits(), "{what}: right at sample {}: {} against {}", t + i, gr[i], br[i]);
        }
        let (now, was) = (&gain.smooth, &before);
        assert!(
            now.value.to_bits() == was.value.to_bits() && now.primed == was.primed,
            "{what}: the smoother after sample {}: {} against {}",
            t + n,
            now.value,
            was.value
        );
        t += n;
    }
    seen
}

#[test]
fn zero_db_is_one_gain_from_the_second_block() {
    // The mixer's fader left at 0 dB, as on most channels: x × 1.0.
    let seen = compare("0 dB", 2 * 48_000, &[BLOCK], true, |_| 0.0);
    assert_eq!(seen.steady(), seen.blocks() - 1, "only the first block, which starts the smoother, isn't one gain");
}

#[test]
fn a_settled_value_is_one_gain() {
    let seen = compare("-6 dB", 48_000, &[BLOCK], true, |_| -6.0);
    assert_eq!(seen.steady(), seen.blocks() - 1);
}

#[test]
fn steps_at_a_block_edge_and_inside_a_block() {
    // 0 dB; -12 dB from a block's edge; +6 dB from inside a block; -120 dB (silence: a gain of 0,
    // which the smoother nears until it rounds to where it stays); 0 dB again.
    let db = |t: usize| match t {
        0..1_280 => 0.0,
        1_280..20_050 => -12.0,
        20_050..40_000 => 6.0,
        40_000..80_000 => -120.0,
        _ => 0.0,
    };
    let seen = compare("steps", 110_000, &[BLOCK], true, db);
    for (held, from) in
        [(0..1_280, 0), (1_280..20_050, 1_280), (20_050..40_000, 20_050), (40_000..80_000, 40_000), (80_000..110_000, 80_000)]
    {
        assert!(!seen.steady_at(from), "the block where the dB changes, at sample {from}, isn't one gain");
        assert!(seen.steady_in(held.clone()) > 0, "the smoother settles within {held:?}, and its blocks are one gain");
    }
}

#[test]
fn ramps_and_automation() {
    // What an automation lane plays: a ramp that moves every sample, a hold, a slow wobble, a hold.
    let db = |t: usize| {
        let s = t as f32 / SR;
        match t {
            0..48_000 => -24.0 + 30.0 * s,
            48_000..72_000 => 6.0,
            72_000..120_000 => 6.0 * (s * 2.0).sin(),
            _ => -3.0,
        }
    };
    let seen = compare("ramps", 160_000, &[BLOCK], true, db);
    assert_eq!(seen.steady_in(0..48_000), 0, "a ramp is never one gain");
    assert!(seen.steady_in(48_000..72_000) > 0 && seen.steady_in(120_000..160_000) > 0, "each hold settles");
}

#[test]
fn a_block_whose_db_changes_at_one_sample() {
    // A settled -6 dB; the last sample of one block, and later the first of another, -6.5 dB.
    let (last, first) = (30 * BLOCK + BLOCK - 1, 60 * BLOCK);
    let seen = compare("one sample", 90 * BLOCK, &[BLOCK], true, |t| if t == last || t == first { -6.5 } else { -6.0 });
    assert!(seen.steady_at(last - BLOCK) && seen.steady_at(first - BLOCK), "settled before each");
    assert!(!seen.steady_at(last) && !seen.steady_at(first), "a block with one other dB isn't one gain");
}

#[test]
fn any_block_size_mono_or_stereo() {
    // Held steps, in blocks of every size a host may ask for, an empty one too.
    let db = |t: usize| match t {
        0..10_000 => 0.0,
        10_000..40_000 => -3.0 - (t / 6_000) as f32,
        _ => -9.0,
    };
    for stereo in [true, false] {
        let seen =
            compare(if stereo { "sizes, stereo" } else { "sizes, mono" }, 70_000, &[1, 37, 0, 128, 64, 3, 128, 128, 100], stereo, db);
        assert!(seen.steady() > seen.blocks() / 2, "most blocks are one gain once settled");
    }
}

#[test]
fn odd_db_values() {
    // -0 dB beside 0 dB, exactly -96 dB (silence) and just above it, then infinities and NaN.
    let db = |t: usize| match t / 6_000 {
        0 => 0.0,
        1 => -0.0,
        2 if t.is_multiple_of(2) => 0.0,
        2 => -0.0,
        3 => -96.0,
        4 => -95.99,
        5 => f32::INFINITY,
        6 => f32::NEG_INFINITY,
        7 => f32::NAN,
        _ => 0.0,
    };
    compare("odd dB", 9 * 6_000, &[BLOCK, 50], true, db);
}

#[test]
fn played_through_its_ports_it_gives_the_same_bits() {
    // The kernel as a host plays it (`process`, its ports, the channel count), in blocks of any
    // size, against the loop before over the whole signal at once: a step, a ramp, a hold.
    let db = |t: usize| match t {
        0..5_000 => 0.0,
        5_000..15_000 => -6.0,
        15_000..25_000 => -6.0 + (t - 15_000) as f32 * 0.001,
        _ => 4.0,
    };
    let frames = 40_000;
    for channels in [1, 2] {
        let mut smooth = Smooth::new(0.005, SR);
        let (l, r): (Vec<f32>, Vec<f32>) = (0..frames).map(|t| (audio(0, t), audio(1, t))).unzip();
        let d: Vec<f32> = (0..frames).map(db).collect();
        let (mut want_l, mut want_r) = (vec![0.0; frames], vec![0.0; frames]);
        per_sample(&mut smooth, &l, (channels == 2).then_some(&r[..]), &d, &mut want_l, &mut want_r);
        for split in [1, 37, BLOCK] {
            let played = play_at::<Gain>(SR, channels, frames, split, &|port, c, t| if port == 0 { audio(c, t) } else { db(t) });
            for (t, got) in played.main().iter().enumerate() {
                assert_eq!(
                    [got[0].to_bits(), got[1].to_bits()],
                    [want_l[t].to_bits(), want_r[t].to_bits()],
                    "{channels} channel(s), blocks of {split}: sample {t}"
                );
            }
        }
    }
}

// ABI v1: a kernel whose state holds a `Vec` clears it in place at a reset (a reset takes no
// memory), and starts over exactly as a new one does. (`@m2/effects`' delay, reverb and widen are
// tried beside their pack's kernels, with the ABI's own tests, which play its delay.)
#[test]
fn kernels_with_lines_reset_in_place_to_where_they_started() {
    use crate::testkit::assert_reset_starts_over;
    assert_reset_starts_over::<crate::fx::Chorus>(&[0.5, 1.0, 0.5, 0.5, 1.0], 5_000);
}
