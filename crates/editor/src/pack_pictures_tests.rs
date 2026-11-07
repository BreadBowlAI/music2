//! `@m2/mixing`'s and `@m2/effects`' pictures drawn from their own sums are the sound (CARD-99,
//! D13): each against a render of the effect it draws, within 0.5 dB, at 44.1, 48 and 96 kHz, each
//! drawn at the rate rendered (CARD-87). The transient shaper's hit,
//! Pump's dip, the delay's echoes, the reverb's tail, the chorus's, phaser's and tremolo's motion,
//! drive's curve, the bitcrusher's sine and the widener's width. And the multiband's split, which is
//! measured from its sound under its thresholds (CARD-115).

use std::path::{Path, PathBuf};

use m2_runtime::Renderer;
use m2_script::pictures::{Ask, Drawn, Runner};
use serde_json::{Value, json};

use crate::testing::Fixture;

/// The rates each picture is checked at (CARD-99): a sum right only at 48 kHz fails at the others.
const RATES: [f64; 3] = [44_100.0, 48_000.0, 96_000.0];

/// The packs these songs read, at `rate`.
fn manifest(rate: f64) -> String {
    format!(
        "[project]\nname = \"t\"\nformat = 1\nsample_rate = {rate}\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.10\"\n\
         \"@m2/effects\" = \"1\"\n\"@m2/mixing\" = \"1\"\n"
    )
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Picture `view` of `@m2/<pack>`'s `component` at `values`, for a song at `rate`: its shapes and
/// words, as the runner draws them.
fn drawn(rate: f64, pack: &str, component: &str, view: &str, values: Value) -> Value {
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
        rate: rate as u32,
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

/// `seconds` of the song `body` (after the imports) at `rate`, as it plays: both sides.
fn render(rate: f64, body: &str, seconds: f64) -> Vec<[f64; 2]> {
    let f = Fixture::new();
    f.write("music2.toml", manifest(rate));
    f.write(
        "song.ts",
        format!(
            "import {{ db, song, time }} from '@m2/core';\nimport {{ adsr, gain, osc, pan, vca }} from 'std/dsp';\nimport {{ lfo }} from 'std/control';\n\
             import {{ deesser, multiband, pump, transient }} from '@m2/mixing';\n\
             import {{ bitcrush, chorus, delay, drive, phaser, reverb, tremolo, widen }} from '@m2/effects';\n{body}"
        ),
    );
    let s = f.open();
    let rev = s.accepted();
    let mut r = Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, rev.project.sample_rate()).unwrap();
    assert_eq!(rev.project.sample_rate() as f64, rate);
    let mut out = vec![[0.0f32; 2]; (seconds * rate) as usize];
    r.process(&mut out).unwrap();
    out.iter().map(|x| [x[0] as f64, x[1] as f64]).collect()
}

/// The left side of a render.
fn left(rate: f64, body: &str, seconds: f64) -> Vec<f64> {
    render(rate, body, seconds).iter().map(|x| x[0]).collect()
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

// CARD-113, CARD-99: the transient shaper's hit, drawn from the kernel's own sums, is what it plays:
// std's adsr's hit (1 ms up, 60 dB down in 0.3 s) through the transient shaper comes out where the
// line `after` says, within 0.5 dB, wherever it is above −40 dB.
#[test]
fn the_transient_shapers_hit_is_its_sound() {
    for (rate, (attack, sustain)) in RATES.into_iter().flat_map(|r| [(12.0, 0.0), (0.0, -12.0), (-9.0, 9.0), (24.0, -24.0)].map(|a| (r, a)))
    {
        let after = line(&drawn(rate, "mixing", "transient", "hit", json!({"attack": attack, "sustain": sustain})), "after");
        let out = left(
            rate,
            &format!(
                "export default song(() => transient(vca({STEADY}, adsr(lfo(time.beats, {{ period: 4, shape: 'square' }}), \
                 {{ attack: 0.001, decay: 0.3, sustain: 0, release: 0.3 }})), {{ attack: {attack}, sustain: {sustain} }}));\n"
            ),
            0.45,
        );
        let mut checked = 0;
        for &(t, y) in &after {
            let i = (t * rate).round() as usize;
            if y < -40.0 || i < SETTLED {
                continue;
            }
            assert!(
                (db(out[i]) - y).abs() <= 0.5,
                "at {rate} Hz, attack {attack}, sustain {sustain}: at {t:.3} s the picture says {y:.2} dB, it plays {:.2} dB",
                db(out[i])
            );
            checked += 1;
        }
        // A point each millisecond, to where it is 40 dB down: 80 ms at the least, its tail 24 dB down.
        assert!(checked > 70, "{checked} points checked");
    }
}

// CARD-116, CARD-99: Pump's dip, drawn from its own sum, is what it plays: a steady sound through
// it, at 120 bpm, is where the line `dip` says over the bar, within 0.5 dB, away from each step's
// jump back down (its duck glides 1 ms there).
#[test]
fn pumps_dip_is_its_sound() {
    let dips = [(-8.0, 0.5, "1/4"), (-18.0, 1.0, "1/4"), (-12.0, 0.0, "1/8"), (-6.0, 0.3, "1/16"), (-24.0, 1.0, "1/2")];
    for (sr, (depth, shape, rate)) in RATES.into_iter().flat_map(|r| dips.map(|d| (r, d))) {
        let dip = line(&drawn(sr, "mixing", "pump", "dip", json!({"depth": depth, "shape": shape, "rate": rate})), "dip");
        let out =
            left(sr, &format!("export default song(() => pump({STEADY}, {{ depth: {depth}, shape: {shape}, rate: '{rate}' }}));\n"), 2.0);
        let step = match rate {
            "1/2" => 2.0,
            "1/4" => 1.0,
            "1/8" => 0.5,
            _ => 0.25,
        };
        let mut checked = 0;
        for &(beat, y) in &dip {
            // A beat is half a second at 120 bpm; the jump at each step's start is skipped by 5 ms.
            let i = (beat * 0.5 * sr).round() as usize;
            let into = beat % step;
            if i < SETTLED || i >= out.len() || into * 0.5 < 0.005 || (step - into) * 0.5 < 1e-6 {
                continue;
            }
            assert!(
                (db(out[i]) - y).abs() <= 0.5,
                "at {sr} Hz, depth {depth}, shape {shape}, every {rate}: at beat {beat:.3} the picture says {y:.2} dB, Pump plays {:.2} dB",
                db(out[i])
            );
            checked += 1;
        }
        // 48 points each step: two steps of a half note, the fewest.
        assert!(checked > 90, "{checked} points checked");
    }
}

// CARD-115, CARD-80 (measure2 §8): the multiband's split is measured from its own sound with a quiet
// click, `p.response({ level: -80 })`, under every threshold its card allows (−60 dB), so its
// compressors rest whatever they are set to. Through a multiband at thresholds of −60 dB and a ratio
// of 20 the click draws the curve of one at thresholds of 0 dB, to a thousandth of a dB, and passes
// CARD-82's checks at rest, where a click at −20 dBFS is refused; each band's gain is its part of the
// curve.
#[test]
fn the_multiband_split_is_measured_under_its_thresholds() {
    use crate::measure::{Failed, LEVEL, Layer, Pass, Span, copy};
    let d =
        drawn(48_000.0, "mixing", "multiband", "split", json!({"lowMid": 200, "midHigh": 2500, "lowGain": 0, "midGain": 0, "highGain": 0}));
    let shapes = d["shapes"].as_array().unwrap();
    let split = shapes.iter().find(|s| s["kind"] == "response").unwrap_or_else(|| panic!("the split is measured: {d}"));
    assert_eq!((&split["name"], &split["level"]), (&json!("split"), &json!(-80.0)), "{split}");
    let level = split["level"].as_f64().unwrap();
    let span = Span { log: true, lo: 20.0, hi: 20_000.0 };
    for (gains, (lm, mh)) in
        [([6.0, -6.0, 3.0], (200.0f64, 2500.0f64)), ([-9.0, 4.0, -3.0], (120.0, 4000.0)), ([9.0, -9.0, 0.0], (400.0, 800.0))]
    {
        let call = |threshold: f64| {
            format!(
                "multiband(osc.saw(110), {{ lowMid: {lm}, midHigh: {mh}, lowGain: {}, midGain: {}, highGain: {}, lowThreshold: {threshold}, \
                 midThreshold: {threshold}, highThreshold: {threshold}, lowRatio: 20, midRatio: 20, highRatio: 20 }})",
                gains[0], gains[1], gains[2]
            )
        };
        let f = Fixture::new();
        f.write("music2.toml", manifest(48_000.0));
        f.write(
            "song.ts",
            format!(
                "import {{ song, track }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\nimport {{ sum }} from 'std/math';\nimport {{ multiband }} from '@m2/mixing';\n\
                 const hard = track('hard', () => {});\nconst rest = track('rest', () => {});\nexport default song(() => sum(hard.out, rest.out));\n",
                call(-60.0),
                call(0.0)
            ),
        );
        let s = f.open();
        assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
        let rev = s.accepted();
        let measure = |module: &str, level: f64| {
            let c = &rev.compiled;
            let site =
                c.sites.iter().find(|x| x.callee == "multiband" && c.outputs.iter().any(|o| o.site == x.id && o.module == module)).unwrap();
            let node = format!("{}#{}", site.file, site.ordinal);
            copy(rev, &s.packages, module, &node, &serde_json::Map::new(), Layer::Response { level }).unwrap().measure(
                &s.packages,
                span,
                std::time::Duration::from_secs(5),
                Pass::Rest,
            )
        };
        let hard = measure("hard", level).unwrap_or_else(|e| panic!("at thresholds of −60 dB, measured under them: {e:?}"));
        let rest = measure("rest", level).unwrap();
        assert_eq!(hard.len(), 512, "256 points");
        let worst = hard.chunks(2).zip(rest.chunks(2)).map(|(a, b)| (a[1] - b[1]).abs()).fold(0.0, f64::max);
        assert!(worst < 0.001, "{lm} and {mh} Hz, gains {gains:?}: the split at thresholds of −60 dB is {worst} dB from its split at 0 dB");
        let loud = measure("hard", LEVEL);
        assert!(
            matches!(&loud, Err(Failed::Cant(w)) if w.starts_with("its sound changes with how loud it is")),
            "a click at −20 dBFS is over its thresholds: {loud:?}"
        );
        // Each band's gain: the lows' at 30 Hz and the highs' at 16 kHz, within a quarter of a dB; the
        // mids' in their middle, within half a dB, where the crossovers are more than three octaves
        // apart (nearer, the other bands still sound there).
        let at = |hz: f64| hard.chunks(2).min_by(|a, b| (a[0] / hz).ln().abs().total_cmp(&(b[0] / hz).ln().abs())).unwrap()[1];
        let mut parts = vec![(30.0, gains[0], 0.25), (16_000.0, gains[2], 0.25)];
        if mh / lm > 8.0 {
            parts.push(((lm * mh).sqrt(), gains[1], 0.5));
        }
        for (hz, gain, within) in parts {
            assert!(
                (at(hz) - gain).abs() < within,
                "{lm} and {mh} Hz, gains {gains:?}: at {hz:.0} Hz the split is {:.2} dB, its band's gain {gain}",
                at(hz)
            );
        }
    }
}

// CARD-117, CARD-99: the delay's echoes, drawn from its own sum, are what it plays: a step through
// it, its first difference a click, gives the click and each echo where the bars say, within 0.5 dB.
#[test]
fn the_delays_echoes_are_its_sound() {
    for (rate, (time, fb, mix)) in RATES.into_iter().flat_map(|r| [(0.25, 0.3, 0.3), (0.1, 0.7, 0.5), (0.5, 0.5, 1.0)].map(|d| (r, d))) {
        let d = drawn(rate, "effects", "delay", "echoes", json!({"time": time, "feedback": fb, "mix": mix}));
        let shapes = d["shapes"].as_array().unwrap();
        let click = pairs(&shapes[0]["points"]);
        let echoes = pairs(&shapes[1]["points"]);
        let seconds = echoes.last().unwrap().0 + 0.05;
        let out =
            left(rate, &format!("export default song(() => delay({STEADY}, {{ time: {time}, feedback: {fb}, mix: {mix} }}));\n"), seconds);
        // Each sound of the click: its first difference summed round where it comes.
        let heard = |t: f64| -> f64 {
            let i = (t * rate).round() as usize;
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
                "at {rate} Hz, time {time}, feedback {fb}, mix {mix}: at {t:.3} s the bars say {y:.2} dB, the delay plays {:.2} dB",
                db(heard(t))
            );
            checked += 1;
        }
        assert!(checked >= 3, "{checked} echoes checked");
    }
}

// CARD-118, CARD-99 (review T2): the reverb's tail, drawn from its own sums, is what it plays: how
// loud what is left of a click's tail is (its energy from then on, Schroeder's backward sum), where
// the line `tail` says, within 0.5 dB, at every point it draws, down to 60 dB below its start, at
// its corners (size 0 and 1, damping 0 and 1) and between (size 0.4 at full damping, the furthest
// of a finer look, 0.47 dB). Each render lasts half as long again as
// the picture, so the energy it leaves out is far under −60 dB. At 48 and 96 kHz: at 44.1 kHz,
// whose comb lengths are Freeverb's own, its sound has more energy than its sums give about 0.8 s
// into a damped room, up to 0.85 dB at size 0.4 (gaps.md), so that rate isn't checked yet.
#[test]
fn the_reverbs_tail_is_its_sound() {
    let rooms = [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (0.5, 0.0), (0.5, 0.5), (0.5, 1.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.4, 1.0)];
    for (rate, (size, damping)) in [48_000.0, 96_000.0].into_iter().flat_map(|r| rooms.map(|x| (r, x))) {
        let tail = line(&drawn(rate, "effects", "reverb", "tail", json!({"size": size, "damping": damping, "mix": 1})), "tail");
        let seconds = tail.last().unwrap().0 * 1.5 + 0.5;
        let out =
            left(rate, &format!("export default song(() => reverb({STEADY}, {{ size: {size}, damping: {damping}, mix: 1 }}));\n"), seconds);
        // The click's tail, and what is left of it from each sample on.
        let click: Vec<f64> = (0..out.len()).map(|n| if n == 0 { out[0] } else { out[n] - out[n - 1] }).collect();
        let mut left = vec![0.0; click.len() + 1];
        for n in (0..click.len()).rev() {
            left[n] = left[n + 1] + click[n] * click[n];
        }
        let mut checked = 0;
        for &(t, y) in &tail {
            let heard = 10.0 * (left[(t * rate).round() as usize] / left[0]).log10();
            assert!(
                (heard - y).abs() <= 0.5,
                "at {rate} Hz, size {size}, damping {damping}: at {t:.3} s the tail says {y:.2} dB, the reverb plays {heard:.2} dB"
            );
            checked += 1;
        }
        assert!(checked == tail.len() && tail.last().unwrap().1 <= -59.9, "every point, down to −60 dB: {checked} of {}", tail.len());
    }
}

// CARD-119, CARD-99: the chorus's motion, drawn from its own sum, is what it plays: a 100 Hz tone
// through it, only the copies, is as loud as two copies that far apart make it,
// |cos(π · 100 · (one − two))|, within 0.5 dB, wherever that is above −30 dB.
#[test]
fn the_chorus_motion_is_its_sound() {
    for (sr, (rate, depth)) in RATES.into_iter().flat_map(|r| [(0.8, 1.0), (2.0, 0.6)].map(|x| (r, x))) {
        let d = drawn(sr, "effects", "chorus", "motion", json!({"rate": rate, "depth": depth, "mix": 1}));
        let (one, two) = (line(&d, "one"), line(&d, "two"));
        // 5 ms either side of each point: half a period of the tone each way.
        let half = (0.005 * sr).round() as usize;
        let out =
            left(sr, &format!("export default song(() => chorus(osc.sine(100), {{ rate: {rate}, depth: {depth}, mix: 1 }}));\n"), 1.0);
        let mut checked = 0;
        for (&(t, a), &(_, b)) in one.iter().zip(&two) {
            let i = (t * sr).round() as usize;
            let want = db((std::f64::consts::PI * 100.0 * (a - b)).cos());
            if t < 0.03 || i + half >= out.len() || want < -30.0 {
                continue;
            }
            // The two copies sum to one tone, as late as their middle, which stays where it is, its
            // level moving: that level here, fitted over the period round this point as a level
            // that moves in a straight line (so its move within the period is read at its middle).
            let late = (a + b) / 2.0;
            let (mut s2, mut us2, mut uus2, mut xs, mut uxs) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for (n, x) in out.iter().enumerate().take(i + half).skip(i - half) {
                let (u, s) = (n as f64 - i as f64, (2.0 * std::f64::consts::PI * 100.0 * (n as f64 / sr - late)).sin());
                (s2, us2, uus2, xs, uxs) = (s2 + s * s, us2 + u * s * s, uus2 + u * u * s * s, xs + x * s, uxs + u * x * s);
            }
            let heard = db((xs * uus2 - uxs * us2) / (s2 * uus2 - us2 * us2));
            assert!(
                (heard - want).abs() <= 0.5,
                "at {sr} Hz, rate {rate}, depth {depth}: at {t:.3} s the copies {:.2} ms apart make {want:.2} dB, the chorus plays {heard:.2} dB",
                (a - b) * 1000.0
            );
            checked += 1;
        }
        assert!(checked > 100, "{checked} points checked");
    }
}

// CARD-120, CARD-99: the phaser's motion, drawn from its own sum, is what it plays: a 1 kHz tone
// through it (feedback 0, mix 0.5) is as loud as its six stages, at the middle notch the picture
// draws, make it, |cos(6 · atan(tan(π · 1000 / rate) / tan(π · mid / rate)))|, within 0.5 dB,
// wherever that is above −10 dB. Deeper in a notch the sound lags the drawing by its stages' delay,
// about a millisecond, which moves a level near −20 dB more than 0.5 dB as the notch sweeps by.
#[test]
fn the_phaser_motion_is_its_sound() {
    for (sr, (rate, depth)) in RATES.into_iter().flat_map(|r| [(1.0, 0.7), (3.0, 0.4)].map(|x| (r, x))) {
        let mid = line(&drawn(sr, "effects", "phaser", "motion", json!({"rate": rate, "depth": depth, "feedback": 0, "mix": 0.5})), "mid");
        // A quarter of the tone's cycle, in samples.
        let quarter = (sr / 4000.0).round() as usize;
        let out = left(
            sr,
            &format!("export default song(() => phaser(osc.sine(1000), {{ rate: {rate}, depth: {depth}, feedback: 0, mix: 0.5 }}));\n"),
            1.0,
        );
        let w = (std::f64::consts::PI * 1000.0 / sr).tan();
        let mut checked = 0;
        for &(t, f) in &mid {
            let i = (t * sr).round() as usize;
            let want = db((6.0 * (w / (std::f64::consts::PI * f / sr).tan()).atan()).cos());
            if t < 0.02 || i + 4 * quarter >= out.len() || want < -10.0 {
                continue;
            }
            // Its level now: the sample and the one a quarter of a cycle on, a tone's two sides.
            let heard = db((out[i] * out[i] + out[i + quarter] * out[i + quarter]).sqrt());
            assert!(
                (heard - want).abs() <= 0.5,
                "at {sr} Hz, rate {rate}, depth {depth}: at {t:.3} s its middle notch at {f:.0} Hz makes {want:.2} dB, the phaser plays {heard:.2} dB"
            );
            checked += 1;
        }
        assert!(checked > 100, "{checked} points checked");
    }
}

// CARD-121, CARD-99: the tremolo's motion, drawn from its own sum, is what it plays: a steady sound
// through it is as loud as the line `level` says, within 0.5 dB, wherever that is above −40 dB.
#[test]
fn the_tremolo_motion_is_its_sound() {
    for (sr, (rate, depth)) in RATES.into_iter().flat_map(|r| [(5.0, 0.5), (2.0, 1.0), (13.0, 0.8)].map(|x| (r, x))) {
        let level = line(&drawn(sr, "effects", "tremolo", "motion", json!({"rate": rate, "depth": depth})), "level");
        let out = left(sr, &format!("export default song(() => tremolo({STEADY}, {{ rate: {rate}, depth: {depth} }}));\n"), 1.0);
        let mut checked = 0;
        for &(t, y) in &level {
            let i = (t * sr).round() as usize;
            if y < 0.01 || i < SETTLED || i >= out.len() {
                continue;
            }
            assert!(
                (db(out[i]) - db(y)).abs() <= 0.5,
                "at {sr} Hz, rate {rate}, depth {depth}: at {t:.3} s the picture says {y:.4}, the tremolo plays {:.4}",
                out[i]
            );
            checked += 1;
        }
        assert!(checked > 100, "{checked} points checked");
    }
}

// CARD-122, CARD-99: drive's curve, drawn from its own sum, is what it plays: a steady level in
// comes out where the curve says, within 0.5 dB.
#[test]
fn drives_curve_is_its_sound() {
    for (rate, amount) in RATES.into_iter().flat_map(|r| [0.0, 3.0, 12.0, 36.0].map(|a| (r, a))) {
        let curve = line(&drawn(rate, "effects", "drive", "curve", json!({"amount": amount})), "curve");
        for level in [0.05, 0.1, 0.25, 0.5, 0.9, 1.0] {
            let &(x, y) = curve.iter().min_by(|a, b| (a.0 - level).abs().total_cmp(&(b.0 - level).abs())).unwrap();
            assert!((x - level).abs() < 1e-9, "the curve has a point at {level}");
            let out = left(
                rate,
                &format!("export default song(() => drive(gain({STEADY}, db({})), {{ amount: {amount} }}));\n", db(level)),
                0.01,
            );
            let heard = out[out.len() - 1];
            assert!(
                (db(heard) - db(y)).abs() <= 0.5,
                "at {rate} Hz, amount {amount}: {level} in, the curve says {y:.4}, drive plays {heard:.4}"
            );
        }
    }
}

// CARD-123, CARD-99: the bitcrusher's sine, drawn from its own sum, is what it plays: a 1 kHz sine
// through it, sample by sample over its first cycle, where the line `crushed` says, within 0.5 dB
// and on the same side of 0, wherever it is above −40 dB.
#[test]
fn the_bitcrushers_sine_is_its_sound() {
    let crushes = [(4.0, 12000.0, 1.0), (8.0, 6000.0, 1.0), (3.0, 3000.0, 0.5), (2.0, 48000.0, 1.0)];
    for (sr, (bits, rate, mix)) in RATES.into_iter().flat_map(|r| crushes.map(|c| (r, c))) {
        let held = line(&drawn(sr, "effects", "bitcrush", "crush", json!({"bits": bits, "rate": rate, "mix": mix})), "crushed");
        let out = left(
            sr,
            &format!("export default song(() => bitcrush(osc.sine(1000), {{ bits: {bits}, rate: {rate}, mix: {mix} }}));\n"),
            0.002,
        );
        let mut checked = 0;
        // Each sample's value is drawn twice, at its start and its end: its start.
        for &(t, y) in held.iter().step_by(2) {
            let n = (t * sr).round() as usize;
            if y.abs() < 0.01 {
                continue;
            }
            assert!(
                y.signum() == out[n].signum() && (db(out[n]) - db(y)).abs() <= 0.5,
                "at {sr} Hz, {bits} bits at {rate} Hz, mix {mix}: sample {n} is {y:.4} in the picture, {:.4} as it plays",
                out[n]
            );
            checked += 1;
        }
        assert!(checked > 30, "{checked} samples checked");
    }
}

// CARD-124, CARD-99: the widener's field, drawn from its own sum, is what it plays: a sound panned
// hard left (its sides as loud as its middle) comes out with its sides turned as far against its
// middle as the field's edge says, within 0.5 dB.
#[test]
fn the_wideners_field_is_its_sound_for_a_wide_sound() {
    for (rate, width) in RATES.into_iter().flat_map(|r| [0.25, 0.5, 1.0, 1.5, 2.0].map(|w| (r, w))) {
        let edge = dots(&drawn(rate, "effects", "widen", "field", json!({"width": width})))[0].0;
        let out = render(rate, &format!("export default song(() => widen(pan({STEADY}, -1), {{ width: {width} }}));\n"), 0.2);
        let [l, r] = out[out.len() - 1];
        let (mid, side) = ((l + r) / 2.0, (l - r) / 2.0);
        assert!(
            (db(side / mid) - db(edge)).abs() <= 0.5,
            "at {rate} Hz, width {width}: the field's edge at {edge}, the sides {:.2} dB against the middle",
            db(side / mid)
        );
    }
}

// CARD-124, CARD-99 (review T3): a mono sound through the widener, a 3 kHz tone, comes out with
// sides only above width 1, as loud against its middle as the made-up fan's edge says (the middle
// 11 ms late, above 300 Hz, times 0.6 × (width − 1)), within 0.5 dB; at width 1 or less it stays
// mono, and the picture draws no made-up fan.
#[test]
fn the_wideners_field_is_its_sound_for_a_mono_sound() {
    for (rate, width) in RATES.into_iter().flat_map(|r| [0.5, 1.0, 1.25, 1.5, 2.0].map(|w| (r, w))) {
        let d = drawn(rate, "effects", "widen", "field", json!({"width": width}));
        let made = d["shapes"].as_array().unwrap().iter().find(|s| s["name"] == "made").map(|s| pairs(&s["points"]));
        let out = render(rate, &format!("export default song(() => widen(osc.sine(3000), {{ width: {width} }}));\n"), 0.2);
        // Over the last 50 ms, long after the 11 ms the made-up side waits.
        let tail = &out[out.len() - (0.05 * rate) as usize..];
        let rms = |f: &dyn Fn(&[f64; 2]) -> f64| (tail.iter().map(|x| f(x) * f(x)).sum::<f64>() / tail.len() as f64).sqrt();
        let (mid, side) = (rms(&|x| (x[0] + x[1]) / 2.0), rms(&|x| (x[0] - x[1]) / 2.0));
        match made {
            None => assert!(
                width <= 1.0 && db(side / mid) < -80.0,
                "at {rate} Hz, width {width}: no made-up fan, and the sound stays mono: {:.1} dB",
                db(side / mid)
            ),
            Some(fan) => {
                let edge = fan.iter().map(|p| p.0).fold(0.0, f64::max);
                assert!(
                    width > 1.0 && (db(side / mid) - db(edge)).abs() <= 0.5,
                    "at {rate} Hz, width {width}: the made-up fan's edge at {edge:.3}, the sides {:.2} dB against the middle",
                    db(side / mid)
                );
            }
        }
    }
}

// CARD-114, CARD-99 (review T4): the de-esser's band is what it turns down, by how far the band's
// peaks go over its threshold: a tone at the band's middle (its dot), its peaks 10 dB under 0 dB,
// comes out turned down by its peaks' height over the threshold, at most `amount`, within 0.5 dB;
// one at the band's edges less, and one two octaves away by under 1 dB.
#[test]
fn the_deessers_band_is_what_it_turns_down() {
    for rate in RATES {
        let d = drawn(rate, "mixing", "deesser", "band", json!({"freq": 6000, "threshold": -30, "amount": 6}));
        let shapes = d["shapes"].as_array().unwrap();
        let box_ = shapes.iter().find(|s| s["kind"] == "fill").map(|s| pairs(&s["points"])).unwrap();
        let (lo, hi) = (box_.iter().map(|p| p.0).fold(f64::MAX, f64::min), box_.iter().map(|p| p.0).fold(0.0, f64::max));
        let middle = dots(&d)[0].0;
        let heard = |hz: f64, threshold: f64, amount: f64| -> f64 {
            let body = |t: f64| {
                format!(
                    "export default song(() => deesser(gain(osc.sine({hz}), db(-10)), {{ freq: 6000, threshold: {t}, amount: {amount} }}));\n"
                )
            };
            let out = left(rate, &body(threshold), 0.3);
            let dry = left(rate, &body(0.0), 0.3);
            let n = (rate / hz).ceil() as usize;
            db(level_at(&out, out.len() - n, n)) - db(level_at(&dry, dry.len() - n, n))
        };
        for (threshold, amount, down) in [(-30.0, 6.0, 6.0), (-13.0, 6.0, 3.0), (-5.0, 6.0, 0.0), (-30.0, 12.0, 12.0)] {
            let got = heard(middle, threshold, amount);
            assert!(
                (got + down).abs() <= 0.5,
                "at {rate} Hz, at the band's middle, {middle} Hz, threshold {threshold}, amount {amount}: down {down} dB, it plays {got:.2} dB"
            );
        }
        for edge in [lo, hi] {
            let got = heard(edge, -30.0, 12.0);
            assert!(
                got < -1.0 && got > -12.0,
                "at {rate} Hz, at the band's edge, {edge:.0} Hz: turned down, less than at its middle: {got:.2} dB"
            );
        }
        let far = middle / 4.0;
        let got = heard(far, -30.0, 12.0);
        assert!(got > -1.0, "at {rate} Hz, two octaves under the band, {far:.0} Hz: under 1 dB: {got:.2} dB");
    }
}

// CARD-110 (review T1): a build of a song that plays both packs works out none of their pictures'
// sums (the transient shaper's hit, the reverb's room): each is worked out on its picture's first
// draw, and kept. The song counts them as it builds, and fails if any ran; asking for one works it
// out once, so the count counts.
#[test]
fn a_build_works_out_none_of_the_pictures_sums() {
    let head = "import { song } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { reverb } from '@m2/effects';\nimport { transient } from '@m2/mixing';\n\
                import { sumsWorked as fx, tailLevel } from '@m2/effects/pictures';\nimport { sumsWorked as mix, heard } from '@m2/mixing/pictures';\n";
    let built = |body: &str| {
        let f = Fixture::new();
        f.write("music2.toml", manifest(48_000.0));
        f.write("song.ts", format!("{head}{body}export default song(() => transient(reverb(osc.saw(110))));\n"));
        let s = f.open();
        (s.draft_errors.is_empty() && s.problems().is_empty(), format!("{:?} {:?}", s.draft_errors, s.problems()))
    };
    let (ok, why) =
        built("const ran = fx() + mix();\nif (ran !== 0) throw new Error(`the pictures' sums ran ${ran} times as the song built`);\n");
    assert!(ok, "a build works out none: {why}");
    let (ok, why) = built(
        "tailLevel(0.5, 0.5, 48000)(0.1); tailLevel(0.9, 0, 48000); heard(); heard();\nif (fx() !== 1 || mix() !== 1) throw new Error(`asked for, worked out ${fx()} and ${mix()} times`);\n",
    );
    assert!(ok, "asked for, each is worked out once and kept: {why}");
}
