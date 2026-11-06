//! Effects that listen, rendered (PKG-19, PACK-28): std's `duck` with nothing to listen to plays its
//! input bit for bit, std's `compress` keyed from another sound turns down only while that sound
//! plays, and `@m2/mixing`'s Pump dips on each beat by its depth and swells back by its shape, with
//! no sidechain at all, and at depth 0 plays its input bit for bit. A song with no lanes plays at
//! 120 bpm, so a beat is half a second: 24 000 frames at 48 kHz.

mod common;

use common::*;

const BEAT: usize = 24_000;

/// A song of one expression, `out`, over `x` (a 220 Hz saw at half level) and `key` (a 50 Hz sine
/// that plays on the first beat of every two and is silent on the second).
fn rendered(what: &str, imports: &str, out: &str, frames: usize) -> Vec<[f32; 2]> {
    let manifest = format!(
        "[project]\nname = \"{what}\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.9\"\n\"@m2/mixing\" = \"1\"\n"
    );
    let song = format!(
        "import {{ song, time }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ lfo, when }} from \"std/control\";\nimport {{ scale }} from \"std/math\";\n{imports}\n\
         export default song(() => {{\n  const x = scale(osc.saw(220), 0.5);\n  const key = when(lfo(time.beats, {{ period: 2, shape: \"square\" }}), osc.sine(50));\n  \
         void key;\n  return {out};\n}});\n"
    );
    let s = project(what, &[("music2.toml", &manifest), ("song.ts", &song)]);
    let mut out = vec![[0.0; 2]; frames];
    s.renderer().process(&mut out).unwrap();
    out
}

/// The input alone, `x`.
fn dry(frames: usize) -> Vec<[f32; 2]> {
    rendered("dry", "", "x", frames)
}

// PKG-19: a Duck with no side, as + Effect adds it, plays its input bit for bit, at any depth.
#[test]
fn an_unkeyed_duck_plays_its_input_bit_for_bit() {
    let frames = 2 * BEAT;
    let x = dry(frames);
    assert!(peak(&x) > 0.4, "the saw plays");
    for out in ["duck(x)", "duck(x, { depth: -30 })"] {
        let y = rendered("duck", "import { duck } from \"std/fx\";", out, frames);
        assert_eq!(first_diff(&y, &x), None, "{out} changes its input");
    }
}

/// The RMS of `x` over each eighth of a beat.
fn levels(x: &[[f32; 2]]) -> Vec<f32> {
    x.chunks(BEAT / 8).map(rms).collect()
}

// PKG-19: keyed from another sound, the compressor turns its input down while the key plays and
// leaves it as it is while the key is silent; with no key it hears its input, and so turns it down
// the whole time (the saw is far over its threshold).
#[test]
fn a_keyed_compressor_turns_down_only_while_its_key_plays() {
    let frames = 4 * BEAT;
    let x = levels(&dry(frames));
    let opts = "{ threshold: -40, ratio: 8, attack: 0.001, release: 0.005 }";
    let keyed = levels(&rendered("keyed", "import { compress } from \"std/fx\";", &format!("compress(x, key, {opts})"), frames));
    let alone = levels(&rendered("alone", "import { compress } from \"std/fx\";", &format!("compress(x, {opts})"), frames));
    // Each eighth of a beat but the first of each beat, where the key starts or stops.
    for (i, ((x, k), a)) in x.iter().zip(&keyed).zip(&alone).enumerate() {
        let (beat, eighth) = (i / 8, i % 8);
        if eighth == 0 {
            continue;
        }
        let (k, a) = (db(*k) - db(*x), db(*a) - db(*x));
        assert!(a < -6.0, "eighth {i}: with no key it hears the saw and turns it down ({a:.1} dB)");
        match beat % 2 {
            // The key plays: turned down.
            0 => assert!(k < -6.0, "eighth {i}: the key plays, but it is down only {k:.1} dB"),
            // The key is silent: as it was.
            _ => assert!(k.abs() < 0.01, "eighth {i}: the key is silent, but it is {k:.2} dB"),
        }
    }
}

// PACK-28: Pump at depth 0 plays its input bit for bit, whatever its shape and rate.
#[test]
fn pump_at_depth_0_plays_its_input_bit_for_bit() {
    let frames = 2 * BEAT;
    let x = dry(frames);
    for out in ["pump(x, { depth: 0 })", "pump(x, { depth: 0, shape: 1, rate: \"1/16\" })"] {
        let y = rendered("pump0", "import { pump } from \"@m2/mixing\";", out, frames);
        assert_eq!(first_diff(&y, &x), None, "{out} changes its input");
    }
}

/// The gain, in dB, of `y` over `x` near frame `at`: from the loudest sample of `x` within a
/// millisecond of it, where the saw is far from zero.
fn gain_at(x: &[[f32; 2]], y: &[[f32; 2]], at: usize) -> f32 {
    let i = (at.saturating_sub(24)..at + 24).max_by(|a, b| x[*a][0].abs().total_cmp(&x[*b][0].abs())).unwrap();
    db(y[i][0].abs()) - db(x[i][0].abs())
}

// PACK-28: Pump's gain follows the song's beats: at each step's start it is down by `depth`, and it
// comes back as `shape` says, (1 - p) in dB at shape 0 and (1 - p)⁴ at 1, where p is how far
// through the step the song is; at rate 1/8 a step is half a beat.
#[test]
fn pumps_level_follows_the_beat() {
    let frames = 4 * BEAT;
    let x = dry(frames);
    for (rate, step, shape) in [("1/4", BEAT, 0.0f32), ("1/8", BEAT / 2, 0.0), ("1/4", BEAT, 1.0), ("1/4", BEAT, 0.5)] {
        let out = format!("pump(x, {{ depth: -12, shape: {shape}, rate: \"{rate}\" }})");
        let y = rendered("pump", "import { pump } from \"@m2/mixing\";", &out, frames);
        for n in 0..frames / step - 1 {
            for p in [0.05f32, 0.25, 0.5, 0.75, 0.95] {
                let at = n * step + (p * step as f32) as usize;
                let q = 1.0 - p;
                let want = -12.0 * (q + (q.powi(4) - q) * shape);
                let got = gain_at(&x, &y, at);
                assert!((got - want).abs() < 0.15, "{out}: step {n} at {p}: {got:.2} dB, not {want:.2}");
            }
        }
    }
}
