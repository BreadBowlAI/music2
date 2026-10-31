//! Native tests for the pack's kernels: what every effect guarantees (m2-dsp's harness, which std's
//! effects are held to too), then what each one is for.

use m2_dsp::testing::effects::{self, BLOCK, Fx, SR, noise, run, torture, with};

use super::*;

/// Every effect at the settings the effect menu starts it with, and playing: the last of
/// bitcrush's is `playing`, 1.
const ALL: [Fx; 1] = [Fx { name: "bitcrush", run: run::<Bitcrush>, controls: &[8.0, 12_000.0, 1.0, 1.0] }];

/// The effects whose last input is `playing`.
const FOLLOW: [&str; 1] = ["bitcrush"];

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
    effects::mono_stays_mono(&ALL);
}

#[test]
fn every_effect_changes_the_sound() {
    effects::every_effect_changes_the_sound(&ALL);
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
