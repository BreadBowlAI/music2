//! `@m2/notes`' descriptors and pictures (PACK-29 to PACK-33): each sound is a note effect in its
//! category (PKG-21), what a call that leaves a value out gets is its kernel's port default (PKG-4),
//! so the kernels' own tests, which play their manifest's defaults, test what songs play; each card's
//! face is its picture and its values. Each picture draws, from the pack's own code, what its rule
//! says, for C, as the picture runner draws it for a window or an agent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m2_script::component::ViewOf;
use m2_script::describe;
use m2_script::pictures::{Ask, Drawn, Runner};
use serde_json::{Value, json};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn packages() -> Vec<(String, PathBuf)> {
    vec![("@m2/notes".to_string(), repo().join("packs/@m2/notes")), ("std".to_string(), repo().join("std"))]
}

/// The port defaults of the pack's kernels, by `(kernel id, port)`, read from its `m2pkg.toml`: a
/// choice's is its place, a switch's 1 or 0.
fn port_defaults() -> BTreeMap<(String, String), f64> {
    let manifest = std::fs::read_to_string(repo().join("packs/@m2/notes/m2pkg.toml")).unwrap();
    let (mut kernel, mut inputs, mut out) = (String::new(), false, BTreeMap::new());
    for line in manifest.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("name = \"").and_then(|s| s.strip_suffix('"')) {
            kernel = format!("@m2/notes:{name}");
        }
        inputs |= line.starts_with("inputs = [");
        if inputs {
            for spec in line.split('"').skip(1).step_by(2) {
                let Some((port, kind)) = spec.split_once(':') else { continue };
                if let Some((_, default)) = kind.rsplit_once(" = ") {
                    out.insert((kernel.clone(), port.trim().to_string()), default.split_whitespace().next().unwrap().parse().unwrap());
                }
            }
        }
        inputs &= !line.ends_with(']');
    }
    out
}

/// A control's name as its kernel's port: `inKey` is `in_key`.
fn snake(name: &str) -> String {
    name.chars().flat_map(|c| if c.is_ascii_uppercase() { vec!['_', c.to_ascii_lowercase()] } else { vec![c] }).collect()
}

// PACK-29, PKG-21, PKG-4: four note effects, each in its category, each value's default its
// kernel's port's; each card's face its picture and its values (but the Arpeggiator's swing, under
// All), with no group; the Arpeggiator's seed a setting.
#[test]
fn the_note_effects_start_where_their_kernels_do() {
    let catalog = describe(&packages());
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let ids: Vec<&str> = catalog.components.keys().filter_map(|k| k.strip_prefix("@m2/notes:")).collect();
    assert_eq!(ids, ["arp", "chord", "echo", "scale"], "the pack's sounds");
    let ports = port_defaults();
    for (id, label, category, face, controls) in [
        ("arp", "Arpeggiator", "Arpeggiator", ["steps", "style", "rate", "octaves", "gate", "hold"].as_slice(), 6),
        ("chord", "Chord", "Chords", &["keyboard", "shape", "inKey", "inversion"], 3),
        ("echo", "Note echo", "Echo", &["echoes", "time", "repeats", "fall", "pitch"], 4),
        ("scale", "Scale", "Scales", &["keyboard", "shift"], 1),
    ] {
        let d = &catalog.components[&format!("@m2/notes:{id}")];
        assert!(d.is_note_effect() && !d.is_effect() && !d.is_instrument(), "{id}: a note effect");
        assert_eq!((d.label.as_str(), d.editor.category.as_deref()), (label, Some(category)), "{id}");
        assert_eq!(d.editor.face, face, "{id}'s face");
        assert!(d.editor.groups.is_empty(), "{id} has no group");
        assert_eq!(d.controls.len(), controls, "{id}'s controls");
        let views: Vec<&str> = d.editor.views.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(views, [face[0]], "{id} draws one picture");
        assert!(matches!(d.editor.views[0].of, ViewOf::Picture { .. }), "{id}'s view is a picture");
        for c in &d.controls {
            let port = (d.id.clone(), snake(&c.name));
            let kernel = ports.get(&port).unwrap_or_else(|| panic!("{id}.{}: its kernel has no port {}", c.name, port.1));
            assert_eq!(c.default.as_ref().map(|s| s.value), Some(*kernel), "{id}.{}: its default is its kernel's port's", c.name);
        }
    }
    let arp = &catalog.components["@m2/notes:arp"];
    let seed = arp.settings.iter().find(|s| s.name == "seed").expect("the Arpeggiator's seed");
    assert_eq!(seed.default, Some(json!(1)));
    let options = |id: &str, name: &str| -> Vec<String> {
        let c = catalog.components[id].controls.iter().find(|c| c.name == name).unwrap();
        c.unit.and_then(|u| u.options()).unwrap_or_else(|| panic!("{id}.{name} is a menu")).names().iter().map(|n| n.to_string()).collect()
    };
    assert_eq!(options("@m2/notes:arp", "rate"), ["1/4", "1/8", "1/8T", "1/16", "1/16T", "1/32"]);
    assert_eq!(options("@m2/notes:echo", "time"), options("@m2/notes:arp", "rate"), "Note echo's times are the Arpeggiator's rates");
    assert_eq!(options("@m2/notes:chord", "shape"), ["octave", "fifth", "triad", "7th", "sus2", "sus4", "add9"]);
    let in_key = catalog.components["@m2/notes:chord"].controls.iter().find(|c| c.name == "inKey").unwrap();
    assert_eq!(in_key.label.as_deref(), Some("In key"));
}

/// Picture `view` of `@m2/notes`' `component`, drawn at `values` (what plays, by name, CARD-63).
fn draw(runner: &Runner, component: &str, view: &str, values: Value) -> Value {
    let catalog = describe(&packages());
    let cid = format!("@m2/notes:{component}");
    let descriptor = catalog.components.get(&cid).cloned().unwrap_or_else(|| panic!("no {cid}: {:?}", catalog.problems));
    let ask = Ask {
        key: "notes".into(),
        packages: packages(),
        song: None,
        module: descriptor.module.clone(),
        descriptor,
        view: view.into(),
        values: values.to_string(),
        memo: format!("{component}\u{0}{view}"),
    };
    match runner.draw(ask) {
        Drawn::Shapes(json) => serde_json::from_str(&json).unwrap(),
        other => panic!("{component}'s {view}: {other:?}"),
    }
}

fn shapes(d: &Value) -> &Vec<Value> {
    d["shapes"].as_array().unwrap()
}

/// The boxes a picture drew (closed lines of five points), as (x0, x1, y0, y1), and whether each is
/// lit (a fill under it).
fn boxes(d: &Value) -> Vec<((f64, f64, f64, f64), bool)> {
    let all = shapes(d);
    let mut out = vec![];
    for (i, s) in all.iter().enumerate() {
        if s["kind"] != "line" || s["points"].as_array().unwrap().len() != 10 {
            continue;
        }
        let p: Vec<f64> = s["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let lit = i > 0 && all[i - 1]["kind"] == "fill";
        out.push(((p[0], p[2], p[1], p[5]), lit));
    }
    out
}

/// The labels a picture drew: (x, words).
fn labels(d: &Value) -> Vec<(f64, String)> {
    shapes(d)
        .iter()
        .filter(|s| s["kind"] == "label")
        .map(|s| (s["x"].as_f64().unwrap(), s["words"].as_str().unwrap().to_string()))
        .collect()
}

/// The guide lines across a picture's x axis: where.
fn marks(d: &Value) -> Vec<f64> {
    shapes(d).iter().filter(|s| s["kind"] == "mark" && s["axis"] == "x").map(|s| s["at"].as_f64().unwrap()).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn arp_values(changes: Value) -> Value {
    let mut v = json!({"style": "up", "rate": "1/16", "octaves": "1", "gate": 0.8, "hold": false, "swing": 0.5, "seed": 1});
    for (k, x) in changes.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

// PACK-30: the steps picture is one turn of the pattern for C major: each step a box from its
// step's start, gate × its step long, at its note's place, low to high; the chord's notes named at
// their first box; a guide line where a turn starts and where the next does. Its line of the step
// playing is the kernel's meter `step`, a live layer.
#[test]
fn the_arpeggiators_steps_are_its_pattern_for_c_major() {
    let catalog = describe(&packages());
    let steps = &catalog.components["@m2/notes:arp"].editor.views[0];
    assert_eq!(serde_json::to_value(&steps.of).unwrap()["live"], json!([{"kind": "playhead", "from": "step"}]));
    let runner = Runner::new();
    let d = draw(&runner, "arp", "steps", arp_values(json!({})));
    let b = boxes(&d);
    assert_eq!(b.len(), 3, "C E G, one turn");
    for (k, ((x0, x1, _, _), lit)) in b.iter().enumerate() {
        assert!(close(*x0, 0.25 * k as f64) && close(*x1, 0.25 * k as f64 + 0.2) && *lit, "step {k}: {:?}", b[k]);
    }
    let heights: Vec<f64> = b.iter().map(|((_, _, y0, _), _)| *y0).collect();
    assert!(heights[0] < heights[1] && heights[1] < heights[2], "low to high: {heights:?}");
    assert_eq!(labels(&d).iter().map(|l| l.1.as_str()).collect::<Vec<_>>(), ["C4", "E4", "G4"]);
    assert_eq!(marks(&d), [0.0, 0.75], "where a turn starts, and the next");
    assert_eq!(d["words"], "Up, sixteenth notes, 1 octave");
    // Up and down, two octaves, eighths, swung 60%, gate 1.5: C E G C' E' G' E' C' G E, the second
    // of each pair at 60% of it, each box 1.5 times its own step.
    let d = draw(
        &runner,
        "arp",
        "steps",
        arp_values(json!({"style": "updown", "octaves": "2", "rate": "1/8", "swing": 0.6, "gate": 1.5, "hold": true})),
    );
    let b = boxes(&d);
    assert_eq!(b.len(), 10);
    let order: Vec<f64> = b.iter().map(|((_, _, y0, _), _)| *y0).collect();
    let rank = |k: usize| order.iter().filter(|y| **y < order[k] - 1e-9).count();
    // Each note's place among C E G C' E' G' (the turn's back half repeats them).
    assert_eq!((0..10).map(rank).collect::<Vec<_>>(), [0, 1, 3, 5, 7, 9, 7, 5, 3, 1]);
    for (k, ((x0, x1, _, _), _)) in b.iter().enumerate() {
        let (start, len) = if k % 2 == 0 { (0.5 * k as f64, 0.6) } else { (0.5 * (k - 1) as f64 + 0.6, 0.4) };
        assert!(close(*x0, start) && close(*x1, start + 1.5 * len), "step {k}: {:?}", b[k]);
    }
    assert!(order[5] > order[4] && order[6] < order[5] && order[9] > order[0], "up to G5, then back down: {order:?}");
    assert_eq!(marks(&d), [0.0, 5.0], "a turn of ten eighths");
    assert_eq!(d["words"], "Up and down, eighth notes, 2 octaves, held while stopped");
    // Random draws the notes it picks from, low to high; Down from the top.
    let d = draw(&runner, "arp", "steps", arp_values(json!({"style": "down", "rate": "1/8T"})));
    assert_eq!(labels(&d).iter().map(|l| l.1.as_str()).collect::<Vec<_>>(), ["G4", "E4", "C4"]);
    assert_eq!(d["words"], "Down, eighth-note triplets, 1 octave");
    let d = draw(&runner, "arp", "steps", arp_values(json!({"style": "random", "octaves": "3"})));
    assert_eq!(boxes(&d).len(), 9);
    assert_eq!(d["words"], "Random, sixteenth notes, 3 octaves");
}

/// Each white key of an octave, and each black key's, in semitones from C: the piano's (`pictures.ts`).
const WHITE: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

/// The semitones from C of the key a box or a label at `x` is drawn on, across in white keys: a
/// black key's sits over the line after its white key, from 0.68 of it on.
fn key_at(x: f64, black: bool) -> i32 {
    let w = x.floor() as i32;
    let white = 12 * (w / 7) + WHITE[(w % 7) as usize];
    if black { white + 1 } else { white }
}

/// The keys a keyboard picture lights, and the name it gives each key, by semitones from C.
fn keys(d: &Value) -> (Vec<i32>, Vec<(i32, String)>) {
    let mut lit: Vec<i32> = boxes(d).into_iter().filter(|(_, l)| *l).map(|((x0, _, y0, _), _)| key_at(x0, y0 > 0.2)).collect();
    lit.sort();
    let mut names: Vec<(i32, String)> = shapes(d)
        .iter()
        .filter(|s| s["kind"] == "label")
        .map(|s| (key_at(s["x"].as_f64().unwrap(), s["y"].as_f64().unwrap() > 0.2), s["words"].as_str().unwrap().to_string()))
        .collect();
    names.sort();
    (lit, names)
}

/// A note's name with its octave, from its semitones above C4.
fn name(semis: i32) -> String {
    const NAMES: [&str; 12] = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
    format!("{}{}", NAMES[semis.rem_euclid(12) as usize], 4 + semis.div_euclid(12))
}

/// The notes Chord's keyboard lights at `values` (over its defaults), by name from C4, each named on its key.
fn lit_chord(runner: &Runner, values: Value) -> (Vec<String>, String) {
    let mut v = json!({"shape": "triad", "inKey": true, "inversion": 0});
    for (k, x) in values.as_object().unwrap() {
        v[k] = x.clone();
    }
    let d = draw(runner, "chord", "keyboard", v);
    assert_eq!(boxes(&d).len(), 25, "C4 to C6: fifteen white keys and ten black");
    let (lit, names) = keys(&d);
    assert_eq!(names.iter().map(|n| n.0).collect::<Vec<_>>(), lit, "each lit key named, and none else");
    assert!(names.iter().all(|(k, w)| name(*k).starts_with(w.as_str())), "named by its note: {names:?}");
    (lit.into_iter().map(name).collect(), d["words"].as_str().unwrap().to_string())
}

// PACK-31: Chord's keyboard lights the notes its chord plays for a C, in C major while In key is
// on, else the fixed shape: a triad of the key is C E G, its 7th has B; a dominant 7th has B♭.
#[test]
fn chords_keyboard_lights_its_chord_on_c() {
    let runner = Runner::new();
    let (lit, words) = lit_chord(&runner, json!({}));
    assert_eq!(lit, ["C4", "E4", "G4"]);
    assert_eq!(words, "A triad of the key on each note: on C in C major, C E G");
    assert_eq!(lit_chord(&runner, json!({"shape": "7th"})).0, ["C4", "E4", "G4", "B4"]);
    let (lit, words) = lit_chord(&runner, json!({"shape": "7th", "inKey": false}));
    assert_eq!(lit, ["C4", "E4", "G4", "B♭4"]);
    assert_eq!(words, "A dominant 7th on each note: on C, C E G B♭");
    let (lit, words) = lit_chord(&runner, json!({"inversion": 1}));
    assert_eq!(lit, ["E4", "G4", "C5"]);
    assert_eq!(words, "A triad of the key on each note: on C in C major, E G C, inversion 1");
    assert_eq!(lit_chord(&runner, json!({"shape": "add9", "inversion": 2})).0, ["G4", "C5", "D5", "E5"]);
    assert_eq!(lit_chord(&runner, json!({"shape": "octave", "inversion": 2})).0, ["C5", "C6"]);
}

// PACK-32: Note echo's echoes are fading bars: the note at 1, each echo `time` later and (1 − fall)
// times as tall, each named by how far it moves when it climbs.
#[test]
fn the_echoes_fade_as_their_values_say() {
    let runner = Runner::new();
    let d = draw(&runner, "echo", "echoes", json!({"time": "1/8", "repeats": 3, "fall": 0.3, "pitch": 0}));
    let b = boxes(&d);
    assert_eq!(b.len(), 4);
    for (k, ((x0, x1, y0, y1), _)) in b.iter().enumerate() {
        assert!(close(*x0, 0.5 * k as f64) && close(*x1, 0.5 * k as f64 + 0.25), "echo {k}: {:?}", b[k]);
        assert!(close(*y0, 0.0) && (y1 - 0.7f64.powi(k as i32)).abs() < 1e-9, "echo {k}'s level: {y1}");
    }
    assert!(labels(&d).is_empty(), "it doesn't climb");
    assert_eq!(d["words"], "3 echoes, an eighth note apart, each 30% quieter");
    let d = draw(&runner, "echo", "echoes", json!({"time": "1/16", "repeats": 2, "fall": 0, "pitch": 12}));
    assert_eq!(labels(&d).iter().map(|l| l.1.as_str()).collect::<Vec<_>>(), ["+12", "+24"]);
    assert_eq!(d["words"], "2 echoes, a sixteenth note apart, each as loud, each 12 semitones higher");
    let d = draw(&runner, "echo", "echoes", json!({"time": "1/4", "repeats": 1, "fall": 0.5, "pitch": -1}));
    assert_eq!(d["words"], "1 echo, a quarter note apart, each 50% quieter, each 1 semitone lower");
}

// PACK-33: Scale's keyboard is the twelve notes of an octave from C, C major's lit, each named by
// the note it plays there at this shift.
#[test]
fn scales_keyboard_names_where_each_note_goes() {
    let runner = Runner::new();
    let d = draw(&runner, "scale", "keyboard", json!({"shift": 0}));
    assert_eq!(boxes(&d).len(), 12, "seven white keys and five black");
    let (lit, names) = keys(&d);
    assert_eq!(lit, [0, 2, 4, 5, 7, 9, 11], "C major's notes");
    let words = |names: Vec<(i32, String)>| names.into_iter().map(|n| n.1).collect::<Vec<_>>();
    assert_eq!(words(names), ["C", "C", "D", "D", "E", "F", "F", "G", "G", "A", "A", "B"], "a tie goes down");
    assert_eq!(
        d["words"],
        "Moves each note to the nearest note of the key, a tie going down. Drawn in C major: the card says your song's key"
    );
    let d = draw(&runner, "scale", "keyboard", json!({"shift": 2}));
    assert_eq!(words(keys(&d).1), ["E", "E", "F", "F", "G", "A", "A", "B", "B", "C", "C", "D"]);
    assert_eq!(
        d["words"],
        "Moves each note to the nearest note of the key, a tie going down, then 2 notes of the key up. Drawn in C major: the card says your song's key"
    );
}
