//! Kits a drum names (`@m2/core` 2.2.0, KIT-3): `editor.kits`, each kit by its name with the tracks
//! a new one makes and its bus's fader, each track a name and values of the instrument's own
//! controls, taken as a call takes them. TypeScript refuses each mistake as the code is typed (`sdk/check/kits.ts`); the build
//! refuses it again, in words, since types can be bypassed: each refusal here is written past the
//! types, with `as any`.

use std::time::Duration;

use super::*;
use crate::component::{Insert, Kit, KitTrack, Spelled};

/// A drum with a choice, a number in a spelling of its own (ms, scale 0.001), a strict one and a
/// switch, naming the kits `kits` (as written in TypeScript).
fn drum(kits: &str) -> String {
    format!(
        r#"import {{ component, control, db, hz, kernel, ms, port, semis, setting, type Audio, type Events }} from "@m2/core";
export const drum = component(
  {{
    id: "drum", label: "Drum", inputs: {{ notes: port.notes() }},
    controls: {{
      drum: control.choice(["kick", "snare", "hats"], "kick").label("sound"), tune: control.semis(0).range(-24, 24),
      decay: control.ms(200).range(10, 2000).strict(), soft: control.bool(false),
    }},
    settings: {{ voices: setting.int(6) }},
    output: port.audio(),
    editor: {{ category: "Drums", kits: {kits} }},
  }},
  (notes: Events, p: any) => kernel<Audio>("test:drum", {{}}),
);
"#
    )
}

fn build_of(files: &[(&str, &str)]) -> Result<Build, Vec<Diagnostic>> {
    let code: BTreeMap<String, String> = files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut o = Options::new(code, "song.ts");
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

const SONG: &str = "import { song } from \"@m2/core\";\nimport { drum } from \"./lib/drum\";\nexport default song(({ lane }) => drum(lane.notes(\"n\")));\n";

/// The descriptor of `lib`'s `drum`, built in a song that plays it.
fn described(lib: &str) -> Result<crate::component::Descriptor, Vec<Diagnostic>> {
    let b = build_of(&[("lib/drum.ts", lib), ("song.ts", SONG)])?;
    Ok(b.components.into_iter().find(|d| d.id == "drum").expect("the drum's descriptor"))
}

/// That `drum` naming `kits` doesn't build, with an error that says `says`.
fn refused(kits: &str, says: &str) {
    let e = described(&drum(kits)).err().unwrap_or_else(|| panic!("{kits} builds"));
    assert!(e.iter().any(|d| d.message.contains(says)), "{kits}: {says}\n{e:?}");
}

fn value(slot: &str, value: f64, spelled: Option<&str>, written: f64) -> Insert {
    Insert { slot: slot.into(), value: Spelled { value, spelled: spelled.map(str::to_string), written } }
}

// KIT-3: the descriptor carries each kit in order, by its name, its bus's fader when given, each
// track by its name and its values in the order written, each as a call takes it: a choice's place,
// a switch's 1 or 0, a quantity in base units with the spelling it was written in (`ms(80)`: 0.08,
// ms, 80), a plain number in the unit's base as it is (0.08 on an ms knob is 0.08 seconds).
#[test]
fn kits_are_carried_in_order() {
    let d = described(&drum(
        r#"{ "Basic kit": { db: db(-2), tracks: [{ name: "Kick", drum: "kick" }, { name: "Hats", drum: "hats", tune: semis(2), decay: ms(80), soft: true }] }, Two: { tracks: [{ name: "Snare", drum: "snare", tune: -1.5, decay: 0.08 }] }, Three: { db: -4.5, tracks: [{ name: "Kick" }] } }"#,
    ))
    .unwrap_or_else(|e| panic!("{e:?}"));
    let track = |name: &str, values: Vec<Insert>| KitTrack { name: name.into(), values };
    let level = |value: f64, spelled: Option<&str>| Some(Spelled { value, spelled: spelled.map(str::to_string), written: value });
    assert_eq!(
        d.editor.kits,
        [
            Kit {
                name: "Basic kit".into(),
                db: level(-2.0, Some("db")),
                tracks: vec![
                    track("Kick", vec![value("drum", 0.0, None, 0.0)]),
                    track(
                        "Hats",
                        vec![
                            value("drum", 2.0, None, 2.0),
                            value("tune", 2.0, Some("semis"), 2.0),
                            value("decay", 80.0 * 0.001, Some("ms"), 80.0),
                            value("soft", 1.0, None, 1.0),
                        ]
                    ),
                ],
            },
            Kit {
                name: "Two".into(),
                db: None,
                tracks: vec![track(
                    "Snare",
                    vec![value("drum", 1.0, None, 1.0), value("tune", -1.5, None, -1.5), value("decay", 0.08, None, 0.08)]
                )],
            },
            Kit { name: "Three".into(), db: level(-4.5, None), tracks: vec![track("Kick", vec![])] },
        ]
    );
}

// KIT-3: a drum that names no kits keeps its bytes: the SDK writes no `kits` key at all; a kit
// that gives no fader has no `db` key.
#[test]
fn a_descriptor_with_no_kits_keeps_its_bytes() {
    let lib = drum("undefined");
    let code: BTreeMap<String, String> =
        [("lib/drum.ts", lib.as_str()), ("song.ts", SONG)].iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut o = Options::new(code, "song.ts");
    o.time_limit = Duration::from_secs(2);
    let [_, _, components, _] = on_own_thread(|| run_sdk(&o).map(|run| run.json)).unwrap_or_else(|e| panic!("{e:?}"));
    let all: serde_json::Value = serde_json::from_str(&components).unwrap();
    let d = all.as_array().unwrap().iter().find(|d| d["id"] == "drum").expect("the drum's descriptor");
    assert!(!d["editor"].as_object().unwrap().contains_key("kits"), "no kits, no kits key: {}", d["editor"]);
    let d = described(&lib).unwrap();
    assert!(d.editor.kits.is_empty());
    assert!(!serde_json::to_string(&d.editor).unwrap().contains("kits"), "nor when written again");
    let d = described(&drum(r#"{ Two: { tracks: [{ name: "Kick" }] } }"#)).unwrap();
    assert!(!serde_json::to_string(&d.editor.kits[0]).unwrap().contains("db"), "no fader, no db key");
}

// KIT-3: past the types, a kit that names a sound the Drum doesn't have, a value its control can't
// take (a quantity of another unit, one past a strict range), a control it doesn't have, a name
// that isn't one short line, or a kit not written as its tracks and its fader is refused in words.
#[test]
fn a_wrong_kit_is_refused_in_words() {
    let kit = |tracks: &str| format!(r#"{{ "Basic kit": {{ tracks: {tracks} }} }} as any"#);
    for (kits, says) in [
        (
            kit(r#"[{ name: "Kick", drum: "kik" }]"#),
            "editor.kits' \"Basic kit\": its track \"Kick\": drum is one of \"kick\", \"snare\", \"hats\", not \"kik\"",
        ),
        (kit(r#"[{ name: "Kick", drum: 0 }]"#), "its track \"Kick\": drum is one of \"kick\", \"snare\", \"hats\", not 0"),
        (kit(r#"[{ name: "Kick", soft: 1 }]"#), "its track \"Kick\": soft is true or false, not 1"),
        (kit(r#"[{ name: "Kick", tune: "2" }]"#), "its track \"Kick\": tune is a number or a semitones value, not \"2\""),
        (kit(r#"[{ name: "Kick", tune: hz(2) }]"#), "its track \"Kick\": tune is a number or a semitones value, not a hz value"),
        (
            kit(r#"[{ name: "Kick", decay: ms(5000) }]"#),
            "its track \"Kick\": decay ms(5000) is outside its strict range, 0.01 to 2 seconds",
        ),
        (kit(r#"[{ name: "Kick", decay: 5 }]"#), "its track \"Kick\": decay 5 is outside its strict range, 0.01 to 2 seconds"),
        (
            kit(r#"[{ name: "Kick", tunee: 2 }]"#),
            "its track \"Kick\" sets \"tunee\", which isn't a control of this sound (controls: drum, tune, decay, soft)",
        ),
        (kit(r#"[{ name: "Kick", voices: 2 }]"#), "its track \"Kick\" sets \"voices\", which isn't a control of this sound"),
        (kit(r#"[{ drum: "kick" }]"#), "editor.kits' \"Basic kit\": a track's name must be one line of 1 to 24 characters"),
        (
            kit(r#"[{ name: " Kick" }]"#),
            "a track's name must be one line of 1 to 24 characters, with no space at either end, not \" Kick\"",
        ),
        (kit(r#"[{ name: "Kick" }, { name: "Kick" }]"#), "editor.kits' \"Basic kit\": the track \"Kick\" is named twice"),
        (kit("[]"), "editor.kits' \"Basic kit\" has no track: a kit has one at least"),
        (kit(r#"{ name: "Kick" }"#), "editor.kits' \"Basic kit\": its tracks must be a list, as [{ name: \"Kick\", drum: \"kick\" }]"),
        (
            r#"{ "Basic kit": [{ name: "Kick" }] } as any"#.to_string(),
            "editor.kits' \"Basic kit\" must be its tracks and its bus's fader, as { db: -2, tracks: [{ name: \"Kick\", drum: \"kick\" }] }",
        ),
        (
            r#"{ "Basic kit": { tracks: [{ name: "Kick" }], gain: 1 } } as any"#.to_string(),
            "editor.kits' \"Basic kit\" has no field \"gain\" (fields: db, tracks)",
        ),
        (
            r#"{ "Basic kit": { db: "x", tracks: [{ name: "Kick" }] } } as any"#.to_string(),
            "editor.kits' \"Basic kit\": db is a number or a db value, not \"x\"",
        ),
        (
            r#"{ "Basic kit": { db: ms(2), tracks: [{ name: "Kick" }] } } as any"#.to_string(),
            "editor.kits' \"Basic kit\": db is a number or a db value, not a seconds value",
        ),
        (
            r#"{ "": { tracks: [{ name: "Kick" }] } }"#.to_string(),
            "editor.kits: a kit's name must be one line of 1 to 24 characters, with no space at either end, not \"\"",
        ),
        (
            r#"{ "A kit's name far too long": { tracks: [{ name: "Kick" }] } }"#.to_string(),
            "a kit's name must be one line of 1 to 24 characters",
        ),
        ("[] as any".to_string(), "editor.kits must be an object"),
    ] {
        refused(&kits, says);
    }
    // Only a drum names kits: a component a new track can't play has none to make, and an
    // instrument of another category (Keys) none either, yet (a wider kit is an addition).
    let says = "editor.kits: only a drum (editor.category \"Drums\") names kits";
    let effect = r#"import { component, control, kernel, port, type Audio } from "@m2/core";
export const drum = component(
  { id: "drum", inputs: { x: port.audio() }, controls: { mix: control.frac(1) }, output: port.audio(), editor: { category: "Delay", kits: { Two: { tracks: [{ name: "A" }] } } as any } },
  (x: Audio, p: any) => kernel<Audio>("test:drum", {}),
);
"#;
    let e = described(effect).expect_err("an effect with kits builds");
    assert!(e.iter().any(|d| d.message.contains(says)), "{says}\n{e:?}");
    let keys = drum(r#"{ Two: { tracks: [{ name: "A" }] } } as any"#).replace("category: \"Drums\"", "category: \"Keys\"");
    let e = described(&keys).expect_err("a Keys instrument with kits builds");
    assert!(e.iter().any(|d| d.message.contains(says)), "{says}\n{e:?}");
}
