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
