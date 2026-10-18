//! The stretch: a sine keeps its pitch at any length, the output is as long as the map says and
//! follows it, the identity map gives the source back, a sine stretched has no clicks, and a
//! transposition changes the pitch, never the time. Each plays in calls of an odd size too, so a
//! read across calls is checked.

use m2_kernel::{Host, OFFLINE, native};

use super::*;
use crate::testkit::{BLOCK, SR, play_cfg};

/// `src` (each channel as long) at `src_rate`, laid out by `map`, transposed by `semitones`, at
/// `rate`, for `seconds`, in calls of `split` frames: both channels.
fn stretch(src: Vec<Vec<f32>>, src_rate: f32, rate: f32, map: &[[f64; 2]], semitones: f64, seconds: f64, split: usize) -> Vec<[f32; 2]> {
    let mut played = play_cfg::<Stretch>(cfg(src, src_rate, rate, map, semitones), frames(seconds, rate), split, &|_, _, _| 0.0);
    played.out.remove(0)
}

fn cfg(src: Vec<Vec<f32>>, src_rate: f32, rate: f32, map: &[[f64; 2]], semitones: f64) -> Cfg {
    let asset = native::asset(src_rate, src);
    let (values, assets, time_map) = (Vec::leak(vec![semitones]), Vec::leak(vec![asset]), Vec::leak(map.to_vec()));
    Cfg { flags: OFFLINE, values, assets, time_map, ..Cfg::new(rate, BLOCK, 2) }
}

fn frames(seconds: f64, rate: f32) -> usize {
    (seconds * rate as f64).round() as usize
}

/// A sine at `hz`, at 0.5, for `seconds` at `rate`.
fn sine(hz: f64, seconds: f64, rate: f32) -> Vec<f32> {
    (0..frames(seconds, rate)).map(|i| (i as f64 / rate as f64 * hz * std::f64::consts::TAU).sin() as f32 * 0.5).collect()
}

fn left(x: &[[f32; 2]]) -> Vec<f32> {
    x.iter().map(|f| f[0]).collect()
}

/// The frequency of `x`, from its upward zero crossings, each placed between its two samples.
fn freq(x: &[f32], rate: f32) -> f64 {
    let ups: Vec<f64> =
        x.windows(2).enumerate().filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0).map(|(i, w)| i as f64 + (w[0] / (w[0] - w[1])) as f64).collect();
    match ups[..] {
        [first, .., last] => (ups.len() - 1) as f64 / ((last - first) / rate as f64),
        _ => 0.0,
    }
}

/// Where the sound of `x` ends, in seconds: its last sample over 0.001.
fn ends(x: &[f32], rate: f32) -> f64 {
    x.iter().rposition(|v| v.abs() > 1e-3).map_or(0.0, |i| (i + 1) as f64 / rate as f64)
}

/// The quietest 10 ms of `x`, by its peak: a hole shows.
fn quietest(x: &[f32], rate: f32) -> f32 {
    x.chunks(frames(0.01, rate)).map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs()))).fold(f32::MAX, f32::min)
}

// D6: a 440 Hz sine laid out at half, 0.8, 1.25 and twice its length is still 440 Hz, within 0.5%,
// with no hole in it.
#[test]
fn a_stretch_keeps_its_pitch() {
    let src = sine(440.0, 2.0, SR);
    for factor in [0.5, 0.8, 1.25, 2.0] {
        let len = 2.0 * factor;
        let out = left(&stretch(vec![src.clone()], SR, SR, &[[0.0, 0.0], [2.0, len]], 0.0, len, BLOCK));
        let middle = &out[frames(0.1, SR)..frames(len - 0.1, SR)];
        let f = freq(middle, SR);
        assert!((f / 440.0 - 1.0).abs() < 0.005, "{factor} times as long: {f:.2} Hz");
        assert!(quietest(middle, SR) > 0.4, "{factor} times as long: a hole in it");
    }
}

// D5: the output lasts as long as the map says, and follows it. A second of sound ends where the
// map puts the source's end; with a pair in the middle, the source's first quarter (440 Hz) is laid
// out three times as long and the rest (660 Hz) at its own speed after it.
#[test]
fn a_stretch_is_as_long_as_its_map_says() {
    let src = sine(440.0, 1.0, SR);
    for len in [0.6, 1.7] {
        let out = left(&stretch(vec![src.clone()], SR, SR, &[[0.0, 0.0], [1.0, len]], 0.0, len + 0.3, 37));
        let end = ends(&out, SR);
        assert!((end - len).abs() < 0.03, "a map to {len} s: the sound ends at {end:.3} s");
        assert!(quietest(&out[..frames(len - 0.03, SR)], SR) > 0.4, "a map to {len} s: a hole before its end");
    }
    let mut two = sine(440.0, 0.25, SR);
    two.extend(sine(660.0, 0.75, SR));
    let out = left(&stretch(vec![two], SR, SR, &[[0.0, 0.0], [0.25, 0.75], [1.0, 1.5]], 0.0, 1.8, BLOCK));
    let end = ends(&out, SR);
    assert!((end - 1.5).abs() < 0.03, "three pairs: the sound ends at {end:.3} s");
    let (a, b) = (freq(&out[frames(0.1, SR)..frames(0.65, SR)], SR), freq(&out[frames(0.85, SR)..frames(1.4, SR)], SR));
    assert!((a - 440.0).abs() < 2.0 && (b - 660.0).abs() < 3.0, "the first quarter, then the rest: {a:.1} Hz, {b:.1} Hz");
}

// D6: the identity map gives the source back, over 40 dB clean away from the ends, in both
// channels; so does no map, bit for bit; and how the host splits its calls changes no bit.
#[test]
fn the_identity_map_gives_the_source_back() {
    let mix = |parts: &[(f64, f32)]| -> Vec<f32> {
        (0..frames(1.0, SR))
            .map(|i| parts.iter().map(|&(hz, a)| a * (i as f64 / SR as f64 * hz * std::f64::consts::TAU).sin() as f32).sum())
            .collect()
    };
    let (l, r) = (mix(&[(220.0, 0.2), (331.7, 0.15), (523.3, 0.1)]), mix(&[(97.0, 0.3), (1234.5, 0.05)]));
    let out = stretch(vec![l.clone(), r.clone()], SR, SR, &[[0.0, 0.0], [1.0, 1.0]], 0.0, 1.0, BLOCK);
    for (c, want) in [l, r].iter().enumerate() {
        let (mut sig, mut err) = (0.0f64, 0.0f64);
        for i in FRAME..want.len() - FRAME {
            sig += (want[i] as f64).powi(2);
            err += (out[i][c] as f64 - want[i] as f64).powi(2);
        }
        let clean = 10.0 * (sig / err.max(1e-30)).log10();
        assert!(clean > 40.0, "channel {c}: {clean:.1} dB clean");
    }
    let src = || vec![mix(&[(220.0, 0.2)]), mix(&[(331.7, 0.2)])];
    let mapped = stretch(src(), SR, SR, &[[0.0, 0.0], [1.0, 1.0]], 0.0, 1.0, BLOCK);
    assert!(stretch(src(), SR, SR, &[], 0.0, 1.0, BLOCK) == mapped, "no map is the identity");
    assert!(stretch(src(), SR, SR, &[[0.0, 0.0], [1.0, 1.0]], 0.0, 1.0, 37) == mapped, "calls of 37 frames");
}

// D6: a sine stretched slower and faster never steps between two frames by more than the sine's own
// steepest step, give or take 15%: frames join without a click.
#[test]
fn a_stretch_has_no_clicks() {
    let src = sine(440.0, 2.0, SR);
    let steepest = 0.5 * std::f32::consts::TAU * 440.0 / SR;
    for factor in [0.73, 1.37] {
        let len = 2.0 * factor;
        let out = left(&stretch(vec![src.clone()], SR, SR, &[[0.0, 0.0], [2.0, len]], 0.0, len, 77));
        let (at, step) = out[..frames(len - 0.05, SR)]
            .windows(2)
            .enumerate()
            .map(|(i, w)| (i, (w[1] - w[0]).abs()))
            .fold((0, 0.0f32), |m, x| if x.1 > m.1 { x } else { m });
        assert!(step < 1.15 * steepest, "{factor} times as long: a step of {step:.4} at frame {at}, the sine's own is {steepest:.4}");
        assert!(quietest(&out[..frames(len - 0.05, SR)], SR) > 0.4, "{factor} times as long: it sounds all through");
    }
}

// D6, D7: with a map, a transposition changes the pitch and keeps the time, from a 44.1 kHz file
// into a 48 kHz song: up 7 semitones over a map half as long again, down an octave over one 0.75
// times as long. And a reset starts over.
#[test]
fn pitch_with_a_map_transposes_and_keeps_time() {
    let rate = 44_100.0;
    let src = sine(440.0, 2.0, rate);
    for (semitones, len) in [(7.0, 3.0), (-12.0, 1.5)] {
        let out = left(&stretch(vec![src.clone()], rate, SR, &[[0.0, 0.0], [2.0, len]], semitones, len + 0.3, BLOCK));
        let want = 440.0 * (semitones / 12.0f64).exp2();
        let f = freq(&out[frames(0.1, SR)..frames(len - 0.1, SR)], SR);
        assert!((f / want - 1.0).abs() < 0.005, "{semitones} semitones: {f:.2} Hz, not {want:.2}");
        let end = ends(&out, SR);
        assert!((end - len).abs() < 0.03, "{semitones} semitones: the sound ends at {end:.3} s, not {len}");
    }
    let mut host = Host::<Stretch>::of(cfg(vec![src], rate, SR, &[[0.0, 0.0], [2.0, 3.0]], 7.0));
    let take = |host: &mut Host<Stretch>| -> Vec<f32> {
        (0..300).flat_map(|_| {
            host.process(BLOCK as u32);
            host.port_mut(0)[..BLOCK].to_vec()
        })
        .collect()
    };
    let first = take(&mut host);
    host.reset();
    assert!(take(&mut host) == first, "a reset starts over");
}
