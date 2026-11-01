//! Drum kits (KIT-1): which kits a song has, worked out from its tracks and their outputs, on the
//! buses fixture (a Drums bus that the Kick and the Hats go to).

use m2_session::Session;

use crate::kits::{self, Kit, Row};
use crate::testing::{Fixture, fixture};
use crate::{Deleted, Gestures, bus};

const BUSES: &str = "core/session/tests/fixtures/buses";

fn row(track: &str, lane: &str, sound: Option<&str>) -> Row {
    Row { track: track.into(), lane: lane.into(), sound: sound.map(str::to_string) }
}

/// A track file playing the Drum at `sound` from lane `hits`, as the code writes one.
fn drum_file(key: &str, label: &str, sound: &str) -> String {
    format!(
        "import {{ track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\n\n\
         export default track(\"{key}\", {{ name: \"{label}\" }}, ({{ lane }}) => drums.drum(lane.notes(\"hits\"), {{ drum: \"{sound}\" }}));\n"
    )
}

/// The buses fixture with track `key` added, its file `text`, its channel `entry` last in the mixer.
fn with_track(f: &Fixture, key: &str, text: &str, entry: &str) {
    f.write(&format!("tracks/{key}.ts"), text);
    let song = f.read("song.ts");
    let song = song.replace(
        "import verb from \"./tracks/verb\";\n",
        &format!("import verb from \"./tracks/verb\";\nimport {key} from \"./tracks/{key}\";\n"),
    );
    let song = song.replace(
        "      echo: { in: echo.out, db: db(-6), sends: { verb: db(-12) } },\n",
        &format!("      echo: {{ in: echo.out, db: db(-6), sends: {{ verb: db(-12) }} }},\n      {entry},\n"),
    );
    f.write("song.ts", song);
}

fn rows(s: &Session) -> Vec<Row> {
    match &kits::kits(s)[..] {
        [kit] => kit.rows.clone(),
        other => panic!("one kit, not {other:?}"),
    }
}

// KIT-1: a bus and the Drum tracks whose channels go to it are one kit, named by the bus, its rows
// in the timeline's order (the layout's, not the mixer's), each with its lane and its sound: the
// Kick's left out (the default, kick), the Hats' and the Snare's as written.
#[test]
fn a_bus_with_three_drums_is_one_kit_in_track_order() {
    let f = fixture(BUSES);
    with_track(&f, "snare", &drum_file("snare", "Snare", "snare"), "snare: { in: snare.out, to: \"drums\" }");
    let s = f.open();
    let kick = row("kick", "hits", Some("kick"));
    let hats = row("hats", "hits", Some("hats"));
    let snare = row("snare", "hits", Some("snare"));
    assert_eq!(kits::kits(&s), [Kit { bus: "drums".into(), label: "Drums".into(), rows: vec![kick.clone(), hats.clone(), snare.clone()] }]);
    // The timeline's order: the Snare moved up above the Hats.
    let mut s = s;
    let order: Vec<String> = ["kick", "snare", "hats", "drums", "bass", "keys", "verb", "echo"].map(String::from).into();
    s.reorder(&order).unwrap();
    assert_eq!(rows(&s), [kick, snare, hats]);
    // The song's view carries it, as the web and `m2 graph --json` read it.
    let v = serde_json::to_value(s.view()).unwrap();
    assert_eq!(
        v["kits"],
        serde_json::json!([{"bus": "drums", "label": "Drums", "rows": [
            {"track": "kick", "lane": "hits", "sound": "kick"},
            {"track": "snare", "lane": "hits", "sound": "snare"},
            {"track": "hats", "lane": "hits", "sound": "hats"},
        ]}])
    );
}

// KIT-1: a Drum sent to the master is in no kit; sent to the bus it is, after the others, and sent
// back it leaves again. Nothing is stored: the routing says it all.
#[test]
fn a_drum_routed_to_the_master_is_in_no_kit() {
    let f = fixture(BUSES);
    with_track(&f, "rim", &drum_file("rim", "Rim", "rim"), "rim: rim.out");
    let mut s = f.open();
    assert!(!rows(&s).iter().any(|r| r.track == "rim"), "{:?}", rows(&s));
    bus::route(&mut s, "rim", Some("drums")).unwrap();
    assert_eq!(rows(&s).last(), Some(&row("rim", "hits", Some("rim"))));
    bus::route(&mut s, "kick", None).unwrap();
    assert_eq!(rows(&s).iter().map(|r| r.track.as_str()).collect::<Vec<_>>(), ["hats", "rim"]);
}

// KIT-1: a track on the bus that plays no Drum is no row (the Keys, a pluck), nor is one that plays
// two Drums, nor a Drum whose notes come from anything but a lane of its own.
#[test]
fn a_non_drum_track_on_the_bus_is_no_row() {
    let f = fixture(BUSES);
    let two = "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { sum } from \"std/math\";\n\n\
               export default track(\"two\", ({ lane }) => sum(drums.drum(lane.notes(\"hits\")), drums.drum(lane.notes(\"hits\"), { drum: \"clap\" })));\n";
    with_track(&f, "two", two, "two: { in: two.out, to: \"drums\" }");
    let mut s = f.open();
    assert!(s.accepted().compiled.flat.routes.contains(&("two".into(), "drums".into())), "the two Drums go to the bus");
    bus::route(&mut s, "keys", Some("drums")).unwrap();
    assert_eq!(rows(&s), [row("kick", "hits", Some("kick")), row("hats", "hits", Some("hats"))]);
}

// KIT-1: delete the bus (its outputs with it) and there is no kit; the Drum tracks stay, plain
// tracks again.
#[test]
fn deleting_the_bus_leaves_no_kit() {
    let f = fixture(BUSES);
    let mut s = f.open();
    assert_eq!(kits::kits(&s).len(), 1);
    match s.delete_track("drums", true).unwrap() {
        Deleted::Done { .. } => {}
        other => panic!("deleted, not {other:?}"),
    }
    assert!(kits::kits(&s).is_empty(), "{:?}", kits::kits(&s));
    let modules = &s.accepted().compiled.ir.modules;
    assert!(modules.iter().any(|m| m.key == "kick") && modules.iter().any(|m| m.key == "hats"));
}

// KIT-1: a Drum whose sound a lane plays has no sound the grid can know: null. Its row is still a
// row.
#[test]
fn a_sound_the_code_works_out_is_null() {
    let f = fixture(BUSES);
    f.write(
        "tracks/hats.ts",
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
         export default track(\"hats\", { name: \"Hats\" }, ({ lane }) => drums.drum(lane.notes(\"hits\"), { drum: lane.control(\"sound\", \"choice\", \"hats\") }));\n",
    );
    let s = f.open();
    assert_eq!(rows(&s), [row("kick", "hits", Some("kick")), row("hats", "hits", None)]);
}
