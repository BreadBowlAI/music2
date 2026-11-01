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
// two Drums.
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

// KIT-1: a Drum whose notes come from another track's lane (the Hats playing the Kick's hits, which
// the Kick exports) has no lane of its own to write: its track is no row.
#[test]
fn a_drum_fed_from_another_tracks_lane_is_no_row() {
    let f = fixture(BUSES);
    f.write(
        "tracks/kick.ts",
        "import { ms, track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { follow } from \"std/dsp\";\n\n\
         export default track(\"kick\", { name: \"Kick\" }, ({ lane }) => {\n  const hits = lane.notes(\"hits\");\n  const out = drums.drum(hits);\n  \
         return { out, env: follow(out, { release: ms(150) }), hits };\n});\n",
    );
    f.write(
        "tracks/hats.ts",
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport kick from \"./kick\";\n\n\
         export default track(\"hats\", { name: \"Hats\" }, () => drums.drum(kick.hits, { drum: \"hats\" }));\n",
    );
    let s = f.open();
    assert!(s.accepted().compiled.flat.routes.contains(&("hats".into(), "drums".into())), "the Hats still go to the bus");
    assert_eq!(rows(&s), [row("kick", "hits", Some("kick"))]);
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

// ---------------------------------------------------------------- steps (KIT-12, KIT-14)

use crate::clips::{self, Gesture, Hit};
use m2_song::{AbsNote, LaneAddr};

fn hits(track: &str, at: &[f64], step: f64, to: Hit) -> Gesture {
    Gesture::Hits { lane: LaneAddr::new(track, "hits").unwrap(), at: at.to_vec(), step, to }
}

fn go(s: &mut Session, g: Gesture) -> crate::clips::ClipsEdited {
    clips::edit(s, &g).unwrap_or_else(|e| panic!("{g:?}: {e}"))
}

/// Track `track`'s hits as they play: each `(start, length, pitch, velocity)`.
fn played(s: &Session, track: &str) -> Vec<(f64, f64, f64, f64)> {
    let n: Vec<AbsNote> = s.accepted().compiled.data.notes(track, "hits");
    n.iter().map(|n| (n.start, n.end - n.start, n.pitch, n.velocity)).collect()
}

/// A clip as written: its start, its length and each note's `(start, length, pitch)` in it.
type Written = (f64, f64, Vec<(f64, f64, f64)>);

/// Track `track`'s clips as they are written.
fn written(s: &Session, track: &str) -> Vec<Written> {
    match s.accepted().compiled.data.lane(track, "hits").map(|d| (**d).clone()) {
        Some(m2_song::LaneData::Notes(c)) => {
            c.iter().map(|c| (c.start, c.length, c.notes.iter().map(|n| (n.start, n.length, n.pitch)).collect())).collect()
        }
        _ => vec![],
    }
}

// KIT-12: a step turned on is a hit at its start, a step long, on the row's own lane, in the clip
// that holds it, at the pitch the row's notes play (the Kick's C1, KIT-14) and velocity 0.8: one undo
// step. Turned off, it goes: one undo step. A step that has a hit stays as it is when turned on.
#[test]
fn a_step_is_one_undo_step_on_its_lane_at_its_pitch_and_time() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let (kick, hats) = (f.read("data/kick/hits.json"), f.read("data/hats/hits.json"));
    let done = go(&mut s, hits("kick", &[2.5], 0.25, Hit::On));
    assert!(played(&s, "kick").contains(&(2.5, 0.25, 24.0, 0.8)), "{:?}", played(&s, "kick"));
    assert_eq!(played(&s, "kick").len(), 33);
    assert_eq!(written(&s, "kick").len(), 1, "in the clip that holds it");
    assert_eq!(f.read("data/hats/hits.json"), hats, "no other lane");
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("no note", "C1 at bar 1 beat 3.5"));
    let with = f.read("data/kick/hits.json");
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read("data/kick/hits.json"), kick, "one undo takes it back");
    crate::redo(&mut s).unwrap();
    assert_eq!(f.read("data/kick/hits.json"), with);
    go(&mut s, hits("kick", &[2.5], 0.25, Hit::Off));
    assert_eq!(f.read("data/kick/hits.json"), kick, "turned off, it goes");
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read("data/kick/hits.json"), with, "one undo puts it back");
    let done = go(&mut s, hits("kick", &[2.0], 0.25, Hit::On));
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("no change", "no change"), "a step with a hit stays");
}

// KIT-14: on a row with no notes, a hit plays its sound's own note, in a new clip the length of its
// bar: the snare's D2, the hats' closed hat, the tom's A2, and A2 for a sound the code works out.
#[test]
fn a_rows_first_hit_plays_its_sounds_own_note() {
    let f = fixture(BUSES);
    with_track(&f, "snare", &drum_file("snare", "Snare", "snare"), "snare: { in: snare.out, to: \"drums\" }");
    with_track(&f, "tom", &drum_file("tom", "Tom", "tom"), "tom: { in: tom.out, to: \"drums\" }");
    f.write("data/hats/hits.json", "{\"clips\": []}\n");
    let mut s = f.open();
    go(&mut s, hits("snare", &[4.5], 0.25, Hit::On));
    assert_eq!(written(&s, "snare"), [(4.0, 4.0, vec![(0.5, 0.25, 38.0)])]);
    go(&mut s, hits("hats", &[0.0], 0.25, Hit::On));
    go(&mut s, hits("tom", &[0.0], 0.25, Hit::On));
    assert_eq!((played(&s, "hats")[0].2, played(&s, "tom")[0].2), (42.0, 45.0));
    f.write(
        "tracks/snare.ts",
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
         export default track(\"snare\", { name: \"Snare\" }, ({ lane }) => drums.drum(lane.notes(\"hits\"), { drum: lane.control(\"sound\", \"choice\", \"snare\") }));\n",
    );
    f.write("data/snare/hits.json", "{\"clips\": []}\n");
    assert!(s.refresh());
    go(&mut s, hits("snare", &[0.0], 0.25, Hit::On));
    assert_eq!(played(&s, "snare")[0].2, 45.0, "A2 plays every sound as itself");
}

// KIT-14: a row writes the pitch most of its notes play, of a tie the lowest: a Hats row of two
// open hats and a closed one writes an open one; level, the closed one.
#[test]
fn a_row_keeps_the_pitch_most_of_its_notes_play() {
    let f = fixture(BUSES);
    f.write(
        "data/hats/hits.json",
        "{\"clips\": [\n  {\"id\": \"hhhhhhhh\", \"start\": 0, \"length\": 4, \"notes\": [\n    \
         {\"pitch\": 46, \"start\": 0, \"length\": 0.25},\n    {\"pitch\": 46, \"start\": 1, \"length\": 0.25},\n    \
         {\"pitch\": 42, \"start\": 2, \"length\": 0.25}\n  ]}\n]}\n",
    );
    let mut s = f.open();
    go(&mut s, hits("hats", &[3.0], 0.25, Hit::On));
    assert!(played(&s, "hats").contains(&(3.0, 0.25, 46.0, 0.8)), "the most: {:?}", played(&s, "hats"));
    go(&mut s, hits("hats", &[0.0, 1.0], 0.25, Hit::Off));
    go(&mut s, hits("hats", &[0.5], 0.25, Hit::On));
    assert!(played(&s, "hats").contains(&(0.5, 0.25, 42.0, 0.8)), "of a tie, the lowest: {:?}", played(&s, "hats"));
}

// KIT-12: a hit in no clip goes into a new clip that is its bar, cut short at the clips beside it;
// a gesture's hits in one bar share it, and a drag across bars makes one a bar.
#[test]
fn a_hit_in_no_clip_gets_its_bar_cut_at_the_clips_beside_it() {
    let f = fixture(BUSES);
    f.write("data/kick/hits.json", "{\"clips\": [\n  {\"id\": \"kkkkkkkk\", \"start\": 6, \"length\": 2, \"notes\": []}\n]}\n");
    let mut s = f.open();
    go(&mut s, hits("kick", &[4.5], 0.25, Hit::On));
    assert_eq!(written(&s, "kick"), [(4.0, 2.0, vec![(0.5, 0.25, 36.0)]), (6.0, 2.0, vec![])]);
    let done = go(&mut s, hits("kick", &[9.0, 9.25, 12.0], 0.25, Hit::On));
    assert_eq!(done.made.len(), 2, "two new clips: {:?}", done.made);
    assert_eq!(written(&s, "kick")[2..], [(8.0, 4.0, vec![(1.0, 0.25, 36.0), (1.25, 0.25, 36.0)]), (12.0, 4.0, vec![(0.0, 0.25, 36.0)])]);
}

// KIT-11, KIT-12: off takes every note that starts in the step, off the grid or not, and a velocity
// reaches each; a step stops at its bar's end, so the next bar's first hit is never in it.
#[test]
fn a_step_holds_every_note_that_starts_in_it() {
    let f = fixture(BUSES);
    f.write(
        "data/kick/hits.json",
        "{\"clips\": [\n  {\"id\": \"kkkkkkkk\", \"start\": 0, \"length\": 8, \"notes\": [\n    \
         {\"pitch\": 36, \"start\": 1, \"length\": 0.25},\n    {\"pitch\": 36, \"start\": 1.1, \"length\": 0.5},\n    \
         {\"pitch\": 36, \"start\": 1.3, \"length\": 0.25},\n    {\"pitch\": 36, \"start\": 3, \"length\": 0.25},\n    \
         {\"pitch\": 36, \"start\": 4, \"length\": 0.25}\n  ]}\n]}\n",
    );
    let mut s = f.open();
    go(&mut s, hits("kick", &[1.0, 3.0], 0.25, Hit::Velocity(0.5)));
    let v: Vec<f64> = played(&s, "kick").iter().map(|n| n.3).collect();
    assert_eq!(v, [0.5, 0.5, 0.8, 0.5, 0.8]);
    go(&mut s, hits("kick", &[1.0], 0.25, Hit::Off));
    assert_eq!(played(&s, "kick").iter().map(|n| n.0).collect::<Vec<_>>(), [1.3, 3.0, 4.0], "both that start in it");
    // Steps of 3 beats: lines at 0 and 3 in each bar of 4, so the step at 3 ends at the bar line.
    go(&mut s, hits("kick", &[3.0], 3.0, Hit::Off));
    assert_eq!(played(&s, "kick").iter().map(|n| n.0).collect::<Vec<_>>(), [1.3, 4.0], "the step ends at the bar line");
}

// KIT-12: only a Drum track's lane has steps, and a step, a place and a velocity are numbers that
// fit; each refusal says so in words.
#[test]
fn steps_are_refused_in_words() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let g = Gesture::Hits { lane: LaneAddr::new("keys", "chords").unwrap(), at: vec![0.0], step: 0.25, to: Hit::On };
    let refused = |s: &mut Session, g: Gesture| clips::edit(s, &g).map(|_| ()).unwrap_err().to_string();
    assert_eq!(refused(&mut s, g), "edit rejected: Keys's chords lane isn't a Drum's: the beat grid writes a Drum track's lane.");
    assert_eq!(refused(&mut s, hits("kick", &[0.0], 0.0, Hit::On)), "edit rejected: A step lasts at least 1/64 of a beat.");
    assert_eq!(refused(&mut s, hits("kick", &[-1.0], 0.25, Hit::On)), "edit rejected: A step can't start before the song starts.");
    assert_eq!(
        refused(&mut s, hits("kick", &[0.0], 0.25, Hit::Velocity(1.5))),
        "edit rejected: A velocity is above 0 and at most 1, not 1.5."
    );
    // A start that isn't on a line of the grid, counted from its bar's start (KIT-12), with nothing
    // written; one a millionth from a line is on it.
    let kick = f.read("data/kick/hits.json");
    assert_eq!(
        refused(&mut s, hits("kick", &[2.0, 0.1], 0.25, Hit::On)),
        "edit rejected: 0.1 isn't where a step starts: on a grid of 0.25 beats, steps start at 0, 0.25, 0.5 and so on from each bar line."
    );
    assert_eq!(refused(&mut s, hits("kick", &[4.4], 0.5, Hit::Off)).contains("4.4 isn't where a step starts"), true);
    assert_eq!(f.read("data/kick/hits.json"), kick, "nothing written");
    go(&mut s, hits("kick", &[2.500001], 0.25, Hit::On));
    let at: Vec<f64> = played(&s, "kick").iter().map(|n| n.0).filter(|t| (2.0..3.0).contains(t)).collect();
    assert_eq!(at, [2.0, 2.5], "a millionth from a line is on it: the hit goes on the line");
}

// KIT-11: a hit a millionth before a line starts on it, as the lane format writes beats: it is in
// the step at that line, so off there takes it and on there adds no second hit beside it. (The
// web's grid pins the same place, `e2e/beat-grid.mjs`.)
#[test]
fn a_hit_a_millionth_before_a_line_is_in_that_lines_step() {
    let f = fixture(BUSES);
    f.write(
        "data/kick/hits.json",
        "{\"clips\": [\n  {\"id\": \"kkkkkkkk\", \"start\": 0, \"length\": 4, \"notes\": [\n    \
         {\"pitch\": 36, \"start\": 0.999999, \"length\": 0.25}\n  ]}\n]}\n",
    );
    let mut s = f.open();
    let done = go(&mut s, hits("kick", &[1.0], 0.25, Hit::On));
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("no change", "no change"), "the step has its hit");
    go(&mut s, hits("kick", &[0.75], 0.25, Hit::Off));
    assert_eq!(played(&s, "kick").len(), 1, "the step before doesn't hold it");
    go(&mut s, hits("kick", &[1.0], 0.25, Hit::Off));
    assert!(played(&s, "kick").is_empty(), "off at 1 takes it");
}

// ---------------------------------------------------------------- + Drum (KIT-15)

// KIT-15: + Drum adds a Drum track to the kit as one undo step: playing the sound picked, its `drum`
// written, named by it, its channel going to the kit's bus, in the timeline after the kit's last
// row. A second of one sound is named apart.
#[test]
fn plus_drum_adds_a_row_routed_to_the_bus() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let song = f.read("song.ts");
    let made = kits::add_drum(&mut s, "drums", "snare").unwrap();
    assert_eq!((made.module.as_str(), made.label.as_str()), ("snare", "Snare"));
    assert_eq!(
        f.read("tracks/snare.ts"),
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
         export default track(\"snare\", { name: \"Snare\" }, ({ lane }) => drums.drum(lane.notes(\"hits\"), { drum: \"snare\" }));\n"
    );
    let now = f.read("song.ts");
    assert!(now.contains("      snare: { in: snare.out, to: \"drums\" },\n"), "{now}");
    assert_eq!(rows(&s), [row("kick", "hits", Some("kick")), row("hats", "hits", Some("hats")), row("snare", "hits", Some("snare"))]);
    assert_eq!(&crate::layout::view(&s).order[..4], ["kick", "hats", "snare", "drums"], "after the kit's last row");
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read("song.ts"), song, "one undo takes it all back");
    assert!(!f.0.join("tracks/snare.ts").exists());
    assert_eq!(rows(&s).len(), 2);
    crate::redo(&mut s).unwrap();
    let again = kits::add_drum(&mut s, "drums", "snare").unwrap();
    assert_eq!((again.module.as_str(), again.label.as_str()), ("snare-2", "Snare 2"));
    assert_eq!(rows(&s).last(), Some(&row("snare-2", "hits", Some("snare"))));
}

// KIT-15: + Drum is refused in words for a sound the Drum hasn't got, or a bus that is no kit's.
#[test]
fn plus_drum_is_refused_in_words() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let said = |r: Result<crate::tracks::Made, crate::EditError>| r.map(|m| m.module).unwrap_err().to_string();
    assert_eq!(
        said(kits::add_drum(&mut s, "drums", "gong")),
        "edit rejected: A Drum plays \"kick\", \"snare\", \"clap\", \"hats\", \"tom\", \"rim\", \"cowbell\", \"cymbal\" or \"shaker\", not \"gong\"."
    );
    assert_eq!(said(kits::add_drum(&mut s, "verb", "kick")), "edit rejected: Reverb is no drum kit's bus: no Drum track goes to it.");
}

// KIT-11, KIT-12: a step ends at the grid's next line from its bar's start, as the grid draws it:
// triplet steps, whose starts the lane writes to a millionth, never reach into the next step.
#[test]
fn triplet_steps_end_at_the_next_line() {
    let f = fixture(BUSES);
    f.write("data/kick/hits.json", "{\"clips\": []}\n");
    let mut s = f.open();
    let third = 1.0 / 6.0;
    let line = |k: f64| (k * third * 1e6).round() / 1e6;
    go(&mut s, hits("kick", &[line(0.0), line(1.0), line(2.0)], third, Hit::On));
    assert_eq!(played(&s, "kick").iter().map(|n| n.0).collect::<Vec<_>>(), [0.0, line(1.0), line(2.0)]);
    go(&mut s, hits("kick", &[line(1.0)], third, Hit::Off));
    assert_eq!(played(&s, "kick").iter().map(|n| n.0).collect::<Vec<_>>(), [0.0, line(2.0)], "only the hit that starts in it");
}
