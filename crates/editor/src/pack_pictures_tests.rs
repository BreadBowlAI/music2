//! `@m2/mixing`'s and `@m2/effects`' pictures drawn from their own sums are the sound (CARD-125,
//! D13): each against a render of the effect it draws, within 0.5 dB. The transient shaper's hit,
//! Pump's dip, the multiband's bands, the delay's echoes, the reverb's tail, the chorus's, phaser's
//! and tremolo's motion, drive's curve, the bitcrusher's sine and the widener's width.

use std::path::{Path, PathBuf};

use m2_runtime::Renderer;
use m2_script::pictures::{Ask, Drawn, Runner};
use serde_json::{Value, json};

use crate::testing::Fixture;

const RATE: f64 = 48_000.0;

/// The packs these songs read.
const MANIFEST: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.10\"\n\
                        \"@m2/effects\" = \"1\"\n\"@m2/mixing\" = \"1\"\n";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Picture `view` of `@m2/<pack>`'s `component` at `values`: its shapes and words, as the runner draws them.
fn drawn(pack: &str, component: &str, view: &str, values: Value) -> Value {
    let packages = vec![(format!("@m2/{pack}"), repo().join("packs/@m2").join(pack)), ("std".to_string(), repo().join("std"))];
    let catalog = m2_script::describe(&packages);
    let cid = format!("@m2/{pack}:{component}");
    let descriptor = catalog.components.get(&cid).cloned().unwrap_or_else(|| panic!("no {cid}: {:?}", catalog.problems));
    let ask = Ask {
        key: pack.into(),
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

/// The points of the shape named `name` a picture drew, as pairs.
fn line(drawn: &Value, name: &str) -> Vec<(f64, f64)> {
    let s = drawn["shapes"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("no line {name}: {drawn}"));
    pairs(&s["points"])
}

fn pairs(points: &Value) -> Vec<(f64, f64)> {
    points.as_array().unwrap().chunks(2).map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect()
}

/// The dots a picture drew: where each is, and what it opens.
fn dots(drawn: &Value) -> Vec<(f64, f64, Option<String>)> {
    drawn["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["kind"] == "handle")
        .map(|h| (h["x"].as_f64().unwrap(), h["y"].as_f64().unwrap(), h["opens"].as_str().map(String::from)))
        .collect()
}

/// `seconds` of the song `body` (after the imports) at 48 kHz, as it plays: both sides.
fn render(body: &str, seconds: f64) -> Vec<[f64; 2]> {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        format!(
            "import {{ db, song, time }} from '@m2/core';\nimport {{ adsr, gain, osc, pan, vca }} from 'std/dsp';\nimport {{ lfo }} from 'std/control';\n\
             import {{ multiband, pump, transient }} from '@m2/mixing';\n\
             import {{ bitcrush, chorus, delay, drive, phaser, reverb, tremolo, widen }} from '@m2/effects';\n{body}"
        ),
    );
    let s = f.open();
    let rev = s.accepted();
    let mut r = Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, rev.project.sample_rate()).unwrap();
    let mut out = vec![[0.0f32; 2]; (seconds * RATE) as usize];
    r.process(&mut out).unwrap();
    out.iter().map(|x| [x[0] as f64, x[1] as f64]).collect()
}

/// The left side of a render.
fn left(body: &str, seconds: f64) -> Vec<f64> {
    render(body, seconds).iter().map(|x| x[0]).collect()
}

/// A steady sound of 1 for 500 s: a square wave at 0.001 Hz, after its first edge.
const STEADY: &str = "osc.square(0.001)";
/// The first sample a square's first edge leaves alone: it is smoothed over a few samples.
const SETTLED: usize = 16;

/// A level in dB.
fn db(x: f64) -> f64 {
    20.0 * x.abs().max(1e-12).log10()
}

/// The loudest sample of `x` within `half` samples either side of `i`: a tone's level there.
fn level_at(x: &[f64], i: usize, half: usize) -> f64 {
    x[i.saturating_sub(half)..(i + half).min(x.len())].iter().fold(0.0, |m, v| m.max(v.abs()))
}

// CARD-113, CARD-125: the transient shaper's hit, drawn from the kernel's own sums, is what it plays:
// std's adsr's hit (1 ms up, 60 dB down in 0.3 s) through the transient shaper comes out where the
// line `after` says, within 0.5 dB, wherever it is above −40 dB.
#[test]
fn the_transient_shapers_hit_is_its_sound() {
    for (attack, sustain) in [(12.0, 0.0), (0.0, -12.0), (-9.0, 9.0), (24.0, -24.0)] {
        let after = line(&drawn("mixing", "transient", "hit", json!({"attack": attack, "sustain": sustain})), "after");
        let out = left(
            &format!(
                "export default song(() => transient(vca({STEADY}, adsr(lfo(time.beats, {{ period: 4, shape: 'square' }}), \
                 {{ attack: 0.001, decay: 0.3, sustain: 0, release: 0.3 }})), {{ attack: {attack}, sustain: {sustain} }}));\n"
            ),
            0.45,
        );
        let mut checked = 0;
        for &(t, y) in &after {
            let i = (t * RATE).round() as usize;
            if y < -40.0 || i < SETTLED {
                continue;
            }
            assert!(
                (db(out[i]) - y).abs() <= 0.5,
                "attack {attack}, sustain {sustain}: at {t:.3} s the picture says {y:.2} dB, it plays {:.2} dB",
                db(out[i])
            );
            checked += 1;
        }
        // A point each millisecond, to where it is 40 dB down: 80 ms at the least, its tail 24 dB down.
        assert!(checked > 70, "{checked} points checked");
    }
}

// CARD-116, CARD-125: Pump's dip, drawn from its own sum, is what it plays: a steady sound through
// it, at 120 bpm, is where the line `dip` says over the bar, within 0.5 dB, away from each step's
// jump back down (its duck glides 1 ms there).
#[test]
fn pumps_dip_is_its_sound() {
    for (depth, shape, rate) in [(-8.0, 0.5, "1/4"), (-18.0, 1.0, "1/4"), (-12.0, 0.0, "1/8"), (-6.0, 0.3, "1/16"), (-24.0, 1.0, "1/2")] {
        let dip = line(&drawn("mixing", "pump", "dip", json!({"depth": depth, "shape": shape, "rate": rate})), "dip");
        let out = left(&format!("export default song(() => pump({STEADY}, {{ depth: {depth}, shape: {shape}, rate: '{rate}' }}));\n"), 2.0);
        let step = match rate {
            "1/2" => 2.0,
            "1/4" => 1.0,
            "1/8" => 0.5,
            _ => 0.25,
        };
        let mut checked = 0;
        for &(beat, y) in &dip {
            // A beat is half a second at 120 bpm; the jump at each step's start is skipped by 5 ms.
            let i = (beat * 0.5 * RATE).round() as usize;
            let into = beat % step;
            if i < SETTLED || i >= out.len() || into * 0.5 < 0.005 || (step - into) * 0.5 < 1e-6 {
                continue;
            }
            assert!(
                (db(out[i]) - y).abs() <= 0.5,
                "depth {depth}, shape {shape}, every {rate}: at beat {beat:.3} the picture says {y:.2} dB, Pump plays {:.2} dB",
                db(out[i])
            );
            checked += 1;
        }
        // 48 points each step: two steps of a half note, the fewest.
        assert!(checked > 90, "{checked} points checked");
    }
}

// CARD-115, CARD-125: the multiband's split, drawn from its crossovers' own sums, is what it plays
// below its thresholds: with its compressors off (ratio 1), a tone at each of a dozen of the line's
// points, the crossovers and the bands' dots among them, comes out turned as far as the line `split`
// says, within 0.5 dB.
#[test]
fn the_multiband_split_is_its_sound() {
    // Two crossovers an octave apart too, where the low band's all-pass matters.
    for (gains, (lm, mh)) in
        [([6.0, -6.0, 3.0], (200.0f64, 2500.0f64)), ([-9.0, 4.0, -3.0], (120.0, 4000.0)), ([9.0, -9.0, 0.0], (400.0, 800.0))]
    {
        let mut v = json!({"lowMid": lm, "midHigh": mh, "lowGain": gains[0], "midGain": gains[1], "highGain": gains[2]});
        for band in ["low", "mid", "high"] {
            for (k, x) in [("Threshold", -18.0), ("Ratio", 1.0), ("Attack", 0.01), ("Release", 0.1)] {
                v[format!("{band}{k}")] = json!(x);
            }
        }
        let d = drawn("mixing", "multiband", "split", v);
        let split = line(&d, "split");
        // log-scale interpolation along the line, at any frequency.
        let on = |hz: f64| -> f64 {
            let i = split.iter().position(|p| p.0 >= hz).unwrap().max(1);
            let ((f0, y0), (f1, y1)) = (split[i - 1], split[i]);
            y0 + (y1 - y0) * (hz / f0).ln() / (f1 / f0).ln()
        };
        let mut tones: Vec<f64> = split.iter().step_by(24).map(|p| p.0).collect();
        tones.extend([lm, mh, (lm * mh).sqrt()]);
        tones.extend(dots(&d).into_iter().filter(|d| d.2.is_some()).map(|d| d.0));
        for hz in tones.into_iter().filter(|hz| *hz < 16_000.0) {
            let period = (RATE / hz).ceil() as usize;
            let body = format!(
                "export default song(() => multiband(osc.sine({hz}), {{ lowMid: {lm}, midHigh: {mh}, lowGain: {}, midGain: {}, highGain: {}, lowRatio: 1, midRatio: 1, highRatio: 1 }}));\n",
                gains[0], gains[1], gains[2]
            );
            let out = left(&body, 0.5);
            let heard = db(level_at(&out, out.len() - period, period));
            let y = on(hz);
            assert!(
                (heard - y).abs() <= 0.5,
                "gains {gains:?}: a tone at {hz:.0} Hz, the line says {y:.2} dB, the multiband plays {heard:.2} dB"
            );
        }
    }
}

// CARD-117, CARD-125: the delay's echoes, drawn from its own sum, are what it plays: a step through
// it, its first difference a click, gives the click and each echo where the bars say, within 0.5 dB.
#[test]
fn the_delays_echoes_are_its_sound() {
    for (time, fb, mix) in [(0.25, 0.3, 0.3), (0.1, 0.7, 0.5), (0.5, 0.5, 1.0)] {
        let d = drawn("effects", "delay", "echoes", json!({"time": time, "feedback": fb, "mix": mix}));
        let shapes = d["shapes"].as_array().unwrap();
        let click = pairs(&shapes[0]["points"]);
        let echoes = pairs(&shapes[1]["points"]);
        let seconds = echoes.last().unwrap().0 + 0.05;
        let out = left(&format!("export default song(() => delay({STEADY}, {{ time: {time}, feedback: {fb}, mix: {mix} }}));\n"), seconds);
        // Each sound of the click: its first difference summed round where it comes.
        let heard = |t: f64| -> f64 {
            let i = (t * RATE).round() as usize;
            (i.saturating_sub(4).max(1)..(i + 24).min(out.len())).map(|n| out[n] - out[n - 1]).sum::<f64>()
                + if i == 0 { out[0] } else { 0.0 }
        };
        let mut checked = 0;
        for &(t, y) in click.iter().chain(&echoes) {
            if y <= -60.0 {
                continue;
            }
            assert!(
                (db(heard(t)) - y).abs() <= 0.5,
                "time {time}, feedback {fb}, mix {mix}: at {t:.3} s the bars say {y:.2} dB, the delay plays {:.2} dB",
                db(heard(t))
            );
            checked += 1;
        }
        assert!(checked >= 3, "{checked} echoes checked");
    }
}

// CARD-118, CARD-125: the reverb's tail, drawn from its own sums, is what it plays: how loud what is
// left of a click's tail is (its energy from then on, Schroeder's backward sum), where the line
// `tail` says, within 0.5 dB, down to 30 dB below its start.
#[test]
fn the_reverbs_tail_is_its_sound() {
    for (size, damping) in [(0.5, 0.0), (0.8, 0.5), (0.3, 1.0), (1.0, 0.2)] {
        let tail = line(&drawn("effects", "reverb", "tail", json!({"size": size, "damping": damping, "mix": 1})), "tail");
        let seconds = tail.last().unwrap().0 + 0.5;
        let out = left(&format!("export default song(() => reverb({STEADY}, {{ size: {size}, damping: {damping}, mix: 1 }}));\n"), seconds);
        // The click's tail, and what is left of it from each sample on.
        let click: Vec<f64> = (0..out.len()).map(|n| if n == 0 { out[0] } else { out[n] - out[n - 1] }).collect();
        let mut left = vec![0.0; click.len() + 1];
        for n in (0..click.len()).rev() {
            left[n] = left[n + 1] + click[n] * click[n];
        }
        let mut checked = 0;
        for &(t, y) in &tail {
            if y < -30.0 {
                continue;
            }
            let heard = 10.0 * (left[(t * RATE).round() as usize] / left[0]).log10();
            assert!(
                (heard - y).abs() <= 0.5,
                "size {size}, damping {damping}: at {t:.3} s the tail says {y:.2} dB, the reverb plays {heard:.2} dB"
            );
            checked += 1;
        }
        assert!(checked > 20, "{checked} points checked");
    }
}

// CARD-119, CARD-125: the chorus's motion, drawn from its own sum, is what it plays: a 100 Hz tone
// through it, only the copies, is as loud as two copies that far apart make it,
// |cos(π · 100 · (one − two))|, within 0.5 dB, wherever that is above −30 dB.
#[test]
fn the_chorus_motion_is_its_sound() {
    for (rate, depth) in [(0.8, 1.0), (2.0, 0.6)] {
        let d = drawn("effects", "chorus", "motion", json!({"rate": rate, "depth": depth, "mix": 1}));
        let (one, two) = (line(&d, "one"), line(&d, "two"));
        let out = left(&format!("export default song(() => chorus(osc.sine(100), {{ rate: {rate}, depth: {depth}, mix: 1 }}));\n"), 1.0);
        let mut checked = 0;
        for (&(t, a), &(_, b)) in one.iter().zip(&two) {
            let i = (t * RATE).round() as usize;
            let want = db((std::f64::consts::PI * 100.0 * (a - b)).cos());
            if t < 0.03 || i + 240 >= out.len() || want < -30.0 {
                continue;
            }
            // The two copies sum to one tone, as late as their middle, which stays where it is, its
            // level moving: that level here, fitted over the period round this point as a level
            // that moves in a straight line (so its move within the period is read at its middle).
            let late = (a + b) / 2.0;
            let (mut s2, mut us2, mut uus2, mut xs, mut uxs) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for (n, x) in out.iter().enumerate().take(i + 240).skip(i - 240) {
                let (u, s) = (n as f64 - i as f64, (2.0 * std::f64::consts::PI * 100.0 * (n as f64 / RATE - late)).sin());
                (s2, us2, uus2, xs, uxs) = (s2 + s * s, us2 + u * s * s, uus2 + u * u * s * s, xs + x * s, uxs + u * x * s);
            }
            let heard = db((xs * uus2 - uxs * us2) / (s2 * uus2 - us2 * us2));
            assert!(
                (heard - want).abs() <= 0.5,
                "rate {rate}, depth {depth}: at {t:.3} s the copies {:.2} ms apart make {want:.2} dB, the chorus plays {heard:.2} dB",
                (a - b) * 1000.0
            );
            checked += 1;
        }
        assert!(checked > 100, "{checked} points checked");
    }
}

// CARD-120, CARD-125: the phaser's motion, drawn from its own sum, is what it plays: a 1 kHz tone
// through it (feedback 0, mix 0.5) is as loud as its six stages, at the middle notch the picture
// draws, make it, |cos(6 · atan(tan(π · 1000 / rate) / tan(π · mid / rate)))|, within 0.5 dB,
// wherever that is above −10 dB. Deeper in a notch the sound lags the drawing by its stages' delay,
// about a millisecond, which moves a level near −20 dB more than 0.5 dB as the notch sweeps by.
#[test]
fn the_phaser_motion_is_its_sound() {
    for (rate, depth) in [(1.0, 0.7), (3.0, 0.4)] {
        let mid = line(&drawn("effects", "phaser", "motion", json!({"rate": rate, "depth": depth, "feedback": 0, "mix": 0.5})), "mid");
        let out = left(
            &format!("export default song(() => phaser(osc.sine(1000), {{ rate: {rate}, depth: {depth}, feedback: 0, mix: 0.5 }}));\n"),
            1.0,
        );
        let w = (std::f64::consts::PI * 1000.0 / RATE).tan();
        let mut checked = 0;
        for &(t, f) in &mid {
            let i = (t * RATE).round() as usize;
            let want = db((6.0 * (w / (std::f64::consts::PI * f / RATE).tan()).atan()).cos());
            if t < 0.02 || i + 48 >= out.len() || want < -10.0 {
                continue;
            }
            // Its level now: the sample and the one a quarter of a cycle on, a tone's two sides.
            let heard = db((out[i] * out[i] + out[i + 12] * out[i + 12]).sqrt());
            assert!(
                (heard - want).abs() <= 0.5,
                "rate {rate}, depth {depth}: at {t:.3} s its middle notch at {f:.0} Hz makes {want:.2} dB, the phaser plays {heard:.2} dB"
            );
            checked += 1;
        }
        assert!(checked > 100, "{checked} points checked");
    }
}

// CARD-121, CARD-125: the tremolo's motion, drawn from its own sum, is what it plays: a steady sound
// through it is as loud as the line `level` says, within 0.5 dB, wherever that is above −40 dB.
#[test]
fn the_tremolo_motion_is_its_sound() {
    for (rate, depth) in [(5.0, 0.5), (2.0, 1.0), (13.0, 0.8)] {
        let level = line(&drawn("effects", "tremolo", "motion", json!({"rate": rate, "depth": depth})), "level");
        let out = left(&format!("export default song(() => tremolo({STEADY}, {{ rate: {rate}, depth: {depth} }}));\n"), 1.0);
        let mut checked = 0;
        for &(t, y) in &level {
            let i = (t * RATE).round() as usize;
            if y < 0.01 || i < SETTLED || i >= out.len() {
                continue;
            }
            assert!(
                (db(out[i]) - db(y)).abs() <= 0.5,
                "rate {rate}, depth {depth}: at {t:.3} s the picture says {y:.4}, the tremolo plays {:.4}",
                out[i]
            );
            checked += 1;
        }
        assert!(checked > 100, "{checked} points checked");
    }
}

// CARD-122, CARD-125: drive's curve, drawn from its own sum, is what it plays: a steady level in
// comes out where the curve says, within 0.5 dB.
#[test]
fn drives_curve_is_its_sound() {
    for amount in [0.0, 3.0, 12.0, 36.0] {
        let curve = line(&drawn("effects", "drive", "curve", json!({"amount": amount})), "curve");
        for level in [0.05, 0.1, 0.25, 0.5, 0.9, 1.0] {
            let &(x, y) = curve.iter().min_by(|a, b| (a.0 - level).abs().total_cmp(&(b.0 - level).abs())).unwrap();
            assert!((x - level).abs() < 1e-9, "the curve has a point at {level}");
            let out =
                left(&format!("export default song(() => drive(gain({STEADY}, db({})), {{ amount: {amount} }}));\n", db(level)), 0.01);
            let heard = out[out.len() - 1];
            assert!((db(heard) - db(y)).abs() <= 0.5, "amount {amount}: {level} in, the curve says {y:.4}, drive plays {heard:.4}");
        }
    }
}

// CARD-123, CARD-125: the bitcrusher's sine, drawn from its own sum, is what it plays: a 1 kHz sine
// through it, sample by sample over its first cycle, where the line `crushed` says, within 0.5 dB
// and on the same side of 0, wherever it is above −40 dB.
#[test]
fn the_bitcrushers_sine_is_its_sound() {
    for (bits, rate, mix) in [(4.0, 12000.0, 1.0), (8.0, 6000.0, 1.0), (3.0, 3000.0, 0.5), (2.0, 48000.0, 1.0)] {
        let held = line(&drawn("effects", "bitcrush", "crush", json!({"bits": bits, "rate": rate, "mix": mix})), "crushed");
        let out =
            left(&format!("export default song(() => bitcrush(osc.sine(1000), {{ bits: {bits}, rate: {rate}, mix: {mix} }}));\n"), 0.002);
        let mut checked = 0;
        // Each sample's value is drawn twice, at its start and its end: its start.
        for &(t, y) in held.iter().step_by(2) {
            let n = (t * RATE).round() as usize;
            if y.abs() < 0.01 {
                continue;
            }
            assert!(
                y.signum() == out[n].signum() && (db(out[n]) - db(y)).abs() <= 0.5,
                "{bits} bits at {rate} Hz, mix {mix}: sample {n} is {y:.4} in the picture, {:.4} as it plays",
                out[n]
            );
            checked += 1;
        }
        assert!(checked > 30, "{checked} samples checked");
    }
}

// CARD-124, CARD-125: the widener's field, drawn from its own sum, is what it plays: a sound panned
// hard left (its sides as loud as its middle) comes out with its sides turned as far against its
// middle as the field's edge says, within 0.5 dB.
#[test]
fn the_wideners_field_is_its_sound() {
    for width in [0.25, 0.5, 1.0, 1.5, 2.0] {
        let edge = dots(&drawn("effects", "widen", "field", json!({"width": width})))[0].0;
        let out = render(&format!("export default song(() => widen(pan({STEADY}, -1), {{ width: {width} }}));\n"), 0.2);
        let [l, r] = out[out.len() - 1];
        let (mid, side) = ((l + r) / 2.0, (l - r) / 2.0);
        assert!(
            (db(side / mid) - db(edge)).abs() <= 0.5,
            "width {width}: the field's edge at {edge}, the sides {:.2} dB against the middle",
            db(side / mid)
        );
    }
}
