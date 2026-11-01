//! Drum kits (KIT-3 to KIT-9): the kits the packs name, + Track's Drum kit, which makes one as a
//! bus and its tracks in one undo step, and what a kit's tracks play. Which tracks are a kit is read
//! from the routes directly (the kit-finding function is track `beat`'s).

use std::collections::BTreeMap;

use super::*;
use crate::Gestures;
use crate::testing::{Fixture, files_under, fixture};

/// Every file of the project but `.music2/`, by path, with its bytes.
fn tree(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_str().unwrap().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

/// The tracks in the timeline's order, by key.
fn order(s: &m2_session::Session) -> Vec<String> {
    s.timeline().tracks.into_iter().filter(|t| t.track).map(|t| t.module).collect()
}

/// Which bus each track's sound goes to, by the checker's routes: the facts a kit is (KIT-1).
fn routes(s: &m2_session::Session) -> BTreeMap<String, String> {
    s.accepted().compiled.flat.routes.iter().cloned().collect()
}

/// What the `drum` menu of module `module`'s Drum plays, as its card says it.
fn sound(s: &m2_session::Session, module: &str) -> String {
    let view = s.view();
    let m = view.modules.iter().find(|m| m.key == module).unwrap_or_else(|| panic!("{module}"));
    let knob = m.nodes.iter().flat_map(|n| &n.knobs).find(|k| k.slot.as_deref() == Some("drum")).expect("its menu");
    knob.value.as_str().unwrap_or_else(|| panic!("{module}'s menu says {}", knob.value)).to_string()
}

const DRUM: &str = "@m2/drums:drum";

// KIT-3, KIT-5: the kits the instruments name, in the list's order, each with its instrument and
// its tracks' names: @m2/drums' three on the Drum (KIT-4).
#[test]
fn the_kits_are_the_packs_own() {
    let f = fixture("templates/empty");
    let s = f.open();
    let kits = presets(&s);
    let got: Vec<(&str, &str, &str, Vec<&str>)> = kits
        .iter()
        .map(|k| (k.name.as_str(), k.component.as_str(), k.instrument.as_str(), k.tracks.iter().map(String::as_str).collect()))
        .collect();
    assert_eq!(
        got,
        [
            ("808 kit", DRUM, "Drum", vec!["Kick", "Snare", "Clap", "Hats", "Toms", "Rim", "Cowbell", "Cymbal"]),
            ("909 kit", DRUM, "Drum", vec!["Kick", "Snare", "Clap", "Hats", "Toms", "Rim", "Cymbal"]),
            ("Basic kit", DRUM, "Drum", vec!["Kick", "Snare", "Hats"]),
        ]
    );
    assert_eq!(kits[0].pack.as_ref().map(|p| p.name.as_str()), Some("@m2/drums"), "from its pack (PACK-11)");
}

// KIT-5: a kit is a Drums bus (a plain Group, its fader at the kit's level) and its tracks, made in one Tx: each track a Drum with its name and
// the kit's values in its call, an empty lane of its own and a channel going to the bus; the bus
// first, then its tracks, last in the layout. One undo takes every byte back, and a redo puts it
// all again.
#[test]
fn a_kit_is_a_bus_and_its_tracks_in_one_undo_step() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    let before = tree(&f);
    let rev = s.accepted().number;
    let made = add_kit(&mut s, DRUM, "Basic kit", None).unwrap();
    let got: Vec<(&str, &str)> = made.iter().map(|m| (m.module.as_str(), m.label.as_str())).collect();
    assert_eq!(got, [("drums", "Drums"), ("kick", "Kick"), ("snare", "Snare"), ("hats", "Hats")]);
    assert_eq!(s.accepted().number, rev + 1, "one revision");
    assert_eq!(
        f.read("tracks/drums.ts"),
        "import { bus } from \"@m2/core\";\nimport { gain } from \"std/dsp\";\n\n\
         export default bus(\"drums\", { name: \"Drums\" }, ({ input }) => gain(input));\n"
    );
    assert_eq!(
        f.read("tracks/kick.ts"),
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
         export default track(\"kick\", { name: \"Kick\" }, ({ lane }) => drums.drum(lane.notes(\"hits\"), { drum: \"kick\" }));\n"
    );
    let song = f.read("song.ts");
    assert!(
        song.contains(
            "      // beat: beat.out, // drums\n      drums: { in: drumsTrack.out, db: -2 },\n      kick: { in: kick.out, to: \"drums\" },\n      \
             snare: { in: snare.out, to: \"drums\" },\n      hats: { in: hats.out, to: \"drums\" },\n"
        ),
        "{song}"
    );
    for k in ["kick", "snare", "hats"] {
        assert_eq!(f.read(&format!("data/{k}/hits.json")), m2_song::canonical(&m2_song::LaneData::Notes(vec![])), "{k}");
    }
    assert!(!f.0.join("data/drums").exists(), "a bus has no lane");
    let bus = s.accepted().compiled.ir.modules.iter().find(|m| m.key == "drums").unwrap();
    assert!(bus.bus && bus.track, "the Drums bus is a bus");
    let to: Vec<(String, String)> = routes(&s).into_iter().collect();
    assert_eq!(to, [("hats".into(), "drums".into()), ("kick".into(), "drums".into()), ("snare".into(), "drums".into())]);
    let sounds: Vec<String> = ["kick", "snare", "hats"].iter().map(|k| sound(&s, k)).collect();
    assert_eq!(sounds, ["kick", "snare", "hats"], "each plays its sound");
    assert_eq!(order(&s), ["drums", "kick", "snare", "hats"], "the bus first, then its tracks");
    let after = tree(&f);
    crate::undo(&mut s).unwrap();
    assert_eq!(tree(&f), before, "one undo takes it all back");
    crate::redo(&mut s).unwrap();
    assert_eq!(tree(&f), after);
}

// KIT-5: in a song that has a Kick and Hats, a kit's tracks take free names and keys (Kick 2, a
// bus Drums), and the group goes after the track it is added after, in colours of its own; a kit's
// values are written in each call, a number as the card writes it.
#[test]
fn a_kit_takes_free_names_after_the_selected_track() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let made = add_kit(&mut s, DRUM, "808 kit", Some("kick")).unwrap();
    let labels: Vec<&str> = made.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(labels, ["Drums", "Kick 2", "Snare", "Clap", "Hats 2", "Toms", "Rim", "Cowbell", "Cymbal"]);
    let keys: Vec<&str> = made.iter().map(|m| m.module.as_str()).collect();
    assert_eq!(&order(&s)[..11], [&["kick"][..], &keys, &["hats"][..]].concat(), "after the kick, together");
    assert!(
        f.read("tracks/kick-2.ts").contains("drums.drum(lane.notes(\"hits\"), { drum: \"kick\", punch: -0.5, decay: 1.6 })"),
        "{}",
        f.read("tracks/kick-2.ts")
    );
    let routed = routes(&s);
    assert!(keys[1..].iter().all(|k| routed.get(*k).map(String::as_str) == Some("drums")), "{routed:?}");
    assert!(!routed.contains_key("kick") && !routed.contains_key("hats"), "the song's own drums stay as they were");
    let colours = crate::layout::effective(&crate::layout::default_order(&s), &crate::layout::read_disk(&f.0)).colours;
    let mine: Vec<&str> = keys.iter().map(|k| colours[*k]).collect();
    assert!(mine.iter().enumerate().all(|(i, c)| !mine[..i].contains(c)), "a colour of its own each: {mine:?}");
}

// KIT-5: a kit no instrument names, or an instrument the lists don't offer, is refused in words,
// and nothing is written.
#[test]
fn a_kit_nobody_names_is_refused() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    let before = tree(&f);
    let why = add_kit(&mut s, DRUM, "Jazz kit", None).unwrap_err().to_string();
    assert!(why.contains("Drum has no kit called Jazz kit: its kits are 808 kit, 909 kit and Basic kit."), "{why}");
    let why = add_kit(&mut s, "@m2/drums:drum-kit", "808 kit", None).unwrap_err().to_string();
    assert!(why.contains("@m2/drums:drum-kit"), "{why}");
    assert_eq!(tree(&f), before);
}

// KIT-4: each kit of @m2/drums plays its beat out of its bus (as heard: after the bus's fader, before
// the master's limiter) under -1 dBTP, and about as loud as the drum machine's beat was, -17 LUFS
// (±2), unless its fader is where its peak puts it (within 0.6 dB of -1): the peak holds first. Two
// bars at velocity 0.8: the kick on every beat, the snare and the clap with it on 2 and 4, the hats
// on the eighths (the last open), a cymbal, the rim and the cowbell on offbeats, a tom fill.
#[test]
fn every_kit_plays_its_beat_under_its_peak_and_as_loud_as_the_drum_machine() {
    let all = presets(&fixture("templates/empty").open());
    assert_eq!(all.len(), 3, "@m2/drums' three");
    for kit in all {
        let f = fixture("templates/empty");
        let mut s = f.open();
        let made = add_kit(&mut s, &kit.component, &kit.name, None).unwrap();
        for (i, m) in made[1..].iter().enumerate() {
            let beats: Vec<(u8, f64)> = match kit.tracks[i].as_str() {
                "Kick" => (0..8).map(|b| (36, b as f64)).collect(),
                "Snare" | "Clap" => [1.0, 3.0, 5.0, 7.0].map(|b| (if kit.tracks[i] == "Snare" { 38 } else { 39 }, b)).to_vec(),
                "Hats" => (0..16).map(|e| (if e == 15 { 46 } else { 42 }, e as f64 * 0.5)).collect(),
                "Cymbal" => vec![(49, 0.0)],
                "Rim" => vec![(37, 2.75), (37, 6.75)],
                "Cowbell" => vec![(56, 0.5), (56, 4.5)],
                "Toms" => vec![(50, 7.0), (48, 7.25), (45, 7.5), (41, 7.75)],
                other => panic!("a beat for {other}"),
            };
            let notes: Vec<String> =
                beats.iter().map(|(p, at)| format!("{{\"pitch\": {p}, \"start\": {at}, \"length\": 0.25, \"velocity\": 0.8}}")).collect();
            f.write(
                &format!("data/{}/hits.json", m.module),
                format!("{{\"clips\": [{{\"start\": 0, \"length\": 8, \"notes\": [{}]}}]}}", notes.join(", ")),
            );
        }
        let s = f.open();
        let seconds = 8.0 * 60.0 / 120.0;
        let ask = crate::listen::Ask {
            from: 0.0,
            to: seconds,
            preroll: 0.0,
            tracks: Some(vec![made[0].module.clone()]),
            overlays: None,
            picture: false,
            threads: crate::render::Threads::Exactly(1),
        };
        let heard = crate::listen::hear(s.accepted_arc(), &s.packages, &ask).unwrap();
        let bus = heard.tracks[0].heard.as_ref().expect("the bus as heard");
        let (lufs, peak) = (bus.lufs.unwrap(), bus.true_peak.unwrap());
        eprintln!("{}: {lufs:.1} LUFS, {peak:.1} dBTP out of its bus", kit.name);
        assert!(peak < -1.0, "{}: its bus peaks at {peak:.1} dBTP", kit.name);
        assert!(
            (lufs + 17.0).abs() <= 2.0 || peak > -1.6,
            "{}: {lufs:.1} LUFS, its peak {peak:.1} dBTP leaves room to be louder",
            kit.name
        );
    }
}

// ---------------------------------------------------------------- the General MIDI table (KIT-6)

// KIT-6: the table, sound by sound in the Drum's menu order (its options, from its descriptor),
// each with its own note among its notes and the words a kit's track of it is called by, the
// presets' own (Toms); no note in two.
#[test]
fn each_sound_answers_to_its_general_midi_notes() {
    let got: Vec<(&str, &[u8], u8, &str)> = GM.iter().map(|g| (g.sound, g.notes, g.own, g.name)).collect();
    let want: [(&str, &[u8], u8, &str); 17] = [
        ("kick", &[36], 36, "Kick"),
        ("kick 2", &[35], 35, "Kick 2"),
        ("snare", &[38], 38, "Snare"),
        ("snare 2", &[40], 40, "Snare 2"),
        ("clap", &[39], 39, "Clap"),
        ("hats", &[42, 44, 46], 42, "Hats"),
        ("tom", &[41, 43, 45, 47, 48, 50], 45, "Toms"),
        ("rim", &[37], 37, "Rim"),
        ("claves", &[75], 75, "Claves"),
        ("cowbell", &[56], 56, "Cowbell"),
        ("cymbal", &[49, 52, 55], 49, "Cymbal"),
        ("cymbal 2", &[57], 57, "Cymbal 2"),
        ("ride", &[51, 53], 51, "Ride"),
        ("ride 2", &[59], 59, "Ride 2"),
        ("shaker", &[69], 69, "Shaker"),
        ("shaker 2", &[70], 70, "Shaker 2"),
        ("tambourine", &[54], 54, "Tambourine"),
    ];
    assert_eq!(got, want);
    let s = fixture("templates/empty").open();
    let menu = s.catalog().components[DRUM].control("drum").and_then(|c| c.options()).expect("the Drum's menu");
    let names: Vec<&str> = (0..17).filter_map(|i| menu.name(i)).collect();
    assert_eq!(names, GM.map(|g| g.sound), "in the Drum's menu order");
    for n in 0..=127u8 {
        let answer: Vec<&str> = GM.iter().filter(|g| g.notes.contains(&n)).map(|g| g.sound).collect();
        assert!(answer.len() <= 1, "{n} is in {answer:?}");
        assert_eq!(sound_of(n), answer.first().copied(), "{n}");
    }
    assert_eq!((notes_of("hats"), own_note("tom"), notes_of("bongo"), own_note("bongo")), (&[42u8, 44, 46][..], Some(45), &[][..], None));
    // Every row the drum machine names is the table's.
    let rows = &s.catalog().components["@m2/drums:drum-kit"].editor.notes;
    assert!(rows.keys().all(|n| sound_of(*n).is_some()), "{rows:?}");
    // The presets' words for each sound are the table's.
    for k in presets(&s) {
        let d = &s.catalog().components[&k.component];
        for t in &d.editor.kits.iter().find(|x| x.name == k.name).unwrap().tracks {
            let place = t.values.iter().find(|v| v.slot == "drum").map_or(0.0, |v| v.value.value) as usize;
            assert_eq!(t.name, GM[place].name, "{}'s {}", k.name, t.name);
        }
    }
}

/// What a song whose one track plays `call` over `notes` sounds like: each note at velocity 0.8,
/// one every 2.5 s (by when every drum has ended in exact silence), a window of 2.5 s each, mono.
fn hits(call: &str, notes: &[u8]) -> Vec<Vec<f32>> {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write("data/arrangement.json", "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}], \"meter\": []}\n");
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\n\n\
             const t = track(\"t\", ({{ lane }}) => {call});\n\nexport default song(() => t.out);\n"
        ),
    );
    let at: Vec<String> = notes
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{{\"pitch\": {n}, \"start\": {}, \"length\": 0.25, \"velocity\": 0.8}}", i * 5))
        .collect();
    f.write(
        "data/t/hits.json",
        format!("{{\"clips\": [{{\"start\": 0, \"length\": {}, \"notes\": [{}]}}]}}", notes.len() * 5, at.join(", ")),
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{call}: {:?}", s.draft_errors);
    let rev = s.accepted();
    let out = m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000)
        .unwrap()
        .render(notes.len() as f64 * 2.5)
        .unwrap();
    out.chunks(120_000).map(|w| w.iter().map(|x| x[0]).collect()).collect()
}

/// What a song whose one track plays `call` over `notes` (each its key and its start, in beats at
/// 120 bpm, a sixteenth long, at velocity 0.8) sounds like, mono, for `seconds`.
fn played(call: &str, notes: &[(u8, f64)], seconds: f64) -> Vec<f32> {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write("data/arrangement.json", "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}], \"meter\": []}\n");
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\n\n\
             const t = track(\"t\", ({{ lane }}) => {call});\n\nexport default song(() => t.out);\n"
        ),
    );
    let at: Vec<String> =
        notes.iter().map(|(n, b)| format!("{{\"pitch\": {n}, \"start\": {b}, \"length\": 0.25, \"velocity\": 0.8}}")).collect();
    f.write(
        "data/t/hits.json",
        format!("{{\"clips\": [{{\"start\": 0, \"length\": {}, \"notes\": [{}]}}]}}", seconds * 2.0, at.join(", ")),
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{call}: {:?}", s.draft_errors);
    let rev = s.accepted();
    let out = m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(seconds).unwrap();
    out.iter().map(|x| x[0]).collect()
}

// KIT-20, KIT-21: a drum machine's lane of one sound's notes plays the same bits on a Drum of that
// sound (at the drum machine's 16 voices): the hits a sixteenth apart, so each rings into the next,
// and the drums that choke cut each other on the lane as on the drum machine. The tom, chromatic on
// a Drum, isn't here (KIT-22 moves its notes).
#[test]
fn a_one_sound_lane_plays_the_drum_machines_bits_on_a_drum() {
    for g in GM.iter().filter(|g| g.sound != "tom") {
        let keys: Vec<u8> = g.notes.iter().copied().filter(|n| ![52, 53, 55].contains(n)).collect();
        let notes: Vec<(u8, f64)> =
            keys.iter().chain(&keys).chain(keys.iter().rev()).enumerate().map(|(i, n)| (*n, i as f64 * 0.25)).collect();
        let machine = played("drums.kit(lane.notes(\"hits\"))", &notes, 5.0);
        let drum = played(&format!("drums.drum(lane.notes(\"hits\"), {{ drum: \"{}\", voices: 16 }})", g.sound), &notes, 5.0);
        assert!(!silent(&machine), "{}", g.sound);
        assert!(machine == drum, "the {} plays {keys:?} as the drum machine does, bit for bit", g.sound);
    }
}

fn silent(w: &[f32]) -> bool {
    w.iter().all(|x| *x == 0.0)
}

/// What a hit sounds like, as a few numbers: its level in dB in 16 bands, from 60 Hz to 12 kHz, in
/// four stretches of time after it starts (its attack, body, ring and tail).
fn heard(w: &[f32]) -> Vec<f64> {
    let rate = 48_000.0;
    let mut out = vec![];
    for (a, b) in [(0.0, 0.03), (0.03, 0.15), (0.15, 0.6), (0.6, 2.4)] {
        let part = &w[(a * rate) as usize..(b * rate) as usize];
        let n = part.len() as f64;
        for k in 0..16 {
            let hz = 60.0 * (12_000.0f64 / 60.0).powf(k as f64 / 15.0);
            let (mut re, mut im) = (0.0, 0.0);
            for (i, x) in part.iter().enumerate() {
                let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n).cos();
                let phase = std::f64::consts::TAU * hz * i as f64 / rate;
                re += *x as f64 * hann * phase.cos();
                im += *x as f64 * hann * phase.sin();
            }
            out.push(10.0 * ((re * re + im * im) / n + 1e-12).log10());
        }
    }
    out
}

// KIT-6, KIT-20: the drum machine plays every note of the table, and none other, but 52, 53 and 55,
// where it is silent and a Drum's cymbal or ride plays; at every other note the very drum the Drum
// of that note's sound plays, bit for bit, but the toms, which the Drum plays chromatically: of the
// Drums, each playing a tom note, the one that sounds most like the drum machine's (its level in
// each band, as it starts, rings and ends) is the Toms, the next at least 1.25 times as far.
#[test]
fn the_drum_machine_plays_each_note_as_the_table_says() {
    let table: Vec<u8> = GM.iter().flat_map(|g| g.notes.iter().copied()).collect();
    let rest: Vec<u8> = (0..=127).filter(|n| !table.contains(n)).collect();
    let others = hits("drums.kit(lane.notes(\"hits\"))", &rest);
    let loud: Vec<u8> = rest.iter().zip(&others).filter(|(_, w)| !silent(w)).map(|(n, _)| *n).collect();
    assert!(loud.is_empty(), "the drum machine plays {loud:?}, which no sound answers to");
    let machine = hits("drums.kit(lane.notes(\"hits\"))", &table);
    let drums: Vec<Vec<Vec<f32>>> =
        GM.iter().map(|g| hits(&format!("drums.drum(lane.notes(\"hits\"), {{ drum: \"{}\" }})", g.sound), &table)).collect();
    for (i, n) in table.iter().enumerate() {
        let sound = sound_of(*n).unwrap();
        let place = GM.iter().position(|g| g.sound == sound).unwrap();
        let (m, d) = (&machine[i], &drums[place][i]);
        assert!(!silent(d), "the Drum's {sound} plays {n}");
        if [52, 53, 55].contains(n) {
            assert!(silent(m), "the drum machine is silent at {n}");
        } else if sound != "tom" {
            assert!(m == d, "at {n} the drum machine plays the Drum's {sound}, bit for bit");
        } else {
            let mine = heard(m);
            let far: Vec<f64> = drums
                .iter()
                .map(|d| heard(&d[i]).iter().zip(&mine).map(|(a, b)| (a.max(-100.0) - b.max(-100.0)).powi(2)).sum::<f64>().sqrt())
                .collect();
            let mut by: Vec<usize> = (0..GM.len()).collect();
            by.sort_by(|a, b| far[*a].total_cmp(&far[*b]));
            assert_eq!(GM[by[0]].sound, sound, "at {n} the drum machine sounds most like the Drum's {}: {far:?}", GM[by[0]].sound);
            assert!(far[by[0]] * 1.25 <= far[by[1]], "at {n} the nearest is plain: {far:?}");
        }
    }
}

// KIT-7: split gives each sound the drum machine's notes of it, in the Drum's menu order, each
// note as it was (a tuned one by the key it plays, its pitch rounded), the hats' closed, pedal and
// open, the toms' pitches; the notes no sound answers to come back apart, in order.
#[test]
fn split_gives_each_sound_its_own_notes() {
    let n = |pitch: f64, start: f64, velocity: f64| m2_song::Note { pitch, start, length: 0.25, velocity };
    let notes = [
        n(42.0, 0.0, 0.5),
        n(36.0, 0.0, 1.0),
        n(60.0, 0.5, 0.7),
        n(46.0, 0.5, 0.6),
        n(38.0, 1.0, 0.9),
        n(43.7, 1.5, 0.4),
        n(41.0, 2.0, 0.8),
        n(48.0, 2.5, 0.8),
        n(35.0, 3.0, 0.8),
        n(81.0, 3.5, 0.3),
    ];
    let s = split(&notes);
    let got: Vec<(&str, Vec<m2_song::Note>)> = s.lanes.iter().map(|(sound, ns)| (*sound, ns.clone())).collect();
    assert_eq!(
        got,
        [
            ("kick", vec![n(36.0, 0.0, 1.0)]),
            ("kick 2", vec![n(35.0, 3.0, 0.8)]),
            ("snare", vec![n(38.0, 1.0, 0.9)]),
            ("hats", vec![n(42.0, 0.0, 0.5), n(46.0, 0.5, 0.6), n(43.7, 1.5, 0.4)]),
            ("tom", vec![n(41.0, 2.0, 0.8), n(48.0, 2.5, 0.8)]),
        ]
    );
    assert_eq!(s.left, [n(60.0, 0.5, 0.7), n(81.0, 3.5, 0.3)], "never dropped");
    assert_eq!(split(&[]), Split { lanes: vec![], left: vec![] });
}

// KIT-9: a Drum's note goes out as a General MIDI drum of its sound: one of its notes as it is,
// another sound's note to its own, or for a tom to the nearest tom note (the lower at an equal
// distance); a note no sound answers to keeps its key when it is a General MIDI drum (35 to 81),
// and goes to the sound's own when it is past them (first-song's Kick on C1, 24).
#[test]
fn a_drums_note_goes_out_as_its_sounds() {
    let out = |sound: &str, keys: &[u8]| keys.iter().map(|k| general_midi(sound, *k)).collect::<Vec<u8>>();
    assert_eq!(out("kick", &[36, 35, 38, 60, 81, 24, 82, 0]), [36, 36, 36, 60, 81, 36, 36, 36]);
    assert_eq!(out("kick 2", &[35, 36, 24]), [35, 35, 35]);
    assert_eq!(out("hats", &[42, 44, 46, 36, 81]), [42, 44, 46, 42, 81]);
    assert_eq!(out("tom", &[41, 44, 46, 49, 60, 37]), [41, 43, 45, 48, 60, 41]);
    assert_eq!(out("cymbal", &[52, 53, 55, 51, 56]), [52, 49, 55, 49, 49]);
    assert_eq!(out("ride", &[51, 53, 59, 49]), [51, 53, 51, 51]);
    assert_eq!(out("shaker", &[54, 70, 42]), [69, 69, 69]);
}

// KIT-5 (review K14): the Empty template's drums lines, uncommented as its comment says, still build
// beside a kit: their one Drum has a key no kit makes.
#[test]
fn the_empty_templates_drum_lines_build_beside_a_kit() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    add_kit(&mut s, DRUM, "Basic kit", None).unwrap();
    let song = f.read("song.ts");
    let lines: Vec<String> = song.lines().map(|l| if l.ends_with("// drums") { l.replacen("// ", "", 1) } else { l.to_string() }).collect();
    assert_eq!(lines.iter().filter(|l| l.ends_with("// drums")).count(), 2, "the two lines that end in \"drums\"");
    f.write("song.ts", lines.join("\n") + "\n");
    s.refresh();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    assert!(s.accepted().compiled.ir.modules.iter().any(|m| m.key == "beat"), "its Drum plays, beside the kit");
}
