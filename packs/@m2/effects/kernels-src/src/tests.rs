//! Native tests for the pack's kernels: what every effect guarantees (m2-dsp's harness, which std's
//! effects are held to too), then what each one is for. Each was std's, with these tests, until it
//! moved here.

use m2_dsp::testing::effects::{self, BLOCK, Fx, SR, db, noise, peak, rms, run, saw, torture, with};
use m2_dsp::testing::{play_at, spectrum};
use m2_kernel::{Cfg, Host, Io, Kernel, db_to_gain};

use super::*;

/// Every effect at the settings the effect menu starts it with, and playing: the last of phaser's,
/// tremolo's and bitcrush's is `playing`, 1. (The delay is tried below, as std tried it: its own
/// tests, and the ABI's.)
const ALL: [Fx; 6] = [
    Fx { name: "reverb", run: run::<Reverb>, controls: &[0.6, 0.5, 0.25] },
    Fx { name: "phaser", run: run::<Phaser>, controls: &[0.4, 0.7, 0.5, 0.5, 1.0] },
    Fx { name: "tremolo", run: run::<Tremolo>, controls: &[5.0, 0.5, 1.0] },
    Fx { name: "bitcrush", run: run::<Bitcrush>, controls: &[8.0, 12_000.0, 1.0, 1.0] },
    Fx { name: "widen", run: run::<Widen>, controls: &[1.5] },
    Fx { name: "clip", run: run::<Clip>, controls: &[6.0, -0.3] },
];

/// The effect named `name`.
fn fx(name: &str) -> &'static Fx {
    ALL.iter().find(|fx| fx.name == name).unwrap()
}

/// The effects whose last input is `playing`.
const FOLLOW: [&str; 3] = ["phaser", "tremolo", "bitcrush"];

// ---------------------------------------------------------------- every effect

#[test]
fn silence_in_silence_out() {
    effects::silence_in_silence_out(&ALL);
}

#[test]
fn tails_end_in_exact_silence() {
    effects::tails_end_in_exact_silence(&ALL);
}

#[test]
fn extreme_settings_and_broken_input_never_make_nan() {
    effects::extreme_settings_and_broken_input_never_make_nan(&ALL);
}

#[test]
fn output_stays_bounded() {
    effects::output_stays_bounded(&ALL);
}

#[test]
fn deterministic_whatever_the_block_size() {
    effects::deterministic_whatever_the_block_size(&ALL);
}

#[test]
fn mono_stays_mono() {
    // Everything but the widener works on one channel too.
    effects::mono_stays_mono(ALL.iter().filter(|fx| fx.name != "widen"));
}

#[test]
fn every_effect_changes_the_sound() {
    effects::every_effect_changes_the_sound(&ALL);
}

// ---------------------------------------------------------------- ABI v1

/// One state's port buffers, each `BLOCK` floats, and the port table pointing at them: inputs
/// first, one mono channel each.
struct Ports {
    bufs: Vec<Vec<f32>>,
    table: Vec<*mut f32>,
}

impl Ports {
    fn new(n: usize) -> Ports {
        let mut bufs = vec![vec![0.0f32; BLOCK]; n];
        let table = bufs.iter_mut().map(|b| b.as_mut_ptr()).collect();
        Ports { bufs, table }
    }
}

type DelayState = m2_kernel::State<Delay>;

/// A block of `@m2/effects:delay` through its exports: its input `x(t)` from sample `t`, a 10 ms echo fed
/// back by half, half wet. Returns its output.
fn delay_block(state: *mut DelayState, ports: &mut Ports, t: usize, x: impl Fn(usize) -> f32) -> Vec<f32> {
    for (k, v) in ports.bufs[0].iter_mut().enumerate() {
        *v = x(t + k);
    }
    for (p, v) in [(1, 0.01), (2, 0.5), (3, 0.5)] {
        ports.bufs[p].fill(v);
    }
    // SAFETY: `state` is one `new` returned, and the table points at this state's own buffers.
    unsafe { crate::delay::process(state, ports.table.as_ptr(), 0, BLOCK as u32) };
    ports.bufs[4].clone()
}

// ABI v1 (report §2.3): every call names its state, so one instance of a module holds many, and no
// state lives in a static. Two delays made through the exports, fed different sounds in turns, each
// play exactly what one plays alone: neither hears the other's echoes.
#[test]
fn two_states_in_one_instance_play_independently() {
    let channels = [1u32; 5];
    let settings = m2_kernel::settings(&m2_kernel::Cfg::new(SR, BLOCK, 1), &channels);
    let sound = |seed: usize| move |t: usize| ((t * (7 + seed)) as f32 * 0.01).sin() * if seed == 0 { 0.8 } else { 0.3 };
    let new = || unsafe { crate::delay::new(&settings) };
    let alone = |seed: usize| {
        let (state, mut ports) = (new(), Ports::new(5));
        (0..20).flat_map(|b| delay_block(state, &mut ports, b * BLOCK, sound(seed))).collect::<Vec<f32>>()
    };
    let (a, b) = (new(), new());
    let (mut pa, mut pb) = (Ports::new(5), Ports::new(5));
    let (mut heard_a, mut heard_b) = (vec![], vec![]);
    for block in 0..20 {
        heard_a.extend(delay_block(a, &mut pa, block * BLOCK, sound(0)));
        heard_b.extend(delay_block(b, &mut pb, block * BLOCK, sound(1)));
    }
    assert!(heard_a.iter().any(|&v| v != 0.0) && heard_a != heard_b, "they play different sounds");
    assert!(heard_a == alone(0), "the first state plays as it does alone");
    assert!(heard_b == alone(1), "the second state plays as it does alone");
}

// ABI v1: a kernel whose state holds a `Vec` clears it in place at a reset (a reset takes no
// memory), and starts over exactly as a new one does.
#[test]
fn kernels_with_lines_reset_in_place_to_where_they_started() {
    use m2_dsp::testing::assert_reset_starts_over;
    assert_reset_starts_over::<Delay>(&[0.5, 0.01, 0.5, 0.5], 5_000);
    assert_reset_starts_over::<Reverb>(&[0.5, 0.6, 0.5, 0.5], 5_000);
    assert_reset_starts_over::<Widen>(&[0.5, 1.5], 5_000);
}

// ---------------------------------------------------------------- each effect

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
    let wet = (fx("reverb").run)(2, 96_000, BLOCK, &with(&noise, &[0.6, 0.5, 1.0]));
    let ratio = db(rms(&wet[48_000..], 0) / rms(&(0..48_000).map(|t| [noise(0, t), 0.0]).collect::<Vec<_>>(), 0));
    assert!((-6.0..3.0).contains(&ratio), "fully wet is {ratio:.1} dB from dry");
    // A bigger room rings longer.
    let burst = |c, t| if t < 4_800 { noise(c, t) } else { 0.0 };
    let tail = |size| rms(&(fx("reverb").run)(2, 72_000, BLOCK, &with(&burst, &[size, 0.5, 1.0]))[48_000..], 0);
    assert!(tail(0.9) > 4.0 * tail(0.3), "size 0.9 tail {} vs size 0.3 tail {}", tail(0.9), tail(0.3));
}

#[test]
fn phaser_is_an_allpass_until_mixed_with_the_dry_sound() {
    let dry = rms(&(0..48_000).map(|t| [noise(0, t), 0.0]).collect::<Vec<_>>(), 0);
    let wet = (fx("phaser").run)(1, 48_000, BLOCK, &with(&noise, &[0.4, 0.7, 0.0, 1.0, 1.0]));
    let level = db(rms(&wet, 0) / dry);
    assert!(level.abs() < 1.0, "fully wet without feedback is {level:.1} dB from dry");
    let mixed = (fx("phaser").run)(1, 48_000, BLOCK, &with(&noise, &[0.4, 0.7, 0.0, 0.5, 1.0]));
    let level = db(rms(&mixed, 0) / dry);
    assert!(level < -1.5, "the notches only take away {level:.1} dB");
}

#[test]
fn tremolo_dips_by_its_depth() {
    let dc = |_, _| 0.5;
    let out = (fx("tremolo").run)(1, 48_000, BLOCK, &with(&dc, &[5.0, 1.0, 1.0]));
    let (lo, hi) = out.iter().fold((f32::MAX, 0.0f32), |(lo, hi), f| (lo.min(f[0]), hi.max(f[0])));
    assert!(lo < 0.005 && hi > 0.499, "level swings between {lo} and {hi}");
    // Five dips a second.
    let dips = out.windows(3).filter(|w| w[1][0] < w[0][0] && w[1][0] <= w[2][0]).count();
    assert_eq!(dips, 5);
    let still = (fx("tremolo").run)(1, 4_800, BLOCK, &with(&dc, &[5.0, 0.0, 1.0]));
    assert!(still.iter().all(|f| f[0] == 0.5), "depth 0 changes the sound");
}

#[test]
fn widen_keeps_mono_compatibility() {
    let fx = fx("widen").run;
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

// ---------------------------------------------------------------- the bitcrusher

#[test]
fn bitcrush_rounds_and_holds() {
    let fx = run::<Bitcrush>;
    let clean = fx(2, 4_800, BLOCK, &with(&noise, &[24.0, SR, 1.0, 1.0]));
    assert!(clean.iter().enumerate().all(|(t, f)| (f[0] - noise(0, t)).abs() < 1e-6), "24 bits at full rate isn't clean");
    let two_bits = fx(2, 4_800, BLOCK, &with(&noise, &[2.0, SR, 1.0, 1.0]));
    assert!(two_bits.iter().all(|f| (f[0] * 2.0).fract() == 0.0 && f[0].abs() <= 1.0), "2 bits gives more than 5 levels");
    let held = fx(1, 4_800, BLOCK, &with(&noise, &[24.0, 12_000.0, 1.0, 1.0]));
    assert!(held.chunks(4).all(|c| c.iter().all(|f| f[0] == c[0][0])), "12 kHz doesn't hold each sample 4 times");
    let dry = fx(2, 4_800, BLOCK, &with(&noise, &[2.0, 4_000.0, 0.0, 1.0]));
    assert!(dry.iter().enumerate().all(|(t, f)| f[0] == noise(0, t)), "mix 0 changes the sound");
}

// ---------------------------------------------------------------- following the song

/// The bitcrusher before it followed the song, kept here as the oracle.
struct OldBitcrush {
    sr: f32,
    clock: f32,
    held: [f32; 2],
    bits: f32,
    steps: f32,
    mix: Glide,
}

impl OldBitcrush {
    fn process(&mut self, x: [f32; 2], bits: f32, rate: f32, mix: f32) -> [f32; 2] {
        let b = clampf(bits, 1.0, 24.0);
        if b != self.bits {
            self.bits = b;
            self.steps = (b - 1.0).exp2();
        }
        let x = x.map(sane);
        if self.clock >= 1.0 {
            self.clock -= 1.0;
            self.held = x.map(|x| (x * self.steps).round() / self.steps);
        }
        self.clock += clampf(rate, 20.0, self.sr) / self.sr;
        let m = self.mix.next(clampf(mix, 0.0, 1.0));
        [x[0] * (1.0 - m) + self.held[0] * m, x[1] * (1.0 - m) + self.held[1] * m]
    }
}

// While the song plays, the bitcrusher is what it always was, whatever its settings do.
#[test]
fn playing_the_bitcrusher_crushes_as_it_always_did() {
    let fx = ALL.iter().find(|fx| fx.name == "bitcrush").unwrap();
    for set in torture(fx).into_iter().map(|mut s| {
        *s.last_mut().unwrap() = 1.0;
        s
    }) {
        // Controls that move every 700 samples, around each setting.
        let control = |p: usize, t: usize| set[p - 1] * [1.0, 0.37, 1.9][t / 700 % 3];
        for channels in [1, 2] {
            let out = (fx.run)(channels, 5_000, 100, &|p, c, t| {
                if p == 0 {
                    noise(c, t)
                } else if p == 4 {
                    1.0
                } else {
                    control(p, t)
                }
            });
            let mut old = OldBitcrush { sr: SR, clock: 1.0, held: [0.0; 2], bits: f32::NAN, steps: 1.0, mix: Glide::new(0.02, SR) };
            for (t, got) in out.iter().enumerate() {
                let x = [noise(0, t), if channels == 2 { noise(1, t) } else { 0.0 }];
                let want = old.process(x, control(1, t), control(2, t), control(3, t));
                let want = [want[0], if channels == 2 { want[1] } else { 0.0 }];
                assert_eq!(got.map(f32::to_bits), want.map(f32::to_bits), "sample {t}, {channels} ch, controls {set:?}");
            }
        }
    }
}

// Stopped (`playing` 0, or NaN), the bitcrusher's clock is put back at Play: played on, the effect
// is what it is with no stop at all, bit for bit. A fresh one that sat stopped plays as one made
// at Play.
#[test]
fn stopped_the_effects_wait_and_play_on_where_they_were() {
    effects::stopped_they_wait_and_play_on_where_they_were(ALL.iter().filter(|fx| FOLLOW.contains(&fx.name)));
}

// Stopped, the bitcrusher's clock runs on, so a tail is still crushed: bit for bit as if the song
// played, a new held sample every period of its rate. A clock that waited would hold one sample,
// a DC level. Fresh and stopped from the start, or stopped after playing.
#[test]
fn stopped_the_bitcrusher_still_crushes() {
    let fx = ALL.iter().find(|fx| fx.name == "bitcrush").unwrap();
    let n = fx.controls.len();
    // 12 kHz on 48 kHz, all wet: a new held sample every 4 samples.
    assert_eq!(fx.controls[1..3], [12_000.0, 1.0]);
    for channels in [1, 2] {
        for pre in [0, 30_001] {
            let stop = pre..pre + 20_003;
            let run = |wait: f32| {
                (fx.run)(channels, stop.end + 1_000, BLOCK, &|p, c, t| {
                    if p == 0 {
                        noise(c, t)
                    } else if p == n && stop.contains(&t) {
                        wait
                    } else {
                        fx.controls[p - 1]
                    }
                })
            };
            let (playing, out) = (run(1.0), run(0.0));
            let diff = out[stop.clone()].iter().zip(&playing[stop.clone()]).position(|(a, b)| a.map(f32::to_bits) != b.map(f32::to_bits));
            assert_eq!(diff.map(|i| pre + i), None, "bitcrush ({channels} ch), stopped after {pre} samples: not crushed as while playing");
            // And that is not one held value: with silence in, it would be, and this test would see nothing.
            let changes = out[stop.clone()].windows(2).filter(|w| w[0] != w[1]).count();
            assert!(
                changes >= stop.len() / 4 * 9 / 10,
                "bitcrush ({channels} ch), stopped after {pre} samples: {changes} new values in {}",
                stop.len()
            );
        }
    }
}

// ---------------------------------------------------------------- the Clipper (PACK-34)

const MANIFEST: &str = include_str!("../../m2pkg.toml");

/// The rates a song may have that the Clipper's tests play at.
const RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];

/// The Clipper without its last step, the cut of what is still over the ceiling at the song's
/// rate: what its cut at four times the rate gives alone.
struct Uncut(Clip);

impl Kernel for Uncut {
    const INPUTS: usize = Clip::INPUTS;
    const OUTPUTS: usize = Clip::OUTPUTS;

    fn new(cfg: &Cfg) -> Self {
        Uncut(Clip::new(cfg))
    }

    fn process(&mut self, io: &mut Io) {
        self.0.run(io, false);
    }
}

/// `K` (the Clipper, or it uncut) on `x` at `sr`, mono, at `gain` and `ceiling` (dB): its output
/// and its meter.
fn clipped<K: Kernel>(sr: f32, x: &dyn Fn(usize) -> f32, frames: usize, gain: f32, ceiling: f32) -> (Vec<f32>, Vec<f32>) {
    let played = play_at::<K>(sr, 1, frames, BLOCK, &|p, _, t| match p {
        0 => x(t),
        1 => gain,
        _ => ceiling,
    });
    (played.out[0].iter().map(|f| f[0]).collect(), played.out[1].iter().map(|f| f[0]).collect())
}

/// A cut at the song's rate: what a naive clipper plays.
fn naive(x: &dyn Fn(usize) -> f32, frames: usize, gain: f32, ceiling: f32) -> Vec<f32> {
    let (g, c) = (db_to_gain(gain), db_to_gain(ceiling));
    (0..frames).map(|t| (x(t) * g).clamp(-c, c)).collect()
}

/// A sine at `hz` and `sr`, peaking at `level`.
fn sine(hz: f64, sr: f32, level: f32) -> impl Fn(usize) -> f32 {
    move |t| level * (std::f64::consts::TAU * hz * t as f64 / sr as f64).sin() as f32
}

/// The loudest partial from 20 Hz to 20 kHz of `x` that is no harmonic of `f0`, in dB under its
/// fundamental: what a clip folded back.
fn worst_alias(x: &[f32], f0: f64, sr: f32) -> f64 {
    let spec = spectrum(x, sr);
    // The Kaiser window's main lobe, and more.
    let lobe = 14.0 * spec[1].0;
    let fundamental = spec.iter().filter(|s| (s.0 - f0).abs() <= lobe).map(|s| s.1).fold(0.0, f64::max);
    let harmonic = |f: f64| {
        let n = (f / f0).round();
        n >= 1.0 && (f - n * f0).abs() <= lobe
    };
    let worst = spec.iter().filter(|s| (20.0..=20_000.0).contains(&s.0) && !harmonic(s.0)).map(|s| s.1).fold(0.0, f64::max);
    10.0 * (worst.max(1e-300) / fundamental).log10()
}

// PACK-34: the Clipper's manifest says what its code is: its ports, its defaults and ranges, its
// meter, and its latency, the frames its two cuts' filters take there and back, 46 each.
#[test]
fn the_clippers_manifest_is_its_code() {
    assert!(MANIFEST.contains(r#"inputs = ["in: audio<*>", "gain: control<db> = 0 in 0..24", "ceiling: control<db> = -0.3 in -24..0"]"#));
    assert!(MANIFEST.contains(r#"meters = ["pushed: control<db>"]"#));
    assert!(MANIFEST.contains(&format!("latency = {}\n", clipper::LATENCY)), "its manifest's latency is its code's");
    assert_eq!((clipper::CUTS, clipper::STAGE, clipper::LATENCY), (2, 46, 92));
}

// PACK-34: its two half-bands are Kaiser-windowed sincs (β for 100 dB), 79 taps at twice the rate
// and 27 at four times: each tap, one side of its centre, as worked out again here.
#[test]
fn the_clippers_filters_are_their_windowed_sincs() {
    fn i0(x: f64) -> f64 {
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..200 {
            term *= (x / (2.0 * k as f64)).powi(2);
            sum += term;
            if term < 1e-17 * sum {
                break;
            }
        }
        sum
    }
    let beta = 0.1102 * (100.0 - 8.7);
    let taps = |m: usize| -> Vec<f64> {
        (0..m.div_ceil(2))
            .map(|q| {
                let k = (m - 2 * q) as f64;
                let w = i0(beta * (1.0 - (k / m as f64).powi(2)).max(0.0).sqrt()) / i0(beta);
                (std::f64::consts::PI * k / 2.0).sin() / (std::f64::consts::PI * k) * w
            })
            .collect()
    };
    for (made, h) in [(taps(39), &clipper::H0[..]), (taps(13), &clipper::H1[..])] {
        assert_eq!(made.len(), h.len());
        for (q, (m, h)) in made.iter().zip(h).enumerate() {
            assert!((*m as f32 - h).abs() <= 1e-7, "tap {q}: {h}, worked out {m}");
        }
    }
}

// PACK-34: no sample out is ever over the ceiling: sines, a saw, noise and clicks, at 44.1, 48 and
// 96 kHz, turned up by 0 to 24 dB, under ceilings from -24 dB to 0 dB.
#[test]
fn no_sample_out_is_ever_over_the_ceiling() {
    let clicks = |t: usize| match (t.is_multiple_of(997), t.is_multiple_of(1994)) {
        (true, true) => 1.0,
        (true, false) => -1.0,
        _ => 0.0,
    };
    type Sound<'a> = (&'a str, Box<dyn Fn(usize) -> f32>);
    for sr in RATES {
        let signals: [Sound; 7] = [
            ("100 Hz", Box::new(sine(100.0, sr, 1.0))),
            ("997 Hz", Box::new(sine(997.0, sr, 1.0))),
            ("5 kHz", Box::new(sine(5_000.0, sr, 1.0))),
            ("15 kHz", Box::new(sine(15_000.0, sr, 0.7))),
            ("a saw", Box::new(|t| saw(0, t))),
            ("noise", Box::new(|t| noise(1, t))),
            ("clicks", Box::new(clicks)),
        ];
        for (name, x) in &signals {
            for gain in [0.0, 6.0, 12.0, 18.0, 24.0] {
                for ceiling in [-24.0, -12.0, -6.0, -0.3, 0.0] {
                    let (out, _) = clipped::<Clip>(sr, x.as_ref(), 6_000, gain, ceiling);
                    let c = db_to_gain(ceiling);
                    let over = out.iter().position(|v| v.abs() > c);
                    assert_eq!(over, None, "{name} at {sr} Hz, gain {gain} dB, ceiling {ceiling} dB: over the ceiling at {over:?}");
                }
            }
        }
    }
}

/// A band-limited saw at `hz`: its harmonics under 20 kHz, peaking near 0.92.
fn saw_at(hz: f64, sr: f32) -> impl Fn(usize) -> f32 {
    let top = (20_000.0 / hz).floor() as usize;
    move |t| {
        let at = std::f64::consts::TAU * hz * t as f64 / sr as f64;
        let s: f64 = (1..=top).map(|n| if n % 2 == 1 { 1.0 } else { -1.0 } * (n as f64 * at).sin() / n as f64).sum();
        (s * 2.0 / std::f64::consts::PI * 0.92) as f32
    }
}

/// What one cut gives alone, driven directly: `x` times `gain` (dB) under `ceiling` (dB), at `sr`.
fn one_cut(x: &dyn Fn(usize) -> f32, frames: usize, gain: f32, ceiling: f32) -> Vec<f32> {
    let (g, c) = (db_to_gain(gain), db_to_gain(ceiling));
    let mut cut = clipper::Cut::NEW;
    (0..frames).map(|t| cut.frame(x(t) * g, c, clipper::QUIET).0).collect()
}

// PACK-34: its cuts, at four times the song's rate, fold far less back into hearing than a cut at the
// song's rate (naive clipping). At 48 kHz, the worst partial that is no harmonic, under the
// fundamental: a 5 kHz sine driven 18 dB over the ceiling, 40 dB under from one cut alone, 35 dB from
// the Clipper (its second cut takes most of the first one's ring off, and its last step cuts what
// still rings over the ceiling at the song's rate); a 997 Hz sine driven 18 dB, 55 dB and 60 dB; a
// band-limited saw at 220 Hz driven 12 dB under a ceiling of -0.3 dB, 65 dB from the Clipper. The
// Clipper 10 dB or more under naive clipping each time.
#[test]
fn the_clipper_folds_far_less_back_than_naive_clipping() {
    let sr = 48_000.0;
    let n = 1 << 16;
    let from = 4096 + clipper::LATENCY;
    // Each: its name, the sound, its fundamental, the gain and ceiling (dB), the most one cut alone
    // and the Clipper fold back (dB).
    type Case<'a> = (&'a str, Box<dyn Fn(usize) -> f32>, f64, f32, f32, Option<f64>, f64);
    let cases: [Case; 3] = [
        ("a 5 kHz sine, +18 dB", Box::new(sine(5_000.0, sr, 1.0)), 5_000.0, 18.0, 0.0, Some(-40.0), -35.0),
        ("a 997 Hz sine, +18 dB", Box::new(sine(997.0, sr, 1.0)), 997.0, 18.0, 0.0, Some(-55.0), -60.0),
        ("a band-limited saw at 220 Hz, +12 dB", Box::new(saw_at(220.0, sr)), 220.0, 12.0, -0.3, None, -65.0),
    ];
    for (name, x, f0, gain, ceiling, alone, most) in cases {
        let (out, _) = clipped::<Clip>(sr, x.as_ref(), from + n, gain, ceiling);
        let (both, _) = clipped::<Uncut>(sr, x.as_ref(), from + n, gain, ceiling);
        let first = one_cut(x.as_ref(), from + n, gain, ceiling);
        let plain = naive(x.as_ref(), from + n, gain, ceiling);
        let [ours, both, first, plain] = [&out, &both, &first, &plain].map(|y| worst_alias(&y[from..], f0, sr));
        println!(
            "{name}: the Clipper {ours:.1} dB; its two cuts alone {both:.1} dB; one cut alone {first:.1} dB; naive clipping {plain:.1} dB"
        );
        if let Some(alone) = alone {
            assert!(first <= alone, "{name}: one cut alone folds back {first:.1} dB, over {alone} dB");
        }
        assert!(ours <= most, "{name}: it folds back {ours:.1} dB, over {most} dB");
        assert!(plain - ours >= 10.0, "{name}: {ours:.1} dB against naive clipping's {plain:.1} dB");
    }
}

// PACK-34: a cut is the clipped sound's own harmonics: one cut, a 1 kHz sine driven 12 dB going in,
// comes out with each odd harmonic up to 19 kHz within 0.1 dB of a clipped sine's (its Fourier
// series), and its even ones gone, so the sound going round the filters and the cut brought back
// down line up.
#[test]
fn the_cut_is_a_clipped_sounds_own_harmonics() {
    let (sr, hz, drive) = (48_000.0f32, 1_000.0, 10f64.powf(12.0 / 20.0));
    let (x, g) = (sine(hz, sr, 1.0), db_to_gain(12.0));
    let from = 4096 + clipper::STAGE;
    let mut cut = clipper::Cut::NEW;
    let out: Vec<f32> = (0..from + 48_000).map(|t| cut.frame(x(t) * g, 1.0, clipper::QUIET).0).collect();
    let out = &out[from..];
    // Each harmonic's level as the projection on its sine and cosine over a second: whole periods.
    let level = |n: usize| -> f64 {
        let w = std::f64::consts::TAU * hz * n as f64 / sr as f64;
        let (s, c) = out.iter().enumerate().fold((0.0, 0.0), |(s, c), (t, &v)| {
            let a = w * (t + from) as f64;
            (s + v as f64 * a.sin(), c + v as f64 * a.cos())
        });
        2.0 * (s * s + c * c).sqrt() / out.len() as f64
    };
    // A clipped sine's Fourier series: b_n = (2/π) ∫_0^π clip(A sin θ) sin(nθ) dθ.
    let series = |n: usize| -> f64 {
        let steps = 200_000;
        let d = std::f64::consts::PI / steps as f64;
        (0..steps).map(|i| (drive * ((i as f64 + 0.5) * d).sin()).min(1.0) * ((i as f64 + 0.5) * d * n as f64).sin()).sum::<f64>() * d * 2.0
            / std::f64::consts::PI
    };
    for n in 1..=19 {
        let got = level(n);
        if n % 2 == 1 {
            let want = series(n).abs();
            assert!(
                (db(got as f32) - db(want as f32)).abs() < 0.1,
                "harmonic {n}: {:.2} dB, a clipped sine's {:.2} dB",
                db(got as f32),
                db(want as f32)
            );
        } else {
            assert!(got < 1e-4, "harmonic {n}: {got}, where a clipped sine has none");
        }
    }
}

// PACK-34: under the ceiling the Clipper is its input times its gain, bit for bit, 92 frames late,
// at every rate: noise and a sweep turned up 6 dB, under a ceiling of -0.3 dB.
#[test]
fn under_the_ceiling_the_clipper_is_its_input_92_frames_late() {
    for sr in RATES {
        let quiet = |t: usize| 0.25 * noise(0, t);
        let sweep = move |t: usize| {
            let s = t as f64 / sr as f64;
            0.4 * (std::f64::consts::TAU * (50.0 * s + 2_000.0 * s * s)).sin() as f32
        };
        for (name, x) in [("noise", &quiet as &dyn Fn(usize) -> f32), ("a sweep", &sweep)] {
            let frames = sr as usize;
            let (out, _) = clipped::<Clip>(sr, x, frames, 6.0, -0.3);
            let (g, late) = (db_to_gain(6.0), 92);
            assert!(out[..late].iter().all(|&v| v == 0.0), "{name} at {sr} Hz: silence while it is late");
            for t in 0..frames - late {
                let want = x(t) * g;
                assert!(want.abs() < db_to_gain(-0.3), "the test's sound is under the ceiling");
                assert_eq!(out[t + late].to_bits(), want.to_bits(), "{name} at {sr} Hz, frame {t}");
            }
        }
    }
}

// PACK-34, the house rules: a sample that isn't finite counts as silence and a control that isn't
// finite as its low end, so what comes out is finite, and under the ceiling.
#[test]
fn nothing_finite_in_still_gives_a_finite_sound() {
    let hostile = |t: usize| match t % 101 {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => 1e30,
        _ => noise(0, t),
    };
    for (gain, ceiling) in [(0.0, -0.3), (24.0, 0.0), (f32::NAN, f32::NAN), (f32::INFINITY, f32::NEG_INFINITY), (-1e9, 1e9)] {
        let (out, pushed) = clipped::<Clip>(48_000.0, &hostile, 4_800, gain, ceiling);
        assert!(out.iter().chain(&pushed).all(|v| v.is_finite()), "gain {gain}, ceiling {ceiling}");
        assert!(out.iter().all(|v| v.abs() <= 1.0), "gain {gain}, ceiling {ceiling}: over 0 dB");
    }
}

// PACK-34, ABI-13: its meter `pushed` says the loudest it heard after its gain, at four times the
// rate, in dB, the most of each call's frames: a sine at -6 dB turned up 12 dB is pushed to +6 dB;
// silence to -100 dB.
#[test]
fn the_clipper_says_how_loud_it_is_pushed() {
    let x = sine(997.0, 48_000.0, 0.5);
    let (_, pushed) = clipped::<Clip>(48_000.0, &x, 9_600, 12.0, -0.3);
    for (k, block) in pushed.chunks(BLOCK).enumerate().skip(4) {
        assert!(block.iter().all(|v| v.to_bits() == block[0].to_bits()), "one number for the call");
        assert!((block[0] - 6.0).abs() < 0.1, "block {k}: pushed {} dB, the sine is at +6 dB", block[0]);
    }
    let (_, pushed) = clipped::<Clip>(48_000.0, &|_| 0.0, 1_280, 12.0, -0.3);
    assert!(pushed.iter().all(|&v| v == -100.0), "silence: -100 dB");
}

/// The Clipper, each of its cuts keeping its filters coming down for `Q` frames once it took nothing
/// off (`QUIET` is the Clipper's own): `usize::MAX` keeps them for good.
struct Kept<const Q: usize>(Clip);

impl<const Q: usize> Kernel for Kept<Q> {
    const INPUTS: usize = Clip::INPUTS;
    const OUTPUTS: usize = Clip::OUTPUTS;

    fn new(cfg: &Cfg) -> Self {
        let mut clip = Clip::new(cfg);
        clip.quiet = Q;
        Kept(clip)
    }

    fn process(&mut self, io: &mut Io) {
        self.0.run(io, true);
    }
}

/// What `K` plays of sparse cuts: bursts of a loud sine, decaying hits, noise bursts, single samples
/// over the ceiling, and a sine just under it with clicks; mono and stereo, in blocks of 128 and 7,
/// at 0 dB under -0.3 dB, and with the gain moving under -3 dB.
fn sparse_cuts<K: Kernel>() -> Vec<Vec<[f32; 2]>> {
    use std::f64::consts::TAU;
    type Sound = Box<dyn Fn(usize, usize) -> f32>;
    let sounds: [Sound; 5] = [
        Box::new(|_, t| if (t / 300) % 3 == 0 { 1.2 * (TAU * 1234.5 * t as f64 / 48_000.0).sin() as f32 } else { 0.0 }),
        Box::new(|_, t| {
            let k = t % 2_371;
            (-(k as f32) / 300.0).exp() * (k as f32 * 0.37).sin() * 1.5
        }),
        Box::new(|c, t| if (t / 97) % 5 == 0 { 1.1 * noise(c, t) } else { 0.02 * noise(c, t) }),
        Box::new(|_, t| if t % 523 == 0 { 1.5 } else { 0.0 }),
        Box::new(|_, t| 0.97 * (TAU * 997.0 * t as f64 / 48_000.0).sin() as f32 + if t % 1_777 == 3 { 0.3 } else { 0.0 }),
    ];
    let mut all = vec![];
    for x in &sounds {
        for channels in [1, 2] {
            for split in [BLOCK, 7] {
                for moving in [false, true] {
                    let played = play_at::<K>(48_000.0, channels, 40_000, split, &|p, c, t| match (p, moving) {
                        (0, _) => x(c, t),
                        (1, false) => 0.0,
                        (1, true) => {
                            if (t / 5_000) % 2 == 0 {
                                6.0
                            } else {
                                0.0
                            }
                        }
                        (_, false) => -0.3,
                        (_, true) => -3.0,
                    });
                    all.push(played.out[0].clone());
                }
            }
        }
    }
    all
}

// PACK-34: each cut leaves out its filters coming down once it has taken nothing off for QUIET frames,
// and that changes no bit: on sparse cuts, mono and stereo, in blocks of 128 and 7, the Clipper plays
// what it plays with them always run. They reach back exactly as far as a cut is late: one frame
// fewer changes the bits.
#[test]
fn leaving_out_the_filters_coming_down_changes_no_bit() {
    let differ = |a: &[Vec<[f32; 2]>], b: &[Vec<[f32; 2]>]| -> usize {
        a.iter()
            .zip(b)
            .map(|(x, y)| x.iter().zip(y).filter(|(p, q)| p[0].to_bits() != q[0].to_bits() || p[1].to_bits() != q[1].to_bits()).count())
            .sum()
    };
    let always = sparse_cuts::<Kept<{ usize::MAX }>>();
    let ours = sparse_cuts::<Clip>();
    let fewer = sparse_cuts::<Kept<{ clipper::QUIET - 1 }>>();
    assert_eq!(clipper::QUIET, clipper::STAGE);
    assert_eq!(differ(&ours, &always), 0, "leaving the filters out at QUIET changes no bit");
    let changed = differ(&fewer, &always);
    println!("with QUIET one frame fewer, {changed} samples differ");
    assert!(changed > 0, "one frame fewer would change no bit either: QUIET isn't as short as it can be");
}

// PACK-34, RENDER-6a: the Clipper says it is active for as long as anything it took in can still come
// out. Played a frame a call, a burst of noise that ends on one sample, of 1, -1, 1e30 or the most a
// float holds, pushed 24 dB under a ceiling of -24 dB, at 44.1, 48 and 96 kHz, mono and stereo:
// nothing comes out after the call where it first says it is done.
#[test]
fn nothing_comes_out_once_the_clipper_says_it_is_done() {
    let stop = 3_000;
    for sr in RATES {
        for last in [1.0f32, -1.0, 1e30, f32::MAX] {
            for channels in [1, 2] {
                let played = play_at::<Clip>(sr, channels, stop + 4 * clipper::LATENCY, 1, &|p, c, t| match p {
                    0 if t < stop => 0.5 * noise(c, t),
                    0 if t == stop => last,
                    0 => 0.0,
                    1 => 24.0,
                    _ => -24.0,
                });
                let out = &played.out[0];
                let done = (stop..out.len()).find(|&t| !played.active[t]).expect("it says it is done");
                let heard = out.iter().rposition(|f| f[0] != 0.0 || f[1] != 0.0).expect("it played");
                assert!(
                    heard <= done,
                    "{last} at {sr} Hz, {channels} channels: sound at frame {heard}, after it said it was done at {done}"
                );
            }
        }
    }
}
