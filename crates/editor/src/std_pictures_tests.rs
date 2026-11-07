//! std's pictures drawn from its own sums are the sound (CARD-99, D13): each against a render of the
//! kernel it draws, within 0.5 dB. The compressor's transfer curve at steady levels in and around
//! its knee; the envelope of std's adsr from its first sample to the end of its release; the LFO's
//! shape over a bar; and std/pictures' lowpass, a drawing for filters inside a voice, against std's
//! own filter measured from its sound, where an analog curve holds (up to a twelfth of the rate).

use std::path::{Path, PathBuf};
use std::time::Duration;

use m2_runtime::Renderer;
use m2_script::pictures::{Ask, Drawn, Runner};
use serde_json::{Map, Value, json};

use crate::measure::{Layer, Pass, Span, copy};
use crate::testing::{Fixture, MANIFEST};

const RATE: f64 = 48_000.0;

fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

/// Picture `view` of the component `cid` of `packages` at `values`: its shapes and words, as the runner draws them.
fn drawn(packages: Vec<(String, PathBuf)>, cid: &str, view: &str, values: Value) -> Value {
    let catalog = m2_script::describe(&packages);
    let descriptor = catalog.components.get(cid).cloned().unwrap_or_else(|| panic!("no {cid}: {:?}", catalog.problems));
    let ask = Ask {
        key: cid.into(),
        packages,
        song: None,
        module: descriptor.module.clone(),
        descriptor,
        view: view.into(),
        values: values.to_string(),
        memo: format!("{cid}\u{0}{view}"),
    };
    match Runner::new().draw(ask) {
        Drawn::Shapes(json) => serde_json::from_str(&json).unwrap(),
        other => panic!("{cid}'s {view} didn't draw: {other:?}"),
    }
}

/// The points of the line `name` a picture drew.
fn line(drawn: &Value, name: &str) -> Vec<(f64, f64)> {
    let s = drawn["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "line" && s["name"] == name)
        .unwrap_or_else(|| panic!("no line {name}: {drawn}"));
    s["points"].as_array().unwrap().chunks(2).map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect()
}

/// `seconds` of the song `body` (after std's imports) at 48 kHz, as it plays: its left side.
fn render(body: &str, seconds: f64) -> Vec<f64> {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        format!(
            "import {{ db, song, track, time }} from '@m2/core';\nimport {{ adsr, gain, osc, vca }} from 'std/dsp';\n\
             import {{ compress }} from 'std/fx';\nimport {{ lfo }} from 'std/control';\n{body}"
        ),
    );
    let s = f.open();
    let rev = s.accepted();
    let mut r = Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, rev.project.sample_rate()).unwrap();
    let mut out = vec![[0.0f32; 2]; (seconds * RATE) as usize];
    r.process(&mut out).unwrap();
    out.iter().map(|x| x[0] as f64).collect()
}

/// A steady sound of 1 for 500 s: a square wave at 0.001 Hz, after its first edge.
const STEADY: &str = "osc.square(0.001)";

/// A level in dB.
fn db(x: f64) -> f64 {
    20.0 * x.abs().max(1e-12).log10()
}

/// The first sample of `frames` the picture's edges leave alone: a square's first edge is smoothed over a few samples.
const SETTLED: usize = 16;

// CARD-99: std's compressor's transfer curve is what it plays: a steady level in comes out where the
// curve says, within 0.5 dB, below its knee, in it and above it, with makeup.
#[test]
fn the_compressors_transfer_curve_is_its_sound() {
    // A gentle one with makeup, and a hard one whose knee alone is more than 0.5 dB at its threshold.
    for (threshold, ratio, makeup) in [(-24.0, 4.0, 3.0), (-30.0, 20.0, 0.0)] {
        let v = json!({"threshold": threshold, "ratio": ratio, "attack": 0.001, "release": 0.05, "makeup": makeup});
        let curve = line(&drawn(vec![("std".into(), std_dir())], "std:compress", "transfer", v), "transfer");
        let at = |level: f64| *curve.iter().min_by(|a, b| (a.0 - level).abs().total_cmp(&(b.0 - level).abs())).unwrap();
        for step in [-60.0, -48.0, -36.0, -6.0, -3.0, -1.0, 0.0, 1.0, 3.0, 6.0, 12.0, 24.0] {
            let level: f64 = threshold + step;
            if !(-60.0..=0.0).contains(&level) {
                continue;
            }
            let (x, y) = at(level);
            assert_eq!(x, level, "the curve has a point at {level} dB");
            let out = render(
                &format!(
                    "export default song(() => compress(gain({STEADY}, db({level})), {{ threshold: db({threshold}), ratio: {ratio}, attack: 0.001, release: 0.05, makeup: db({makeup}) }}));\n"
                ),
                0.5,
            );
            let heard = db(out[out.len() - 1]);
            assert!(
                (heard - y).abs() <= 0.5,
                "ratio {ratio}, at {level} dB in: the curve says {y:.2} dB out, the compressor plays {heard:.2} dB"
            );
        }
    }
}

// CARD-99: std's adsr's envelope is what it plays: the picture's line, across its attack, decay,
// held sustain and release, is the kernel's level at that time within 0.5 dB wherever it is above
// −40 dB, when the gate is held as long as the picture holds it.
#[test]
fn the_envelope_is_adsrs_sound() {
    let (a, d, s, r): (f64, f64, f64, f64) = (0.05, 0.3, 0.4, 0.45);
    // The hold is a quarter of the attack and decay, 50 ms at least (CARD-96).
    let hold: f64 = (0.25 * (a + d)).max(0.05);
    let total = a + d + hold + r;
    let v = json!({"attack": a, "decay": d, "sustain": s, "release": r});
    let env = line(&drawn(vec![("std".into(), std_dir())], "std:adsr", "shape", v), "envelope");
    // The gate: a square LFO on for its first half, as long as the attack, decay and hold together
    // (120 bpm: a beat is 0.5 s).
    let period = 2.0 * (a + d + hold) * 2.0;
    let out = render(
        &format!(
            "export default song(() => vca({STEADY}, adsr(lfo(time.beats, {{ period: {period}, shape: 'square' }}), {{ attack: {a}, decay: {d}, sustain: {s}, release: {r} }})));\n"
        ),
        total + 0.05,
    );
    assert!(env.last().is_some_and(|p| (p.0 - total).abs() < 1e-9), "it ends where the release does: {:?}", env.last());
    let mut checked = 0;
    // Its x is in seconds (CARD-96).
    for &(x, y) in &env {
        let i = (x * RATE).round() as usize;
        if y < 0.01 || i < SETTLED || i >= out.len() {
            continue;
        }
        let heard = out[i];
        assert!((db(heard) - db(y)).abs() <= 0.5, "at {x:.4} s the envelope says {y:.4}, adsr plays {heard:.4}");
        checked += 1;
    }
    assert!(checked > 100, "{checked} points checked");
}

// CARD-99: the LFO's shape over a bar is what std's lfo plays at each beat, within 0.5 dB where it
// is above −40 dB, for each of its smooth shapes and with a phase.
#[test]
fn the_lfo_shape_is_its_sound() {
    for (shape, period, phase) in [("sine", 1.5, 0.0), ("tri", 0.75, 0.25), ("saw", 2.0, 0.1)] {
        let v = json!({"period": period, "phase": phase, "shape": shape});
        let wave = line(&drawn(vec![("std".into(), std_dir())], "std:lfo", "wave", v), "shape");
        let out = render(
            &format!(
                "export default song(() => vca({STEADY}, lfo(time.beats, {{ period: {period}, phase: {phase}, shape: '{shape}' }})));\n"
            ),
            2.0,
        );
        let mut checked = 0;
        for &(beat, y) in &wave {
            let i = (beat * 0.5 * RATE).round() as usize;
            // A saw's fall is a jump: the points next to it are a sample apart in time.
            let near_jump = shape == "saw" && ((beat / period + phase).fract() < 0.01 || (beat / period + phase).fract() > 0.99);
            if y < 0.01 || i < SETTLED || i >= out.len() || near_jump {
                continue;
            }
            assert!((db(out[i]) - db(y)).abs() <= 0.5, "{shape}: at beat {beat:.3} the picture says {y:.4}, lfo plays {:.4}", out[i]);
            checked += 1;
        }
        assert!(checked > 100, "{shape}: {checked} points checked");
    }
}

/// A test pack whose card draws std/pictures' lowpass from its maths, over std's own lowpass.
const DRAWN_LOWPASS: &str = r#"import { component, control, port } from "@m2/core";
import { lowpass } from "std/filter";
import { curvePicture, lowpass as drawLowpass, lowpassWords } from "std/pictures";

export const drawn = component(
  {
    id: "drawn",
    label: "Drawn",
    inputs: { x: port.audio() },
    controls: { cutoff: control.hz(1000).range(20, 20000).log(), q: control.scalar(0.707).range(0.3, 20).log() },
    output: port.audio(),
    editor: { views: { curve: curvePicture({ live: false }) } },
  },
  (x, { cutoff, q }) => lowpass(x, { cutoff, q }),
  { curve: { draw: (v, p) => drawLowpass(p, v, { cutoff: "cutoff", q: "q" }), words: (v) => lowpassWords(v, { cutoff: "cutoff", q: "q" }) } },
);
"#;

// CARD-30, CARD-99: std/pictures' lowpass, drawn from its maths, is std's state-variable lowpass as
// its sound measures it, within 0.5 dB wherever it is above −60 dB up to a twelfth of the rate
// (4 kHz at 48 kHz), where a filter made digital still follows its analog curve.
#[test]
fn the_drawn_lowpass_is_stds_filter_up_to_a_twelfth_of_the_rate() {
    let pack = std::env::temp_dir().join(format!("m2-editor-drawn-lowpass-{}", std::process::id()));
    std::fs::create_dir_all(pack.join("src")).unwrap();
    std::fs::write(pack.join("src/index.ts"), DRAWN_LOWPASS).unwrap();
    for (cutoff, q) in [(200.0, 0.707), (800.0, 4.0), (300.0, 10.0)] {
        let pic = drawn(
            vec![("@test/drawn".into(), pack.clone()), ("std".into(), std_dir())],
            "@test/drawn:drawn",
            "curve",
            json!({"cutoff": cutoff, "q": q}),
        );
        let drawn = line(&pic, "curve");
        let f = Fixture::new();
        f.write("music2.toml", MANIFEST);
        f.write("song.ts", format!("import {{ song }} from '@m2/core';\nimport {{ lowpass }} from 'std/filter';\nimport {{ osc }} from 'std/dsp';\nimport {{ track }} from '@m2/core';\n\
             const t = track('t', () => lowpass(osc.saw(110), {{ cutoff: {cutoff}, q: {q} }}));\nexport default song(() => t.out);\n"));
        let s = f.open();
        let rev = s.accepted();
        let site = rev.compiled.sites.iter().find(|x| x.file == "song.ts" && x.callee == "lowpass").unwrap();
        let node = format!("{}#{}", site.file, site.ordinal);
        let c = copy(rev, &s.packages, "t", &node, &Map::new(), Layer::Response).unwrap();
        let span = Span { log: true, lo: 20.0, hi: 20_000.0 };
        let measured = c.measure(&s.packages, span, Duration::from_secs(5), Pass::Rest).unwrap();
        // Between the two measured points either side, on the log scale the points are spaced on.
        let heard = |hz: f64| -> f64 {
            let at = measured.chunks(2).position(|p| p[0] >= hz).unwrap();
            let (lo, hi) = (&measured[2 * at.saturating_sub(1)..], &measured[2 * at..]);
            if at == 0 || hi[0] == hz {
                return hi[1];
            }
            let t = (hz / lo[0]).ln() / (hi[0] / lo[0]).ln();
            lo[1] + t * (hi[1] - lo[1])
        };
        let mut checked = 0;
        for &(hz, y) in drawn.iter().filter(|(hz, y)| *hz <= RATE / 12.0 && *y > -60.0) {
            let m = heard(hz);
            assert!((m - y).abs() <= 0.5, "cutoff {cutoff}, q {q}: at {hz:.0} Hz the drawing says {y:.2} dB, the filter {m:.2} dB");
            checked += 1;
        }
        assert!(checked > 50, "{checked} points checked");
        assert!(pic["words"].as_str().unwrap().starts_with("Cuts the highs above"), "{}", pic["words"]);
    }
    let _ = std::fs::remove_dir_all(&pack);
}
