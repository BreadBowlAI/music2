//! The card's menus, switches, groups and views (CARD-20 to CARD-26): a sound that declares every
//! kind and view, as the kinds track's `every` test component does, played by a project.

use serde_json::{Value as Json, json};

use crate::kind::{Choice, Kind};
use crate::testing::{Fixture, MANIFEST};
use crate::{Gestures, Knob, ViewNode};

/// A sound with a choice, a switch, an envelope's and a filter's controls, regions, its four views
/// (its envelope std's picture, CARD-96; its filter's curve its own maths, on std's curve) and three
/// groups (the design's §3.5 example).
pub(crate) const WOBBLE: &str = "import { component, control, knob, port, setting, view } from \"@m2/core\";
import { lowpass } from \"std/filter\";
import { sampler } from \"std/sampler\";
import { curvePicture, envelope, envelopePicture, envelopeWords } from \"std/pictures\";

const ENV = { attack: \"attack\", decay: \"decay\", sustain: \"sustain\", release: \"release\" } as const;
/** A lowpass's curve in dB at `f`, from its cutoff `c` and q, drawn from the sound's own maths. */
const cut = (f: number, c: number, q: number) => -10 * Math.log10((1 - (f / c) ** 2) ** 2 + (f / c / q) ** 2);

export const wobble = component(
  {
    id: \"wobble\",
    label: \"Wobble\",
    inputs: { notes: port.notes() },
    controls: {
      shape: control.choice([\"saw\", \"square\", \"sine\"], \"saw\"),
      sub: control.bool(false),
      cutoff: control.hz(800).range(40, 8000).log(),
      q: control.scalar(0.7),
      attack: control.secs(0.005),
      decay: control.secs(0.2),
      sustain: control.frac(0.6),
      release: control.secs(0.3),
    },
    settings: { regions: setting.regions() },
    output: port.audio(),
    editor: {
      category: \"Instruments\",
      views: { amp: envelopePicture(), filter: curvePicture({ live: false }), keys: view.keys(\"regions\"), scope: view.scope() },
      groups: { Sound: [\"shape\", \"sub\", \"scope\"], Filter: [\"filter\", \"cutoff\", \"q\"], Amp: [\"amp\", \"attack\", \"decay\", \"sustain\", \"release\"] },
    },
  },
  (notes, { cutoff, q, attack, release, regions }) => lowpass(sampler(notes, { regions, attack, release }), { cutoff, q }),
  {
    amp: { draw: (v, p) => envelope(p, v, ENV), words: (v) => envelopeWords(v, ENV) },
    filter: {
      draw: (v, p) => {
        const [c, q] = [v.cutoff ?? 800, v.q ?? 0.7];
        const points = Array.from({ length: 61 }, (_, i) => 20 * 1000 ** (i / 60)).flatMap((f) => [f, Math.max(-24, cut(f, c, q))]);
        return [p.line(points, { name: \"curve\" }), p.handle(c, 20 * Math.log10(q), { x: \"cutoff\", y: knob(\"q\"), wheel: \"q\" })];
      },
      words: (v) => `Cuts the highs above ${v.cutoff} Hz`,
    },
  },
);
";

/// The regions the song gives Wobble: two recordings, the second for softer notes only.
pub(crate) const REGIONS: &str =
    "[{ file: \"c4.wav\", root: 60, keys: [48, 72] }, { file: \"c5.wav\", root: 72, keys: [73, 96], velocity: [1, 100] }]";

/// A song whose one track plays Wobble with `options` written in its call.
pub(crate) fn wobble(options: &str) -> Fixture {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write("lib/wobble.ts", WOBBLE);
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ wobble }} from \"./lib/wobble\";\n\
             const lead = track(\"lead\", ({{ lane }}) => wobble(lane.notes(\"notes\"), {options}));\nexport default song(() => lead.out);\n"
        ),
    );
    f
}

/// Wobble's card.
fn card(s: &m2_session::Session) -> ViewNode {
    let v = s.view();
    let m = v.modules.into_iter().find(|m| m.key == "lead").expect("the lead track");
    m.nodes.into_iter().find(|n| n.title.as_deref() == Some("Wobble")).expect("Wobble's card")
}

/// The knob of `slot` on Wobble's card.
fn knob(s: &m2_session::Session, slot: &str) -> Knob {
    card(s).knobs.into_iter().find(|k| k.slot.as_deref() == Some(slot)).unwrap_or_else(|| panic!("no {slot} knob"))
}

fn options(names: &[(Json, &str)]) -> Vec<Choice> {
    names.iter().map(|(v, l)| Choice::new(v.clone(), *l)).collect()
}

// CARD-20: a choice control's knob is a menu of its names, each its own label, and its value and
// default are names; a switch's is a switch of false and true, its value true or false. A number
// knob is as before.
#[test]
fn a_choice_and_a_switch_control_are_a_menu_and_a_switch() {
    let f = wobble("{ shape: 'square' }");
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let shape = knob(&s, "shape");
    assert_eq!((shape.kind, &shape.value, &shape.default, shape.written), (Kind::Choice, &json!("square"), &Some(json!("saw")), true));
    assert_eq!(shape.options, options(&[(json!("saw"), "saw"), (json!("square"), "square"), (json!("sine"), "sine")]));
    let sub = knob(&s, "sub");
    assert_eq!((sub.kind, &sub.value, &sub.default, sub.written), (Kind::Switch, &json!(false), &Some(json!(false)), false));
    assert_eq!(sub.options, options(&[(json!(false), "Off"), (json!(true), "On")]));
    let cutoff = knob(&s, "cutoff");
    assert_eq!((cutoff.kind, &cutoff.value, cutoff.options.is_empty()), (Kind::Number, &json!(800), true));
    // As sent: what an agent reads.
    let sent = serde_json::to_value(&shape).unwrap();
    assert_eq!(
        (sent["kind"].clone(), sent["value"].clone(), sent["options"][1].clone()),
        (json!("choice"), json!("square"), json!({"value": "square", "label": "square"}))
    );
}

// CARD-21: the one write sets a choice by its name, keeping the literal's quotes, and a switch the
// call leaves out to true, written into the call: each one Tx and one undo step, the code before
// and after exact. A name the choice doesn't have, a number, and anything but a boolean for a
// switch, are refused in words, and nothing is written. The value it has already writes nothing.
#[test]
fn a_choice_is_written_by_its_name_and_a_switch_as_true() {
    let f = wobble("{ shape: 'saw' }");
    let mut s = f.open();
    let song = f.read("song.ts");
    let shape = knob(&s, "shape");
    let at = s.accepted().number;
    let done = s.write(at, &shape.id, &json!("square"), None).unwrap();
    let want = song.replace("{ shape: 'saw' }", "{ shape: 'square' }");
    assert_eq!(f.read("song.ts"), want);
    assert_eq!((done.before.as_str(), done.after.as_str(), done.value), ("'saw'", "'square'", 1.0));
    assert_eq!(knob(&s, "shape").value, json!("square"));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), song, "one undo step");
    assert!(!s.can_undo(), "only one step was made");

    let sub = knob(&s, "sub");
    let at = s.accepted().number;
    s.write(at, &sub.id, &json!(true), None).unwrap();
    assert_eq!(f.read("song.ts"), song.replace("{ shape: 'saw' }", "{ shape: 'saw', sub: true }"));
    assert_eq!((knob(&s, "sub").value, knob(&s, "sub").written), (json!(true), true));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), song);

    let at = s.accepted().number;
    let refused = |s: &mut m2_session::Session, id: &str, v: Json| s.write(at, id, &v, None).unwrap_err().to_string();
    let e = refused(&mut s, &shape.id, json!("squar"));
    assert!(e.contains("shape is one of \"saw\", \"square\" or \"sine\", not \"squar\""), "{e}");
    let e = refused(&mut s, &shape.id, json!(1));
    assert!(e.contains("shape is one of \"saw\", \"square\" or \"sine\", not 1"), "{e}");
    let e = refused(&mut s, &sub.id, json!(1));
    assert!(e.contains("sub is a switch: true or false, not 1"), "{e}");
    let e = refused(&mut s, &sub.id, json!("on"));
    assert!(e.contains("sub is a switch: true or false, not \"on\""), "{e}");
    let cutoff = knob(&s, "cutoff");
    let e = refused(&mut s, &cutoff.id, json!("loud"));
    assert!(e.contains("cutoff is a number, not \"loud\""), "{e}");
    assert_eq!(f.read("song.ts"), song, "nothing written");
    let same = s.write(at, &shape.id, &json!("saw"), None).unwrap();
    assert_eq!((same.revision, same.before.as_str(), same.after.as_str()), (at, "'saw'", "'saw'"));
    assert!(!s.can_undo());
    // A number still writes a number knob, as `set` does.
    s.write(at, &cutoff.id, &json!(1200), None).unwrap();
    assert_eq!(f.read("song.ts"), song.replace("{ shape: 'saw' }", "{ shape: 'saw', cutoff: 1200 }"));
}

// CARD-22: the values one gesture moves are written as one Tx: one undo step. Two left out of one
// call are written into it together; one written is rewritten in place. A value refused refuses
// them all, and nothing is written.
#[test]
fn values_one_gesture_moves_are_one_undo_step() {
    let f = wobble("{ decay: 0.4 }");
    let mut s = f.open();
    let song = f.read("song.ts");
    let (attack, decay, sustain) = (knob(&s, "attack"), knob(&s, "decay"), knob(&s, "sustain"));
    let at = s.accepted().number;
    let done =
        s.write_all(at, &[(decay.id.clone(), json!(0.25)), (sustain.id.clone(), json!(0.5)), (attack.id.clone(), json!(0.01))]).unwrap();
    assert_eq!(f.read("song.ts"), song.replace("{ decay: 0.4 }", "{ decay: 0.25, sustain: 0.5, attack: 0.01 }"));
    assert_eq!(done.iter().map(|c| c.value).collect::<Vec<_>>(), [0.25, 0.5, 0.01]);
    assert_eq!((knob(&s, "sustain").value, knob(&s, "attack").written), (json!(0.5), true));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), song, "one undo step");
    assert!(!s.can_undo());
    let at = s.accepted().number;
    let e = s.write_all(at, &[(decay.id.clone(), json!(0.3)), (sustain.id.clone(), json!("loud"))]).unwrap_err().to_string();
    assert!(e.contains("sustain is a number, not \"loud\""), "{e}");
    assert_eq!(f.read("song.ts"), song, "nothing written");
}

// CARD-21 (review V10, V16): a choice changed elsewhere is said by its name, a switch by on or
// off, never by a place or 1 or 0; a number sent where a name goes says how it is set; and a
// gesture of several values says each one's name before and after.
#[test]
fn a_choice_and_a_gesture_are_said_in_words() {
    let f = wobble("{ shape: 'saw', decay: 0.4 }");
    let mut s = f.open();
    let at = s.accepted().number;
    let shape = knob(&s, "shape");
    let e = s.write(at, &shape.id, &json!("sine"), Some(&json!("square"))).unwrap_err().to_string();
    assert!(e.contains("shape changed elsewhere: it is \"saw\" now, not \"square\""), "{e}");
    let sub = knob(&s, "sub");
    let e = s.write(at, &sub.id, &json!(true), Some(&json!(true))).unwrap_err().to_string();
    assert!(e.contains("sub changed elsewhere: it is false now, not true"), "{e}");
    let e = s.set(at, &shape.id, 2.0, None).unwrap_err().to_string();
    assert!(e.contains("shape is one of \"saw\", \"square\" or \"sine\": set it by its name"), "{e}");
    let e = s.set(at, &sub.id, 1.0, None).unwrap_err().to_string();
    assert!(e.contains("sub is a switch: set it to true or false"), "{e}");
    let (decay, sustain) = (knob(&s, "decay"), knob(&s, "sustain"));
    let done = s.write_all(at, &[(decay.id.clone(), json!(0.25)), (sustain.id.clone(), json!(0.5))]).unwrap();
    assert_eq!((done[0].before.as_str(), done[0].after.as_str()), ("decay 0.4, sustain (default)", "decay 0.25, sustain 0.5"));
}

/// `{"unit": "choice", "clips": [...]}` as a lane of names' file holds it.
fn steps_file(clips: &str) -> String {
    format!("{{\"unit\": \"choice\", \"clips\": [{clips}]}}\n")
}

/// The lead's lane `key` as the timeline shows it.
fn lane(s: &m2_session::Session, key: &str) -> crate::timeline::TimelineLane {
    let t = s.timeline();
    let track = t.tracks.into_iter().find(|t| t.module == "lead").expect("the lead");
    track.lanes.into_iter().find(|l| l.key() == key).unwrap_or_else(|| panic!("no {key} lane"))
}

// CARD-23: Automate on a choice makes a lane of names, and the timeline shows it as a lane of steps:
// its options, its fallback as written, and its clips with each point's name, an unknown one too.
// A switch's lane has false and true.
#[test]
fn a_lane_of_names_is_a_lane_of_steps_on_the_timeline() {
    let f = wobble("{ shape: \"square\" }");
    let mut s = f.open();
    let shape = knob(&s, "shape");
    s.automate(s.accepted().number, &shape.id).unwrap();
    let sub = knob(&s, "sub");
    s.automate(s.accepted().number, &sub.id).unwrap();
    let want_options = options(&[(json!("saw"), "saw"), (json!("square"), "square"), (json!("sine"), "sine")]);
    match lane(&s, "shape") {
        crate::timeline::TimelineLane::Steps { label, options: got, default, clips, .. } => {
            assert_eq!((label.as_str(), &got, &default, clips.len()), ("shape", &want_options, &json!("square"), 0));
        }
        other => panic!("not a lane of steps: {other:?}"),
    }
    f.write(
        "data/lead/shape.json",
        steps_file("{\"id\": \"a\", \"start\": 0, \"length\": 4, \"points\": [[0, \"sine\"], [2, \"sqaure\"]]}"),
    );
    s.refresh();
    let sent = serde_json::to_value(lane(&s, "shape")).unwrap();
    assert_eq!(sent["kind"], "steps");
    assert_eq!(sent["clips"], json!([{"id": "a", "start": 0.0, "length": 4.0, "points": [[0.0, "sine"], [2.0, "sqaure"]]}]));
    match lane(&s, "sub") {
        crate::timeline::TimelineLane::Steps { options: got, default, .. } => {
            assert_eq!((got, default), (options(&[(json!(false), "Off"), (json!(true), "On")]), json!(false)));
        }
        other => panic!("not a lane of steps: {other:?}"),
    }
}

// CARD-24: the clip gestures on a lane of names, one Tx each: a new clip holds the name the lane
// plays there; points drawn by name, an unknown name refused in words naming the options; a move,
// a split (its second part starting with the name playing at the cut), a duplicate and a delete; a
// copy onto a switch's lane refused; join refused.
#[test]
fn clip_gestures_draw_a_lane_of_names() {
    use crate::clips::{ClipRef, Gesture, Point, edit};
    use m2_song::LaneAddr;
    let f = wobble("{ shape: \"square\" }");
    let mut s = f.open();
    let shape = knob(&s, "shape");
    s.automate(s.accepted().number, &shape.id).unwrap();
    let sub = knob(&s, "sub");
    s.automate(s.accepted().number, &sub.id).unwrap();
    let addr = LaneAddr::new("lead", "shape").unwrap();
    let file = || f.read("data/lead/shape.json");
    let made = edit(&mut s, &Gesture::Create { lane: addr.clone(), start: 4.0, length: Some(4.0) }).unwrap();
    let id = made.made[0].1.clone();
    assert!(file().contains(&format!("{{\"id\": \"{id}\", \"start\": 4, \"length\": 4, \"points\": [[0, \"square\"]]}}")), "{}", file());
    let clip = ClipRef { lane: addr.clone(), id: Some(id.clone()), at: None, sha: None };
    let name = |n: &str| Point::Step(m2_song::lane::Step::Name(n.into()));
    edit(&mut s, &Gesture::Points { clip: clip.clone(), points: vec![(2.0, name("sine")), (0.0, name("saw"))] }).unwrap();
    assert!(
        file().contains(&format!("{{\"id\": \"{id}\", \"start\": 4, \"length\": 4, \"points\": [[0, \"saw\"], [2, \"sine\"]]}}")),
        "{}",
        file()
    );
    let e = edit(&mut s, &Gesture::Points { clip: clip.clone(), points: vec![(0.0, name("squar"))] }).unwrap_err().to_string();
    assert!(e.contains("lead's shape lane is one of \"saw\", \"square\" or \"sine\", not \"squar\""), "{e}");
    let e = edit(&mut s, &Gesture::Points { clip: clip.clone(), points: vec![(0.0, Point::Number(1.0))] }).unwrap_err().to_string();
    assert!(e.contains("lead's shape lane is one of \"saw\", \"square\" or \"sine\", not 1"), "{e}");
    let before = file();
    edit(&mut s, &Gesture::Move { clips: vec![clip.clone()], by: 4.0, to: None, copy: false }).unwrap();
    assert!(
        file().contains(&format!("{{\"id\": \"{id}\", \"start\": 8, \"length\": 4, \"points\": [[0, \"saw\"], [2, \"sine\"]]}}")),
        "{}",
        file()
    );
    s.undo().unwrap();
    assert_eq!(file(), before, "one undo step");
    let split = edit(&mut s, &Gesture::Split { clips: vec![clip.clone()], at: 7.0 }).unwrap();
    let second = split.made[0].1.clone();
    assert!(
        file().contains(&format!("{{\"id\": \"{id}\", \"start\": 4, \"length\": 3, \"points\": [[0, \"saw\"], [2, \"sine\"]]}}")),
        "{}",
        file()
    );
    assert!(file().contains(&format!("{{\"id\": \"{second}\", \"start\": 7, \"length\": 1, \"points\": [[0, \"sine\"]]}}")), "{}", file());
    let both = vec![clip.clone(), ClipRef { lane: addr.clone(), id: Some(second.clone()), at: None, sha: None }];
    let e = edit(&mut s, &Gesture::Join { clips: both }).unwrap_err().to_string();
    assert!(e.contains("can't be joined"), "{e}");
    let to = LaneAddr::new("lead", "sub").unwrap();
    let e = edit(&mut s, &Gesture::Move { clips: vec![clip.clone()], by: 0.0, to: Some(to), copy: true }).unwrap_err().to_string();
    assert!(e.contains("A choice's clip can't go on a switch's lane"), "{e}");
    edit(&mut s, &Gesture::Duplicate { clips: vec![clip.clone()], range: None, grid: None, times: None }).unwrap();
    assert_eq!(file().matches("\"points\": [[0, \"saw\"], [2, \"sine\"]]").count(), 2, "{}", file());
    edit(&mut s, &Gesture::Delete { clips: vec![clip] }).unwrap();
    assert_eq!(file().matches("\"points\": [[0, \"saw\"], [2, \"sine\"]]").count(), 1, "{}", file());
}

// CARD-24 (review V9, V13): a left edge moved past a lane of names' points keeps what plays: the
// points before the clip's new start become one at its start, holding the name playing there. A
// clip goes onto a lane of names of other names neither by a move nor by a paste.
#[test]
fn a_lane_of_names_resizes_and_pastes_by_its_names() {
    use crate::clips::{ClipRef, Copied, Gesture, edit};
    use m2_song::LaneAddr;
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "lib/tone.ts",
        "import { component, control, port } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\
         export const tone = component(\n  { id: \"tone\", controls: { shape: control.choice([\"saw\", \"square\", \"sine\"], \"saw\"), mode: control.choice([\"lp\", \"hp\"], \"lp\") }, output: port.audio() },\n  () => osc.saw(110),\n);\n",
    );
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { tone } from \"./lib/tone\";\n\
         const lead = track(\"lead\", ({ lane }) => tone({ shape: lane.control(\"shape\", \"choice\", \"saw\"), mode: lane.control(\"mode\", \"choice\", \"lp\") }));\n\
         export default song(() => lead.out);\n",
    );
    f.write(
        "data/lead/shape.json",
        steps_file("{\"id\": \"a\", \"start\": 0, \"length\": 8, \"points\": [[0, \"saw\"], [2, \"square\"], [5, \"sine\"]]}"),
    );
    f.write("data/lead/mode.json", "{\"unit\": \"choice\", \"clips\": []}\n");
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let shape = LaneAddr::new("lead", "shape").unwrap();
    let clip = ClipRef { lane: shape.clone(), id: Some("a".into()), at: None, sha: None };
    edit(&mut s, &Gesture::Resize { clip: clip.clone(), start: 3.0, length: 5.0 }).unwrap();
    let read = |f: &Fixture| f.read("data/lead/shape.json");
    assert!(
        read(&f).contains("{\"id\": \"a\", \"start\": 3, \"length\": 5, \"points\": [[0, \"square\"], [2, \"sine\"]]}"),
        "{}",
        read(&f)
    );
    // Its left edge back out: the names stay where they play.
    edit(&mut s, &Gesture::Resize { clip: clip.clone(), start: 1.0, length: 7.0 }).unwrap();
    assert!(
        read(&f).contains("{\"id\": \"a\", \"start\": 1, \"length\": 7, \"points\": [[2, \"square\"], [4, \"sine\"]]}"),
        "{}",
        read(&f)
    );
    let mode = LaneAddr::new("lead", "mode").unwrap();
    let e =
        edit(&mut s, &Gesture::Move { clips: vec![clip.clone()], by: 0.0, to: Some(mode.clone()), copy: true }).unwrap_err().to_string();
    assert!(e.contains("can't share clips: their choices have other names"), "{e}");
    let copied = Copied { from: shape.clone(), clip: json!({"start": 0, "length": 4, "points": [[0, "sine"]]}) };
    let e = edit(&mut s, &Gesture::Paste { clips: vec![copied], to: Some(mode), at: 0.0, into: None, fit: false }).unwrap_err().to_string();
    assert!(e.contains("can't share clips: their choices have other names"), "{e}");
}

// CARD-25: the view carries the card's groups in their declared order, each with its rows in the
// order the group names them (an input by its id, a setting by its id, a view by its name), and a
// last group, More, of what no group names; every control input says its slot. The notes input is
// a connection, in no group.
#[test]
fn a_cards_groups_are_in_the_view() {
    let f = wobble(&format!("{{ shape: \"square\", regions: {REGIONS} }}"));
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let card = card(&s);
    let input =
        |slot: &str| card.inputs.iter().find(|p| p.slot.as_deref() == Some(slot)).unwrap_or_else(|| panic!("no {slot} input")).id.clone();
    let regions = card.settings.iter().find(|x| x.slot == "regions").expect("the regions setting").id.clone();
    use crate::views::Row::{Input, Setting, View};
    let groups: Vec<(&str, Vec<crate::views::Row>)> = card.groups.iter().map(|g| (g.name.as_str(), g.rows.clone())).collect();
    assert_eq!(
        groups,
        [
            ("Sound", vec![Input(input("shape")), Input(input("sub")), View("scope".into())]),
            ("Filter", vec![View("filter".into()), Input(input("cutoff")), Input(input("q"))]),
            (
                "Amp",
                vec![View("amp".into()), Input(input("attack")), Input(input("decay")), Input(input("sustain")), Input(input("release"))]
            ),
            ("More", vec![Setting(regions), View("keys".into())]),
        ]
    );
    assert!(card.inputs.iter().any(|p| p.kind == "notes" && p.slot.is_none()), "the notes input is a connection: {:?}", card.inputs);
    let sent = serde_json::to_value(&card).unwrap();
    assert_eq!(
        sent["groups"][0],
        json!({"name": "Sound", "rows": [{"input": input("shape")}, {"input": input("sub")}, {"view": "scope"}]})
    );
}

// CARD-26, CARD-63: the view carries the card's views, resolved for the call: an envelope and a
// filter, std's pictures, each value they read with its knob; a keys view its setting, the regions
// the Sampler plays and the notes lane; a scope nothing. A value a lane drives has no knob, says
// why, and is what plays with no clip.
#[test]
fn a_cards_views_are_in_the_view() {
    use crate::views::{Drawn, KeyRegion};
    let f = wobble(&format!("{{ shape: \"square\", cutoff: 1200, regions: {REGIONS} }}"));
    let mut s = f.open();
    let card_now = card(&s);
    let knob_id = |slot: &str| card_now.knobs.iter().find(|k| k.slot.as_deref() == Some(slot)).map(|k| k.id.clone());
    let names: Vec<&str> = card_now.views.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["amp", "filter", "keys", "scope"]);
    let value = |view: &crate::views::CardView, name: &str| match &view.of {
        Drawn::Picture { values, .. } => values.iter().find(|v| v.name == name).cloned().unwrap_or_else(|| panic!("no {name}")),
        other => panic!("not a picture: {other:?}"),
    };
    for slot in ["attack", "decay", "sustain", "release"] {
        let v = value(&card_now.views[0], slot);
        assert_eq!((&v.knob, &v.why), (&knob_id(slot), &None), "{slot}");
    }
    assert_eq!((value(&card_now.views[1], "cutoff").knob, value(&card_now.views[1], "q").knob), (knob_id("cutoff"), knob_id("q")));
    assert!(knob_id("cutoff").is_some());
    let regions = card_now.settings.iter().find(|x| x.slot == "regions").unwrap().id.clone();
    match &card_now.views[2].of {
        Drawn::Keys { setting, regions: played, lane, why } => {
            assert_eq!((setting, why), (&regions, &None));
            assert_eq!(lane.as_ref().map(|l| (l.module.as_str(), l.lane.as_str())), Some(("lead", "notes")));
            let c4 = KeyRegion { file: "c4.wav".into(), root: 60, keys: [48, 72], velocity: [1, 127], round_robin: [1, 1], release: false };
            let c5 = KeyRegion { file: "c5.wav".into(), root: 72, keys: [73, 96], velocity: [1, 100], round_robin: [1, 1], release: false };
            assert_eq!(played, &vec![c4, c5]);
        }
        other => panic!("not keys: {other:?}"),
    }
    assert!(matches!(card_now.views[3].of, Drawn::Scope {}));
    let sent = serde_json::to_value(&card_now).unwrap();
    assert_eq!(sent["views"][3], json!({"name": "scope", "kind": "scope"}));
    assert_eq!(sent["views"][0]["kind"], "picture");
    // The attack automated: its part has no knob, and says the lane drives it.
    let attack = knob(&s, "attack");
    s.automate(s.accepted().number, &attack.id).unwrap();
    let now = card(&s);
    let (attack, decay) = (value(&now.views[0], "attack"), value(&now.views[0], "decay"));
    assert_eq!(attack.knob, None);
    assert_eq!(attack.why.as_deref(), Some("it follows the attack lane"));
    // Drawn where it plays with no clip: the lane's fallback, in its unit (V6).
    assert_eq!(attack.value, json!(0.005));
    // The other corners stay the card's to move.
    assert_eq!(decay.knob, knob_id("decay"));
}

// CARD-63 (review V2): a picture moves a knob only in the unit it is declared in. A cutoff written in
// semitones (std's FREQ accepts them: `semis(72)`) has no knob in the filter's picture, which says
// why, and is null for it; its q, in its own unit, is still the card's.
#[test]
fn a_view_moves_only_a_knob_in_a_unit_it_draws() {
    use crate::views::Drawn;
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "lib/tone.ts",
        "import { component, control, knob, port } from \"@m2/core\";\nimport { lowpass } from \"std/filter\";\nimport { osc } from \"std/dsp\";\n\
         import { curvePicture } from \"std/pictures\";\n\
         export const tone = component(\n  { id: \"tone\", label: \"Tone\", controls: { cutoff: control.hz(1000).range(20, 20000).log().accepts(\"semitones\", 0, 127), q: control.scalar(0.7) },\n\
         output: port.audio(), editor: { views: { filter: curvePicture({ live: false }) } } },\n  ({ cutoff, q }) => lowpass(osc.saw(cutoff), { q }),\n\
         { filter: { draw: (v, p) => [p.handle(v.cutoff ?? 1000, 0, { x: \"cutoff\", y: knob(\"q\"), wheel: \"q\" })], words: (v) => `a filter at ${v.cutoff}` } },\n);\n",
    );
    f.write(
        "song.ts",
        "import { semis, song, track } from \"@m2/core\";\nimport { tone } from \"./lib/tone\";\n\
         const lead = track(\"lead\", () => tone({ cutoff: semis(72) }));\nexport default song(() => lead.out);\n",
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let v = s.view();
    let m = v.modules.into_iter().find(|m| m.key == "lead").unwrap();
    let card = m.nodes.into_iter().find(|n| n.title.as_deref() == Some("Tone")).expect("Tone's card");
    let cutoff = card.knobs.iter().find(|k| k.slot.as_deref() == Some("cutoff")).expect("the cutoff's knob");
    assert_eq!((cutoff.unit, &cutoff.value), (Some("semitones"), &json!(72)));
    let Drawn::Picture { values, .. } = &card.views[0].of else { panic!("not a picture: {:?}", card.views[0].of) };
    let (cutoff, q) = (values.iter().find(|v| v.name == "cutoff").unwrap(), values.iter().find(|v| v.name == "q").unwrap());
    assert_eq!((&cutoff.knob, &cutoff.value), (&None, &Json::Null));
    assert_eq!(cutoff.why.as_deref(), Some("it is written in semitones, which this picture doesn't draw"));
    assert!(q.knob.is_some() && q.why.is_none(), "{q:?}");
}

// CARD-25 (review V7): a group may name a dictionary slot; its entries' values are its rows.
#[test]
fn a_group_naming_a_dictionary_holds_its_entries() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { component, control, each, port, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\
         const duo = component(\n  { id: \"duo\", label: \"Duo\", controls: { tone: control.hz(220), parts: each({ in: port.audio(), db: control.db(0), shape: control.choice([\"saw\", \"square\"], \"saw\") }) },\n\
         output: port.audio(), editor: { groups: { Parts: [\"parts\"], Tone: [\"tone\"] } } },\n  ({ tone }) => osc.saw(tone),\n);\n\
         export default song(() => duo({ tone: 220, parts: { a: { in: osc.saw(110), db: -6, shape: \"square\" }, b: { in: osc.saw(220), db: -3 } } }));\n",
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let v = s.view();
    let card = v.modules.iter().flat_map(|m| &m.nodes).find(|n| n.title.as_deref() == Some("Duo")).expect("Duo's card").clone();
    let slot = |r: &crate::views::Row| match r {
        crate::views::Row::Input(id) => card.inputs.iter().find(|p| &p.id == id).and_then(|p| p.slot.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    };
    let groups: Vec<(String, Vec<String>)> = card.groups.iter().map(|g| (g.name.clone(), g.rows.iter().map(slot).collect())).collect();
    let parts = groups.iter().find(|g| g.0 == "Parts").expect("Parts").1.clone();
    let mut sorted = parts.clone();
    sorted.sort();
    assert_eq!(sorted, ["parts.a.db", "parts.a.shape", "parts.b.db", "parts.b.shape"], "{groups:?}");
    assert!(groups.iter().all(|g| g.0 != "More"), "nothing left for More: {groups:?}");
}

// CARD-41: the view carries a card's face, its rows in the order the face names them, as a group's
// rows are; and every call's code on one line, an effect's first argument written `…`. A card with
// no face sends none. `m2 graph` prints the face as a heading and the code under the call.
#[test]
fn a_cards_face_and_code_are_in_the_view() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "lib/wobble.ts",
        WOBBLE.replace("      groups: {", "      face: [\"amp\", \"cutoff\", \"attack\", \"release\"],\n      groups: {"),
    );
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { gain } from \"std/dsp\";\nimport { wobble } from \"./lib/wobble\";\n\
         const lead = track(\"lead\", ({ lane }) =>\n  gain(wobble(lane.notes(\"notes\"), { shape: \"square\", // its wave\n    cutoff: 1200 }), { db: -3 }),\n);\n\
         export default song(() => lead.out);\n",
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let wobble = card(&s);
    let input =
        |slot: &str| wobble.inputs.iter().find(|p| p.slot.as_deref() == Some(slot)).unwrap_or_else(|| panic!("no {slot} input")).id.clone();
    use crate::views::Row::{Input, View};
    assert_eq!(wobble.face, [View("amp".into()), Input(input("cutoff")), Input(input("attack")), Input(input("release"))]);
    assert_eq!(wobble.code.as_deref(), Some("wobble(lane.notes(\"notes\"), { shape: \"square\", cutoff: 1200 })"));
    let v = s.view();
    let gain = v.modules.iter().flat_map(|m| &m.nodes).find(|n| n.title.as_deref() == Some("Gain")).expect("the gain card");
    assert!(gain.face.is_empty());
    assert_eq!(gain.code.as_deref(), Some("gain(…, { db: -3 })"));
    let sent = serde_json::to_value(gain).unwrap();
    assert!(sent.get("face").is_none(), "no face, no field: {sent}");
    assert_eq!(serde_json::to_value(&wobble).unwrap()["face"][0], json!({"view": "amp"}));
}

// CARD-41 (review F5, F7): an effect's sound written by its name is shown as written (`lowpass(w,
// …)`, `duck(bass.out, …)`): only a call, which the card before it shows, is `…`. The code line takes
// out comments, never a string's `//`, and leaves what it can't read safely as written: a call with a
// regex or a template string shows its first line.
#[test]
fn the_code_line_keeps_a_named_sound_and_reads_code_safely() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write("lib/wobble.ts", WOBBLE);
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { lowpass } from \"std/filter\";\nimport { wobble } from \"./lib/wobble\";\n\
         const lead = track(\"lead\", ({ lane }) => {\n  const w = wobble(lane.notes(\"notes\"));\n  return lowpass(w, { cutoff: 900 });\n});\n\
         export default song(() => lead.out);\n",
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let v = s.view();
    let lowpass = v.modules.iter().flat_map(|m| &m.nodes).find(|n| n.title.as_deref() == Some("Lowpass")).expect("the lowpass card");
    assert_eq!(lowpass.code.as_deref(), Some("lowpass(w, { cutoff: 900 })"));
    use super::one_line;
    for (code, line) in [
        ("f(x, {\n  url: \"http://a\", // where\n  cutoff: 1,\n})", "f(x, { url: \"http://a\", cutoff: 1 })"),
        ("f(x, { cutoff: 900 /* warm */ })", "f(x, { cutoff: 900 })"),
        ("f(x, { s: 'it\\'s' })", "f(x, { s: 'it\\'s' })"),
        ("f(x, { re: /\\/*x/, cutoff: 1 })", "f(x, { re: /\\/*x/, cutoff: 1 })"),
        ("f(x, {\n  re: /\"/, // why\n  cutoff: 1,\n})", "f(x, { …"),
        ("f(x, {\n  name: `a\nb`,\n})", "f(x, { …"),
        ("f(x, { period: 1 / 4 })", "f(x, { period: 1 / 4 })"),
    ] {
        assert_eq!(one_line(code), line, "{code}");
    }
}

// CARD-31: every sound music2's own packs list is dressed, and its card shows it: the groups its
// descriptor declares, in their order (More last, for what no group names), or none when it declares
// none (the reverb and the phaser, whose knobs are all on their faces, CARD-110), and its views. Every
// instrument of @m2/synths and @m2/drums has groups; every polysynth sound with a wave to pick has
// its shape as a menu; every sound with a whole envelope has one to drag; every sound with a cutoff
// and a resonance its filter's curve.
#[test]
fn music2s_own_sounds_show_their_groups_and_views() {
    let calls = [
        ("@m2/synths:synth-custom", "synth.custom(lane.notes(\"n\"))"),
        ("@m2/synths:synth-supersaw", "synth.supersaw(lane.notes(\"n\"))"),
        ("@m2/synths:synth-lead", "synth.lead(lane.notes(\"n\"))"),
        ("@m2/synths:synth-pad", "synth.pad(lane.notes(\"n\"))"),
        ("@m2/synths:synth-pluck", "synth.pluck(lane.notes(\"n\"))"),
        ("@m2/synths:synth-sub", "synth.sub(lane.notes(\"n\"))"),
        ("@m2/synths:synth-bass", "synth.bass(lane.notes(\"n\"))"),
        ("@m2/synths:synth-strings", "synth.strings(lane.notes(\"n\"))"),
        ("@m2/synths:fm-custom", "fm.custom(lane.notes(\"n\"))"),
        ("@m2/synths:fm-keys", "fm.keys(lane.notes(\"n\"))"),
        ("@m2/synths:fm-bell", "fm.bell(lane.notes(\"n\"))"),
        ("@m2/synths:fm-metal", "fm.metal(lane.notes(\"n\"))"),
        ("@m2/drums:drum-kit", "drums.kit(lane.notes(\"n\"))"),
        ("@m2/drums:drum", "drums.drum(lane.notes(\"n\"))"),
        ("@m2/effects:reverb", "reverb(osc.saw(110))"),
        ("@m2/effects:phaser", "phaser(osc.saw(110))"),
        ("@m2/mixing:eq", "eq(osc.saw(110))"),
        ("@m2/mixing:multiband", "multiband(osc.saw(110))"),
    ];
    let f = Fixture::new();
    f.write("music2.toml", format!("{MANIFEST}\"@m2/mixing\" = \"1\"\n"));
    let tracks: Vec<String> =
        calls.iter().enumerate().map(|(i, (_, call))| format!("const t{i} = track(\"t{i}\", ({{ lane }}) => {call});")).collect();
    let outs: Vec<String> = (0..calls.len()).map(|i| format!("t{i}: t{i}.out")).collect();
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\nimport {{ phaser, reverb }} from \"@m2/effects\";\n\
             import {{ eq, multiband }} from \"@m2/mixing\";\n\
             import {{ fm, synth }} from \"@m2/synths\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ mixer }} from \"std/mix\";\n\n{}\n\n\
             export default song(() => mixer({{ {} }}));\n",
            tracks.join("\n"),
            outs.join(", ")
        ),
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{:?} {:?}", s.draft_errors, s.problems());
    let view = s.view();
    let catalog = s.catalog();
    for (i, (id, _)) in calls.iter().enumerate() {
        let d = &catalog.components[*id];
        let module = view.modules.iter().find(|m| m.key == format!("t{i}")).unwrap();
        let card = module.nodes.iter().find(|n| n.title.as_deref() == Some(d.label.as_str())).unwrap_or_else(|| panic!("{id}'s card"));
        let mut want: Vec<&str> = d.editor.groups.iter().map(|g| g.name.as_str()).collect();
        let grouped: Vec<&str> = d.editor.groups.iter().flat_map(|g| g.items.iter().map(String::as_str)).collect();
        let values = d.controls.iter().map(|c| c.name.as_str()).chain(d.settings.iter().map(|x| x.name.as_str()));
        let views = d.editor.views.iter().map(|v| v.name.as_str());
        if !want.is_empty() && values.chain(views).any(|v| !grouped.contains(&v)) {
            want.push("More");
        }
        let got: Vec<&str> = card.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(got, want, "{id}'s groups");
        let views: Vec<&str> = card.views.iter().map(|v| v.name.as_str()).collect();
        let declared: Vec<&str> = d.editor.views.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(views, declared, "{id}'s views");
        let instrument = id.starts_with("@m2/synths:") || id.starts_with("@m2/drums:");
        assert!(!instrument || !d.editor.groups.is_empty(), "{id} has groups");
        let has = |slot: &str| d.controls.iter().any(|c| c.name == slot);
        if ["attack", "decay", "sustain", "release"].iter().all(|x| has(x)) {
            assert!(declared.contains(&"amp"), "{id} draws its envelope");
        }
        if has("cutoff") && has("resonance") {
            assert!(declared.contains(&"filter"), "{id} draws its filter");
        }
        if id.starts_with("@m2/synths:synth-") && has("saw")
            || ["@m2/synths:synth-supersaw", "@m2/synths:synth-lead", "@m2/synths:synth-pad"].contains(id)
        {
            let shape = card.knobs.iter().find(|k| k.slot.as_deref() == Some("shape")).unwrap_or_else(|| panic!("{id}'s shape"));
            assert_eq!(shape.kind, crate::kind::Kind::Choice, "{id}'s shape is a menu");
        }
    }
    // The Drum's menu (review Y11): its drums, a control, so a pick plays with no rebuild and can be
    // automated, labelled "sound" so its undo step reads "Drum sound" (KIT-2); its voices the one setting.
    // Since KIT-20 every drum the drum machine plays, the nine first, then its others; since KIT-28
    // the open hat after the hats.
    let drum = view.modules.iter().find(|m| m.key == "t13").unwrap().nodes.iter().find(|n| n.title.as_deref() == Some("Drum")).unwrap();
    let menu = drum.knobs.iter().find(|k| k.slot.as_deref() == Some("drum")).expect("the Drum's menu");
    assert_eq!((menu.kind, menu.label.as_str()), (crate::kind::Kind::Choice, "sound"));
    let names: Vec<String> = menu.options.iter().map(|o| o.value.as_str().unwrap().to_string()).collect();
    assert_eq!(
        names,
        [
            "kick",
            "kick 2",
            "snare",
            "snare 2",
            "clap",
            "hats",
            "open hat",
            "tom",
            "rim",
            "claves",
            "cowbell",
            "cymbal",
            "cymbal 2",
            "ride",
            "ride 2",
            "shaker",
            "shaker 2",
            "tambourine"
        ]
    );
    assert_eq!(drum.settings.iter().map(|x| x.slot.as_str()).collect::<Vec<_>>(), ["voices"]);
}

// CARD-34: every value of std and music2's own packs reads as words on its card. A control or a
// setting whose name is written in camelCase (`subOctave`, `tableA`) has a label ("sub octave"), so
// the card, its undo step and `m2 graph` never show the code's spelling.
#[test]
fn music2s_own_values_read_as_words() {
    let f = Fixture::new();
    // Every bundled pack (PACK-15), the song needing each so the catalog holds them all.
    f.write("music2.toml", format!("{MANIFEST}\"@m2/mixing\" = \"1\"\n\"@m2/vocals\" = \"1\"\n\"@m2/wavetable\" = \"1\"\n"));
    f.write("song.ts", "import { song } from \"@m2/core\";\nimport { mixer } from \"std/mix\";\nexport default song(() => mixer({}));\n");
    let s = f.open();
    let catalog = s.catalog();
    let camel = |name: &str| name.as_bytes().windows(2).any(|w| w[0].is_ascii_lowercase() && w[1].is_ascii_uppercase());
    let (mut seen, mut wrong) = (0, vec![]);
    for (id, d) in catalog.components.iter().filter(|(id, _)| id.starts_with("@m2/") || id.starts_with("std:")) {
        let controls = d.controls.iter().filter(|c| c.each.is_none()).map(|c| (&c.name, &c.label));
        for (name, label) in controls.chain(d.settings.iter().map(|x| (&x.name, &x.label))) {
            seen += 1;
            if camel(&crate::kind::named(label.as_deref(), name).label) {
                wrong.push(format!("{id}: {name}"));
            }
        }
    }
    assert!(seen > 200, "{seen} values");
    for pack in ["@m2/drums", "@m2/effects", "@m2/mixing", "@m2/synths", "@m2/vocals", "@m2/wavetable"] {
        assert!(catalog.components.keys().any(|id| id.starts_with(&format!("{pack}:"))), "{pack} is in the catalog");
    }
    assert!(wrong.is_empty(), "shown by a name in camelCase, so it wants a label (CARD-34): {wrong:?}");
}

// CARD-34 (review L2): every row of a card is named by the one rule, whatever drives its value: a
// value the code works out, a cable, or a signal written in the call. EQ3's frequencies are "low
// frequency", "mid frequency" and "high frequency" in each, never `lowFreq`; and the read-only
// value and the cable themselves (what `m2 graph` prints) say so too.
#[test]
fn every_row_of_a_card_is_named_by_its_label() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        r#"import { hz, song } from "@m2/core";
import { lfo } from "std/control";
import { osc } from "std/dsp";
import { eq3 } from "std/fx";
import { xrange } from "std/math";
import { mixer } from "std/mix";

export default song(({ time }) => {
  const sweep = xrange(lfo(time.beats), hz(500), hz(2000));
  return mixer({ a: eq3(osc.saw(110), { lowFreq: 100 * 2, midFreq: sweep, highFreq: xrange(lfo(time.beats, { period: 2 }), hz(3000), hz(6000)) }) });
});
"#,
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{:?} {:?}", s.draft_errors, s.problems());
    let view = s.view();
    let card = view.modules.iter().flat_map(|m| &m.nodes).find(|n| n.title.as_deref() == Some("EQ3")).expect("EQ3's card");
    let row = |slot: &str| {
        let p = card.inputs.iter().find(|p| p.slot.as_deref() == Some(slot)).unwrap_or_else(|| panic!("no {slot} in {:?}", card.inputs));
        (p.label.as_str(), p.entry.as_deref(), p.readonly.is_some(), p.inlet.is_some())
    };
    assert_eq!(row("lowFreq"), ("low frequency", None, true, false), "worked out by the code");
    assert_eq!(row("midFreq"), ("mid frequency", None, false, true), "fed by a cable");
    assert_eq!(row("highFreq").0, "high frequency", "fed by a signal written in the call");
    assert_eq!(card.readonly.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["low frequency"]);
    assert_eq!(card.inlets.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["mid frequency"]);
}

/// A sound whose card draws a picture over its values (CARD-60): `tone` in `lib/tone.ts`.
const PICTURED: &str = "import { axis, component, control, port, setting, view } from \"@m2/core\";
import { lowpass } from \"std/filter\";
import { osc } from \"std/dsp\";
export const tone = component(
  {
    id: \"tone\", label: \"Tone\",
    controls: { cutoff: control.hz(1000).range(20, 20000).log().accepts(\"semitones\", 0, 127), q: control.scalar(0.7), boost: control.db(0), shape: control.choice([\"soft\", \"hard\"], \"soft\"), on: control.bool(true) },
    settings: { voices: setting.int(4) },
    output: port.audio(),
    editor: {
      views: {
        curve: view.picture({ x: axis.log(\"hz\", 20, 20000), y: axis.lin(\"db\", -24, 24) }),
      },
      face: [\"curve\", \"cutoff\"],
    },
  },
  ({ cutoff, q }) => lowpass(osc.saw(cutoff), { q }),
  {
    curve: {
      draw: (v, p) => [p.handle(v.cutoff, v.boost, { x: \"cutoff\", y: \"boost\" })],
      words: (v) => `cut at ${v.cutoff} Hz`,
    },
  },
);
";

// CARD-63: a picture's view part carries its axes and, by name, every value its draw reads: what
// plays (written, the default, a lane's fallback; null in a unit it doesn't draw), the knob that moves it,
// and why the card can't, never its shapes or its words.
#[test]
fn a_pictures_values_are_in_the_view() {
    use crate::views::Drawn;
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write("lib/tone.ts", PICTURED);
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { tone } from \"./lib/tone\";\n\
         const lead = track(\"lead\", () => tone({ cutoff: 2400, shape: \"hard\", q: 2 }));\nexport default song(() => lead.out);\n",
    );
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let tone = |s: &m2_session::Session| {
        let v = s.view();
        let m = v.modules.into_iter().find(|m| m.key == "lead").unwrap();
        m.nodes.into_iter().find(|n| n.title.as_deref() == Some("Tone")).expect("Tone's card")
    };
    let card = tone(&s);
    let id = |slot: &str| card.knobs.iter().find(|k| k.slot.as_deref() == Some(slot)).map(|k| k.id.clone()).unwrap();
    let sent = serde_json::to_value(&card).unwrap();
    assert_eq!(
        sent["views"][0],
        json!({
            "name": "curve", "kind": "picture",
            "x": {"scale": "log", "unit": "hz", "lo": 20.0, "hi": 20000.0},
            "y": {"scale": "lin", "unit": "db", "lo": -24.0, "hi": 24.0},
            "values": [
                {"name": "cutoff", "value": 2400, "knob": id("cutoff")},
                {"name": "q", "value": 2, "knob": id("q")},
                {"name": "boost", "value": 0, "knob": id("boost")},
                {"name": "shape", "value": "hard", "knob": id("shape")},
                {"name": "on", "value": true, "knob": id("on")},
                {"name": "voices", "value": 4},
            ],
            "words": null,
        })
    );
    // Automated, a value plays its lane's fallback, which the card can't move, and says why.
    let q = id("q");
    s.automate(s.accepted().number, &q).unwrap();
    let Drawn::Picture { values, .. } = &tone(&s).views[0].of else { panic!("a picture") };
    let q = values.iter().find(|v| v.name == "q").unwrap();
    assert_eq!((&q.value, &q.knob, q.why.as_deref()), (&json!(2.0), &None, Some("it follows the q lane")));
}

// VOX-26: a call whose sound reads the song's key (`time.key`) says so on its card: Tune, and a
// component of the song's own that reads it. A call that doesn't (a reverb) says nothing, and nor
// does a track that reads it outside any call.
#[test]
fn a_call_that_reads_the_key_says_so() {
    let f = Fixture::new();
    f.write("music2.toml", format!("{MANIFEST}\"@m2/vocals\" = \"1\"\n"));
    f.write(
        "song.ts",
        "import { component, control, kernel, port, signature, song, time, track, type Audio, type Params } from \"@m2/core\";\n\
         import { osc } from \"std/dsp\";\nimport { reverb } from \"@m2/effects\";\nimport { mixer } from \"std/mix\";\nimport { tune } from \"@m2/vocals\";\n\
         const keyed = component({ id: \"keyed\", label: \"Keyed\", signature: signature<(x: Audio) => Audio>(), inputs: { in: port.audio() }, controls: {}, output: port.audio() },\n\
           (x: Audio, _: Params) => kernel(\"@m2/vocals:shift\", { in: x, semitones: kernel(\"@m2/vocals:snap\", { hz: 220, clarity: 1, key: time.key }) }));\n\
         const vox = track(\"vox\", () => reverb(tune(osc.saw(220))));\n\
         const own = track(\"own\", () => keyed(osc.saw(110)));\n\
         const bare = track(\"bare\", () => kernel(\"@m2/vocals:shift\", { in: osc.saw(330), semitones: kernel(\"@m2/vocals:snap\", { hz: 330, clarity: 1, key: time.key }) }));\n\
         export default song(() => mixer({ vox: vox.out, own: own.out, bare: bare.out }));\n",
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let reads = |module: &str| -> Vec<(String, bool)> {
        let v = s.view();
        let m = v.modules.iter().find(|m| m.key == module).unwrap();
        m.nodes.iter().filter(|n| n.kind == "call").map(|n| (n.label.clone(), n.reads_key)).collect()
    };
    assert_eq!(reads("vox").iter().filter(|(_, r)| *r).map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["tune"], "{:?}", reads("vox"));
    assert!(reads("vox").iter().any(|(l, r)| l == "reverb" && !r), "{:?}", reads("vox"));
    assert!(reads("own").iter().any(|(l, r)| l == "keyed" && *r), "{:?}", reads("own"));
    assert!(reads("bare").iter().all(|(_, r)| !r), "no call there reads it: {:?}", reads("bare"));
}

// VOX-20 (review U10): an audio track's lane card takes an effect from its +, as a call's result
// does: Tune goes into the track's own code, around what it returns, so the song, a send and a
// Duck's side all hear the tuned voice. One Tx; its pack's line and lock come with it.
#[test]
fn tune_goes_into_an_audio_tracks_own_code() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "tracks/vox.ts",
        "import { track } from \"@m2/core\";\n\nexport default track(\"vox\", { name: \"Vox\" }, ({ lane }) => lane.audio(\"clips\"));\n",
    );
    f.write("song.ts", "import { song } from \"@m2/core\";\nimport { mixer } from \"std/mix\";\nimport vox from \"./tracks/vox\";\nexport default song(() => mixer({ vox: vox.out }));\n");
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let v = s.view();
    let lane = v.modules.iter().find(|m| m.key == "vox").unwrap().nodes.iter().find(|n| n.kind == "lane").unwrap().clone();
    assert!(lane.audio, "the vocal lane's card has a +: {lane:?}");
    let at = s.accepted().number;
    s.add_effect(at, &lane.id, "@m2/vocals:tune").unwrap();
    assert!(
        f.read("tracks/vox.ts").contains("({ lane }) => tune(lane.audio(\"clips\"), { glide: 0.1, amount: 1 }));"),
        "around what the track returns: {}",
        f.read("tracks/vox.ts")
    );
    assert!(f.read("tracks/vox.ts").contains("import { tune } from \"@m2/vocals\";"));
    assert!(f.read("music2.toml").contains("\"@m2/vocals\" = \"1.1\""), "{}", f.read("music2.toml"));
    assert!(f.read("song.ts").contains("vox: vox.out"), "the song reads the track as it did");
}

// CARD-72: a picture knows the notes lane that plays its call, as the keys view does (taste.md DV-9), so
// its `p.play` place plays there: the Drum's hit on its track's hits; an effect's picture, which no
// notes lane plays, none.
#[test]
fn a_picture_knows_the_notes_lane_that_plays_its_call() {
    use crate::views::Drawn;
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { limit } from \"std/fx\";\n\
         const kick = track(\"kick\", ({ lane }) => limit(drums.drum(lane.notes(\"hits\"))));\nexport default song(() => kick.out);\n",
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let m = s.view().modules.into_iter().find(|m| m.key == "kick").unwrap();
    let lane_of = |title: &str| {
        let card = m.nodes.iter().find(|n| n.title.as_deref() == Some(title)).unwrap_or_else(|| panic!("{title}'s card"));
        let Drawn::Picture { lane, .. } = &card.views[0].of else { panic!("not a picture: {:?}", card.views[0].of) };
        lane.as_ref().map(|l| (l.module.clone(), l.lane.clone()))
    };
    assert_eq!(lane_of("Drum"), Some(("kick".to_string(), "hits".to_string())));
    assert_eq!(lane_of("Limit"), None);
}
