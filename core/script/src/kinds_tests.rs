//! The kinds a descriptor declares (`@m2/core` 1.8.0, CARD-10 to CARD-17): a choice and a switch
//! among its controls, lanes of names, views and groups. TypeScript refuses each mistake as the
//! code is typed (`sdk/check/kinds.ts`); the build refuses it again, in words, since types can be
//! bypassed: each refusal here is written past the types, with `as any`.

use std::time::Duration;

use m2_ir::{Node, Op, Unit, split_ref};

use super::*;
use crate::component::{Group, View, ViewOf};

/// A sound that uses every kind a descriptor declares: `every` in `sdk/check/kinds.ts`, which `tsc`
/// checks as the code editor does. `tone` and `wave` are for handing a choice on.
const EVERY: &str = r#"import { component, control, kernel, port, setting, view, type Audio } from "@m2/core";
export const every = component(
  {
    id: "every", label: "Every kind", inputs: { notes: port.notes() },
    controls: {
      shape: control.choice(["saw", "square", "sine"], "saw"), sub: control.bool(false),
      cutoff: control.hz(800).range(40, 8000).log(), q: control.scalar(0.7),
      attack: control.secs(0.005), decay: control.ms(200), sustain: control.frac(0.6), release: control.secs(0.3),
    },
    settings: { voices: setting.int(8), regions: setting.regions() },
    output: port.audio(),
    editor: {
      category: "Instruments",
      views: { amp: view.envelope("attack", "decay", "sustain", "release"), filter: view.filter("cutoff", "q"), keys: view.keys("regions"), scope: view.scope() },
      groups: { Sound: ["shape", "sub", "scope"], Filter: ["filter", "cutoff", "q"], Amp: ["amp", "attack", "decay", "sustain", "release"] },
    },
  },
  (notes, { shape, sub, cutoff }) => kernel<Audio>("test:every", { shape, sub, cutoff }),
);
export const tone = component(
  { id: "tone", controls: { cutoff: control.hz(800) }, output: port.audio() },
  ({ cutoff }) => kernel<Audio>("test:tone", { cutoff }),
);
export const wave = component(
  { id: "wave", controls: { shape: control.choice(["sine", "saw"], "sine"), on: control.bool(true) }, output: port.audio() },
  ({ shape, on }) => kernel<Audio>("test:wave", { shape, on }),
);
"#;

fn build_of(files: &[(&str, &str)]) -> Result<Build, Vec<Diagnostic>> {
    let code: BTreeMap<String, String> = files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut o = Options::new(code, "song.ts");
    o.packages.push(("std".into(), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")));
    o.dependencies.push("std".into());
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

/// A song whose function (given `{ lane }`) returns `body`, with `lib/every.ts` imported.
fn song(body: &str) -> Result<Build, Vec<Diagnostic>> {
    let song = format!(
        "import {{ song }} from \"@m2/core\";\nimport {{ every, tone, wave }} from \"./lib/every\";\nexport default song(({{ lane }}) => {{\n{body}\n}});\n"
    );
    build_of(&[("lib/every.ts", EVERY), ("song.ts", &song)])
}

fn built(body: &str) -> Build {
    song(body).unwrap_or_else(|e| panic!("{body}: {e:?}"))
}

/// That `body` doesn't build, with an error that says `says`.
fn refused(body: &str, says: &str) {
    let e = song(body).err().unwrap_or_else(|| panic!("{body} builds"));
    assert!(e.iter().any(|d| d.message.contains(says)), "{body}: {says}\n{e:?}");
}

/// The node feeding input `port` of the song's kernel `kernel`.
fn feeding<'a>(b: &'a Build, kernel: &str, port: &str) -> &'a Node {
    let m = &b.ir.modules[0];
    let k = m.nodes.iter().find(|n| matches!(&n.op, Op::Kernel { kernel: k, .. } if k == kernel)).expect("the kernel");
    let Op::Kernel { inputs, .. } = &k.op else { unreachable!() };
    let (id, _) = split_ref(&inputs[port]);
    m.nodes.iter().find(|n| n.id == id).expect("the node it reads")
}

/// The constant a node is, with its unit.
fn constant(n: &Node) -> (f64, String) {
    match &n.op {
        Op::Const { value, unit } => (*value, unit.map_or_else(String::new, |u| u.to_string())),
        op => panic!("not a constant: {op:?}"),
    }
}

// CARD-10, CARD-12: a choice is written by its name, and its constant holds the option's place,
// in the choice's own unit: a cell of the call's control (PKG-3), written or left out.
#[test]
fn a_choice_reaches_its_kernel_as_its_place() {
    let b = built("return every(lane.notes(\"n\"), { shape: \"sine\" });");
    let c = feeding(&b, "test:every", "shape");
    assert_eq!(constant(c), (2.0, "choice<saw|square|sine>".to_string()));
    let o = c.origin.as_ref().expect("a cell");
    assert_eq!((o.arg.clone(), o.slot(), o.inherited()), (vec!["1".to_string(), "shape".to_string()], Some("shape"), false));
    // Left out: the default, saw, the first.
    let b = built("return every(lane.notes(\"n\"));");
    let c = feeding(&b, "test:every", "shape");
    assert_eq!(constant(c).0, 0.0);
    assert!(c.origin.as_ref().is_some_and(|o| o.inherited()));
    // The descriptor carries the choice as its unit, and its default as its place.
    let d = b.components.iter().find(|d| d.id == "every").expect("its descriptor");
    let shape = d.control("shape").unwrap();
    let names: Vec<&str> = shape.options().unwrap().names().iter().map(|n| &**n).collect();
    assert_eq!(names, ["saw", "square", "sine"]);
    assert_eq!(shape.default.as_ref().map(|d| (d.value, d.spelled.clone())), Some((0.0, None)));
    assert!(d.is_instrument(), "a choice with a default lets an instrument build from its notes alone");
}

// CARD-11, CARD-12: a switch is true or false, and its constant holds 1 or 0, in the unit `switch`.
#[test]
fn a_switch_reaches_its_kernel_as_0_or_1() {
    let b = built("return every(lane.notes(\"n\"), { sub: true });");
    assert_eq!(constant(feeding(&b, "test:every", "sub")), (1.0, "switch".to_string()));
    let b = built("return every(lane.notes(\"n\"));");
    assert_eq!(constant(feeding(&b, "test:every", "sub")), (0.0, "switch".to_string()));
    let d = b.components.iter().find(|d| d.id == "every").unwrap();
    assert_eq!(d.control("sub").unwrap().unit, Some(Unit::Switch));
}

// CARD-10: a name the choice doesn't have, a place, or anything else is refused in words naming
// the options; so is a choice declared with names that can't be its unit, or a default it doesn't
// have.
#[test]
fn a_wrong_choice_is_refused_in_words() {
    refused(
        "return every(lane.notes(\"n\"), { shape: \"squar\" as any });",
        "every(): shape is one of \"saw\", \"square\", \"sine\", not \"squar\"",
    );
    refused("return every(lane.notes(\"n\"), { shape: 1 as any });", "every(): shape is one of \"saw\", \"square\", \"sine\", not 1");
    refused("return every(lane.notes(\"n\"), { shape: true as any });", "not true");
    for (declared, says) in [
        ("control.choice([\"a\", \"a\"], \"a\")", "its option \"a\" is named twice"),
        ("control.choice([\"a|b\"])", "must be one line, without |, < or >"),
        ("control.choice([\" a\"])", "has a space at an end"),
        ("control.choice([])", "a choice needs a list of 1 to 64 names"),
        ("control.choice([\"abcdefghijklmnopqrstuvwxy\"])", "a name of 1 to 24 characters"),
        ("control.choice([\"a\", \"b\"], \"c\" as any)", "its default must be one of \"a\", \"b\", not \"c\""),
        ("(control.choice([\"a\"]) as any).range(0, 1)", "is a choice, not a number"),
    ] {
        let lib = format!(
            "import {{ component, control, kernel }} from \"@m2/core\";\nexport const c = component({{ id: \"c\", controls: {{ x: {declared} }} }}, (p: any) => kernel(\"test:c\", p));\n"
        );
        let e = build_of(&[
            ("lib/c.ts", &lib),
            ("song.ts", "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(() => c());\n"),
        ]);
        let e = e.err().unwrap_or_else(|| panic!("{declared} builds"));
        assert!(e.iter().any(|d| d.message.contains(says) && d.message.contains("the control \"x\"")), "{declared}: {says}\n{e:?}");
    }
}

// CARD-11: a switch takes only true or false.
#[test]
fn a_wrong_switch_is_refused_in_words() {
    refused("return every(lane.notes(\"n\"), { sub: 1 as any });", "every(): sub is true or false, not 1");
    refused("return every(lane.notes(\"n\"), { sub: \"on\" as any });", "every(): sub is true or false, not \"on\"");
    let lib = "import { component, control, kernel } from \"@m2/core\";\nexport const c = component({ id: \"c\", controls: { x: control.bool(1 as any) } }, (p: any) => kernel(\"test:c\", p));\n";
    let e = build_of(&[
        ("lib/c.ts", lib),
        ("song.ts", "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(() => c());\n"),
    ]);
    assert!(e.err().unwrap().iter().any(|d| d.message.contains("its default must be true or false, not 1")));
}

// CARD-13: past the types, a choice or a switch given where a number goes, and a choice given to a
// choice of other options, are refused in words where they are given.
#[test]
fn a_choice_or_a_switch_is_refused_where_a_number_goes() {
    // A lib whose `wired` hands its choice and its switch on to a number, and to a choice of other options.
    let wired = |body: &str| {
        let lib = format!(
            "{EVERY}export const wired = component({{ id: \"wired\", inputs: {{ notes: port.notes() }}, controls: {{ shape: control.choice([\"saw\", \"square\"], \"saw\"), sub: control.bool(true) }}, output: port.audio() }},\n  (notes: any, {{ shape, sub }}: any) => {body});\n"
        );
        let song = "import { song } from \"@m2/core\";\nimport { wired } from \"./lib/every\";\nexport default song(({ lane }) => wired(lane.notes(\"n\")));\n";
        build_of(&[("lib/every.ts", &lib), ("song.ts", song)]).err().unwrap_or_else(|| panic!("{body} builds"))
    };
    let says = |e: &[Diagnostic], what: &str| assert!(e.iter().any(|d| d.message.contains(what)), "{what}\n{e:?}");
    says(&wired("tone({ cutoff: shape })"), "tone(): cutoff is a number (hz), and this is a choice of \"saw\", \"square\"");
    says(&wired("tone({ cutoff: sub })"), "tone(): cutoff is a number (hz), and this is a switch");
    says(&wired("every(notes, { shape })"), "every(): shape is one of \"saw\", \"square\", \"sine\": give it a name, or a lane of them");
    says(&wired("every(notes, { shape })"), "not a choice of \"saw\", \"square\"");
    says(&wired("every(notes, { sub: shape })"), "every(): sub is true or false");
    // A choice of the same options, and a switch, hand on.
    let ok = format!(
        "{EVERY}export const same = component({{ id: \"same\", inputs: {{ notes: port.notes() }}, controls: {{ shape: control.choice([\"saw\", \"square\", \"sine\"], \"sine\"), sub: control.bool(true) }}, output: port.audio() }},\n  (notes: any, {{ shape, sub }}: any) => every(notes, {{ shape, sub }}));\n"
    );
    let song = "import { song } from \"@m2/core\";\nimport { same } from \"./lib/every\";\nexport default song(({ lane }) => same(lane.notes(\"n\")));\n";
    let b = build_of(&[("lib/every.ts", &ok), ("song.ts", song)]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(constant(feeding(&b, "test:every", "shape")), (2.0, "choice<saw|square|sine>".to_string()));
}

/// The lane `key` of the song as the IR declares it: its kind and its fallback.
fn lane_of(b: &Build, key: &str) -> (String, Option<f64>) {
    let l = b.ir.modules[0].lanes.iter().find(|l| l.key == key).expect("the lane");
    (l.kind.to_string(), l.default)
}

// CARD-14: a lane of names is declared as `lane.control` declares any lane, with its kind where a
// unit goes; it takes the options of the choice it feeds, and its fallback is a place made from
// its name, the first when left out. A switch's lane falls back to 1 or 0.
#[test]
fn a_lane_of_names_takes_the_options_of_the_choice_it_feeds() {
    let b = built(
        "return every(lane.notes(\"n\"), { shape: lane.control(\"shape\", \"choice\", \"square\"), sub: lane.control(\"sub\", \"switch\", true) });",
    );
    assert_eq!(lane_of(&b, "shape"), ("control<choice<saw|square|sine>>".to_string(), Some(1.0)));
    assert_eq!(lane_of(&b, "sub"), ("control<switch>".to_string(), Some(1.0)));
    let n = feeding(&b, "test:every", "shape");
    assert!(matches!(&n.op, Op::Lane { lane } if lane == "shape"), "the kernel reads the lane");
    let b =
        built("return every(lane.notes(\"n\"), { shape: lane.control(\"shape\", \"choice\"), sub: lane.control(\"sub\", \"switch\") });");
    assert_eq!((lane_of(&b, "shape").1, lane_of(&b, "sub").1), (Some(0.0), Some(0.0)));
    // One lane may feed several choices of the same options.
    built(
        "const s = lane.control(\"shape\", \"choice\", \"sine\");\nevery(lane.notes(\"n\"), { shape: s });\nreturn every(lane.notes(\"n\"), { shape: s });",
    );
}

// CARD-14: a lane of names that feeds no choice, feeds choices of other options, or falls back to a
// name its choice doesn't have, is refused in words; so is a fallback of another kind, and a lane
// of names where a number goes.
#[test]
fn a_lane_of_names_is_refused_in_words() {
    refused(
        "lane.control(\"shape\", \"choice\", \"saw\");\nreturn every(lane.notes(\"n\"));",
        "lane \"shape\" of \"song\" is a lane of names, but it feeds no choice",
    );
    refused(
        "return every(lane.notes(\"n\"), { shape: lane.control(\"shape\", \"choice\", \"tri\") });",
        "every(): shape: the lane \"shape\" falls back to \"tri\", which isn't one of \"saw\", \"square\", \"sine\"",
    );
    refused(
        "const s = lane.control(\"shape\", \"choice\", \"saw\");\nwave({ shape: s });\nreturn every(lane.notes(\"n\"), { shape: s });",
        "the lane \"shape\" also feeds wave()'s shape, one of \"sine\", \"saw\": a lane of names has one set of names",
    );
    refused(
        "return every(lane.notes(\"n\"), { sub: lane.control(\"sub\", \"switch\", 1 as any) });",
        "lane \"sub\": a switch's lane falls back to true or false, not 1",
    );
    refused(
        "return every(lane.notes(\"n\"), { shape: lane.control(\"shape\", \"choice\", 2 as any) });",
        "a choice's lane falls back to one of its choice's names",
    );
    refused(
        "return tone({ cutoff: lane.control(\"shape\", \"choice\", \"saw\") as any });",
        "tone(): cutoff is a number (hz), and this is a lane of names",
    );
    refused(
        "return every(lane.notes(\"n\"), { shape: lane.control(\"sub\", \"switch\") as any });",
        "every(): shape is one of \"saw\", \"square\", \"sine\"",
    );
}

/// The descriptor `d` declares, built in a song that calls it: `component(d, …)` with `controls`.
fn declared(controls: &str, settings: &str, editor: &str) -> Result<Build, Vec<Diagnostic>> {
    let lib = format!(
        "import {{ component, control, kernel, port, setting, view }} from \"@m2/core\";\n\
         export const c = component({{ id: \"c\", controls: {{ {controls} }}, settings: {{ {settings} }}, editor: {editor} as any }}, (p: any) => kernel(\"test:c\", {{}}));\n"
    );
    build_of(&[
        ("lib/c.ts", &lib),
        ("song.ts", "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(() => c());\n"),
    ])
}

fn declaration_refused(controls: &str, settings: &str, editor: &str, says: &str) {
    let e = declared(controls, settings, editor).err().unwrap_or_else(|| panic!("{editor} builds"));
    assert!(e.iter().any(|d| d.message.contains(says)), "{editor}: {says}\n{e:?}");
}

const ADSR: &str = "a: control.ms(5), d: control.beats(1), s: control.db(-6), r: control.secs(0.2), f: control.khz(2), q: control.scalar(1), shape: control.choice([\"lo\", \"hi\"], \"lo\")";

// CARD-30: a filter view's q is a q (a plain number) or a resonance from 0 to 1, as a synth's knob
// says it; anything else is refused in words naming both.
#[test]
fn a_filter_views_q_is_a_q_or_a_resonance() {
    let b = declared("f: control.hz(800), res: control.frac(0.3)", "", "{ views: { lp: view.filter(\"f\", \"res\") } }")
        .unwrap_or_else(|e| panic!("{e:?}"));
    let d = b.components.iter().find(|d| d.id == "c").unwrap();
    assert_eq!(d.editor.views[0].of, ViewOf::Filter { cutoff: "f".into(), q: "res".into() });
    declaration_refused(
        "f: control.hz(800), g: control.db(0)",
        "",
        "{ views: { lp: view.filter(\"f\", \"g\") } }",
        "the filter's q names \"g\", a control in db, but it must be a q (scalar or unit01)",
    );
}

// CARD-16: a view names this very descriptor's controls, in a unit that fits (or for keys its
// regions or keymap setting), by a name of its own; the descriptor carries the views in order.
#[test]
fn views_are_checked_against_their_descriptor() {
    let b = declared(ADSR, "zones: setting.keymap()", "{ views: { env: view.envelope(\"a\", \"d\", \"s\", \"r\"), lp: view.filter(\"f\", \"q\"), map: view.keys(\"zones\"), out: view.scope() } }")
        .unwrap_or_else(|e| panic!("{e:?}"));
    let d = b.components.iter().find(|d| d.id == "c").unwrap();
    let s = |x: &str| x.to_string();
    assert_eq!(
        d.editor.views,
        [
            View { name: s("env"), of: ViewOf::Envelope { attack: s("a"), decay: s("d"), sustain: s("s"), release: s("r") } },
            View { name: s("lp"), of: ViewOf::Filter { cutoff: s("f"), q: s("q") } },
            View { name: s("map"), of: ViewOf::Keys { regions: s("zones") } },
            View { name: s("out"), of: ViewOf::Scope },
        ]
    );
    for (editor, says) in [
        (
            "{ views: { env: view.envelope(\"a\", \"d\", \"s\", \"rel\") } }",
            "editor.views' \"env\": the envelope's release names \"rel\", which isn't a control of this sound",
        ),
        (
            "{ views: { lp: view.filter(\"a\", \"q\") } }",
            "editor.views' \"lp\": the filter's cutoff names \"a\", a control in seconds, but it must be a frequency (hz)",
        ),
        (
            "{ views: { env: view.envelope(\"a\", \"d\", \"shape\", \"r\") } }",
            "the envelope's sustain names \"shape\", a choice, but it must be a level (unit01 or gain or db)",
        ),
        (
            "{ views: { env: view.envelope(\"f\", \"d\", \"s\", \"r\") } }",
            "the envelope's attack names \"f\", a control in hz, but it must be a time (seconds or beats)",
        ),
        (
            "{ views: { map: view.keys(\"file\") } }",
            "the key map's setting names \"file\", a recording setting, but a key map shows regions or a keymap",
        ),
        ("{ views: { map: view.keys(\"a\") } }", "the key map's setting names \"a\", which isn't a setting of this sound"),
        ("{ views: { f: view.scope() } }", "editor.views' \"f\": \"f\" is a value of this sound; give the view a name of its own"),
        ("{ views: { env: { kind: \"envelope\" } } }", "editor.views' \"env\" must be made with view.envelope()"),
        ("{ looks: {} }", "editor has no field \"looks\""),
    ] {
        declaration_refused(ADSR, "file: setting.asset(\"a.wav\")", editor, says);
    }
    // No views: the descriptor's editor keeps its bytes.
    let b = declared(ADSR, "", "{}").unwrap();
    let json = serde_json::to_string(&b.components.iter().find(|d| d.id == "c").unwrap().editor).unwrap();
    assert!(!json.contains("views") && !json.contains("groups"), "{json}");
}

// CARD-17: groups name this descriptor's values and views, each in one group, once; the descriptor
// carries them in order.
#[test]
fn groups_are_checked_against_their_descriptor() {
    let b = declared(
        ADSR,
        "n: setting.int(1)",
        "{ views: { lp: view.filter(\"f\", \"q\") }, groups: { Filter: [\"lp\", \"f\", \"q\"], Other: [\"n\", \"shape\"] } }",
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let d = b.components.iter().find(|d| d.id == "c").unwrap();
    let g = |name: &str, items: &[&str]| Group { name: name.into(), items: items.iter().map(|s| s.to_string()).collect() };
    assert_eq!(d.editor.groups, [g("Filter", &["lp", "f", "q"]), g("Other", &["n", "shape"])]);
    for (editor, says) in [
        ("{ groups: { One: [\"a\", \"b\"] } }", "editor.groups' \"One\" names \"b\", which isn't a value or a view of this sound"),
        (
            "{ groups: { One: [\"a\"], Two: [\"d\", \"a\"] } }",
            "editor.groups' \"Two\" names \"a\", which is in \"One\" too: each value or view goes in one group, once",
        ),
        ("{ groups: { One: [\"a\", \"a\"] } }", "editor.groups' \"One\" names \"a\", which is in it already"),
        ("{ groups: { One: \"a\" } }", "editor.groups' \"One\" must be a list of this sound's values and views"),
        ("{ groups: { \"A name that is far too long\": [\"a\"] } }", "a group's name is one line of 1 to 24 characters"),
        ("{ groups: { \"\": [\"a\"] } }", "a group's name is one line of 1 to 24 characters"),
    ] {
        declaration_refused(ADSR, "n: setting.int(1)", editor, says);
    }
}

// CARD-16, CARD-17, PKG-11: the catalog editors read (`Session::catalog`, from `describe`) carries a
// package's views and groups as the build does.
#[test]
fn the_catalog_carries_views_and_groups() {
    let dir = std::env::temp_dir().join(format!("m2-script-{}-kinds-catalog", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/every.ts"), EVERY).unwrap();
    let catalog = describe(&[("pkg".into(), dir.clone())]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let d = &catalog.components["pkg:every"];
    let views: Vec<&str> = d.editor.views.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(views, ["amp", "filter", "keys", "scope"]);
    let groups: Vec<(&str, usize)> = d.editor.groups.iter().map(|g| (g.name.as_str(), g.items.len())).collect();
    assert_eq!(groups, [("Sound", 3), ("Filter", 3), ("Amp", 5)]);
    assert!(d.control("shape").and_then(|c| c.options()).is_some());
    assert_eq!(catalog.instruments().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["pkg:every"]);
}

/// A song with a track `a` whose function is `track`, then the song's `song`; `lib/every.ts` imported.
fn two(track: &str, song: &str) -> Result<Build, Vec<Diagnostic>> {
    let text = format!(
        "import {{ song, track }} from \"@m2/core\";\nimport {{ every, tone, wave }} from \"./lib/every\";\n\
         const a = track(\"a\", ({{ lane }}) => {{\n{track}\n}});\nexport default song(({{ lane }}) => {{\n{song}\n}});\n"
    );
    build_of(&[("lib/every.ts", EVERY), ("song.ts", &text)])
}

// CARD-14 (review K4): a choice read from another track keeps its kind: an import's kind is its
// export's, known once every module has run, and a choice given one is checked then, as any is. A
// lane of names exported to another track takes the options of the choice it feeds there.
#[test]
fn a_choice_from_another_track_keeps_its_kind() {
    // The lane, fed in its own track and in the song.
    let both = "const s = lane.control(\"shape\", \"choice\", \"square\");\nreturn { out: every(lane.notes(\"n\"), { shape: s }), s };";
    two(both, "return every(lane.notes(\"n\"), { shape: a.s });").unwrap_or_else(|e| panic!("{e:?}"));
    // The lane, fed only in the song: it takes the options there.
    let b = two(
        "return { out: tone(), s: lane.control(\"shape\", \"choice\", \"square\") };",
        "return every(lane.notes(\"n\"), { shape: a.s });",
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let lane = b.ir.modules.iter().find(|m| m.key == "a").unwrap().lanes.iter().find(|l| l.key == "shape").unwrap();
    assert_eq!((lane.kind.to_string(), lane.default), ("control<choice<saw|square|sine>>".to_string(), Some(1.0)));
    // A switch, too.
    two("return { out: tone(), on: lane.control(\"on\", \"switch\", true) };", "return every(lane.notes(\"n\"), { sub: a.on });")
        .unwrap_or_else(|e| panic!("{e:?}"));
    // A choice of other options, a number, and a switch where a choice goes, each refused in words.
    for (track, says) in [
        (
            "const w = lane.control(\"w\", \"choice\", \"sine\");\nwave({ shape: w });\nreturn { out: tone(), s: w };",
            "the lane \"w\" also feeds wave()'s shape, one of \"sine\", \"saw\": a lane of names has one set of names",
        ),
        ("return { out: tone(), s: lane.control(\"f\", \"hz\", 800) };", "not a.s, a number in hz"),
        ("return { out: tone(), s: lane.control(\"on\", \"switch\") };", "not a.s, a switch"),
    ] {
        let e = two(track, "return every(lane.notes(\"n\"), { shape: a.s as any });").err().unwrap_or_else(|| panic!("{track} builds"));
        assert!(e.iter().any(|d| d.message.contains(says)), "{track}: {says}\n{e:?}");
    }
}

// CARD-14 (review K11): a lane of names declared twice agrees when its fallbacks name one option, a
// fallback left out being the first; else it is `lane.conflict`, in words about a choice's lane.
#[test]
fn a_lane_of_names_declared_twice_agrees_on_its_fallback() {
    let twice = |a: &str, b: &str| {
        song(&format!(
            "every(lane.notes(\"n\"), {{ shape: lane.control(\"shape\", \"choice\"{a}) }});\nreturn every(lane.notes(\"n\"), {{ shape: lane.control(\"shape\", \"choice\"{b}) }});"
        ))
    };
    for (a, b) in [("", ", \"saw\""), (", \"saw\"", ""), (", \"sine\"", ", \"sine\"")] {
        twice(a, b).unwrap_or_else(|e| panic!("{a} and {b}: {e:?}"));
    }
    let e = twice("", ", \"square\"").err().expect("a conflict");
    let d = e.iter().find(|d| d.code == "lane.conflict").unwrap_or_else(|| panic!("{e:?}"));
    assert!(
        d.message.contains("a choice's lane falling back to \"saw\"") && d.message.contains("falling back to \"square\""),
        "{}",
        d.message
    );
    assert!(!d.message.contains("control<choice>"), "{}", d.message);
}
