//! Note effects (PKG-21, `@m2/core` 2.5.0): a component that takes notes first and gives notes is
//! one, by its ports alone. The build checks it as it checks an effect, with notes for audio; a
//! call of one may be bypassed (PKG-3); and it reads the song's tempo, `time.tempo` (RENDER-7).
//! The IR it makes is the events note's (§6 row 3): a kernel at the module's top level whose notes
//! come from a lane or from another note effect. The types take the same calls
//! (`sdk/check/calls.ts`); the build refuses each mistake again, in words.

use std::time::Duration;

use m2_ir::{Node, Op, TimeOf, Unit, split_ref};

use super::*;

/// An arpeggiator and a chord, each a note effect over a kernel of its own that gives notes, and a
/// plain voice to play them.
const LIB: &str = r#"import { component, control, kernel, poly, port, signature, time, type Audio, type Events, type EventsKind, type Params } from "@m2/core";
export const arp = component(
  {
    id: "arp", label: "Arpeggiator",
    inputs: { notes: port.notes() },
    controls: { rate: control.choice(["1/8", "1/16"], "1/16"), gate: control.scalar(0.8).range(0.1, 2) },
    output: port.notes(),
    editor: { category: "Arpeggiator" },
  },
  (notes: Events, { rate, gate }: Params) => kernel<EventsKind>("test:arp", { notes, beats: time.beats, tempo: time.tempo, rate, gate }),
);
export const chord = component(
  {
    id: "chord", label: "Chord",
    signature: signature<(notes: Events) => Events>(),
    inputs: { notes: port.notes() },
    output: port.notes(),
    editor: { category: "Chords" },
  },
  (notes: Events) => kernel<EventsKind>("test:chord", { notes, key: time.key }),
);
export const tone = component(
  { id: "tone", label: "Tone", inputs: { notes: port.notes() }, output: port.audio(), editor: { category: "Keys" } },
  (notes: Events): Audio => poly(notes, (n) => kernel("test:tone", { pitch: n.pitch, gate: n.gate })),
);
"#;

fn build_of(files: &[(&str, &str)]) -> Result<Build, Vec<Diagnostic>> {
    let code: BTreeMap<String, String> = files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut o = Options::new(code, "song.ts");
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

/// A song that returns `body`, given `{ lane }`, with the lib's sounds imported.
fn song(body: &str) -> Result<Build, Vec<Diagnostic>> {
    let song = format!(
        "import {{ song }} from \"@m2/core\";\nimport {{ arp, chord, tone }} from \"./lib/notes\";\nexport default song(({{ lane }}) => {body});\n"
    );
    build_of(&[("lib/notes.ts", LIB), ("song.ts", &song)])
}

/// The node `r` (a reference, `n3` or `n3.out`) names in `m`'s top level.
fn node<'a>(m: &'a m2_ir::Module, r: &str) -> &'a Node {
    m.nodes.iter().find(|n| n.id == split_ref(r).0).unwrap_or_else(|| panic!("no node {r} at the top level"))
}

/// The top-level kernel node of `kernel` in `m`, and its inputs.
fn kernel_of<'a>(m: &'a m2_ir::Module, kernel: &str) -> (&'a Node, &'a BTreeMap<String, String>) {
    for n in &m.nodes {
        if let Op::Kernel { kernel: k, inputs, .. } = &n.op
            && k == kernel
        {
            return (n, inputs);
        }
    }
    panic!("no {kernel} at the module's top level")
}

/// The node the top-level `poly` reads its notes from.
fn poly_reads(m: &m2_ir::Module) -> &Node {
    let events = m.nodes.iter().find_map(|n| match &n.op {
        Op::Poly { events, .. } => Some(events.clone()),
        _ => None,
    });
    node(m, &events.expect("a poly"))
}

// PKG-21: notes first and notes out, with a category, is a note effect: `is_note_effect`, and no
// instrument and no effect. Its descriptor is a component's like any other: no field says it.
#[test]
fn a_note_effect_is_notes_first_and_notes_out() {
    let b = song("tone(arp(lane.notes(\"chords\")))").unwrap_or_else(|e| panic!("{e:?}"));
    let d = |id: &str| b.components.iter().find(|d| d.id == id).unwrap_or_else(|| panic!("{id}"));
    for id in ["arp", "chord"] {
        assert!(d(id).is_note_effect(), "{id}");
        assert!(!d(id).is_instrument() && !d(id).is_effect(), "{id}");
    }
    assert!(!d("tone").is_note_effect() && d("tone").is_instrument());
    let json = serde_json::to_value(d("arp")).unwrap();
    let keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    let tone = serde_json::to_value(d("tone")).unwrap();
    assert_eq!(keys, tone.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), "the same fields as an instrument's");
    assert_eq!((json["inputs"][0]["kind"].as_str(), json["output"]["kind"].as_str()), (Some("notes"), Some("notes")));
    assert_eq!(json["editor"]["category"], "Arpeggiator");
}

// PKG-21 (the events note's §6 row 3): a note effect's kernel is at the module's top level, its
// notes from the lane, and what plays it reads its kernel; a second reads the first's.
#[test]
fn a_note_effects_kernel_reads_its_notes_and_is_read() {
    let b = song("tone(arp(lane.notes(\"chords\")))").unwrap_or_else(|e| panic!("{e:?}"));
    let m = &b.ir.modules[0];
    let (arp, inputs) = kernel_of(m, "test:arp");
    assert!(matches!(&node(m, &inputs["notes"]).op, Op::Lane { lane } if lane == "chords"), "{inputs:?}");
    assert!(matches!(&node(m, &inputs["tempo"]).op, Op::Time { of: TimeOf::Tempo }), "{inputs:?}");
    assert_eq!(poly_reads(m).id, arp.id, "the voice plays the arpeggiator's notes");

    let b = song("tone(arp(chord(lane.notes(\"chords\")), { rate: \"1/8\" }))").unwrap_or_else(|e| panic!("{e:?}"));
    let m = &b.ir.modules[0];
    let (chord, chord_in) = kernel_of(m, "test:chord");
    let (arp, arp_in) = kernel_of(m, "test:arp");
    assert!(matches!(&node(m, &chord_in["notes"]).op, Op::Lane { lane } if lane == "chords"));
    assert_eq!(node(m, &arp_in["notes"]).id, chord.id, "the arpeggiator plays the chord's notes");
    assert_eq!(poly_reads(m).id, arp.id);
    let frames: Vec<&str> = m.frames.iter().filter_map(|f| f.component.as_deref()).collect();
    assert_eq!(frames, ["chord", "arp", "tone"], "each call has its frame, the innermost first");
}

// PKG-3, PKG-21: a notes-to-notes call may be turned off: its frame records it, its body still
// runs (its knobs stay to set), and what plays it reads its notes input. An instrument still
// can't be, in words that name what can.
#[test]
fn a_notes_to_notes_call_may_be_bypassed() {
    let b = song("tone(arp(lane.notes(\"chords\"), { gate: 0.5, bypass: true }))").unwrap_or_else(|e| panic!("{e:?}"));
    let m = &b.ir.modules[0];
    let frame = m.frames.iter().find(|f| f.component.as_deref() == Some("arp")).expect("arp's frame");
    assert!(frame.bypass);
    assert!(matches!(&poly_reads(m).op, Op::Lane { lane } if lane == "chords"), "the voice plays the lane's notes");
    let on = song("tone(arp(lane.notes(\"chords\"), { gate: 0.5 }))").unwrap_or_else(|e| panic!("{e:?}"));
    assert!(!on.ir.modules[0].frames.iter().any(|f| f.bypass));
    let e = song("tone(lane.notes(\"chords\"), { bypass: true } as any)").err().expect("refused");
    assert!(
        e.iter().any(|d| d.message.contains(
            "tone(): can't be bypassed: only a call that takes audio first and gives audio, or notes first and gives notes, can be"
        )),
        "{e:?}"
    );
    let e = song("tone(arp(lane.notes(\"chords\"), { bypass: 1 } as any))").err().expect("refused");
    assert!(e.iter().any(|d| d.message.contains("arp(): bypass must be true or false, not 1")), "{e:?}");
}

/// `component()` declared as `decl`, with a category, called on a lane's notes and `options` (as
/// adding it writes its insert), and what the build says.
fn declared(decl: &str, options: &str) -> Result<Build, Vec<Diagnostic>> {
    let lib = format!(
        "import {{ component, control, kernel, port, type EventsKind }} from \"@m2/core\";\n\
         export const c = component({decl}, (...a: any[]) => kernel<EventsKind>(\"test:c\", {{}}));\n"
    );
    let song = "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(({ lane }) => (c as any)(lane.notes(\"n\")OPTIONS));\n".replace("OPTIONS", options);
    build_of(&[("lib/c.ts", &lib), ("song.ts", &song)])
}

// PKG-21: a note effect is checked as an effect is, with notes for audio: its notes first, the
// one input a call must give; it builds from its insert alone. Each refused in words, past the
// types. An input it may leave out, or many, is still one.
#[test]
fn a_note_effect_is_checked_as_an_effect_is() {
    let out = "output: port.notes(), editor: { category: \"Arpeggiator\"";
    for (ok, options) in [
        (format!("{{ id: \"c\", inputs: {{ notes: port.notes(), side: port.notes({{ sidechain: true }}).optional() }}, {out} }} }}"), ""),
        (
            format!(
                "{{ id: \"c\", inputs: {{ notes: port.notes() }}, controls: {{ rate: control.scalar() }}, {out}, insert: {{ rate: 2 }} }} }}"
            ),
            ", { rate: 2 }",
        ),
    ] {
        let b = declared(&ok, options).unwrap_or_else(|e| panic!("{ok}: {e:?}"));
        assert!(b.components[0].is_note_effect(), "{ok}");
    }
    for (decl, says) in [
        (
            format!("{{ id: \"c\", inputs: {{ notes: port.notes(), side: port.notes() }}, {out} }} }}"),
            "a note effect (editor.category, notes first and notes out) has one input a call must give, not \"side\" too: declare it .optional()",
        ),
        (
            format!("{{ id: \"c\", inputs: {{ notes: port.notes() }}, controls: {{ rate: control.scalar() }}, {out} }} }}"),
            "a note effect (editor.category, notes first and notes out) must build from its insert alone, which leaves out \"rate\"",
        ),
        // Review N2: its notes are one input a call must give, so a wrap and a bypass have one to pass on.
        (
            format!("{{ id: \"c\", inputs: {{ notes: port.notes().optional() }}, {out} }} }}"),
            "a note effect's notes are one input a call must give: not .optional() or .many()",
        ),
        (
            format!("{{ id: \"c\", inputs: {{ notes: port.notes().many() }}, {out} }} }}"),
            "a note effect's notes are one input a call must give: not .optional() or .many()",
        ),
        (
            format!("{{ id: \"c\", inputs: {{ notes: port.notes(), x: port.control() }}, positional: [\"x\", \"notes\"], {out} }} }}"),
            "a note effect (editor.category, notes first and notes out) takes notes as its first argument",
        ),
    ] {
        let e = declared(&decl, "").err().unwrap_or_else(|| panic!("{decl} builds"));
        assert!(e.iter().any(|d| d.message.contains(says)), "{decl}: {says}\n{e:?}");
    }
}

// RENDER-7: `time.tempo` is a `time` node of `tempo`, in the unit `bpm`: one per module outside any
// call, the same as `ctx.time.tempo`, as `time.beats` is. `bpm` is a unit of number of its own,
// spelled `bpm(…)` in the one table (PKG-11), which fits only itself.
#[test]
fn time_tempo_is_a_bpm_signal() {
    let song = r#"import { bpm, kernel, song, time } from "@m2/core";
export default song((ctx) => kernel("test:sync", { tempo: time.tempo, again: ctx.time.tempo, fixed: bpm(120) }));
"#;
    let b = build_of(&[("song.ts", song)]).unwrap_or_else(|e| panic!("{e:?}"));
    let m = &b.ir.modules[0];
    let tempos: Vec<&Node> = m.nodes.iter().filter(|n| matches!(&n.op, Op::Time { of: TimeOf::Tempo })).collect();
    assert_eq!(tempos.len(), 1, "one node");
    let (_, inputs) = kernel_of(m, "test:sync");
    for port in ["tempo", "again"] {
        assert_eq!(split_ref(&inputs[port]).0, tempos[0].id, "{port}");
    }
    assert_eq!(TimeOf::Tempo.unit(), Unit::Bpm);
    assert_eq!(serde_json::to_value(TimeOf::Tempo).unwrap(), serde_json::json!("tempo"));
    assert_eq!("bpm".parse::<Unit>(), Ok(Unit::Bpm));
    assert!(Unit::Bpm.is_number() && Unit::ALL.contains(&Unit::Bpm));
    assert!(Unit::ALL.iter().all(|&u| u == Unit::Bpm || (!Unit::Bpm.fits(u) && !u.fits(Unit::Bpm))), "bpm fits only itself");
    let ctor: Vec<(&str, Unit, f64, bool)> =
        b.units.iter().filter(|u| u.unit == Unit::Bpm).map(|u| (u.name.as_str(), u.unit, u.scale, u.canonical)).collect();
    assert_eq!(ctor, [("bpm", Unit::Bpm, 1.0, true)]);
    let fixed = node(m, &inputs["fixed"]);
    assert!(matches!(&fixed.op, Op::Const { value, unit, .. } if *value == 120.0 && *unit == Some(Unit::Bpm)), "{:?}", fixed.op);
}
