//! The card's menus, switches, groups and views (CARD-20 to CARD-29): a sound that declares every
//! kind and view, as the kinds track's `every` test component does, played by a project.

use serde_json::{Value as Json, json};

use crate::kind::{Choice, Kind};
use crate::testing::{Fixture, MANIFEST};
use crate::{Gestures, Knob, ViewNode};

/// A sound with a choice, a switch, an envelope's and a filter's controls, regions, its four views
/// and three groups (the design's §3.5 example).
pub(crate) const WOBBLE: &str = "import { component, control, port, setting, view } from \"@m2/core\";
import { lowpass } from \"std/filter\";
import { sampler } from \"std/sampler\";

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
      views: { amp: view.envelope(\"attack\", \"decay\", \"sustain\", \"release\"), filter: view.filter(\"cutoff\", \"q\"), keys: view.keys(\"regions\"), scope: view.scope() },
      groups: { Sound: [\"shape\", \"sub\", \"scope\"], Filter: [\"filter\", \"cutoff\", \"q\"], Amp: [\"amp\", \"attack\", \"decay\", \"sustain\", \"release\"] },
    },
  },
  (notes, { cutoff, q, attack, release, regions }) => lowpass(sampler(notes, { regions, attack, release }), { cutoff, q }),
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

// CARD-26: the view carries the card's views, resolved for the call: each part of an envelope and
// a filter names its slot and its knob; a keys view its setting, the regions the Sampler plays and
// the notes lane; a scope nothing. A part driven by a lane has no knob, and says why.
#[test]
fn a_cards_views_are_in_the_view() {
    use crate::views::{Drawn, KeyRegion};
    let f = wobble(&format!("{{ shape: \"square\", cutoff: 1200, regions: {REGIONS} }}"));
    let mut s = f.open();
    let card_now = card(&s);
    let knob_id = |slot: &str| card_now.knobs.iter().find(|k| k.slot.as_deref() == Some(slot)).map(|k| k.id.clone());
    let names: Vec<&str> = card_now.views.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["amp", "filter", "keys", "scope"]);
    match &card_now.views[0].of {
        Drawn::Envelope { attack, decay, sustain, release } => {
            for (part, slot) in [(attack, "attack"), (decay, "decay"), (sustain, "sustain"), (release, "release")] {
                assert_eq!((part.slot.as_str(), &part.knob, &part.why), (slot, &knob_id(slot), &None), "{slot}");
            }
        }
        other => panic!("not an envelope: {other:?}"),
    }
    match &card_now.views[1].of {
        Drawn::Filter { cutoff, q } => {
            assert_eq!((cutoff.knob.clone(), q.knob.clone()), (knob_id("cutoff"), knob_id("q")));
            assert!(cutoff.knob.is_some());
        }
        other => panic!("not a filter: {other:?}"),
    }
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
    assert_eq!(sent["views"][0]["attack"]["slot"], "attack");
    // The attack automated: its part has no knob, and says the lane drives it.
    let attack = knob(&s, "attack");
    s.automate(s.accepted().number, &attack.id).unwrap();
    match &card(&s).views[0].of {
        Drawn::Envelope { attack, decay, .. } => {
            assert_eq!(attack.knob, None);
            assert_eq!(attack.why.as_deref(), Some("it follows the attack lane"));
            // Drawn where it plays with no clip: the lane's fallback, in its unit (V6).
            assert_eq!((attack.value, attack.unit), (Some(0.005), Some("seconds")));
            // The other corners stay the card's to move.
            assert_eq!((decay.knob.clone(), decay.value), (knob_id("decay"), None));
        }
        other => panic!("not an envelope: {other:?}"),
    }
}

// CARD-26 (review V2): a view moves a knob only in a unit it draws. A cutoff written in semitones
// (std's FREQ accepts them: `semis(72)`) has no knob in the filter's picture, which says why, and
// draws nothing for it; its q, in its own unit, is still the card's.
#[test]
fn a_view_moves_only_a_knob_in_a_unit_it_draws() {
    use crate::views::Drawn;
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "lib/tone.ts",
        "import { component, control, port, view } from \"@m2/core\";\nimport { lowpass } from \"std/filter\";\nimport { osc } from \"std/dsp\";\n\
         export const tone = component(\n  { id: \"tone\", label: \"Tone\", controls: { cutoff: control.hz(1000).range(20, 20000).log().accepts(\"semitones\", 0, 127), q: control.scalar(0.7) },\n\
         output: port.audio(), editor: { views: { filter: view.filter(\"cutoff\", \"q\") } } },\n  ({ cutoff, q }) => lowpass(osc.saw(cutoff), { q }),\n);\n",
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
    match &card.views[0].of {
        Drawn::Filter { cutoff, q } => {
            assert_eq!((&cutoff.knob, cutoff.value), (&None, None));
            assert_eq!(cutoff.why.as_deref(), Some("it is written in semitones, which this picture doesn't draw"));
            assert!(q.knob.is_some() && q.why.is_none(), "{q:?}");
        }
        other => panic!("not a filter: {other:?}"),
    }
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

// CARD-31: every sound music2's own packs list is dressed, and its card shows it: the groups its
// descriptor declares, in their order (More last, for what no group names), and its views. Every
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
    ];
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let tracks: Vec<String> =
        calls.iter().enumerate().map(|(i, (_, call))| format!("const t{i} = track(\"t{i}\", ({{ lane }}) => {call});")).collect();
    let outs: Vec<String> = (0..calls.len()).map(|i| format!("t{i}: t{i}.out")).collect();
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\nimport {{ phaser, reverb }} from \"@m2/effects\";\n\
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
        if values.chain(views).any(|v| !grouped.contains(&v)) {
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
    // One drum's menu (review Y11): its drums, a control, so a pick plays with no rebuild and can be
    // automated, labelled "sound" so its undo step reads "One drum sound"; its voices the one setting.
    let drum = view.modules.iter().find(|m| m.key == "t13").unwrap().nodes.iter().find(|n| n.title.as_deref() == Some("One drum")).unwrap();
    let menu = drum.knobs.iter().find(|k| k.slot.as_deref() == Some("drum")).expect("One drum's menu");
    assert_eq!((menu.kind, menu.label.as_str()), (crate::kind::Kind::Choice, "sound"));
    let names: Vec<String> = menu.options.iter().map(|o| o.value.as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["kick", "snare", "clap", "hats", "tom", "rim", "cowbell", "cymbal", "shaker"]);
    assert_eq!(drum.settings.iter().map(|x| x.slot.as_str()).collect::<Vec<_>>(), ["voices"]);
}

// CARD-34: every value of std and music2's own packs reads as words on its card. A control or a
// setting whose name is written in camelCase (`subOctave`, `tableA`) has a label ("sub octave"), so
// the card, its undo step and `m2 graph` never show the code's spelling.
#[test]
fn music2s_own_values_read_as_words() {
    let f = Fixture::new();
    // Every bundled pack (PACK-15), the song needing each so the catalog holds them all.
    f.write("music2.toml", format!("{MANIFEST}\"@m2/mixing\" = \"1\"\n\"@m2/wavetable\" = \"1\"\n"));
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
    for pack in ["@m2/drums", "@m2/effects", "@m2/mixing", "@m2/synths", "@m2/wavetable"] {
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
