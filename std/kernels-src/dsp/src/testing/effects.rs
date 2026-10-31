//! The harness of every effect's native tests, and what every effect guarantees, as functions any
//! crate's tests call on its own effects: std's, and those of music2's own packs. So an effect that
//! moves from std into a pack keeps every test it had.

use m2_kernel::{Host, Kernel, PORT_CHANNELS};

pub use super::{BLOCK, EXTREMES, Input, SR};

/// Output port 0 of kernel `K` over `frames` samples, processed `split` frames at a time.
pub fn run<K: Kernel>(channels: usize, frames: usize, split: usize, input: Input<'_>) -> Vec<[f32; 2]> {
    let mut host = Host::<K>::new(SR, BLOCK as u32, channels as u32);
    let mut out = Vec::with_capacity(frames);
    let mut t = 0;
    while t < frames {
        let n = split.clamp(1, BLOCK).min(frames - t);
        for p in 0..K::INPUTS {
            let buf = host.port_mut(p);
            for c in 0..PORT_CHANNELS {
                for k in 0..n {
                    buf[c * BLOCK + k] = input(p, c, t + k);
                }
            }
        }
        host.process(n as u32);
        let o = host.port_mut(K::INPUTS);
        out.extend((0..n).map(|k| [o[k], o[BLOCK + k]]));
        t += n;
    }
    out
}

/// One effect under test: how to run it, and its controls' values.
pub struct Fx {
    pub name: &'static str,
    pub run: fn(usize, usize, usize, Input<'_>) -> Vec<[f32; 2]>,
    pub controls: &'static [f32],
}

/// Inputs: `audio(channel, t)` on port 0 and constant `controls` on the rest.
pub fn with<'a>(audio: &'a dyn Fn(usize, usize) -> f32, controls: &'a [f32]) -> impl Fn(usize, usize, usize) -> f32 + 'a {
    move |p, c, t| if p == 0 { audio(c, t) } else { controls[p - 1] }
}

/// Naive saws, full scale: 110 Hz on the left, 165 Hz on the right.
pub fn saw(c: usize, t: usize) -> f32 {
    let f = if c == 0 { 110.0 } else { 165.0 };
    2.0 * (t as f32 * f / SR).fract() - 1.0
}

/// Deterministic white noise in -1..1, different on each channel.
pub fn noise(c: usize, t: usize) -> f32 {
    let mut x = (t as u32).wrapping_mul(0x9E37_79B9) ^ (c as u32 + 1).wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    x as f32 / u32::MAX as f32 * 2.0 - 1.0
}

pub fn peak(x: &[[f32; 2]]) -> f32 {
    x.iter().fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

pub fn rms(x: &[[f32; 2]], c: usize) -> f32 {
    (x.iter().map(|f| f[c] as f64 * f[c] as f64).sum::<f64>() / x.len() as f64).sqrt() as f32
}

pub fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// Control settings to torture `fx` with: all at each extreme together, each alone at each
/// extreme with the rest as usual, and pseudo-random mixes of extremes.
pub fn torture(fx: &Fx) -> Vec<Vec<f32>> {
    super::torture(fx.controls, &[])
}

/// `fx` on `channels`: silence for `pre` samples, then `stopped` samples with `playing` (its last
/// input) at `wait` (and silence), then a saw. Played on through, `wait` is 1.
pub fn stopped(fx: &Fx, channels: usize, pre: usize, stopped: usize, wait: f32) -> Vec<[f32; 2]> {
    let n = fx.controls.len();
    let start = pre + stopped;
    (fx.run)(channels, start + 24_000, BLOCK, &|p, c, t| {
        if p == 0 {
            if t < start { 0.0 } else { saw(c, t - start) }
        } else if p == n && (pre..start).contains(&t) {
            wait
        } else {
            fx.controls[p - 1]
        }
    })
}

// ---------------------------------------------------------------- what every effect guarantees

/// Silence in, silence out, mono and stereo.
pub fn silence_in_silence_out<'a>(fxs: impl IntoIterator<Item = &'a Fx>) {
    for fx in fxs {
        for channels in [1, 2] {
            let out = (fx.run)(channels, 24_000, BLOCK, &with(&|_, _| 0.0, fx.controls));
            assert!(out.iter().all(|f| f[0] == 0.0 && f[1] == 0.0), "{} ({channels} ch) makes sound from silence", fx.name);
        }
    }
}

/// A 100 ms burst, then nothing: every effect settles to exact zeros (not a crawl through
/// denormals) a few seconds later.
pub fn tails_end_in_exact_silence<'a>(fxs: impl IntoIterator<Item = &'a Fx>) {
    let burst = |c, t| if t < 4_800 { saw(c, t) } else { 0.0 };
    for fx in fxs {
        let out = (fx.run)(2, 48_000 * 12, BLOCK, &with(&burst, fx.controls));
        let last = out.iter().rposition(|f| f[0] != 0.0 || f[1] != 0.0).unwrap_or(0);
        assert!(last < 48_000 * 10, "{} is still not silent {:.1} s after its input stopped", fx.name, last as f32 / SR);
    }
}

/// No setting, and no input sample no effect should ever see, makes a NaN or an infinity.
pub fn extreme_settings_and_broken_input_never_make_nan<'a>(fxs: impl IntoIterator<Item = &'a Fx>) {
    // Noise broken by samples no effect should ever see.
    let hostile = |c, t| match t % 3_001 {
        1_000 => f32::NAN,
        2_000 => f32::INFINITY,
        3_000 => -1e30,
        _ => noise(c, t),
    };
    for fx in fxs {
        for set in torture(fx) {
            for channels in [1, 2] {
                let out = (fx.run)(channels, 7_000, BLOCK, &with(&hostile, &set));
                let bad = out.iter().position(|f| !f[0].is_finite() || !f[1].is_finite());
                assert_eq!(bad, None, "{} ({channels} ch) with controls {set:?}", fx.name);
            }
        }
    }
}

/// At their starting settings, full-scale input stays near full scale; at any setting, below
/// +80 dBFS (the worst is three EQ bands boosting the same frequencies by 24 dB each, +72 dB).
pub fn output_stays_bounded<'a>(fxs: impl IntoIterator<Item = &'a Fx>) {
    let fxs: Vec<&Fx> = fxs.into_iter().collect();
    for fx in &fxs {
        for input in [saw as fn(usize, usize) -> f32, noise] {
            let p = peak(&(fx.run)(2, 48_000, BLOCK, &with(&input, fx.controls)));
            assert!(p < 2.5, "{} peaks at {p}", fx.name);
        }
    }
    for fx in &fxs {
        for set in torture(fx) {
            let p = peak(&(fx.run)(2, 7_000, BLOCK, &with(&noise, &set)));
            assert!(p < 1e4, "{} peaks at {p} with controls {set:?}", fx.name);
        }
    }
}

/// The same output however the host splits its blocks, and from run to run.
pub fn deterministic_whatever_the_block_size<'a>(fxs: impl IntoIterator<Item = &'a Fx>) {
    for fx in fxs {
        // Controls that jump around every 1000 samples, to exercise the smoothing too.
        let moving = |p: usize, c, t: usize| if p == 0 { noise(c, t) } else { fx.controls[p - 1] * (0.6 + 0.3 * ((t / 1_000) % 3) as f32) };
        for channels in [1, 2] {
            let reference = (fx.run)(channels, 12_000, BLOCK, &moving);
            assert!((fx.run)(channels, 12_000, BLOCK, &moving) == reference, "{} differs between runs", fx.name);
            for split in [1, 7, 100] {
                let out = (fx.run)(channels, 12_000, split, &moving);
                let diff = out.iter().zip(&reference).position(|(a, b)| a != b);
                assert_eq!(diff, None, "{} ({channels} ch) in blocks of {split} differs", fx.name);
            }
        }
    }
}

/// Each works on one channel alone, so it fits wherever a mono sound goes (a panned mixer
/// channel). The caller leaves out an effect that makes stereo (the widener).
pub fn mono_stays_mono<'a>(fxs: impl IntoIterator<Item = &'a Fx>) {
    for fx in fxs {
        let out = (fx.run)(1, 24_000, BLOCK, &with(&saw, fx.controls));
        assert!(peak(&out) > 0.1 && out.iter().all(|f| f[1] == 0.0), "{} doesn't process mono on its own", fx.name);
    }
}

/// At its starting settings, each changes a saw audibly.
pub fn every_effect_changes_the_sound<'a>(fxs: impl IntoIterator<Item = &'a Fx>) {
    let dry = |c, t| 0.5 * saw(c, t);
    for fx in fxs {
        let out = (fx.run)(2, 48_000, BLOCK, &with(&dry, fx.controls));
        let diff = out.iter().enumerate().map(|(t, f)| (f[0] - dry(0, t)).abs().max((f[1] - dry(1, t)).abs())).fold(0.0, f32::max);
        assert!(diff > 0.05, "{} barely changes a saw (by {diff})", fx.name);
    }
}

/// Of the effects that follow the song (their last input is `playing`): stopped (`playing` 0, or
/// NaN), each waits where it is (an LFO), or is put back at Play to where it was (the bitcrusher's
/// clock): played on, the effect is what it is with no stop at all, bit for bit. A fresh one that
/// sat stopped plays as one made at Play.
pub fn stopped_they_wait_and_play_on_where_they_were<'a>(fxs: impl IntoIterator<Item = &'a Fx>) {
    // 20 003 samples: no whole number of any of their cycles.
    let pause = 20_003;
    for fx in fxs {
        for channels in [1, 2] {
            // The bitcrusher's clock steps by exactly 1/4 at its starting settings (12 kHz on 48
            // kHz), so a stop after a whole number of 4 samples finds it at 1, where it starts:
            // putting back 1 at Play couldn't be told from putting back where it was. 30 001 stops
            // it at 1.25.
            for pre in [0, 30_000, 30_001] {
                let straight = stopped(fx, channels, pre, 0, 1.0);
                let free = stopped(fx, channels, pre, pause, 1.0);
                assert!(free[pre + pause..] != straight[pre..], "{}: what it did while stopped can't be heard", fx.name);
                for wait in [0.0, f32::NAN] {
                    let out = stopped(fx, channels, pre, pause, wait);
                    let diff =
                        out[pre + pause..].iter().zip(&straight[pre..]).position(|(a, b)| a.map(f32::to_bits) != b.map(f32::to_bits));
                    assert_eq!(diff, None, "{} ({channels} ch), after {pre} samples, stopped with playing {wait}", fx.name);
                }
            }
        }
    }
}
