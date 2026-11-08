//! Grains (PACK-36), measured as its rule says it: how long its grains are and how many start a
//! second, counted on a steady sound; that its level follows its input's; that Pitch moves a tone
//! by the semitones asked; that silence comes out once the buffer has passed; that every grain is
//! windowed, at most 16 play and the oldest goes first; that it reads only the past, follows the
//! song, and is the same every time; at 44.1, 48 and 96 kHz.

use std::f64::consts::TAU;

use m2_dsp::testing::effects::{BLOCK, noise, saw};
use m2_dsp::testing::{Played, defaults_in, manifest_in, max_step, play_at};
use m2_kernel::{Cfg, Host, Io, Kernel};

use super::{MANIFEST, RATES, counting};
use crate::cloud::{self, Grains};

/// Its values for a test: size (s), density (a second), spread (s), pitch (semitones) and mix.
#[derive(Clone, Copy, Debug)]
struct Set {
    size: f32,
    density: f32,
    spread: f32,
    pitch: f32,
    mix: f32,
}

/// Grains at `sr` on `channels`, `frames` long, in calls of `split`: its input `x(c, t)`, its values
/// `set`, and `playing(t)`. Its output and its meter.
fn grains(
    sr: f32,
    channels: usize,
    frames: usize,
    split: usize,
    set: Set,
    x: &dyn Fn(usize, usize) -> f32,
    playing: &dyn Fn(usize) -> f32,
) -> Played {
    play_at::<Grains>(sr, channels, frames, split, &|p, c, t| match p {
        0 => x(c, t),
        1 => set.size,
        2 => set.density,
        3 => set.spread,
        4 => set.pitch,
        5 => set.mix,
        _ => playing(t),
    })
}

/// The song playing all along.
fn on(_: usize) -> f32 {
    1.0
}

/// The grains alone (Mix 1) on a steady 0.5 at `sr`, mono, for `seconds`.
fn on_a_steady_sound(sr: f32, set: Set, seconds: f32) -> Vec<f32> {
    grains(sr, 1, (seconds * sr) as usize, BLOCK, Set { mix: 1.0, ..set }, &|_, _| 0.5, &on).left()
}

/// Each run of samples other than 0 in `x[from..]`: where it starts, and how long it is.
fn humps(x: &[f32], from: usize) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut start = None;
    for (t, &v) in x.iter().enumerate().skip(from) {
        match (start, v != 0.0) {
            (None, true) => start = Some(t),
            (Some(s), false) => {
                out.push((s, t - s));
                start = None;
            }
            _ => {}
        }
    }
    out
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-300).log10()
}

/// How loud Grains plays its grains: turned down by 1/√(3/8 × n) when that is over 1, `n` the
/// grains playing at once on average, 16 at most (PACK-36).
fn level(set: Set) -> f64 {
    let n = (set.size as f64 * set.density as f64).min(16.0);
    1.0 / (3.0 / 8.0 * n).max(1.0).sqrt()
}

// PACK-36: Grains' manifest says what its code is: its ports, defaults and ranges, its meter, and no
// latency.
#[test]
fn the_grains_manifest_is_its_code() {
    let defaults = defaults_in(MANIFEST, "grains", &["in", "size", "density", "spread", "pitch", "mix", "playing"]);
    assert_eq!(defaults, [0.0, 0.1, 20.0, 0.3, 0.0, 0.5, 1.0]);
    assert_eq!(manifest_in(MANIFEST, "grains").1, ["out"]);
    let entry = &MANIFEST[MANIFEST.find("name = \"grains\"").unwrap()..];
    let entry = &entry[..entry.find("[[kernel]]").unwrap_or(entry.len())];
    for port in [
        "\"size: control<seconds> = 0.1 in 0.01..0.5\"",
        "\"density: control<hz> = 20 in 1..50\"",
        "\"spread: control<seconds> = 0.3 in 0..2\"",
        "\"pitch: control<semitones> = 0 in -12..12\"",
    ] {
        assert!(entry.contains(port), "{port}: {entry}");
    }
    assert!(entry.contains("meters = [\"reads: control<seconds>\"]"), "{entry}");
    assert!(!entry.lines().any(|l| l.starts_with("latency")), "no latency: {entry}");
    assert!(!entry.contains("freeze"), "no Freeze: {entry}");
    assert_eq!((Grains::INPUTS, Grains::OUTPUTS), (7, 2));
    assert_eq!((cloud::BUFFER, cloud::MOST, cloud::LET_GO), (2.5, 16, 0.005));
}

// PACK-36: Size and Density are a grain's length and how many start a second, counted: on a steady
// sound, sparse grains (each alone) come out as humps of exactly Size (round(size × rate) frames,
// the first of them silent), at their middle as loud as the sound, as many a second as Density
// within 6% over 60 s, and the gaps between them from half to one and a half of the gap Density
// gives, all of that reached (from under 0.55 to over 1.45 of it); at 44.1, 48 and 96 kHz.
#[test]
fn size_and_density_are_a_grains_length_and_how_many_start_a_second() {
    for sr in RATES {
        for (size, density) in [(0.02f32, 5.0f32), (0.05, 2.0), (0.01, 20.0)] {
            let set = Set { size, density, spread: 0.5, pitch: 0.0, mix: 1.0 };
            let x = on_a_steady_sound(sr, set, 61.0);
            let all = humps(&x, sr as usize);
            let what = format!("{sr} Hz, size {size}, density {density}");
            let len = (size * sr).round() as usize;
            assert!(
                all.iter().all(|h| h.1 == len - 1),
                "{what}: humps of {:?}, not {}",
                all.iter().map(|h| h.1).collect::<Vec<_>>(),
                len - 1
            );
            for &(start, _) in &all {
                let middle = x[start + len / 2 - 1];
                assert!((middle - 0.5).abs() < 0.01, "{what}: a grain at {start} is {middle} at its middle, the sound 0.5");
            }
            let want = density as f64 * 60.0;
            let n = all.len() as f64;
            println!("{what}: {n} grains in 60 s, {want} asked");
            assert!((n / want - 1.0).abs() < 0.06, "{what}: {n} grains in 60 s, not {want}");
            // The gaps, against the one Density gives: from half to one and a half of it, at random.
            let gaps: Vec<f64> = all.windows(2).map(|w| (w[1].0 - w[0].0) as f64 * density as f64 / sr as f64).collect();
            let (closest, furthest) = gaps.iter().fold((f64::MAX, 0.0f64), |(lo, hi), &g| (lo.min(g), hi.max(g)));
            assert!(closest >= 0.5 - 1.0 / sr as f64 && furthest < 1.5, "{what}: gaps from {closest:.3} to {furthest:.3} of Density's");
            assert!(closest < 0.55 && furthest > 1.45, "{what}: gaps only from {closest:.3} to {furthest:.3} of Density's");
        }
    }
}

/// A sound under 5 kHz, at RMS 1/√2 × `level`: 64 tones from 50 Hz to 5 kHz, evenly in octaves, at
/// phases the noise gives.
fn under_5k(level: f64, sr: f32) -> impl Fn(usize, usize) -> f32 {
    move |c, t| {
        let s: f64 = (0..64)
            .map(|k| {
                let hz = 50.0 * 100f64.powf(k as f64 / 63.0);
                (TAU * hz * t as f64 / sr as f64 + TAU * noise(c, 1_000 + k) as f64).sin()
            })
            .sum();
        (level * s / 8.0) as f32
    }
}

// PACK-36: its level follows its input's: a sound under 5 kHz in at −30 dB, then at −10 dB, through
// its grains alone from places spread over 0.5 s, comes out at the input's level within 1 dB at
// each, 3 or more grains playing at once on average, whatever Size and Density give that; and 20 dB
// louder within 0.5 dB. White noise, to the top of hearing, within 2 dB: the read between samples
// dulls its highest sounds a little. At 44.1, 48 and 96 kHz.
#[test]
fn its_level_follows_its_inputs() {
    for sr in RATES {
        let s = |x: f32| (x * sr) as usize;
        let (soft, loud) = (under_5k(0.0447, sr), under_5k(0.447, sr));
        let tones = move |c, t| if t < s(3.0) { soft(c, t) } else { loud(c, t) };
        let white = move |c, t| if t < s(3.0) { 0.0316 * noise(c, t) } else { 0.316 * noise(c, t) };
        for (sound, x, within) in [("under 5 kHz", &tones as &dyn Fn(usize, usize) -> f32, 1.0), ("white noise", &white, 2.0)] {
            for (size, density) in [(0.1f32, 40.0f32), (0.3, 10.0), (0.06, 50.0), (0.2, 40.0)] {
                let set = Set { size, density, spread: 0.5, pitch: 0.0, mix: 1.0 };
                let out = grains(sr, 1, s(6.0), BLOCK, set, x, &on).left();
                let input: Vec<f32> = (0..s(6.0)).map(|t| x(0, t)).collect();
                let what = format!("{sr} Hz, {sound}, size {size}, density {density}");
                let at = |a: f32, b: f32| db(rms(&out[s(a)..s(b)]) / rms(&input[s(a)..s(b)]));
                let (low, high) = (at(1.0, 3.0), at(4.0, 6.0));
                println!("{what}: {low:+.2} dB at −30 dB, {high:+.2} dB at −10 dB");
                assert!(low.abs() < within && high.abs() < within, "{what}: {low:+.2} dB and {high:+.2} dB from the input's level");
                let step = db(rms(&out[s(4.0)..s(6.0)]) / rms(&out[s(1.0)..s(3.0)]));
                assert!((step - 20.0).abs() < 0.5, "{what}: 20 dB louder in, {step:.2} dB louder out");
            }
        }
    }
}

/// The frequency of a tone in a window, `x`, at `sr`, near `want`: where its spectrum is loudest,
/// found to a hundred-thousandth.
fn tone_of(x: &[f32], sr: f32, want: f64) -> f64 {
    let size = |hz: f64| {
        let w = TAU * hz / sr as f64;
        let (re, im) = x
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(re, im), (n, &v)| (re + v as f64 * (w * n as f64).cos(), im - v as f64 * (w * n as f64).sin()));
        re * re + im * im
    };
    // The loudest of a coarse look, then closer, a golden section at a time.
    let (mut lo, mut hi) = (want * 0.9, want * 1.1);
    let best =
        (0..=400).map(|i| lo + (hi - lo) * i as f64 / 400.0).fold((want, 0.0), |b, hz| if size(hz) > b.1 { (hz, size(hz)) } else { b });
    (lo, hi) = (best.0 - (hi - lo) / 400.0, best.0 + (hi - lo) / 400.0);
    while hi - lo > want * 1e-6 {
        let (a, b) = (hi - 0.618 * (hi - lo), lo + 0.618 * (hi - lo));
        if size(a) > size(b) { hi = b } else { lo = a }
    }
    (lo + hi) / 2.0
}

// PACK-36: Pitch moves each grain by the semitones asked: a 440 Hz tone through its grains alone,
// each grain alone (100 ms, 2 a second), comes out of each at 440 × 2^(pitch/12) Hz, within 0.1%,
// at −12, −7, 7 and 12; at 44.1, 48 and 96 kHz.
#[test]
fn pitch_moves_a_tone_by_the_semitones_asked() {
    for sr in RATES {
        for pitch in [-12.0f32, -7.0, 7.0, 12.0] {
            let set = Set { size: 0.1, density: 2.0, spread: 0.2, pitch, mix: 1.0 };
            let tone = |_, t: usize| (0.5 * (TAU * 440.0 * t as f64 / sr as f64).sin()) as f32;
            let out = grains(sr, 1, (6.0 * sr) as usize, BLOCK, set, &tone, &on).left();
            let want = 440.0 * 2f64.powf(pitch as f64 / 12.0);
            // Each whole grain: a hump as long as Size, the first frame of it silent.
            let whole = (0.1 * sr).round() as usize - 1;
            let all: Vec<(usize, usize)> = humps(&out, sr as usize).into_iter().filter(|h| h.1 == whole).collect();
            assert!(all.len() >= 8, "{sr} Hz, pitch {pitch}: {} grains", all.len());
            for (start, len) in all {
                let got = tone_of(&out[start..start + len], sr, want);
                assert!((got / want - 1.0).abs() < 0.001, "{sr} Hz, pitch {pitch}: a grain at {got:.2} Hz, not {want:.2}");
            }
        }
    }
}

// PACK-36: silence in gives silence out once its buffer has passed: noise for 1 s, then silence, at
// the settings that reach furthest back (Spread 2 s, Size 0.5 s, an octave down) and at those it
// opens at: exactly 0 from 2.6 s after the noise (the furthest a grain reads, and plays on, past it),
// and it says it is done 2.5 s after it (its buffer's turn).
#[test]
fn silence_in_gives_silence_out_once_its_buffer_has_passed() {
    let sr = 48_000.0;
    let s = |x: f32| (x * sr) as usize;
    let x = |c, t| if t < s(1.0) { 0.5 * noise(c, t) } else { 0.0 };
    for set in [
        Set { size: 0.5, density: 50.0, spread: 2.0, pitch: -12.0, mix: 1.0 },
        Set { size: 0.1, density: 20.0, spread: 0.3, pitch: 0.0, mix: 0.5 },
    ] {
        let played = grains(sr, 2, s(6.0), BLOCK, set, &x, &on);
        let last = played.out[0].iter().rposition(|f| f[0] != 0.0 || f[1] != 0.0).unwrap();
        assert!(last < s(3.6), "{set:?}: sound {:.3} s after the noise", last as f32 / sr - 1.0);
        let done = played.active.iter().rposition(|&a| a).map_or(0, |t| t + 1);
        assert!(done > s(3.4) && done < s(3.6), "{set:?}: done {:.3} s after the noise", done as f32 / sr - 1.0);
    }
}

// PACK-36: at Mix 0 it plays its input, bit for bit, whatever its other values, mono and stereo.
#[test]
fn mix_0_is_its_input_bit_for_bit() {
    for channels in [1, 2] {
        for set in [
            Set { size: 0.1, density: 20.0, spread: 0.3, pitch: 0.0, mix: 0.0 },
            Set { size: 0.5, density: 50.0, spread: 2.0, pitch: 12.0, mix: 0.0 },
        ] {
            let out = grains(48_000.0, channels, 30_000, BLOCK, set, &noise, &on);
            for (t, f) in out.out[0].iter().enumerate() {
                assert_eq!(f[0].to_bits(), noise(0, t).to_bits(), "{channels} ch, frame {t}");
                if channels == 2 {
                    assert_eq!(f[1].to_bits(), noise(1, t).to_bits(), "{channels} ch, frame {t}");
                }
            }
        }
    }
}

// PACK-36: every grain is shaped by its window, so none clicks: on a slow 2.7 Hz tone at 0.9 (which
// isn't where it was 2.5 s before), its grains alone never step from one sample to the next by more
// than 17 grains at their steepest can (a Hann window's π/length a sample, a grain let go's
// 1/(5 ms), and the tone's own), at every Size, Density, Pitch and Spread at their ends: a grain cut
// off, or reading past the newest sound into what is 2.5 s old, steps by far more.
#[test]
fn every_grain_is_windowed_so_none_clicks() {
    let sr = 48_000.0;
    let tone = |_, t: usize| (0.9 * (TAU * 2.7 * t as f64 / sr as f64).sin()) as f32;
    for size in [0.01f32, 0.1, 0.5] {
        for density in [1.0f32, 20.0, 50.0] {
            for pitch in [-12.0f32, 0.0, 12.0] {
                for spread in [0.0f32, 2.0] {
                    let set = Set { size, density, spread, pitch, mix: 1.0 };
                    let out = grains(sr, 1, (4.0 * sr) as usize, BLOCK, set, &tone, &on).left();
                    let len = (size * sr).round() as f64;
                    let ratio = 2f64.powf(pitch as f64 / 12.0);
                    let steepest =
                        (std::f64::consts::PI / len + 1.0 / (cloud::LET_GO as f64 * sr as f64)) * 0.9 + ratio * TAU * 2.7 / sr as f64 * 0.9;
                    let most = 17.0 * level(set) * steepest;
                    let step = max_step(&out, 1, out.len()) as f64;
                    assert!(
                        step <= most,
                        "size {size}, density {density}, pitch {pitch}, spread {spread}: a step of {step:.5}, over {most:.5}"
                    );
                }
            }
        }
    }
}

/// Grains, keeping after each call how many grains have started and what its meter `reads` says.
struct Metered(Grains);

thread_local! {
    static SAID: std::cell::RefCell<Vec<(u64, f32)>> = const { std::cell::RefCell::new(vec![]) };
}

impl Kernel for Metered {
    const INPUTS: usize = Grains::INPUTS;
    const OUTPUTS: usize = Grains::OUTPUTS;

    fn new(cfg: &Cfg) -> Self {
        Metered(Grains::new(cfg))
    }

    fn process(&mut self, io: &mut Io) {
        self.0.process(io);
        let ([], [reads]) = io.ports([], [(1, 0)]);
        let said = reads[reads.len() - 1];
        SAID.with(|s| s.borrow_mut().push((self.0.census().4, said)));
    }
}

// PACK-36, ABI-13: its meter follows the newest grain: frame by frame, at Pitch 0 (where a grain reads
// from one place all its life), with grains 0.5 s long, 2 a second (so often two play), it moves at
// every frame a grain starts, to a place in the band, and at no other frame but where the newest
// ends.
#[test]
fn its_meter_follows_the_newest_grain() {
    SAID.with(|s| s.borrow_mut().clear());
    let set = Set { size: 0.5, density: 2.0, spread: 1.0, pitch: 0.0, mix: 1.0 };
    let played = play_at::<Metered>(48_000.0, 1, 10 * 48_000, 1, &|p, _, _| match p {
        0 => 0.5,
        1 => set.size,
        2 => set.density,
        3 => set.spread,
        4 => set.pitch,
        5 => set.mix,
        _ => 1.0,
    });
    let said = SAID.with(|s| s.borrow().clone());
    let out = played.left();
    let (mut starts, mut overlapping) = (0, 0);
    for t in 1..said.len() {
        let ((was, before), (now, after)) = (said[t - 1], said[t]);
        if now > was {
            starts += 1;
            assert!(
                after != before && (2.0 / 48_000.0..=1.0 + 2.0 / 48_000.0).contains(&after),
                "frame {t}: a grain started and it says {after}, not a new place"
            );
            // Another grain still playing: the one before it (its hump not yet over).
            overlapping += (out[t - 1] != 0.0) as usize;
        } else if after != before {
            // Only where the newest ends: then it says the next newest's place, or 0.
            assert!(after == 0.0 || out[t] != 0.0, "frame {t}: it moved with no grain starting, to {after}");
        }
    }
    assert!(starts > 15 && overlapping > 5, "{starts} grains started, {overlapping} while another played");
}

/// Grains, checking after each call that at most 16 grains play (not counting those let go, which
/// fade out), and that each grain let go is older than every grain still playing: the oldest goes
/// first. It counts the grains let go.
struct Watched(Grains);

thread_local! {
    static LET_GO: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Kernel for Watched {
    const INPUTS: usize = Grains::INPUTS;
    const OUTPUTS: usize = Grains::OUTPUTS;

    fn new(cfg: &Cfg) -> Self {
        Watched(Grains::new(cfg))
    }

    fn process(&mut self, io: &mut Io) {
        let was = self.0.census().1;
        self.0.process(io);
        let (playing, letting_go, oldest_kept, youngest_let_go, _) = self.0.census();
        assert!(playing <= cloud::MOST, "{playing} grains playing, at most {} may", cloud::MOST);
        if let (Some(kept), Some(gone)) = (oldest_kept, youngest_let_go) {
            assert!(gone < kept, "a grain let go (started {gone}) is younger than one kept (started {kept})");
        }
        LET_GO.with(|n| n.set(n.get() + letting_go.saturating_sub(was)));
    }
}

// PACK-36: at most 16 grains play at once, and one more lets the oldest go: at Size 0.5 s and Density
// 50 (25 at once on average), frame by frame for 4 s, never more than 16 play, every grain let go is
// older than all those kept, and grains are let go.
#[test]
fn at_most_16_play_and_the_oldest_goes_first() {
    LET_GO.with(|n| n.set(0));
    let set = Set { size: 0.5, density: 50.0, spread: 2.0, pitch: 0.0, mix: 1.0 };
    play_at::<Watched>(48_000.0, 1, 4 * 48_000, 1, &|p, _, t| match p {
        0 => 0.5 * noise(0, t),
        1 => set.size,
        2 => set.density,
        3 => set.spread,
        4 => set.pitch,
        5 => set.mix,
        _ => 1.0,
    });
    let gone = LET_GO.with(|n| n.get());
    assert!(gone > 50, "only {gone} grains let go");
}

// PACK-36, RENDER-33: it follows the song. A fresh one that sat stopped (playing 0, silence in) for
// 5 blocks, 20,003 frames or 2 s plays, from Play on, what a fresh one plays from the start, bit for
// bit, mono and stereo, at the values it opens at and an octave up spread wide, in calls of 128 and 7.
#[test]
fn played_after_it_sat_stopped_it_plays_as_one_born_at_play() {
    for set in [
        Set { size: 0.1, density: 20.0, spread: 0.3, pitch: 0.0, mix: 0.5 },
        Set { size: 0.3, density: 40.0, spread: 1.5, pitch: 12.0, mix: 0.8 },
    ] {
        for channels in [1, 2] {
            for split in [BLOCK, 7] {
                let frames = 3 * 48_000;
                let born = grains(48_000.0, channels, frames, split, set, &saw, &on);
                for idle in [5 * BLOCK, 20_003, 96_000] {
                    let x = |c, t| if t < idle { 0.0 } else { saw(c, t - idle) };
                    let late = grains(48_000.0, channels, idle + frames, split, set, &x, &|t| (t >= idle) as u8 as f32);
                    let diff = late.out[0][idle..].iter().zip(&born.out[0]).position(|(a, b)| a.map(f32::to_bits) != b.map(f32::to_bits));
                    assert_eq!(diff, None, "{set:?}, {channels} ch, in calls of {split}, after {idle} frames stopped");
                }
            }
        }
    }
}

// PACK-36: stopped, no grain starts, the grains playing play out, and Play carries its clock on from
// where it stopped: on a steady sound, stopped from 1 s to 3 s, its grains alone are silent from
// 1.02 s (the last grain's end) to 3 s, and from 1.02 s on play what they play with no stop, 2 s
// later, bit for bit.
#[test]
fn stopped_no_grain_starts_and_play_carries_its_clock_on() {
    let sr = 48_000.0;
    let s = |x: f32| (x * sr) as usize;
    let set = Set { size: 0.02, density: 20.0, spread: 0.5, pitch: 0.0, mix: 1.0 };
    let steady = |_, _| 0.5;
    let straight = grains(sr, 1, s(6.0), BLOCK, set, &steady, &on).left();
    let stopped = grains(sr, 1, s(8.0), BLOCK, set, &steady, &|t| (t < s(1.0) || t >= s(3.0)) as u8 as f32).left();
    assert!(stopped[s(1.02)..s(3.0)].iter().all(|&v| v == 0.0), "a grain started while stopped");
    assert!(
        stopped[s(0.9)..s(1.0)].iter().any(|&v| v != 0.0) && stopped[s(3.0)..s(3.2)].iter().any(|&v| v != 0.0),
        "grains around the stop"
    );
    let diff = (s(1.02)..s(6.0)).find(|&t| stopped[t + s(2.0)].to_bits() != straight[t].to_bits());
    assert_eq!(diff, None, "after Play, at {:?} it plays otherwise than with no stop", diff.map(|t| t as f32 / sr));
}

// PACK-36: its grains read only the past, from where its band says: a click into its grains alone
// (Pitch 0, Spread 0.3 s, Size 50 ms, Density 50) comes out no sooner than it went in, and only from
// 2 frames to 0.3 s and 4 frames after it (the band, and the read between samples), at 44.1, 48 and
// 96 kHz.
#[test]
fn its_grains_read_only_the_past_from_its_band() {
    for sr in RATES {
        let at = sr as usize;
        let set = Set { size: 0.05, density: 50.0, spread: 0.3, pitch: 0.0, mix: 1.0 };
        let out = grains(sr, 1, 3 * sr as usize, BLOCK, set, &|_, t| (t == at) as u8 as f32, &on).left();
        // A grain on the click comes out as long as it plays: Size after the click's place in it.
        let heard: Vec<usize> = (0..out.len()).filter(|&t| out[t] != 0.0).collect();
        assert!(!heard.is_empty(), "{sr} Hz: the click comes out");
        let (first, last) = (heard[0], *heard.last().unwrap());
        assert!(first >= at, "{sr} Hz: sound at {first}, before the click at {at}");
        let far = at + (0.3 * sr) as usize + 4;
        assert!(last <= far, "{sr} Hz: the click comes out {:.4} s after it went in, past the band", (last - at) as f32 / sr);
    }
}

// PACK-36, ABI-13: its meter `reads` says how far back the newest grain that plays reads, in
// seconds: at Pitch 0 on a steady sound, from 2 frames to Spread's 0.5 s and 2 frames back, at many
// places, while a grain plays; 0 at the end of a call where none plays; and still while the song is
// stopped.
#[test]
fn grains_say_where_the_newest_reads() {
    let sr = 48_000.0;
    let set = Set { size: 0.05, density: 10.0, spread: 0.5, pitch: 0.0, mix: 1.0 };
    let played = grains(sr, 1, 5 * 48_000, BLOCK, set, &|_, _| 0.5, &|t| (t < 4 * 48_000) as u8 as f32);
    let (out, reads) = (played.left(), played.out[1].iter().map(|f| f[0]).collect::<Vec<f32>>());
    let (mut places, mut none) = (vec![], 0);
    let band = 2.0 / sr..=0.5 + 2.0 / sr;
    for b in 0..4 * 48_000 / BLOCK {
        let last = b * BLOCK + BLOCK - 1;
        let r = reads[last];
        // A grain that starts at the call's last frame plays its first frame silent.
        assert!(r == 0.0 || band.contains(&r), "block {b}: the newest grain reads {r} s back");
        if out[last] != 0.0 {
            assert!(band.contains(&r), "block {b}: a grain plays, and it says {r}");
            places.push(r.to_bits());
        } else if r == 0.0 {
            none += 1;
        }
    }
    places.sort_unstable();
    places.dedup();
    assert!(places.len() > 20 && none > 20, "it read from {} places, and said none played {none} times", places.len());
    let stopped = &reads[4 * 48_000 + (0.1 * sr) as usize..];
    assert!(stopped.iter().all(|&r| r == stopped[0]), "stopped, it stays");
}

// PACK-36, the house rules: a control that isn't finite counts as an end: NaN as its low end, ±∞ as
// the end it points to. Size NaN plays as 10 ms and ∞ as 500 ms; Density as 1 and 50; Spread as 0
// and 2 s; Pitch as −12 and 12; Mix as 0 and 1.
#[test]
fn a_control_that_isnt_finite_counts_as_an_end() {
    let play = |port: usize, v: f32| {
        play_at::<Grains>(48_000.0, 2, 48_000, BLOCK, &|p, c, t| match p {
            0 => noise(c, t),
            _ if p == port => v,
            1 => 0.1,
            2 => 20.0,
            3 => 0.3,
            4 => 0.0,
            5 => 0.7,
            _ => 1.0,
        })
        .out
        .swap_remove(0)
        .iter()
        .flat_map(|f| f.map(f32::to_bits))
        .collect::<Vec<u32>>()
    };
    for (port, lo, hi) in [(1, 0.01, 0.5), (2, 1.0, 50.0), (3, 0.0, 2.0), (4, -12.0, 12.0), (5, 0.0, 1.0)] {
        assert!(play(port, f32::NAN) == play(port, lo), "port {port}: NaN plays as {lo}");
        assert!(play(port, f32::INFINITY) == play(port, hi), "port {port}: ∞ plays as {hi}");
        assert!(play(port, f32::NEG_INFINITY) == play(port, lo), "port {port}: −∞ plays as {lo}");
    }
}

// RENDER-28, PACK-36: once born, it allocates nothing while it plays, at 44.1, 48 and 96 kHz, in
// stereo: noise, every value moving, stopping and playing, grains let go.
#[test]
fn grains_allocate_nothing_while_they_play_at_every_rate() {
    for sr in RATES {
        let mut h = Host::<Grains>::new(sr, BLOCK as u32, 2);
        let before = counting::here();
        for b in 0..3_000 {
            for k in 0..BLOCK {
                let t = b * BLOCK + k;
                h.port_mut(0)[k] = noise(0, t);
                h.port_mut(0)[BLOCK + k] = noise(1, t);
            }
            for (p, v) in [
                (1, 0.01 + (b % 7) as f32 * 0.08),
                (2, 1.0 + (b % 11) as f32 * 4.9),
                (3, (b % 5) as f32 * 0.5),
                (4, (b % 9) as f32 * 3.0 - 12.0),
                (5, (b % 3) as f32 / 2.0),
                (6, (b % 50 != 0) as u8 as f32),
            ] {
                h.port_mut(p)[..BLOCK].fill(v);
            }
            h.process(BLOCK as u32);
        }
        assert_eq!(counting::here() - before, 0, "{sr} Hz: allocated while it played");
    }
}

// PACK-36, RENDER-6a: it says it is active for as long as anything it took in can still come out.
// Played a frame a call, a burst of noise that ends on one sample of 1, -1, 1e30 or the most a float
// holds, at the values that reach furthest back, at 44.1, 48 and 96 kHz, mono and stereo: nothing
// comes out after the call where it first says it is done, and it says so within 2.6 s.
#[test]
fn nothing_comes_out_once_grains_says_it_is_done() {
    for sr in RATES {
        let stop = (0.5 * sr) as usize;
        let set = Set { size: 0.5, density: 50.0, spread: 2.0, pitch: -12.0, mix: 0.5 };
        for last in [1.0f32, -1.0, 1e30, f32::MAX] {
            for channels in [1, 2] {
                let x = |c, t: usize| {
                    if t + 2_000 < stop {
                        0.0
                    } else if t < stop {
                        0.5 * noise(c, t)
                    } else if t == stop {
                        last
                    } else {
                        0.0
                    }
                };
                let played = grains(sr, channels, stop + (3.0 * sr) as usize, 1, set, &x, &on);
                let out = &played.out[0];
                let what = format!("{last} at {sr} Hz, {channels} channels");
                let done = (stop..out.len()).find(|&t| !played.active[t]).unwrap_or_else(|| panic!("{what}: it never says it is done"));
                assert!(done < stop + (2.6 * sr) as usize, "{what}: done {:.2} s after the burst", (done - stop) as f32 / sr);
                let heard = out.iter().rposition(|f| f[0] != 0.0 || f[1] != 0.0).expect("it played");
                assert!(heard <= done, "{what}: sound at frame {heard}, after it said it was done at {done}");
            }
        }
    }
}
