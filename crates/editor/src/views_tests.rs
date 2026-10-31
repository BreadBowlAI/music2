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
pub(crate) const REGIONS: &str = "[{ file: \"c4.wav\", root: 60, keys: [48, 72] }, { file: \"c5.wav\", root: 72, keys: [73, 96], velocity: [1, 100] }]";

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
    assert_eq!((sent["kind"].clone(), sent["value"].clone(), sent["options"][1].clone()), (json!("choice"), json!("square"), json!({"value": "square", "label": "square"})));
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
    let done = s.write_all(at, &[(decay.id.clone(), json!(0.25)), (sustain.id.clone(), json!(0.5)), (attack.id.clone(), json!(0.01))]).unwrap();
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
    f.write("data/lead/shape.json", steps_file("{\"id\": \"a\", \"start\": 0, \"length\": 4, \"points\": [[0, \"sine\"], [2, \"sqaure\"]]}"));
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
    assert!(file().contains(&format!("{{\"id\": \"{id}\", \"start\": 4, \"length\": 4, \"points\": [[0, \"saw\"], [2, \"sine\"]]}}")), "{}", file());
    let e = edit(&mut s, &Gesture::Points { clip: clip.clone(), points: vec![(0.0, name("squar"))] }).unwrap_err().to_string();
    assert!(e.contains("lead's shape lane is one of \"saw\", \"square\" or \"sine\", not \"squar\""), "{e}");
    let e = edit(&mut s, &Gesture::Points { clip: clip.clone(), points: vec![(0.0, Point::Number(1.0))] }).unwrap_err().to_string();
    assert!(e.contains("lead's shape lane is one of \"saw\", \"square\" or \"sine\", not 1"), "{e}");
    let before = file();
    edit(&mut s, &Gesture::Move { clips: vec![clip.clone()], by: 4.0, to: None, copy: false }).unwrap();
    assert!(file().contains(&format!("{{\"id\": \"{id}\", \"start\": 8, \"length\": 4, \"points\": [[0, \"saw\"], [2, \"sine\"]]}}")), "{}", file());
    s.undo().unwrap();
    assert_eq!(file(), before, "one undo step");
    let split = edit(&mut s, &Gesture::Split { clips: vec![clip.clone()], at: 7.0 }).unwrap();
    let second = split.made[0].1.clone();
    assert!(file().contains(&format!("{{\"id\": \"{id}\", \"start\": 4, \"length\": 3, \"points\": [[0, \"saw\"], [2, \"sine\"]]}}")), "{}", file());
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
    let input = |slot: &str| card.inputs.iter().find(|p| p.slot.as_deref() == Some(slot)).unwrap_or_else(|| panic!("no {slot} input")).id.clone();
    let regions = card.settings.iter().find(|x| x.slot == "regions").expect("the regions setting").id.clone();
    use crate::views::Row::{Input, Setting, View};
    let groups: Vec<(&str, Vec<crate::views::Row>)> = card.groups.iter().map(|g| (g.name.as_str(), g.rows.clone())).collect();
    assert_eq!(
        groups,
        [
            ("Sound", vec![Input(input("shape")), Input(input("sub")), View("scope".into())]),
            ("Filter", vec![View("filter".into()), Input(input("cutoff")), Input(input("q"))]),
            ("Amp", vec![View("amp".into()), Input(input("attack")), Input(input("decay")), Input(input("sustain")), Input(input("release"))]),
            ("More", vec![Setting(regions), View("keys".into())]),
        ]
    );
    assert!(card.inputs.iter().any(|p| p.kind == "notes" && p.slot.is_none()), "the notes input is a connection: {:?}", card.inputs);
    let sent = serde_json::to_value(&card).unwrap();
    assert_eq!(sent["groups"][0], json!({"name": "Sound", "rows": [{"input": input("shape")}, {"input": input("sub")}, {"view": "scope"}]}));
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
        Drawn::Envelope { attack, .. } => {
            assert_eq!(attack.knob, None);
            assert_eq!(attack.why.as_deref(), Some("it follows the attack lane"));
        }
        other => panic!("not an envelope: {other:?}"),
    }
}
