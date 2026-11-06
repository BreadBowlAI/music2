//! An instrument with an input a call may leave out (PKG-20): it is still an instrument, built from
//! its notes alone, and its optional input (PKG-18) is given by its place after the notes. An input
//! may be labelled, for its card's row of another track (CARD-35). The rest of PKG-18 is
//! `optional_input_tests.rs`'.

use std::time::Duration;

use m2_ir::{Op, split_ref};

use super::*;

/// A drum whose `cutBy` is an optional notes input that listens: its kernel reads `cut` only when
/// it is given.
const DRUM: &str = r#"import { component, control, kernel, poly, port, signature, type Audio, type Events } from "@m2/core";
export const drum = component(
  {
    id: "drum", label: "Drum",
    signature: signature<(notes: Events, cutBy?: Events) => Audio>(),
    inputs: { notes: port.notes(), cutBy: port.notes({ sidechain: true, label: "Cut by" }).optional() },
    controls: { tune: control.semis(0).range(-24, 24) },
    output: port.audio(),
    editor: { category: "Drums" },
  },
  (notes: Events, cutBy: Events | undefined, p: any): Audio => {
    const cut = cutBy === undefined ? undefined : poly(cutBy, (n) => kernel("test:mark", { pitch: n.pitch }));
    return poly(notes, (n) => kernel("test:drum", cut === undefined ? { pitch: n.pitch, tune: p.tune } : { pitch: n.pitch, tune: p.tune, cut }));
  },
);
"#;

fn walk(ns: &[m2_ir::Node], f: &mut impl FnMut(&m2_ir::Node)) {
    for n in ns {
        f(n);
        if let Op::Poly { body, .. } = &n.op {
            walk(&body.nodes, f);
        }
    }
}

fn build_of(files: &[(&str, &str)]) -> Result<Build, Vec<Diagnostic>> {
    let code: BTreeMap<String, String> = files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut o = Options::new(code, "song.ts");
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

/// A song whose track `a` plays `call` (the drum, imported), and a track `b` that returns its notes.
fn song(call: &str) -> Result<Build, Vec<Diagnostic>> {
    let song = format!(
        "import {{ song, track }} from \"@m2/core\";\nimport {{ drum }} from \"./lib/drum\";\n\
         const b = track(\"b\", ({{ lane }}) => ({{ out: drum(lane.notes(\"hits\")), hits: lane.notes(\"hits\") }}));\n\
         const a = track(\"a\", ({{ lane }}) => {call});\n\
         export default song(() => a.out);\n"
    );
    build_of(&[("lib/drum.ts", DRUM), ("song.ts", &song)])
}

/// What track `a`'s drum kernel reads at `cut`: the node's op.
fn cut_of(b: &Build) -> Option<Op> {
    let m = b.ir.modules.iter().find(|m| m.key == "a").expect("track a");
    let mut found = None;
    walk(&m.nodes, &mut |n| {
        if let Op::Kernel { kernel, inputs, .. } = &n.op
            && kernel == "test:drum"
        {
            found = inputs.get("cut").cloned();
        }
    });
    let r = found?;
    let mut op = None;
    walk(&m.nodes, &mut |n| {
        if n.id == split_ref(&r).0 {
            op = Some(n.op.clone());
        }
    });
    op
}

// PKG-20: an instrument whose second input may be left out is an instrument (+ Track lists it), and
// the descriptor carries the input's label (CARD-35) beside `optional`, each only where it is given.
#[test]
fn an_instrument_may_take_an_optional_input_after_its_notes() {
    let b = song("drum(lane.notes(\"hits\"))").unwrap_or_else(|e| panic!("{e:?}"));
    let d = b.components.iter().find(|d| d.id == "drum").expect("the drum");
    assert!(d.is_instrument(), "{:?}", d.inputs);
    let json = serde_json::to_value(&d.inputs).unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            { "name": "notes", "kind": "notes", "unit": null, "sidechain": false, "many": false },
            { "name": "cutBy", "kind": "notes", "unit": null, "sidechain": true, "many": false, "optional": true, "label": "Cut by" },
        ])
    );
}

// PKG-18, PKG-20: left out, the body gets `undefined`; given after the notes, the body reads it
// (here another track's notes, through its export), and the frame records it as a sidechain.
#[test]
fn an_instruments_optional_input_is_given_after_its_notes() {
    let left = song("drum(lane.notes(\"hits\"), { tune: 2 })").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(cut_of(&left), None, "left out, nothing is cut");
    let given = song("drum(lane.notes(\"hits\"), b.hits, { tune: 2 })").unwrap_or_else(|e| panic!("{e:?}"));
    assert!(matches!(cut_of(&given), Some(Op::Poly { .. })), "{:?}", cut_of(&given));
    let a = given.ir.modules.iter().find(|m| m.key == "a").unwrap();
    let frame = a.frames.iter().find(|f| f.component.as_deref() == Some("drum")).unwrap();
    assert_eq!(frame.sidechain.iter().map(|x| x.input.as_str()).collect::<Vec<_>>(), ["cutBy"], "it only listens");
}

/// `component()` with `inputs`, as an instrument declares it, and what it says.
fn declared(inputs: &str) -> Result<Build, Vec<Diagnostic>> {
    let lib = format!(
        "import {{ component, control, kernel, port, type Audio }} from \"@m2/core\";\n\
         export const c = component({{ id: \"c\", inputs: {inputs}, controls: {{ x: control.frac(0.5) }}, output: port.audio(), editor: {{ category: \"Drums\" }} }}, (...a: any[]) => kernel<Audio>(\"test:c\", {{}}));\n"
    );
    let song = "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(({ lane }) => (c as any)(lane.notes(\"n\")));\n";
    build_of(&[("lib/c.ts", &lib), ("song.ts", song)])
}

// PKG-20: a second input a call must give is still refused, in words that say what to do; a label
// is one line of 1 to 24 characters, as a value's is (CARD-34).
#[test]
fn an_instruments_other_inputs_are_optional_ones() {
    declared("{ notes: port.notes(), cut: port.notes().optional() }").unwrap_or_else(|e| panic!("{e:?}"));
    for (inputs, says) in [
        (
            "{ notes: port.notes(), cut: port.notes() }",
            "an instrument (editor.category, notes first) has one input a call must give, not \"cut\" too: declare it .optional()",
        ),
        ("{ notes: port.notes(), cut: port.notes({ label: \"\" }).optional() }", "the input \"cut\": its label"),
    ] {
        let e = declared(inputs).err().unwrap_or_else(|| panic!("{inputs} builds"));
        assert!(e.iter().any(|d| d.message.contains(says)), "{inputs}: {says}\n{e:?}");
    }
}
