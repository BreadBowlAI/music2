//! music2's own instruments' pictures drawn from their kernels' sums are the sound (CARD-131,
//! CARD-133, CARD-136, CARD-140; D13): each against a render of the kernel it draws, within 0.5 dB.
//! The Wavetable's tables, harmonic by harmonic; its filter's four shapes, and the polysynth's
//! ladder, each as the change it makes to a note's harmonics; the FM synth's partials as a note
//! starts. Each kernel is played straight, on a note at 187.5 Hz, so 8,192 samples at 48 kHz are 32
//! whole cycles and each harmonic falls on one bin of a plain transform.

use std::path::PathBuf;

use m2_runtime::Renderer;
use m2_script::pictures::{Ask, Drawn, Runner};
use serde_json::{Value, json};

use crate::testing::{Fixture, MANIFEST};

const RATE: f64 = 48_000.0;
/// The note every kernel here plays: 32 whole cycles in `WINDOW` samples.
const F0: f64 = 187.5;
const WINDOW: usize = 8192;

/// The note `F0` as a pitch in semitones (69 is A 440).
fn pitch() -> f64 {
    69.0 + 12.0 * (F0 / 440.0).log2()
}

/// The bundled packs, by name and folder, and std.
fn packages() -> Vec<(String, PathBuf)> {
    let mut p = m2_song::pack::test_packs();
    p.push(("std".into(), std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")));
    p
}

/// Picture `view` of the component `cid` at `values`: its shapes and words, as the runner draws them.
fn drawn(cid: &str, view: &str, values: Value) -> Value {
    let packages = packages();
    let catalog = m2_script::describe(&packages);
    let descriptor = catalog.components.get(cid).cloned().unwrap_or_else(|| panic!("no {cid}: {:?}", catalog.problems));
    let ask = Ask {
        key: cid.split(':').next().unwrap().into(),
        packages,
        song: None,
        module: descriptor.module.clone(),
        descriptor,
        view: view.into(),
        values: values.to_string(),
        memo: format!("{cid}\u{0}{view}"),
        rate: 48_000,
    };
    match Runner::new().draw(ask) {
        Drawn::Shapes(json) => serde_json::from_str(&json).unwrap(),
        other => panic!("{cid}'s {view} didn't draw: {other:?}"),
    }
}

/// The points of the first shape of `kind` (named `name`, when given) a picture drew.
fn points(drawn: &Value, kind: &str, name: Option<&str>) -> Vec<(f64, f64)> {
    let s = drawn["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == kind && name.is_none_or(|n| s["name"] == n))
        .unwrap_or_else(|| panic!("no {kind} {name:?}: {drawn}"));
    s["points"].as_array().unwrap().chunks(2).map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect()
}

/// `WINDOW` samples from `from` seconds of a song whose track `t` plays `call` (a kernel of a pack):
/// its left side.
fn render(call: &str, from: f64) -> Vec<f64> {
    let f = Fixture::new();
    f.write("music2.toml", format!("{MANIFEST}\"@m2/wavetable\" = \"1\"\n"));
    f.write(
        "song.ts",
        format!(
            "import {{ kernel, song, track }} from '@m2/core';\nconst t = track('t', () => {call});\nexport default song(() => t.out);\n"
        ),
    );
    let s = f.open();
    let rev = s.accepted();
    let mut r = Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, rev.project.sample_rate()).unwrap();
    let skip = (from * RATE) as usize;
    let mut out = vec![[0.0f32; 2]; skip + WINDOW];
    r.process(&mut out).unwrap();
    out[skip..].iter().map(|x| x[0] as f64).collect()
}

/// The size of harmonic `h` of `F0` in `x`, `WINDOW` samples of whole cycles.
fn harmonic(x: &[f64], h: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * h * F0 / RATE;
    let (mut re, mut im) = (0.0, 0.0);
    for (n, v) in x.iter().enumerate() {
        re += v * (w * n as f64).cos();
        im -= v * (w * n as f64).sin();
    }
    2.0 * (re * re + im * im).sqrt() / x.len() as f64
}

fn db(x: f64) -> f64 {
    20.0 * x.abs().max(1e-12).log10()
}

/// A drawn curve's level at `hz`, between its points on their log scale.
fn curve_at(curve: &[(f64, f64)], hz: f64) -> f64 {
    let at = curve.iter().position(|p| p.0 >= hz).unwrap();
    if at == 0 || curve[at].0 == hz {
        return curve[at].1;
    }
    let ((f0, y0), (f1, y1)) = (curve[at - 1], curve[at]);
    y0 + (hz / f0).ln() / (f1 / f0).ln() * (y1 - y0)
}

/// The Wavetable's voice playing `F0` held, with these of its inputs, and its table's settings.
fn wavetable(inputs: &str, table: usize) -> String {
    format!(
        "kernel('@m2/wavetable:voice', {{ pitch: {}, gate: 1, a_level: 1, b_level: 0, attack: 0.001, sustain: 1, {inputs} }}, {{ values: [{table}, 0, -1, -1, 1] }})",
        pitch()
    )
}

/// Harmonic `h` of `F0` in `x` (`WINDOW` samples of whole cycles) as a complex number, `(re, im)`.
fn phasor(x: &[f64], h: f64) -> (f64, f64) {
    let w = 2.0 * std::f64::consts::PI * h * F0 / RATE;
    let (mut re, mut im) = (0.0, 0.0);
    for (n, v) in x.iter().enumerate() {
        re += v * (w * n as f64).cos();
        im -= v * (w * n as f64).sin();
    }
    (2.0 * re / x.len() as f64, 2.0 * im / x.len() as f64)
}

/// The harmonics `(size, phase)` of a cycle, each turned back by h times the fundamental's phase, so
/// two cycles of one shape compare whatever their start and their delay.
fn aligned(c: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let first = c[0].1.atan2(c[0].0);
    c.iter().enumerate().map(|(i, (re, im))| ((re * re + im * im).sqrt(), im.atan2(*re) - (i + 1) as f64 * first)).collect()
}

/// An angle wrapped to −π..π.
fn wrapped(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    a - t * (a / t).round()
}

// CARD-131: each built-in table, drawn at a position between two of its frames, is what the
// Wavetable plays there, in shape and not only in size: every harmonic up to the 20th the drawing
// holds above −40 dB is the played one's, against the loudest of them, within 0.5 dB, and its phase
// against the fundamental's within 5° (review U2: a frame drawn upside down, or backwards in time,
// has the right sizes and the wrong shape).
#[test]
fn the_drawn_tables_are_the_wavetables_sound() {
    for (t, table, position) in [(0, "basic", 0.5), (1, "pulse", 0.7), (2, "vowels", 0.35), (3, "harmonics", 0.6), (4, "digital", 0.4)] {
        let pic =
            drawn("@m2/wavetable:wavetable", "wavesA", json!({"tableA": table, "fileA": null, "positionA": position, "lfoPosition": 0}));
        let frame: Vec<f64> = points(&pic, "line", Some("frame")).iter().map(|p| p.1).collect();
        let n = frame.len() - 1;
        // The drawing's harmonics: one cycle of n points.
        let drew: Vec<(f64, f64)> = (1..=20)
            .map(|h| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, y) in frame[..n].iter().enumerate() {
                    let th = 2.0 * std::f64::consts::PI * (h * i) as f64 / n as f64;
                    re += y * th.cos();
                    im -= y * th.sin();
                }
                (2.0 * re / n as f64, 2.0 * im / n as f64)
            })
            .collect();
        // Its filter a highpass at 20 Hz, whose own turn of each harmonic (its digital curve: the
        // analog one at tan(π f / rate) over tan(π 20 / rate)) is taken back.
        let out = render(&wavetable(&format!("a_position: {position}, filter: 2, cutoff: 20, q: 0.707"), t), 0.5);
        let heard: Vec<(f64, f64)> = (1..=20)
            .map(|h| {
                let (re, im) = phasor(&out, h as f64);
                let w = (std::f64::consts::PI * h as f64 * F0 / RATE).tan() / (std::f64::consts::PI * 20.0 / RATE).tan();
                // H = −w² / (1 − w² + i w / q).
                let (dr, di) = (1.0 - w * w, w / 0.707);
                let (hr, hi) = (-w * w * dr / (dr * dr + di * di), w * w * di / (dr * dr + di * di));
                let m = hr * hr + hi * hi;
                ((re * hr + im * hi) / m, (im * hr - re * hi) / m)
            })
            .collect();
        let (drew, heard) = (aligned(&drew), aligned(&heard));
        let (dtop, htop) = (drew.iter().map(|x| x.0).fold(0.0, f64::max), heard.iter().map(|x| x.0).fold(0.0, f64::max));
        let mut checked = 0;
        for h in 0..20 {
            let d = db(drew[h].0 / dtop);
            if d < -40.0 {
                continue;
            }
            let m = db(heard[h].0 / htop);
            assert!((m - d).abs() <= 0.5, "{table} at {position}: harmonic {} is drawn {d:.2} dB, played {m:.2} dB", h + 1);
            let turn = wrapped(drew[h].1 - heard[h].1).to_degrees();
            assert!(turn.abs() <= 5.0, "{table} at {position}: harmonic {} is drawn {turn:.1}° from where it is played", h + 1);
            checked += 1;
        }
        assert!(checked >= 5, "{table}: {checked} harmonics checked");
    }
}

// CARD-133: the Wavetable's filter, drawn for each of its four shapes, is what its voice does to a
// saw's harmonics, within 0.5 dB, wherever the drawing is above −60 dB, up to a twelfth of the rate.
#[test]
fn the_drawn_filter_is_the_wavetables_filter() {
    // Table basic at 2/3 is a saw: every harmonic, each as the reference plays it.
    let saw = |inputs: &str| render(&wavetable(&format!("a_position: 0.6666667, {inputs}"), 0), 0.5);
    let open = saw("filter: 2, cutoff: 20, q: 0.707");
    for (shape, place, cutoff, q) in
        [("lowpass", 0, 1000.0, 2.0), ("bandpass", 1, 800.0, 4.0), ("highpass", 2, 600.0, 0.707), ("notch", 3, 1000.0, 1.0)]
    {
        let pic = drawn("@m2/wavetable:wavetable", "curve", json!({"filter": shape, "cutoff": cutoff, "q": q}));
        let curve = points(&pic, "line", Some("curve"));
        let out = saw(&format!("filter: {place}, cutoff: {cutoff}, q: {q}"));
        let mut checked = 0;
        for h in 1..=(RATE / 12.0 / F0) as usize {
            let hz = h as f64 * F0;
            let d = curve_at(&curve, hz);
            if d < -60.0 {
                continue;
            }
            let m = db(harmonic(&out, h as f64) / harmonic(&open, h as f64));
            assert!((m - d).abs() <= 0.5, "{shape} at {cutoff} Hz, q {q}: at {hz} Hz it is drawn {d:.2} dB, played {m:.2} dB");
            checked += 1;
        }
        assert!(checked >= 10, "{shape}: {checked} harmonics checked");
        assert!(pic["words"].as_str().unwrap().contains(" q "), "{}", pic["words"]);
    }
}

/// The polysynth's voice playing `F0` held, a quiet saw alone (so its ladder stays clean), with these inputs.
fn synth(inputs: &str) -> String {
    format!(
        "kernel('@m2/synths:synth', {{ pitch: {}, gate: 1, saw: 0.1, keytrack: 0, env: 0, attack: 0.001, sustain: 1, {inputs} }})",
        pitch()
    )
}

// CARD-136: the polysynth's 24 dB ladder, drawn from its sums, is what its voice does to a saw's
// harmonics, within 0.5 dB wherever the drawing is above −60 dB, up to a twelfth of the rate: the
// drawing's change from the filter wide open to these settings is the played one's.
#[test]
fn the_drawn_ladder_is_the_polysynths_filter() {
    let curve_of = |cutoff: f64, resonance: f64| {
        let pic = drawn("@m2/synths:synth-custom", "filter", json!({"cutoff": cutoff, "resonance": resonance}));
        (points(&pic, "line", Some("curve")), pic["words"].as_str().unwrap().to_string())
    };
    let (wide, _) = curve_of(20_000.0, 0.0);
    let open = render(&synth("cutoff: 20000, resonance: 0"), 0.5);
    for (cutoff, resonance) in [(500.0, 0.0), (1200.0, 0.5), (2000.0, 0.8)] {
        let (curve, words) = curve_of(cutoff, resonance);
        let out = render(&synth(&format!("cutoff: {cutoff}, resonance: {resonance}")), 0.5);
        let mut checked = 0;
        for h in 1..=(RATE / 12.0 / F0) as usize {
            let hz = h as f64 * F0;
            let d = curve_at(&curve, hz) - curve_at(&wide, hz);
            if curve_at(&curve, hz) < -60.0 {
                continue;
            }
            let m = db(harmonic(&out, h as f64) / harmonic(&open, h as f64));
            assert!((m - d).abs() <= 0.5, "cutoff {cutoff}, resonance {resonance}: at {hz} Hz it is drawn {d:.2} dB, played {m:.2} dB");
            checked += 1;
        }
        assert!(checked >= 8, "cutoff {cutoff}: {checked} harmonics checked");
        assert!(words.contains("24 dB an octave"), "{words}");
    }
}

/// The size of `hz` in `x`, through a Hann window, against the other sizes it gives: for partials
/// that fall between a plain transform's bins (middle C's), each many bins from the next.
fn windowed(x: &[f64], hz: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * hz / RATE;
    let n = x.len() as f64;
    let (mut re, mut im) = (0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n).cos();
        re += hann * v * (w * i as f64).cos();
        im -= hann * v * (w * i as f64).sin();
    }
    (re * re + im * im).sqrt()
}

// CARD-140: the FM synth's partials, drawn as a note starts at middle C, are what its voice plays
// then: each drawn above −40 dB against the loudest is the played one's within 0.5 dB. Pair A alone
// whose partials fold back below 0 and land together (1 and 1); both pairs at once, each with its
// share of the mix (A's partials odd, B's even, so none land together); and a modulator so high that
// the kernel limits its index, as the drawing must (review U3).
#[test]
fn the_drawn_partials_are_the_fm_synths_sound() {
    let middle_c = 440.0 * 2f64.powf(-9.0 / 12.0);
    for (a, b, mix, tone) in [
        ((1.0, 1.0, 1.5), (1.0, 14.0, 0.0), 0.0, 0.5),
        ((1.0, 2.0, 2.0), (2.0, 4.0, 1.2), 0.4, 0.75),
        ((1.0, 24.0, 4.0), (1.0, 14.0, 0.0), 0.0, 0.75),
    ] {
        let pic = drawn(
            "@m2/synths:fm-custom",
            "partials",
            json!({"carrierA": a.0, "modulatorA": a.1, "amountA": a.2, "carrierB": b.0, "modulatorB": b.1, "amountB": b.2, "mixB": mix, "tone": tone, "feedback": 0}),
        );
        let bars: Vec<(f64, f64)> = pic["shapes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["kind"] == "bars")
            .flat_map(|s| {
                s["points"].as_array().unwrap().chunks(2).map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect::<Vec<_>>()
            })
            .collect();
        // A long brightness decay, so the index holds over the 0.17 s heard.
        let call = format!(
            "kernel('@m2/synths:fm', {{ pitch: 60, gate: 1, velocity: 0.8, ratio_a: {}, mod_a: {}, index_a: {}, fall_a: 60, ratio_b: {}, mod_b: {}, index_b: {}, fall_b: 60, mix_b: {mix}, feedback: 0, detune: 0, tone: {tone}, attack: 0.001, sustain: 1 }})",
            a.0, a.1, a.2, b.0, b.1, b.2
        );
        let out = render(&call, 0.02);
        let top = bars.iter().map(|b| b.1).fold(f64::MIN, f64::max);
        let heard: Vec<f64> = bars.iter().map(|b| db(windowed(&out, b.0 * middle_c))).collect();
        let loudest = heard.iter().cloned().fold(f64::MIN, f64::max);
        let mut checked = 0;
        for (bar, h) in bars.iter().zip(&heard) {
            if bar.1 - top < -40.0 {
                continue;
            }
            assert!(
                ((h - loudest) - (bar.1 - top)).abs() <= 0.5,
                "{a:?} and {b:?}, mix {mix}, tone {tone}: at ratio {} it is drawn {:.2} dB, played {:.2} dB",
                bar.0,
                bar.1 - top,
                h - loudest
            );
            checked += 1;
        }
        assert!(checked >= 2, "{a:?}: {checked} partials checked");
    }
}

// CARD-136: each sound's note picture has a y range that fits its sound: middle C at its defaults,
// as `p.hit` measures it, peaks inside the picture and fills at least a third of its height, so its
// attack and decay can be read (review U4: on ±1 the polysynth's note was a band a few px tall).
#[test]
fn each_note_fits_its_picture() {
    use crate::measure::{Layer, copy};
    let sounds = [
        ("@m2/synths:synth-custom", "synth.custom"),
        ("@m2/synths:synth-pluck", "synth.pluck"),
        ("@m2/synths:synth-sub", "synth.sub"),
        ("@m2/synths:synth-bass", "synth.bass"),
        ("@m2/synths:fm-keys", "fm.keys"),
        ("@m2/synths:fm-bell", "fm.bell"),
        ("@m2/synths:fm-metal", "fm.metal"),
    ];
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let tracks: Vec<String> = sounds
        .iter()
        .enumerate()
        .map(|(i, (_, call))| format!("const t{i} = track('t{i}', ({{ lane }}) => {call}(lane.notes('notes')));\n"))
        .collect();
    let outs: Vec<String> = (0..sounds.len()).map(|i| format!("t{i}: t{i}.out")).collect();
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from '@m2/core';\nimport {{ fm, synth }} from '@m2/synths';\nimport {{ mixer }} from 'std/mix';\n{}export default song(() => mixer({{ {} }}));\n",
            tracks.concat(),
            outs.join(", ")
        ),
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let rev = s.accepted();
    let catalog = m2_script::describe(&packages());
    for (i, (cid, call)) in sounds.iter().enumerate() {
        let m2_script::component::ViewOf::Picture { y, .. } =
            &catalog.components[*cid].editor.views.iter().find(|v| v.name == "note").unwrap_or_else(|| panic!("{cid}'s note")).of
        else {
            panic!("{cid}'s note is a picture")
        };
        let site = rev.compiled.sites.iter().find(|x| x.file == "song.ts" && &x.callee == call).unwrap();
        let node = format!("{}#{}", site.file, site.ordinal);
        let c = copy(rev, &s.packages, &format!("t{i}"), &node, &serde_json::Map::new(), Layer::Hit { note: 60, seconds: 1.0 }).unwrap();
        let peak = c
            .frames(&s.packages, std::time::Duration::from_secs(5))
            .unwrap()
            .iter()
            .map(|f| (0.5 * (f[0] + f[1]) as f64).abs())
            .fold(0.0, f64::max);
        assert!(peak <= y.hi && -peak >= y.lo, "{cid}: middle C peaks at {peak:.3}, past its picture's {}..{}", y.lo, y.hi);
        assert!(peak >= y.hi / 3.0, "{cid}: middle C peaks at {peak:.3}, under a third of its picture's {}", y.hi);
    }
}
