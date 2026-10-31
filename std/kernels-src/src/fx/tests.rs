//! Native tests for the effects: first what every effect guarantees, then what each one
//! is for.

use m2_kernel::{Host, PORT_CHANNELS};

use super::*;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

/// `input(port, channel, t)`: every input sample a kernel sees.
type Input<'a> = &'a dyn Fn(usize, usize, usize) -> f32;

/// Output port 0 of kernel `K` over `frames` samples, processed `split` frames at a time.
fn run<K: Kernel>(channels: usize, frames: usize, split: usize, input: Input<'_>) -> Vec<[f32; 2]> {
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
struct Fx {
    name: &'static str,
    run: fn(usize, usize, usize, Input<'_>) -> Vec<[f32; 2]>,
    controls: &'static [f32],
}

/// Every effect at the settings the effect menu starts it with (the EQ gets some boost and
/// cut, since flat it does nothing), and playing: the last of chorus's, phaser's and tremolo's is
/// `playing`, 1. The bitcrusher's are `@m2/effects`' own (`packs/@m2/effects/kernels-src`).
const ALL: [Fx; 7] = [
    Fx { name: "reverb", run: run::<Reverb>, controls: &[0.6, 0.5, 0.25] },
    Fx { name: "chorus", run: run::<Chorus>, controls: &[0.8, 0.5, 0.5, 1.0] },
    Fx { name: "phaser", run: run::<Phaser>, controls: &[0.4, 0.7, 0.5, 0.5, 1.0] },
    Fx { name: "tremolo", run: run::<Tremolo>, controls: &[5.0, 0.5, 1.0] },
    Fx { name: "compress", run: run::<Compress>, controls: &[-18.0, 3.0, 0.01, 0.15, 4.0] },
    Fx { name: "eq3", run: run::<Eq3>, controls: &[4.0, -3.0, 5.0, 200.0, 1000.0, 4000.0] },
    Fx { name: "widen", run: run::<Widen>, controls: &[1.5] },
];

/// Inputs: `audio(channel, t)` on port 0 and constant `controls` on the rest.
fn with<'a>(audio: &'a dyn Fn(usize, usize) -> f32, controls: &'a [f32]) -> impl Fn(usize, usize, usize) -> f32 + 'a {
    move |p, c, t| if p == 0 { audio(c, t) } else { controls[p - 1] }
}

/// Naive saws, full scale: 110 Hz on the left, 165 Hz on the right.
fn saw(c: usize, t: usize) -> f32 {
    let f = if c == 0 { 110.0 } else { 165.0 };
    2.0 * (t as f32 * f / SR).fract() - 1.0
}

/// Deterministic white noise in -1..1, different on each channel.
fn noise(c: usize, t: usize) -> f32 {
    let mut x = (t as u32).wrapping_mul(0x9E37_79B9) ^ (c as u32 + 1).wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    x as f32 / u32::MAX as f32 * 2.0 - 1.0
}

fn peak(x: &[[f32; 2]]) -> f32 {
    x.iter().fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

fn rms(x: &[[f32; 2]], c: usize) -> f32 {
    (x.iter().map(|f| f[c] as f64 * f[c] as f64).sum::<f64>() / x.len() as f64).sqrt() as f32
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

// ---------------------------------------------------------------- every effect

#[test]
fn silence_in_silence_out() {
    for fx in &ALL {
        for channels in [1, 2] {
            let out = (fx.run)(channels, 24_000, BLOCK, &with(&|_, _| 0.0, fx.controls));
            assert!(out.iter().all(|f| f[0] == 0.0 && f[1] == 0.0), "{} ({channels} ch) makes sound from silence", fx.name);
        }
    }
}

#[test]
fn tails_end_in_exact_silence() {
    // A 100 ms burst, then nothing: every effect must settle to exact zeros (not a crawl
    // through denormals) a few seconds later.
    let burst = |c, t| if t < 4_800 { saw(c, t) } else { 0.0 };
    for fx in &ALL {
        let out = (fx.run)(2, 48_000 * 12, BLOCK, &with(&burst, fx.controls));
        let last = out.iter().rposition(|f| f[0] != 0.0 || f[1] != 0.0).unwrap_or(0);
        assert!(last < 48_000 * 10, "{} is still not silent {:.1} s after its input stopped", fx.name, last as f32 / SR);
    }
}

/// Values no control should break on.
const EXTREMES: [f32; 11] = [f32::NAN, f32::NEG_INFINITY, f32::MIN, -1e9, -1.0, 0.0, 1e-9, 0.5, 1e9, f32::MAX, f32::INFINITY];

/// Control settings to torture `fx` with: all at each extreme together, each alone at each
/// extreme with the rest as usual, and pseudo-random mixes of extremes.
fn torture(fx: &Fx) -> Vec<Vec<f32>> {
    let n = fx.controls.len();
    let mut sets: Vec<Vec<f32>> = EXTREMES.iter().map(|&v| vec![v; n]).collect();
    for k in 0..n {
        for &v in &EXTREMES {
            let mut s = fx.controls.to_vec();
            s[k] = v;
            sets.push(s);
        }
    }
    let mut seed = 1u32;
    for _ in 0..40 {
        sets.push(
            (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    EXTREMES[(seed >> 16) as usize % EXTREMES.len()]
                })
                .collect(),
        );
    }
    sets
}

#[test]
fn extreme_settings_and_broken_input_never_make_nan() {
    // Noise broken by samples no effect should ever see.
    let hostile = |c, t| match t % 3_001 {
        1_000 => f32::NAN,
        2_000 => f32::INFINITY,
        3_000 => -1e30,
        _ => noise(c, t),
    };
    for fx in &ALL {
        for set in torture(fx) {
            for channels in [1, 2] {
                let out = (fx.run)(channels, 7_000, BLOCK, &with(&hostile, &set));
                let bad = out.iter().position(|f| !f[0].is_finite() || !f[1].is_finite());
                assert_eq!(bad, None, "{} ({channels} ch) with controls {set:?}", fx.name);
            }
        }
    }
}

#[test]
fn output_stays_bounded() {
    // At their starting settings, full-scale input stays near full scale.
    for fx in &ALL {
        for input in [saw as fn(usize, usize) -> f32, noise] {
            let p = peak(&(fx.run)(2, 48_000, BLOCK, &with(&input, fx.controls)));
            assert!(p < 2.5, "{} peaks at {p}", fx.name);
        }
    }
    // At any setting, the worst is three EQ bands boosting the same frequencies by 24 dB
    // each (+72 dB).
    for fx in &ALL {
        for set in torture(fx) {
            let p = peak(&(fx.run)(2, 7_000, BLOCK, &with(&noise, &set)));
            assert!(p < 1e4, "{} peaks at {p} with controls {set:?}", fx.name);
        }
    }
}

#[test]
fn deterministic_whatever_the_block_size() {
    for fx in &ALL {
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

#[test]
fn mono_stays_mono() {
    // Everything but the widener works on one channel too, so it fits wherever a mono
    // sound goes (a panned mixer channel).
    for fx in ALL.iter().filter(|fx| fx.name != "widen") {
        let out = (fx.run)(1, 24_000, BLOCK, &with(&saw, fx.controls));
        assert!(peak(&out) > 0.1 && out.iter().all(|f| f[1] == 0.0), "{} doesn't process mono on its own", fx.name);
    }
}

#[test]
fn every_effect_changes_the_sound() {
    let dry = |c, t| 0.5 * saw(c, t);
    for fx in &ALL {
        let out = (fx.run)(2, 48_000, BLOCK, &with(&dry, fx.controls));
        let diff = out.iter().enumerate().map(|(t, f)| (f[0] - dry(0, t)).abs().max((f[1] - dry(1, t)).abs())).fold(0.0, f32::max);
        assert!(diff > 0.05, "{} barely changes a saw (by {diff})", fx.name);
    }
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
fn reverb_rings_on_and_then_lets_a_voice_end() {
    let mut host = Host::<Reverb>::new(SR, BLOCK as u32, 2);
    let mut tail = vec![];
    let mut ended = None;
    for b in 0..(48_000 * 12 / BLOCK) {
        for c in 0..2 {
            for k in 0..BLOCK {
                let t = b * BLOCK + k;
                host.port_mut(0)[c * BLOCK + k] = if t < 4_800 { noise(0, t) } else { 0.0 };
            }
        }
        for (p, v) in [(1, 0.6), (2, 0.5), (3, 0.5)] {
            host.port_mut(p).fill(v);
        }
        host.process(BLOCK as u32);
        let o = host.port_mut(Reverb::INPUTS);
        tail.extend((0..BLOCK).map(|k| [o[k], o[BLOCK + k]]));
        if ended.is_none() && !host.active() {
            ended = Some(b * BLOCK);
        }
    }
    let s = |x: f32| (x * SR) as usize;
    assert!(peak(&tail[s(0.3)..s(0.6)]) > 1e-2, "no tail after the input stopped");
    assert!(tail[s(0.3)..].iter().any(|f| (f[0] - f[1]).abs() > 1e-3), "a centred sound should get a stereo tail");
    let ended = ended.expect("the reverb never stops being active");
    assert!(ended > s(0.6) && ended < s(10.0), "active until {:.2} s", ended as f32 / SR);
    assert!(peak(&tail[ended..]) < 1e-4, "still audible after it stopped being active");
}

#[test]
fn reverb_wet_level_and_size() {
    // Fully wet on sustained noise, it's about as loud as the dry sound.
    let wet = (ALL[0].run)(2, 96_000, BLOCK, &with(&noise, &[0.6, 0.5, 1.0]));
    let ratio = db(rms(&wet[48_000..], 0) / rms(&(0..48_000).map(|t| [noise(0, t), 0.0]).collect::<Vec<_>>(), 0));
    assert!((-6.0..3.0).contains(&ratio), "fully wet is {ratio:.1} dB from dry");
    // A bigger room rings longer.
    let burst = |c, t| if t < 4_800 { noise(c, t) } else { 0.0 };
    let tail = |size| rms(&(ALL[0].run)(2, 72_000, BLOCK, &with(&burst, &[size, 0.5, 1.0]))[48_000..], 0);
    assert!(tail(0.9) > 4.0 * tail(0.3), "size 0.9 tail {} vs size 0.3 tail {}", tail(0.9), tail(0.3));
}

#[test]
fn chorus_spreads_a_centred_sound_at_the_same_level() {
    let mono = |_, t| 0.5 * saw(0, t);
    let out = (ALL[1].run)(2, 48_000, BLOCK, &with(&mono, ALL[1].controls));
    let width = out.iter().map(|f| (f[0] - f[1]).abs()).fold(0.0, f32::max);
    assert!(width > 0.05, "left and right only differ by {width}");
    let dry = rms(&(0..48_000).map(|t| [mono(0, t), 0.0]).collect::<Vec<_>>(), 0);
    let level = db(rms(&out, 0) / dry);
    assert!(level.abs() < 3.0, "chorus changes the level by {level:.1} dB");
}

#[test]
fn phaser_is_an_allpass_until_mixed_with_the_dry_sound() {
    let dry = rms(&(0..48_000).map(|t| [noise(0, t), 0.0]).collect::<Vec<_>>(), 0);
    let wet = (ALL[2].run)(1, 48_000, BLOCK, &with(&noise, &[0.4, 0.7, 0.0, 1.0, 1.0]));
    let level = db(rms(&wet, 0) / dry);
    assert!(level.abs() < 1.0, "fully wet without feedback is {level:.1} dB from dry");
    let mixed = (ALL[2].run)(1, 48_000, BLOCK, &with(&noise, &[0.4, 0.7, 0.0, 0.5, 1.0]));
    let level = db(rms(&mixed, 0) / dry);
    assert!(level < -1.5, "the notches only take away {level:.1} dB");
}

#[test]
fn tremolo_dips_by_its_depth() {
    let dc = |_, _| 0.5;
    let out = (ALL[3].run)(1, 48_000, BLOCK, &with(&dc, &[5.0, 1.0, 1.0]));
    let (lo, hi) = out.iter().fold((f32::MAX, 0.0f32), |(lo, hi), f| (lo.min(f[0]), hi.max(f[0])));
    assert!(lo < 0.005 && hi > 0.499, "level swings between {lo} and {hi}");
    // Five dips a second.
    let dips = out.windows(3).filter(|w| w[1][0] < w[0][0] && w[1][0] <= w[2][0]).count();
    assert_eq!(dips, 5);
    let still = (ALL[3].run)(1, 4_800, BLOCK, &with(&dc, &[5.0, 0.0, 1.0]));
    assert!(still.iter().all(|f| f[0] == 0.5), "depth 0 changes the sound");
}

#[test]
fn compressor_follows_its_curve() {
    let fx = ALL[4].run;
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
    let fx = ALL[5].run;
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
fn widen_keeps_mono_compatibility() {
    let fx = ALL[6].run;
    let same = fx(1, 4_800, BLOCK, &with(&saw, &[1.0]));
    assert!(
        same.iter().enumerate().all(|(t, f)| (f[0] - saw(0, t)).abs() < 1e-6 && (f[1] - saw(1, t)).abs() < 1e-6),
        "width 1 changes the sound"
    );
    let mono = fx(1, 4_800, BLOCK, &with(&saw, &[0.0]));
    assert!(mono.iter().all(|f| f[0] == f[1]), "width 0 isn't mono");
    // A mono input gets width, and the added side cancels when summed back to mono.
    let center = |_, t| 0.5 * saw(0, t);
    let wide = fx(1, 48_000, BLOCK, &with(&center, &[2.0]));
    let width = wide.iter().map(|f| (f[0] - f[1]).abs()).fold(0.0, f32::max);
    assert!(width > 0.1, "a mono input only gets {width} of side");
    assert!(wide.iter().enumerate().all(|(t, f)| ((f[0] + f[1]) * 0.5 - center(0, t)).abs() < 1e-6), "not mono compatible");
}

#[test]
fn pan_places_a_mono_sound_and_balances_a_stereo_one() {
    use std::f32::consts::SQRT_2;

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
const FOLLOW: [&str; 3] = ["chorus", "phaser", "tremolo"];

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

/// `fx` on `channels`: silence for `pre` samples, then `stopped` samples with `playing` at `wait`
/// (and silence), then a saw. Played on through, `wait` is 1.
fn stopped(fx: &Fx, channels: usize, pre: usize, stopped: usize, wait: f32) -> Vec<[f32; 2]> {
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

// Stopped (`playing` 0, or NaN), each LFO waits where it is: played on, the effect is what it is
// with no stop at all, bit for bit. A fresh one that sat stopped plays as one made at Play.
#[test]
fn stopped_the_effects_wait_and_play_on_where_they_were() {
    // 20 003 samples: no whole number of any of their cycles.
    let pause = 20_003;
    for fx in ALL.iter().filter(|fx| FOLLOW.contains(&fx.name)) {
        for channels in [1, 2] {
            // Stops a sample apart, as @m2/effects' bitcrusher is tried (its clock steps by 1/4).
            for pre in [0, 30_000, 30_001] {
                let straight = stopped(fx, channels, pre, 0, 1.0);
                let free = stopped(fx, channels, pre, pause, 1.0);
                assert!(free[pre + pause..] != straight[pre..], "{}: what its LFO did while stopped can't be heard", fx.name);
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
