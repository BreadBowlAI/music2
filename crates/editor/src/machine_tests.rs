//! Make it a kit (KIT-22, KIT-23): a drum machine track becomes a kit's bus and a Drum track for each
//! sound its notes use, in one undo step, keeping the sound, or named and nothing written.

use std::collections::BTreeMap;

use super::*;
use crate::Gestures;
use crate::testing::{Fixture, MANIFEST, files_under};

/// Every file of the project but `.music2/`, by path, with its bytes.
fn tree(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_str().unwrap().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

/// A clip as these tests write one: its start, its length, and its notes (key, start in it, velocity).
type Clip<'a> = (f64, f64, &'a [(f64, f64, f64)]);

/// A song whose track `beat` ("Beat") plays the drum machine as `call` (its options after the notes)
/// from its lane `hits`, holding `clips` (each a start, a length and its notes: key, start, velocity),
/// in a channel at -5 dB, panned, sending to a Group bus `verb`.
fn machine_song(f: &Fixture, options: &str, clips: &[Clip]) {
    f.write("music2.toml", MANIFEST);
    f.write("data/arrangement.json", "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}], \"meter\": []}\n");
    f.write(
        "song.ts",
        "import { db, song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport beat from \"./tracks/beat\";\nimport verb from \"./tracks/verb\";\n\n\
         export default song(() =>\n  master(\n    mixer({\n      beat: { in: beat.out, db: db(-5), pan: 0.2, sends: { verb: db(-12) } },\n      verb: verb.out,\n    }),\n  ),\n);\n",
    );
    f.write(
        "tracks/beat.ts",
        format!(
            "import {{ track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\n\n// The beat, on one lane.\n\
             export default track(\"beat\", {{ name: \"Beat\" }}, ({{ lane }}) => drums.kit(lane.notes(\"hits\"){options}));\n"
        ),
    );
    f.write("tracks/verb.ts", "import { bus } from \"@m2/core\";\nimport { gain } from \"std/dsp\";\n\nexport default bus(\"verb\", ({ input }) => gain(input));\n");
    f.write("data/beat/hits.json", lane(clips));
}

/// A notes lane's file: each clip its start, length and notes (key, start in it, velocity).
fn lane(clips: &[Clip]) -> String {
    let clips: Vec<String> = clips
        .iter()
        .enumerate()
        .map(|(i, (start, length, notes))| {
            let notes: Vec<String> =
                notes.iter().map(|(p, s, v)| format!("{{\"pitch\": {p}, \"start\": {s}, \"length\": 0.25, \"velocity\": {v}}}")).collect();
            format!("{{\"id\": \"c{i}\", \"start\": {start}, \"length\": {length}, \"notes\": [{}]}}", notes.join(", "))
        })
        .collect();
    format!("{{\"clips\": [{}]}}\n", clips.join(", "))
}

/// The song at `s`, rendered for `seconds`: its samples' bits, both channels.
fn bits(s: &m2_session::Session, seconds: f64) -> Vec<[u32; 2]> {
    let rev = s.accepted();
    let out = m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(seconds).unwrap();
    assert!(out.iter().any(|x| x[0] != 0.0), "it plays");
    out.iter().map(|x| [x[0].to_bits(), x[1].to_bits()]).collect()
}

/// A note as these tests read one back: its pitch and its start.
type Pitched = (f64, f64);

/// The notes of lane `hits` of track `module`, clip by clip: (clip start, clip length, [(pitch, start)]).
fn notes(s: &m2_session::Session, module: &str) -> Vec<(f64, f64, Vec<Pitched>)> {
    let lane = s.accepted().compiled.data.lane(module, "hits").unwrap_or_else(|| panic!("{module}'s hits")).clone();
    let m2_song::LaneData::Notes(clips) = &*lane else { panic!("notes") };
    clips.iter().map(|c| (c.start, c.length, c.notes.iter().map(|n| (n.pitch, n.start)).collect())).collect()
}

/// The beat these tests play: a kick, closed and open hats, a snare, a ride, toms on the drum
/// machine's 41 and 48, the 52 it is silent at and a 60 off the table; then a bar of kick and hat.
const BEAT: &[Clip] = &[
    (
        0.0,
        4.0,
        &[
            (36.0, 0.0, 0.9),
            (42.0, 0.0, 0.6),
            (38.0, 1.0, 0.8),
            (46.0, 1.5, 0.7),
            (51.0, 2.0, 0.5),
            (41.0, 2.5, 0.8),
            (48.0, 3.0, 0.8),
            (52.0, 3.5, 0.8),
            (60.0, 3.75, 0.8),
        ],
    ),
    (4.0, 4.0, &[(36.0, 0.0, 0.9), (42.0, 0.5, 0.6)]),
];

// KIT-22: the drum machine's track becomes the kit's bus, keeping its key, name, channel (fader,
// pan, send) and the comment above it, and a Drum track for each sound its notes use goes to it,
// in the menu's order, named by the table, after it in the timeline: each with that sound's notes,
// clip by clip where they were, the toms on the keys that play their tuning. The notes the drum
// machine is silent at go, and are said. The kit is the song's (KIT-1). One undo puts every file back.
#[test]
fn a_drum_machine_track_becomes_a_kit_in_one_undo_step() {
    let f = Fixture::new();
    machine_song(&f, "", BEAT);
    let before = tree(&f);
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let made = make_kit(&mut s, "beat").unwrap();
    assert_eq!(made.bus, "beat");
    let keys: Vec<&str> = made.tracks.iter().map(|m| m.module.as_str()).collect();
    assert_eq!(keys, ["kick", "snare", "hats", "toms", "ride"]);
    let labels: Vec<&str> = made.tracks.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(labels, ["Kick", "Snare", "Hats", "Toms", "Ride"]);
    assert_eq!(
        f.read("tracks/beat.ts"),
        "import { bus } from \"@m2/core\";\nimport { gain } from \"std/dsp\";\n\n// The beat, on one lane.\n\
         export default bus(\"beat\", { name: \"Beat\" }, ({ input }) => gain(input));\n"
    );
    assert_eq!(
        f.read("tracks/ride.ts"),
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
         export default track(\"ride\", { name: \"Ride\" }, ({ lane }) => drums.drum(lane.notes(\"hits\"), { drum: \"ride\", voices: 16 }));\n"
    );
    let song = f.read("song.ts");
    assert!(song.contains("beat: { in: beat.out, db: db(-5), pan: 0.2, sends: { verb: db(-12) } },"), "{song}");
    for k in &keys {
        assert!(song.contains(&format!("{k}: {{ in: {k}.out, to: \"beat\" }}")), "{k}: {song}");
    }
    assert!(!f.0.join("data/beat/hits.json").exists(), "the bus has no notes lane");
    assert_eq!(notes(&s, "kick"), [(0.0, 4.0, vec![(36.0, 0.0)]), (4.0, 4.0, vec![(36.0, 0.0)])]);
    assert_eq!(notes(&s, "hats"), [(0.0, 4.0, vec![(42.0, 0.0), (46.0, 1.5)]), (4.0, 4.0, vec![(42.0, 0.5)])]);
    assert_eq!(notes(&s, "toms"), [(0.0, 4.0, vec![(39.0, 2.5), (50.0, 3.0)])], "41 and 48 on the keys that play their tuning");
    assert_eq!(notes(&s, "ride"), [(0.0, 4.0, vec![(51.0, 2.0)])]);
    assert!(made.said.iter().any(|w| w.contains("2 notes") && w.contains("silent")), "{:?}", made.said);
    assert!(made.said.iter().any(|w| w.contains("toms") && w.contains("half a semitone")), "{:?}", made.said);
    let rev = s.accepted();
    assert!(rev.compiled.ir.modules.iter().any(|m| m.key == "beat" && m.bus), "the track is a bus");
    let kits = crate::kits::drum_kits(&s);
    assert_eq!(kits.len(), 1, "{kits:?}");
    assert_eq!((kits[0].bus.as_str(), kits[0].label.as_str()), ("beat", "Beat"));
    assert_eq!(kits[0].rows.iter().map(|r| r.track.as_str()).collect::<Vec<_>>(), keys);
    let order: Vec<String> = s.timeline().tracks.into_iter().filter(|t| t.track).map(|t| t.module).collect();
    assert_eq!(order, ["beat", "kick", "snare", "hats", "toms", "ride", "verb"]);
    crate::undo(&mut s).unwrap();
    assert!(tree(&f) == before, "one undo puts every file back, byte for byte");
}

// KIT-22: a lane that plays one sound renders the same bits as the kit made of it, the drum
// machine's chokes included (an open hat cut by a closed one): the hats, the ride, the claves.
#[test]
fn a_one_sound_lane_renders_the_same_bits_as_its_kit() {
    for clips in [
        &[(0.0, 4.0, &[(42.0, 0.0, 0.6), (46.0, 0.5, 0.8), (42.0, 1.0, 0.6), (44.0, 1.5, 0.7), (46.0, 2.0, 0.9), (42.0, 2.25, 0.5)][..])][..],
        &[(0.0, 4.0, &[(51.0, 0.0, 0.6), (51.0, 0.25, 0.8), (51.0, 0.5, 0.6), (51.0, 0.75, 0.7)][..])][..],
        &[(0.0, 4.0, &[(75.0, 0.0, 0.6), (75.0, 0.125, 0.8)][..])][..],
    ] {
        let f = Fixture::new();
        machine_song(&f, "", clips);
        let mut s = f.open();
        let was = bits(&s, 4.0);
        make_kit(&mut s, "beat").unwrap();
        assert!(bits(&s, 4.0) == was, "{clips:?}: the same bits");
    }
}

// KIT-22: every track of a kit made of a lane of several sounds renders the bits the drum machine
// renders playing only that track's notes: per hit the same drum, pitch, level, length and choke.
#[test]
fn each_track_of_the_kit_plays_its_notes_as_the_drum_machine_did() {
    let beat: &[Clip] = &[(
        0.0,
        4.0,
        &[(36.0, 0.0, 0.9), (42.0, 0.0, 0.6), (38.0, 1.0, 0.8), (46.0, 1.5, 0.7), (42.0, 2.0, 0.6), (35.0, 2.5, 0.8), (56.0, 3.0, 0.7)],
    )];
    let f = Fixture::new();
    machine_song(&f, "", beat);
    let mut s = f.open();
    let made = make_kit(&mut s, "beat").unwrap();
    for t in &made.tracks {
        let sound = |p: f64| crate::drums::sound_of(p as u8);
        let mine: Vec<(f64, f64, f64)> =
            beat[0].2.iter().copied().filter(|n| Some(t.label.to_lowercase()) == sound(n.0).map(str::to_string)).collect();
        // The drum machine playing only this track's notes.
        let g = Fixture::new();
        machine_song(&g, "", &[(0.0, 4.0, &mine)]);
        let alone = bits(&g.open(), 4.0);
        // The kit with every other track's lane empty.
        for o in made.tracks.iter().filter(|o| o.module != t.module) {
            f.write(&format!("data/{}/hits.json", o.module), lane(&[]));
        }
        f.write(&format!("data/{}/hits.json", t.module), lane(&[(0.0, 4.0, &mine)]));
        assert!(bits(&f.open(), 4.0) == alone, "{}: the drum machine's bits for its notes", t.label);
    }
}

// KIT-23: the drum machine's knobs reach the Drums they move, by the single drums' conversions; each
// group's level is the fader of its tracks; its voices are each Drum's.
#[test]
fn the_drum_machines_knobs_reach_its_drums() {
    let f = Fixture::new();
    machine_song(
        &f,
        ", { kickTune: hz(50), punch: 0.6, snap: 0.75, hatDecay: ms(90), openDecay: ms(900), tomTune: hz(100), drive: 3, hatLevel: 2, percLevel: -3, voices: 12 }",
        &[(
            0.0,
            4.0,
            &[(36.0, 0.0, 0.9), (35.0, 0.5, 0.9), (40.0, 1.0, 0.8), (42.0, 1.5, 0.6), (46.0, 2.0, 0.6), (45.0, 2.5, 0.8), (51.0, 3.0, 0.5)],
        )],
    );
    f.write("tracks/beat.ts", f.read("tracks/beat.ts").replace("import { track }", "import { hz, ms, track }"));
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let made = make_kit(&mut s, "beat").unwrap();
    let call = |key: &str| {
        let text = f.read(&format!("tracks/{key}.ts"));
        text[text.find("drums.drum(").unwrap()..text.rfind(");").unwrap()].to_string()
    };
    let keys: Vec<&str> = made.tracks.iter().map(|m| m.module.as_str()).collect();
    assert_eq!(keys, ["kick", "hats", "toms", "soft-kick", "snappy-snare", "ride"]);
    assert_eq!(call("kick"), "drums.drum(lane.notes(\"hits\"), { drum: \"kick\", tune: 0.706724, punch: 0.1, drive: 3, voices: 12 })");
    assert_eq!(
        call("soft-kick"),
        "drums.drum(lane.notes(\"hits\"), { drum: \"soft kick\", tune: 0.706724, punch: 0.1, drive: 3, voices: 12 })"
    );
    assert_eq!(call("snappy-snare"), "drums.drum(lane.notes(\"hits\"), { drum: \"snappy snare\", punch: 0.15, drive: 3, voices: 12 })");
    assert_eq!(call("hats"), "drums.drum(lane.notes(\"hits\"), { drum: \"hats\", decay: 2, drive: 3, voices: 12 })");
    assert_eq!(call("toms"), "drums.drum(lane.notes(\"hits\"), { drum: \"tom\", tune: -4.542139, decay: 0.877058, drive: 3, voices: 12 })");
    assert_eq!(call("ride"), "drums.drum(lane.notes(\"hits\"), { drum: \"ride\", drive: 3, voices: 12 })");
    let song = f.read("song.ts");
    assert!(song.contains("hats: { in: hats.out, db: 2, to: \"beat\" }"), "{song}");
    assert!(song.contains("ride: { in: ride.out, db: -3, to: \"beat\" }"), "{song}");
    assert!(song.contains("kick: { in: kick.out, to: \"beat\" }"), "a level left out is no fader: {song}");
}

// KIT-23: a knob on a lane moves the lane to the Drum it moves, its unit and every point converted
// as the single drums' are, and the drum machine's track keeps no lane of it.
#[test]
fn a_knobs_lane_moves_to_the_drum_it_moves() {
    let f = Fixture::new();
    machine_song(&f, ", { kickDecay: lane.control(\"boom\", \"seconds\", 0.45) }", &[(0.0, 4.0, &[(36.0, 0.0, 0.9), (42.0, 1.0, 0.6)])]);
    f.write(
        "data/beat/boom.json",
        "{\"unit\": \"seconds\", \"clips\": [\n  {\"id\": \"a1\", \"start\": 0, \"length\": 4, \"points\": [[0, 0.45], [4, 0.9]]}\n]}\n",
    );
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    make_kit(&mut s, "beat").unwrap();
    assert!(
        f.read("tracks/kick.ts")
            .contains("drums.drum(lane.notes(\"hits\"), { drum: \"kick\", decay: lane.control(\"boom\", \"scalar\", 1), voices: 16 })")
    );
    assert!(f.read("data/kick/boom.json").contains("\"unit\": \"scalar\""), "{}", f.read("data/kick/boom.json"));
    assert!(f.read("data/kick/boom.json").contains("[4, 2]"), "{}", f.read("data/kick/boom.json"));
    assert!(!f.0.join("data/beat/boom.json").exists());
    assert!(!f.read("tracks/hats.ts").contains("boom"));
}

// KIT-23: what it can't carry exactly is named by file and line, and nothing is written: a lane of
// a knob that moves two of the new Drums, a level on a lane, a value worked out by code.
#[test]
fn what_it_cant_carry_is_named_and_nothing_is_written() {
    for (options, said) in [
        (", { drive: lane.control(\"grit\", \"db\", 0) }", "its `drive` is on a lane, and it moves 2 Drums"),
        (", { hatLevel: lane.control(\"lift\", \"db\", 0) }", "its `hatLevel` is on a lane, and a fader takes no lane"),
        (", { kickTune: 48 + 2 }", "its `kickTune` is worked out by code (`48 + 2`)"),
    ] {
        let f = Fixture::new();
        machine_song(&f, options, &[(0.0, 4.0, &[(36.0, 0.0, 0.9), (42.0, 1.0, 0.6)])]);
        let before = tree(&f);
        let mut s = f.open();
        assert!(s.draft_errors.is_empty(), "{options}: {:?}", s.draft_errors);
        let e = make_kit(&mut s, "beat").map(|m| m.bus).unwrap_err().to_string();
        assert!(e.contains(&format!("tracks/beat.ts:5: {said}")), "{options}: {e}");
        assert!(tree(&f) == before, "{options}: nothing written");
    }
}

// KIT-23: refused in words for a track it can't make a kit of.
#[test]
fn a_track_it_cant_make_a_kit_of_is_refused_in_words() {
    let f = Fixture::new();
    machine_song(&f, "", BEAT);
    let mut s = f.open();
    let said = |s: &mut m2_session::Session, key: &str| make_kit(s, key).map(|m| m.bus).unwrap_err().to_string();
    assert_eq!(said(&mut s, "verb"), "edit rejected: verb is a bus: it plays no drum machine.");
    // Another lane of notes beside the drum machine's.
    f.write(
        "tracks/beat.ts",
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { sum } from \"std/math\";\n\n\
         export default track(\"beat\", { name: \"Beat\" }, ({ lane }) => sum(drums.kit(lane.notes(\"hits\")), drums.drum(lane.notes(\"kick\"))));\n",
    );
    let mut s = f.open();
    assert_eq!(
        said(&mut s, "beat"),
        "edit rejected: Beat plays the drum machine and another instrument: put the drum machine on a track of its own first."
    );
    // A channel that plays it through an effect.
    f.write("tracks/beat.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"beat\", { name: \"Beat\" }, ({ lane }) => drums.kit(lane.notes(\"hits\")));\n");
    f.write(
        "song.ts",
        f.read("song.ts")
            .replace("beat: { in: beat.out,", "beat: { in: gain(beat.out, db(-1)),")
            .replace("import { master", "import { gain } from \"std/dsp\";\nimport { master"),
    );
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    assert!(said(&mut s, "beat").contains("Beat's channel, beat, plays it through other code"), "{}", said(&mut s, "beat"));
    // No drum machine at all.
    f.write("tracks/beat.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"beat\", { name: \"Beat\" }, ({ lane }) => drums.drum(lane.notes(\"hits\")));\n");
    let mut s = f.open();
    assert_eq!(said(&mut s, "beat"), "edit rejected: Beat plays no drum machine.");
}
