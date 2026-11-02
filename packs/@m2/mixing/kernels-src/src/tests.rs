//! Native tests for the pack's kernels (PACK-26): what every effect guarantees (m2-dsp's harness,
//! which std's effects are held to too), then what each tool promises, measured.

use std::f64::consts::{PI, SQRT_2};

use m2_dsp::testing::effects::{self, BLOCK, Fx, SR, noise, run, saw, torture, with};
use m2_dsp::testing::{Input, defaults_in, high_rise, s};
use m2_kernel::{Host, Kernel};

use super::*;

const MANIFEST: &str = include_str!("../../m2pkg.toml");

// ---------------------------------------------------------------- the kernels' ports

const EQ_FIELDS: [&str; 5] = ["shape", "freq", "gain", "q", "on"];
const GATE: [&str; 7] = ["in", "key", "threshold", "attack", "hold", "release", "range"];
const TRANSIENT: [&str; 3] = ["in", "attack", "sustain"];
const DEESSER: [&str; 4] = ["in", "freq", "threshold", "amount"];

fn eq_ports() -> Vec<String> {
    let mut names = vec!["in".to_string()];
    for b in 1..=8 {
        names.extend(EQ_FIELDS.iter().map(|f| format!("{f}{b}")));
    }
    names
}

fn multiband_ports() -> Vec<String> {
    let mut names = vec!["in".to_string(), "low_mid".to_string(), "mid_high".to_string()];
    for band in ["low", "mid", "high"] {
        names.extend(["threshold", "ratio", "attack", "release", "gain"].iter().map(|f| format!("{band}_{f}")));
    }
    names
}

/// A kernel's controls at the manifest's defaults (its inputs after the audio ones), its port names
/// checked against the manifest's order.
fn defaults(kernel: &str, names: &[String], audio: usize) -> Vec<f32> {
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    defaults_in(MANIFEST, kernel, &names)[audio..].to_vec()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

// The code reads its ports in the manifest's order, as many as it says.
#[test]
fn each_kernels_ports_are_its_manifests() {
    assert_eq!(defaults("eq", &eq_ports(), 0).len(), Eq::INPUTS);
    assert_eq!(defaults("gate", &strings(&GATE), 0).len(), Gate::INPUTS);
    assert_eq!(defaults("transient", &strings(&TRANSIENT), 0).len(), Transient::INPUTS);
    assert_eq!(defaults("deesser", &strings(&DEESSER), 0).len(), Deesser::INPUTS);
    assert_eq!(defaults("multiband", &multiband_ports(), 0).len(), Multiband::INPUTS);
}

// ---------------------------------------------------------------- every effect

/// The gate keyed by its own input, as `gate(x)` plays it: its key is its audio, and the harness's
/// controls are the rest.
fn run_gate(channels: usize, frames: usize, split: usize, input: Input<'_>) -> Vec<[f32; 2]> {
    run::<Gate>(channels, frames, split, &|p, c, t| match p {
        0 | 1 => input(0, c, t),
        p => input(p - 1, c, t),
    })
}

fn leak(v: Vec<f32>) -> &'static [f32] {
    Box::leak(v.into_boxed_slice())
}

/// Each tool at the values + Effect starts it with (its descriptor's `insert` over its defaults).
fn all() -> Vec<Fx> {
    let mut transient = defaults("transient", &strings(&TRANSIENT), 1);
    transient[0] = 6.0;
    vec![
        Fx { name: "eq", run: run::<Eq>, controls: leak(defaults("eq", &eq_ports(), 1)) },
        Fx { name: "gate", run: run_gate, controls: leak(defaults("gate", &strings(&GATE), 2)) },
        Fx { name: "transient", run: run::<Transient>, controls: leak(transient) },
        Fx { name: "deesser", run: run::<Deesser>, controls: leak(defaults("deesser", &strings(&DEESSER), 1)) },
        Fx { name: "multiband", run: run::<Multiband>, controls: leak(defaults("multiband", &multiband_ports(), 1)) },
    ]
}

#[test]
fn silence_in_silence_out() {
    effects::silence_in_silence_out(&all());
}

#[test]
fn tails_end_in_exact_silence() {
    effects::tails_end_in_exact_silence(&all());
}

#[test]
fn extreme_settings_and_broken_input_never_make_nan() {
    effects::extreme_settings_and_broken_input_never_make_nan(&all());
}

// The EQ too (review Z8): eight bands boosting one frequency are capped just under +80 dBFS.
#[test]
fn output_stays_bounded() {
    effects::output_stays_bounded(&all());
}

#[test]
fn deterministic_whatever_the_block_size() {
    effects::deterministic_whatever_the_block_size(&all());
}

#[test]
fn mono_stays_mono() {
    effects::mono_stays_mono(&all());
}

// The EQ starts flat; with a bell up 6 dB it changes a saw as the others do at their start.
#[test]
fn every_effect_changes_the_sound() {
    let mut fxs: Vec<Fx> = all().into_iter().filter(|fx| fx.name != "eq").collect();
    let mut boosted = defaults("eq", &eq_ports(), 1);
    boosted[eq(4, "gain")] = 6.0;
    fxs.push(Fx { name: "eq, a bell up 6 dB", run: run::<Eq>, controls: leak(boosted) });
    effects::every_effect_changes_the_sound(&fxs);
}

// ---------------------------------------------------------------- measuring

/// The window measurements are taken over: 2^15 samples, so a tone on one of its bins is a whole
/// number of cycles in it.
const N: usize = 1 << 15;

/// The frequency of bin `k` of the window.
fn bin_hz(k: usize) -> f64 {
    k as f64 * SR as f64 / N as f64
}

/// The bin nearest `hz`.
fn bin_of(hz: f64) -> usize {
    (hz * N as f64 / SR as f64).round() as usize
}

/// The amplitude of bin `k` in `x` (whole cycles, so a tone on that bin is measured exactly).
fn amplitude(x: &[f32], k: usize) -> f64 {
    assert_eq!(x.len(), N);
    let w = 2.0 * PI * k as f64 / N as f64;
    let (re, im) = x
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(re, im), (n, &v)| (re + v as f64 * (w * n as f64).cos(), im - v as f64 * (w * n as f64).sin()));
    2.0 * (re * re + im * im).sqrt() / N as f64
}

/// A sum of tones of `amp` each on bins `bins`, the same on both channels.
fn tones(bins: &[usize], amp: f32) -> impl Fn(usize, usize) -> f32 + '_ {
    move |_, t| bins.iter().map(|&k| amp * (2.0 * PI * k as f64 * t as f64 / N as f64).sin() as f32).sum()
}

fn left(x: &[[f32; 2]]) -> Vec<f32> {
    x.iter().map(|f| f[0]).collect()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len() as f64).sqrt()
}

/// How much louder `out` is than `dry` over `range`, in dB.
fn gain_db(out: &[f32], dry: &[f32], range: std::ops::Range<usize>) -> f64 {
    db(rms(&out[range.clone()]) / rms(&dry[range]))
}

// ---------------------------------------------------------------- the EQ

/// The control of band `band` (1 to 8) named `field`.
fn eq(band: usize, field: &str) -> usize {
    (band - 1) * 5 + EQ_FIELDS.iter().position(|f| *f == field).unwrap()
}

const SHAPES: [&str; 5] = ["low cut", "low shelf", "bell", "high shelf", "high cut"];

/// The EQ with every band off but `band`, which is `shape` at `hz`, `gain` dB and `q`.
fn one_band(band: usize, shape: usize, hz: f32, gain: f32, q: f32) -> Vec<f32> {
    let mut c = defaults("eq", &eq_ports(), 1);
    for b in 1..=8 {
        c[eq(b, "on")] = 0.0;
    }
    c[eq(band, "shape")] = shape as f32;
    c[eq(band, "freq")] = hz;
    c[eq(band, "gain")] = gain;
    c[eq(band, "q")] = q;
    c[eq(band, "on")] = 1.0;
    c
}

/// The gain of the RBJ cookbook biquad of `shape` at `hz`, in dB (Robert Bristow-Johnson's "Cookbook
/// formulae for audio EQ biquad filter coefficients"): the oracle the EQ is held to.
fn rbj_db(shape: usize, f0: f64, gain: f64, q: f64, hz: f64) -> f64 {
    let w0 = 2.0 * PI * f0 / SR as f64;
    let (c, alpha, a) = (w0.cos(), w0.sin() / (2.0 * q), 10f64.powf(gain / 40.0));
    let sa = 2.0 * a.sqrt() * alpha;
    let (b, d) = match shape {
        0 => ([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
        1 => (
            [a * ((a + 1.0) - (a - 1.0) * c + sa), 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - sa)],
            [(a + 1.0) + (a - 1.0) * c + sa, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - sa],
        ),
        2 => ([1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a], [1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a]),
        3 => (
            [a * ((a + 1.0) + (a - 1.0) * c + sa), -2.0 * a * ((a - 1.0) + (a + 1.0) * c), a * ((a + 1.0) + (a - 1.0) * c - sa)],
            [(a + 1.0) - (a - 1.0) * c + sa, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - sa],
        ),
        _ => ([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
    };
    let w = 2.0 * PI * hz / SR as f64;
    let at = |k: [f64; 3]| {
        let re = k[0] + k[1] * w.cos() + k[2] * (2.0 * w).cos();
        let im = -k[1] * w.sin() - k[2] * (2.0 * w).sin();
        (re * re + im * im).sqrt()
    };
    db(at(b) / at(d))
}

/// What the EQ with `controls` does to each tone on `bins`, measured once it has settled: dB each.
fn eq_response(controls: &[f32], bins: &[usize]) -> Vec<f64> {
    let amp = 0.02;
    let out = run::<Eq>(1, 2 * N, BLOCK, &with(&tones(bins, amp), controls));
    let out = left(&out[N..]);
    bins.iter().map(|&k| db(amplitude(&out, k) / amp as f64)).collect()
}

// PACK-26: each band is the RBJ cookbook biquad of its shape, measured within 0.1 dB, from two
// octaves below its frequency to two above.
#[test]
fn the_eq_is_rbjs_response() {
    let mut worst = (0.0, String::new());
    for (shape, name) in SHAPES.iter().enumerate() {
        for f0 in [80.0, 1000.0, 7000.0] {
            for gain in [-12.0, 9.0] {
                for q in [0.5, 0.71, 2.0, 6.0] {
                    let bins: Vec<usize> =
                        [0.25, 0.5, 0.71, 1.0, 1.41, 2.0, 4.0].iter().map(|r| bin_of(f0 * r)).filter(|&k| bin_hz(k) < 20_000.0).collect();
                    let got = eq_response(&one_band(3, shape, f0 as f32, gain as f32, q as f32), &bins);
                    for (&k, got) in bins.iter().zip(got) {
                        let want = rbj_db(shape, f0, gain, q, bin_hz(k));
                        let off = (got - want).abs();
                        let what = format!("{name} at {f0} Hz, {gain} dB, q {q}: at {:.0} Hz {got:.3} dB, RBJ's {want:.3} dB", bin_hz(k));
                        assert!(off < 0.1, "{what}");
                        if off > worst.0 {
                            worst = (off, what);
                        }
                    }
                }
            }
        }
    }
    println!("the farthest from RBJ's: {:.4} dB ({})", worst.0, worst.1);
}

// PACK-26: a bell's gain at its frequency is its gain, within 0.1 dB, on every band.
#[test]
fn a_bells_gain_at_its_frequency_is_its_gain() {
    for band in 1..=8 {
        for hz in [50.0, 300.0, 1000.0, 5000.0, 12_000.0] {
            let k = bin_of(hz);
            for gain in [-18.0, -6.0, 3.0, 12.0, 24.0] {
                for q in [0.3, 1.0, 8.0] {
                    let got = eq_response(&one_band(band, 2, bin_hz(k) as f32, gain, q), &[k])[0];
                    assert!((got - gain as f64).abs() < 0.1, "band {band}, a bell at {hz} Hz, {gain} dB, q {q}: {got:.3} dB");
                }
            }
        }
    }
}

// Each band's menu picks its shape: every band, every shape, measured where shapes differ.
#[test]
fn every_band_plays_every_shape() {
    let bins = [bin_of(250.0), bin_of(1000.0), bin_of(4000.0)];
    for band in 1..=8 {
        for (shape, name) in SHAPES.iter().enumerate() {
            let got = eq_response(&one_band(band, shape, 1000.0, 6.0, 0.71), &bins);
            for (&k, got) in bins.iter().zip(got) {
                let want = rbj_db(shape, 1000.0, 6.0, 0.71, bin_hz(k));
                assert!((got - want).abs() < 0.1, "band {band} as a {name}: at {:.0} Hz {got:.3} dB, RBJ's {want:.3}", bin_hz(k));
            }
        }
    }
}

// PACK-26: it starts flat, changing no bit: the cuts off, the other bands at 0 dB. Every band off
// is flat too, whatever its gain.
#[test]
fn the_eq_starts_flat_bit_for_bit() {
    let start = defaults("eq", &eq_ports(), 1);
    let mut off = start.clone();
    for b in 1..=8 {
        off[eq(b, "on")] = 0.0;
        off[eq(b, "gain")] = 12.0;
        off[eq(b, "freq")] = 300.0 * b as f32;
    }
    for (what, controls) in [("at its start", &start), ("every band off", &off)] {
        for channels in [1, 2] {
            let out = run::<Eq>(channels, 24_000, BLOCK, &with(&noise, controls));
            let diff = out
                .iter()
                .enumerate()
                .position(|(t, f)| f[0].to_bits() != noise(0, t).to_bits() || (channels == 2 && f[1].to_bits() != noise(1, t).to_bits()));
            assert_eq!(diff, None, "the EQ {what} ({channels} ch) changes the sound");
        }
    }
}

// PACK-26: a change of any band's value glides, so it never clicks: a tone through the EQ while its
// gain, frequency, Q, shape and switch jump, measured as std's synth tests measure a click (power
// above 8 kHz just after the jump against before). The same jump of the tone's own level, unsmoothed,
// is a click this measure hears.
#[test]
fn no_eq_change_clicks() {
    let tone = |_: usize, t: usize| 0.5 * (2.0 * PI * 220.0 * t as f64 / SR as f64).sin() as f32;
    let start = one_band(3, 2, 250.0, 0.0, 1.0);
    let at = [s(0.3), s(0.6), s(0.9), s(1.2), s(1.5), s(1.8)];
    let controls = |t: usize| {
        let mut c = start.clone();
        if t >= at[0] {
            c[eq(3, "gain")] = 18.0;
        }
        if t >= at[1] {
            c[eq(3, "freq")] = 4000.0;
        }
        if t >= at[2] {
            c[eq(3, "q")] = 10.0;
        }
        if t >= at[3] {
            c[eq(3, "shape")] = 4.0;
        }
        if t >= at[4] {
            c[eq(3, "on")] = 0.0;
        }
        if t >= at[5] {
            c[eq(3, "on")] = 1.0;
        }
        c
    };
    let out = left(&run::<Eq>(1, s(2.1), BLOCK, &|p, c, t| if p == 0 { tone(c, t) } else { controls(t)[p - 1] }));
    let words = [
        "the gain from 0 to 18 dB",
        "the frequency from 250 Hz to 4 kHz",
        "the q from 1 to 10",
        "the shape from bell to high cut",
        "the band off",
        "the band on",
    ];
    for (&t, what) in at.iter().zip(words) {
        let rise = high_rise(&out, t);
        assert!(rise < 3.0, "{what}: {rise:.1} dB more above 8 kHz");
    }
    let jumped: Vec<f32> = (0..s(0.5)).map(|t| tone(0, t) * if t >= s(0.3) { 8.0 } else { 1.0 }).collect();
    assert!(high_rise(&jumped, s(0.3)) > 20.0, "an unsmoothed jump: {:.1} dB", high_rise(&jumped, s(0.3)));
}

/// A tone of `hz` at a quarter of full scale through the EQ, its controls `before` until `at` (a
/// whole number of blocks) and `after` from there, for `frames`: its first channel.
fn eq_jump(before: &[f32], after: &[f32], at: usize, frames: usize, hz: f64) -> Vec<f32> {
    let mut host = Host::<Eq>::new(SR, BLOCK as u32, 1);
    let mut out = Vec::with_capacity(frames);
    for start in (0..frames).step_by(BLOCK) {
        let controls = if start < at { before } else { after };
        for (p, &v) in controls.iter().enumerate() {
            host.port_mut(p + 1).fill(v);
        }
        for (k, x) in host.port_mut(0)[..BLOCK].iter_mut().enumerate() {
            *x = (0.25 * (2.0 * PI * hz * (start + k) as f64 / SR as f64).sin()) as f32;
        }
        host.process(BLOCK as u32);
        out.extend_from_slice(&host.port_mut(Eq::INPUTS)[..BLOCK]);
    }
    out
}

/// How far, in dB, the EQ's output while it moves from `before` to `after` goes over the louder of
/// its settled peaks before and after, the worst of tones from a half to twice `hz`; and which tone.
fn overshoot(before: &[f32], after: &[f32], hz: f32) -> (f64, f64) {
    let (at, end) = (s(0.5) / BLOCK * BLOCK, s(3.0));
    let mut worst = (f64::MIN, 0.0);
    for r in [0.5, 0.71, 0.95, 1.0, 1.05, 1.41, 2.0] {
        let tone = hz as f64 * r;
        let out = eq_jump(before, after, at, end, tone);
        let peak = |r: std::ops::Range<usize>| out[r].iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
        let settled = peak(s(0.3)..at).max(peak(s(2.5)..end));
        let over = db(peak(at..s(2.5)) / settled);
        if over > worst.0 {
            worst = (over, tone);
        }
    }
    worst
}

// PACK-26: a shape change goes through off, so it never booms: every pair of shapes, at every Q up
// to 18, gains of 12 and 24 dB, at 150 Hz and 1 kHz, over tones from a half to twice the band's
// frequency, peaks within 1 dB of the louder settled level.
#[test]
fn no_shape_change_booms() {
    let mut worst = (f64::MIN, String::new());
    for q in [0.71, 2.0, 4.0, 18.0] {
        for gain in [12.0, 24.0] {
            for hz in [150.0, 1000.0] {
                for (from, from_name) in SHAPES.iter().enumerate() {
                    for to in (0..5).filter(|&to| to != from) {
                        let (before, after) = (one_band(3, from, hz, gain, q), one_band(3, to, hz, gain, q));
                        let (over, tone) = overshoot(&before, &after, hz);
                        let what = format!(
                            "{} to {} at {hz} Hz, {gain} dB, q {q}: {over:+.2} dB over settled (a {tone:.0} Hz tone)",
                            from_name, SHAPES[to]
                        );
                        assert!(over < 1.0, "{what}");
                        if over > worst.0 {
                            worst = (over, what);
                        }
                    }
                }
            }
        }
    }
    println!("the worst shape change: {}", worst.1);
}

// PACK-26 (review Z2): a band moves to a new gain, Q or frequency by crossfading to a settled
// filter there, so no jump of them booms: shelves and a bell at every Q up to 18, the gain from 0 to
// 24 dB, -24 to 24 and back, -12 to 12, and small steps of 1 and 2 dB that settle for less; every
// shape's Q from 0.5 to 18 and back, and its frequency a quarter octave and a whole octave and a half
// away; at 150 Hz and 1 kHz: within 1 dB of the louder settled level.
#[test]
fn no_jump_of_a_bands_values_booms() {
    let mut worst = (f64::MIN, String::new());
    for shape in [1, 2, 3, 0, 4] {
        for hz in [150.0, 1000.0] {
            let mut jumps = vec![];
            for q in [0.71, 2.0, 4.0, 18.0] {
                for (g0, g1) in [(0.0, 24.0), (-24.0, 24.0), (24.0, -24.0), (-12.0, 12.0), (0.0, 1.0), (0.0, 2.0)] {
                    if shape != 0 && shape != 4 {
                        jumps.push((format!("gain {g0} to {g1} dB, q {q}"), one_band(3, shape, hz, g0, q), one_band(3, shape, hz, g1, q)));
                    }
                }
            }
            for (q0, q1) in [(0.5, 18.0), (18.0, 0.5)] {
                jumps.push((format!("q {q0} to {q1}, 12 dB"), one_band(3, shape, hz, 12.0, q0), one_band(3, shape, hz, 12.0, q1)));
            }
            for (q, octaves) in [(4.0, 0.25), (4.0, -1.5), (18.0, 0.25)] {
                let to = hz * 2f32.powf(octaves);
                jumps.push((format!("{hz} to {to:.0} Hz, 12 dB, q {q}"), one_band(3, shape, hz, 12.0, q), one_band(3, shape, to, 12.0, q)));
            }
            for (what, before, after) in jumps {
                let (over, tone) = overshoot(&before, &after, hz);
                let what = format!("a {} at {hz} Hz, {what}: {over:+.2} dB over settled (a {tone:.0} Hz tone)", SHAPES[shape]);
                assert!(over < 1.0, "{what}");
                if over > worst.0 {
                    worst = (over, what);
                }
            }
        }
    }
    println!("the worst jump: {}", worst.1);
}

// PACK-26: a small move, a lane's line or a knob's drag, is followed at once, as std's eq3 follows
// it: a bell's gain drawn from 0 to 12 dB over half a second is +6 dB halfway, and +12 dB as the line
// ends.
#[test]
fn a_lanes_line_is_followed_at_once() {
    let tone = |_: usize, t: usize| (0.25 * (2.0 * PI * 1000.0 * t as f64 / SR as f64).sin()) as f32;
    let start = one_band(3, 2, 1000.0, 0.0, 1.0);
    let gain = eq(3, "gain");
    let out = left(&run::<Eq>(1, s(0.6), BLOCK, &|p, c, t| {
        if p == 0 {
            tone(c, t)
        } else if p - 1 == gain {
            12.0 * (t as f32 / s(0.5) as f32).min(1.0)
        } else {
            start[p - 1]
        }
    }));
    let dry: Vec<f32> = (0..s(0.6)).map(|t| tone(0, t)).collect();
    let halfway = gain_db(&out, &dry, s(0.245)..s(0.255));
    let ended = gain_db(&out, &dry, s(0.5)..s(0.51));
    assert!((halfway - 6.0).abs() < 0.3 && (ended - 12.0).abs() < 0.1, "halfway {halfway:.2} dB, as the line ends {ended:.2} dB");
}

// ---------------------------------------------------------------- the gate

/// A tone of `hz` whose peak is `level` dBFS.
fn tone_at(hz: f64, level: f64) -> impl Fn(usize) -> f32 {
    let amp = 10f64.powf(level / 20.0);
    move |t| (amp * (2.0 * PI * hz * t as f64 / SR as f64).sin()) as f32
}

/// The gate's controls: threshold, attack, hold, release and range.
fn gate(threshold: f32, attack: f32, hold: f32, release: f32, range: f32) -> [f32; 5] {
    [threshold, attack, hold, release, range]
}

/// The gate playing `x`, keyed by `key`, on `channels`.
fn gated(channels: usize, frames: usize, x: &dyn Fn(usize) -> f32, key: &dyn Fn(usize) -> f32, c: [f32; 5]) -> Vec<f32> {
    left(&run::<Gate>(channels, frames, BLOCK, &|p, _, t| match p {
        0 => x(t),
        1 => key(t),
        p => c[p - 2],
    }))
}

// PACK-26: closed, its output is its input at `range` dB.
#[test]
fn a_closed_gate_is_its_range() {
    let x = tone_at(1000.0, -60.0);
    for range in [-80.0, -40.0, -12.0, 0.0] {
        for channels in [1, 2] {
            let out = gated(channels, s(0.5), &x, &x, gate(-40.0, 0.001, 0.05, 0.1, range));
            let dry: Vec<f32> = (0..s(0.5)).map(&x).collect();
            let got = gain_db(&out, &dry, s(0.1)..s(0.5));
            assert!((got - range as f64).abs() < 0.1, "range {range} dB ({channels} ch): closed at {got:.3} dB");
        }
    }
}

// PACK-26: it opens over `attack`, a straight line in level; it stays open while its key is
// loud, then for `hold` once it has gone 6 dB under the threshold; then it closes over `release`,
// a straight line in dB, down to `range`. Open, its output is its input, bit for bit.
#[test]
fn the_gate_opens_holds_and_closes_in_its_times() {
    let loud = tone_at(1000.0, -20.0);
    let quiet = tone_at(1000.0, -70.0);
    let x = |t: usize| if t < s(0.5) { loud(t) } else { quiet(t) };
    let out = gated(1, s(1.0), &x, &x, gate(-40.0, 0.01, 0.1, 0.2, -60.0));
    let level = |t: usize| (out[t] / x(t)) as f64;
    // The first sample is above the threshold: the gate opens there, from -60 dB, in 10 ms.
    let halfway = (s(0.004)..s(0.006)).filter(|&t| x(t).abs() > 1e-3).map(level).collect::<Vec<_>>();
    assert!(halfway.iter().all(|g| (0.38..0.62).contains(g)), "halfway through its attack: {halfway:?}");
    let open = s(0.0105)..s(0.598);
    assert!(open.clone().all(|t| out[t].to_bits() == x(t).to_bits()), "open, it changes the sound");
    // Closing: hold ends near 0.6 s, release takes 200 ms to -60 dB, a straight line in dB.
    let mid = gain_db(&out, &(0..s(1.0)).map(x).collect::<Vec<_>>(), s(0.697)..s(0.703));
    assert!((-33.0..-27.0).contains(&mid), "halfway through its release: {mid:.1} dB");
    let closed = s(0.81)..s(1.0);
    assert!(closed.clone().all(|t| (out[t] - x(t) * 1e-3).abs() <= 1e-9), "closed, it isn't at -60 dB");
}

// PACK-26: a key that hovers around the threshold, never 6 dB under it, never closes the gate, and
// neither do the gaps between the waves of a low note: it holds at least 25 ms.
#[test]
fn the_gate_never_chatters() {
    let threshold = -30.0;
    // From 2 dB above the threshold to 4 dB under it and back, three times a second.
    let hover = |t: usize| {
        let level = threshold - 1.0 + 3.0 * (2.0 * PI * 3.0 * t as f64 / SR as f64).cos();
        tone_at(1000.0, level)(t)
    };
    let low = |hz: f64| move |t: usize| tone_at(hz, threshold + 1.0)(t);
    let low30 = low(30.0);
    let low20 = low(20.0);
    let keys: [(&str, &dyn Fn(usize) -> f32); 3] =
        [("a key hovering at the threshold", &hover), ("a 30 Hz tone", &low30), ("a 20 Hz tone", &low20)];
    for (what, key) in keys {
        let out = gated(1, s(3.0), key, key, gate(threshold as f32, 0.001, 0.0, 0.05, -80.0));
        let from = s(0.01);
        let changed = (from..s(3.0)).filter(|&t| out[t].to_bits() != key(t).to_bits()).count();
        assert_eq!(changed, 0, "{what}: the gate closed for {changed} samples");
    }
}

// PACK-26: its key may be another sound (a sidechain): the gate opens while the key is loud, and
// closes after it, whatever its own input does. A key under the threshold never opens it.
#[test]
fn a_key_opens_the_gate() {
    let x = tone_at(440.0, -20.0);
    let burst = |level: f64| move |t: usize| if (s(0.5)..s(0.6)).contains(&t) { tone_at(1000.0, level)(t) } else { 0.0 };
    let key = burst(-10.0);
    let out = gated(2, s(1.0), &x, &key, gate(-40.0, 0.001, 0.05, 0.05, -80.0));
    let dry: Vec<f32> = (0..s(1.0)).map(&x).collect();
    let closed = |r: std::ops::Range<usize>| r.clone().all(|t| (out[t] - dry[t] * 1e-4).abs() <= 1e-9);
    assert!(closed(0..s(0.5)), "before the key, it is closed");
    assert!((s(0.502)..s(0.6)).all(|t| out[t].to_bits() == dry[t].to_bits()), "while the key is loud, it is open");
    assert!(closed(s(0.71)..s(1.0)), "after the key, its hold and its release, it is closed again");
    let soft = burst(-50.0);
    let out = gated(2, s(1.0), &x, &soft, gate(-40.0, 0.001, 0.05, 0.05, -80.0));
    assert!((0..s(1.0)).all(|t| (out[t] - dry[t] * 1e-4).abs() <= 1e-9), "a key under the threshold opened it");
}

// ---------------------------------------------------------------- the transient shaper

/// How often a hit comes.
const EVERY: f32 = 0.6;

/// Hits: a 100 Hz tone that starts at full level every `EVERY` seconds and fades (by 1/e every 80 ms).
fn hits(_: usize, t: usize) -> f32 {
    let at = t % s(EVERY);
    (0.5 * (2.0 * PI * 100.0 * at as f64 / SR as f64).sin() * (-(at as f64) / (0.08 * SR as f64)).exp()) as f32
}

/// Three hits through the transient shaper, and as they were.
fn shaped(attack: f32, sustain: f32) -> (Vec<f32>, Vec<f32>) {
    let out = left(&run::<Transient>(1, s(3.0 * EVERY), BLOCK, &with(&hits, &[attack, sustain])));
    (out, (0..s(3.0 * EVERY)).map(|t| hits(0, t)).collect())
}

// PACK-26: `attack` raises (or lowers) a hit's start, and leaves its tail as it was, bit for bit.
#[test]
fn the_transient_shaper_raises_a_hits_start_not_its_tail() {
    for (attack, louder) in [(12.0, true), (-12.0, false)] {
        let (out, dry) = shaped(attack, 0.0);
        for hit in 0..3 {
            let t = hit * s(EVERY);
            let start = gain_db(&out, &dry, t..t + s(0.005));
            assert!(if louder { start > 9.0 } else { start < -9.0 }, "attack {attack}: hit {hit}'s first 5 ms by {start:.1} dB");
            let tail = t + s(0.1)..t + s(0.55);
            let changed = tail.clone().filter(|&i| out[i].to_bits() != dry[i].to_bits()).count();
            assert_eq!(changed, 0, "attack {attack}: hit {hit}'s tail changed in {changed} samples");
        }
    }
}

// PACK-26: `sustain` raises (or lowers) a hit's tail, all of it once the tail is 6 dB under the
// slow follower, and leaves its start as it was. (The tail before a hit is still turned while the
// new hit's level rises past the slow follower: in its first two milliseconds.)
#[test]
fn sustain_raises_the_tail_not_the_start() {
    for sustain in [12.0f32, -12.0] {
        let (out, dry) = shaped(0.0, sustain);
        for hit in 0..3 {
            let t = hit * s(EVERY);
            let from = if hit == 0 { t } else { t + s(0.002) };
            let changed = (from..t + s(0.02)).filter(|&i| out[i].to_bits() != dry[i].to_bits()).count();
            assert_eq!(changed, 0, "sustain {sustain}: hit {hit}'s start changed in {changed} samples");
            let tail = gain_db(&out, &dry, t + s(0.2)..t + s(0.5));
            assert!((tail - sustain as f64).abs() < 0.1, "sustain {sustain}: hit {hit}'s tail by {tail:.2} dB");
        }
    }
}

/// A hit every second: a 100 Hz tone, or noise, that starts at half of full scale and fades by 1/e
/// every `fade` seconds.
fn faded(noisy: bool, fade: f32) -> impl Fn(usize, usize) -> f32 {
    move |_, t| {
        let at = t % s(1.0);
        let source = if noisy { noise(0, at) } else { (2.0 * PI * 100.0 * at as f64 / SR as f64).sin() as f32 };
        source * 0.5 * (-(at as f32) / (fade * SR)).exp()
    }
}

// PACK-26 (review Z1): `sustain` turns every tail, however slowly it fades: a tone's and noise's,
// fading by 1/e every 30 ms to 1 s. The faster a tail fades below the slow follower, the more of
// `sustain` it gets: all of it from 120 ms down, and at least a sixth of it at 1 s (200 to 400 ms
// into the third hit).
#[test]
fn sustain_turns_tails_that_fade_slowly_too() {
    for noisy in [false, true] {
        for fade in [0.03, 0.12, 0.3, 0.6, 1.0] {
            for sustain in [12.0f32, -12.0] {
                let hit = faded(noisy, fade);
                let out = left(&run::<Transient>(1, s(3.0), BLOCK, &with(&hit, &[0.0, sustain])));
                let dry: Vec<f32> = (0..s(3.0)).map(|t| hit(0, t)).collect();
                let tail = gain_db(&out, &dry, s(2.2)..s(2.4)) / sustain as f64;
                let what = format!(
                    "{} fading by 1/e every {} ms, sustain {sustain}: its tail by {:+.2} dB",
                    if noisy { "noise" } else { "a tone" },
                    fade * 1000.0,
                    tail * sustain as f64
                );
                assert!(tail <= 1.0 + 1e-3, "{what}: more than sustain");
                assert!(tail >= if fade <= 0.12 { 0.99 } else { 1.0 / 6.0 }, "{what}");
            }
        }
    }
}

// Where the level holds, nothing changes: a steady tone comes out bit for bit once its start is past.
#[test]
fn a_steady_sound_passes_the_transient_shaper() {
    let x = |_: usize, t: usize| tone_at(100.0, -6.0)(t);
    let out = left(&run::<Transient>(2, s(1.0), BLOCK, &with(&x, &[12.0, 12.0])));
    let changed = (s(0.3)..s(1.0)).filter(|&t| out[t].to_bits() != x(0, t).to_bits()).count();
    assert_eq!(changed, 0, "a steady tone changed in {changed} samples");
}

// ---------------------------------------------------------------- the de-esser

fn deessed(x: &dyn Fn(usize) -> f32, freq: f32, threshold: f32, amount: f32) -> Vec<f32> {
    left(&run::<Deesser>(2, 2 * N, BLOCK, &|p, _, t| [x(t), freq, threshold, amount][p]))
}

// PACK-26: it turns down the band around its frequency (6 kHz here), by at most `amount`, while that
// band is over the threshold. A 200 Hz tone, loud as it may be, comes out bit for bit; a 7 kHz one is
// turned down; together, the 200 Hz tone keeps its level.
#[test]
fn the_deesser_turns_down_its_band() {
    let (low, high) = (bin_of(200.0), bin_of(7000.0));
    let on = |k: usize, amp: f64| move |t: usize| (amp * (2.0 * PI * k as f64 * t as f64 / N as f64).sin()) as f32;
    let low_tone = on(low, 0.99);
    let out = deessed(&low_tone, 6000.0, -30.0, 6.0);
    assert!((0..2 * N).all(|t| out[t].to_bits() == low_tone(t).to_bits()), "a 200 Hz tone at 0 dBFS changed");
    let high_tone = on(high, 0.3);
    let out = deessed(&high_tone, 6000.0, -30.0, 6.0);
    let down = db(amplitude(&out[N..], high) / 0.3);
    assert!((-6.1..-4.0).contains(&down), "a 7 kHz tone at -10 dBFS: {down:.2} dB");
    let both = |t: usize| low_tone(t) * 0.5 + high_tone(t);
    let out = deessed(&both, 6000.0, -30.0, 6.0);
    let kept = db(amplitude(&out[N..], low) / (0.99 * 0.5));
    let cut = db(amplitude(&out[N..], high) / 0.3);
    assert!(kept.abs() < 0.05, "beside a 7 kHz tone, the 200 Hz tone moved by {kept:.3} dB");
    assert!(cut < -4.0, "beside a 200 Hz tone, the 7 kHz tone by {cut:.2} dB");
    // Under the threshold, or with no amount, nothing changes.
    let soft = on(high, 0.003);
    let out = deessed(&soft, 6000.0, -30.0, 6.0);
    assert!((0..2 * N).all(|t| out[t].to_bits() == soft(t).to_bits()), "a 7 kHz tone under the threshold changed");
    let out = deessed(&high_tone, 6000.0, -30.0, 0.0);
    assert!((0..2 * N).all(|t| out[t].to_bits() == high_tone(t).to_bits()), "amount 0 changed the sound");
}

// PACK-26 (review Z4): its band is a bell around `freq`, so a loud tone near it is turned down a
// little too, and one far from it not at all. At the defaults (6 kHz, -30 dB) and the most `amount`,
// 24 dB: a tone at 1 kHz is untouched, at 2 kHz down at most 0.5 dB, at 3 kHz (an octave under) at
// most 1.5 dB, at 4 kHz at most 3.5 dB, and at 12 kHz (an octave over) at most 0.5 dB.
#[test]
fn the_deessers_bell_spares_what_is_far_from_it() {
    for (hz, level, most) in [(1000.0, -6.0, 0.0), (2000.0, -6.0, 0.5), (3000.0, -6.0, 1.5), (4000.0, -12.0, 3.5), (12_000.0, -12.0, 0.5)] {
        let k = bin_of(hz);
        let amp = 10f64.powf(level / 20.0);
        let tone = move |t: usize| (amp * (2.0 * PI * k as f64 * t as f64 / N as f64).sin()) as f32;
        let out = deessed(&tone, 6000.0, -30.0, 24.0);
        let change = db(amplitude(&out[N..], k) / amp);
        assert!(change <= 1e-6 && change >= -most - 1e-6, "a {hz} Hz tone at {level} dBFS: {change:.3} dB");
    }
}

// ---------------------------------------------------------------- the multiband compressor

/// The multiband's controls with every band at `ratio` (and its other values at their defaults).
fn multiband(low_mid: f32, mid_high: f32, ratio: f32) -> Vec<f32> {
    let mut c = defaults("multiband", &multiband_ports(), 1);
    c[0] = low_mid;
    c[1] = mid_high;
    for band in 0..3 {
        c[2 + band * 5 + 1] = ratio;
    }
    c
}

/// RBJ's all-pass at `f0`, Q 1/√2, in f64: what a Linkwitz-Riley pair sums to.
fn allpass(x: &[f64], f0: f64) -> Vec<f64> {
    let w0 = 2.0 * PI * f0 / SR as f64;
    let (c, alpha) = (w0.cos(), w0.sin() / SQRT_2);
    let (b, a) = ([1.0 - alpha, -2.0 * c, 1.0 + alpha], [1.0 + alpha, -2.0 * c, 1.0 - alpha]);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    x.iter()
        .map(|&x0| {
            let y0 = (b[0] * x0 + b[1] * x1 + b[2] * x2 - a[1] * y1 - a[2] * y2) / a[0];
            (x2, x1, y2, y1) = (x1, x0, y1, y0);
            y0
        })
        .collect()
}

// PACK-26: Linkwitz-Riley bands sum to an all-pass. At ratio 1 everywhere the output is the input
// through the two crossovers' all-passes, within float rounding, and its level at every frequency
// is the input's within 0.01 dB.
#[test]
fn the_multiband_sums_to_its_crossovers_all_pass() {
    for (low_mid, mid_high) in [(200.0, 2500.0), (100.0, 1000.0), (500.0, 8000.0), (1000.0, 1000.0)] {
        let controls = multiband(low_mid, mid_high, 1.0);
        let out = left(&run::<Multiband>(1, s(1.0), BLOCK, &with(&noise, &controls)));
        let x: Vec<f64> = (0..s(1.0)).map(|t| noise(0, t) as f64).collect();
        let want = allpass(&allpass(&x, low_mid as f64), mid_high as f64);
        let worst = out.iter().zip(&want).map(|(&a, &b)| (a as f64 - b).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-5, "crossovers at {low_mid} and {mid_high} Hz: {worst:e} from the all-pass");
        let bins: Vec<usize> =
            [20.0, 50.0, 100.0, 200.0, 400.0, 1000.0, 2500.0, 5000.0, 10_000.0, 15_000.0, 20_000.0].iter().map(|&f| bin_of(f)).collect();
        let out = left(&run::<Multiband>(1, 2 * N, BLOCK, &with(&tones(&bins, 0.02), &controls)));
        for &k in &bins {
            let level = db(amplitude(&out[N..], k) / 0.02);
            assert!(level.abs() < 0.01, "crossovers at {low_mid} and {mid_high} Hz: {:.0} Hz at {level:.4} dB", bin_hz(k));
        }
    }
}

// PACK-26: each band is compressed on its own: squeezing one leaves a tone in another as it was.
// A band's gain moves only that band.
#[test]
fn each_multiband_band_compresses_on_its_own() {
    // Two octaves or more from the crossovers, so each tone is in one band (a Linkwitz-Riley band is
    // 48 dB down there).
    let probes = [bin_of(40.0), bin_of(670.0), bin_of(12_000.0)];
    let amp = 0.3;
    let x = tones(&probes, amp);
    for (band, name) in ["low", "mid", "high"].iter().enumerate() {
        let mut c = multiband(150.0, 3000.0, 1.0);
        c[2 + band * 5] = -40.0;
        c[2 + band * 5 + 1] = 20.0;
        let out = left(&run::<Multiband>(2, 2 * N, BLOCK, &with(&x, &c)));
        for (other, &k) in probes.iter().enumerate() {
            let level = db(amplitude(&out[N..], k) / amp as f64);
            if other == band {
                assert!(level < -10.0, "the {name} band squeezed: its own tone by {level:.2} dB");
            } else {
                assert!(level.abs() < 0.1, "the {name} band squeezed: the tone at {:.0} Hz by {level:.3} dB", bin_hz(k));
            }
        }
        let mut c = multiband(150.0, 3000.0, 1.0);
        c[2 + band * 5 + 4] = 6.0;
        let out = left(&run::<Multiband>(2, 2 * N, BLOCK, &with(&x, &c)));
        for (other, &k) in probes.iter().enumerate() {
            let level = db(amplitude(&out[N..], k) / amp as f64);
            let want = if other == band { 6.0 } else { 0.0 };
            assert!((level - want).abs() < 0.1, "the {name} band's gain at 6 dB: the tone at {:.0} Hz by {level:.3} dB", bin_hz(k));
        }
    }
}

// PACK-26 (review Z7): split points that cross are put in order: the lower splits low from mid,
// whichever knob it is, so the mid band is never empty. Swapped, they play the same bits; and
// squeezing the mid band turns down a tone between them.
#[test]
fn crossed_split_points_are_put_in_order() {
    let probe = [bin_of(1000.0)];
    let x = tones(&probe, 0.3);
    let squeezed = |low_mid: f32, mid_high: f32| {
        let mut c = multiband(low_mid, mid_high, 1.0);
        c[2 + 5] = -40.0;
        c[2 + 5 + 1] = 20.0;
        left(&run::<Multiband>(1, 2 * N, BLOCK, &with(&x, &c)))
    };
    let (ordered, crossed) = (squeezed(500.0, 2000.0), squeezed(2000.0, 500.0));
    assert!(ordered.iter().zip(&crossed).all(|(a, b)| a.to_bits() == b.to_bits()), "crossed split points play otherwise than ordered ones");
    let level = db(amplitude(&crossed[N..], probe[0]) / 0.3);
    assert!(level < -10.0, "split at 2000 and 500 Hz, the mid band squeezed: a 1 kHz tone by {level:.2} dB");
}

// ---------------------------------------------------------------- real time

/// Allocations on this thread, counted: the kernels must make none while they play (ABI-5).
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static MADE: Cell<usize> = const { Cell::new(0) };
    }

    pub struct Counting;

    // SAFETY: System's, with a count beside.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            MADE.with(|m| m.set(m.get() + 1));
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    pub fn made() -> usize {
        MADE.with(Cell::get)
    }
}

#[global_allocator]
static COUNTING: counting::Counting = counting::Counting;

/// Plays `K` for 400 blocks of every size from 1 to 128 frames, its inputs `input`, and counts the
/// allocations its `process` makes.
fn allocations<K: Kernel>(input: Input<'_>) -> usize {
    let mut host = Host::<K>::new(SR, BLOCK as u32, 2);
    let (mut t, mut made) = (0, 0);
    for b in 0..400 {
        let n = b % BLOCK + 1;
        for p in 0..K::INPUTS {
            let buf = host.port_mut(p);
            for c in 0..2 {
                for k in 0..n {
                    buf[c * BLOCK + k] = input(p, c, t + k);
                }
            }
        }
        let before = counting::made();
        host.process(n as u32);
        made += counting::made() - before;
        t += n;
    }
    made
}

/// Plays a kernel and counts its allocations, as [`allocations`] does.
type Counted = fn(Input<'_>) -> usize;

// ABI-5: no tool allocates while it plays, at its start and at every torture setting, with its
// controls moving and its blocks of every size.
#[test]
fn no_tool_allocates_while_it_plays() {
    let tools: [(&str, Counted, Vec<f32>, usize); 5] = [
        ("eq", allocations::<Eq>, defaults("eq", &eq_ports(), 1), 1),
        ("gate", allocations::<Gate>, defaults("gate", &strings(&GATE), 2), 2),
        ("transient", allocations::<Transient>, vec![6.0, -6.0], 1),
        ("deesser", allocations::<Deesser>, defaults("deesser", &strings(&DEESSER), 1), 1),
        ("multiband", allocations::<Multiband>, defaults("multiband", &multiband_ports(), 1), 1),
    ];
    for (name, play, start, audio) in tools {
        let fx = Fx { name, run: run::<Eq>, controls: leak(start.clone()) };
        let mut sets = torture(&fx);
        sets.push(start);
        for set in &sets {
            let made = play(&|p, c, t| match p {
                p if p < audio => saw(c, t) * if (t / 3_000) % 2 == 0 { 1.0 } else { 0.01 },
                p => set[p - audio] * (0.5 + ((t / 900) % 3) as f32 * 0.4),
            });
            assert_eq!(made, 0, "{name} allocated {made} times while it played, controls {set:?}");
        }
    }
}

// PACK-26, RENDER-33: silence brings each tool back to the state it was born in, so a song played
// after the engine sat stopped (its tools fed silence) plays its export. A sound, then 15 s of
// silence, then hits: the hits come out as from a fresh tool, bit for bit.
#[test]
fn silence_brings_each_tool_back_to_its_birth() {
    // Not a whole number of milliseconds, nor of blocks: nothing that counts time lines up by luck.
    let after = s(16.0) + 17;
    for fx in all() {
        for channels in [1, 2] {
            let fresh = (fx.run)(channels, s(1.8), BLOCK, &with(&hits, fx.controls));
            let rested = (fx.run)(
                channels,
                after + s(1.8),
                BLOCK,
                &with(
                    &|c, t| {
                        if t < s(1.0) {
                            saw(c, t)
                        } else if t < after {
                            0.0
                        } else {
                            hits(c, t - after)
                        }
                    },
                    fx.controls,
                ),
            );
            let diff = rested[after..].iter().zip(&fresh).position(|(a, b)| a.map(f32::to_bits) != b.map(f32::to_bits));
            assert_eq!(diff, None, "{} ({channels} ch) after a sound and silence plays otherwise than fresh", fx.name);
        }
    }
}
