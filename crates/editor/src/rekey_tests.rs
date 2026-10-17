//! Change a track's ID on first-song (design-8b §11.5): what it writes and moves, byte for byte,
//! what is refused in words, and that one undo puts every byte back.

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

fn refusal(r: Result<Edited, EditError>) -> String {
    match r {
        Err(EditError::Rejected(why)) => why,
        other => panic!("refused, not {other:?}"),
    }
}

// Bass → low: the key string in its track() call, its channel (named as it was), its data folder
// and its layout entry (its colour kept). The file and the binding keep their names; the label
// stays "Bass". One revision, and the session takes in its own write.
#[test]
fn rekey_moves_code_data_and_layout() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    s.recolour("bass", "green").unwrap();
    let (bass, song, notes) = (f.read("tracks/bass.ts"), f.read("song.ts"), f.read("data/bass/notes.json"));
    let rev = s.accepted().number;
    let done = rekey(&mut s, "bass", "low").unwrap();
    assert_eq!((done.file.as_str(), done.before.as_str(), done.after.as_str()), ("tracks/bass.ts", "bass", "low (and its mixer channel)"));
    assert_eq!(done.revision, rev + 1);
    assert_eq!(f.read("tracks/bass.ts"), bass.replace("track(\"bass\"", "track(\"low\""));
    assert_eq!(f.read("song.ts"), song.replace("      bass: duck(bass.out", "      low: duck(bass.out"));
    assert!(!f.0.join("data/bass").exists(), "the old folder is gone");
    assert_eq!(f.read("data/low/notes.json"), notes);
    let layout = f.read("layout.json");
    assert!(layout.contains("\"low\"") && !layout.contains("\"bass\""), "{layout}");
    assert!(layout.contains("\"low\": { \"colour\": \"green\" }"), "the colour stays with it: {layout}");
    let m = s.accepted().compiled.ir.modules.iter().find(|m| m.key == "low").expect("a track called low");
    assert_eq!(m.label(), "Bass");
    assert!(!s.refresh(), "the session took in its own write");
    assert_eq!(s.timeline().tracks.iter().map(|t| t.module.as_str()).collect::<Vec<_>>(), ["kick", "hats", "low", "keys"]);
}

// ID-18: the channel follows only when it has the track's key. Keys' channel called `pads` stays
// `pads`, and the label says nothing of a channel.
#[test]
fn the_channel_key_follows_only_when_it_equals() {
    let f = fixture("examples/first-song");
    f.write("song.ts", f.read("song.ts").replace("keys: { in: keys.out", "pads: { in: keys.out"));
    let mut s = f.open();
    let song = f.read("song.ts");
    let done = rekey(&mut s, "keys", "chords").unwrap();
    assert_eq!(done.after, "chords");
    assert_eq!(f.read("song.ts"), song, "song.ts is as it was");
    assert!(f.read("tracks/keys.ts").contains("track(\"chords\""));
    assert!(f.0.join("data/chords/chords.json").exists());
}

// A key another track, a channel or a data folder has is refused, in ID words; so is the same key.
#[test]
fn a_taken_key_is_refused() {
    let f = fixture("examples/first-song");
    f.write("song.ts", f.read("song.ts").replace("keys: { in: keys.out", "pads: { in: keys.out"));
    f.write("data/old/notes.json", "[]\n");
    let mut s = f.open();
    let before = tree(&f);
    assert_eq!(refusal(rekey(&mut s, "bass", "hats")), "hats is taken: another track or its data has that ID");
    assert_eq!(refusal(rekey(&mut s, "bass", "Hats")), "Hats isn't an ID: IDs are lowercase letters, digits and hyphens, such as hats-2");
    assert_eq!(refusal(rekey(&mut s, "bass", "old")), "old is taken: another track or its data has that ID");
    assert_eq!(refusal(rekey(&mut s, "bass", "pads")), "The mixer already has a channel called pads: pick another ID");
    assert_eq!(refusal(rekey(&mut s, "bass", "song")), "song is taken: another track or its data has that ID");
    assert_eq!(refusal(rekey(&mut s, "bass", "bass")), "Bass's ID is bass already.");
    assert!(check(&s, "bass", "low").is_ok() && check(&s, "bass", "bass").is_ok());
    assert_eq!(tree(&f), before, "nothing written");
}

// An ID that isn't one says what an ID is, with one like it.
#[test]
fn an_invalid_key_says_what_an_id_is() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    assert_eq!(
        refusal(rekey(&mut s, "bass", "lead vox")),
        "lead vox isn't an ID: IDs are lowercase letters, digits and hyphens, such as lead-vox"
    );
    assert_eq!(
        refusal(rekey(&mut s, "bass", "Low End!")),
        "Low End! isn't an ID: IDs are lowercase letters, digits and hyphens, such as low-end"
    );
    assert!(matches!(rekey(&mut s, "nobody", "low"), Err(EditError::NotFound(_))));
}

// One undo puts every byte back (code, data folder, layout), and redo moves them again.
#[test]
fn undoing_a_rekey_puts_every_byte_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    s.recolour("kick", "plum").unwrap();
    let before = tree(&f);
    rekey(&mut s, "kick", "boom").unwrap();
    let after = tree(&f);
    assert!(after.contains_key("data/boom/hits.json") && !after.contains_key("data/kick/hits.json"));
    crate::undo(&mut s).unwrap();
    assert_eq!(tree(&f), before);
    assert!(s.accepted().compiled.ir.modules.iter().any(|m| m.key == "kick"));
    crate::redo(&mut s).unwrap();
    assert_eq!(tree(&f), after);
}
