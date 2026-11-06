//! The song's key (VOX-10): written into `data/arrangement.json` with the tempo and the meter, one
//! undo step in a revision, read back into the timeline from the revision that plays.

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

const TEMPO: &str = "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 124}\n  ],\n  \"meter\": []";

// VOX-10: `"key": {"root": "A", "scale": "minor"}` in the arrangement, after the meter: one undo
// step with a revision; the timeline shows it, and nothing else is written.
#[test]
fn a_key_is_written_into_the_arrangement() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (rev, song) = (s.accepted().number, f.read("song.ts"));
    let done = set(&mut s, key("A", "minor")).unwrap();
    assert_eq!((done.file.as_str(), done.before.as_str(), done.after.as_str()), ("data/arrangement.json", "", "A minor"));
    assert!(done.revision > rev, "a revision");
    assert_eq!(f.read("data/arrangement.json"), format!("{TEMPO},\n  \"key\": {{\"root\": \"A\", \"scale\": \"minor\"}}\n}}\n"));
    assert_eq!(s.accepted().project.arrangement.key(), key("A", "minor"));
    assert_eq!(shown(&s), key("A", "minor"));
    assert_eq!(f.read("song.ts"), song);
    assert!(!f.0.join("layout.json").exists(), "the layout holds no key");
    // A root by another name is kept in the file's spelling; the label in the app's.
    let done = set(&mut s, key("D♯", "Major")).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("A minor", "E♭ major"));
    assert_eq!(shown(&s), key("Eb", "major"));
    // The same key again writes nothing.
    let (text, rev) = (f.read("data/arrangement.json"), s.accepted().number);
    let same = set(&mut s, key("Eb", "major")).unwrap();
    assert_eq!((same.before.as_str(), same.after.as_str(), same.revision), ("no change", "no change", rev));
    assert_eq!(f.read("data/arrangement.json"), text);
    // The tempo and the meter stay as they are.
    crate::song::tempo(&mut s, 0.0, 90.0).unwrap();
    assert_eq!(shown(&s), key("Eb", "major"));
}

#[test]
fn no_key_removes_it() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    set(&mut s, key("C", "major")).unwrap();
    let done = set(&mut s, None).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("C major", ""));
    assert_eq!(f.read("data/arrangement.json"), format!("{TEMPO}\n}}\n"));
    assert_eq!(shown(&s), None);
}

// FMT-19, VOX-10: setting the key never touches `layout.json`, which holds no key: one there from
// before (an older song's, VOX-14) is no key, and stays as it is.
#[test]
fn the_layout_is_left_alone() {
    let f = fixture("examples/first-song");
    let layout = "{\"order\": [\"bass\", \"kick\"], \"key\": {\"root\": \"E\", \"scale\": \"minor\"}, \"zoom\": 3}";
    f.write("layout.json", layout);
    let mut s = f.open();
    assert_eq!(shown(&s), None);
    set(&mut s, key("F#", "minor")).unwrap();
    assert_eq!(f.read("layout.json"), layout);
    assert_eq!(shown(&s), key("F#", "minor"));
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
    assert_eq!(f.read("data/arrangement.json"), format!("{TEMPO}\n}}\n"));
    let done = set_written(&mut s, written("d♭", "Major Pentatonic")).unwrap();
    assert_eq!(done.after, "C♯ major pentatonic");
}

// VOX-11: a key written by hand that music2 doesn't know is no key, with a Warning; setting the key
// writes over it.
#[test]
fn a_key_music2_doesnt_know_is_no_key() {
    let f = fixture("examples/first-song");
    f.write("data/arrangement.json", format!("{TEMPO},\n  \"key\": {{\"root\": \"A\", \"scale\": \"bebop\"}}\n}}\n"));
    let mut s = f.open();
    assert_eq!(shown(&s), None);
    assert!(s.problems().iter().any(|d| d.code == "arrangement.key-unknown"), "{:?}", s.problems());
    let done = set(&mut s, key("A", "minor")).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("", "A minor"));
    assert_eq!(shown(&s), key("A", "minor"));
    assert!(!s.problems().iter().any(|d| d.code == "arrangement.key-unknown"));
}

// NOTE-1, FMT-19: a key in any scale of the table is written by the scale's name, however the
// request spelled it, and reads back.
#[test]
fn a_key_in_any_scale_is_written_into_the_arrangement() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    for row in crate::scales::TABLE.iter() {
        let done = set(&mut s, key("D", &row.label.to_uppercase())).unwrap();
        let k = SongKey::new("D", row.name).unwrap();
        assert_eq!(done.after, k.name());
        let text = f.read("data/arrangement.json");
        assert!(text.contains(&format!("\"key\": {{\"root\": \"D\", \"scale\": \"{}\"}}", row.name)), "{text}");
        assert_eq!(shown(&s), Some(k));
    }
    assert_eq!(f.read("data/arrangement.json").matches("\"key\"").count(), 1);
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
    assert_eq!(f.read("data/arrangement.json"), format!("{TEMPO}\n}}\n"));
    assert_eq!(shown(&s), None);
    assert!(s.accepted().number > rev, "an undo is a revision of its own");
    s.redo().unwrap();
    assert_eq!(shown(&s), key("E", "minor"));
}

// VOX-10: a layout.json that doesn't read costs the key nothing: the key isn't there.
#[test]
fn an_unreadable_layout_doesnt_matter() {
    let f = fixture("examples/first-song");
    let broken = "{\n<<<<<<< HEAD\n  \"order\": [\"kick\"]\n=======\n}\n";
    f.write("layout.json", broken);
    let mut s = f.open();
    set(&mut s, key("A", "minor")).unwrap();
    assert_eq!(f.read("layout.json"), broken);
    assert_eq!(shown(&s), key("A", "minor"));
}
