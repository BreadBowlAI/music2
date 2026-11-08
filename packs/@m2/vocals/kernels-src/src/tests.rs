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

/// C minor: C, D, E♭, F, G, A♭ and B♭.
const C_MINOR: f32 = (1 | 1 << 2 | 1 << 3 | 1 << 5 | 1 << 7 | 1 << 8 | 1 << 10) as f32;

/// What snap's two meters say at the end of each block, pitch then snap playing `voice` in `key`
/// with `glide` and `amount` (VOX-22): how far the voice is from the note it aims at, and how far
/// its move leaves it. Each is one number for the block.
fn pitch_line(sr: f32, voice: &[f32], key: f32, glide: f32, amount: f32) -> (Vec<f32>, Vec<f32>) {
    let (mut pitch, mut snap) = (Host::<Pitch>::of(cfg(sr, 1)), Host::<Snap>::of(cfg(sr, 1)));
    let (mut off, mut left) = (vec![], vec![]);
    for chunk in voice.chunks(BLOCK) {
        let n = chunk.len();
        pitch.port_mut(0)[..n].copy_from_slice(chunk);
        pitch.process(n as u32);
        let (hz, clarity) = (pitch.port_mut(1)[..n].to_vec(), pitch.port_mut(2)[..n].to_vec());
        snap.port_mut(0)[..n].copy_from_slice(&hz);
        snap.port_mut(1)[..n].copy_from_slice(&clarity);
        for (port, v) in [(2, key), (3, glide), (4, amount)] {
            snap.port_mut(port)[..n].fill(v);
        }
        snap.process(n as u32);
        for (o, said) in [(6, &mut off), (7, &mut left)] {
            let m = &snap.port_mut(o)[..n];
            assert!(m.iter().all(|x| x.to_bits() == m[0].to_bits()), "a meter holds one number for the block: {:?}", &m[..2]);
            said.push(m[0]);
        }
    }
    (off, left)
}

// VOX-22, ABI-13: snap says how far the voice is from the note of the key it aims at (`off`, + sharp)
// and how far its move leaves it (`left`), in semitones, for Tune's pitch line (VOX-27). A voice
// sung 30 cents over A, in C major: off within 3 cents of +0.30 (pitch's own reading, VOX-21), and
// left 0 once eased at Amount 1, a fifth of off at Amount 0.8; in C minor it aims at B♭, nearer
// than A♭: off −0.70. Silence and an "s" have no pitch: both 0. And when the voice stops, both are 0
// at once, while the move still eases back.
#[test]
fn snap_says_how_far_the_voice_is_from_its_note() {
    let sr = 48_000.0;
    let sung = steady(69.3, 0.0, 1.0).sing(sr);
    let settled = |v: &[f32]| v[v.len() / 2..].to_vec();
    let near = |v: &[f32], to: f64, by: f64| v.iter().all(|&x| (x as f64 - to).abs() < by);
    let (off, left) = pitch_line(sr, &sung, C_MAJOR, 0.1, 1.0);
    let (off, left) = (settled(&off), settled(&left));
    assert!(near(&off, 0.3, 0.03), "30 cents sharp of A, in C major: {:?}", &off[..4]);
    assert!(near(&left, 0.0, 0.005), "Amount 1, once eased: on the note: {:?}", &left[..4]);
    let (off, left) = pitch_line(sr, &sung, C_MAJOR, 0.1, 0.8);
    let (off, left) = (settled(&off), settled(&left));
    assert!(near(&left, 0.06, 0.01), "Amount 0.8, once eased: 6 cents sharp: {:?}", &left[..4]);
    let fifth = off.iter().zip(&left).map(|(&o, &l)| (l - 0.2 * o).abs()).fold(0.0f32, f32::max);
    assert!(fifth < 0.005, "Amount 0.8 leaves a fifth of how far it is: {fifth}");
    let (off, _) = pitch_line(sr, &sung, C_MINOR, 0.1, 1.0);
    assert!(near(&settled(&off), -0.7, 0.03), "70 cents flat of B♭, in C minor: {:?}", &settled(&off)[..4]);
    let silence = vec![0.0; 24_000];
    let s = Song { secs: 0.5, midi: &|_| 60.0, vibrato: 0.0, formants: AH, s: Some((-1.0, 2.0)) }.sing(sr);
    for (what, x) in [("silence", &silence), ("an s", &s)] {
        let (off, left) = pitch_line(sr, x, C_MAJOR, 0.1, 1.0);
        assert!(off.iter().chain(&left).all(|&v| v == 0.0), "{what}: no pitch, 0 and 0");
    }
    let stopped: Vec<f32> = sung[..24_000].iter().copied().chain(silence).collect();
    let (off, left) = pitch_line(sr, &stopped, C_MAJOR, 0.4, 1.0);
    let after = 36_000 / BLOCK;
    assert!(off[after..].iter().chain(&left[after..]).all(|&v| v == 0.0), "the voice stopped: 0 and 0 at once");
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

    /// A singer as pop sings (the review's S2, S3, S11): a breath, C4 scooped up from B3, a glide to
    /// E4 over 120 ms, an "s", G4 held with a vibrato that grows to `vib` semitones (and a tremolo
    /// with it), a legato step to F4, a breath, and D4 with a soft 40 ms attack.
    fn pop_singer(sr: f64, vib: f64) -> Vec<f32> {
        let n = (5.0 * sr) as usize;
        let width = 0.0004;
        let pulse = |tau: f64| if (0.0..width).contains(&tau) { 0.5 - 0.5 * (std::f64::consts::TAU * tau / width).cos() } else { 0.0 };
        let mut filters = AH.map(|(f, bw)| Formant::new(sr, f, bw));
        let (mut phase, mut pulses, mut last) = (0.0f64, [f64::NEG_INFINITY; 2], 0.0f64);
        let mut noise = Lcg(5);
        let mut nl = 0.0;
        let ramp = |t: f64, a: f64, b: f64, d: f64| ((t - a).min(b - t) / d).clamp(0.0, 1.0);
        let swell = |t: f64| ((t - 2.1) / 0.4).clamp(0.0, 1.0) * (std::f64::consts::TAU * 5.5 * t).sin();
        let midi = |t: f64| -> f64 {
            if t < 0.61 {
                59.0 + (t - 0.55).max(0.0) / 0.06
            } else if t < 1.0 {
                60.0
            } else if t < 1.12 {
                60.0 + 4.0 * (t - 1.0) / 0.12
            } else if t < 1.7 {
                64.0
            } else if t < 3.5 {
                67.0 + vib * swell(t)
            } else if t < 4.2 {
                65.0
            } else {
                62.0
            }
        };
        let voiced = |t: f64| ramp(t, 0.55, 1.6, 0.02) + ramp(t, 1.75, 4.0, 0.03) + ramp(t, 4.45, 4.8, 0.04);
        let breath = |t: f64| ramp(t, 0.3, 0.5, 0.05) + ramp(t, 4.05, 4.35, 0.05);
        let ess = |t: f64| ramp(t, 1.6, 1.75, 0.005);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / sr;
            let f = hz_of(midi(t));
            phase += f / sr;
            if phase >= 1.0 {
                phase -= 1.0;
                pulses = [pulses[1], t - phase / f];
            }
            let g = pulses.iter().map(|&p| pulse(t - p)).sum::<f64>();
            let mut v = g - last;
            last = g;
            for fl in &mut filters {
                v = fl.tick(v);
            }
            let trem = 1.0 + 0.17 * vib * swell(t);
            let w = noise.next() as f64 / 32_768.0 - 1.0;
            let h = w - nl;
            nl = w;
            out.push((voiced(t) * trem * v * 6.0 + ess(t) * h * 0.15 + breath(t) * w * 0.03) as f32);
        }
        out
    }

    // VOX-42 (the review's S2, S3, S11): the pop singer's five notes at every rate, whatever the
    // held note's vibrato up to a semitone each way: breaths and the "s" are no note, the held G4
    // with its wide vibrato stays one, the glide is cut at the same moment at every rate (within a
    // hop, 6 ms at 44.1 kHz), each start sung after silence or a breath is found within 8 ms (a
    // legato step within 10 ms; after the "s", within 11 ms: two hops), and each centre within 15
    // cents.
    #[test]
    fn notes_hears_a_pop_singer_at_every_rate() {
        let mut glide_cuts = vec![];
        for sr in RATES {
            for vib in [0.3, 0.5, 0.7, 1.0] {
                let found = runs(&measured(sr, pop_singer(sr as f64, vib)), sr);
                let said: Vec<String> = found.iter().map(|(a, b, c)| format!("{a:.4}-{b:.4}@{c:.2}")).collect();
                assert_eq!(found.len(), 5, "{sr} Hz, vibrato ±{vib}: five notes, not {}", said.join("  "));
                // After the "s", a reading's window holds the hiss up to 11 ms into the note; across a
                // legato step it holds both notes, up to 10 ms.
                let sung = [(0.55, 60.0, 0.008), (f64::NAN, 64.0, 0.0), (1.75, 67.0, 0.011), (3.5, 65.0, 0.010), (4.45, 62.0, 0.008)];
                for ((at, _, centre), (when, midi, within)) in found.iter().zip(sung) {
                    assert!((centre - midi).abs() < 0.15, "{sr} Hz, ±{vib}: a centre {centre:.3} for {midi}: {}", said.join("  "));
                    if when.is_finite() {
                        assert!((at - when).abs() < within, "{sr} Hz, ±{vib}: a start at {at:.4}, sung at {when}: {}", said.join("  "));
                    }
                }
                assert!((1.0..1.12).contains(&found[1].0), "{sr} Hz: the glide is cut inside it: {}", found[1].0);
                glide_cuts.push(found[1].0);
            }
        }
        let (lo, hi) = glide_cuts.iter().fold((f64::MAX, f64::MIN), |(a, b), &c| (a.min(c), b.max(c)));
        assert!(hi - lo < 0.006, "the glide is cut at one moment at every rate, within a hop: from {lo:.4} to {hi:.4} s");
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

    // VOX-45, RENDER-30b (the review's S1): a note edited in a line sung legato, its neighbours
    // touching it: past the note, where nothing moves, the grains line up with the file again, so
    // each end of the phrase, where it is crossfaded into the clip's normal path, is the file within
    // a thousandth of its peak, at every rate and for a move up, a small one and one down.
    #[test]
    fn a_phrase_sung_legato_ends_on_the_file() {
        for sr in RATES {
            let song = Song {
                secs: 2.6,
                midi: &|t| {
                    if t < 1.0 {
                        52.0
                    } else if t < 1.5 {
                        55.0
                    } else {
                        57.0
                    }
                },
                vibrato: 0.3,
                formants: AH,
                s: None,
            };
            let x = between(song.sing(sr), sr, 0.3, 2.4);
            for by in [2.0, 0.5, -3.0] {
                let notes = [[0.3, 0.7, 52.0, 0.0, 0.0], [1.0, 0.5, 55.0, by, 0.0], [1.5, 0.9, 57.0, 0.0, 0.0]];
                let out = voiced(sr, &x, &[[0.0, 0.0]], (0.95, 1.55), &notes);
                let f0 = (0.95 * sr as f64).round() as usize;
                let worst = |a: f64, b: f64| {
                    let (a, b) = ((a * sr as f64) as usize - f0, (b * sr as f64) as usize - f0);
                    (a..b.min(out.len())).map(|i| (out[i] - cubic(&x, (f0 + i) as f64)).abs()).fold(0.0f32, f32::max)
                };
                let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                let (start, end) = (worst(0.95, 0.96) / peak, worst(1.54, 1.55) / peak);
                assert!(start < 1e-3 && end < 1e-3, "{sr} Hz, moved {by}: its ends are the file: start {start}, end {end} of its peak");
                assert!(worst(1.1, 1.4) > 0.05 * peak, "{sr} Hz, moved {by}: and the note is moved");
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

// ---------------------------------------------------------------- the Vocoder (VOX-29)

mod vocoded {
    use m2_dsp::testing::effects::{self, Fx};
    use m2_dsp::testing::{defaults_in, manifest_in, spectrum};
    use m2_kernel::{Host, Kernel, PORT_CHANNELS};

    use super::*;
    use crate::bands::{BANDS, centre};

    const CLARITY: usize = 2;
    const LISTEN: usize = 5;

    /// A band-limited saw at `f0`: its harmonics up to `top` Hz, each `1/n`, peaking at 0.5.
    fn saw(sr: f32, f0: f64, top: f64, secs: f64) -> Vec<f32> {
        let n = (secs * sr as f64) as usize;
        let mut out = vec![0f64; n];
        let mut h = 1;
        while h as f64 * f0 <= top.min(0.49 * sr as f64) {
            let w = std::f64::consts::TAU * h as f64 * f0 / sr as f64;
            for (i, o) in out.iter_mut().enumerate() {
                *o += (w * i as f64).sin() / h as f64;
            }
            h += 1;
        }
        let most = out.iter().fold(0f64, |m, v| m.max(v.abs()));
        out.iter().map(|v| (0.5 * v / most) as f32).collect()
    }

    /// A voice-like sound: a pulse at `f0` (every harmonic alike, up to 0.45 of the rate) through
    /// two formant band-passes side by side, peaking at `level`.
    fn vowel(sr: f32, f0: f64, formants: [f64; 2], secs: f64, level: f64) -> Vec<f32> {
        let n = (secs * sr as f64) as usize;
        let mut pulse = vec![0f64; n];
        let mut h = 1;
        while h as f64 * f0 <= 0.45 * sr as f64 {
            let w = std::f64::consts::TAU * h as f64 * f0 / sr as f64;
            for (i, o) in pulse.iter_mut().enumerate() {
                *o += (w * i as f64).cos();
            }
            h += 1;
        }
        let mut f = formants.map(|f| Formant::new(sr as f64, f, 0.1 * f + 50.0));
        let out: Vec<f64> = pulse.iter().map(|&p| f[0].tick(p) + f[1].tick(p)).collect();
        let most = out[n / 2..].iter().fold(0f64, |m, v| m.max(v.abs()));
        out.iter().map(|v| (v * level / most) as f32).collect()
    }

    /// White noise over `hz` (a 4th-order high-pass), peaking near `level`: an S.
    fn hiss(sr: f32, hz: f32, secs: f64, level: f32) -> Vec<f32> {
        let mut rng = Lcg(3);
        let f = [m2_dsp::fx::Filter::highpass(sr, hz, 0.5412), m2_dsp::fx::Filter::highpass(sr, hz, 1.3066)];
        let mut s = [[0f32; 2]; 2];
        let raw: Vec<f32> = (0..(secs * sr as f64) as usize)
            .map(|_| {
                let a = f[0].tick(&mut s[0], rng.next() as f32 / 32_768.0 - 1.0);
                f[1].tick(&mut s[1], a)
            })
            .collect();
        let most = raw.iter().fold(0f32, |m, v| m.max(v.abs()));
        raw.iter().map(|v| v * level / most).collect()
    }

    /// The Vocoder, mono, on `carrier` with `voice`, its controls held at `[clarity, smooth,
    /// formant]`, listening, `split` frames a call.
    fn played(sr: f32, carrier: &[f32], voice: &[f32], controls: [f32; 3], split: usize) -> Vec<f32> {
        let mut h = Host::<Vocoder>::of(cfg(sr, 1));
        let mut out = Vec::with_capacity(carrier.len());
        let mut at = 0;
        while at < carrier.len() {
            let n = split.min(carrier.len() - at);
            h.port_mut(0)[..n].copy_from_slice(&carrier[at..at + n]);
            h.port_mut(1)[..n].copy_from_slice(&voice[at..at + n]);
            for (p, v) in controls.iter().enumerate() {
                h.port_mut(CLARITY + p)[..n].fill(*v);
            }
            h.port_mut(LISTEN)[..n].fill(1.0);
            h.process(n as u32);
            out.extend_from_slice(&h.port_mut(Vocoder::INPUTS)[..n]);
            at += n;
        }
        out
    }

    fn play(sr: f32, carrier: &[f32], voice: &[f32], controls: [f32; 3]) -> Vec<f32> {
        played(sr, carrier, voice, controls, BLOCK)
    }

    /// From half a second in: the followers have long settled.
    fn settled(x: &[f32], sr: f32) -> &[f32] {
        &x[(0.5 * sr) as usize..]
    }

    /// The power in dB, by an FFT (m2-dsp's, Kaiser's window), of each of the Vocoder's bands:
    /// from a sixth of an octave under its centre to a sixth over.
    fn band_levels(x: &[f32], sr: f32) -> [f64; BANDS] {
        let spec = spectrum(settled(x, sr), sr);
        std::array::from_fn(|k| {
            let (lo, hi) = (centre(k) as f64 * 2f64.powf(-1.0 / 6.0), centre(k) as f64 * 2f64.powf(1.0 / 6.0));
            10.0 * spec.iter().filter(|(f, _)| *f >= lo && *f < hi).map(|(_, p)| p).sum::<f64>().max(1e-30).log10()
        })
    }

    /// The power in dB from `lo` Hz to `hi`, by the same FFT.
    fn level_between(x: &[f32], sr: f32, lo: f64, hi: f64) -> f64 {
        let spec = spectrum(settled(x, sr), sr);
        10.0 * spec.iter().filter(|(f, _)| *f >= lo && *f < hi).map(|(_, p)| p).sum::<f64>().max(1e-30).log10()
    }

    /// Each band's level against the loudest band's, in dB.
    fn shape(levels: &[f64]) -> Vec<f64> {
        let most = levels.iter().cloned().fold(f64::MIN, f64::max);
        levels.iter().map(|l| l - most).collect()
    }

    /// Pearson's correlation of `a` and `b`.
    fn correlation(a: &[f64], b: &[f64]) -> f64 {
        let n = a.len() as f64;
        let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
        let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
        for (x, y) in a.iter().zip(b) {
            ab += (x - ma) * (y - mb);
            aa += (x - ma) * (x - ma);
            bb += (y - mb) * (y - mb);
        }
        ab / (aa * bb).sqrt()
    }

    /// The band from 315 Hz up: every band there holds a harmonic of a 105 Hz voice.
    const FROM: usize = 5;

    /// What the Vocoder did to the carrier in each band (the out's level less the carrier's, in
    /// dB, against its loudest band), and the voice's band levels against its loudest.
    fn gains_and_voice(sr: f32, formants: [f64; 2]) -> (Vec<f64>, Vec<f64>) {
        let secs = 1.5;
        let c = saw(sr, 55.0, 20_000.0, secs);
        let v = vowel(sr, 105.0, formants, secs, 0.5);
        let o = play(sr, &c, &v, [0.0, 0.05, 0.0]);
        let (cb, ob) = (band_levels(&c, sr), band_levels(&o, sr));
        let g: Vec<f64> = (0..BANDS).map(|k| ob[k] - cb[k]).collect();
        (shape(&g), shape(&band_levels(&v, sr)))
    }

    /// Of the bands within two of `band`, the one the out plays loudest.
    fn hill(g: &[f64], band: usize) -> usize {
        (band - 2..=band + 2).max_by(|&a, &b| g[a].total_cmp(&g[b])).unwrap()
    }

    // VOX-29: the voice's shape is the out's. A buzzy carrier (a band-limited saw at 55 Hz) under a
    // voice-like sound (a pulse at 105 Hz through two formants, at 525 Hz and 2.1 kHz, each on a
    // harmonic, in the bands 7 and 13) gives an out whose band levels over the carrier's follow the
    // voice's, at 44.1, 48 and 96 kHz: its two hills in the formants' bands, the dip between them at
    // least 8 dB deep (21 dB in the voice), the two rising and falling together (a correlation over
    // 0.9 across the bands from 315 Hz up), and each band within 9 dB of the voice's there, where
    // the voice is within 20 dB of its loudest. The tolerance: each band of each bank hears its
    // neighbours' centres 8.5 dB down, so where the voice falls faster than that from one band to
    // the next, the out's band reads its neighbour's leak, up to about 8.5 dB over the voice's.
    // Measured, 7.5 dB at worst, a correlation of 0.95 and a dip of 10.3 dB.
    #[test]
    fn the_out_takes_the_shape_of_the_voice() {
        for sr in RATES {
            let (g, v) = gains_and_voice(sr, [525.0, 2100.0]);
            assert_eq!((hill(&g, 7), hill(&g, 13)), (7, 13), "{sr}: the out's hills are the voice's formants' bands: {g:.1?}");
            let dip = (8..13).map(|k| g[k]).fold(f64::MAX, f64::min);
            assert!(g[7].min(g[13]) - dip >= 8.0, "{sr}: the dip between the hills is {:.1} dB deep: {g:.1?}", g[7].min(g[13]) - dip);
            let r = correlation(&g[FROM..], &v[FROM..]);
            assert!(r > 0.9, "{sr}: the out's band levels follow the voice's by {r:.3}");
            for k in (FROM..BANDS).filter(|&k| v[k] > -20.0) {
                assert!(
                    (g[k] - v[k]).abs() < 9.0,
                    "{sr}: band {k} ({} Hz) plays {:.1} dB where the voice is {:.1} dB",
                    centre(k),
                    g[k],
                    v[k]
                );
            }
        }
    }

    // VOX-29: moving the voice's formants moves the out's. The voice's formants moved from 525 Hz
    // and 2.1 kHz (an "ah") to 315 Hz and 3.15 kHz (an "ee"), each two bands, the out's hills move
    // with them, and each out follows its own voice better than the other.
    #[test]
    fn moving_the_voices_formants_moves_the_outs() {
        let sr = 48_000.0;
        let (ga, va) = gains_and_voice(sr, [525.0, 2100.0]);
        let (ge, ve) = gains_and_voice(sr, [315.0, 3150.0]);
        assert_eq!((hill(&ga, 7), hill(&ga, 13)), (7, 13), "ah: {ga:.1?}");
        assert_eq!((hill(&ge, 5), hill(&ge, 15)), (5, 15), "ee: {ge:.1?}");
        let dip = (6..15).map(|k| ge[k]).fold(f64::MAX, f64::min);
        assert!(ge[5].min(ge[15]) - dip >= 8.0, "ee's dip: {ge:.1?}");
        let r = |g: &[f64], v: &[f64]| correlation(&g[FROM..], &v[FROM..]);
        assert!(
            r(&ga, &va) > r(&ga, &ve) + 0.2 && r(&ge, &ve) > r(&ge, &va) + 0.2,
            "each out follows its own voice: {:.3} {:.3} {:.3} {:.3}",
            r(&ga, &va),
            r(&ga, &ve),
            r(&ge, &ve),
            r(&ge, &va)
        );
    }

    /// The out's gain over the carrier, in dB, from 250 Hz to 4 kHz on a grid of 1/48 octave: at
    /// each harmonic of the carrier, a 27.5 Hz saw (the FFT's power within 3 bins), drawn straight
    /// between them on a log scale of frequency.
    fn gain_curve(out: &[f32], carrier: &[f32], sr: f32) -> Vec<f64> {
        gain_curve_of(settled(out, sr), settled(carrier, sr), sr)
    }

    /// [`gain_curve`] of two windows of the same frames, `out` against `carrier`, as they are.
    fn gain_curve_of(out: &[f32], carrier: &[f32], sr: f32) -> Vec<f64> {
        let (so, sc) = (spectrum(out, sr), spectrum(carrier, sr));
        let bin = so[1].0;
        let points: Vec<(f64, f64)> = (1..)
            .map(|h| h as f64 * 27.5)
            .take_while(|f| *f < 4_400.0)
            .filter(|f| *f > 225.0)
            .map(|f| {
                let k = (f / bin).round() as usize;
                let power = |s: &[(f64, f64)]| s[k - 3..=k + 3].iter().map(|x| x.1).sum::<f64>();
                (f.log2(), 10.0 * (power(&so) / power(&sc)).log10())
            })
            .collect();
        (0..=(16.0f64.log2() * 48.0) as usize)
            .map(|i| {
                let x = 250f64.log2() + i as f64 / 48.0;
                let j = points.iter().position(|p| p.0 >= x).unwrap().max(1);
                let (a, b) = (points[j - 1], points[j]);
                a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
            })
            .collect()
    }

    /// How many semitones up `b` lies from `a`: the shift that lays it on `a` best (least squares),
    /// to a 48th of an octave, then a parabola through the three best.
    fn moved(a: &[f64], b: &[f64]) -> f64 {
        let fit = |l: i64| -> f64 {
            let pairs = (0..a.len() as i64).filter(|i| (0..b.len() as i64).contains(&(i + l)));
            let (sum, n) = pairs.fold((0.0, 0.0), |(s, n), i| (s + (a[i as usize] - b[(i + l) as usize]).powi(2), n + 1.0));
            -sum / n
        };
        let best = (-60..=60).max_by(|&x, &y| fit(x).total_cmp(&fit(y))).unwrap();
        let (y0, y1, y2) = (fit(best - 1), fit(best), fit(best + 1));
        (best as f64 + 0.5 * (y0 - y2) / (y0 - 2.0 * y1 + y2)) / 4.0
    }

    /// The carrier's bank, every band open, summed by `sign`: its gain in dB from 125 Hz to 6.4 kHz
    /// at 400 points a log step apart, by a plain DFT of its answer to a click (half a second), at
    /// least and at most.
    fn bank(sr: f32, sign: &dyn Fn(usize) -> f32) -> (f64, f64) {
        let bands: Vec<[m2_dsp::fx::Filter; 2]> = (0..BANDS).map(|k| [crate::bands::section(sr, centre(k)); 2]).collect();
        let mut states = vec![[[0f32; 2]; 2]; BANDS];
        let answer: Vec<f32> = (0..(0.5 * sr) as usize)
            .map(|i| {
                let x = if i == 0 { 1.0 } else { 0.0 };
                (0..BANDS)
                    .map(|k| {
                        let a = bands[k][0].tick(&mut states[k][0], x);
                        sign(k) * bands[k][1].tick(&mut states[k][1], a)
                    })
                    .sum()
            })
            .collect();
        (0..400).fold((f64::MAX, f64::MIN), |(lo, hi), j| {
            let f = 125.0 * (6400.0f64 / 125.0).powf(j as f64 / 399.0);
            let w = std::f64::consts::TAU * f / sr as f64;
            let (re, im) = answer
                .iter()
                .enumerate()
                .fold((0.0, 0.0), |(re, im), (i, &v)| (re + v as f64 * (w * i as f64).cos(), im - v as f64 * (w * i as f64).sin()));
            let db = 10.0 * (re * re + im * im).log10();
            (lo.min(db), hi.max(db))
        })
    }

    // VOX-29: the carrier's bands, all open, give it back within 1.6 dB, every other band's sign
    // turned over as the kernel sums them (`bands::sign`), at 44.1, 48 and 96 kHz; summed as they
    // are, two bands' phases cancel where they meet (130° apart), in notches over 20 dB deep.
    #[test]
    fn the_carriers_bands_all_open_give_it_back_within_1_6_db() {
        for sr in RATES {
            let (lo, hi) = bank(sr, &crate::bands::sign);
            assert!(lo > -0.5 && hi < 1.6, "{sr}: all open, from {lo:.2} to {hi:.2} dB");
            let (lo, _) = bank(sr, &|_| 1.0);
            assert!(lo < -20.0, "{sr}: as they are, notches only {lo:.2} dB deep");
        }
    }

    // VOX-29: Formant moves the out's hills by the semitones asked, both ways, at every rate: the
    // out's gain curve over a 27.5 Hz saw, from 250 Hz to 4 kHz, at Formant −12, −7, 5 and 12 lies
    // that many semitones from the one at 0, within a tenth of a semitone (measured within 0.04).
    #[test]
    fn formant_moves_the_outs_hills_by_its_semitones() {
        for sr in RATES {
            let secs = 1.5;
            let c = saw(sr, 27.5, 20_000.0, secs);
            let v = vowel(sr, 105.0, [525.0, 2100.0], secs, 0.5);
            let at = |formant: f32| gain_curve(&play(sr, &c, &v, [0.0, 0.05, formant]), &c, sr);
            let still = at(0.0);
            for formant in [-12.0, -7.0, 5.0, 12.0] {
                let by = moved(&still, &at(formant));
                assert!((by - formant as f64).abs() < 0.1, "{sr}: Formant {formant} moves the hills {by:.3} semitones");
            }
        }
    }

    // VOX-29: Formant moved while the song plays moves the hills (review W3): stepped from 0 to 7
    // half way through, under a 27.5 Hz saw, the out's gain curve over the 0.7 s from 40 ms after
    // the step (Formant glides there in 20 ms, a time constant) lies 7 semitones up from the one
    // before it, within 0.1, at every rate. The carrier's bands are placed again as Formant moves,
    // every 16 frames; placed once only (review W3's M2), the hills don't move.
    #[test]
    fn formant_moved_while_playing_moves_the_hills() {
        for sr in RATES {
            let secs = 3.0;
            let c = saw(sr, 27.5, 20_000.0, secs);
            let v = vowel(sr, 105.0, [525.0, 2100.0], secs, 0.5);
            let step = (1.5 * sr) as usize;
            let o = run(sr, 1, c.len(), BLOCK, &|p, _, t| match p {
                0 => c[t],
                1 => v[t],
                3 => 0.05,
                4 if t >= step => 7.0,
                LISTEN => 1.0,
                _ => 0.0,
            });
            let out: Vec<f32> = o.iter().map(|f| f[0]).collect();
            // About 0.7 s at every rate, so its harmonics stand apart in the spectrum.
            let window = ((0.6 * sr) as usize).next_power_of_two();
            let curve = |from: usize| gain_curve_of(&out[from..from + window], &c[from..from + window], sr);
            let by = moved(&curve((0.5 * sr) as usize), &curve(step + (0.04 * sr) as usize));
            assert!((by - 7.0).abs() < 0.1, "{sr}: Formant stepped to 7 while playing moves the hills {by:.3} semitones");
        }
    }

    // VOX-29: a band's follower rises in 2 ms (review W3): a voice that starts at once at band 10's
    // centre, under a steady carrier there, brings that band's out within 1 dB of its steady level
    // in under 10 ms, the band's own filters' rise included. The voice is at −90 dB, far under the
    // knee, so the out follows the follower in step (at a normal level the knee brings it there
    // sooner). At every rate.
    #[test]
    fn a_band_opens_within_10_ms() {
        for sr in RATES {
            let f = centre(10) as f64;
            let start = (0.25 * sr) as usize;
            let n = start + (0.5 * sr) as usize;
            let sine = |a: f64, i: usize| (a * (std::f64::consts::TAU * f * i as f64 / sr as f64).sin()) as f32;
            let c: Vec<f32> = (0..n).map(|i| sine(0.5, i)).collect();
            let v: Vec<f32> = (0..n).map(|i| if i < start { 0.0 } else { sine(10f64.powf(-90.0 / 20.0), i) }).collect();
            let o = play(sr, &c, &v, [0.0, 0.05, 0.0]);
            let cycle = (sr as f64 / f).ceil() as usize;
            let height = |at: usize| o[at..at + cycle].iter().fold(0f32, |m, x| m.max(x.abs())) as f64;
            let steady = height(start + (0.3 * sr) as usize);
            let open = (start..n).find(|&t| height(t) >= steady * 10f64.powf(-1.0 / 20.0)).unwrap();
            let ms = (open - start) as f64 / sr as f64 * 1000.0;
            assert!(ms < 10.0, "{sr}: the band came within 1 dB of its steady level {ms:.1} ms after the voice started");
        }
    }

    // VOX-29: the bands hear the mono sum of the voice (review W3): in stereo, a vowel on the right
    // channel alone shapes the out as the same vowel on the left alone does, bit for bit, at
    // Clarity 0 (each channel's own highs left out), and shapes it; at every rate.
    #[test]
    fn a_voice_on_either_channel_shapes_the_out_alike() {
        for sr in RATES {
            let c = saw(sr, 55.0, 20_000.0, 1.0);
            let v = vowel(sr, 105.0, [525.0, 2100.0], 1.0, 0.5);
            let on = |side: usize| {
                run(sr, 2, c.len(), BLOCK, &|p, ch, t| match p {
                    0 => c[t],
                    1 if ch == side => v[t],
                    3 => 0.05,
                    LISTEN => 1.0,
                    _ => 0.0,
                })
            };
            let (left, right) = (on(0), on(1));
            assert!(left == right, "{sr}: a vowel on the right shapes the out otherwise than on the left");
            assert!(left.iter().skip((0.5 * sr) as usize).any(|f| f[0].abs() > 1e-3), "{sr}: the vowel opens no band");
        }
    }

    // VOX-29: Clarity is the voice's own highs. Under a dull synth (nothing over 4 kHz), a voice of
    // a vowel and a hiss over 6 kHz: with Clarity 0 the out over 6 kHz is at least 50 dB under the
    // hiss (measured 74 dB), so it doesn't come through; at 1 it is the hiss's level within half a
    // dB (measured 0.01 dB under), at 0.5 6 dB under it; at 44.1, 48 and 96 kHz.
    #[test]
    fn clarity_lets_the_voices_own_highs_through() {
        for sr in RATES {
            let secs = 1.5;
            let c = saw(sr, 55.0, 4_000.0, secs);
            let s = hiss(sr, 6_000.0, secs, 0.2);
            let v: Vec<f32> = vowel(sr, 105.0, [525.0, 2100.0], secs, 0.4).iter().zip(&s).map(|(a, b)| a + b).collect();
            let top = 0.49 * sr as f64;
            let hissed = level_between(&s, sr, 6_000.0, top);
            let through = |clarity: f32| level_between(&play(sr, &c, &v, [clarity, 0.05, 0.0]), sr, 6_000.0, top) - hissed;
            let (none, half, all) = (through(0.0), through(0.5), through(1.0));
            assert!(none < -50.0, "{sr}: at Clarity 0 the hiss comes through {none:.1} dB under");
            assert!(all.abs() < 0.5, "{sr}: at Clarity 1 it comes through at {all:.2} dB");
            assert!((half + 6.02).abs() < 0.5, "{sr}: at Clarity 0.5, {half:.2} dB");
        }
    }

    // VOX-29: a silent voice gives silence out, every band shut: under a saw, at any Clarity,
    // Smooth and Formant, mono and stereo, at every rate, every sample is 0.
    #[test]
    fn a_silent_voice_shuts_every_band() {
        for sr in RATES {
            let c = saw(sr, 55.0, 20_000.0, 0.5);
            let quiet = vec![0.0; c.len()];
            for controls in [[0.5, 0.05, 0.0], [1.0, 0.01, 12.0], [1.0, 0.2, -12.0]] {
                assert!(play(sr, &c, &quiet, controls).iter().all(|&v| v == 0.0), "{sr} {controls:?}: sound from a silent voice");
            }
            let stereo = run(sr, 2, c.len(), BLOCK, &|p, ch, t| match p {
                0 => c[t] * if ch == 0 { 1.0 } else { -0.5 },
                CLARITY => 1.0,
                3 => 0.05,
                LISTEN => 1.0,
                _ => 0.0,
            });
            assert!(stereo.iter().all(|f| f[0] == 0.0 && f[1] == 0.0), "{sr}: sound in stereo from a silent voice");
        }
    }

    /// The Vocoder on `channels` channels, its every input `input(port, channel, frame)`, `split`
    /// frames a call: what came out, both channels, and whether it said it was active after each.
    fn run_active(
        sr: f32,
        channels: usize,
        frames: usize,
        split: usize,
        input: &dyn Fn(usize, usize, usize) -> f32,
    ) -> (Vec<[f32; 2]>, Vec<bool>) {
        let mut h = Host::<Vocoder>::of(cfg(sr, channels));
        let (mut out, mut active) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
        let mut t = 0;
        while t < frames {
            let n = split.clamp(1, BLOCK).min(frames - t);
            for p in 0..Vocoder::INPUTS {
                for c in 0..PORT_CHANNELS {
                    for k in 0..n {
                        h.port_mut(p)[c * BLOCK + k] = input(p, c, t + k);
                    }
                }
            }
            h.process(n as u32);
            let o = h.port_mut(Vocoder::INPUTS);
            out.extend((0..n).map(|k| [o[k], o[BLOCK + k]]));
            let a = h.active();
            active.extend((0..n).map(|_| a));
            t += n;
        }
        (out, active)
    }

    fn run(sr: f32, channels: usize, frames: usize, split: usize, input: &dyn Fn(usize, usize, usize) -> f32) -> Vec<[f32; 2]> {
        run_active(sr, channels, frames, split, input).0
    }

    // VOX-29: no band is louder than the carrier, and the knee is soft, at −48 dB. A sine voice at
    // band 10's centre (1 kHz), from −110 dB to +40 dB, plays a saw there at most at the saw's own
    // level and the bank's ripple (1.6 dB); far under the knee (−110 to −90 dB) 1 dB quieter for each
    // dB quieter (its level over the knee); at the knee about half (the follower reads a steady sine
    // at 0.88 of its height: −6.6 dB); and at −18 dB, a voice at a normal level, within a dB of the
    // saw's own level. At 44.1, 48 and 96 kHz.
    #[test]
    fn no_band_plays_louder_than_the_carrier() {
        for sr in RATES {
            let c = saw(sr, 55.0, 20_000.0, 1.0);
            let at = |db: f64| -> f64 {
                let a = 10f64.powf(db / 20.0);
                let v: Vec<f32> =
                    (0..c.len()).map(|i| (a * (std::f64::consts::TAU * centre(10) as f64 * i as f64 / sr as f64).sin()) as f32).collect();
                band_levels(&play(sr, &c, &v, [0.0, 0.05, 0.0]), sr)[10] - band_levels(&c, sr)[10]
            };
            let gains: Vec<(f64, f64)> = [-110.0, -90.0, -70.0, -48.0, -18.0, 0.0, 20.0, 40.0].into_iter().map(|db| (db, at(db))).collect();
            for &(db, g) in &gains {
                assert!(g < 1.6, "{sr}: a voice at {db} dB plays the carrier {g:.2} dB louder");
            }
            let g = |db: f64| gains.iter().find(|x| x.0 == db).unwrap().1;
            assert!((g(-90.0) - g(-110.0) - 20.0).abs() < 1.0, "{sr}: 1 dB a dB far under the knee: {gains:.2?}");
            assert!((-8.0..-5.0).contains(&g(-48.0)), "{sr}: at the knee, about half: {gains:.2?}");
            assert!(g(-18.0) > -1.0, "{sr}: a voice at a normal level plays the carrier near its own level: {gains:.2?}");
        }
    }

    // VOX-29: Smooth is how fast a band lets go: once a voice stops, what it opened falls by 1/e
    // (8.7 dB) in each Smooth. A sine voice at −90 dB (far under the knee, so the out follows it in
    // step) at band 10's centre stops at 0.5 s; under a sine carrier there, the out's height (the
    // loudest of each cycle), fitted in dB from 5 ms after the stop (the band's own ring is gone) to
    // 20 dB under, falls by 1/e in Smooth, within 3%, for Smooth 10 ms, 50 ms and 200 ms, at 44.1, 48
    // and 96 kHz.
    #[test]
    fn smooth_is_how_fast_a_band_lets_go() {
        for sr in RATES {
            smooth_at(sr);
        }
    }

    fn smooth_at(sr: f32) {
        let f = centre(10) as f64;
        let stop = (0.5 * sr) as usize;
        let n = stop + (2.5 * sr) as usize;
        let sine = |a: f64, i: usize| (a * (std::f64::consts::TAU * f * i as f64 / sr as f64).sin()) as f32;
        let c: Vec<f32> = (0..n).map(|i| sine(0.5, i)).collect();
        let v: Vec<f32> = (0..n).map(|i| if i < stop { sine(10f64.powf(-90.0 / 20.0), i) } else { 0.0 }).collect();
        let cycle = (sr as f64 / f).ceil() as usize;
        for smooth in [0.01f32, 0.05, 0.2] {
            let o = play(sr, &c, &v, [0.0, smooth, 0.0]);
            let height = |at: usize| o[at..at + cycle].iter().fold(0f32, |m, x| m.max(x.abs())) as f64;
            let from = stop + (0.005 * sr) as usize;
            let top = height(from);
            let points: Vec<(f64, f64)> = (from..n - cycle)
                .step_by(cycle)
                .map(|t| ((t - stop) as f64 / sr as f64, (height(t) / top).ln()))
                .take_while(|p| p.1 > -(10f64.ln())) // 20 dB under: ln 10 in nepers
                .collect();
            let (mt, my) = (
                points.iter().map(|p| p.0).sum::<f64>() / points.len() as f64,
                points.iter().map(|p| p.1).sum::<f64>() / points.len() as f64,
            );
            let slope =
                points.iter().map(|p| (p.0 - mt) * (p.1 - my)).sum::<f64>() / points.iter().map(|p| (p.0 - mt).powi(2)).sum::<f64>();
            let fell = -1.0 / slope;
            assert!(
                points.len() > 10 && (fell / smooth as f64 - 1.0).abs() < 0.03,
                "{sr}: Smooth {smooth}: it fell by 1/e in {:.2} ms ({} points)",
                fell * 1000.0,
                points.len()
            );
        }
    }

    /// The five vowels' first three formants (Hz), a man's (Peterson and Barney, rounded).
    const VOWELS: [[f64; 3]; 5] =
        [[730.0, 1090.0, 2440.0], [530.0, 1840.0, 2480.0], [270.0, 2290.0, 3010.0], [570.0, 840.0, 2410.0], [300.0, 870.0, 2240.0]];

    /// A made-up talker (review W): 4 syllables a second, each a vowel that glides from the one
    /// before, on a buzz at about `f0` Hz that rises and falls slowly (or on noise: a whisper), an
    /// "s" after every other one; `rms_db` dBFS RMS over the whole.
    fn talker(sr: f32, secs: f64, f0: f64, whisper: bool, rms_db: f64) -> Vec<f32> {
        let sr64 = sr as f64;
        let n = (secs * sr64) as usize;
        let mut rng = Lcg(11);
        let mut phase = 0.0f64;
        let hp = [m2_dsp::fx::Filter::highpass(sr, 4500.0, 0.5412), m2_dsp::fx::Filter::highpass(sr, 4500.0, 1.3066)];
        let mut hs = [[0f32; 2]; 2];
        let mut f: Vec<Formant> = VOWELS[0].iter().map(|&x| Formant::new(sr64, x, 100.0)).collect();
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / sr64;
            let syl = (t * 4.0).floor() as usize;
            let u = t * 4.0 - syl as f64;
            let (a, b) = (VOWELS[(syl + 4) % 5], VOWELS[syl % 5]);
            let mix = (u / 0.3).min(1.0);
            if i % 16 == 0 {
                for (k, ff) in f.iter_mut().enumerate() {
                    let y = ff.y;
                    *ff = Formant::new(sr64, a[k] + (b[k] - a[k]) * mix, [90.0, 110.0, 170.0][k]);
                    ff.y = y;
                }
            }
            let hz = f0 * (1.0 + 0.12 * (std::f64::consts::TAU * 0.4 * t).sin());
            phase += hz / sr64;
            let source = if whisper {
                rng.next() as f64 / 32_768.0 - 1.0
            } else {
                let mut s = 0.0;
                let mut h = 1.0;
                while h * hz < 0.45 * sr64 {
                    s += (std::f64::consts::TAU * h * phase).sin() / h;
                    h += 1.0;
                }
                s
            };
            let env = if u < 0.65 { (std::f64::consts::PI * u / 0.65).sin().powf(0.6) } else { 0.0 };
            let v = f.iter_mut().zip([1.0, 0.6, 0.3]).map(|(ff, g)| g * ff.tick(source)).sum::<f64>() * env;
            let w = rng.next() as f32 / 32_768.0 - 1.0;
            let s = {
                let a = hp[0].tick(&mut hs[0], w);
                hp[1].tick(&mut hs[1], a)
            };
            let s_env = if syl % 2 == 1 && (0.72..0.95).contains(&u) { (std::f64::consts::PI * (u - 0.72) / 0.23).sin() } else { 0.0 };
            out.push(v + 0.5 * s as f64 * s_env);
        }
        at_level(&out.iter().map(|&v| v as f32).collect::<Vec<f32>>(), rms_db)
    }

    /// `x` scaled to `rms_db` dBFS RMS over the whole.
    fn at_level(x: &[f32], rms_db: f64) -> Vec<f32> {
        let k = 10f64.powf(rms_db / 20.0) / rms_of(x);
        x.iter().map(|&v| (v as f64 * k) as f32).collect()
    }

    fn rms_of(x: &[f32]) -> f64 {
        (x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    /// A chord of three band-limited saws (A2, C3, E3) at `rms_db` dBFS RMS: the synth.
    fn chord(sr: f32, secs: f64, rms_db: f64) -> Vec<f32> {
        let notes = [110.0, 130.813, 164.814].map(|f| saw(sr, f, 20_000.0, secs));
        at_level(&(0..notes[0].len()).map(|i| notes.iter().map(|n| n[i]).sum()).collect::<Vec<f32>>(), rms_db)
    }

    /// How loud the out is against the synth alone (by RMS, in dB), from a quarter of a second in.
    fn against_dry(out: &[f32], dry: &[f32], sr: f32) -> f64 {
        let from = (0.25 * sr) as usize;
        20.0 * (rms_of(&out[from..]) / rms_of(&dry[from..])).log10()
    }

    // VOX-29: how loud the synth comes out doesn't hang on how loud the voice was recorded (review
    // W1). Under a chord of three saws at −15 dBFS RMS, a made-up talker (a man at 120 Hz, a woman at
    // 220 Hz) and a sung "ah" at 220 Hz, each at −30, −18 and −6 dBFS RMS, come out within 6 dB of
    // themselves at the other levels and no more than 12 dB under the synth alone; a whisper at −40
    // dBFS RMS within 12 dB of the man at −18; a room's noise at −60 dBFS RMS (a pause in the voice)
    // at least 20 dB under him; and digital silence, silence. At 44.1, 48 and 96 kHz.
    #[test]
    fn the_out_is_about_as_loud_whatever_the_voices_level() {
        for sr in RATES {
            let secs = 4.0;
            let dry = chord(sr, secs, -15.0);
            let out = |voice: &[f32]| against_dry(&play(sr, &dry, voice, [0.5, 0.05, 0.0]), &dry, sr);
            let voice = |name: &str, db: f64| match name {
                "a man" => talker(sr, secs, 120.0, false, db),
                "a woman" => talker(sr, secs, 220.0, false, db),
                _ => at_level(&steady(57.0, 0.3, secs).sing(sr), db),
            };
            let mut man_18 = 0.0;
            for name in ["a man", "a woman", "a sung ah"] {
                let levels: Vec<f64> = [-30.0, -18.0, -6.0].into_iter().map(|db| out(&voice(name, db))).collect();
                if name == "a man" {
                    man_18 = levels[1];
                }
                let (lo, hi) = (levels.iter().cloned().fold(f64::MAX, f64::min), levels.iter().cloned().fold(f64::MIN, f64::max));
                assert!(
                    hi - lo <= 6.0 && lo >= -12.0,
                    "{sr}: {name} at -30, -18 and -6 dBFS RMS comes out {levels:.1?} dB against the synth"
                );
            }
            let whisper = out(&talker(sr, secs, 0.0, true, -40.0));
            assert!((whisper - man_18).abs() <= 12.0, "{sr}: a whisper comes out {whisper:.1} dB, the man at -18 {man_18:.1} dB");
            let mut rng = Lcg(29);
            let room = at_level(&(0..(2.0 * sr) as usize).map(|_| rng.next() as f32 / 32_768.0 - 1.0).collect::<Vec<f32>>(), -60.0);
            let paused = against_dry(&play(sr, &dry[..room.len()], &room, [0.5, 0.05, 0.0]), &dry[..room.len()], sr);
            assert!(paused <= man_18 - 20.0, "{sr}: a room's noise comes out {paused:.1} dB, the man at -18 {man_18:.1} dB");
            assert!(play(sr, &dry, &vec![0.0; dry.len()], [0.5, 0.05, 0.0]).iter().all(|&v| v == 0.0), "{sr}: sound from digital silence");
        }
    }

    // VOX-29, RENDER-6a: it says it is active while any band is open, and once it says it is done,
    // nothing more comes out. A voice burst ending on a huge sample, then silence, under a saw that
    // plays on, a frame a call, at every rate and Smooth 200 ms: after the first call where it says
    // it is done, every sample out is 0; and it said so within 6 s (a follower falls to 0 below
    // m2-dsp's floor).
    #[test]
    fn nothing_comes_out_once_the_vocoder_says_it_is_done() {
        for sr in RATES {
            let c = saw(sr, 55.0, 20_000.0, 8.0);
            let burst = (0.2 * sr) as usize;
            let v = vowel(sr, 105.0, [525.0, 2100.0], 0.2, 0.5);
            let (out, active) = run_active(sr, 1, c.len(), 1, &|p, _, t| match p {
                0 => c[t],
                1 if t + 1 == burst => 1e30,
                1 if t < burst => v[t],
                CLARITY => 1.0,
                3 => 0.2,
                LISTEN => 1.0,
                _ => 0.0,
            });
            assert!(active[burst] && out[burst..].iter().any(|f| f[0] != 0.0), "{sr}: active, and heard, after the voice stops");
            let done =
                active.iter().skip(burst).position(|a| !a).map(|i| burst + i).unwrap_or_else(|| panic!("{sr}: still active after 8 s"));
            assert!(done < burst + 6 * sr as usize, "{sr}: active for {} s after the voice stopped", (done - burst) as f32 / sr);
            assert!(out[done + 1..].iter().all(|f| f[0] == 0.0), "{sr}: sound after it said it was done at {done}");
        }
    }

    // VOX-29, PKG-19: with nothing to listen to (`listen` 0, as `vocoder(x)` sets it) it plays its
    // input bit for bit: mono and stereo, any controls, whatever the voice, and samples no effect
    // should see kept as they are.
    #[test]
    fn with_nothing_to_listen_to_it_plays_its_input_bit_for_bit() {
        let odd = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0, 1e30, -1e-40, 0.25];
        for channels in [1, 2] {
            for listen in [0.0, f32::NAN, -1.0, 0.49] {
                let x = |c: usize, t: usize| if t.is_multiple_of(97) { odd[(t / 97) % odd.len()] } else { effects::saw(c, t) };
                let out = run(48_000.0, channels, 9_000, 100, &|p, c, t| match p {
                    0 => x(c, t),
                    1 => effects::noise(c, t),
                    LISTEN => listen,
                    _ => EXTREMES[(t / 1000 + p) % EXTREMES.len()],
                });
                for (t, f) in out.iter().enumerate() {
                    assert_eq!(f[0].to_bits(), x(0, t).to_bits(), "{channels} ch, listen {listen}: frame {t}");
                    if channels == 2 {
                        assert_eq!(f[1].to_bits(), x(1, t).to_bits(), "{channels} ch, listen {listen}: frame {t}, right");
                    }
                }
            }
        }
    }

    /// The Vocoder as the effects' harness runs an effect: its sound on `in` and on `voice` alike (a
    /// sound vocoded by itself), its controls Clarity, Smooth and Formant, listening.
    fn harnessed(channels: usize, frames: usize, split: usize, input: effects::Input<'_>) -> Vec<[f32; 2]> {
        run(effects::SR, channels, frames, split, &|p, c, t| match p {
            0 | 1 => input(0, c, t),
            LISTEN => 1.0,
            p => input(p - 1, c, t),
        })
    }

    const VOCODER: Fx = Fx { name: "vocoder", run: harnessed, controls: &[0.5, 0.05, 0.0] };

    // VOX-29: it holds to what every effect guarantees (m2-dsp's harness): silence in, silence out;
    // a tail that ends in exact silence; no setting or input makes a NaN or an infinity; a
    // full-scale sound stays near full scale; the same bits however its blocks split; a mono sound
    // stays mono; and it changes a saw.
    #[test]
    fn the_vocoder_holds_to_what_every_effect_guarantees() {
        effects::silence_in_silence_out([&VOCODER]);
        effects::tails_end_in_exact_silence([&VOCODER]);
        effects::extreme_settings_and_broken_input_never_make_nan([&VOCODER]);
        effects::output_stays_bounded([&VOCODER]);
        effects::deterministic_whatever_the_block_size([&VOCODER]);
        effects::mono_stays_mono([&VOCODER]);
        effects::every_effect_changes_the_sound([&VOCODER]);
    }

    // VOX-29: NaN and infinity in either input give a finite sound out, the voice's own included, at
    // every rate, mono and stereo, whatever the controls.
    #[test]
    fn nothing_finite_in_still_gives_a_finite_sound() {
        let odd = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, -1e30, f32::MAX];
        for sr in RATES {
            for channels in [1, 2] {
                let out = run(sr, channels, 20_000, BLOCK, &|p, c, t| match p {
                    0 | 1 if t % (37 + 11 * p) == 0 => odd[(t + p) % odd.len()],
                    0 => effects::saw(c, t),
                    1 => effects::noise(c, t),
                    LISTEN => 1.0,
                    _ => EXTREMES[(t / 777 + p) % EXTREMES.len()],
                });
                assert!(out.iter().all(|f| f[0].is_finite() && f[1].is_finite()), "{sr}, {channels} ch");
            }
        }
    }

    // VOX-29: its manifest is its code: its ports in the kit's order, with their defaults (Clarity
    // 0.5, Smooth 50 ms, Formant 0, listening; the voice silence when left out, ABI-12), one
    // output, and no latency.
    #[test]
    fn the_vocoders_manifest_is_its_code() {
        let defaults = defaults_in(MANIFEST, "vocoder", &["in", "voice", "clarity", "smooth", "formant", "listen"]);
        assert_eq!(defaults, [0.0, 0.0, 0.5, 0.05, 0.0, 1.0]);
        assert_eq!(manifest_in(MANIFEST, "vocoder").1, ["out"]);
        assert_eq!((Vocoder::INPUTS, Vocoder::OUTPUTS), (6, 1));
        let entry = &MANIFEST[MANIFEST.find("name = \"vocoder\"").unwrap()..];
        let entry = &entry[..entry.find("[[kernel]]").unwrap_or(entry.len())];
        assert!(entry.contains("\"voice: audio<*> = 0\"") && !entry.lines().any(|l| l.starts_with("latency")), "{entry}");
    }

    // RENDER-28, VOX-29: once born, it allocates nothing while it plays, at 44.1, 48 and 96 kHz, in
    // stereo: a voice and a saw, Formant moving (its bands placed again), Smooth and Clarity moving.
    #[test]
    fn the_vocoder_allocates_nothing_while_it_plays_at_every_rate() {
        for sr in RATES {
            let mut h = Host::<Vocoder>::of(cfg(sr, 2));
            let v = vowel(sr, 105.0, [525.0, 2100.0], 1.0, 0.5);
            let before = counting::here();
            for b in 0..v.len() / BLOCK {
                for k in 0..BLOCK {
                    let t = b * BLOCK + k;
                    h.port_mut(0)[k] = effects::saw(0, t);
                    h.port_mut(0)[BLOCK + k] = effects::saw(1, t);
                    h.port_mut(1)[k] = v[t];
                    h.port_mut(1)[BLOCK + k] = -v[t];
                    h.port_mut(4)[k] = 12.0 * (t as f32 / 9_000.0).sin();
                }
                h.port_mut(CLARITY)[..BLOCK].fill((b % 7) as f32 / 6.0);
                h.port_mut(3)[..BLOCK].fill(0.01 + (b % 5) as f32 * 0.04);
                h.port_mut(LISTEN)[..BLOCK].fill(1.0);
                h.process(BLOCK as u32);
            }
            assert_eq!(counting::here() - before, 0, "{sr} Hz: allocated while it played");
        }
    }

    // RENDER-2, VOX-29: a reset starts over, bit for bit, with no memory taken.
    #[test]
    fn a_reset_vocoder_starts_over() {
        let sr = 48_000.0;
        let c = saw(sr, 55.0, 20_000.0, 0.5);
        let v = vowel(sr, 105.0, [525.0, 2100.0], 0.5, 0.5);
        let mut h = Host::<Vocoder>::of(cfg(sr, 1));
        let take = |h: &mut Host<Vocoder>| -> Vec<f32> {
            let mut out = vec![];
            for b in 0..c.len() / BLOCK {
                h.port_mut(0)[..BLOCK].copy_from_slice(&c[b * BLOCK..(b + 1) * BLOCK]);
                h.port_mut(1)[..BLOCK].copy_from_slice(&v[b * BLOCK..(b + 1) * BLOCK]);
                for (p, x) in [(CLARITY, 0.7), (3, 0.03), (4, 3.0), (LISTEN, 1.0)] {
                    h.port_mut(p)[..BLOCK].fill(x);
                }
                h.process(BLOCK as u32);
                out.extend_from_slice(&h.port_mut(Vocoder::INPUTS)[..BLOCK]);
            }
            out
        };
        let first = take(&mut h);
        let before = counting::here();
        h.reset();
        assert_eq!(counting::here(), before, "a reset took memory");
        assert!(take(&mut h) == first && first.iter().any(|&x| x != 0.0), "a reset doesn't start over");
    }
}
