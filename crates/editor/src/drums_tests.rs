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
    let kits = kits(&s);
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

// KIT-5: a kit is a Drums bus and its tracks, made in one Tx: each track a Drum with its name and
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
            "      // drums: beat.out, // drums\n      drums: drumsTrack.out,\n      kick: { in: kick.out, to: \"drums\" },\n      \
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

// KIT-4: every kit of @m2/drums sounds, each track its own drum, and a beat on all of its tracks
// at once, at full velocity, stays under full scale on its bus.
#[test]
fn every_kit_sounds_under_full_scale() {
    let all = kits(&fixture("templates/empty").open());
    assert_eq!(all.len(), 3, "@m2/drums' three");
    for kit in all {
        let f = fixture("templates/empty");
        let mut s = f.open();
        let made = add_kit(&mut s, &kit.component, &kit.name, None).unwrap();
        // Sixteenths for two bars on every track, each hit at full velocity: the hats open on the
        // offbeats, the toms down the drums.
        for (i, m) in made[1..].iter().enumerate() {
            let notes: Vec<String> = (0..32)
                .map(|n| {
                    let pitch = match kit.tracks[i].as_str() {
                        "Hats" if n % 4 == 2 => 46,
                        "Hats" => 42,
                        "Toms" => [50, 48, 45, 43, 41][n % 5],
                        _ => 36,
                    };
                    format!("{{\"pitch\": {pitch}, \"start\": {}, \"length\": 0.25, \"velocity\": 1}}", n as f64 * 0.25)
                })
                .collect();
            f.write(
                &format!("data/{}/hits.json", m.module),
                format!("{{\"clips\": [{{\"start\": 0, \"length\": 8, \"notes\": [{}]}}]}}", notes.join(", ")),
            );
        }
        let s = f.open();
        let rev = s.accepted();
        let out = m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(4.0).unwrap();
        let peak = out.iter().flat_map(|x| x.iter()).fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.1 && peak < 1.0, "{}: its peak is {peak}", kit.name);
    }
}
