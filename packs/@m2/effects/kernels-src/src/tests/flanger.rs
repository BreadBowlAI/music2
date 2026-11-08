//! The Flanger (PACK-35), measured as its rule says it: where its comb's notches and peaks lie for
//! the delay its LFO is at, as its meter says it; how far and how fast the copy's delay sweeps, from
//! a tone's phase; how loud its peaks ring with feedback, and where its sign moves them; that it is
//! stable at its ends, plays its input at Mix 0, sweeps with no steps, and waits while stopped; at
//! 44.1, 48 and 96 kHz.

use std::f64::consts::{PI, TAU};

use m2_dsp::testing::effects::{BLOCK, noise};
use m2_dsp::testing::{defaults_in, manifest_in, max_step, play_at, spectrum};
use m2_kernel::{Host, Kernel};

use super::{MANIFEST, RATES, counting};
use crate::comb::{self, Flanger};

/// How late the copy is at LFO phase `phase` and `depth`, in seconds: PACK-35's sum.
fn late(depth: f64, phase: f64) -> f64 {
    0.0001 * 100f64.powf(depth * (0.5 - 0.5 * (TAU * phase).cos()))
}

/// The Flanger at `sr`, mono, swept at `rate` Hz for `sweep` frames with silence in, then held still
/// (`playing` 0) and given a click: where its LFO stopped, as its meter `phase` says it, and what the
/// click becomes, until it has rung down under a ten-billionth.
fn held(sr: f32, rate: f32, sweep: usize, depth: f32, feedback: f32, mix: f32) -> (f64, Vec<f64>) {
    // At Feedback ±0.9 the copy goes round at most 10 ms a time, and is 0.9 times as loud each time.
    let frames = (0.011 * 230.0 * sr) as usize;
    let played = play_at::<Flanger>(sr, 1, sweep + frames, BLOCK, &|p, _, t| match p {
        0 => (t == sweep) as u8 as f32,
        1 => rate,
        2 => depth,
        3 => feedback,
        4 => mix,
        _ => (t < sweep) as u8 as f32,
    });
    let phase = played.out[1].last().unwrap()[0] as f64;
    (phase, played.out[0][sweep..].iter().map(|f| f[0] as f64).collect())
}

/// How loud a click's sound `h` (at `sr`) makes `hz`, against the click: the size of its spectrum
/// there.
fn gain_at(h: &[f64], hz: f64, sr: f64) -> f64 {
    let w = TAU * hz / sr;
    let (mut re, mut im) = (0.0, 0.0);
    // A turning pointer, set again from the exact angle every 4096 samples so it doesn't drift.
    let (mut c, mut s) = (1.0f64, 0.0f64);
    let (cw, sw) = (w.cos(), w.sin());
    for (n, &v) in h.iter().enumerate() {
        if n % 4096 == 0 {
            (c, s) = ((w * n as f64).cos(), (w * n as f64).sin());
        }
        re += v * c;
        im -= v * s;
        (c, s) = (c * cw - s * sw, s * cw + c * sw);
    }
    (re * re + im * im).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-300).log10()
}

// PACK-35: the Flanger's manifest says what its code is: its ports, defaults and ranges, its meter,
// and no latency.
#[test]
fn the_flangers_manifest_is_its_code() {
    let defaults = defaults_in(MANIFEST, "flanger", &["in", "rate", "depth", "feedback", "mix", "playing"]);
    assert_eq!(defaults, [0.0, 0.25, 0.7, 0.5, 0.5, 1.0]);
    assert_eq!(manifest_in(MANIFEST, "flanger").1, ["out"]);
    let entry = &MANIFEST[MANIFEST.find("name = \"flanger\"").unwrap()..];
    let entry = &entry[..entry.find("[[kernel]]").unwrap_or(entry.len())];
    for port in ["\"rate: control<hz> = 0.25 in 0.05..10\"", "\"feedback: control<bipolar> = 0.5 in -0.9..0.9\""] {
        assert!(entry.contains(port), "{port}: {entry}");
    }
    assert!(entry.contains("meters = [\"phase: control<unit01>\"]"), "{entry}");
    assert!(!entry.lines().any(|l| l.starts_with("latency")), "no latency: {entry}");
    assert_eq!((Flanger::INPUTS, Flanger::OUTPUTS), (6, 2));
    assert_eq!((comb::SHORTEST, comb::RANGE, comb::MOST_FEEDBACK, comb::MOST_RATE), (0.0001, 100.0, 0.9, 10.0));
}

/// Where the Flanger is held for the comb's tests: the depth, and how long it sweeps at 1 Hz first
/// (a quarter or half a cycle), so its copy is about 1, 2.5, 0.5 and 10 ms late; and at its start,
/// 0.1 ms.
const HOLDS: [(f32, f64); 5] = [(1.0, 0.25), (0.7, 0.5), (0.35, 0.5), (1.0, 0.5), (0.5, 0.0)];

// PACK-35: held still at a delay D (its LFO stopped where its meter says), at Mix 0.5, its notches
// lie at (k + ½)/D and its peaks at k/D with Feedback 0 or above, and below 0 half way between;
// feedback rings at the peaks, (2 − |f|)/(2(1 − |f|)) as loud as the sound, and the notches are
// silent at 0, else |f|/(2(1 + |f|)) of it. Each place within 0.1% (a notch or a peak at it, against
// 0.1% either side), each level within 0.5 dB, up to 5 kHz, at 44.1, 48 and 96 kHz, the copy 0.5 to
// 10 ms late; at 0.1 ms, the first notch within 0.3% of 5 kHz.
#[test]
fn its_notches_and_peaks_lie_where_the_delay_puts_them() {
    // The furthest any level came from its sum, in dB, and any notch's depth, at each rate.
    for sr in RATES {
        let (mut worst, mut shallowest) = (0.0f64, f64::MIN);
        for (depth, cycles) in HOLDS {
            for feedback in [0.0f32, 0.5, -0.5, 0.9, -0.9] {
                let (phase, h) = held(sr, 1.0, (cycles * sr as f64) as usize, depth, feedback, 0.5);
                let d = late(depth as f64, phase);
                let f = feedback.abs() as f64;
                let (peak, notch) = ((2.0 - f) / (2.0 * (1.0 - f)), f / (2.0 * (1.0 + f)));
                let gain = |hz: f64| gain_at(&h, hz, sr as f64);
                // Notches, then peaks: (k + ½)/D and k/D, or the other way round below 0.
                let half = if feedback < 0.0 { 0.0 } else { 0.5 };
                let what = format!("{sr} Hz, the copy {:.3} ms late, feedback {feedback}", d * 1000.0);
                if d < 0.0002 {
                    // The first notch (feedback 0 and up) or peak (below 0) at 5 kHz.
                    let at = 0.5 / d;
                    let near = (0..=600).map(|i| at * (0.97 + 0.0001 * i as f64)).fold((at, f64::MAX), |m, hz| {
                        let g = gain(hz) * if feedback < 0.0 { -1.0 } else { 1.0 };
                        if g < m.1 { (hz, g) } else { m }
                    });
                    assert!(
                        (near.0 / at - 1.0).abs() < 0.003,
                        "{what}: its first {} at {:.0} Hz, not {at:.0}",
                        if feedback < 0.0 { "peak" } else { "notch" },
                        near.0
                    );
                    continue;
                }
                let top = 5000.0;
                let ks = |off: f64| -> Vec<f64> {
                    let n = ((top * d) - off).floor().max(0.0) as usize;
                    // The first three and the last two under 5 kHz.
                    let mut ks: Vec<usize> = (0..3.min(n + 1)).chain(n.saturating_sub(1)..=n).collect();
                    ks.dedup();
                    ks.into_iter().map(|k| (k as f64 + off) / d).filter(|&hz| hz > 20.0 && hz <= top).collect()
                };
                for hz in ks(half) {
                    let (g, lo, hi) = (gain(hz), gain(hz * 0.999), gain(hz * 1.001));
                    assert!(g < lo && g < hi, "{what}: no notch at {hz:.1} Hz ({:.2}, {:.2}, {:.2} dB)", db(lo), db(g), db(hi));
                    if f == 0.0 {
                        shallowest = shallowest.max(db(g));
                        assert!(db(g) < -40.0, "{what}: the notch at {hz:.1} Hz is only {:.1} dB deep", db(g));
                    } else {
                        worst = worst.max((db(g) - db(notch)).abs());
                        assert!(
                            (db(g) - db(notch)).abs() < 0.5,
                            "{what}: the notch at {hz:.1} Hz is {:.2} dB, not {:.2}",
                            db(g),
                            db(notch)
                        );
                    }
                }
                for hz in ks(0.5 - half) {
                    let (g, lo, hi) = (gain(hz), gain(hz * 0.999), gain(hz * 1.001));
                    if f > 0.0 {
                        assert!(g > lo && g > hi, "{what}: no peak at {hz:.1} Hz ({:.2}, {:.2}, {:.2} dB)", db(lo), db(g), db(hi));
                    }
                    worst = worst.max((db(g) - db(peak)).abs());
                    assert!((db(g) - db(peak)).abs() < 0.5, "{what}: the peak at {hz:.1} Hz is {:.2} dB, not {:.2}", db(g), db(peak));
                }
            }
        }
        println!("{sr} Hz: levels within {worst:.3} dB of their sums; notches at Feedback 0 at most {shallowest:.1} dB");
    }
}

// PACK-35, CARD-128: it has no make-up gain, so its loudest peak is `(1 − mix) + mix / (1 − |f|)` as
// loud as the sound, what its card's words say: +14.8 dB at Mix 0.5 and +20 dB at Mix 1 at Feedback
// ±0.9. Held still with the copy 0.5 to 10 ms late, at Feedback ±0.9 and ±0.5 and Mix 0.5 and 1, its
// first peak (0 Hz with Feedback 0 and up, 1/(2D) below) is within 0.3 dB of that, and none of its
// peaks up to 5 kHz is louder by more than 0.01 dB; at 44.1, 48 and 96 kHz.
#[test]
fn its_loudest_peak_is_as_loud_as_its_card_says() {
    for sr in RATES {
        let mut worst = 0.0f64;
        for (depth, cycles) in HOLDS {
            for feedback in [0.9f32, -0.9, 0.5, -0.5] {
                for mix in [0.5f32, 1.0] {
                    let (phase, h) = held(sr, 1.0, (cycles * sr as f64) as usize, depth, feedback, mix);
                    let d = late(depth as f64, phase);
                    if d < 0.0004 {
                        continue;
                    }
                    let f = feedback.abs() as f64;
                    let sum = (1.0 - mix as f64) + mix as f64 / (1.0 - f);
                    let gain = |hz: f64| gain_at(&h, hz, sr as f64);
                    let what = format!("{sr} Hz, the copy {:.3} ms late, feedback {feedback}, mix {mix}", d * 1000.0);
                    // Its peaks, k/D or (k + ½)/D, each the loudest within 0.2% of its place.
                    let off = if feedback < 0.0 { 0.5 } else { 0.0 };
                    let peak = |k: usize| {
                        let at = (k as f64 + off) / d;
                        if at == 0.0 { gain(0.0) } else { (0..=40).map(|i| gain(at * (0.998 + 0.0001 * i as f64))).fold(0.0, f64::max) }
                    };
                    let first = peak(0);
                    worst = worst.max((db(first) - db(sum)).abs());
                    assert!((db(first) - db(sum)).abs() < 0.3, "{what}: its first peak is {:+.2} dB, not {:+.2}", db(first), db(sum));
                    let last = ((5000.0 * d) - off).floor() as usize;
                    for k in [1, 2, last] {
                        let g = peak(k);
                        assert!(db(g) <= db(sum) + 0.01, "{what}: its peak {k} is {:+.2} dB, over {:+.2}", db(g), db(sum));
                    }
                }
            }
        }
        println!("{sr} Hz: its first peaks within {worst:.3} dB of (1 − mix) + mix / (1 − |f|)");
    }
}

/// How late the copy is at each frame, on each side, at `sr`: the Flanger, its copy alone and no
/// feedback, on a 50 Hz tone (sine and cosine, both sides), read from the tone's phase going out.
fn delays(sr: f32, rate: f32, depth: f32, frames: usize) -> Vec<[f64; 2]> {
    let f0 = 50.0;
    let tone = |shift: f64| {
        play_at::<Flanger>(sr, 2, frames, BLOCK, &move |p, _, t| match p {
            0 => (TAU * f0 * t as f64 / sr as f64 + shift).sin() as f32,
            1 => rate,
            2 => depth,
            3 => 0.0,
            _ => 1.0,
        })
        .out
        .swap_remove(0)
    };
    let (sin, cos) = (tone(0.0), tone(PI / 2.0));
    (0..frames)
        .map(|t| {
            [0, 1].map(|c| {
                let angle = (sin[t][c] as f64).atan2(cos[t][c] as f64);
                (TAU * f0 * t as f64 / sr as f64 - angle).rem_euclid(TAU) / (TAU * f0)
            })
        })
        .collect()
}

// PACK-35: the copy's delay sweeps from 0.1 ms to 0.1 ms × 100^depth and back, once each 1/rate
// seconds, starting at 0.1 ms; on a stereo sound the right side a quarter cycle behind the left. Read
// from a 50 Hz tone's phase through the copy alone: its shortest and longest within 0.5%, and the
// time between two crossings of its middle, 0.1 ms × 100^(depth / 2), within 2 ms of a cycle; at
// 44.1, 48 and 96 kHz.
#[test]
fn its_delay_sweeps_over_depths_range_at_rates_period() {
    for sr in RATES {
        for (rate, depth) in [(0.5f32, 1.0f32), (2.0, 0.5), (0.25, 0.7)] {
            let period = 1.0 / rate as f64;
            let frames = ((2.0 * period + 0.1) * sr as f64) as usize;
            let d = delays(sr, rate, depth, frames);
            let from = (0.02 * sr) as usize;
            let what = format!("{sr} Hz, rate {rate}, depth {depth}");
            for c in 0..2 {
                let (lo, hi) = d[from..].iter().fold((f64::MAX, 0.0f64), |(lo, hi), x| (lo.min(x[c]), hi.max(x[c])));
                let (want_lo, want_hi) = (0.0001, 0.0001 * 100f64.powf(depth as f64));
                assert!((lo / want_lo - 1.0).abs() < 0.005, "{what}, side {c}: shortest {:.4} ms, not {:.4}", lo * 1e3, want_lo * 1e3);
                assert!((hi / want_hi - 1.0).abs() < 0.005, "{what}, side {c}: longest {:.4} ms, not {:.4}", hi * 1e3, want_hi * 1e3);
            }
            // Where each side's delay rises through its middle.
            let mid = 0.0001 * 100f64.powf(depth as f64 / 2.0);
            let rises = |c: usize| -> Vec<f64> {
                (from + 1..frames).filter(|&t| d[t - 1][c] < mid && d[t][c] >= mid).map(|t| t as f64 / sr as f64).collect()
            };
            let (left, right) = (rises(0), rises(1));
            assert_eq!(left.len(), 2, "{what}: the left rises through its middle twice in two cycles: {left:?}");
            assert!((left[1] - left[0] - period).abs() < 0.002, "{what}: a cycle of {:.4} s, not {period}", left[1] - left[0]);
            assert!((left[0] - period / 4.0).abs() < 0.002, "{what}: half way up at {:.4} s, a quarter cycle in", left[0]);
            assert!(
                (right[0] - left[0] - period / 4.0).abs() < 0.002,
                "{what}: the right side {:.4} s behind, not a quarter cycle",
                right[0] - left[0]
            );
        }
    }
}

// PACK-35: Feedback ±0.9, with the copy held at its shortest and its longest, or swept at its
// slowest and its fastest, is stable: a burst of full-scale noise rings down to exact silence within
// 5 s, its output never over 20, at 44.1, 48 and 96 kHz.
#[test]
fn it_is_stable_at_its_ends() {
    for sr in RATES {
        for feedback in [0.9f32, -0.9] {
            for (rate, depth, playing) in [(1.0f32, 1.0f32, 0.0f32), (1.0, 0.0, 0.0), (0.05, 1.0, 1.0), (10.0, 1.0, 1.0)] {
                // Held at the longest: half a cycle swept first.
                let hold = (0.5 * sr) as usize;
                let frames = (6.0 * sr) as usize;
                let burst = (0.05 * sr) as usize;
                let played = play_at::<Flanger>(sr, 2, hold + frames, BLOCK, &|p, c, t| match p {
                    0 => {
                        if (hold..hold + burst).contains(&t) {
                            noise(c, t)
                        } else {
                            0.0
                        }
                    }
                    1 => rate,
                    2 => depth,
                    3 => feedback,
                    4 => 0.5,
                    _ => {
                        if t < hold {
                            1.0
                        } else {
                            playing
                        }
                    }
                });
                let out = &played.out[0][hold..];
                let what = format!("{sr} Hz, feedback {feedback}, rate {rate}, depth {depth}, playing {playing}");
                let loudest = out.iter().fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
                assert!(loudest.is_finite() && loudest < 20.0, "{what}: up to {loudest}");
                let last = out.iter().rposition(|f| f[0] != 0.0 || f[1] != 0.0).unwrap();
                assert!(last < (5.0 * sr) as usize, "{what}: still sounding {:.2} s after the burst", last as f32 / sr);
            }
        }
    }
}

// PACK-35: at Mix 0 it plays its input, bit for bit, whatever its other knobs, mono and stereo.
#[test]
fn mix_0_is_its_input_bit_for_bit() {
    for channels in [1, 2] {
        for (rate, depth, feedback) in [(0.25f32, 0.7f32, 0.5f32), (10.0, 1.0, -0.9), (0.05, 0.0, 0.9)] {
            let out = play_at::<Flanger>(48_000.0, channels, 20_000, BLOCK, &|p, c, t| match p {
                0 => noise(c, t),
                1 => rate,
                2 => depth,
                3 => feedback,
                4 => 0.0,
                _ => 1.0,
            });
            for (t, f) in out.out[0].iter().enumerate() {
                assert_eq!(f[0].to_bits(), noise(0, t).to_bits(), "{channels} ch, frame {t}");
                if channels == 2 {
                    assert_eq!(f[1].to_bits(), noise(1, t).to_bits(), "{channels} ch, frame {t}");
                }
            }
        }
    }
}

// PACK-35: the copy is read between samples, so it sweeps with no steps: a 1 kHz tone through the copy
// alone, swept over the whole range, keeps everything under 800 Hz and over 1250 Hz more than 80 dB
// under it, though its pitch wobbles by up to 3%; at 44.1, 48 and 96 kHz.
#[test]
fn it_sweeps_with_no_steps() {
    for sr in RATES {
        let out = play_at::<Flanger>(sr, 1, (0.1 * sr) as usize + (1 << 17), BLOCK, &|p, _, t| match p {
            0 => (0.5 * (TAU * 1000.0 * t as f64 / sr as f64).sin()) as f32,
            1 => 0.5,
            2 => 1.0,
            3 => 0.0,
            _ => 1.0,
        });
        let x: Vec<f32> = out.out[0][(0.1 * sr) as usize..].iter().map(|f| f[0]).collect();
        let spec = spectrum(&x, sr);
        let all: f64 = spec.iter().map(|s| s.1).sum();
        let away: f64 = spec.iter().filter(|s| s.0 < 800.0 || s.0 > 1250.0).map(|s| s.1).sum();
        let under = 10.0 * (away / all).log10();
        println!("{sr} Hz: away from the tone {under:.1} dB");
        assert!(under < -80.0, "{sr} Hz: away from the tone only {under:.1} dB under it");
    }
}

// PACK-35, ABI-13: its meter `phase` says its LFO's phase at each call's last frame, as it counts it:
// at 1 Hz, as many seconds into each cycle as the song has played, within a millionth of a cycle;
// stopped, it stays where it is.
#[test]
fn the_flanger_says_where_its_sweep_is() {
    let sr = 48_000.0;
    let played = play_at::<Flanger>(sr, 1, 3 * 48_000, BLOCK, &|p, _, t| match p {
        0 => 0.0,
        1 => 1.0,
        5 => (t < 100_000) as u8 as f32,
        _ => 0.5,
    });
    let phase = &played.out[1];
    // The phase as the kernel counts it: a step of rate over the song's rate added each frame the
    // song plays, in 64-bit floats.
    let (mut at, mut counted) = (0.0f64, vec![]);
    for t in 0..3 * 48_000 {
        counted.push(at);
        if t < 100_000 {
            at += 1.0 / 48_000.0;
            if at >= 1.0 {
                at -= 1.0;
            }
        }
    }
    for b in 0..3 * 48_000 / BLOCK {
        let last = b * BLOCK + BLOCK - 1;
        let (got, exact) = (phase[last][0], (last.min(100_000) as f64 / 48_000.0).fract());
        assert_eq!(got.to_bits(), (counted[last] as f32).to_bits(), "block {b}: phase {got}, as the kernel counts it {}", counted[last]);
        let off = (got as f64 - exact).abs();
        assert!(off.min(1.0 - off) < 1e-6, "block {b}: phase {got}, the song's {exact}");
        assert!(phase[b * BLOCK..=last].iter().all(|f| f[0] == phase[last][0]), "one number a call");
    }
}

// PACK-35, the house rules: a control that isn't finite counts as an end: NaN as its low end, ±∞ as
// the end it points to. Feedback NaN plays as −0.9, +∞ as 0.9; depth and mix NaN as 0, ∞ as 1.
#[test]
fn a_control_that_isnt_finite_counts_as_an_end() {
    let play = |depth: f32, feedback: f32, mix: f32| {
        play_at::<Flanger>(48_000.0, 2, 9_600, BLOCK, &|p, c, t| match p {
            0 => noise(c, t),
            1 => 2.0,
            2 => depth,
            3 => feedback,
            4 => mix,
            _ => 1.0,
        })
        .out
        .swap_remove(0)
    };
    let bits = |x: Vec<[f32; 2]>| x.iter().flat_map(|f| f.map(f32::to_bits)).collect::<Vec<u32>>();
    for (odd, end) in [(f32::NAN, -0.9), (f32::INFINITY, 0.9), (f32::NEG_INFINITY, -0.9)] {
        assert_eq!(bits(play(0.7, odd, 0.5)), bits(play(0.7, end, 0.5)), "feedback {odd} plays as {end}");
    }
    for (odd, end) in [(f32::NAN, 0.0), (f32::INFINITY, 1.0)] {
        assert_eq!(bits(play(odd, 0.5, 0.5)), bits(play(end, 0.5, 0.5)), "depth {odd} plays as {end}");
        assert_eq!(bits(play(0.7, 0.5, odd)), bits(play(0.7, 0.5, end)), "mix {odd} plays as {end}");
    }
}

// RENDER-28, PACK-35: once born, it allocates nothing while it plays, at 44.1, 48 and 96 kHz, in
// stereo: noise, every knob moving, stopping and playing.
#[test]
fn the_flanger_allocates_nothing_while_it_plays_at_every_rate() {
    for sr in RATES {
        let mut h = Host::<Flanger>::new(sr, BLOCK as u32, 2);
        let before = counting::here();
        for b in 0..2_000 {
            for k in 0..BLOCK {
                let t = b * BLOCK + k;
                h.port_mut(0)[k] = noise(0, t);
                h.port_mut(0)[BLOCK + k] = noise(1, t);
            }
            for (p, v) in [
                (1, 0.05 + (b % 9) as f32),
                (2, (b % 5) as f32 / 4.0),
                (3, (b % 7) as f32 / 3.0 - 1.0),
                (4, (b % 3) as f32 / 2.0),
                (5, (b % 4 != 0) as u8 as f32),
            ] {
                h.port_mut(p)[..BLOCK].fill(v);
            }
            h.process(BLOCK as u32);
        }
        assert_eq!(counting::here() - before, 0, "{sr} Hz: allocated while it played");
    }
}

// PACK-35, RENDER-6a: it says it is active for as long as anything it took in can still come out.
// Played a frame a call, a burst of noise that ends on one sample of 1, -1, 1e30 or the most a float
// holds, at Feedback ±0.9 with the copy at its longest, at 44.1, 48 and 96 kHz, mono and stereo:
// nothing comes out after the call where it first says it is done, and it says so within 5 s.
#[test]
fn nothing_comes_out_once_the_flanger_says_it_is_done() {
    for sr in RATES {
        let stop = (0.5 * sr) as usize;
        for last in [1.0f32, -1.0, 1e30, f32::MAX] {
            for feedback in [0.9f32, -0.9] {
                for channels in [1, 2] {
                    let played = play_at::<Flanger>(sr, channels, stop + (6.0 * sr) as usize, 1, &|p, c, t| match p {
                        0 if t + 2_000 < stop => 0.0,
                        0 if t < stop => 0.5 * noise(c, t),
                        0 if t == stop => last,
                        0 => 0.0,
                        1 => 1.0,
                        2 => 1.0,
                        3 => feedback,
                        4 => 0.5,
                        _ => (t < stop) as u8 as f32,
                    });
                    let out = &played.out[0];
                    let what = format!("{last} at {sr} Hz, feedback {feedback}, {channels} channels");
                    let done = (stop..out.len()).find(|&t| !played.active[t]).unwrap_or_else(|| panic!("{what}: it never says it is done"));
                    let heard = out.iter().rposition(|f| f[0] != 0.0 || f[1] != 0.0).expect("it played");
                    assert!(heard <= done, "{what}: sound at frame {heard}, after it said it was done at {done}");
                }
            }
        }
    }
}

// PACK-35: its knobs glide, so turning them never clicks: on a 300 Hz tone at 0.5, Feedback stepped
// from 0.5 to −0.5 and back (its copy turned over), Depth from 0 to 1 and back, and Mix from 0 to 1
// and back: what comes out never steps from one sample to the next by more than 0.06, where the tone
// itself steps by up to 0.02, and the comb at Feedback 0.5 makes it up to 1.5 times as loud (a
// knob that jumped would step by up to 0.5).
#[test]
fn turning_its_knobs_never_clicks() {
    let sr = 48_000.0;
    let every = 14_400;
    for knob in [3usize, 2, 4] {
        let played = play_at::<Flanger>(sr, 1, 8 * every, BLOCK, &|p, _, t| {
            let up = (t / every) % 2 == 1;
            match p {
                0 => (0.5 * (TAU * 300.0 * t as f64 / sr as f64).sin()) as f32,
                1 => 0.5,
                _ if p == knob => match (knob, up) {
                    (3, true) => -0.5,
                    (3, false) => 0.5,
                    (_, true) => 1.0,
                    (_, false) => 0.0,
                },
                2 => 0.7,
                3 => 0.5,
                4 => 0.5,
                _ => 1.0,
            }
        });
        let x = played.left();
        let step = max_step(&x, 1, x.len());
        println!("port {knob} stepped every 0.3 s: steps of up to {step:.4}");
        assert!(step < 0.06, "port {knob} stepped every 0.3 s: a step of {step}");
    }
}
