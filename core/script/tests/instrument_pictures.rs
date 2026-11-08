//! music2's own instruments' cards (CARD-130 to CARD-144): each picture draws at the values given,
//! with the dots and the words its rule says, as the picture runner draws it for a window or an
//! agent; and std's Limit and Master their ceiling. Whether a picture drawn from a kernel's own sums
//! is the sound is checked against a render in `crates/editor/src/instrument_pictures_tests.rs`.

use std::path::{Path, PathBuf};

use m2_script::pictures::{Ask, Drawn, Runner};
use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// std and music2's own packs, by name and folder.
fn packages() -> Vec<(String, PathBuf)> {
    let mut p = vec![("std".to_string(), root().join("std"))];
    for pack in ["drums", "synths", "wavetable", "vocals", "mixing", "effects"] {
        p.push((format!("@m2/{pack}"), root().join("packs/@m2").join(pack)));
    }
    p
}

/// The ask for picture `view` of the component `cid` at `values` (what plays, by name, CARD-63).
fn ask(cid: &str, view: &str, values: Value) -> Ask {
    let packages = packages();
    let catalog = m2_script::describe(&packages);
    let descriptor = catalog.components.get(cid).cloned().unwrap_or_else(|| panic!("no {cid}: {:?}", catalog.problems));
    Ask {
        key: cid.split(':').next().unwrap().into(),
        packages,
        song: None,
        module: descriptor.module.clone(),
        descriptor,
        view: view.into(),
        values: values.to_string(),
        memo: format!("{cid}\u{0}{view}"),
        rate: 48_000,
    }
}

/// Picture `view` of the component `cid`, drawn at `values`.
fn draw(runner: &Runner, cid: &str, view: &str, values: Value) -> Result<Value, String> {
    shapes(runner.draw(ask(cid, view, values)), cid, view)
}

fn shapes(drawn: Drawn, cid: &str, view: &str) -> Result<Value, String> {
    match drawn {
        Drawn::Shapes(json) => Ok(serde_json::from_str(&json).unwrap()),
        Drawn::Cant(why) => Err(why),
        other => panic!("{cid}'s {view}: {other:?}"),
    }
}

fn kinds(drawn: &Value) -> Vec<&str> {
    drawn["shapes"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect()
}

/// The shapes of `kind` a picture drew.
fn of<'a>(drawn: &'a Value, kind: &str) -> Vec<&'a Value> {
    drawn["shapes"].as_array().unwrap().iter().filter(|s| s["kind"] == kind).collect()
}

/// What each dot moves, by its `moves` names: `["x=cutoff", "y=q(knob)", …]`.
fn dots(drawn: &Value) -> Vec<String> {
    of(drawn, "handle")
        .iter()
        .map(|s| {
            let m = s["moves"].as_object().unwrap();
            let mut said: Vec<String> = m
                .iter()
                .filter(|(_, v)| v.get("name").is_some())
                .map(|(k, v)| format!("{k}={}{}", v["name"].as_str().unwrap(), if v.get("by").is_some() { "(knob)" } else { "" }))
                .collect();
            said.sort();
            said.join(" ")
        })
        .collect()
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

// CARD-131: the Wavetable's table A draws its frames faint, the one at its position as the line
// `frame`, and a dot on it at the position that moves the position across; with the LFO moving it,
// the frames at both ends of its reach and the line between them; and a file, which it can't read,
// only as the dot, saying so.
#[test]
fn the_wavetable_draws_its_table_with_the_lfos_reach() {
    let runner = Runner::new();
    let v = json!({"tableA": "vowels", "fileA": null, "positionA": 0.35, "lfoPosition": 0.3});
    let d = draw(&runner, "@m2/wavetable:wavetable", "wavesA", v).unwrap();
    assert_eq!(kinds(&d), ["part", "line", "line", "line", "line", "handle"]);
    assert_eq!(d["shapes"][0]["shapes"].as_array().unwrap().len(), 8, "eight frames behind");
    assert_eq!(d["shapes"][3]["name"], "reach");
    assert_eq!(d["shapes"][4]["name"], "frame");
    assert_eq!(dots(&d), ["x=positionA"]);
    let dot = &of(&d, "handle")[0];
    assert_eq!(dot["x"], 0.35, "the dot is at the position, so it moves across with it");
    let frame = floats(&d["shapes"][4]["points"]);
    assert_eq!(frame.len(), 2 * 65);
    assert!((frame[0] - 0.35 * 0.3).abs() < 1e-12, "the frame starts across at its position, as a stack does: {}", frame[0]);
    assert_eq!(d["words"], "Table A, vowels, at position 0.35: drag across to move through it. The LFO moves it from 0.05 to 0.65");
    let d = draw(
        &runner,
        "@m2/wavetable:wavetable",
        "wavesA",
        json!({"tableA": "basic", "fileA": "assets/table.wav", "positionA": 0.5, "lfoPosition": 0}),
    )
    .unwrap();
    assert_eq!(kinds(&d), ["handle"]);
    assert_eq!(d["words"], "Table A is your file, at position 0.5: this picture can't draw a file yet. Drag across to move through it");
    let d =
        draw(&runner, "@m2/wavetable:wavetable", "wavesB", json!({"tableB": "digital", "fileB": null, "positionB": 0, "lfoPosition": 0}))
            .unwrap();
    assert_eq!(dots(&d), ["x=positionB"]);
    assert_eq!(d["words"], "Table B, digital, at position 0: drag across to move through it");
}

// CARD-133, CARD-134: the Wavetable's filter draws its own shape, its dot at the cutoff, at its peak
// (a bandpass's 0 dB, a notch's at the foot); its LFO whole cycles of a sine reaching a second.
#[test]
fn the_wavetable_draws_its_filters_shape_and_its_lfo() {
    let runner = Runner::new();
    for (shape, y, said) in [
        ("lowpass", 20.0 * 2f64.log10(), "Cuts the highs above 1 kHz, q 2"),
        ("bandpass", 0.0, "Keeps a band around 1 kHz, q 2"),
        ("highpass", 20.0 * 2f64.log10(), "Cuts the lows below 1 kHz, q 2"),
        ("notch", -24.0, "Cuts a narrow band out at 1 kHz, q 2"),
    ] {
        let d = draw(&runner, "@m2/wavetable:wavetable", "curve", json!({"filter": shape, "cutoff": 1000, "q": 2})).unwrap();
        assert_eq!(kinds(&d), ["fill", "line", "handle"], "{shape}");
        assert_eq!(dots(&d), ["wheel=q x=cutoff y=q(knob)"]);
        let h = &of(&d, "handle")[0];
        assert!((h["y"].as_f64().unwrap() - y).abs() < 1e-9, "{shape}: {h}");
        assert!(d["words"].as_str().unwrap().starts_with(said), "{shape}: {}", d["words"]);
    }
    let d = draw(&runner, "@m2/wavetable:wavetable", "lfo", json!({"lfoRate": 2.5, "lfoPosition": 0.2, "lfoCutoff": 0})).unwrap();
    assert_eq!(kinds(&d), ["mark", "line"]);
    let line = floats(&d["shapes"][1]["points"]);
    // 2.5 Hz: three whole cycles reach a second, 1.2 s.
    assert!((line[line.len() - 2] - 1.2).abs() < 1e-9, "{}", line[line.len() - 2]);
    assert!(line[3] > 0.0, "a sine that rises first");
    assert_eq!(d["words"], "A sine from each note's start, once every 400 ms: it moves both positions 0.2 each way");
}

// CARD-136: the polysynth's sounds draw one note, measured, that a click plays, as long as its
// attack and decay and a little more (0.3 s to 1 s); and the polysynth its 24 dB ladder, its dot
// on the curve at the cutoff.
#[test]
fn the_polysynth_draws_one_note_and_its_ladder() {
    let runner = Runner::new();
    let d = draw(&runner, "@m2/synths:synth-custom", "note", json!({"attack": 0.003, "decay": 0.5})).unwrap();
    assert_eq!(kinds(&d), ["hit", "play"]);
    assert_eq!(of(&d, "hit")[0]["note"], 60);
    assert!((of(&d, "hit")[0]["seconds"].as_f64().unwrap() - 0.703).abs() < 1e-9);
    assert_eq!(of(&d, "play")[0]["note"], 60);
    assert_eq!(d["words"], "Middle C as this sound plays it, held 703 ms. Click it to hear it");
    let d = draw(&runner, "@m2/synths:synth-pad", "filter", json!({"cutoff": 1000, "resonance": 0})).unwrap();
    assert_eq!(kinds(&d), ["fill", "line", "handle"]);
    assert_eq!(dots(&d), ["wheel=resonance x=cutoff y=resonance(knob)"]);
    // At its cutoff a ladder with no resonance is 12 dB down: |(1 + i)⁴| = 4.
    let h = &of(&d, "handle")[0];
    assert!((h["y"].as_f64().unwrap() + 20.0 * 4f64.log10()).abs() < 1e-9, "{h}");
    let curve = floats(&d["shapes"][1]["points"]);
    let at = |hz: f64| curve.chunks(2).min_by(|a, b| (a[0] - hz).abs().total_cmp(&(b[0] - hz).abs())).unwrap().to_vec();
    // Three and four octaves above it: nearly 24 dB an octave.
    let ([f1, y1], [f2, y2]) = (<[f64; 2]>::try_from(at(8000.0)).unwrap(), <[f64; 2]>::try_from(at(16000.0)).unwrap());
    let slope = (y1 - y2) / (f2 / f1).log2();
    assert!((23.5..=24.0).contains(&slope), "{slope} dB an octave");
    assert_eq!(d["words"], "Cuts the highs above 1 kHz, 24 dB an octave, resonance 0, at middle C before its envelope opens it");
}

// CARD-140: the FM synth draws its two pairs' partials as bars, and a dot on each pair's first
// partial above its carrier that moves its modulator across and its amount as its knob.
#[test]
fn the_fm_synth_draws_its_partials() {
    let runner = Runner::new();
    let v = json!({"carrierA": 1, "modulatorA": 2, "amountA": 1.5, "carrierB": 1, "modulatorB": 14, "amountB": 0.8, "mixB": 0.35, "tone": 0.5, "feedback": 0});
    let d = draw(&runner, "@m2/synths:fm-custom", "partials", v).unwrap();
    assert_eq!(kinds(&d), ["bars", "handle", "bars", "handle"]);
    assert_eq!(dots(&d), ["wheel=amountA x=modulatorA y=amountA(knob)", "wheel=amountB x=modulatorB y=amountB(knob)"]);
    let [a, b] = [of(&d, "handle")[0], of(&d, "handle")[1]];
    assert_eq!((a["x"].as_f64(), a["mark"].as_str()), (Some(3.0), Some("A")), "at carrier + modulator");
    assert_eq!((b["x"].as_f64(), b["mark"].as_str()), (Some(15.0), Some("B")));
    let bars = floats(&d["shapes"][0]["points"]);
    // 1 bent by 2 holds only odd partials: 1, 3, 5 …
    assert!(bars.chunks(2).all(|p| (p[0] as i64) % 2 == 1), "{bars:?}");
    assert_eq!(
        d["words"],
        "The partials of middle C as it starts: pair A, 1 bent by 2, and pair B, 1 bent by 14 (−9.12 dB). Drag a dot across for its modulator, up for its amount"
    );
}

// CARD-139, CARD-140: the FM keys, bell and metal draw one note; the metal's as long as its decay.
#[test]
fn the_fm_sounds_draw_one_note() {
    let runner = Runner::new();
    for (cid, v) in [
        ("fm-keys", json!({"attack": 0.002, "decay": 3.5})),
        ("fm-bell", json!({"attack": 0.002, "decay": 4})),
        ("fm-metal", json!({"decay": 0.5})),
    ] {
        let d = draw(&runner, &format!("@m2/synths:{cid}"), "note", v).unwrap();
        assert_eq!(kinds(&d), ["hit", "play"], "{cid}");
    }
    let d = draw(&runner, "@m2/synths:fm-metal", "note", json!({"decay": 0.5})).unwrap();
    assert!((of(&d, "hit")[0]["seconds"].as_f64().unwrap() - 0.7).abs() < 1e-9);
}

// CARD-141: the Drum draws one hit at A2, measured, as long as its drum rings times its decay (a
// second at most); a place a click plays; and one dot, at its start, that moves the punch up and
// down, under the pointer. The decay is its knob's: no dot on a time axis moves a multiple under the
// pointer (review U8).
#[test]
fn the_drum_draws_its_hit() {
    let runner = Runner::new();
    let d = draw(&runner, "@m2/drums:drum", "hit", json!({"drum": "snare", "tune": 0, "decay": 2, "punch": 0.25, "drive": 0})).unwrap();
    assert_eq!(kinds(&d), ["hit", "play", "handle"]);
    assert_eq!(dots(&d), ["y=punch"]);
    let hit = &of(&d, "hit")[0];
    assert_eq!(hit["note"], 45);
    assert!((hit["seconds"].as_f64().unwrap() - 0.7).abs() < 1e-9, "a snare rings 0.35 s, times 2: {hit}");
    let start = of(&d, "handle")[0];
    assert_eq!((start["x"].as_f64(), start["y"].as_f64()), (Some(0.015), Some(0.25)));
    assert_eq!(d["words"], "Snare, one hit as it sounds: 700 ms. Click it to hear it; drag its start for the punch");
    let d = draw(&runner, "@m2/drums:drum", "hit", json!({"drum": "cymbal", "decay": 3, "punch": 0})).unwrap();
    assert!((of(&d, "hit")[0]["seconds"].as_f64().unwrap() - 1.0).abs() < 1e-9, "a second at most");
    assert_eq!(d["words"], "Cymbal, one hit as it sounds: 1 s of it. Click it to hear it; drag its start for the punch");
}

// CARD-143, CARD-144: std's Limit and Master draw their ceiling over their last seconds: the level
// going in and coming out, from −36 to +6 dB, a guide line at now and one at the ceiling, and a dot on
// the ceiling's line that moves it up and down. No line at 0 dB and no reduction above it: at the
// usual −1 dB the ceiling's line read as the 0 dB one (review U11).
#[test]
fn limit_and_master_draw_their_ceiling_to_drag() {
    let runner = Runner::new();
    let catalog = m2_script::describe(&packages());
    for (cid, said) in [
        ("std:limit", "Turns peaks down so they stay under −3 dB. Drag the line to move the ceiling"),
        ("std:master", "The song's last 3 seconds: no peak passes its ceiling, −3 dB. Drag the line to move it"),
    ] {
        let d = draw(&runner, cid, "level", json!({"ceiling": -3, "db": 0})).unwrap();
        assert_eq!(kinds(&d), ["mark", "mark", "handle"], "{cid}");
        assert_eq!(dots(&d), ["y=ceiling"]);
        let marks = of(&d, "mark");
        assert_eq!((&marks[0]["axis"], &marks[0]["at"]), (&json!("x"), &json!(3.0)));
        assert_eq!((&marks[1]["axis"], &marks[1]["at"]), (&json!("y"), &json!(-3.0)));
        assert_eq!(of(&d, "handle")[0]["y"], -3.0);
        assert_eq!(d["words"], said);
        let view = catalog.components[cid].editor.views.iter().find(|v| v.name == "level").unwrap();
        let m2_script::component::ViewOf::Picture { y, live, .. } = &view.of else { panic!("{cid}'s level is a picture") };
        assert_eq!((y.lo, y.hi), (-36.0, 6.0), "{cid}");
        let layers = serde_json::to_string(live).unwrap();
        assert!(!layers.contains("meter"), "{cid}: the levels in and out only: {layers}");
    }
}

// CARD-142: each of these cards names its face, and a picture on its face is on no tab,
// so no card draws one picture twice.
#[test]
fn a_face_picture_is_on_no_tab() {
    let catalog = m2_script::describe(&packages());
    let mut checked = 0;
    for cid in [
        "@m2/wavetable:wavetable",
        "@m2/synths:synth-custom",
        "@m2/synths:synth-supersaw",
        "@m2/synths:synth-lead",
        "@m2/synths:synth-pad",
        "@m2/synths:synth-pluck",
        "@m2/synths:synth-sub",
        "@m2/synths:synth-bass",
        "@m2/synths:synth-strings",
        "@m2/synths:fm-custom",
        "@m2/synths:fm-keys",
        "@m2/synths:fm-bell",
        "@m2/synths:fm-metal",
        "@m2/drums:drum",
        "std:limit",
        "std:master",
    ] {
        let d = catalog.components.get(cid).unwrap_or_else(|| panic!("no {cid}"));
        let (groups, named) = (&d.editor.groups, !d.editor.face.is_empty());
        // The face it names, else its first group (taste.md DV-12), which is then no tab.
        let face = if named { d.editor.face.clone() } else { groups.first().unwrap_or_else(|| panic!("{cid} has a face")).items.clone() };
        let tabs = if named { &groups[..] } else { &groups[1..] };
        let pictures: Vec<&String> = face.iter().filter(|n| d.editor.views.iter().any(|v| &v.name == *n)).collect();
        assert!(!pictures.is_empty(), "{cid}'s face has a picture");
        for g in tabs {
            for p in &pictures {
                assert!(!g.items.contains(p), "{cid}'s {p} is on its face and on its tab {}", g.name);
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 16);
}

// CARD-135, CARD-138, CARD-139, CARD-141, CARD-143, CARD-144: each card's face is the one its rule
// lists, in its order (review U6); the Drum's is its one group, Drum.
#[test]
fn each_face_is_the_one_its_rule_lists() {
    let catalog = m2_script::describe(&packages());
    let face = |cid: &str| {
        let d = catalog.components.get(cid).unwrap_or_else(|| panic!("no {cid}"));
        if d.editor.face.is_empty() { d.editor.groups[0].items.clone() } else { d.editor.face.clone() }
    };
    for (cid, want) in [
        ("@m2/wavetable:wavetable", &["wavesA", "positionA", "cutoff", "attack", "release"][..]),
        ("@m2/synths:synth-custom", &["note", "amp", "shape", "cutoff", "resonance", "attack", "release"]),
        ("@m2/synths:synth-supersaw", &["filter", "amp", "shape", "detune", "spread", "cutoff"]),
        ("@m2/synths:synth-lead", &["filter", "amp", "shape", "glide", "vibrato", "cutoff"]),
        ("@m2/synths:synth-pad", &["filter", "amp", "shape", "spread", "cutoff"]),
        ("@m2/synths:synth-strings", &["filter", "amp", "shape", "ensemble", "cutoff"]),
        ("@m2/synths:synth-pluck", &["note", "amp", "shape", "brightness", "spread"]),
        ("@m2/synths:synth-sub", &["note", "amp", "drop", "drive", "glide"]),
        ("@m2/synths:synth-bass", &["note", "amp", "shape", "glide"]),
        ("@m2/synths:fm-custom", &["partials", "amp", "amountA", "amountB", "mixB", "tone"]),
        ("@m2/synths:fm-keys", &["note", "amp", "tone", "tine"]),
        ("@m2/synths:fm-bell", &["note", "amp", "tone"]),
        ("@m2/synths:fm-metal", &["note", "tone", "feedback", "drop"]),
        ("@m2/drums:drum", &["hit", "drum", "tune", "decay", "punch", "drive"]),
        ("std:limit", &["level", "ceiling"]),
        ("std:master", &["level", "db", "ceiling"]),
    ] {
        assert_eq!(face(cid), want, "{cid}'s face");
    }
}

/// The time a picture's draw takes, its first in a fresh runtime (`warm`, a cheap picture of the same
/// package, loads the package's code first, which has its own limit): the median of five fresh
/// runners, each the CPU time of the runtime's thread for the draw and Rust's checks of its shapes
/// (`Runner::draw_timed`, CARD-75). Not the test's own setup: the ask is made first, reading every
/// pack's manifest (about 1.5 ms here, which the wall clock counted before), nor the wait for a CPU.
fn first_draw(warm: (&str, &str, Value), cid: &str, view: &str, values: &Value) -> std::time::Duration {
    let mut times: Vec<std::time::Duration> = (0..5)
        .map(|_| {
            let runner = Runner::new();
            draw(&runner, warm.0, warm.1, warm.2.clone()).unwrap_or_else(|why| panic!("{}'s {}: {why}", warm.0, warm.1));
            let asked = ask(cid, view, values.clone());
            let (drawn, took) = runner.draw_timed(asked);
            shapes(drawn, cid, view).unwrap_or_else(|why| panic!("{cid}'s {view}: {why}"));
            took
        })
        .collect();
    times.sort();
    times[2]
}

// CARD-75: every picture of music2's own instruments, and std's ceiling, draws in a fifth of the
// runner's 20 ms limit for a draw, its first in a fresh runtime included, so a slow or busy machine
// never stops it (the Wavetable's table was stopped on CI's runners). It judges speed, so it runs in
// the nightly tier (ci/nightly-tests.txt).
#[test]
fn every_instrument_picture_draws_in_a_fifth_of_its_limit() {
    // 4 ms on the Linux server, scaled by this machine's speed (CARD-75).
    let budget = m2_script::speed::scaled(std::time::Duration::from_millis(4));
    let mut said = vec![];
    let mut slow = vec![];
    let envelope = json!({"attack": 0.01, "decay": 0.3, "sustain": 0.5, "release": 0.3});
    let mut time = |cid: &str, warm: (&str, &str, Value), view: &str, values: Value| {
        let t = first_draw(warm, cid, view, &values);
        said.push(format!("{cid} {view} {values}: {:.2} ms", t.as_secs_f64() * 1e3));
        if t > budget {
            slow.push(said.last().unwrap().clone());
        }
    };
    let wt = "@m2/wavetable:wavetable";
    for table in ["basic", "pulse", "vowels", "harmonics", "digital"] {
        time(wt, (wt, "amp", envelope.clone()), "wavesA", json!({"tableA": table, "fileA": null, "positionA": 0.37, "lfoPosition": 0.3}));
    }
    time(wt, (wt, "amp", envelope.clone()), "wavesB", json!({"tableB": "harmonics", "fileB": null, "positionB": 0.81, "lfoPosition": 0.5}));
    time(wt, (wt, "amp", envelope.clone()), "curve", json!({"filter": "bandpass", "cutoff": 1000, "q": 2}));
    time(wt, (wt, "amp", envelope.clone()), "lfo", json!({"lfoRate": 20, "lfoPosition": 0.2, "lfoCutoff": 1}));
    time("@m2/synths:synth-custom", ("@m2/synths:synth-custom", "amp", envelope.clone()), "note", json!({"attack": 0.003, "decay": 0.5}));
    time(
        "@m2/synths:synth-custom",
        ("@m2/synths:synth-custom", "amp", envelope.clone()),
        "filter",
        json!({"cutoff": 1000, "resonance": 0.9}),
    );
    time(
        "@m2/synths:fm-custom",
        ("@m2/synths:fm-custom", "amp", envelope.clone()),
        "partials",
        json!({"carrierA": 1, "modulatorA": 1, "amountA": 8, "carrierB": 3, "modulatorB": 0.5, "amountB": 8, "mixB": 0.5, "tone": 1, "feedback": 0}),
    );
    // The Drum's one picture keeps nothing between draws: one at other values loads its code.
    time(
        "@m2/drums:drum",
        ("@m2/drums:drum", "hit", json!({"drum": "kick", "decay": 1, "punch": 0})),
        "hit",
        json!({"drum": "cymbal", "decay": 3, "punch": 0.5}),
    );
    // Each of std's modules loads at its first picture: one of the same module, or the same at other values.
    time("std:limit", ("std:compress", "history", json!({})), "level", json!({"ceiling": -1}));
    time("std:master", ("std:master", "level", json!({"ceiling": -3, "db": 0})), "level", json!({"ceiling": -1, "db": 0}));
    eprintln!("{}", said.join("\n"));
    assert!(
        slow.is_empty(),
        "past {budget:?}, the Linux server's 4 ms × {:.2} for this machine:\n{}",
        m2_script::speed::ratio(),
        slow.join("\n")
    );
}
