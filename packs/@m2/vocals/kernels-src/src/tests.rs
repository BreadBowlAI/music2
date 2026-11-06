//! Native tests of the pack's kernels, each measured by another method than the kernel's own: a
//! pitch by an FFT's peak (m2-dsp's `peak_hz`), a vowel's formants by a smoothed spectrum (a
//! cepstrum). They play a made-up singer: a pulse train whose pitch wobbles at 5.5 Hz, through three
//! formant filters for "ah", and a burst of noise for "s". Then what every kernel guarantees: no
//! allocation while it plays at 44.1, 48 and 96 kHz, the same bits however its blocks are split, a
//! reset that starts over, and nothing broken by any input.

use m2_dsp::testing::{EXTREMES, Lcg, peak_hz};
use m2_kernel::{Cfg, Host, latency_frames};

use super::*;
use crate::fft::Fft;

const MANIFEST: &str = include_str!("../../m2pkg.toml");
const BLOCK: usize = 128;
/// A hop at 48 kHz.
const HOP_48: usize = 256;
const RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];
/// Every note allowed (VOX-12), and C major.
const EVERY: f32 = 4095.0;
const C_MAJOR: f32 = (1 | 1 << 2 | 1 << 4 | 1 << 5 | 1 << 7 | 1 << 9 | 1 << 11) as f32;
/// "Ah": three formants and their widths, in Hz.
const AH: [(f64, f64); 3] = [(700.0, 120.0), (1220.0, 140.0), (2600.0, 200.0)];

// ---------------------------------------------------------------- the made-up singer

fn hz_of(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

fn midi_of(hz: f64) -> f64 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

/// A two-pole resonance at `f` Hz, `bw` wide, about 1 at its peak.
struct Formant {
    a1: f64,
    a2: f64,
    b0: f64,
    y: [f64; 2],
}

impl Formant {
    fn new(sr: f64, f: f64, bw: f64) -> Self {
        let r = (-std::f64::consts::PI * bw / sr).exp();
        let th = 2.0 * std::f64::consts::PI * f / sr;
        Formant { a1: 2.0 * r * th.cos(), a2: -r * r, b0: (1.0 - r) * (1.0 - 2.0 * r * (2.0 * th).cos() + r * r).sqrt(), y: [0.0; 2] }
    }

    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.a1 * self.y[0] + self.a2 * self.y[1];
        self.y = [y, self.y[0]];
        y
    }
}

/// What the singer sings: `midi(t)` (t in seconds), its pitch wobbling by `vibrato` semitones at
/// 5.5 Hz, the vowel "ah" with `formants`; and, from `s.0` to `s.1` seconds, "s" instead.
struct Song<'a> {
    secs: f64,
    midi: &'a dyn Fn(f64) -> f64,
    vibrato: f64,
    formants: [(f64, f64); 3],
    s: Option<(f64, f64)>,
}

impl Song<'_> {
    /// The voice at `sr`, mono, peaking near 0.5.
    fn sing(&self, sr: f32) -> Vec<f32> {
        let sr64 = sr as f64;
        let n = (self.secs * sr64) as usize;
        // Each glottal pulse a raised cosine 0.4 ms long, at the moment the phase wraps (between
        // samples), so the pitch is exact; then its slope (the lips), so no DC.
        let width = 0.0004;
        let pulse = |tau: f64| if (0.0..width).contains(&tau) { 0.5 - 0.5 * (std::f64::consts::TAU * tau / width).cos() } else { 0.0 };
        let mut filters = self.formants.map(|(f, bw)| Formant::new(sr64, f, bw));
        let (mut phase, mut pulses, mut last) = (0.0f64, [f64::NEG_INFINITY; 2], 0.0f64);
        let mut noise = Lcg(7);
        let mut out = Vec::with_capacity(n);
        let mut noise_last = 0.0f64;
        for i in 0..n {
            let t = i as f64 / sr64;
            let f = hz_of((self.midi)(t) + self.vibrato * (std::f64::consts::TAU * 5.5 * t).sin());
            phase += f / sr64;
            if phase >= 1.0 {
                phase -= 1.0;
                pulses = [pulses[1], t - phase / f];
            }
            let g = pulses.iter().map(|&p| pulse(t - p)).sum::<f64>();
            let mut v = g - last;
            last = g;
            for f in &mut filters {
                v = f.tick(v);
            }
            // "s": white noise, its slope (bright), fading in and out over 5 ms.
            let (mut voice, mut hiss) = (1.0, 0.0);
            if let Some((a, b)) = self.s {
                let fade = ((t - a).min(b - t) / 0.005).clamp(0.0, 1.0);
                (voice, hiss) = (1.0 - fade, fade);
            }
            let w = noise.next() as f64 / 32_768.0 - 1.0;
            let h = w - noise_last;
            noise_last = w;
            out.push((voice * v * 6.0 + hiss * h * 0.15) as f32);
        }
        out
    }
}

/// One steady note, no "s".
fn steady(midi: f64, vibrato: f64, secs: f64) -> Song<'static> {
    let m: &'static dyn Fn(f64) -> f64 = Box::leak(Box::new(move |_| midi));
    Song { secs, midi: m, vibrato, formants: AH, s: None }
}

// ---------------------------------------------------------------- playing the kernels

fn cfg(sr: f32, channels: usize) -> Cfg {
    Cfg::new(sr, BLOCK, channels)
}

/// What Tune's three kernels made, frame by frame.
struct Tuned {
    out: Vec<[f32; 2]>,
    hz: Vec<f32>,
    clarity: Vec<f32>,
    semitones: Vec<f32>,
}

/// Tune's chain, as the pack joins it (VOX-20): pitch, then snap, then shift, on `input` (both
/// channels; `channels` 1 reads the left), `split` frames at a time.
#[allow(clippy::too_many_arguments)]
fn tune_split(sr: f32, channels: usize, input: &[[f32; 2]], key: f32, glide: f32, amount: f32, formant: f32, split: usize) -> Tuned {
    let (mut pitch, mut snap, mut shift) =
        (Host::<Pitch>::of(cfg(sr, channels)), Host::<Snap>::of(cfg(sr, 1)), Host::<Shift>::of(cfg(sr, channels)));
    let mut t = Tuned { out: vec![], hz: vec![], clarity: vec![], semitones: vec![] };
    let mut at = 0;
    while at < input.len() {
        let n = split.clamp(1, BLOCK).min(input.len() - at);
        for c in 0..2 {
            let buf = pitch.port_mut(0);
            for k in 0..n {
                buf[c * BLOCK + k] = input[at + k][c];
            }
        }
        pitch.process(n as u32);
        let hz: Vec<f32> = pitch.port_mut(1)[..n].to_vec();
        let clarity: Vec<f32> = pitch.port_mut(2)[..n].to_vec();
        for (port, values) in [(0, &hz[..]), (1, &clarity[..])] {
            snap.port_mut(port)[..n].copy_from_slice(values);
        }
        for (port, v) in [(2, key), (3, glide), (4, amount)] {
            snap.port_mut(port)[..n].fill(v);
        }
        snap.process(n as u32);
        let semitones: Vec<f32> = snap.port_mut(5)[..n].to_vec();
        for c in 0..2 {
            let buf = shift.port_mut(0);
            for k in 0..n {
                buf[c * BLOCK + k] = input[at + k][c];
            }
        }
        for (port, values) in [(1, &semitones[..]), (2, &hz[..]), (3, &clarity[..])] {
            shift.port_mut(port)[..n].copy_from_slice(values);
        }
        shift.port_mut(4)[..n].fill(formant);
        shift.process(n as u32);
        let o = shift.port_mut(5);
        t.out.extend((0..n).map(|k| [o[k], o[BLOCK + k]]));
        t.hz.extend(hz);
        t.clarity.extend(clarity);
        t.semitones.extend(semitones);
        at += n;
    }
    t
}

fn tune(sr: f32, voice: &[f32], key: f32, glide: f32, amount: f32) -> Tuned {
    let input: Vec<[f32; 2]> = voice.iter().map(|&v| [v, v]).collect();
    tune_split(sr, 1, &input, key, glide, amount, 0.0, BLOCK)
}

/// `shift` alone, at a fixed move, its pitch input from `pitch` (as Tune feeds it).
fn shifted(sr: f32, voice: &[f32], semitones: f32, formant: f32) -> Vec<f32> {
    let input: Vec<[f32; 2]> = voice.iter().map(|&v| [v, v]).collect();
    let (mut pitch, mut shift) = (Host::<Pitch>::of(cfg(sr, 1)), Host::<Shift>::of(cfg(sr, 1)));
    let mut out = vec![];
    for chunk in input.chunks(BLOCK) {
        let n = chunk.len();
        for (k, f) in chunk.iter().enumerate() {
            pitch.port_mut(0)[k] = f[0];
            shift.port_mut(0)[k] = f[0];
        }
        pitch.process(n as u32);
        let (hz, clarity) = (pitch.port_mut(1)[..n].to_vec(), pitch.port_mut(2)[..n].to_vec());
        shift.port_mut(1)[..n].fill(semitones);
        shift.port_mut(2)[..n].copy_from_slice(&hz);
        shift.port_mut(3)[..n].copy_from_slice(&clarity);
        shift.port_mut(4)[..n].fill(formant);
        shift.process(n as u32);
        out.extend_from_slice(&shift.port_mut(5)[..n]);
    }
    out
}

fn left(x: &[[f32; 2]]) -> Vec<f32> {
    x.iter().map(|f| f[0]).collect()
}

/// `x` from `from` seconds, `secs` long.
fn part(x: &[f32], sr: f32, from: f64, secs: f64) -> &[f32] {
    let a = (from * sr as f64) as usize;
    &x[a..a + (secs * sr as f64) as usize]
}

/// The pitch an FFT's peak finds within a semitone and a half of `near` Hz (another method than
/// YIN's).
fn fft_hz(x: &[f32], sr: f32, near: f64) -> f64 {
    let k = 2f64.powf(1.5 / 12.0);
    peak_hz(x, sr, near / k, near * k)
}

fn median(v: &[f32]) -> f32 {
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    s[s.len() / 2]
}

/// The three formants of a steady vowel `x`: brought down to a sixth of its rate (a windowed
/// sinc), then the peaks of its linear prediction's envelope (order 10, autocorrelation and
/// Levinson-Durbin), each refined by a parabola. A voice is pulses through resonances, which is
/// what linear prediction models, so it reads the resonances and not the harmonics.
fn formants(x: &[f32], sr: f32) -> [f64; 3] {
    let down = 6;
    let taps = 255;
    let cut = 0.9 * 0.5 / down as f64;
    let h: Vec<f64> = (0..taps)
        .map(|i| {
            let m = i as f64 - (taps / 2) as f64;
            let sinc = if m == 0.0 { 2.0 * cut } else { (std::f64::consts::TAU * cut * m).sin() / (std::f64::consts::PI * m) };
            sinc * (0.54 - 0.46 * (std::f64::consts::TAU * i as f64 / (taps - 1) as f64).cos())
        })
        .collect();
    let y: Vec<f64> = (taps..x.len()).step_by(down).map(|i| (0..taps).map(|k| h[k] * x[i - k] as f64).sum()).collect();
    let n = y.len();
    let w: Vec<f64> = (0..n).map(|i| y[i] * (0.54 - 0.46 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos())).collect();
    const P: usize = 10;
    let r: Vec<f64> = (0..=P).map(|l| (l..n).map(|i| w[i] * w[i - l]).sum()).collect();
    let (mut a, mut e) = (vec![0.0; P + 1], r[0]);
    a[0] = 1.0;
    for i in 1..=P {
        let k = -(0..i).map(|j| a[j] * r[i - j]).sum::<f64>() / e;
        let prev = a.clone();
        for j in 1..i {
            a[j] = prev[j] + k * prev[i - j];
        }
        a[i] = k;
        e *= 1.0 - k * k;
    }
    let rate = sr as f64 / down as f64;
    let env = |f: f64| -> f64 {
        let w = std::f64::consts::TAU * f / rate;
        let (re, im) =
            a.iter().enumerate().fold((0.0, 0.0), |(re, im), (j, c)| (re + c * (w * j as f64).cos(), im - c * (w * j as f64).sin()));
        -(re * re + im * im).ln()
    };
    [(500.0, 950.0), (950.0, 1700.0), (2200.0, 3200.0)].map(|(lo, hi)| {
        let step = 1.0;
        let k = (0..((hi - lo) / step) as usize).max_by(|&i, &j| env(lo + i as f64 * step).total_cmp(&env(lo + j as f64 * step))).unwrap();
        let f = lo + k as f64 * step;
        let (l, c, r) = (env(f - step), env(f), env(f + step));
        f + step * 0.5 * (l - r) / (l - 2.0 * c + r)
    })
}

// ---------------------------------------------------------------- pitch

// VOX-21: the pitch pitch finds is the one an FFT's peak finds, within 3 cents, from 80 Hz to a
// kilohertz, at every rate.
#[test]
fn pitch_finds_the_pitch_an_fft_peak_finds() {
    for sr in RATES {
        for midi in [40.0, 45.5, 57.0, 64.3, 69.0, 76.0, 83.0] {
            let voice = steady(midi, 0.0, 1.0).sing(sr);
            let hz: Vec<f32> = {
                let mut p = Host::<Pitch>::of(cfg(sr, 1));
                let mut got = vec![];
                for chunk in voice.chunks(BLOCK) {
                    p.port_mut(0)[..chunk.len()].copy_from_slice(chunk);
                    p.process(chunk.len() as u32);
                    got.extend_from_slice(&p.port_mut(1)[..chunk.len()]);
                }
                got
            };
            let found = median(part(&hz, sr, 0.3, 0.6)) as f64;
            let fft = fft_hz(part(&voice, sr, 0.2, 0.8), sr, hz_of(midi));
            assert!(cents(found, fft).abs() < 3.0, "{sr} Hz, note {midi}: pitch says {found:.2} Hz, an FFT's peak {fft:.2} Hz");
            assert!(cents(fft, hz_of(midi)).abs() < 3.0, "{sr} Hz, note {midi}: the singer sings {fft:.2}, not {:.2}", hz_of(midi));
        }
    }
}

// VOX-21: "s", a breath and silence have no pitch: clarity low, and silence 0 Hz.
#[test]
fn pitch_hears_no_pitch_in_an_s_or_silence() {
    let sr = 48_000.0;
    let song = Song { secs: 1.5, midi: &|_| 60.0, vibrato: 0.3, formants: AH, s: Some((0.5, 1.0)) };
    let voice = song.sing(sr);
    let mut input: Vec<[f32; 2]> = voice.iter().map(|&v| [v, v]).collect();
    input.extend(std::iter::repeat_n([0.0; 2], 24_000));
    let t = tune_split(sr, 1, &input, EVERY, 0.0, 1.0, 0.0, BLOCK);
    assert!(median(part(&t.clarity, sr, 0.2, 0.25)) > 0.8, "a sung note is clear");
    assert!(part(&t.clarity, sr, 0.65, 0.3).iter().all(|&c| c < 0.5), "an s is not: {}", median(part(&t.clarity, sr, 0.65, 0.3)));
    assert!(part(&t.hz, sr, 1.6, 0.3).iter().all(|&h| h == 0.0), "silence has no pitch");
    assert!(part(&t.clarity, sr, 1.6, 0.3).iter().all(|&c| c == 0.0), "and no clarity");
}

// VOX-21 (review U2): pitch reads every 5.33 ms counted from its birth (235 samples at 44.1 kHz,
// 256 at 48, 512 at 96), and only the past: its readings up to a moment don't hang on what comes
// after it, nor on how its blocks are split.
#[test]
fn pitch_reads_every_5_ms_and_only_the_past() {
    for (sr, hop) in [(44_100.0, 235), (48_000.0, 256), (96_000.0, 512)] {
        assert_eq!(yin::hop(sr), hop, "{sr} Hz");
        let a = Song { secs: 1.0, midi: &|t| 60.0 + 4.0 * t, vibrato: 0.2, formants: AH, s: None }.sing(sr);
        let mut b = a.clone();
        let cut = (0.6 * sr) as usize;
        b[cut..].iter_mut().for_each(|v| *v = -*v * 0.3);
        let read = |x: &[f32], split: usize| -> Vec<f32> {
            let input: Vec<[f32; 2]> = x.iter().map(|&v| [v, v]).collect();
            tune_split(sr, 1, &input, EVERY, 0.0, 1.0, 0.0, split).hz
        };
        let (ra, rb) = (read(&a, BLOCK), read(&b, 37));
        assert!(ra[..cut] == rb[..cut], "{sr} Hz: what came later changed an earlier reading");
        let changes: Vec<usize> = (1..ra.len()).filter(|&i| ra[i] != ra[i - 1]).collect();
        assert!(changes.len() > 100, "{sr} Hz: it reads a moving voice: {} changes", changes.len());
        assert!(changes.iter().all(|i| i % hop == hop - 1), "{sr} Hz: a reading changes only at a hop: {:?}", &changes[..4]);
    }
}

// ---------------------------------------------------------------- snap

/// Snap's semitones for a steady sung `midi` (clarity 1), after `secs`.
fn snapped(midi: f64, key: f32, glide: f32, amount: f32) -> f32 {
    let mut s = Host::<Snap>::of(cfg(48_000.0, 1));
    let mut last = 0.0;
    for _ in 0..40 {
        for (port, v) in [(0, hz_of(midi) as f32), (1, 1.0), (2, key), (3, glide), (4, amount)] {
            s.port_mut(port)[..BLOCK].fill(v);
        }
        s.process(BLOCK as u32);
        last = s.port_mut(5)[BLOCK - 1];
    }
    last
}

// VOX-22: snap moves a sung note to the nearest note of the key, the song's key as a keyset;
// with every note allowed, to the nearest note; an octave off, the same move.
#[test]
fn snap_moves_to_the_nearest_note_of_the_key() {
    let near = |a: f32, b: f64| (a as f64 - b).abs() < 1e-3;
    assert!(near(snapped(63.3, C_MAJOR, 0.0, 1.0), 0.7), "D# 30 cents sharp goes up to E in C major");
    assert!(near(snapped(63.3, EVERY, 0.0, 1.0), -0.3), "and down to D# with every note allowed");
    assert!(near(snapped(66.4, C_MAJOR, 0.0, 1.0), 0.6), "F# 40 cents sharp, up to G (nearer than F)");
    for octave in [-24.0, -12.0, 12.0] {
        assert!(near(snapped(63.3 + octave, C_MAJOR, 0.0, 1.0), 0.7), "an octave off ({octave}): the same move");
    }
    assert!(near(snapped(61.0, 1.0, 0.0, 1.0), -1.0), "a key of C alone: C# goes down to C");
    assert!(near(snapped(66.0, 1.0, 0.0, 1.0), -6.0) || near(snapped(66.0, 1.0, 0.0, 1.0), 6.0), "a tritone away: either C");
}

// VOX-22: amount scales the move; glide 0 jumps at once, and a glide eases toward it.
#[test]
fn amount_scales_and_glide_eases() {
    assert!((snapped(64.4, EVERY, 0.0, 0.5) + 0.2).abs() < 1e-3, "half of 40 cents");
    assert_eq!(snapped(64.4, EVERY, 0.0, 0.0), 0.0, "amount 0 moves nothing");
    let mut s = Host::<Snap>::of(cfg(48_000.0, 1));
    let mut got = vec![];
    for _ in 0..375 {
        for (port, v) in [(0, hz_of(64.4) as f32), (1, 1.0), (2, EVERY), (3, 0.1), (4, 1.0)] {
            s.port_mut(port)[..BLOCK].fill(v);
        }
        s.process(BLOCK as u32);
        got.extend_from_slice(&s.port_mut(5)[..BLOCK]);
    }
    let at = |secs: f64| got[(secs * 48_000.0) as usize];
    assert!((at(0.1) + 0.4 * (1.0 - (-1.0f32).exp())).abs() < 0.01, "a glide of 0.1 s is 63% of the way after 0.1 s: {}", at(0.1));
    assert!((at(0.99) + 0.4).abs() < 1e-3, "and there after a second: {}", at(0.99));
    let mut jump = Host::<Snap>::of(cfg(48_000.0, 1));
    for (port, v) in [(0, hz_of(64.4) as f32), (1, 1.0), (2, EVERY), (3, 0.0), (4, 1.0)] {
        jump.port_mut(port)[..BLOCK].fill(v);
    }
    jump.process(BLOCK as u32);
    assert!((jump.port_mut(5)[0] + 0.4).abs() < 1e-4, "glide 0 is there at the first sample");
}

// VOX-22: the current note is kept until another is nearer by 0.2 semitones, both ways.
#[test]
fn the_current_note_is_kept_by_a_fifth_of_a_semitone() {
    let mut s = Host::<Snap>::of(cfg(48_000.0, 1));
    let mut aim = |midi: f64| -> f64 {
        for (port, v) in [(0, hz_of(midi) as f32), (1, 1.0), (2, EVERY), (3, 0.0), (4, 1.0)] {
            s.port_mut(port)[..BLOCK].fill(v);
        }
        s.process(BLOCK as u32);
        (midi + s.port_mut(5)[BLOCK - 1] as f64).round()
    };
    let up: Vec<f64> = [64.0, 64.45, 64.55, 64.65, 65.0].map(&mut aim).to_vec();
    assert_eq!(up, [64.0, 64.0, 64.0, 65.0, 65.0], "going up it leaves E only past 64.6");
    let down: Vec<f64> = [64.55, 64.45, 64.35, 64.0].map(&mut aim).to_vec();
    assert_eq!(down, [65.0, 65.0, 64.0, 64.0], "coming down it leaves F only below 64.4");
}

// VOX-22: no clear pitch (an "s", a breath) eases back to 0, over the glide; a key that isn't a
// whole number from 1 to 4095 keeps the last good one.
#[test]
fn no_pitch_eases_back_and_a_bad_key_keeps_the_last() {
    let mut s = Host::<Snap>::of(cfg(48_000.0, 1));
    let mut run = |hz: f32, clarity: f32, key: f32, blocks: usize| -> f32 {
        for _ in 0..blocks {
            for (port, v) in [(0, hz), (1, clarity), (2, key), (3, 0.0), (4, 1.0)] {
                s.port_mut(port)[..BLOCK].fill(v);
            }
            s.process(BLOCK as u32);
        }
        s.port_mut(5)[BLOCK - 1]
    };
    assert!((run(hz_of(63.3) as f32, 1.0, C_MAJOR, 2) - 0.7).abs() < 1e-3);
    assert_eq!(run(hz_of(63.3) as f32, 0.2, C_MAJOR, 2), 0.0, "unclear: no move");
    assert_eq!(run(0.0, 1.0, C_MAJOR, 2), 0.0, "no pitch: no move");
    for bad in [0.0, 4096.0, 100.5, f32::NAN, -3.0] {
        assert!((run(hz_of(63.3) as f32, 1.0, bad, 2) - 0.7).abs() < 1e-3, "key {bad}: C major still");
    }
}

// ---------------------------------------------------------------- shift

// VOX-23, VOX-24: with no move, shift gives the voice back exactly as it came, as late as its
// manifest says at each rate (0.015 s, rounded up): a sung note, an "s" and silence.
#[test]
fn with_no_move_the_voice_comes_back_15_ms_late() {
    assert!(MANIFEST.contains("latency_s = 0.015") && LATENCY_S == 0.015, "the manifest says the latency the kernel keeps");
    for sr in RATES {
        let late = latency_frames(LATENCY_S, sr as f64) as usize;
        let song = Song { secs: 1.2, midi: &|t| 57.0 + 3.0 * t, vibrato: 0.3, formants: AH, s: Some((0.6, 0.9)) };
        let voice = song.sing(sr);
        let out = shifted(sr, &voice, 0.0, 0.0);
        assert!(out[..late].iter().all(|&v| v == 0.0), "{sr} Hz: nothing before its latency");
        let worst = (late..voice.len()).map(|i| (out[i] - voice[i - late]).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-5, "{sr} Hz: the voice {late} frames late, off by {worst}");
    }
}

// VOX-23: shift moves the pitch by its semitones, as an FFT's peak measures, up and down.
#[test]
fn shift_moves_the_pitch_an_fft_peak_measures() {
    for sr in RATES {
        for (midi, by) in [(50.0, 4.0), (57.0, -3.0), (64.0, 1.0), (71.0, -7.0), (45.0, 7.0), (52.0, 12.0)] {
            let voice = steady(midi, 0.0, 1.2).sing(sr);
            let out = shifted(sr, &voice, by as f32, 0.0);
            let got = fft_hz(part(&out, sr, 0.3, 0.8), sr, hz_of(midi + by));
            assert!(
                cents(got, hz_of(midi + by)).abs() < 5.0,
                "{sr} Hz: note {midi} moved by {by}: {got:.2} Hz, not {:.2}",
                hz_of(midi + by)
            );
        }
    }
}

/// The singer's own formants, as its filters make them: the peaks of one pulse's spectrum through
/// them (no harmonics to read between), each refined by a parabola.
fn formants_made(sr: f32) -> [f64; 3] {
    let n = 1 << 16;
    let sr64 = sr as f64;
    let mut filters = AH.map(|(f, bw)| Formant::new(sr64, f, bw));
    let width = 0.0004;
    let (mut last, mut re) = (0.0, vec![0.0; n]);
    for (i, r) in re.iter_mut().enumerate() {
        let t = i as f64 / sr64;
        let g = if t < width { 0.5 - 0.5 * (std::f64::consts::TAU * t / width).cos() } else { 0.0 };
        let mut v = g - last;
        last = g;
        for f in &mut filters {
            v = f.tick(v);
        }
        *r = v;
    }
    let mut im = vec![0.0; n];
    Fft::new(n).run(&mut re, &mut im, false);
    let p: Vec<f64> = (0..n).map(|k| (re[k] * re[k] + im[k] * im[k] + 1e-30).ln()).collect();
    let bin = sr64 / n as f64;
    [(500.0, 950.0), (950.0, 1700.0), (2200.0, 3200.0)].map(|(lo, hi)| {
        let k = ((lo / bin) as usize..=(hi / bin) as usize).max_by(|&i, &j| p[i].total_cmp(&p[j])).unwrap();
        (k as f64 + 0.5 * (p[k - 1] - p[k + 1]) / (p[k - 1] - 2.0 * p[k] + p[k + 1])) * bin
    })
}

// VOX-23 (review U1): shift moves from 7 semitones down to 12 up, where its grains overlap; a move
// past either end is that end, as its manifest's range says. An octave up is an octave up.
#[test]
fn shift_reaches_7_down_and_12_up() {
    assert!(MANIFEST.contains("\"semitones: control<semitones> = 0 in -7..12\""), "the manifest's range");
    let sr = 48_000.0;
    for (midi, asked, by) in [(57.0, -12.0, -7.0), (57.0, -7.0, -7.0), (52.0, 12.0, 12.0), (52.0, 20.0, 12.0)] {
        let voice = steady(midi, 0.0, 1.2).sing(sr);
        let out = shifted(sr, &voice, asked as f32, 0.0);
        let got = fft_hz(part(&out, sr, 0.3, 0.8), sr, hz_of(midi + by));
        assert!(cents(got, hz_of(midi + by)).abs() < 5.0, "note {midi} asked to move {asked}: {:.2}, not {}", midi_of(got), midi + by);
    }
}

// VOX-23 (D5): shifted by 4 semitones, the vowel keeps its formants, each within 5%.
#[test]
fn a_4_semitone_shift_keeps_the_formants_within_5_percent() {
    let sr = 48_000.0;
    for (midi, by) in [(40.0, 4.0), (47.0, 4.0), (47.0, -4.0), (51.0, -4.0)] {
        let voice = steady(midi, 0.2, 1.5).sing(sr);
        let out = shifted(sr, &voice, by, 0.0);
        let (before, after) = (formants(part(&voice, sr, 0.3, 0.7), sr), formants(part(&out, sr, 0.3, 0.7), sr));
        for (i, (b, a)) in before.iter().zip(&after).enumerate() {
            assert!((a / b - 1.0).abs() < 0.05, "note {midi} by {by}: formant {} at {b:.0} Hz moved to {a:.0} Hz", i + 1);
            let made = formants_made(sr)[i];
            assert!((b / made - 1.0).abs() < 0.05, "the measure finds the singer's formant {} at {b:.0}, not {made:.0}", i + 1);
        }
    }
}

// VOX-23: formant moves the colour on its own: up 4 semitones, the formants rise by about 26%,
// and the pitch stays.
#[test]
fn formant_moves_the_colour_alone() {
    let sr = 48_000.0;
    let voice = steady(47.0, 0.2, 1.5).sing(sr);
    let out = shifted(sr, &voice, 0.0, 4.0);
    let got = fft_hz(part(&out, sr, 0.3, 0.8), sr, hz_of(47.0));
    assert!(cents(got, hz_of(47.0)).abs() < 5.0, "the pitch stays: {got:.2}");
    let (before, after) = (formants(part(&voice, sr, 0.3, 0.7), sr), formants(part(&out, sr, 0.3, 0.7), sr));
    for i in 0..2 {
        let ratio = after[i] / before[i];
        assert!((ratio / 2f64.powf(4.0 / 12.0) - 1.0).abs() < 0.08, "formant {}: {:.0} to {:.0} Hz", i + 1, before[i], after[i]);
    }
}

// VOX-23: an "s" passes through: the hiss comes out as it went in, only late.
#[test]
fn an_s_passes_through() {
    let sr = 48_000.0;
    let late = latency_frames(LATENCY_S, sr as f64) as usize;
    let song = Song { secs: 1.5, midi: &|_| 62.4, vibrato: 0.3, formants: AH, s: Some((0.6, 1.0)) };
    let voice = song.sing(sr);
    let t = tune(sr, &voice, C_MAJOR, 0.0, 1.0);
    let out = left(&t.out);
    let (a, b) = ((0.7 * sr) as usize, (0.9 * sr) as usize);
    let worst = (a..b).map(|i| (out[i + late] - voice[i]).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-5, "the s, as it came, off by {worst}");
}

// VOX-23: both channels move by the same pulses, found in their mono sum: a right channel at half
// the left comes out at half the left; a voice on the left alone still has its pitch found, and a
// silent right stays silent.
#[test]
fn stereo_shares_its_pulses_from_the_mono_sum() {
    let sr = 48_000.0;
    let voice = steady(57.3, 0.3, 1.2).sing(sr);
    let half: Vec<[f32; 2]> = voice.iter().map(|&v| [v, 0.5 * v]).collect();
    let t = tune_split(sr, 2, &half, C_MAJOR, 0.0, 1.0, 0.0, BLOCK);
    let worst = t.out.iter().map(|f| (f[1] - 0.5 * f[0]).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "the right is half the left, off by {worst}");
    let one: Vec<[f32; 2]> = voice.iter().map(|&v| [v, 0.0]).collect();
    let t = tune_split(sr, 2, &one, C_MAJOR, 0.0, 1.0, 0.0, BLOCK);
    assert!(t.out.iter().all(|f| f[1] == 0.0), "a silent right stays silent");
    let got = fft_hz(part(&left(&t.out), sr, 0.4, 0.7), sr, hz_of(57.0));
    assert!(cents(got, hz_of(57.0)).abs() < 5.0, "the left, tuned to A from its mono sum: {got:.2}");
}

// ---------------------------------------------------------------- Tune: the three in a row

// VOX-20: a note sung 30 cents sharp comes out on its note; at amount 0.5, halfway; with no key
// (every note allowed), on the nearest note; in C major, on the key's.
#[test]
fn tune_puts_a_sung_note_on_the_key() {
    let sr = 48_000.0;
    for (midi, key, amount, want) in [
        (64.3, EVERY, 1.0, 64.0),
        (64.3, EVERY, 0.5, 64.15),
        (63.3, EVERY, 1.0, 63.0),
        (63.3, C_MAJOR, 1.0, 64.0),
        (58.3, C_MAJOR, 1.0, 59.0),
    ] {
        let voice = steady(midi, 0.0, 1.2).sing(sr);
        let t = tune(sr, &voice, key, 0.0, amount);
        let got = fft_hz(part(&left(&t.out), sr, 0.3, 0.8), sr, hz_of(want));
        assert!(cents(got, hz_of(want)).abs() < 4.0, "sung {midi}, amount {amount}: {:.3}, not {want}", midi_of(got));
    }
}

// VOX-20, VOX-22 (D4's Robot): at glide 0 a voice gliding up from C to E comes out in steps:
// each moves within one hop of where the sung pitch, as pitch hears it, passes the edge between two
// notes (0.1 semitone past the middle, the bias), and between steps the voice is on its note, as
// an FFT's peak measures.
#[test]
fn at_glide_0_a_gliding_voice_steps_within_one_hop_of_each_note_change() {
    let sr = 48_000.0;
    let glide = |t: f64| 60.0 + 4.0 * ((t - 0.25) / 2.0).clamp(0.0, 1.0);
    let voice = Song { secs: 2.6, midi: &glide, vibrato: 0.0, formants: AH, s: None }.sing(sr);
    let t = tune(sr, &voice, EVERY, 0.0, 1.0);
    // Where the target (the sung pitch plus the move) steps, and where pitch's reading passes an edge.
    let aim: Vec<f64> =
        t.hz.iter().zip(&t.semitones).map(|(&h, &s)| if h > 0.0 { midi_of(h as f64) + s as f64 } else { f64::NAN }).collect();
    let from = (0.3 * sr) as usize;
    let steps: Vec<usize> = (from..aim.len()).filter(|&i| (aim[i] - aim[i - 1]).abs() > 0.5).collect();
    assert_eq!(steps.len(), 4, "four steps from C to E: {steps:?}");
    for (n, &at) in steps.iter().enumerate() {
        let edge = 60.0 + n as f64 + 0.6;
        let passed = (from..aim.len()).find(|&i| midi_of(t.hz[i] as f64) >= edge).unwrap();
        assert!(at.abs_diff(passed) <= HOP_48, "step {} at {at}, the edge passed at {passed}", n + 1);
        assert!(aim[at + 1..at + HOP_48].iter().all(|a| (a - (61.0 + n as f64)).abs() < 1e-3), "on the note after the step");
    }
    // On the sound (review U3), by a plain autocorrelation, 15 ms late: the voice is on a note
    // almost all the time, and moves from one to the next within 10 ms of where the singer's pitch
    // passed their edge (0.1 past the middle).
    let late_s = LATENCY_S;
    let track = heard(&left(&t.out), sr, 62.0, 0.35 + late_s, 2.2 + late_s);
    // A window of 20 ms over a step holds both notes: those within 11 ms of one don't count.
    let steps_heard: Vec<f64> = track.windows(2).filter(|w| w[0].1.round() != w[1].1.round()).map(|w| w[1].0).collect();
    let away: Vec<&(f64, f64)> = track.iter().filter(|(t, _)| steps_heard.iter().all(|s| (t - s).abs() > 0.011)).collect();
    let on = away.iter().filter(|(_, m)| (m - m.round()).abs() < 0.1).count();
    assert!(on as f64 > 0.98 * away.len() as f64, "on a note in {on} of {} milliseconds away from a step", away.len());
    for n in 0..4 {
        let edge = 0.25 + 0.5 * (n as f64 + 0.6);
        let moved = track.iter().find(|(_, m)| m.round() >= 61.0 + n as f64).unwrap().0 - late_s;
        assert!((moved - edge).abs() <= 0.010, "the sound steps to {} at {moved:.4} s, the singer passed the edge at {edge:.4} s", 61 + n);
    }
    // Between the steps, the voice is on its note (an FFT's peak of 40 ms there, 15 ms late).
    let late = latency_frames(LATENCY_S, sr as f64) as usize;
    let out = left(&t.out);
    for (n, w) in steps.windows(2).enumerate() {
        let (a, b) = (w[0] + late + 960, w[1] + late - 960);
        let got = fft_hz(&out[a..b.max(a + 2048)], sr, hz_of(61.0 + n as f64));
        assert!(cents(got, hz_of(61.0 + n as f64)).abs() < 10.0, "between steps {} and {}: {:.3}", n + 1, n + 2, midi_of(got));
    }
}

// VOX-20 (D4's Natural): at a glide of 0.1 s a note's vibrato mostly stays; at 0 it is flattened.
// Measured on the sound by a plain autocorrelation (review U3), not by `pitch`, the kernel under test.
#[test]
fn a_glide_keeps_vibrato_and_robot_flattens_it() {
    let sr = 48_000.0;
    let voice = steady(64.2, 0.4, 2.0).sing(sr);
    let wobble = |x: &[f32]| -> f64 {
        let m: Vec<f64> = heard(x, sr, 64.0, 0.5, 1.9).into_iter().map(|(_, m)| m).collect();
        m.iter().cloned().fold(f64::MIN, f64::max) - m.iter().cloned().fold(f64::MAX, f64::min)
    };
    let tuned = |glide: f32| wobble(&left(&tune(sr, &voice, EVERY, glide, 1.0).out));
    let (sung, natural, robot) = (wobble(&voice), tuned(0.1), tuned(0.0));
    assert!(sung > 0.7, "the singer wobbles by 0.8 semitones: {sung:.2}");
    assert!(natural > 0.8 * sung, "Natural keeps the vibrato: {natural:.2} of {sung:.2} semitones");
    assert!(robot < 0.3 * sung, "Robot flattens it: {robot:.2} of {sung:.2}");
}

/// The pitch of `x` from `from` to `to` seconds, every millisecond, as `(seconds, MIDI)`: the lag
/// of the largest normalised autocorrelation over 20 ms, within 3 semitones of `near`, refined by a
/// parabola. Another method than `pitch`'s (no difference function, no threshold).
fn heard(x: &[f32], sr: f32, near: f64, from: f64, to: f64) -> Vec<(f64, f64)> {
    let sr64 = sr as f64;
    let w = (0.02 * sr64) as usize;
    let (lo, hi) = ((sr64 / hz_of(near + 3.0)) as usize, (sr64 / hz_of(near - 3.0)).ceil() as usize + 1);
    let mut out = vec![];
    let mut t = from;
    while t < to {
        let c = (t * sr64) as usize;
        let a = c - w / 2;
        let r = |lag: usize| -> f64 {
            let (mut xy, mut xx, mut yy) = (0.0, 0.0, 0.0);
            for i in a..a + w {
                let (p, q) = (x[i] as f64, x[i + lag] as f64);
                xy += p * q;
                xx += p * p;
                yy += q * q;
            }
            xy / (xx * yy).sqrt().max(1e-30)
        };
        let rs: Vec<f64> = (lo - 1..=hi + 1).map(r).collect();
        let k = (1..rs.len() - 1).max_by(|&i, &j| rs[i].total_cmp(&rs[j])).unwrap();
        let bend = rs[k - 1] - 2.0 * rs[k] + rs[k + 1];
        let lag = (lo - 1 + k) as f64 + if bend < 0.0 { 0.5 * (rs[k - 1] - rs[k + 1]) / bend } else { 0.0 };
        out.push((t, midi_of(sr64 / lag)));
        t += 0.001;
    }
    out
}

// ---------------------------------------------------------------- what every kernel guarantees

mod counting {
    //! A count of what this thread allocates and frees.
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static HERE: Cell<u64> = const { Cell::new(0) };
    }

    pub struct Counting;

    fn count() {
        let _ = HERE.try_with(|c| c.set(c.get() + 1));
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count();
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            count();
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            count();
            unsafe { System.realloc(ptr, layout, size) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    /// What this thread has allocated and freed so far.
    pub fn here() -> u64 {
        HERE.with(Cell::get)
    }
}

// RENDER-28, ABI-5: once born, the three kernels allocate nothing while they play, at 44.1, 48
// and 96 kHz: a voice gliding, with vibrato and an "s", in stereo, shifted and with its formants
// moved, then silence.
#[test]
fn tune_allocates_nothing_while_it_plays_at_every_rate() {
    for sr in RATES {
        let song = Song { secs: 1.5, midi: &|t| 50.0 + 8.0 * t, vibrato: 0.5, formants: AH, s: Some((0.7, 0.9)) };
        let voice = song.sing(sr);
        let (mut pitch, mut snap, mut shift) = (Host::<Pitch>::of(cfg(sr, 2)), Host::<Snap>::of(cfg(sr, 1)), Host::<Shift>::of(cfg(sr, 2)));
        let blocks = voice.len() / BLOCK + 100;
        let before = counting::here();
        for b in 0..blocks {
            for k in 0..BLOCK {
                let v = voice.get(b * BLOCK + k).copied().unwrap_or(0.0);
                pitch.port_mut(0)[k] = v;
                pitch.port_mut(0)[BLOCK + k] = -0.5 * v;
            }
            pitch.process(BLOCK as u32);
            for k in 0..BLOCK {
                let (hz, clarity) = (pitch.port_mut(1)[k], pitch.port_mut(2)[k]);
                snap.port_mut(0)[k] = hz;
                snap.port_mut(1)[k] = clarity;
                shift.port_mut(2)[k] = hz;
                shift.port_mut(3)[k] = clarity;
                let v = pitch.port_mut(0)[k];
                shift.port_mut(0)[k] = v;
                shift.port_mut(0)[BLOCK + k] = -0.5 * v;
            }
            for (port, v) in [(2, C_MAJOR), (3, 0.05), (4, 1.0)] {
                snap.port_mut(port)[..BLOCK].fill(v);
            }
            snap.process(BLOCK as u32);
            for k in 0..BLOCK {
                shift.port_mut(1)[k] = snap.port_mut(5)[k] + if b % 200 < 100 { 5.0 } else { -5.0 };
            }
            shift.port_mut(4)[..BLOCK].fill(if b % 300 < 150 { 3.0 } else { -2.0 });
            shift.process(BLOCK as u32);
        }
        assert_eq!(counting::here() - before, 0, "{sr} Hz: no allocation in {blocks} blocks");
        assert!(shift.port_mut(5).iter().all(|v| v.is_finite()));
    }
}

// RENDER-2, RENDER-33: the same bits however the host splits its blocks, and a reset starts over.
#[test]
fn the_same_bits_however_blocks_split_and_after_a_reset() {
    let sr = 48_000.0;
    let song = Song { secs: 1.0, midi: &|t| 55.0 + 5.0 * t, vibrato: 0.4, formants: AH, s: Some((0.5, 0.7)) };
    let input: Vec<[f32; 2]> = song.sing(sr).iter().map(|&v| [v, 0.25 * v]).collect();
    let whole = tune_split(sr, 2, &input, C_MAJOR, 0.03, 0.8, 1.0, BLOCK);
    for split in [1, 17, 64, 127] {
        let t = tune_split(sr, 2, &input, C_MAJOR, 0.03, 0.8, 1.0, split);
        assert!(t.out == whole.out && t.semitones == whole.semitones, "split {split}: other bits");
    }
    // A reset is a birth: the same bits again.
    let mut shift = Host::<Shift>::of(cfg(sr, 2));
    let play = |shift: &mut Host<Shift>| -> Vec<f32> {
        let mut got = vec![];
        for (b, chunk) in input.chunks(BLOCK).enumerate() {
            for (k, f) in chunk.iter().enumerate() {
                shift.port_mut(0)[k] = f[0];
                shift.port_mut(0)[BLOCK + k] = f[1];
                shift.port_mut(1)[k] = whole.semitones[b * BLOCK + k];
                shift.port_mut(2)[k] = whole.hz[b * BLOCK + k];
                shift.port_mut(3)[k] = whole.clarity[b * BLOCK + k];
                shift.port_mut(4)[k] = 1.0;
            }
            shift.process(chunk.len() as u32);
            got.extend_from_slice(&shift.port_mut(5)[..chunk.len()]);
        }
        got
    };
    let first = play(&mut shift);
    shift.reset();
    assert!(play(&mut shift) == first, "a reset doesn't start over");
    assert!(first == left(&whole.out), "the shift alone plays the chain's bits");
}

// Anything in: no NaN out, and nothing much louder than what came in, whatever the inputs say.
#[test]
fn no_input_breaks_them() {
    let sr = 48_000.0;
    let voice = steady(60.0, 0.3, 0.5).sing(sr);
    let mut rng = Lcg(3);
    for round in 0..60 {
        let pick = |rng: &mut Lcg| EXTREMES[rng.next() as usize % EXTREMES.len()];
        let (semis, hz, clarity, formant, key, glide, amount) =
            (pick(&mut rng), pick(&mut rng), pick(&mut rng), pick(&mut rng), pick(&mut rng), pick(&mut rng), pick(&mut rng));
        let audio = if round % 3 == 0 { pick(&mut rng) } else { 0.0 };
        let (mut pitch, mut snap, mut shift) = (Host::<Pitch>::of(cfg(sr, 2)), Host::<Snap>::of(cfg(sr, 1)), Host::<Shift>::of(cfg(sr, 2)));
        for b in 0..(voice.len() / BLOCK) {
            for k in 0..BLOCK {
                let v = if round % 3 == 0 { audio } else { voice[b * BLOCK + k] };
                pitch.port_mut(0)[k] = v;
                pitch.port_mut(0)[BLOCK + k] = v;
                shift.port_mut(0)[k] = v;
                shift.port_mut(0)[BLOCK + k] = v;
            }
            pitch.process(BLOCK as u32);
            for (port, v) in [(0, hz), (1, clarity), (2, key), (3, glide), (4, amount)] {
                snap.port_mut(port)[..BLOCK].fill(v);
            }
            snap.process(BLOCK as u32);
            let hzs: Vec<f32> = pitch.port_mut(1)[..BLOCK].to_vec();
            for (k, &heard) in hzs.iter().enumerate() {
                shift.port_mut(1)[k] = if round % 2 == 0 { semis } else { snap.port_mut(5)[k] };
                shift.port_mut(2)[k] = if round % 2 == 0 { hz } else { heard };
                shift.port_mut(3)[k] = clarity;
                shift.port_mut(4)[k] = formant;
            }
            shift.process(BLOCK as u32);
            let outs = [
                pitch.port_mut(1)[..BLOCK].to_vec(),
                pitch.port_mut(2)[..BLOCK].to_vec(),
                snap.port_mut(5)[..BLOCK].to_vec(),
                shift.port_mut(5).to_vec(),
            ];
            for (i, o) in outs.iter().enumerate() {
                assert!(o.iter().all(|v| v.is_finite()), "round {round}: output {i} isn't finite");
            }
            // Grains laid over each other are weighed, never added up: never louder than what came in.
            let most = if round % 3 == 0 { m2_dsp::fx::sane(audio).abs() } else { voice.iter().fold(0.0f32, |m, v| m.max(v.abs())) };
            assert!(outs[3].iter().all(|v| v.abs() <= most * 1.001 + 1e-6), "round {round}: louder than what came in ({most})");
        }
    }
}

// ---------------------------------------------------------------- a voice's notes (VOX-42, VOX-45)

mod sung {
    use m2_kernel::{Host, OFFLINE, native};

    use super::*;
    use crate::measure::HOP;

    /// `x` silent before `from` and after `to` seconds, with 5 ms fades.
    fn between(mut x: Vec<f32>, sr: f32, from: f64, to: f64) -> Vec<f32> {
        for (i, v) in x.iter_mut().enumerate() {
            let t = i as f64 / sr as f64;
            *v *= ((t - from).min(to - t) / 0.005).clamp(0.0, 1.0) as f32;
        }
        x
    }

    /// What `notes` measures of a mono file `x` at `sr`, played `split` frames at a time: each
    /// hop's `hz`, `clarity`, `level`, `note` and `centre`.
    fn measured_split(sr: f32, x: Vec<f32>, split: usize) -> Vec<[f32; 5]> {
        let hops = x.len().div_ceil(HOP);
        let assets = Vec::leak(vec![native::asset(sr, vec![x])]);
        let mut k = Host::<Notes>::of(Cfg { flags: OFFLINE, assets, ..Cfg::new(sr, BLOCK, 1) });
        let mut out = vec![];
        while out.len() < hops {
            let n = (hops - out.len()).min(split).min(BLOCK);
            k.process(n as u32);
            for i in 0..n {
                out.push([0, 1, 2, 3, 4].map(|o| k.port_mut(o)[i]));
            }
        }
        out
    }

    fn measured(sr: f32, x: Vec<f32>) -> Vec<[f32; 5]> {
        measured_split(sr, x, BLOCK)
    }

    /// The notes of a measure: each run of one note number, as (start s, end s, centre).
    fn runs(m: &[[f32; 5]], sr: f32) -> Vec<(f64, f64, f64)> {
        let secs = |k: usize| (k * HOP) as f64 / sr as f64;
        let mut out: Vec<(f64, f64, f64)> = vec![];
        let mut last = 0.0;
        for (k, h) in m.iter().enumerate() {
            if h[3] != 0.0 && h[3] == last {
                out.last_mut().unwrap().1 = secs(k + 1);
            } else if h[3] != 0.0 {
                out.push((secs(k), secs(k + 1), h[4] as f64));
            }
            last = h[3];
        }
        out
    }

    // VOX-42: a singer sings C4, moves to E4 at once, says "s", then sings G4: three notes, each
    // starting within 15 ms of where it was sung, each centre within 10 cents of the note, numbered
    // 1, 2, 3; nothing before or after.
    #[test]
    fn notes_finds_each_note_a_singer_sings() {
        for sr in RATES {
            let song = Song {
                secs: 2.2,
                midi: &|t| {
                    if t < 0.7 {
                        60.0
                    } else if t < 1.25 {
                        64.0
                    } else {
                        67.0
                    }
                },
                vibrato: 0.25,
                formants: AH,
                s: Some((1.2, 1.35)),
            };
            let x = between(song.sing(sr), sr, 0.2, 1.9);
            let m = measured(sr, x.clone());
            assert_eq!(m.len(), x.len().div_ceil(HOP), "{sr}: a value each hop");
            let found = runs(&m, sr);
            let want = [(0.2, 60.0), (0.7, 64.0), (1.35, 67.0)];
            assert_eq!(found.len(), 3, "{sr}: three notes, not {found:?}");
            for ((at, _, centre), (when, midi)) in found.iter().zip(want) {
                assert!((at - when).abs() < 0.015, "{sr}: a note at {at:.3}, sung at {when}");
                assert!((centre - midi).abs() < 0.1, "{sr}: its centre {centre:.3}, sung {midi}");
            }
            let numbers: Vec<f32> = m.iter().map(|h| h[3]).filter(|&n| n != 0.0).collect();
            assert!(numbers.windows(2).all(|w| w[1] == w[0] || w[1] == w[0] + 1.0) && numbers[0] == 1.0, "{sr}: numbered in order");
            assert!(found.last().unwrap().1 < 1.92, "{sr}: the last ends where the voice does");
        }
    }

    // VOX-42: `hz`, `clarity` and `level` are `pitch`'s reading of the window centred on each hop,
    // the same numbers; and the same bits however the host splits the calls.
    #[test]
    fn notes_reads_the_pitch_line_pitch_reads() {
        let sr = 48_000.0;
        let x = between(steady(57.0, 0.3, 1.0).sing(sr), sr, 0.1, 0.9);
        let m = measured(sr, x.clone());
        let mut p = <Pitch as m2_kernel::Kernel>::new(&Cfg::new(sr, BLOCK, 1));
        let (span, half) = (p.span() as i64, p.window() as i64 / 2);
        for k in [40usize, 80, 120] {
            let end = (k * HOP) as i64 + half;
            let w: Vec<f32> = (end - span..end).map(|f| if f >= 0 && (f as usize) < x.len() { x[f as usize] } else { 0.0 }).collect();
            let (hz, clarity, level) = p.read_window(&w);
            assert_eq!([m[k][0], m[k][1], m[k][2]], [hz, clarity, level], "hop {k}");
            assert!(cents(hz as f64, hz_of(57.0)).abs() < 60.0, "hop {k}: {hz} Hz");
        }
        for split in [1, 7, 100] {
            assert!(measured_split(sr, x.clone(), split) == m, "split by {split}");
        }
    }

    /// What `voice` renders of `src` (each channel as long) at `src_rate`, at `sr`: output frames
    /// `from..to` of a clip laid out by `map`, with `lead_in` seconds and `notes`, `split` frames a
    /// call: both channels.
    #[allow(clippy::too_many_arguments)]
    fn voiced_split(
        sr: f32,
        src: Vec<Vec<f32>>,
        src_rate: f32,
        map: &[[f64; 2]],
        (from, to): (usize, usize),
        notes: &[[f64; 5]],
        split: usize,
    ) -> Vec<[f32; 2]> {
        let channels = src.len();
        let mut values = vec![from as f64, to as f64, 0.1];
        values.extend(notes.iter().flatten());
        let (values, assets, time_map) = (Vec::leak(values), Vec::leak(vec![native::asset(src_rate, src)]), Vec::leak(map.to_vec()));
        let mut k = Host::<Voice>::of(Cfg { flags: OFFLINE, values, assets, time_map, ..Cfg::new(sr, BLOCK, channels) });
        let mut out = vec![];
        while out.len() < to - from {
            let n = (to - from - out.len()).min(split).min(BLOCK);
            k.process(n as u32);
            let p = k.port_mut(0);
            for i in 0..n {
                out.push([p[i], p[BLOCK + i]]);
            }
        }
        out
    }

    fn voiced(sr: f32, x: &[f32], map: &[[f64; 2]], span: (f64, f64), notes: &[[f64; 5]]) -> Vec<f32> {
        let f = |s: f64| (s * sr as f64).round() as usize;
        left(&voiced_split(sr, vec![x.to_vec()], sr, map, (f(span.0), f(span.1)), notes, BLOCK))
    }

    /// `x` read at fractional frame `pos` as a clip's normal path reads it (a cubic Hermite,
    /// `stream::read_in`), silence outside.
    fn cubic(x: &[f32], pos: f64) -> f32 {
        if pos < 0.0 || pos > x.len() as f64 - 1.0 {
            return 0.0;
        }
        let i = pos.floor() as isize;
        let t = (pos - i as f64) as f32;
        let at = |k: isize| if k < 0 || k as usize >= x.len() { 0.0 } else { x[k as usize] };
        let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }

    // VOX-45: with nothing to move (no note edited, or none there) and no warp, the voice is the
    // file within 1e-5, read from the clip's offset as the normal path reads it, at the song's rate
    // or another, and the same bits however the host splits its calls.
    #[test]
    fn with_nothing_to_move_the_voice_comes_back() {
        let sr = 48_000.0;
        let song = Song { secs: 1.6, midi: &|t| 55.0 + 4.0 * t, vibrato: 0.5, formants: AH, s: Some((0.8, 0.95)) };
        let x = song.sing(sr);
        let unedited = [[0.3, 0.4, 56.0, 0.0, 0.0], [0.75, 0.5, 58.0, 0.0, 0.0]];
        for (offset, notes) in [(0.0, &unedited[..]), (0.123_456_7, &[][..])] {
            let out = voiced(sr, &x, &[[offset, 0.0]], (0.2, 1.3), notes);
            for (i, v) in out.iter().enumerate() {
                let want = cubic(&x, offset * sr as f64 + (i + (0.2 * sr) as usize) as f64);
                assert!((v - want).abs() < 1e-5, "offset {offset}: frame {i}: {v} for {want}");
            }
        }
        let f = |s: f64| (s * 44_100.0f64).round() as usize;
        let other = left(&voiced_split(44_100.0, vec![x.clone()], sr, &[[0.0, 0.0]], (f(0.2), f(1.3)), &[], BLOCK));
        for (i, v) in other.iter().enumerate() {
            let want = cubic(&x, (f(0.2) + i) as f64 * sr as f64 / 44_100.0);
            assert!((v - want).abs() < 1e-5, "at 44.1 kHz: frame {i}: {v} for {want}");
        }
        let notes = [[0.3, 0.6, 56.3, 1.5, 0.5]];
        let whole = voiced_split(sr, vec![x.clone(), x.clone()], sr, &[[0.0, 0.0]], (9_600, 62_400), &notes, BLOCK);
        for split in [1, 77] {
            assert!(
                voiced_split(sr, vec![x.clone(), x.clone()], sr, &[[0.0, 0.0]], (9_600, 62_400), &notes, split) == whole,
                "split by {split}"
            );
        }
    }

    // VOX-45 (the note's §8): a note sung 31 cents sharp with vibrato, straightened, plays at 64.31,
    // one straight pitch; moved by -0.31 too, it plays at 64. Measured by an autocorrelation, not by
    // the pitch line the kernel reads.
    #[test]
    fn a_note_sung_31_cents_sharp_straightened_plays_at_64_31() {
        for sr in RATES {
            let x = steady(64.31, 0.4, 1.6).sing(sr);
            let heard_in = |y: &[f32]| -> (f64, f64) {
                let m: Vec<f64> = heard(y, sr, 64.3, 0.45, 1.05).into_iter().map(|(_, m)| m).collect();
                let mean = m.iter().sum::<f64>() / m.len() as f64;
                (mean, m.iter().cloned().fold(f64::MIN, f64::max) - m.iter().cloned().fold(f64::MAX, f64::min))
            };
            let (_, sung) = heard_in(&x);
            assert!(sung > 0.7, "{sr}: the singer wobbles by 0.8 semitones: {sung:.2}");
            for (moved, want) in [(0.0, 64.31), (-0.31, 64.0)] {
                let out = voiced(sr, &x, &[[0.0, 0.0]], (0.0, 1.5), &[[0.2, 1.1, 64.31, moved, 1.0]]);
                let (mean, wobble) = heard_in(&out);
                assert!((mean - want).abs() < 0.05, "{sr}: moved {moved}: plays at {mean:.3}, not {want}");
                assert!(wobble < 0.15, "{sr}: moved {moved}: straight, but wobbles by {wobble:.3}");
            }
        }
    }

    // VOX-45: laid out by its time map. A note stretched to twice its length keeps its pitch (within
    // 10 cents), and an "s" lands where the map puts it.
    #[test]
    fn a_voice_follows_its_time_map() {
        let sr = 48_000.0;
        let song = Song { secs: 2.0, midi: &|_| 60.0, vibrato: 0.0, formants: AH, s: Some((1.0, 1.1)) };
        let x = song.sing(sr);
        let map = [[0.0, 0.0], [0.5, 0.5], [1.0, 1.5], [2.0, 2.5]];
        let out = voiced(sr, &x, &map, (0.0, 2.4), &[[0.1, 0.85, 60.0, 0.5, 0.0]]);
        let m: Vec<f64> = heard(&out, sr, 60.5, 0.7, 1.3).into_iter().map(|(_, m)| m).collect();
        let mean = m.iter().sum::<f64>() / m.len() as f64;
        assert!((mean - 60.5).abs() < 0.1, "stretched twice as long, moved half a semitone: {mean:.3}");
        let loud = |a: f64, b: f64| part(&out, sr, a, b - a).iter().map(|v| v * v).sum::<f32>() / ((b - a) * sr as f64) as f32;
        let hiss = |y: &[f32]| y.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>() / y.len() as f32;
        assert!(hiss(part(&out, sr, 1.52, 0.06)) > 4.0 * hiss(part(&out, sr, 1.3, 0.06)), "the s lands at 1.5 s");
        let (stretched, sung) = (loud(0.7, 1.3), loud(0.2, 0.45));
        assert!(stretched > 0.5 * sung, "no hole in the stretched note: {stretched} for {sung}");
    }

    // RENDER-28, ABI-5: once born, neither allocates while it runs; and no input breaks them.
    #[test]
    fn notes_and_voice_allocate_nothing_and_nothing_breaks_them() {
        for sr in RATES {
            let x = steady(62.0, 0.3, 0.8).sing(sr);
            let assets = Vec::leak(vec![native::asset(sr, vec![x.clone(), x.clone()])]);
            let mut m = Host::<Notes>::of(Cfg { flags: OFFLINE, assets, ..Cfg::new(sr, BLOCK, 1) });
            let values = Vec::leak(vec![0.0, (0.7 * sr) as f64, 0.1, 0.1, 0.5, 62.2, 2.0, 0.5]);
            let time_map = Vec::leak(vec![[0.0, 0.0], [0.8, 0.7]]);
            let mut v = Host::<Voice>::of(Cfg { flags: OFFLINE, values, assets, time_map, ..Cfg::new(sr, BLOCK, 2) });
            let before = counting::here();
            for _ in 0..(x.len() / 256).div_ceil(BLOCK) {
                m.process(BLOCK as u32);
            }
            for _ in 0..((0.7 * sr) as usize).div_ceil(BLOCK) {
                v.process(BLOCK as u32);
                assert!(v.port_mut(0).iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            }
            assert_eq!(counting::here(), before, "{sr}: allocated while it ran");
        }
        let mut rng = Lcg(11);
        for _ in 0..40 {
            let pick = |rng: &mut Lcg| EXTREMES[rng.next() as usize % EXTREMES.len()] as f64;
            let x = vec![pick(&mut rng) as f32; 4_000];
            let values = Vec::leak(vec![
                pick(&mut rng),
                pick(&mut rng),
                pick(&mut rng),
                pick(&mut rng),
                pick(&mut rng),
                pick(&mut rng),
                pick(&mut rng),
                pick(&mut rng),
            ]);
            let time_map = Vec::leak(vec![[pick(&mut rng), pick(&mut rng)], [pick(&mut rng), pick(&mut rng)]]);
            let assets = Vec::leak(vec![native::asset(48_000.0, vec![x])]);
            let mut v = Host::<Voice>::of(Cfg { flags: OFFLINE, values, assets, time_map, ..Cfg::new(48_000.0, BLOCK, 1) });
            let mut m = Host::<Notes>::of(Cfg { flags: OFFLINE, assets, ..Cfg::new(48_000.0, BLOCK, 1) });
            for _ in 0..8 {
                v.process(BLOCK as u32);
                m.process(BLOCK as u32);
                assert!(v.port_mut(0).iter().all(|s| s.is_finite()), "{values:?} {time_map:?}");
                assert!((0..5).all(|o| m.port_mut(o).iter().all(|s| s.is_finite())));
            }
        }
    }

    // The manifest says what the code does: `notes`' hop, and both are offline.
    #[test]
    fn the_manifest_says_the_hop() {
        assert!(MANIFEST.contains(&format!("hop = {HOP}")), "notes' hop is {HOP}");
    }
}
