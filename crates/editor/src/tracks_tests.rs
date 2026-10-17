//! The track builders on the fixtures, the templates and p16: what each writes, byte for byte,
//! that it is one undo step, and what is refused.

use m2_session::Session;

use super::*;
use crate::Gestures;
use crate::testing::{Fixture, MANIFEST, files_under, fixture};

fn render(s: &Session, seconds: f64) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(seconds).unwrap()
}

/// Every file of the project, by path, with its bytes.
fn tree(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_str().unwrap().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

/// A p16 reference project (16 tracks, design-8b §3.6).
fn p16() -> Fixture {
    let f = Fixture::new();
    m2_session::reference::write(&f.0, &m2_session::reference::Reference::P16).unwrap();
    f
}

fn order(s: &Session) -> Vec<String> {
    s.timeline().tracks.into_iter().filter(|t| t.track).map(|t| t.module).collect()
}

fn colours(s: &Session) -> BTreeMap<String, String> {
    s.timeline().tracks.into_iter().filter(|t| t.track).map(|t| (t.module, t.colour)).collect()
}

// WRITE-10, ID-2, ID-18, R2: adding a Synth pluck writes its file, one import and one channel in
// the song, its empty lane and the layout, and is Confined on every fixture and p16: every node it
// adds has a place in the ranges it declares (the import nodes and std's new strip in the mixer
// call). What every other track sounds like is untouched. It is a member of an exported object, so
// the file imports the object and calls the member.
#[test]
fn adding_a_track_is_confined_on_every_fixture() {
    for f in [fixture("examples/first-song"), fixture("examples/resample"), fixture("templates/starter"), fixture("templates/empty"), p16()]
    {
        let mut s = f.open();
        let song = f.read("song.ts");
        let before = colours(&s);
        let made = s.add_track("std:synth-pluck", None, None, None).unwrap_or_else(|e| panic!("{}: {e}", f.0.display()));
        assert_eq!(
            (made.module.as_str(), made.label.as_str(), made.edited.file.as_str()),
            ("synth-pluck", "Synth pluck", "tracks/synth-pluck.ts")
        );
        assert_eq!(
            f.read("tracks/synth-pluck.ts"),
            "import { track } from \"@m2/core\";\nimport { synth } from \"std/synth\";\n\n\
             export default track(\"synth-pluck\", { name: \"Synth pluck\" }, ({ lane }) => synth.pluck(lane.notes(\"notes\")));\n"
        );
        assert_eq!(f.read("data/synth-pluck/notes.json"), m2_song::canonical(&LaneData::Notes(vec![])));
        let now = f.read("song.ts");
        assert_eq!(now.matches("import synthPluck from \"./tracks/synth-pluck\";").count(), 1, "{now}");
        assert_eq!(now.matches("\"synth-pluck\": synthPluck.out").count(), 1, "{now}");
        // The new track is last, in the first colour no other track has; the others keep theirs.
        let after = colours(&s);
        assert_eq!(order(&s).last().map(String::as_str), Some("synth-pluck"));
        for (k, c) in &before {
            assert_eq!(after.get(k), Some(c), "{k} kept its colour");
        }
        assert!(!before.values().any(|c| *c == after["synth-pluck"]) || before.len() >= 12);
        // Only song.ts's import and its mixer changed.
        let removed: Vec<&str> = song.lines().filter(|l| !now.lines().any(|n| n == *l)).collect();
        assert!(
            removed.iter().all(|l| l.trim().starts_with('}') || l.contains("drums") || l.trim().ends_with(".out") || l.contains("})")),
            "{removed:?}"
        );
        assert!(!s.refresh(), "the session took in its own write");
    }
}

// The same song after an add: a strip of silence adds +0.0 to the mix, which can only turn a -0.0
// sample into +0.0, so the renders are compared by value (`==` on floats), not by bits.
#[test]
fn the_song_sounds_the_same_after_adding_a_track() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = render(&s, 2.0);
    s.add_track("std:synth-pad", Some("Big pad"), None, None).unwrap();
    assert_eq!(render(&s, 2.0), before);
}

// T3 and break 1 of the taste merge: every instrument the list offers adds to first-song Confined
// (add's own expectation, which places its time nodes, a voice's note fields and std's new strip
// in the ranges it declares), plays from the lane its group says, and the song sounds the same with
// all of them in: a silent track adds +0.0, so the renders are compared by value.
#[test]
fn every_listed_instrument_adds_confined_and_the_song_sounds_the_same() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = render(&s, 2.0);
    let list = s.instruments();
    assert_eq!(list.len(), 22);
    for i in &list {
        let made = s.add_track(&i.component, None, None, None).unwrap_or_else(|e| panic!("{}: {e}", i.component));
        let text = f.read(&made.edited.file);
        assert!(text.contains(&format!("{}(lane.notes(\"{}\"))", i.name, i.lane)), "{text}");
        assert!(f.0.join(format!("data/{}/{}.json", made.module, i.lane)).exists(), "{}", i.component);
    }
    assert_eq!(order(&s).len(), 4 + 22);
    assert_eq!(render(&s, 2.0), before);
}

// ID-18, TL-3: the Empty template's mixer holds only comments; the first channel goes before its
// closing brace at the comments' indentation, and std's silent stand-in for an empty mixer goes.
// A track keyed kick into a file that imports `kick` is bound as `kickTrack`; drums play from `hits`.
#[test]
fn adding_to_the_empty_template_fills_its_mixer() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    let song = f.read("song.ts");
    let made = s.add_track("std:drum-kick", None, Some("kick"), None).unwrap();
    assert_eq!(made.module, "kick");
    let want = song
        .replace(
            "import { master, mixer } from \"std/mix\";",
            "import { master, mixer } from \"std/mix\";\nimport kickTrack from \"./tracks/kick\";",
        )
        .replace("      // drums: drums.out, // drums\n", "      // drums: drums.out, // drums\n      kick: kickTrack.out,\n");
    assert_eq!(f.read("song.ts"), want);
    assert!(f.read("tracks/kick.ts").contains("drums.kick(lane.notes(\"hits\"))"));
    assert_eq!(f.read("data/kick/hits.json"), m2_song::canonical(&LaneData::Notes(vec![])));
    // A second, into the object with one property on its line now.
    s.add_track("std:drum-hats", None, None, Some("kick")).unwrap();
    assert!(f.read("song.ts").contains("      kick: kickTrack.out,\n      hats: hats.out,\n"), "{}", f.read("song.ts"));
}

// ID-2: a key never takes an unclaimed `data/<k>/` folder, in any capitals, nor a track file's name.
#[test]
fn a_key_never_takes_an_unclaimed_folder() {
    let f = fixture("examples/first-song");
    f.write("data/Synth-pluck/notes.json", "{\"clips\": []}");
    f.write("data/synth-pluck-2/notes.json", "{\"clips\": []}");
    let mut s = f.open();
    let made = s.add_track("std:synth-pluck", None, None, None).unwrap();
    assert_eq!(made.module, "synth-pluck-3");
    assert_eq!(made.edited.file, "tracks/synth-pluck-3.ts");
    // A key asked for is checked by the key rule, and refused when taken.
    let e = s.add_track("std:synth-pluck", None, Some("Lead Vox"), None).unwrap_err().to_string();
    assert!(e.contains("Lead Vox isn't an ID") && e.contains("lead-vox"), "{e}");
    let e = s.add_track("std:synth-pluck", None, Some("kick"), None).unwrap_err().to_string();
    assert!(e.contains("taken"), "{e}");
    let made = s.add_track("std:synth-pluck", Some("Lead Vox"), None, None).unwrap();
    assert_eq!((made.module.as_str(), made.label.as_str()), ("lead-vox", "Lead Vox"));
    assert!(f.read("song.ts").contains("\"lead-vox\": leadVox.out"), "a key that isn't an identifier is quoted");
}

// ID-1: bindings never collide with what the file binds, and a key of digits is bound as `t808`.
#[test]
fn bindings_never_collide() {
    assert_eq!(camel("lead-vox"), "leadVox");
    assert_eq!(camel("808"), "t808");
    assert_eq!(camel("a-2b"), "a2b");
    let src = "import { kick } from \"std/instruments\";\nconst kickTrack = 1;\n";
    assert_eq!(free_binding(src, "kick"), "kickTrack2");
    assert_eq!(free_binding(src, "default"), "defaultTrack");
    let f = fixture("examples/first-song");
    let mut s = f.open();
    assert_eq!(s.add_track("std:drum-kick", Some("808"), None, None).unwrap().module, "808");
    assert!(f.read("song.ts").contains("import t808 from \"./tracks/808\";") && f.read("song.ts").contains("\"808\": t808.out"));
}

// R12: a song with no mixer, or two, has no one place for a new channel: refused, saying so.
#[test]
fn a_song_without_one_mixer_is_refused() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write("song.ts", "import { song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nexport default song(() => osc.sine(440));\n");
    let mut s = f.open();
    let e = s.add_track("std:synth-pluck", None, None, None).unwrap_err().to_string();
    assert!(e.contains("mixes its tracks its own way"), "{e}");
    f.write(
        "song.ts",
        "import { song } from \"@m2/core\";\nimport { add } from \"std/math\";\nimport { mixer } from \"std/mix\";\n\
         export default song(() => add(mixer({}), mixer({})));\n",
    );
    assert!(s.refresh());
    let e = s.add_track("std:synth-pluck", None, None, None).unwrap_err().to_string();
    assert!(e.contains("2 mixers"), "{e}");
    assert!(!f.0.join("tracks").exists());
}

// WRITE-21: one undo step takes back every file add wrote, the layout's first write included.
#[test]
fn undo_removes_everything_add_wrote() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = tree(&f);
    s.add_track("std:synth-bass", None, None, Some("kick")).unwrap();
    assert!(f.0.join("layout.json").exists() && f.0.join("data/synth-bass").exists());
    s.undo().unwrap();
    assert_eq!(tree(&f), before);
    assert!(!f.0.join("data/synth-bass").exists() && !f.0.join("layout.json").exists());
}

// FMT-12, ID-18: a duplicate copies the code under its new key and name, every clip with a new
// id, and the channel with what it does: the bass's duck stays. It goes below its original, in its
// colour.
#[test]
fn duplicating_copies_code_data_and_channel() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let colour = colours(&s)["bass"].clone();
    let made = s.duplicate_track("bass").unwrap();
    assert_eq!((made.module.as_str(), made.label.as_str(), made.edited.file.as_str()), ("bass-2", "Bass 2", "tracks/bass-2.ts"));
    assert_eq!(
        f.read("tracks/bass-2.ts"),
        f.read("tracks/bass.ts").replace("track(\"bass\", { name: \"Bass\" }", "track(\"bass-2\", { name: \"Bass 2\" }")
    );
    let song = f.read("song.ts");
    assert!(song.contains("import bass2 from \"./tracks/bass-2\";"), "{song}");
    assert!(song.contains("\"bass-2\": duck(bass2.out, kick.env, { depth: db(-9) }),"), "{song}");
    assert!(song.contains("bass: duck(bass.out, kick.env, { depth: db(-9) }),"), "the original's channel stays");
    // The clips, each with a new id, unique.
    let ids = |p: &str| -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(&f.read(p)).unwrap();
        v["clips"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap_or("").to_string()).collect()
    };
    for lane in ["notes", "cutoff", "drop"] {
        let (old, new) = (ids(&format!("data/bass/{lane}.json")), ids(&format!("data/bass-2/{lane}.json")));
        assert_eq!(old.len(), new.len());
        assert!(new.iter().all(|i| !i.is_empty() && !old.contains(i)), "{lane}: {old:?} {new:?}");
        let mut unique = new.clone();
        unique.dedup();
        assert_eq!(unique.len(), new.len());
    }
    let o = order(&s);
    let at = o.iter().position(|k| k == "bass").unwrap();
    assert_eq!(o[at + 1], "bass-2");
    assert_eq!(colours(&s)["bass-2"], colour);
    s.undo().unwrap();
    assert!(!f.0.join("tracks/bass-2.ts").exists() && !f.0.join("data/bass-2").exists());
}

// WRITE-15: a plugin's state is copied byte for byte, with its sidecar.
#[test]
fn duplicating_copies_plugin_state() {
    let f = fixture("examples/with-plugins");
    let mut s = f.open();
    if !s.draft_errors.is_empty() || !s.accepted().has_code() {
        // The plugins aren't installed here: the song doesn't build, so nothing can be duplicated.
        return;
    }
    s.duplicate_track("lead").unwrap();
    assert_eq!(std::fs::read(f.0.join("data/lead-2/lead.state")).unwrap(), std::fs::read(f.0.join("data/lead/lead.state")).unwrap());
    assert_eq!(f.read("data/lead-2/lead.state.json"), f.read("data/lead/lead.state.json"));
}

// R12: a track a helper makes (no one `track()` call for it) is refused, saying so.
#[test]
fn a_track_a_helper_makes_is_not_duplicated() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { hat } from \"std/instruments\";\nimport { mixer } from \"std/mix\";\n\
         const make = (k: string) => track(k, ({ lane }) => hat(lane.notes(\"hits\")));\n\
         const a = make(\"a\");\nconst b = make(\"b\");\n\
         export default song(() => mixer({ a: a.out, b: b.out }));\n",
    );
    let mut s = f.open();
    let e = s.duplicate_track("a").unwrap_err().to_string();
    assert!(e.contains("made by code that makes several tracks"), "{e}");
    let e = s.delete_track("a", false).unwrap_err().to_string();
    assert!(e.contains("made by code that makes several tracks"), "{e}");
}

// A track written in the song's own file (the Empty template's way) is duplicated as its
// statement, right after itself, with a free binding.
#[test]
fn a_track_in_the_song_file_is_duplicated_as_its_statement() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let song = "import { song, track } from \"@m2/core\";\nimport { kick } from \"std/instruments\";\nimport { master, mixer } from \"std/mix\";\n\n\
                const drums = track(\"drums\", { name: \"Drums\" }, ({ lane }) => kick(lane.notes(\"hits\")));\n\n\
                export default song(() =>\n  master(\n    mixer({\n      drums: drums.out,\n    }),\n  ),\n);\n";
    f.write("song.ts", song);
    let mut s = f.open();
    s.duplicate_track("drums").unwrap();
    let want = song
        .replace(
            "kick(lane.notes(\"hits\")));\n",
            "kick(lane.notes(\"hits\")));\nconst drums2 = track(\"drums-2\", { name: \"Drums 2\" }, ({ lane }) => kick(lane.notes(\"hits\")));\n",
        )
        .replace("      drums: drums.out,\n", "      drums: drums.out,\n      \"drums-2\": drums2.out,\n");
    assert_eq!(f.read("song.ts"), want);
    // And deleted as its statement.
    match s.delete_track("drums-2", false).unwrap() {
        Deleted::Done { .. } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(f.read("song.ts"), song);
}

// WRITE-10: deleting Hats takes out its file, its import, its channel, its data and its layout
// entry; the other channels stay as they were.
#[test]
fn deleting_removes_file_import_channel_data_and_layout() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    s.reorder(&order(&s)).unwrap();
    let song = f.read("song.ts");
    let Deleted::Done { edited, excised } = s.delete_track("hats", false).unwrap() else { panic!() };
    assert_eq!((edited.before.as_str(), edited.after.as_str(), excised.len()), ("Hats", "", 0));
    assert!(!f.0.join("tracks/hats.ts").exists() && !f.0.join("data/hats").exists());
    let want =
        song.replace("import hats from \"./tracks/hats\";\n", "").replace("      hats: { in: hats.out, db: db(-6), pan: 0.3 },\n", "");
    assert_eq!(f.read("song.ts"), want);
    let layout: serde_json::Value = serde_json::from_str(&f.read("layout.json")).unwrap();
    assert!(!layout.to_string().contains("hats"), "{layout}");
    assert_eq!(order(&s), ["kick", "bass", "keys"]);
}

// MON-2, the code and graph design's cascade rule: the kick feeds the bass's duck, so deleting it
// asks (409 at the route) and lists the duck; with the cascade, the duck comes off (`bass:
// bass.out`); undo puts every byte back.
#[test]
fn deleting_the_kick_lists_its_ducks_and_the_cascade_takes_them_off() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = tree(&f);
    let Deleted::Asks(deps) = s.delete_track("kick", false).unwrap() else { panic!() };
    assert_eq!(deps.len(), 1);
    assert_eq!(
        (deps[0].module.as_str(), deps[0].label.as_str(), deps[0].component.as_str(), deps[0].file.as_str()),
        ("bass", "Bass", "Duck", "song.ts")
    );
    assert_eq!(tree(&f), before, "asking writes nothing");
    let Deleted::Done { excised, .. } = s.delete_track("kick", true).unwrap() else { panic!() };
    assert_eq!(excised.len(), 1);
    let song = f.read("song.ts");
    assert!(song.contains("      bass: bass.out,\n"), "{song}");
    // Nothing reads `duck` now, so its import goes too; `db` is still read by the hats. A comment
    // is the producer's, so the one above the bass stays.
    assert!(!song.contains("kick.") && !song.contains("duck(") && !song.contains("{ duck }") && !song.contains("./tracks/kick"), "{song}");
    assert!(song.contains("import { song, db } from \"@m2/core\";"), "{song}");
    s.undo().unwrap();
    assert_eq!(tree(&f), before);
}

// The starter's ducks sit inside channel objects: the cascade leaves `{ in: bass.out, db: … }`.
#[test]
fn the_cascade_takes_the_ducks_off_inside_a_channel_object() {
    let f = fixture("templates/starter");
    let mut s = f.open();
    let Deleted::Asks(deps) = s.delete_track("kick", false).unwrap() else { panic!() };
    let fed: Vec<&str> = deps.iter().map(|d| d.module.as_str()).collect();
    assert_eq!(fed, ["bass", "chords"]);
    s.delete_track("kick", true).unwrap();
    let song = f.read("song.ts");
    assert!(song.contains("bass: { in: bass.out, db: db(-10) },") && song.contains("chords: { in: chords.out, db: db(-10) },"), "{song}");
}

// A read that no effect can lose blocks: refused with where it is.
#[test]
fn a_blocking_read_refuses() {
    let f = fixture("examples/first-song");
    f.write(
        "tracks/keys.ts",
        "import { track } from \"@m2/core\";\nimport { pluck } from \"std/instruments\";\nimport { scale } from \"std/math\";\nimport kick from \"./kick\";\n\n\
         export default track(\"keys\", { name: \"Keys\" }, ({ lane }) => scale(pluck(lane.notes(\"chords\")), kick.env));\n",
    );
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let before = tree(&f);
    let Deleted::Blocked(deps) = s.delete_track("kick", true).unwrap() else { panic!() };
    assert_eq!((deps[0].module.as_str(), deps[0].file.as_str(), deps[0].line), ("keys", "tracks/keys.ts", 6));
    assert_eq!(tree(&f), before);
}

// FMT-14: a file under the track's folder that isn't data keeps the folder; assets stay.
#[test]
fn a_folder_with_other_files_stays() {
    let f = fixture("examples/first-song");
    f.write("data/hats/README.txt", "notes about the hats");
    let mut s = f.open();
    s.delete_track("hats", false).unwrap();
    assert!(!f.0.join("data/hats/hits.json").exists());
    assert_eq!(f.read("data/hats/README.txt"), "notes about the hats");
}

// WRITE-15, WRITE-21: undoing a delete restores every byte, a plugin's state blob included.
#[test]
fn undoing_a_delete_restores_every_byte() {
    let f = fixture("examples/with-plugins");
    let mut s = f.open();
    if !s.accepted().has_code() {
        return;
    }
    let before = tree(&f);
    s.delete_track("lead", false).unwrap();
    assert!(!f.0.join("data/lead/lead.state").exists());
    s.undo().unwrap();
    assert_eq!(tree(&f), before);
}

// D8, taste.md §3.4: with no layout, tracks show in the mixer's order, not their import order
// (first-song imports bass, hats, keys, kick, and mixes kick, hats, bass, keys).
#[test]
fn tracks_show_in_the_mixers_order() {
    let f = fixture("examples/first-song");
    let s = f.open();
    assert_eq!(order(&s), ["kick", "hats", "bass", "keys"]);
    assert_eq!(colours(&s).values().cloned().collect::<std::collections::BTreeSet<_>>().len(), 4);
    assert_eq!(colours(&s)["kick"], "rose");
    assert_eq!(colours(&s)["hats"], "teal");
}

/// Whether two labels are hard to tell apart: one is the other with a word added or taken away
/// ("Pad" and "Synth pad"), or they differ by one letter ("Hat" and "Hats").
fn near(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (wa, wb): (Vec<&str>, Vec<&str>) = (a.split_whitespace().collect(), b.split_whitespace().collect());
    let (short, long) = if wa.len() < wb.len() { (&wa, &wb) } else { (&wb, &wa) };
    let word = long.len() == short.len() + 1 && (0..long.len()).any(|i| long[..i].iter().chain(&long[i + 1..]).eq(short.iter()));
    let (ca, cb): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let letter = match ca.len().abs_diff(cb.len()) {
        0 => ca.iter().zip(&cb).filter(|(x, y)| x != y).count() <= 1,
        1 => {
            let (s, l) = if ca.len() < cb.len() { (&ca, &cb) } else { (&cb, &ca) };
            (0..l.len()).any(|i| l[..i].iter().chain(&l[i + 1..]).eq(s.iter()))
        }
        _ => false,
    };
    word || letter
}

// T3, the + Track list: std's ready-made sounds first, in groups (drums from a `hits` lane): the
// drum kit, then the single drums and FM metal; then the instruments; then the two with every knob.
// std's first five instruments have fuller ones in std/drums and std/synth, so they have no
// category and the list leaves them out. No two rows are hard to tell apart.
#[test]
fn the_instruments_are_stds_sounds_in_order() {
    let f = fixture("templates/empty");
    let s = f.open();
    let list = s.instruments();
    let rows: Vec<(&str, &str, &str, &str)> = list.iter().map(|i| (i.component.as_str(), i.label.as_str(), i.category.as_str(), i.lane.as_str())).collect();
    let drum = |c, l| (c, l, "Drums", "hits");
    let inst = |c, l| (c, l, "Instruments", "notes");
    let design = |c, l| (c, l, "Sound design", "notes");
    let want = [
        drum("std:drum-kit", "Drum kit"),
        drum("std:drum-kick", "Kick drum"),
        drum("std:drum-snare", "Snare"),
        drum("std:drum-clap", "Clap"),
        drum("std:drum-hats", "Hats"),
        drum("std:drum-tom", "Tom"),
        drum("std:drum-rim", "Rim"),
        drum("std:drum-cowbell", "Cowbell"),
        drum("std:drum-cymbal", "Cymbal"),
        drum("std:drum-shaker", "Shaker"),
        drum("std:fm-metal", "FM metal"),
        inst("std:fm-keys", "FM keys"),
        inst("std:fm-bell", "FM bell"),
        inst("std:synth-supersaw", "Supersaw"),
        inst("std:synth-lead", "Lead"),
        inst("std:synth-pad", "Synth pad"),
        inst("std:synth-pluck", "Synth pluck"),
        inst("std:synth-sub", "808 sub"),
        inst("std:synth-bass", "Synth bass"),
        inst("std:synth-strings", "Strings"),
        design("std:fm-custom", "FM synth"),
        design("std:synth-custom", "Polysynth"),
    ];
    assert_eq!(rows, want);
    assert!(list.iter().all(|i| i.module != "std/instruments"), "the first five are left out");
    assert_eq!((list[0].name.as_str(), list[0].module.as_str()), ("drums.kit", "std/drums"), "imported as a member of its object");
    for (k, a) in list.iter().enumerate() {
        for b in &list[k + 1..] {
            assert!(!near(&a.label, &b.label), "“{}” and “{}” are hard to tell apart", a.label, b.label);
        }
    }
    assert!(near("Pad", "Synth pad") && near("Hat", "Hats") && near("Kick", "Kick drum") && !near("Synth pad", "Synth pluck"));
    assert!(s.effects().iter().all(|e| !e.component.contains("kick")), "an instrument isn't an effect");
}

// The Empty template's comment says what is true before and after + Track (design-8b §3.1), and
// it still plays silence.
#[test]
fn the_empty_templates_comment_reads_true_after_add() {
    let f = fixture("templates/empty");
    let song = f.read("song.ts");
    assert!(song.contains("Add a track with + Track (⌘T), or write one in code"), "{song}");
    assert!(!song.contains("no tracks yet"));
    let mut s = f.open();
    assert!(render(&s, 0.5).iter().all(|x| x[0] == 0.0 && x[1] == 0.0));
    s.add_track("std:drum-kick", None, None, None).unwrap();
    assert!(f.read("song.ts").contains("Add a track with + Track (⌘T), or write one in code"));
}
