//! A call's settings on its card, and the one write that changes them (CARD-1 to CARD-5).

use std::collections::BTreeSet;

use m2_script::component::SettingKind;
use m2_session::{Home, Session};
use serde_json::json;

use super::*;
use crate::gate5_tests::every_component;
use crate::import::{self, Source};
use crate::kind::Kind;
use crate::testing::{Fixture, MANIFEST};
use crate::{Gestures, ViewNode};

/// A song whose `song.ts` is `text`.
fn song(text: &str) -> Fixture {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write("song.ts", text);
    f
}

/// The card of the project's call to `callee`.
fn card(s: &Session, callee: &str) -> ViewNode {
    let site = s.accepted().compiled.sites.iter().find(|x| x.in_project && x.callee == callee).unwrap_or_else(|| panic!("no `{callee}`"));
    let id = format!("{}#{}", site.file, site.ordinal);
    s.view().modules.into_iter().flat_map(|m| m.nodes).find(|n| n.id == id).unwrap_or_else(|| panic!("no card for `{callee}`"))
}

/// Setting `slot` of the card of the call to `callee`.
fn setting(s: &Session, callee: &str, slot: &str) -> Setting {
    let card = card(s, callee);
    card.settings.iter().find(|x| x.slot == slot).cloned().unwrap_or_else(|| panic!("no {slot} in {:?}", card.settings))
}

/// The kind a setting kind is on a card (CARD-2), and whether it is whole.
fn kind_of(k: SettingKind) -> (Kind, bool) {
    match k {
        SettingKind::Number => (Kind::Number, false),
        SettingKind::Int => (Kind::Number, true),
        SettingKind::Bool => (Kind::Switch, false),
        SettingKind::Choice => (Kind::Choice, false),
        SettingKind::Asset => (Kind::Recording, false),
        SettingKind::Keymap => (Kind::Keymap, false),
        SettingKind::Regions => (Kind::Regions, false),
        SettingKind::Key => (Kind::Track, false),
    }
}

// CARD-1, CARD-2, HOME-1: on a song that calls every std component, each call's card carries every
// setting its component declares, in the descriptor's order, then each dictionary entry's, with
// the kind its declaration says, a choice's options, its value as JSON, written or not, and its
// home. A value left out plays its default; a written one is read from the call.
#[test]
fn every_setting_of_every_std_component_is_on_its_card() {
    let f = every_component();
    let s = f.open();
    let rev = s.accepted();
    let view = s.view();
    let mut kinds = BTreeSet::new();
    let mut cards = 0;
    for m in &rev.compiled.ir.modules {
        let mv = view.modules.iter().find(|v| v.key == m.key).unwrap();
        for frame in &m.frames {
            let Some(cid) = &frame.component else { continue };
            let Some(site) = rev.compiled.sites.iter().find(|x| x.id == frame.site && x.in_project) else { continue };
            let Some(node) = mv.nodes.iter().find(|n| n.id == format!("{}#{}", site.file, site.ordinal) && n.kind == "call") else {
                continue;
            };
            let d = &rev.compiled.components[cid];
            let at = format!("{}:{} {cid}", site.file, site.line);
            let own: Vec<&str> = node.settings.iter().filter(|x| !x.slot.contains('.')).map(|x| x.slot.as_str()).collect();
            assert_eq!(own, d.settings.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), "{at}: its own, in order");
            for x in &node.settings {
                let decl = match x.slot.split_once('.') {
                    None => d.settings.iter().find(|y| y.name == x.slot).unwrap(),
                    Some((dict, rest)) => {
                        let each = d.controls.iter().find(|c| c.name == dict).and_then(|c| c.each.as_deref()).unwrap();
                        let name = rest.rsplit('.').next().unwrap();
                        each.settings.iter().find(|y| y.name == name).unwrap_or_else(|| panic!("{at}: {}", x.slot))
                    }
                };
                assert_eq!((x.kind, x.whole), kind_of(decl.kind), "{at}: {}", x.slot);
                assert_eq!(x.home, Home::Code, "{at}: {}", x.slot);
                assert_eq!(x.id, format!("{}#{}/@{}", site.file, site.ordinal, x.slot), "{at}");
                if decl.kind == SettingKind::Choice {
                    let options: Vec<Json> = decl.options.iter().flatten().map(|o| json!(o)).collect();
                    assert_eq!(x.options.iter().map(|o| o.value.clone()).collect::<Vec<_>>(), options, "{at}: {}", x.slot);
                }
                if !x.written {
                    assert_eq!(x.value, decl.default.clone().unwrap_or(Json::Null), "{at}: {} plays its default", x.slot);
                }
                kinds.insert(format!("{:?}", x.kind));
            }
            assert!(d.settings.is_empty() || !node.settings.is_empty(), "{at}: its settings");
            cards += !node.settings.is_empty() as usize;
        }
    }
    assert!(cards >= 29, "{cards} cards with settings");
    for k in ["Number", "Choice", "Recording", "Keymap", "Regions", "Track"] {
        assert!(kinds.contains(k), "no {k} among {kinds:?}");
    }
    // Some by name: the LFO's shape plays its default; the Sampler's mode is written; a voice count is whole.
    let shape = setting(&s, "lfo", "shape");
    assert_eq!((shape.kind, &shape.value, shape.written, &shape.readonly), (Kind::Choice, &json!("sine"), false, &None));
    assert_eq!(shape.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["sine", "tri", "saw", "square"]);
    let mode = setting(&s, "sampler", "mode");
    assert_eq!((&mode.value, mode.written, &mode.readonly), (&json!("gate"), true, &None));
    let voices = setting(&s, "sampler", "voices");
    assert_eq!((voices.kind, voices.whole, &voices.value, voices.written), (Kind::Number, true, &json!(8), false));
    let sample = setting(&s, "sampler", "sample");
    assert_eq!((sample.kind, &sample.value, sample.written, &sample.readonly), (Kind::Recording, &Json::Null, false, &None));
    // Regions and keymaps are shown in words here, and changed in code (their picture is later).
    let regions = setting(&s, "sampler", "regions");
    assert_eq!((regions.kind, &regions.value, regions.written), (Kind::Regions, &json!([]), false));
    assert!(regions.readonly.is_some());
    // The mixer's channels: each entry's `to`, a menu of the song's tracks.
    let to = setting(&s, "mixer", "channels.chops.to");
    assert_eq!((to.kind, to.label.as_str(), &to.value, to.written, &to.readonly), (Kind::Track, "chops.to", &Json::Null, false, &None));
}

/// A song with a component of its own whose settings are of every kind a card changes, played
/// through std's Sampler, and an LFO on its level.
const BOX: &str = r#"import { component, port, setting, song, track } from "@m2/core";
import { lfo } from "std/control";
import { vca } from "std/dsp";
import { master, mixer } from "std/mix";
import { sampler } from "std/sampler";

const box = component(
  {
    id: "box",
    label: "Box",
    inputs: { notes: port.notes() },
    settings: {
      held: setting.bool(false),
      voices: setting.int(4),
      sample: setting.asset().optional(),
      tone: setting.choice(["soft", "hard"], "soft"),
      follows: setting.key().optional(),
    },
    output: port.audio(),
  },
  (notes, p) => sampler(notes, { sample: p.sample, mode: p.held ? "gate" : "oneshot", voices: p.voices, tune: p.tone === "hard" ? 12 : 0 }),
);

const keys = track("keys", ({ lane }) => box(lane.notes("notes"), { voices: 6 }));

export default song(({ time }) => master(mixer({ keys: vca(keys.out, lfo(time.beats, { period: 4 })) })));
"#;

/// Setting `slot` of `callee` set to `value` at the accepted revision, then undone: `after` is the
/// call's code after it, exactly; one Tx, one undo step, and the code as it was after the undo.
fn set_then_undo(f: &Fixture, s: &mut Session, callee: &str, slot: &str, value: Json, from: &str, to: &str) {
    let before = f.read("song.ts");
    assert!(before.contains(from), "{from} in {before}");
    let id = setting(s, callee, slot).id;
    let steps = s.last_step();
    let at = s.accepted().number;
    let done = set(s, at, &id, &value, None).unwrap_or_else(|e| panic!("{slot}: {e}"));
    assert_eq!(f.read("song.ts"), before.replacen(from, to, 1), "{slot}: the code after, exactly");
    assert_eq!(s.last_step(), steps + 1, "{slot}: one undo step");
    assert!(done.revision > at, "{slot}: a new revision");
    let now = setting(s, callee, slot);
    assert_eq!((&now.value, now.written), (&value, true), "{slot}: the card shows it written");
    crate::undo(s).unwrap();
    assert_eq!(f.read("song.ts"), before, "{slot}: undo puts the code back, exactly");
}

// CARD-3, CARD-4: a choice, a switch and a whole number are each one Tx that writes the literal into
// the call by PKG-3's insertion rules (or over the literal written there), one undo step.
#[test]
fn a_choice_a_switch_and_a_number_are_written_into_the_call() {
    let f = song(BOX);
    let mut s = f.open();
    // Left out: written into the options object, after what it holds.
    set_then_undo(
        &f,
        &mut s,
        "lfo",
        "shape",
        json!("square"),
        "lfo(time.beats, { period: 4 })",
        r#"lfo(time.beats, { period: 4, shape: "square" })"#,
    );
    set_then_undo(
        &f,
        &mut s,
        "box",
        "held",
        json!(true),
        "box(lane.notes(\"notes\"), { voices: 6 })",
        "box(lane.notes(\"notes\"), { voices: 6, held: true })",
    );
    set_then_undo(&f, &mut s, "box", "tone", json!("hard"), "{ voices: 6 }", r#"{ voices: 6, tone: "hard" }"#);
    // Written: the literal is replaced.
    set_then_undo(&f, &mut s, "box", "voices", json!(12), "{ voices: 6 }", "{ voices: 12 }");
    // A choice written with single quotes keeps them.
    f.write("song.ts", BOX.replace("{ period: 4 }", "{ period: 4, shape: 'saw' }"));
    assert!(s.refresh());
    assert_eq!(setting(&s, "lfo", "shape").value, json!("saw"));
    set_then_undo(&f, &mut s, "lfo", "shape", json!("tri"), "shape: 'saw'", "shape: 'tri'");
}

// CARD-4: a value of the wrong kind is refused in words, and nothing is written: a fraction where a
// whole number goes, a word that isn't one of a choice's options, a number for a switch.
#[test]
fn a_value_of_the_wrong_kind_is_refused_in_words() {
    let f = song(BOX);
    let mut s = f.open();
    let before = f.read("song.ts");
    let at = s.accepted().number;
    let refused = |s: &mut Session, callee: &str, slot: &str, value: Json| {
        let id = setting(s, callee, slot).id;
        match set(s, at, &id, &value, None) {
            Err(EditError::Rejected(why)) => why,
            other => panic!("{slot} = {value}: {other:?}"),
        }
    };
    let why = refused(&mut s, "box", "voices", json!(2.5));
    assert!(why.contains("voices is a whole number") && why.contains("2.5"), "{why}");
    let why = refused(&mut s, "lfo", "shape", json!("triangle"));
    assert!(why.contains(r#"shape is one of "sine", "tri", "saw", "square""#) && why.contains("triangle"), "{why}");
    let why = refused(&mut s, "box", "held", json!(1));
    assert!(why.contains("held is true or false, not 1"), "{why}");
    let why = refused(&mut s, "box", "held", json!("on"));
    assert!(why.contains(r#"held is true or false, not "on""#), "{why}");
    // CARD-4, CARD-5: a recording names a file in assets/, as a drop's does.
    let why = refused(&mut s, "box", "sample", json!("missing.wav"));
    assert!(why.contains("missing.wav isn't in assets/") && why.contains("drop the file on the card"), "{why}");
    // CARD-2: a key setting names a track by its key; the build judges which.
    let why = refused(&mut s, "box", "follows", json!("Not A Key"));
    assert!(why.contains("follows names a track by its key") && why.contains("Not A Key"), "{why}");
    assert_eq!(f.read("song.ts"), before, "nothing written");
    assert_eq!(s.accepted().number, at, "no revision");
}

// CARD-3: a value the code works out shows read-only, with the code that works it out, and the
// write refuses it in words; so does a setting whose call's options aren't written in it.
#[test]
fn a_value_the_code_works_out_is_read_only() {
    let f = song(&BOX.replace("const keys", "const shape = \"saw\";\nconst keys").replace("{ period: 4 }", "{ period: 4, shape: shape }"));
    let mut s = f.open();
    let shape = setting(&s, "lfo", "shape");
    assert_eq!((&shape.value, shape.written, shape.expression.as_deref()), (&Json::Null, true, Some("shape")));
    assert!(shape.readonly.as_deref().is_some_and(|w| w.contains("worked out")), "{shape:?}");
    let before = f.read("song.ts");
    let at = s.accepted().number;
    let e = set(&mut s, at, &shape.id, &json!("tri"), None).unwrap_err().to_string();
    assert!(e.contains("can be changed only in its code"), "{e}");
    assert_eq!(f.read("song.ts"), before);
    // Options given as a variable: what the call gives isn't known from the call.
    let f = song(
        &BOX.replace("box(lane.notes(\"notes\"), { voices: 6 })", "box(lane.notes(\"notes\"), opts)")
            .replace("const keys", "const opts = { voices: 6 };\nconst keys"),
    );
    let s = f.open();
    let voices = setting(&s, "box", "voices");
    assert_eq!(voices.value, Json::Null);
    assert!(voices.readonly.is_some(), "{voices:?}");
}

/// A short WAV.
fn tone() -> Vec<u8> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
    for i in 0..4_410 {
        w.write_sample(((i as f32 * 0.03).sin() * 12_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    out.into_inner()
}

// CARD-5: a file dropped on a recording comes in as Import Audio brings one in, then one Tx writes
// its name into the call; one undo step takes the code back, and the file stays. The planner checks
// that the call's kernels read it.
#[test]
fn a_recording_takes_a_dropped_file() {
    let f = song(BOX);
    let mut s = f.open();
    let before = f.read("song.ts");
    let sample = setting(&s, "box", "sample");
    assert_eq!((sample.kind, &sample.value, &sample.readonly), (Kind::Recording, &Json::Null, &None));
    let kick = import::audio(&f.0, Source::Bytes { name: "kick.wav".into(), bytes: tone() }, &mut |_| {}).unwrap();
    let steps = s.last_step();
    let at = s.accepted().number;
    set_recording(&mut s, at, &sample.id, &kick).unwrap();
    assert_eq!(f.read("song.ts"), before.replace("{ voices: 6 }", r#"{ voices: 6, sample: "kick.wav" }"#));
    assert_eq!(s.last_step(), steps + 1);
    assert_eq!(setting(&s, "box", "sample").value, json!("kick.wav"));
    // The same file again is refused in words.
    let at = s.accepted().number;
    let e = set_recording(&mut s, at, &sample.id, &kick).unwrap_err().to_string();
    assert!(e.contains("plays kick.wav already"), "{e}");
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read("song.ts"), before);
    assert!(f.0.join("assets/kick.wav").is_file(), "the file stays");
}

// HOME-1 as the view sends it: every setting names its home.
#[test]
fn every_setting_names_its_home_as_sent() {
    let f = song(BOX);
    let s = f.open();
    let view = serde_json::to_value(s.view()).unwrap();
    let mut n = 0;
    for node in view["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap()) {
        for x in node["settings"].as_array().into_iter().flatten() {
            assert_eq!(x["home"], "code", "{x}");
            n += 1;
        }
    }
    assert!(n >= 5, "{n} settings");
}

// CARD-6: a plugin's parameter is a switch when stepped from 0 to 1, a choice of the plugin's names
// when stepped with 2 to 64 steps all named differently (each writing its step's number), else a
// number.
#[test]
fn a_plugin_parameter_is_a_switch_a_choice_or_a_number() {
    use crate::kind::{Choice, of_param};
    let names = |n: &[&str]| n.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let filters = names(&["LP", "HP", "BP", "Notch"]);
    assert_eq!(
        of_param(true, 0.0, 3.0, Some(&filters)),
        (Kind::Choice, vec![Choice::new(0, "LP"), Choice::new(1, "HP"), Choice::new(2, "BP"), Choice::new(3, "Notch")])
    );
    assert_eq!(of_param(true, 1.0, 2.0, Some(&names(&["Up", "Down"]))), (Kind::Choice, vec![Choice::new(1, "Up"), Choice::new(2, "Down")]));
    assert_eq!(of_param(true, 0.0, 1.0, Some(&names(&["Off", "On"]))), (Kind::Switch, vec![Choice::new(0, "Off"), Choice::new(1, "On")]));
    assert_eq!(of_param(true, 0.0, 1.0, None), (Kind::Switch, vec![Choice::new(0, "Off"), Choice::new(1, "On")]));
    // Two steps named alike, a step with no name, too many steps, not stepped: a number.
    assert_eq!(of_param(true, 0.0, 2.0, Some(&names(&["A", "A", "B"]))).0, Kind::Number);
    assert_eq!(of_param(true, 0.0, 2.0, Some(&names(&["A", "", "B"]))).0, Kind::Number);
    assert_eq!(of_param(true, 0.0, 2.0, None).0, Kind::Number);
    let many: Vec<String> = (0..65).map(|i| format!("step {i}")).collect();
    assert_eq!(of_param(true, 0.0, 64.0, Some(&many)).0, Kind::Number);
    assert_eq!(of_param(false, 0.0, 3.0, Some(&filters)).0, Kind::Number);
}

// CARD-6 with a real plugin: Airwindows Consolidated's Bypass is stepped from 0 to 1, which the host
// names "Off" and "On" when it describes the plugin: its knob is a switch. Its Dry/Wet is a number.
// (Surge XT 1.3.4 marks none of its parameters stepped, so none of them is a choice.) Skips without
// the plugin.
#[test]
fn a_stepped_plugin_parameter_from_0_to_1_is_a_switch() {
    m2_plugins::test_main_thread();
    if m2_plugins::find("com.airwindows.consolidated").is_err() {
        eprintln!("skipping: com.airwindows.consolidated is not installed");
        return;
    }
    let f = song(
        "import { plugin, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\
         export default song(() => plugin(\"clap:com.airwindows.consolidated\", { in: osc.saw(110), bypass_2920836328: 0, dry_wet_2944917344: 0.35 }));\n",
    );
    let s = f.open();
    let (param, steps) = s.packages.plugin_param("clap:com.airwindows.consolidated", "bypass_2920836328").expect("Bypass");
    assert_eq!((param.stepped, steps), (true, Some(&["Off".to_string(), "On".to_string()][..])), "the host named its steps");
    let card = card(&s, "plugin");
    let knob =
        |label: &str| card.knobs.iter().find(|k| k.label.starts_with(label)).unwrap_or_else(|| panic!("no {label} in {:?}", card.knobs));
    let bypass = knob("bypass");
    assert_eq!(
        (bypass.kind, bypass.options.iter().map(|o| (o.value.clone(), o.label.as_str())).collect::<Vec<_>>()),
        (Kind::Switch, vec![(json!(0), "Off"), (json!(1), "On")])
    );
    assert_eq!((knob("dry").kind, knob("dry").options.len()), (Kind::Number, 0));
}

// CARD-4: the value a setting has already writes nothing: no revision, no undo step, the code as it
// was; whether it plays its default (left out) or is written.
#[test]
fn the_value_a_setting_has_already_writes_nothing() {
    let f = song(BOX);
    let mut s = f.open();
    let before = f.read("song.ts");
    let (at, steps) = (s.accepted().number, s.last_step());
    for (callee, slot, value) in [("lfo", "shape", json!("sine")), ("box", "voices", json!(6)), ("box", "held", json!(false))] {
        let id = setting(&s, callee, slot).id;
        let done = set(&mut s, at, &id, &value, None).unwrap_or_else(|e| panic!("{slot}: {e}"));
        assert_eq!(done.revision, at, "{slot}: no revision");
    }
    assert_eq!((f.read("song.ts"), s.last_step(), s.accepted().number), (before, steps, at));
}

// CARD-4: a setting write is confined to its call. A component that hands its setting to code
// outside its call (here through a variable the song reads after it) would change a node made
// there: refused in words, and nothing is written.
#[test]
fn a_setting_whose_change_reaches_outside_its_call_is_refused() {
    let f = song(
        r#"import { component, port, setting, song } from "@m2/core";
import { gain, osc } from "std/dsp";

let level = 1;
const box = component(
  { id: "box", label: "Box", inputs: { in: port.audio() }, settings: { level: setting.number(1) }, output: port.audio() },
  (x, p) => {
    level = p.level;
    return gain(x, 0.5);
  },
);

export default song(() => {
  const a = box(osc.saw(110));
  return gain(a, level);
});
"#,
    );
    let mut s = f.open();
    let before = f.read("song.ts");
    let x = setting(&s, "box", "level");
    assert_eq!((&x.value, &x.readonly), (&json!(1), &None));
    let at = s.accepted().number;
    let e = set(&mut s, at, &x.id, &json!(0.5), None).unwrap_err().to_string();
    assert!(e.contains("setting level would change more than its call"), "{e}");
    assert_eq!(f.read("song.ts"), before, "nothing written");
}

// CARD-3, C4: a Sampler that plays zones shows its file read-only, as theirs, and its zones say
// what they play: one file by its name, more by how many and which.
#[test]
fn a_samplers_zones_say_their_files_and_take_its_file() {
    let one = song(
        "import { song, track } from \"@m2/core\";\nimport { sampler } from \"std/sampler\";\n\
         const s = track(\"s\", ({ lane }) => sampler(lane.notes(\"n\"), { zones: { 60: \"tone.wav\" } }));\n\
         export default song(() => s.out);\n",
    );
    let s = one.open();
    let zones = setting(&s, "sampler", "zones");
    assert_eq!((zones.kind, zones.written, zones.said.as_deref()), (Kind::Keymap, true, Some("tone.wav")));
    let sample = setting(&s, "sampler", "sample");
    assert!(sample.readonly.as_deref().is_some_and(|w| w.contains("This Sampler plays zones")), "{sample:?}");
    let two = song(
        "import { song, track } from \"@m2/core\";\nimport { sampler } from \"std/sampler\";\n\
         const s = track(\"s\", ({ lane }) => sampler(lane.notes(\"n\"), { zones: { 36: \"a.wav\", 38: \"b.wav\" } }));\n\
         export default song(() => s.out);\n",
    );
    let s = two.open();
    assert_eq!(setting(&s, "sampler", "zones").said.as_deref(), Some("2 files: a.wav, b.wav"));
}

// CARD-4, CARD-5 (C2): a recording set by the write alone (`m2 set`, the route) names a file in
// assets/, and the planner checks that the call's sound reads it, as for a dropped file.
#[test]
fn a_recording_written_by_name_must_be_a_file_the_sound_reads() {
    let f = song(BOX);
    f.write("assets/kick.wav", tone());
    let mut s = f.open();
    let before = f.read("song.ts");
    let id = setting(&s, "box", "sample").id;
    let at = s.accepted().number;
    set(&mut s, at, &id, &json!("kick.wav"), None).unwrap();
    assert_eq!(f.read("song.ts"), before.replace("{ voices: 6 }", r#"{ voices: 6, sample: "kick.wav" }"#));
    let c = &s.accepted().compiled;
    let mut reads = false;
    for m in &c.ir.modules {
        crate::controls::walk(&m.nodes, &mut |n| {
            reads |= matches!(&n.op, m2_ir::Op::Kernel { assets, .. } if assets.contains(&"kick.wav".to_string()))
        });
    }
    assert!(reads, "a kernel reads it");
}

/// A song whose mixer has a bus channel keyed `wet` (not its bus's key), reading the Reverb bus.
fn bused() -> (Fixture, Session) {
    let f = crate::testing::fixture("examples/first-song");
    let mut s = f.open();
    crate::bus::add_bus(&mut s, crate::bus::BusKind::Reverb, None, None).unwrap();
    f.write("song.ts", f.read("song.ts").replace("      reverb: reverb.out,", "      wet: reverb.out,"));
    assert!(s.refresh());
    (f, s)
}

// CARD-2, CARD-4 (C1): a mixer channel's `to` offers exactly what the song's mixer takes: Master
// (none) and the bus channels it can go to without a loop, by their channel keys, named by their
// buses. Written into a bare channel, it wraps it; Master takes it out again. Anything else is
// refused in words. A key setting of any other sound offers no list of its own (the page offers the
// song's tracks) and takes any key, which the build judges.
#[test]
fn a_mixer_channels_to_offers_master_and_its_bus_channels() {
    let (f, mut s) = bused();
    let to = setting(&s, "mixer", "channels.kick.to");
    assert_eq!((to.kind, to.label.as_str(), &to.value, to.written, &to.readonly), (Kind::Track, "kick.to", &Json::Null, false, &None));
    let options: Vec<(Json, &str)> = to.options.iter().map(|o| (o.value.clone(), o.label.as_str())).collect();
    assert_eq!(options, [(Json::Null, "Master"), (json!("wet"), "Reverb")]);
    let wet = setting(&s, "mixer", "channels.wet.to");
    assert_eq!(wet.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["Master"], "a bus can't go to itself");
    set_then_undo(
        &f,
        &mut s,
        "mixer",
        "channels.kick.to",
        json!("wet"),
        "      kick: kick.out,",
        "      kick: { in: kick.out, to: \"wet\" },",
    );
    // Master takes `to` out: a channel left with only its input is bare again.
    let before = f.read("song.ts");
    let at = s.accepted().number;
    set(&mut s, at, &to.id, &json!("wet"), None).unwrap();
    let (at, id) = (s.accepted().number, setting(&s, "mixer", "channels.kick.to").id);
    set(&mut s, at, &id, &Json::Null, None).unwrap();
    assert_eq!(f.read("song.ts"), before, "Master takes it out again");
    assert_eq!(setting(&s, "mixer", "channels.kick.to").value, Json::Null);
    // A track that isn't a bus channel of this mixer, and a bus's own key, are refused in words.
    let at = s.accepted().number;
    for wrong in ["hats", "reverb"] {
        let e = set(&mut s, at, &to.id, &json!(wrong), None).unwrap_err().to_string();
        assert!(e.contains(r#"kick.to is one of Master, "wet""#) && e.contains(wrong), "{e}");
    }
    assert_eq!(f.read("song.ts"), before);
    // Any other key setting: no list of its own, any key.
    let f = song(BOX);
    let mut s = f.open();
    let follows = setting(&s, "box", "follows");
    assert_eq!((follows.kind, follows.options.len(), &follows.readonly), (Kind::Track, 0, &None));
    set_then_undo(&f, &mut s, "box", "follows", json!("keys"), "{ voices: 6 }", r#"{ voices: 6, follows: "keys" }"#);
}

// CARD-6 with a real host: the probe plugin's Mode is stepped from 0 to 2, and the host names its
// steps Soft, Warm and Hard when it describes the plugin. Its knob is a choice of those names, each
// writing its step's number; the menu's write puts the number in the code, one undo step. Its
// Bright, stepped from 0 to 1 and named Off and On, is a switch.
#[test]
fn a_plugins_named_steps_become_a_menu() {
    m2_plugins::test_main_thread();
    m2_plugins::probe::install();
    let f = song(
        "import { plugin, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\
         export default song(() => plugin(\"clap:m2.probe\", { in: osc.saw(110), mode_3: 1, bright_4: 0 }));\n",
    );
    let mut s = f.open();
    let (param, steps) = s.packages.plugin_param("clap:m2.probe", "mode_3").expect("Mode");
    assert_eq!((param.stepped, param.min, param.max), (true, 0.0, 2.0));
    assert_eq!(steps, Some(&["Soft".to_string(), "Warm".to_string(), "Hard".to_string()][..]), "the host named its steps");
    let knob = card(&s, "plugin").knobs.into_iter().find(|k| k.label.starts_with("mode")).expect("Mode's knob");
    let options: Vec<(Json, &str)> = knob.options.iter().map(|o| (o.value.clone(), o.label.as_str())).collect();
    assert_eq!((knob.kind, knob.value), (Kind::Choice, json!(1)));
    assert_eq!(options, [(json!(0), "Soft"), (json!(1), "Warm"), (json!(2), "Hard")]);
    let bright = card(&s, "plugin").knobs.into_iter().find(|k| k.label.starts_with("bright")).expect("Bright's knob");
    let options: Vec<(Json, &str)> = bright.options.iter().map(|o| (o.value.clone(), o.label.as_str())).collect();
    assert_eq!((bright.kind, options), (Kind::Switch, vec![(json!(0), "Off"), (json!(1), "On")]));
    let before = f.read("song.ts");
    let (at, steps) = (s.accepted().number, s.last_step());
    s.set(at, &knob.id, 2.0, None).unwrap();
    assert_eq!(f.read("song.ts"), before.replace("mode_3: 1", "mode_3: 2"));
    assert_eq!(s.last_step(), steps + 1);
}
