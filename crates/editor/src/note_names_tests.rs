//! Note names on the fixtures: a drum kit's lane names its notes, a pluck's names none.

use crate::Gestures;
use crate::TimelineLane;
use crate::testing::fixture;

/// The names the timeline gives lane `lane` of `module`.
fn names(s: &m2_session::Session, module: &str, lane: &str) -> Vec<(u8, String)> {
    let t = s.timeline();
    let track = t.tracks.iter().find(|t| t.module == module).expect("the track");
    match track.lanes.iter().find(|l| l.key() == lane) {
        Some(TimelineLane::Notes { names, .. }) => names.iter().map(|(k, v)| (*k, v.clone())).collect(),
        other => panic!("{module}.{lane} isn't a notes lane: {other:?}"),
    }
}

// A track playing the drum kit (added as + Track adds one) shows its rows by name: the GM notes
// the kit plays, from its descriptor. The timeline's JSON says them by note.
#[test]
fn a_drum_kit_lane_names_its_notes() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let made = s.add_track("@m2/drums:drum-kit", None, None, None).unwrap();
    let got = names(&s, &made.module, "hits");
    assert_eq!(got.len(), 20, "{got:?}");
    assert!(
        got.contains(&(36, "Kick".into())) && got.contains(&(38, "Snare".into())) && got.contains(&(42, "Closed hat".into())),
        "{got:?}"
    );
    let json = serde_json::to_value(s.timeline()).unwrap();
    let lane = json["tracks"].as_array().unwrap().iter().find(|t| t["module"] == made.module.as_str()).unwrap()["lanes"][0].clone();
    assert_eq!(lane["names"]["36"], "Kick");
    // Played through another call (a project's own wrapper), the kit still names them.
    f.write(
        &made.edited.file,
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { scale } from \"std/math\";\n\n\
         export default track(\"drum-kit\", { name: \"Drum kit\" }, ({ lane }) => scale(drums.kit(lane.notes(\"hits\")), 0.5));\n",
    );
    s.refresh();
    assert_eq!(names(&s, "drum-kit", "hits").len(), 20);
}

// A pluck names no notes, nor does a lane two different instruments read, nor the hats' lane of
// @m2/synths' simple hat.
#[test]
fn a_pluck_lane_has_no_names() {
    let f = fixture("examples/first-song");
    let s = f.open();
    assert!(names(&s, "keys", "chords").is_empty());
    assert!(names(&s, "hats", "hits").is_empty());
    let json = serde_json::to_value(s.timeline()).unwrap();
    let keys = json["tracks"].as_array().unwrap().iter().find(|t| t["module"] == "keys").unwrap()["lanes"][0].clone();
    assert!(keys.get("names").is_none(), "no names in the JSON: {keys}");
    let f = fixture("examples/first-song");
    f.write(
        "tracks/hats.ts",
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { pluck } from \"@m2/synths\";\nimport { sum } from \"std/math\";\n\n\
         export default track(\"hats\", { name: \"Hats\" }, ({ lane }) => sum(drums.kit(lane.notes(\"hits\")), pluck(lane.notes(\"hits\"))));\n",
    );
    let s = f.open();
    assert!(names(&s, "hats", "hits").is_empty(), "read by a kit and a pluck: no names");
}
