//! The song's key (the polish plan's D2): written into `layout.json` by a Tx of that file alone,
//! read back into the timeline, keeping what else the file says, never over a file that doesn't
//! read.

use super::*;
use crate::Gestures;
use crate::testing::fixture;

fn key(root: &str, scale: &str) -> Option<SongKey> {
    Some(SongKey::new(root, scale).unwrap())
}

/// The key the timeline shows.
fn shown(s: &m2_session::Session) -> Option<SongKey> {
    s.timeline().key.map(|k| k.key)
}

// Q22: `key: {"root": "A", "scale": "minor"}`, one undo step with no revision (PLANE-3); the
// timeline shows it.
#[test]
fn a_key_is_written_into_layout_json() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (rev, song) = (s.accepted().number, f.read("song.ts"));
    let done = set(&mut s, key("A", "minor")).unwrap();
    assert_eq!((done.revision, done.file.as_str(), done.before.as_str(), done.after.as_str()), (rev, "layout.json", "", "A minor"));
    assert_eq!(f.read("layout.json"), "{\n  \"order\": [],\n  \"tracks\": {},\n  \"key\": { \"root\": \"A\", \"scale\": \"minor\" }\n}\n");
    assert_eq!(shown(&s), key("A", "minor"));
    assert_eq!(f.read("song.ts"), song);
    // A root by another name is kept in the file's spelling; the label in the app's.
    let done = set(&mut s, key("D♯", "Major")).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("A minor", "E♭ major"));
    assert_eq!(shown(&s), key("Eb", "major"));
    // The same key again writes nothing.
    let text = f.read("layout.json");
    let same = set(&mut s, key("Eb", "major")).unwrap();
    assert_eq!((same.before.as_str(), same.after.as_str()), ("no change", "no change"));
    assert_eq!(f.read("layout.json"), text);
}

#[test]
fn no_key_removes_it() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    set(&mut s, key("C", "major")).unwrap();
    let done = set(&mut s, None).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("C major", ""));
    assert!(!f.read("layout.json").contains("\"key\""), "{}", f.read("layout.json"));
    assert_eq!(shown(&s), None);
}

// FMT-19: the order, the colours, the markers and keys this version doesn't know stay as read;
// the key goes after the markers, in place when it is there already.
#[test]
fn markers_and_unknown_keys_stay() {
    let f = fixture("examples/first-song");
    f.write(
        "layout.json",
        "{\"order\": [\"bass\", \"kick\", \"hats\", \"keys\"], \"tracks\": {\"kick\": {\"colour\": \"cyan\"}}, \
         \"zoom\": 3, \"markers\": [{\"beat\": 16, \"name\": \"Chorus\"}]}",
    );
    let mut s = f.open();
    set(&mut s, key("F#", "minor")).unwrap();
    assert_eq!(
        f.read("layout.json"),
        "{\n  \"order\": [\"bass\", \"kick\", \"hats\", \"keys\"],\n  \"tracks\": {\n    \"kick\": { \"colour\": \"cyan\" }\n  },\n  \
         \"markers\": [\n    { \"beat\": 16, \"name\": \"Chorus\" }\n  ],\n  \"key\": { \"root\": \"F#\", \"scale\": \"minor\" },\n  \"zoom\": 3\n}\n"
    );
    set(&mut s, key("G", "major")).unwrap();
    let text = f.read("layout.json");
    assert!(text.contains("\"key\": { \"root\": \"G\", \"scale\": \"major\" },\n  \"zoom\": 3"), "in its place: {text}");
    assert_eq!(s.timeline().tracks.iter().find(|t| t.module == "kick").unwrap().colour, "cyan");
}

// NOTE-1: a scale that isn't in the table, or a root that isn't one, is no key: a key as a request
// writes it is refused in words, as an edit, and nothing is written (review N9, N13).
#[test]
fn a_scale_that_isnt_in_the_table_is_refused() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let written = |root: &str, scale: &str| Some(Written { root: root.into(), scale: scale.into() });
    let e = set_written(&mut s, written("A", "bebop")).unwrap_err().to_string();
    assert_eq!(
        e,
        "edit rejected: a key's scale is one of major, minor, dorian, phrygian, lydian, mixolydian, locrian, harmonic-minor, melodic-minor, major-pentatonic, minor-pentatonic, blues, chromatic, not bebop"
    );
    let e = set_written(&mut s, written("H", "minor")).unwrap_err().to_string();
    assert!(e.contains("H isn't a key's root"), "{e}");
    assert!(!f.0.join("layout.json").exists());
    let done = set_written(&mut s, written("d♭", "Major Pentatonic")).unwrap();
    assert_eq!(done.after, "C♯ major pentatonic");
    // A key written by hand that doesn't read is no key.
    f.write("layout.json", "{\"key\": {\"root\": \"A\", \"scale\": \"bebop\"}}");
    assert_eq!(shown(&s), None);
}

// NOTE-1, FMT-19: a key in any scale of the table is written by the scale's name, however the
// request spelled it, and reads back.
#[test]
fn a_key_in_any_scale_is_written_into_layout_json() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    for row in crate::scales::TABLE.iter() {
        let done = set(&mut s, key("D", &row.label.to_uppercase())).unwrap();
        let k = SongKey::new("D", row.name).unwrap();
        assert_eq!(done.after, k.name());
        let text = f.read("layout.json");
        assert!(text.contains(&format!("\"key\": {{ \"root\": \"D\", \"scale\": \"{}\" }}", row.name)), "{text}");
        assert_eq!(shown(&s), Some(k));
    }
    assert_eq!(f.read("layout.json").matches("\"key\"").count(), 1);
    // A key file written before this table still reads.
    f.write("layout.json", "{\"key\": {\"root\": \"A\", \"scale\": \"minor\"}}");
    assert_eq!(shown(&s), key("A", "minor"));
}

// NOTE-2: the timeline publishes the key with what it means: its notes, the snap table and its
// chords, as the roll reads them.
#[test]
fn the_timeline_publishes_what_the_key_means() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    set(&mut s, key("A", "minor")).unwrap();
    let v = serde_json::to_value(s.timeline()).unwrap()["key"].clone();
    assert_eq!(
        (&v["root"], &v["scale"], &v["name"]),
        (&serde_json::json!("A"), &serde_json::json!("minor"), &serde_json::json!("A minor"))
    );
    assert_eq!(v["notes"], serde_json::json!([9, 11, 0, 2, 4, 5, 7]));
    assert_eq!(v["snap"], serde_json::json!([0, -1, 0, -1, 0, 0, -1, 0, -1, 0, -1, 0]));
    let chords = v["chords"].as_array().unwrap();
    assert_eq!(chords.len(), 7);
    assert_eq!(chords[0]["root"], 9);
    assert_eq!(chords[0]["numeral"], "i");
    assert_eq!(chords[0]["kinds"]["triad"], serde_json::json!({"name": "Am", "shapes": [[0, 3, 7], [3, 7, 12], [7, 12, 15]]}));
    assert_eq!(chords[4]["kinds"]["seventh"]["name"], "Em7");
    let kinds: Vec<&String> = chords[0]["kinds"].as_object().unwrap().keys().collect();
    assert_eq!(kinds, ["add9", "seventh", "sus2", "sus4", "triad"]);
    // No key, nothing published.
    set(&mut s, None).unwrap();
    assert_eq!(serde_json::to_value(s.timeline()).unwrap()["key"], serde_json::Value::Null);
}

#[test]
fn undo_puts_it_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let rev = s.accepted().number;
    set(&mut s, key("E", "minor")).unwrap();
    set(&mut s, key("A", "minor")).unwrap();
    s.undo().unwrap();
    assert_eq!(shown(&s), key("E", "minor"));
    s.undo().unwrap();
    assert!(!f.0.join("layout.json").exists());
    assert_eq!((shown(&s), s.accepted().number), (None, rev));
}

// R15: a layout.json that doesn't read is never overwritten: the key is refused, saying so.
#[test]
fn an_unreadable_layout_refuses() {
    let f = fixture("examples/first-song");
    let broken = "{\n<<<<<<< HEAD\n  \"order\": [\"kick\"]\n=======\n}\n";
    f.write("layout.json", broken);
    let mut s = f.open();
    let e = set(&mut s, key("A", "minor")).unwrap_err().to_string();
    assert!(e.contains("layout.json doesn't read: fix it, or reset the layout"), "{e}");
    assert_eq!(f.read("layout.json"), broken);
    assert_eq!(shown(&s), None);
}
