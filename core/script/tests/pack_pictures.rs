//! `@m2/mixing`'s and `@m2/effects`' cards' pictures (CARD-110 to CARD-124): each draws, from the
//! pack's own code, with the dots and the words its rule says, as the picture runner draws it for a
//! window or an agent. Whether a picture drawn from a pack's own sums is the sound is checked
//! against a render in `crates/editor/src/pack_pictures_tests.rs` (CARD-99).

use std::path::{Path, PathBuf};

use m2_script::pictures::{Ask, Drawn, Runner};
use serde_json::{Value, json};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Picture `view` of `@m2/<pack>`'s `component`, drawn at `values` (what plays, by name, CARD-63).
fn draw(runner: &Runner, pack: &str, component: &str, view: &str, values: Value) -> Result<Value, String> {
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

/// The handles a picture drew.
fn handles(drawn: &Value) -> Vec<&Value> {
    drawn["shapes"].as_array().unwrap().iter().filter(|s| s["kind"] == "handle").collect()
}

/// What each dot moves, by its `moves` names: `["x=freq1 y=q1(knob)", …]`.
fn dots(drawn: &Value) -> Vec<String> {
    handles(drawn)
        .iter()
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

/// Where each dot is drawn.
fn places(drawn: &Value) -> Vec<(f64, f64)> {
    handles(drawn).iter().map(|h| (h["x"].as_f64().unwrap(), h["y"].as_f64().unwrap())).collect()
}

/// The points of the line `name` a picture drew, as pairs.
fn line(drawn: &Value, name: &str) -> Vec<(f64, f64)> {
    let s = drawn["shapes"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("no line {name}: {drawn}"));
    s["points"].as_array().unwrap().chunks(2).map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect()
}

fn close(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
}

/// The EQ's values: its defaults, with `changes`.
fn eq_values(changes: Value) -> Value {
    let mut v = json!({});
    let defaults =
        [("low cut", 30.0, 0.71, false), ("low shelf", 100.0, 0.71, true), ("bell", 250.0, 1.0, true), ("bell", 1000.0, 1.0, true)];
    let more =
        [("bell", 2500.0, 1.0, true), ("bell", 6000.0, 1.0, true), ("high shelf", 10000.0, 0.71, true), ("high cut", 18000.0, 0.71, false)];
    for (i, (shape, freq, q, on)) in defaults.iter().chain(more.iter()).enumerate() {
        let b = i + 1;
        v[format!("shape{b}")] = json!(shape);
        v[format!("freq{b}")] = json!(freq);
        v[format!("gain{b}")] = json!(0);
        v[format!("q{b}")] = json!(q);
        v[format!("on{b}")] = json!(on);
    }
    for (k, x) in changes.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

// CARD-111: the Parametric EQ draws its curve measured from its own sound and a dot for each band,
// numbered 1 to 8 in one colour (D14): across its frequency, up and down its gain (a cut's q, as
// its knob, at its peak), the wheel its q, a double-click its on, picked its tab; a band that is off
// hollow. Its words say how many bands are on and what each that changes the sound does.
#[test]
fn the_eq_draws_its_measured_curve_and_a_dot_for_each_band() {
    let runner = Runner::new();
    let d = draw(&runner, "mixing", "eq", "curve", eq_values(json!({"gain4": -3.5, "freq4": 800, "q1": 2, "on1": true}))).unwrap();
    assert_eq!(kinds(&d), ["response", "handle", "handle", "handle", "handle", "handle", "handle", "handle", "handle"]);
    assert_eq!(d["shapes"][0]["name"], "curve");
    let dots = dots(&d);
    assert_eq!(dots[0], "toggles=on1 wheel=q1 x=freq1 y=q1(knob)", "a cut's dot moves its q up and down");
    assert_eq!(dots[3], "toggles=on4 wheel=q4 x=freq4 y=gain4");
    assert_eq!(dots[7], "toggles=on8 wheel=q8 x=freq8 y=q8(knob)");
    let h = handles(&d);
    for (b, dot) in h.iter().enumerate() {
        assert_eq!((dot["mark"].as_str(), dot["opens"].as_str()), (Some(&*format!("{}", b + 1)), Some(&*format!("Band {}", b + 1))));
    }
    assert!(close(places(&d)[0], (30.0, 20.0 * 2f64.log10())), "a cut's dot at its peak: {:?}", places(&d)[0]);
    assert!(close(places(&d)[3], (800.0, -3.5)));
    assert_eq!(
        h.iter().map(|x| x["off"] == true).collect::<Vec<_>>(),
        [false, false, false, false, false, false, false, true],
        "band 8 is off"
    );
    assert_eq!(d["words"], "7 bands on: band 1 cuts the lows below 30 Hz; band 4 bell −3.5 dB at 800 Hz");
    let flat = draw(&runner, "mixing", "eq", "curve", eq_values(json!({}))).unwrap();
    assert_eq!(flat["words"], "6 bands on, all at 0 dB: the sound as it is");
}

// CARD-112: the gate draws its levels over the last 3 seconds with the line where it opens, and a
// dot at its right end, now, that moves the threshold up and down.
#[test]
fn the_gate_draws_where_it_opens_over_its_levels() {
    let runner = Runner::new();
    let v = json!({"threshold": -40, "attack": 0.001, "hold": 0.05, "release": 0.1, "range": -80});
    let d = draw(&runner, "mixing", "gate", "levels", v).unwrap();
    assert_eq!(kinds(&d), ["line", "handle"]);
    assert_eq!(line(&d, "threshold"), [(0.0, -40.0), (3.0, -40.0)]);
    assert_eq!(dots(&d), ["y=threshold"]);
    assert!(close(places(&d)[0], (2.8, -40.0)), "near its right end, now");
    assert_eq!(d["words"], "Opens when the sound reaches −40 dB; below that it turns it down to −80 dB. Drag the line");
}

// CARD-113: the transient shaper draws one hit, before (faint) and after (the line `after`), from
// the kernel's own sums: early in the hit all of `attack` applies, on its tail all of `sustain`;
// each dot sits on the line where its value applies in full, and moves it up and down.
#[test]
fn the_transient_shaper_draws_one_hit_before_and_after() {
    let runner = Runner::new();
    let plain = draw(&runner, "mixing", "transient", "hit", json!({"attack": 0, "sustain": 0})).unwrap();
    let d = draw(&runner, "mixing", "transient", "hit", json!({"attack": 9, "sustain": -6})).unwrap();
    assert_eq!(kinds(&d), ["fill", "line", "handle", "handle"]);
    assert_eq!(dots(&d), ["y=attack", "y=sustain"]);
    let (before, after) = (line(&plain, "after"), line(&d, "after"));
    assert_eq!(before.len(), 400, "a point each millisecond from the first to 0.4 s");
    let at = |l: &[(f64, f64)], t: f64| l.iter().find(|p| (p.0 - t).abs() < 1e-9).unwrap().1;
    assert!((at(&after, 0.005) - at(&before, 0.005) - 9.0).abs() < 1e-6, "its start is up 9 dB");
    assert!((at(&after, 0.1) - at(&before, 0.1) + 6.0).abs() < 1e-6, "its tail is down 6 dB");
    let p = places(&d);
    assert!(close(p[0], (0.005, at(&after, 0.005))) && close(p[1], (0.1, at(&after, 0.1))), "{p:?}");
    // Unchanged where it is neither: a hit's peak, once the start has passed, is as it was there.
    assert_eq!(d["words"], "One hit, before (faint) and after: its start up 9 dB, its tail down 6 dB");
}

// CARD-114 (review T4): the de-esser draws the band it listens to over the spectrum, a box over the
// whole height between the points its Q of 1 is 3 dB down, and no line on the spectrum's scale: the
// kernel compares the band's peaks with its threshold, which no bar of the spectrum reads, so the
// threshold is in its words and on its dot's knob. The dot, in the box's middle, moves the frequency
// across and the threshold as its knob does.
#[test]
fn the_deesser_draws_its_band_over_the_spectrum() {
    let runner = Runner::new();
    let d = draw(&runner, "mixing", "deesser", "band", json!({"freq": 6000, "threshold": -30, "amount": 6})).unwrap();
    assert_eq!(kinds(&d), ["fill", "handle"]);
    assert_eq!(dots(&d), ["x=freq y=threshold(knob)"]);
    assert!(close(places(&d)[0], (6000.0, -45.0)), "in the box's middle: {:?}", places(&d)[0]);
    let box_: Vec<(f64, f64)> =
        d["shapes"][0]["points"].as_array().unwrap().chunks(2).map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
    let (lo, hi) = (box_.iter().map(|p| p.0).fold(f64::MAX, f64::min), box_.iter().map(|p| p.0).fold(0.0, f64::max));
    let (foot, top) = (box_.iter().map(|p| p.1).fold(f64::MAX, f64::min), box_.iter().map(|p| p.1).fold(f64::MIN, f64::max));
    assert!((lo * hi - 6000.0 * 6000.0).abs() < 1e-3 && (hi - lo - 6000.0).abs() < 1e-6, "a Q of 1: {lo} to {hi}");
    assert_eq!((foot, top), (-90.0, 0.0), "the whole height: no level on the spectrum's scale");
    assert_eq!(
        d["words"],
        "Listens from 3.71 kHz to 9.71 kHz: when that band's peaks go over −30 dB, it turns the band down by as much, up to 6 dB"
    );
}

// CARD-115: the multiband draws its split: how it turns each frequency below its thresholds,
// measured with a click at −80 dB (the line `split`, a response the server fills in, whose curve
// `the_multiband_split_is_measured_under_its_thresholds` checks), the crossovers as guide lines
// with a dot each that moves it across, and a dot in each band that moves its gain and shows its
// tab; crossed, the lower one splits low from mid, as the kernel does.
#[test]
fn the_multiband_draws_its_split() {
    let runner = Runner::new();
    let mut v = json!({"lowMid": 200, "midHigh": 2500, "lowGain": 0, "midGain": 0, "highGain": 0});
    for band in ["low", "mid", "high"] {
        for (k, x) in [("Threshold", -18.0), ("Ratio", 2.0), ("Attack", 0.01), ("Release", 0.1)] {
            v[format!("{band}{k}")] = json!(x);
        }
    }
    (v["lowGain"], v["midGain"], v["highGain"]) = (json!(3), json!(-2), json!(0));
    let d = draw(&runner, "mixing", "multiband", "split", v.clone()).unwrap();
    assert_eq!(kinds(&d), ["response", "mark", "mark", "handle", "handle", "handle", "handle", "handle"]);
    let split = &d["shapes"][0];
    assert_eq!(
        (&split["name"], &split["level"], &split["points"]),
        (&json!("split"), &json!(-80.0), &Value::Null),
        "measured, under its thresholds"
    );
    assert_eq!(dots(&d), ["x=lowMid", "x=midHigh", "y=lowGain", "y=midGain", "y=highGain"]);
    let opens: Vec<&str> = handles(&d).iter().filter_map(|h| h["opens"].as_str()).collect();
    assert_eq!(opens, ["Low", "Mid", "High"]);
    let mid = places(&d)[3];
    assert!(close(mid, ((200.0f64 * 2500.0).sqrt(), -2.0)), "the mid's dot in its middle, at its gain: {mid:?}");
    assert_eq!(
        d["words"],
        "Lows below 200 Hz, mids to 2.5 kHz, highs above, each compressed on its own, then turned 3 dB, −2 dB and 0 dB: as it sounds while quieter than its thresholds"
    );
    v["lowMid"] = json!(3000);
    let crossed = draw(&runner, "mixing", "multiband", "split", v).unwrap();
    let marks: Vec<f64> =
        crossed["shapes"].as_array().unwrap().iter().filter(|s| s["kind"] == "mark").map(|s| s["at"].as_f64().unwrap()).collect();
    assert_eq!(marks, [2500.0, 3000.0], "crossed, the lower splits low from mid");
}

// CARD-116: Pump draws one bar of its dip from its own sum: down `depth` at each step's start and
// back to 0 dB at its end, at its rate; a dot at the first step's foot moves the depth, one on its
// curve a third of the way through moves the shape as its knob does and by the wheel.
#[test]
fn pump_draws_its_dip_over_a_bar() {
    let runner = Runner::new();
    let d = draw(&runner, "mixing", "pump", "dip", json!({"depth": -8, "shape": 0.5, "rate": "1/8"})).unwrap();
    assert_eq!(kinds(&d), ["fill", "line", "handle", "handle"]);
    assert_eq!(dots(&d), ["y=depth", "wheel=shape y=shape(knob)"]);
    let dip = line(&d, "dip");
    assert_eq!(dip.first(), Some(&(0.0, -8.0)));
    assert_eq!(dip.last(), Some(&(4.0, 0.0)), "a bar of 4 beats");
    assert_eq!(dip.iter().filter(|p| p.1 == -8.0).count(), 8, "eight dips of an eighth note in a bar");
    let third = -8.0 * (0.5 * (2.0 / 3.0) + 0.5 * (2.0f64 / 3.0).powi(4));
    let p = places(&d);
    assert!(close(p[0], (0.5, -8.0)) && close(p[1], (0.5 + 0.5 / 3.0, third)), "on its second step: {p:?}");
    assert_eq!(d["words"], "Dips 8 dB every 1/8 note and comes back steadily. Drag the foot for the depth, the curve for the shape");
}

// CARD-117: the delay draws its click and its echoes as bars down to −60 dB, from its own sum, a dot
// on the first echo (its time across, its mix as its knob) and one on the second (its feedback).
#[test]
fn the_delay_draws_its_echoes() {
    let runner = Runner::new();
    let d = draw(&runner, "effects", "delay", "echoes", json!({"time": 0.25, "feedback": 0.5, "mix": 0.5})).unwrap();
    assert_eq!(kinds(&d), ["bars", "bars", "handle", "handle"]);
    assert_eq!(dots(&d), ["x=time y=mix(knob)", "wheel=feedback y=feedback(knob)"]);
    let bars: Vec<f64> = d["shapes"][1]["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    // 0.5, 0.25, …: 6 dB down each, to −60 dB.
    assert_eq!(bars.len() / 2, 9);
    assert!((bars[0] - 0.25).abs() < 1e-12 && (bars[1] - 20.0 * 0.5f64.log10()).abs() < 1e-9);
    assert!((bars[3] - 20.0 * 0.25f64.log10()).abs() < 1e-9);
    assert!(close(places(&d)[1], (0.5, 20.0 * 0.25f64.log10())));
    assert_eq!(
        d["words"],
        "An echo every 250 ms, each 6.02 dB quieter: 9 echoes before −60 dB. Drag the first for its time and mix, the second for the feedback"
    );
}

// CARD-118: the reverb draws its tail from its own sums, from 0 dB down to −60 dB, falling for ever,
// longer the bigger it is and sooner the more it is damped; a dot where it has fallen 30 dB moves
// the size as its knob does.
#[test]
fn the_reverb_draws_its_tail() {
    let runner = Runner::new();
    let tail = |size: f64, damping: f64| {
        draw(&runner, "effects", "reverb", "tail", json!({"size": size, "damping": damping, "mix": 0.25})).unwrap()
    };
    let d = tail(0.6, 0.5);
    assert_eq!(kinds(&d), ["fill", "line", "handle"]);
    assert_eq!(dots(&d), ["wheel=size x=size(knob)"]);
    let l = line(&d, "tail");
    assert_eq!(l[0], (0.0, 0.0));
    assert!(l.windows(2).all(|w| w[1].1 <= w[0].1), "it only falls");
    assert!((l.last().unwrap().1 + 60.0).abs() < 0.1, "it ends where it has fallen 60 dB: {:?}", l.last());
    let at30 = |d: &Value| places(d)[0].0;
    assert!(at30(&tail(0.9, 0.5)) > at30(&d) && at30(&tail(0.6, 1.0)) < at30(&d), "bigger is longer, damped is shorter");
    assert!(d["words"].as_str().unwrap().starts_with("Its tail fades 30 dB in "), "{}", d["words"]);
}

// CARD-119 to CARD-121: the moving effects draw a second of their motion from their own sums: the
// chorus's two copies drifting apart, the phaser's three notches round its sweep, the tremolo's
// level; none has a dot.
#[test]
fn the_moving_effects_draw_a_second_of_their_motion() {
    let runner = Runner::new();
    let d = draw(&runner, "effects", "chorus", "motion", json!({"rate": 1, "depth": 0.5, "mix": 0.5})).unwrap();
    assert_eq!(kinds(&d), ["line", "line"]);
    let (one, two) = (line(&d, "one"), line(&d, "two"));
    assert!(close(one[0], (0.0, 0.0095)) && close(two[0], (0.0, 0.0095)), "both at the middle as the song starts");
    let quarter = one.iter().position(|p| (p.0 - 0.25).abs() < 1e-9).unwrap();
    assert!((one[quarter].1 - 0.011).abs() < 1e-6 && (two[quarter].1 - 0.008).abs() < 1e-6, "{:?} {:?}", one[quarter], two[quarter]);
    let d = draw(&runner, "effects", "phaser", "motion", json!({"rate": 1, "depth": 0.4, "feedback": 0.5, "mix": 0.5})).unwrap();
    assert_eq!(kinds(&d), ["line", "line", "line"]);
    let mid = line(&d, "mid");
    assert!((mid[0].1 - 640.0).abs() < 1e-6, "the middle notch at the sweep's centre as the song starts: {:?}", mid[0]);
    let top = mid.iter().map(|p| p.1).fold(0.0, f64::max);
    assert!((top / 640.0).log2() > 0.99 && (top / 640.0).log2() < 1.01, "an octave up at depth 0.4: {top}");
    let d = draw(&runner, "effects", "tremolo", "motion", json!({"rate": 2, "depth": 0.5})).unwrap();
    assert_eq!(kinds(&d), ["fill", "line"]);
    let level = line(&d, "level");
    let low = level.iter().map(|p| p.1).fold(1.0, f64::min);
    assert!((level[0].1 - 1.0).abs() < 1e-12 && (low - 0.5).abs() < 1e-6);
    assert_eq!(d["words"], "The level dips to 0.5 and back, 2 times a second");
}

// CARD-122, CARD-123: drive draws its curve from its own sum, a dot a quarter of the way in moving
// the amount as its knob; the bitcrusher a cycle of a 1 kHz sine as it plays it, a step each sample.
#[test]
fn drive_and_bitcrush_draw_their_curves() {
    let runner = Runner::new();
    let d = draw(&runner, "effects", "drive", "curve", json!({"amount": 12})).unwrap();
    assert_eq!(kinds(&d), ["line", "line", "handle"]);
    assert_eq!(dots(&d), ["wheel=amount y=amount(knob)"]);
    let g = 10f64.powf(12.0 / 20.0);
    let at = |x: f64| (x * g).tanh() / g.tanh();
    assert!(line(&d, "curve").iter().all(|p| (p.1 - at(p.0)).abs() < 1e-9));
    assert!(close(places(&d)[0], (0.25, at(0.25))));
    let d = draw(&runner, "effects", "bitcrush", "crush", json!({"bits": 2, "rate": 12000, "mix": 1})).unwrap();
    assert_eq!(kinds(&d), ["line", "line"]);
    let held = line(&d, "crushed");
    assert_eq!(held.len(), 96, "48 samples, each held");
    // Two bits: 0, ±0.5, ±1. Held for four samples at 12 kHz.
    assert!(held.iter().all(|p| [0.0, 0.5, 1.0].contains(&p.1.abs())), "{held:?}");
    assert_eq!(held[1].1, held[7].1);
    assert_eq!(d["words"], "A 1 kHz sine, rounded to 2 bits and held 12 thousand times a second");
}

// CARD-124 (review T3): widen draws a fan as wide as its width either side, how far apart it spreads
// a stereo sound's left and right, the faint lines where 1 is, and a dot at its right edge that
// moves the width across; above width 1, a second fan, `made`, the width it makes up for a mono
// sound from its sound above 300 Hz: 0.6 × (width − 1). Its words say both, with no jargon.
#[test]
fn widen_draws_its_stereo_field() {
    let runner = Runner::new();
    let d = draw(&runner, "effects", "widen", "field", json!({"width": 1.5})).unwrap();
    assert_eq!(kinds(&d), ["line", "line", "fill", "line", "line", "handle"]);
    assert_eq!(dots(&d), ["x=width"]);
    assert_eq!(line(&d, "field"), [(-1.5, 0.85), (0.0, 0.0), (1.5, 0.85)]);
    let made = line(&d, "made");
    assert!(close(made[0], (-0.3, 0.85)) && close(made[2], (0.3, 0.85)), "{made:?}");
    assert!(close(places(&d)[0], (1.5, 0.85)));
    assert_eq!(
        d["words"],
        "Width 1.5: a stereo sound's left and right spread 1.5 times as far apart; a mono sound gets a made-up width of 0.3 from its sound above 300 Hz (the faint fan). Drag the edge"
    );
    let narrow = draw(&runner, "effects", "widen", "field", json!({"width": 0.5})).unwrap();
    assert_eq!(kinds(&narrow), ["line", "line", "fill", "line", "handle"], "no made-up width at or under 1");
    assert_eq!(
        narrow["words"],
        "Width 0.5: a stereo sound's left and right come 0.5 times as far apart, toward mono; a mono sound stays as it is. Drag the edge"
    );
}

// CARD-110: a picture whose values a cable sets greys, saying which, as std's do.
#[test]
fn a_value_set_elsewhere_greys_the_picture_in_words() {
    let runner = Runner::new();
    let why = draw(&runner, "mixing", "pump", "dip", json!({"depth": null, "shape": 0.5, "rate": "1/4"})).unwrap_err();
    assert!(why.contains("its depth is set by a cable or worked out by the code, so it can't be drawn"), "{why}");
    let why = draw(&runner, "effects", "delay", "echoes", json!({"time": 0.25, "feedback": null, "mix": 0.3})).unwrap_err();
    assert!(why.contains("its feedback is set by a cable"), "{why}");
    // Review T8: a rate the code works out greys Pump's picture, never a 1/4 note drawn in its place.
    let why = draw(&runner, "mixing", "pump", "dip", json!({"depth": -8, "shape": 0.5, "rate": null})).unwrap_err();
    assert!(why.contains("its rate is set by a cable or worked out by the code, so it can't be drawn"), "{why}");
}

// CARD-117 (review T9): a delay at mix 0 still has its two dots, resting at the picture's foot, so its
// time and mix can be dragged back up.
#[test]
fn a_silent_delay_keeps_its_dots() {
    let runner = Runner::new();
    let d = draw(&runner, "effects", "delay", "echoes", json!({"time": 0.25, "feedback": 0.3, "mix": 0})).unwrap();
    assert_eq!(dots(&d), ["x=time y=mix(knob)", "wheel=feedback y=feedback(knob)"]);
    assert!(close(places(&d)[0], (0.25, -60.0)) && close(places(&d)[1], (0.5, -60.0)), "{:?}", places(&d));
}

/// A test pack whose reverb picture names a value it doesn't have.
const MISNAMED: &str = r#"import { component, control, port } from "@m2/core";
import { tail, tailPicture } from "@m2/effects/pictures";

export const room = component(
  {
    id: "room",
    inputs: { x: port.audio() },
    controls: { size: control.frac(0.5), damping: control.frac(0.5) },
    output: port.audio(),
    editor: { views: { tail: tailPicture() } },
  },
  (x) => x,
  { tail: { draw: (v, p) => tail(p, v, { size: "size", damping: "dampin" } as never), words: () => "a room" } },
);
"#;

// CARD-110 (review T7): a picture that names a value its sound hasn't got says so, never that a cable
// sets it.
#[test]
fn a_misnamed_value_is_said_as_one() {
    let dir = std::env::temp_dir().join(format!("m2-script-misnamed-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/index.ts"), MISNAMED).unwrap();
    let packages = vec![
        ("@test/room".to_string(), dir.clone()),
        ("@m2/effects".to_string(), repo().join("packs/@m2/effects")),
        ("std".to_string(), repo().join("std")),
    ];
    let catalog = m2_script::describe(&packages);
    let descriptor = catalog.components.get("@test/room:room").cloned().unwrap_or_else(|| panic!("{:?}", catalog.problems));
    let ask = Ask {
        key: "room".into(),
        packages,
        song: None,
        module: descriptor.module.clone(),
        descriptor,
        view: "tail".into(),
        values: json!({"size": 0.5, "damping": 0.5}).to_string(),
        memo: "room\u{0}tail".into(),
        rate: 48_000,
    };
    let why = match Runner::new().draw(ask) {
        Drawn::Cant(why) => why,
        other => panic!("{other:?}"),
    };
    assert!(why.contains("dampin, which this sound doesn't have") && !why.contains("cable"), "{why}");
    let _ = std::fs::remove_dir_all(&dir);
}

// CARD-75 (a fifth of CARD-64's limit): every picture of both packs draws in 4 ms at most, its first
// draw too (which works out its kept sums, CARD-110), once its module is loaded (a limit of its own):
// at its defaults, then at five more values. It judges speed, so it runs in the nightly tier.
#[test]
fn every_pack_picture_draws_inside_a_fifth_of_its_limit() {
    let fifth = std::time::Duration::from_millis(4);
    let mut slow = vec![];
    for pack in ["mixing", "effects"] {
        let packages = vec![(format!("@m2/{pack}"), repo().join("packs/@m2").join(pack)), ("std".to_string(), repo().join("std"))];
        let catalog = m2_script::describe(&packages);
        let runner = Runner::new();
        let mut loaded = false;
        for (cid, d) in catalog.components.iter().filter(|(c, _)| c.starts_with(&format!("@m2/{pack}:"))) {
            for view in d.editor.views.iter().filter(|v| matches!(v.of, m2_script::component::ViewOf::Picture { .. })) {
                let defaults = |nudge: f64| {
                    let mut values = serde_json::Map::new();
                    for c in d.controls.iter().filter(|c| c.each.is_none()) {
                        let at = c.default.as_ref().map_or(0.0, |s| s.value);
                        values.insert(
                            c.name.clone(),
                            match c.unit {
                                Some(m2_ir::Unit::Switch) => json!(at != 0.0),
                                _ => match c.options() {
                                    Some(names) => json!(names.name(at as usize).unwrap_or_default()),
                                    None => json!(at * nudge),
                                },
                            },
                        );
                    }
                    for s in &d.settings {
                        values.insert(s.name.clone(), s.default.clone().filter(|v| v.is_number() || v.is_string()).unwrap_or(Value::Null));
                    }
                    Value::Object(values).to_string()
                };
                for (i, nudge) in [1.0, 0.9, 0.8, 1.1, 0.7, 0.95].into_iter().enumerate() {
                    let ask = Ask {
                        key: pack.into(),
                        packages: packages.clone(),
                        song: None,
                        module: d.module.clone(),
                        descriptor: d.clone(),
                        view: view.name.clone(),
                        values: defaults(nudge),
                        memo: format!("{cid}\u{0}{}", view.name),
                        rate: 48_000,
                    };
                    // The module's load first: it has a limit of its own (CARD-64).
                    if !loaded {
                        let _ = runner.draw(ask.clone());
                        loaded = true;
                        continue;
                    }
                    let t = std::time::Instant::now();
                    let drawn = runner.draw(ask);
                    let took = t.elapsed();
                    assert!(matches!(drawn, Drawn::Shapes(_)), "{cid}'s {}: {drawn:?}", view.name);
                    if took > fifth {
                        slow.push(format!("{cid}'s {} draw {i}: {took:?}", view.name));
                    }
                }
            }
        }
    }
    assert!(slow.is_empty(), "past a fifth of a draw's 20 ms: {slow:#?}");
}
