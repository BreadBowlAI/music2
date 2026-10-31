//! Native tests for the pack's kernels: what every effect of std's guarantees (std's
//! `fx/tests.rs`, where these were until each left std), then what each one is for.

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

/// Every effect at the settings the effect menu starts it with, and playing: the last of phaser's,
/// tremolo's and bitcrush's is `playing`, 1.
const ALL: [Fx; 5] = [
    Fx { name: "reverb", run: run::<Reverb>, controls: &[0.6, 0.5, 0.25] },
    Fx { name: "phaser", run: run::<Phaser>, controls: &[0.4, 0.7, 0.5, 0.5, 1.0] },
    Fx { name: "tremolo", run: run::<Tremolo>, controls: &[5.0, 0.5, 1.0] },
    Fx { name: "bitcrush", run: run::<Bitcrush>, controls: &[8.0, 12_000.0, 1.0, 1.0] },
    Fx { name: "widen", run: run::<Widen>, controls: &[1.5] },
];

/// The effect named `name`.
fn fx(name: &str) -> &'static Fx {
    ALL.iter().find(|fx| fx.name == name).unwrap()
}

/// The effects whose last input is `playing`.
const FOLLOW: [&str; 3] = ["phaser", "tremolo", "bitcrush"];

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
    // At any setting, as std's effects are held to.
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
    // Everything but the widener works on one channel too, so it fits wherever a mono sound goes
    // (a panned mixer channel).
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
    use m2_dsp::testkit::assert_reset_starts_over;
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

// Stopped (`playing` 0, or NaN), the bitcrusher's clock is put back at Play: played on, the effect
// is what it is with no stop at all, bit for bit. A fresh one that sat stopped plays as one made
// at Play.
#[test]
fn stopped_the_effects_wait_and_play_on_where_they_were() {
    // 20 003 samples: no whole number of any of their cycles.
    let pause = 20_003;
    for fx in ALL.iter().filter(|fx| FOLLOW.contains(&fx.name)) {
        for channels in [1, 2] {
            // The bitcrusher's clock steps by exactly 1/4 here (12 kHz on 48 kHz), so a stop after
            // a whole number of 4 samples finds it at 1, where it starts: putting back 1 at Play
            // couldn't be told from putting back where it was. 30 001 stops it at 1.25.
            for pre in [0, 30_000, 30_001] {
                let straight = stopped(fx, channels, pre, 0, 1.0);
                let free = stopped(fx, channels, pre, pause, 1.0);
                assert!(free[pre + pause..] != straight[pre..], "{}: what its clock did while stopped can't be heard", fx.name);
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
