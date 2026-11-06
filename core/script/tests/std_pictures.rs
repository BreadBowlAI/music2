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
// rides), a faint line where in is out, a dot at the threshold (across, the ratio by the wheel) and
// one at the curve's end (the makeup up and down, the ratio by the wheel); and how far it turns
// down, from its meter, beside it.
#[test]
fn compress_draws_its_transfer_curve_and_how_far_it_turns_down() {
    let runner = Runner::new();
    let v = json!({"threshold": -18, "ratio": 4, "attack": 0.01, "release": 0.15, "makeup": 3});
    let d = draw(&runner, "compress", "transfer", v.clone()).unwrap();
    assert_eq!(kinds(&d), ["line", "line", "handle", "handle"]);
    assert_eq!(d["shapes"][1]["name"], "transfer");
    assert_eq!(dots(&d), ["wheel=ratio x=threshold", "wheel=ratio y=makeup"]);
    let points: Vec<f64> = d["shapes"][1]["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    // Below the knee it is a straight line up by the makeup; above it 1 dB out for each 4 in.
    let at = |level: f64| points.chunks(2).find(|p| (p[0] - level).abs() < 1e-9).unwrap()[1];
    assert!((at(-40.0) - (-37.0)).abs() < 1e-9);
    assert!((at(0.0) - (-18.0 + 18.0 / 4.0 + 3.0)).abs() < 1e-9);
    assert_eq!(d["words"], "Above −18 dB the level rises only 1 dB for each 4, then up 3 dB");
    let down = draw(&runner, "compress", "down", v).unwrap();
    assert_eq!(kinds(&down), Vec::<&str>::new());
    assert_eq!(down["words"], "How far it turned the sound down in the last 3 seconds");
}

// CARD-94, CARD-95: limit and duck draw how far they turn down, from their meters; the duck's most
// as a guide line.
#[test]
fn limit_and_duck_draw_how_far_they_turn_down() {
    let runner = Runner::new();
    let d = draw(&runner, "limit", "down", json!({"ceiling": -1})).unwrap();
    assert_eq!(kinds(&d), Vec::<&str>::new());
    assert_eq!(d["words"], "Turns peaks down so they stay under −1 dB");
    let d = draw(&runner, "duck", "down", json!({"depth": -8})).unwrap();
    assert_eq!(d["shapes"], json!([{"kind": "mark", "axis": "y", "at": 8.0, "words": "The most it turns down: 8 dB"}]));
    assert_eq!(d["words"], "Turns down by up to 8 dB while what it listens to plays");
}

// CARD-96: adsr draws its envelope from its own sums across the whole picture, its three corners
// moving its four values; a value a cable sets greys it, saying which.
#[test]
fn adsr_draws_its_envelope_and_three_corners() {
    let runner = Runner::new();
    let v = json!({"attack": 0.01, "decay": 0.2, "sustain": 0.5, "release": 0.3});
    let d = draw(&runner, "adsr", "shape", v).unwrap();
    assert_eq!(kinds(&d), ["fill", "line", "handle", "handle", "handle"]);
    assert_eq!(dots(&d), ["x=attack(knob)", "x=decay(knob) y=sustain", "x=release(knob)"]);
    let line: Vec<f64> = d["shapes"][1]["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!((line[0], line[1]), (0.0, 0.0));
    assert_eq!((line[line.len() - 2], line[line.len() - 1]), (1.0, 0.0), "it ends at the picture's end");
    // The hold is a quarter of the rest: (0.01 + 0.2 + 0.3) / 4.
    let total = 0.01 + 0.2 + 0.1275 + 0.3;
    assert!((d["shapes"][2]["x"].as_f64().unwrap() - 0.01 / total).abs() < 1e-9);
    assert_eq!(d["words"], "How loud a note is over time: attack 10 ms, decay 200 ms, sustain 0.5, release 300 ms");
    let why = draw(&runner, "adsr", "shape", json!({"attack": null, "decay": 0.2, "sustain": 0.5, "release": 0.3})).unwrap_err();
    assert!(why.contains("its attack is set by a cable or worked out by the code, so it can't be drawn"), "{why}");
}

// CARD-98: the LFO draws its shape over one bar of 4 beats, from its own sum, with a dot at the end
// of its first cycle that moves its period.
#[test]
fn the_lfo_draws_its_shape_over_one_bar() {
    let runner = Runner::new();
    let d = draw(&runner, "lfo", "wave", json!({"period": 2, "phase": 0, "shape": "saw"})).unwrap();
    assert_eq!(kinds(&d), ["line", "handle"]);
    assert_eq!(dots(&d), ["x=period"]);
    let line: Vec<f64> = d["shapes"][0]["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!((line[0], line[1]), (0.0, 0.0));
    let at = line.chunks(2).find(|p| p[0] > 1.0).unwrap();
    assert!((at[1] - at[0] / 2.0).abs() < 1e-9, "a saw over 2 beats: {at:?}");
    assert_eq!(d["words"], "A saw wave from 0 to 1, once every 2 beats");
}
