//! Optional inputs (PKG-18): `port.notes().optional()`, given by its name in the options object and
//! never by its place, `undefined` in the body when a call leaves it out. TypeScript marks each
//! mistake as the code is typed (`sdk/check/calls.ts`); the build refuses it again, in words, since
//! types can be bypassed: each refusal here is written past the types, with `as any`.

use std::time::Duration;

use m2_ir::{Op, split_ref};

use super::*;

/// A drum whose `cutBy` is an optional notes input: its kernel reads `cut` only when it is given.
const DRUM: &str = r#"import { component, control, kernel, poly, port, type Audio, type Events } from "@m2/core";
export const drum = component(
  {
    id: "drum", label: "Drum",
    inputs: { notes: port.notes(), cutBy: port.notes().optional() },
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

/// A song whose track `a` plays `call` (the drum, imported), and a track `b` that exports its notes.
fn song(call: &str) -> Result<Build, Vec<Diagnostic>> {
    let song = format!(
        "import {{ song, track }} from \"@m2/core\";\nimport {{ drum }} from \"./lib/drum\";\n\
         const b = track(\"b\", ({{ lane }}) => ({{ out: drum(lane.notes(\"hits\")), hits: lane.notes(\"hits\") }}));\n\
         const a = track(\"a\", ({{ lane }}) => {call});\n\
         export default song(() => a.out);\n"
    );
    build_of(&[("lib/drum.ts", DRUM), ("song.ts", &song)])
}

fn refused(call: &str, says: &str) {
    let e = song(call).err().unwrap_or_else(|| panic!("{call} builds"));
    assert!(e.iter().any(|d| d.message.contains(says)), "{call}: {says}\n{e:?}");
}

/// What track `a`'s drum kernel reads at `cut`: the node's op, followed through an import.
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

// PKG-18: the descriptor says `optional: true` on the optional input alone, so every other input,
// and every other descriptor, keeps its bytes.
#[test]
fn an_optional_input_is_carried_in_the_descriptor() {
    let b = song("drum(lane.notes(\"hits\"))").unwrap_or_else(|e| panic!("{e:?}"));
    let d = b.components.iter().find(|d| d.id == "drum").expect("the drum");
    let json = serde_json::to_value(&d.inputs).unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            { "name": "notes", "kind": "notes", "unit": null, "sidechain": false, "many": false },
            { "name": "cutBy", "kind": "notes", "unit": null, "sidechain": false, "many": false, "optional": true },
        ])
    );
    assert!(d.inputs[1].optional && !d.inputs[0].optional);
}

// PKG-18: left out, the body gets `undefined` (the drum reads no cut); given by its name in the
// options, the body gets that signal (here another track's notes, through its export).
#[test]
fn an_optional_input_left_out_is_undefined_and_given_by_name_is_read() {
    let left = song("drum(lane.notes(\"hits\"), { tune: 2 })").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(cut_of(&left), None, "left out, nothing is cut");
    let given = song("drum(lane.notes(\"hits\"), { tune: 2, cutBy: b.hits })").unwrap_or_else(|e| panic!("{e:?}"));
    assert!(matches!(cut_of(&given), Some(Op::Poly { .. })), "{:?}", cut_of(&given));
    // `undefined` where it goes is left out, as for a control (a helper may forward one it wasn't given).
    let none = song("drum(lane.notes(\"hits\"), { cutBy: undefined })").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(cut_of(&none), None);
}

// PKG-18: never by its place, never a value that isn't a signal, never a track itself.
#[test]
fn an_optional_input_is_given_by_name_only() {
    refused("(drum as any)(lane.notes(\"hits\"), b.hits)", "drum(): takes at most 1 argument");
    refused("drum(lane.notes(\"hits\"), { cutBy: 0.5 as any })", "drum(): the input cutBy must be a signal, not 0.5");
    refused("drum(lane.notes(\"hits\"), { cutBy: b as any })", "drum(): cutBy: `b` is a track; use `b.out`");
    refused("drum(lane.notes(\"hits\"), { cutby: b.hits } as any)", "no option \"cutby\" (options: tune, cutBy)");
    // A required input is still an argument, and says so.
    refused("drum(lane.notes(\"hits\"), { notes: b.hits } as any)", "\"notes\" is an input, given as an argument");
}

/// `component()` with `inputs` and `positional`, as a song declares it, and what it says.
fn declared(inputs: &str, extra: &str) -> Result<Build, Vec<Diagnostic>> {
    let lib = format!(
        "import {{ component, control, kernel, port, type Audio }} from \"@m2/core\";\n\
         export const c = component({{ id: \"c\", inputs: {inputs}, controls: {{ x: control.frac(0.5) }}, output: port.audio(){extra} }}, (...a: any[]) => kernel<Audio>(\"test:c\", {{}}));\n"
    );
    let song = "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(({ lane }) => (c as any)(lane.notes(\"n\")));\n";
    build_of(&[("lib/c.ts", &lib), ("song.ts", song)])
}

fn declared_refused(inputs: &str, extra: &str, says: &str) {
    let e = declared(inputs, extra).err().unwrap_or_else(|| panic!("{inputs}{extra} builds"));
    assert!(e.iter().any(|d| d.message.contains(says)), "{inputs}{extra}: {says}\n{e:?}");
}

// PKG-18: an optional input takes no place in a call: never many, never positional; an instrument
// and an effect may have one after their first input, and still no second required one.
#[test]
fn an_optional_input_is_declared_by_these_rules() {
    declared_refused(
        "{ notes: port.notes(), more: (port.notes().optional() as any).many() }",
        "",
        "the input \"more\" is optional, so it can't take many signals",
    );
    declared_refused(
        "{ notes: port.notes(), more: port.notes().many().optional() }",
        "",
        "the input \"more\" is optional, so it can't take many signals",
    );
    declared_refused(
        "{ notes: port.notes(), cut: port.notes().optional() }",
        ", positional: [\"notes\", \"cut\", \"x\"]",
        "positional names \"cut\", an optional input: it is given by its name in the options",
    );
    let instrument = ", editor: { category: \"Drums\" }";
    declared("{ notes: port.notes(), cut: port.notes().optional() }", instrument).unwrap_or_else(|e| panic!("{e:?}"));
    declared_refused(
        "{ notes: port.notes(), cut: port.notes() }",
        instrument,
        "an instrument (editor.category, notes first) has one input, not \"cut\" too",
    );
    let effect = ", editor: { category: \"Mixing\" }";
    let lib = "import { component, kernel, port, type Audio } from \"@m2/core\";\n\
               export const c = component({ id: \"c\", inputs: { in: port.audio(), key: port.audio({ sidechain: true }).optional() }, output: port.audio(), editor: { category: \"Mixing\" } }, (x: Audio) => x);\n";
    let song =
        "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(({ lane }) => c(lane.audio(\"a\")));\n";
    build_of(&[("lib/c.ts", lib), ("song.ts", song)]).unwrap_or_else(|e| panic!("{effect}: {e:?}"));
}
