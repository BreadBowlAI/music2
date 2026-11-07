//! std's own cards' pictures (CARD-90 to CARD-98): each draws at its defaults, from std/pictures,
//! with the dots and the words its rule says, as the picture runner draws it for a window or an
//! agent. Whether a picture drawn from std's own sums is the sound is checked against a render in
//! `crates/editor/src/std_pictures_tests.rs`.

use std::path::{Path, PathBuf};

use m2_script::pictures::{Ask, Drawn, Runner};
use serde_json::{Value, json};

fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

/// Picture `view` of std's `component`, drawn at `values` (what plays, by name, CARD-63).
fn draw(runner: &Runner, component: &str, view: &str, values: Value) -> Result<Value, String> {
    let packages = vec![("std".to_string(), std_dir())];
    let catalog = m2_script::describe(&packages);
    let cid = format!("std:{component}");
    let descriptor = catalog.components.get(&cid).cloned().unwrap_or_else(|| panic!("no {cid}: {:?}", catalog.problems));
    let ask = Ask {
        key: "std".into(),
        packages,
        song: None,
        module: descriptor.module.clone(),
        descriptor,
        view: view.into(),
        values: values.to_string(),
        memo: format!("{component}\u{0}{view}"),
        rate: 48_000,
    };
    match runner.draw(ask) {
        Drawn::Shapes(json) => Ok(serde_json::from_str(&json).unwrap()),
        Drawn::Cant(why) => Err(why),
        other => panic!("{component}'s {view}: {other:?}"),
    }
}

fn kinds(drawn: &Value) -> Vec<&str> {
    drawn["shapes"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect()
}

/// What each dot moves, by its `moves` names: `["x=cutoff", "y=q(knob)", …]`.
fn dots(drawn: &Value) -> Vec<String> {
    drawn["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["kind"] == "handle")
        .map(|s| {
            let m = s["moves"].as_object().unwrap();
            let mut said: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}={}{}", v["name"].as_str().unwrap(), if v.get("by").is_some() { "(knob)" } else { "" }))
                .collect();
            said.sort();
            said.join(" ")
        })
        .collect()
}

// CARD-91: each of std's four filters draws its curve measured from its own sound, and one dot: the
// cutoff across, the q as its knob and by the wheel, at the filter's peak (a notch's at the foot).
#[test]
fn std_filters_draw_their_measured_curve_and_one_dot() {
    let runner = Runner::new();
    for (filter, said, y) in [
        ("lowpass", "Cuts the highs above 1 kHz, q 2", 20.0 * 2f64.log10()),
        ("highpass", "Cuts the lows below 1 kHz, q 2", 20.0 * 2f64.log10()),
        ("bandpass", "Keeps a band around 1 kHz, q 2", 20.0 * 2f64.log10()),
        ("notch", "Cuts a narrow band out at 1 kHz, q 2", -24.0),
    ] {
        let d = draw(&runner, filter, "curve", json!({"cutoff": 1000, "q": 2})).unwrap();
        assert_eq!(kinds(&d), ["response", "handle"], "{filter}");
        assert_eq!(d["shapes"][0]["name"], "curve");
        assert_eq!(dots(&d), ["wheel=q x=cutoff y=q(knob)"], "{filter}");
        let h = &d["shapes"][1];
        assert_eq!(h["x"], 1000.0);
        assert!((h["y"].as_f64().unwrap() - y).abs() < 1e-9, "{filter}: {h}");
        assert!(d["words"].as_str().unwrap().starts_with(said), "{filter}: {}", d["words"]);
    }
}

// CARD-92: eq3 draws its measured curve and a dot for each band, numbered 1 to 3 in one colour
// (D14), each moving its band's frequency across and its gain up and down; a cut past the
// picture's foot rests there.
#[test]
fn eq3_draws_its_measured_curve_and_a_dot_for_each_band() {
    let runner = Runner::new();
    let v = json!({"low": -60, "mid": 3.5, "high": 0, "lowFreq": 200, "midFreq": 1000, "highFreq": 4000});
    let d = draw(&runner, "eq3", "curve", v).unwrap();
    assert_eq!(kinds(&d), ["response", "handle", "handle", "handle"]);
    assert_eq!(dots(&d), ["x=lowFreq y=low", "x=midFreq y=mid", "x=highFreq y=high"]);
    let s = d["shapes"].as_array().unwrap();
    assert_eq!([&s[1]["mark"], &s[2]["mark"], &s[3]["mark"]], ["1", "2", "3"]);
    assert_eq!((s[1]["x"].as_f64(), s[1]["y"].as_f64()), (Some(200.0), Some(-24.0)), "a cut rests at the foot");
    assert_eq!((s[2]["x"].as_f64(), s[2]["y"].as_f64()), (Some(1000.0), Some(3.5)));
    assert_eq!(d["words"], "Low −60 dB at 200 Hz; mid 3.5 dB at 1 kHz; high 0 dB at 4 kHz");
}

// CARD-93: compress draws its transfer curve from its own sum (the line `transfer` its live level
// rides), a faint line where in is out, a dot at the threshold (across) and one on the line above the
// knee (the ratio, across as its knob's drag does, and by the wheel; review S4); and its last
// seconds beside it, in, out and how far it turned down, on one axis.
#[test]
fn compress_draws_its_transfer_curve_and_its_last_seconds() {
    let runner = Runner::new();
    let v = json!({"threshold": -18, "ratio": 4, "attack": 0.01, "release": 0.15, "makeup": 3});
    let d = draw(&runner, "compress", "transfer", v.clone()).unwrap();
    assert_eq!(kinds(&d), ["line", "line", "handle", "handle"]);
    assert_eq!(d["shapes"][1]["name"], "transfer");
    assert_eq!(dots(&d), ["x=threshold", "wheel=ratio x=ratio(knob)"]);
    let points: Vec<f64> = d["shapes"][1]["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    // Below the knee it is a straight line up by the makeup; above it 1 dB out for each 4 in.
    let at = |level: f64| points.chunks(2).find(|p| (p[0] - level).abs() < 1e-9).unwrap()[1];
    assert!((at(-40.0) - (-37.0)).abs() < 1e-9);
    assert!((at(0.0) - (-18.0 + 18.0 / 4.0 + 3.0)).abs() < 1e-9);
    // The ratio's dot is on the line, above the knee: halfway from the threshold to 0 dB in.
    let r = &d["shapes"][3];
    assert_eq!(r["x"], -9.0);
    assert!((r["y"].as_f64().unwrap() - (-18.0 + 9.0 / 4.0 + 3.0)).abs() < 1e-9, "{r}");
    assert_eq!(
        d["words"],
        "Above −18 dB the level rises only 1 dB for each 4, then up 3 dB. Drag the corner for the threshold, the line for the ratio"
    );
    let history = draw(&runner, "compress", "history", v).unwrap();
    assert_eq!(
        history["shapes"],
        json!([{"kind": "mark", "axis": "y", "at": 0.0, "words": "0 dB: the levels below, how far it turned down above"}])
    );
    assert!(
        history["words"].as_str().unwrap().starts_with("The last 3 seconds: the level going in (faint) and coming out"),
        "{}",
        history["words"]
    );
}

// CARD-94: the duck draws how far it turns down, from its meter; its most as a guide line,
// kept on the picture past its top, and saying so (review S9). (The limiter draws its ceiling over
// its last seconds: CARD-143.)
#[test]
fn the_duck_draws_how_far_it_turns_down() {
    let runner = Runner::new();
    let d = draw(&runner, "duck", "down", json!({"depth": -8})).unwrap();
    assert_eq!(d["shapes"], json!([{"kind": "mark", "axis": "y", "at": 8.0, "words": "The most it turns down: 8 dB"}]));
    assert_eq!(d["words"], "Turns down by up to 8 dB while what it listens to plays");
    let d = draw(&runner, "duck", "down", json!({"depth": -40})).unwrap();
    assert_eq!(
        d["shapes"],
        json!([{"kind": "mark", "axis": "y", "at": 24.0, "words": "The most it turns down: 40 dB, past this picture's top (24 dB)"}])
    );
}

// CARD-96: adsr draws its envelope from its own sums, in seconds, its three corners moving its four
// values across from where they are (each a control in seconds, the sustain up and down); a value a
// cable sets greys it, saying which.
#[test]
fn adsr_draws_its_envelope_and_three_corners() {
    let runner = Runner::new();
    let v = json!({"attack": 0.01, "decay": 0.2, "sustain": 0.5, "release": 0.3});
    let d = draw(&runner, "adsr", "shape", v).unwrap();
    assert_eq!(kinds(&d), ["fill", "line", "handle", "handle", "handle"]);
    assert_eq!(dots(&d), ["x=attack", "x=decay y=sustain", "x=release"]);
    let line: Vec<f64> = d["shapes"][1]["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!((line[0], line[1]), (0.0, 0.0));
    // The hold is a quarter of the attack and decay, 50 ms at least: (0.01 + 0.2) / 4 = 0.0525 s.
    let end = 0.01 + 0.2 + 0.0525 + 0.3;
    assert!((line[line.len() - 2] - end).abs() < 1e-9 && line[line.len() - 1] == 0.0, "it ends at the release's end, in seconds");
    let xs: Vec<f64> = d["shapes"].as_array().unwrap()[2..].iter().map(|h| h["x"].as_f64().unwrap()).collect();
    assert!((xs[0] - 0.01).abs() < 1e-12 && (xs[1] - 0.21).abs() < 1e-12 && (xs[2] - end).abs() < 1e-9, "{xs:?}");
    assert_eq!(d["words"], "How loud a note is over time: attack 10 ms, decay 200 ms, sustain 0.5, release 300 ms");
    let why = draw(&runner, "adsr", "shape", json!({"attack": null, "decay": 0.2, "sustain": 0.5, "release": 0.3})).unwrap_err();
    assert!(why.contains("its attack is set by a cable or worked out by the code, so it can't be drawn"), "{why}");
}

// CARD-98: the LFO draws its shape over whole cycles, the fewest that reach a bar of 4/4, from its
// own sum, with a dot at the end of its first cycle that moves its period; so the song's beat, gone
// round the picture, is where it is, for any period (review S3).
#[test]
fn the_lfo_draws_whole_cycles_of_its_shape() {
    let runner = Runner::new();
    let d = draw(&runner, "lfo", "wave", json!({"period": 2, "phase": 0, "shape": "saw"})).unwrap();
    assert_eq!(kinds(&d), ["line", "handle"]);
    assert_eq!(dots(&d), ["x=period"]);
    let line: Vec<f64> = d["shapes"][0]["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!((line[0], line[1]), (0.0, 0.0));
    let at = line.chunks(2).find(|p| p[0] > 1.0).unwrap();
    assert!((at[1] - at[0] / 2.0).abs() < 1e-9, "a saw over 2 beats: {at:?}");
    assert_eq!(d["words"], "A saw wave from 0 to 1, once every 2 beats");
    for (period, span) in [(0.75, 4.5), (1.0, 4.0), (3.0, 6.0), (6.0, 6.0), (8.0, 8.0), (16.0, 16.0), (0.0625, 4.0)] {
        let d = draw(&runner, "lfo", "wave", json!({"period": period, "phase": 0.25, "shape": "sine"})).unwrap();
        let line: Vec<f64> = d["shapes"][0]["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let last = line[line.len() - 2];
        assert!((last - span).abs() < 1e-9, "period {period}: drawn over {last} beats, not {span}");
        // Whole cycles: at the picture's end it is where it starts, as the beat going round is.
        assert!(
            (line[line.len() - 1] - line[1]).abs() < 1e-6,
            "period {period}: {} at its end, {} at its start",
            line[line.len() - 1],
            line[1]
        );
        assert_eq!(d["shapes"][1]["x"].as_f64(), Some(period), "the dot at the end of its first cycle");
    }
}

// CARD-75, CARD-90: every picture std and music2's own packs draw does draw, at its defaults: each
// value as its control's default (a choice's name, a switch's true or false), each setting as its.
#[test]
fn every_bundled_picture_draws_at_its_defaults() {
    let packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/@m2");
    let mut packages = vec![("std".to_string(), std_dir())];
    for p in std::fs::read_dir(&packs).unwrap().flatten() {
        packages.push((format!("@m2/{}", p.file_name().to_string_lossy()), p.path()));
    }
    let catalog = m2_script::describe(&packages);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let runner = Runner::new();
    let (mut drew, mut wrong) = (0, vec![]);
    for (cid, d) in &catalog.components {
        for view in d.editor.views.iter().filter(|v| matches!(v.of, m2_script::component::ViewOf::Picture { .. })) {
            let mut values = serde_json::Map::new();
            for c in d.controls.iter().filter(|c| c.each.is_none()) {
                let at = c.default.as_ref().map_or(0.0, |s| s.value);
                let v = match c.unit {
                    Some(m2_ir::Unit::Switch) => json!(at != 0.0),
                    _ => match c.options() {
                        Some(names) => json!(names.name(at as usize).unwrap_or_default()),
                        None => json!(at),
                    },
                };
                values.insert(c.name.clone(), v);
            }
            for s in &d.settings {
                values.insert(
                    s.name.clone(),
                    s.default.clone().filter(|v| v.is_number() || v.is_string() || v.is_boolean()).unwrap_or(Value::Null),
                );
            }
            let ask = Ask {
                key: cid.split(':').next().unwrap().into(),
                packages: packages.clone(),
                song: None,
                module: d.module.clone(),
                descriptor: d.clone(),
                view: view.name.clone(),
                values: Value::Object(values).to_string(),
                memo: format!("{cid}\u{0}{}", view.name),
                rate: 48_000,
            };
            match runner.draw(ask) {
                Drawn::Shapes(_) => drew += 1,
                other => wrong.push(format!("{cid}'s {}: {other:?}", view.name)),
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(drew >= 30, "{drew} pictures drawn");
}

/// A test pack's sound whose envelope picture names `controls` past the types (`as any`).
fn wrong_envelope(controls: &str) -> String {
    format!(
        r#"import {{ component, control, kernel, port, type Audio }} from "@m2/core";
import {{ envelope, envelopePicture, envelopeWords }} from "std/pictures";
const N = {{ attack: "a", decay: "d", sustain: "s", release: "r" }} as any;
export const wrong = component(
  {{ id: "wrong", label: "Wrong", controls: {{ {controls} }}, output: port.audio(), editor: {{ views: {{ env: envelopePicture() }} }} }},
  () => kernel<Audio>("test:wrong", {{}}),
  {{ env: {{ draw: (v: any, p: any) => envelope(p, v, N), words: (v: any) => envelopeWords(v, N) }} }} as any,
);
"#
    )
}

// CARD-62, CARD-65, CARD-96 (review S1): an envelope whose values aren't in the units it draws,
// named past the types, is refused as it draws, in words naming the value: an attack in Hz or a
// decay in beats isn't a time in seconds, and a sustain that is a choice isn't a level.
#[test]
fn an_envelope_over_values_in_other_units_is_refused_in_words() {
    let runner = Runner::new();
    for (controls, values, says) in [
        (
            "a: control.hz(100), d: control.secs(0.1), s: control.frac(1), r: control.secs(0.1)",
            json!({"a": 100, "d": 0.1, "s": 1, "r": 0.1}),
            "moves a across, a control in hz, but its x axis is in seconds",
        ),
        (
            "a: control.secs(0.1), d: control.beats(1), s: control.frac(1), r: control.secs(0.1)",
            json!({"a": 0.1, "d": 1, "s": 1, "r": 0.1}),
            "moves d across, a control in beats, but its x axis is in seconds",
        ),
        (
            "a: control.secs(0.1), d: control.secs(0.1), s: control.choice([\"lo\", \"hi\"], \"lo\"), r: control.secs(0.1)",
            json!({"a": 0.1, "d": 0.1, "s": 0.5, "r": 0.1}),
            "moves s up and down",
        ),
    ] {
        let dir = std::env::temp_dir().join(format!("m2-std-pictures-wrong-{}-{}", std::process::id(), says.len()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/index.ts"), wrong_envelope(controls)).unwrap();
        let packages = vec![("@test/wrong".to_string(), dir.clone()), ("std".to_string(), std_dir())];
        let catalog = m2_script::describe(&packages);
        let descriptor = catalog.components.get("@test/wrong:wrong").cloned().unwrap_or_else(|| panic!("{:?}", catalog.problems));
        let ask = Ask {
            key: format!("wrong{}", says.len()),
            packages,
            song: None,
            module: descriptor.module.clone(),
            descriptor,
            view: "env".into(),
            values: values.to_string(),
            memo: "wrong\u{0}env".into(),
            rate: 48_000,
        };
        let why = match runner.draw(ask) {
            Drawn::Cant(why) => why,
            other => panic!("{controls}: it drew: {other:?}"),
        };
        assert!(why.contains(says), "{controls}: {why}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
