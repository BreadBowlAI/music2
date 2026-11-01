//! Kits an instrument names (`@m2/core` 2.2.0, KIT-3): `editor.kits`, each kit by its name with the
//! tracks a new one makes, each track a name and values of the instrument's own controls, as a call
//! writes them. TypeScript refuses each mistake as the code is typed (`sdk/check/kits.ts`); the build
//! refuses it again, in words, since types can be bypassed: each refusal here is written past the
//! types, with `as any`.

use std::time::Duration;

use super::*;
use crate::component::{Insert, Kit, KitTrack, Spelled};

/// A drum with a choice, a number in a spelling of its own, a strict one and a switch, naming the
/// kits `kits` (as written in TypeScript).
fn drum(kits: &str) -> String {
    format!(
        r#"import {{ component, control, kernel, port, setting, type Audio, type Events }} from "@m2/core";
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

// KIT-3: the descriptor carries each kit in order, by its name, each track by its name and its
// values in the order written: a choice's place, a number in its spelling, a switch's 1 or 0.
#[test]
fn kits_are_carried_in_order() {
    let d = described(&drum(
        r#"{ "Basic kit": [{ name: "Kick", drum: "kick" }, { name: "Hats", drum: "hats", tune: 2, decay: 80, soft: true }], Two: [{ name: "Snare", drum: "snare", tune: -1.5 }] }"#,
    ))
    .unwrap_or_else(|e| panic!("{e:?}"));
    let track = |name: &str, values: Vec<Insert>| KitTrack { name: name.into(), values };
    assert_eq!(
        d.editor.kits,
        [
            Kit {
                name: "Basic kit".into(),
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
                tracks: vec![track("Snare", vec![value("drum", 1.0, None, 1.0), value("tune", -1.5, Some("semis"), -1.5)])]
            },
        ]
    );
}

// KIT-3: an instrument that names no kits keeps its bytes: the SDK writes no `kits` key at all.
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
}

// KIT-3: past the types, a kit that names a sound the Drum doesn't have, a value its control can't
// take, a control it doesn't have, or a name that isn't one short line is refused in words.
#[test]
fn a_wrong_kit_is_refused_in_words() {
    for (kits, says) in [
        (
            r#"{ "Basic kit": [{ name: "Kick", drum: "kik" }] } as any"#,
            "editor.kits' \"Basic kit\": its track \"Kick\": drum is one of \"kick\", \"snare\", \"hats\", not \"kik\"",
        ),
        (
            r#"{ "Basic kit": [{ name: "Kick", drum: 0 }] } as any"#,
            "its track \"Kick\": drum is one of \"kick\", \"snare\", \"hats\", not 0",
        ),
        (r#"{ "Basic kit": [{ name: "Kick", soft: 1 }] } as any"#, "its track \"Kick\": soft is true or false, not 1"),
        (r#"{ "Basic kit": [{ name: "Kick", tune: "2" }] } as any"#, "its track \"Kick\": tune is a number, not \"2\""),
        (
            r#"{ "Basic kit": [{ name: "Kick", decay: 5000 }] } as any"#,
            "its track \"Kick\": decay 5000 is outside its strict range 10..2000",
        ),
        (
            r#"{ "Basic kit": [{ name: "Kick", tunee: 2 }] } as any"#,
            "its track \"Kick\" sets \"tunee\", which isn't a control of this sound (controls: drum, tune, decay, soft)",
        ),
        (
            r#"{ "Basic kit": [{ name: "Kick", voices: 2 }] } as any"#,
            "its track \"Kick\" sets \"voices\", which isn't a control of this sound",
        ),
        (
            r#"{ "Basic kit": [{ drum: "kick" }] } as any"#,
            "editor.kits' \"Basic kit\": a track's name must be one line of 1 to 24 characters",
        ),
        (
            r#"{ "Basic kit": [{ name: " Kick" }] } as any"#,
            "a track's name must be one line of 1 to 24 characters, with no space at either end, not \" Kick\"",
        ),
        (
            r#"{ "Basic kit": [{ name: "Kick" }, { name: "Kick" }] } as any"#,
            "editor.kits' \"Basic kit\": the track \"Kick\" is named twice",
        ),
        (r#"{ "Basic kit": [] }"#, "editor.kits' \"Basic kit\" has no track: a kit has one at least"),
        (
            r#"{ "Basic kit": { name: "Kick" } } as any"#,
            "editor.kits' \"Basic kit\" must be a list of tracks, as [{ name: \"Kick\", drum: \"kick\" }]",
        ),
        (
            r#"{ "": [{ name: "Kick" }] }"#,
            "editor.kits: a kit's name must be one line of 1 to 24 characters, with no space at either end, not \"\"",
        ),
        (r#"{ "A kit's name far too long": [{ name: "Kick" }] }"#, "a kit's name must be one line of 1 to 24 characters"),
        ("[] as any", "editor.kits must be an object"),
    ] {
        refused(kits, says);
    }
    // Only an instrument names kits: a component a new track can't play has none to make.
    let effect = r#"import { component, control, kernel, port, type Audio } from "@m2/core";
export const drum = component(
  { id: "drum", inputs: { x: port.audio() }, controls: { mix: control.frac(1) }, output: port.audio(), editor: { category: "Delay", kits: { Two: [{ name: "A" }] } as any } },
  (x: Audio, p: any) => kernel<Audio>("test:drum", {}),
);
"#;
    let e = described(effect).err().expect("an effect with kits builds");
    let says = "editor.kits: only an instrument (editor.category, notes first) names kits";
    assert!(e.iter().any(|d| d.message.contains(says)), "{says}\n{e:?}");
}
