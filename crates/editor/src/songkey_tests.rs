//! The song's key (the polish plan's D2): written into `layout.json` by a Tx of that file alone,
//! read back into the timeline, keeping what else the file says, never over a file that doesn't
//! read.

use super::*;
use crate::Gestures;
use crate::testing::fixture;

fn key(root: &str, scale: &str) -> Option<SongKey> {
    Some(SongKey { root: root.into(), scale: scale.into() })
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
    assert_eq!(s.timeline().key, key("A", "minor"));
    assert_eq!(f.read("song.ts"), song);
    // A root by another name is kept in the file's spelling; the label in the app's.
    let done = set(&mut s, key("D♯", "Major")).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("A minor", "E♭ major"));
    assert_eq!(s.timeline().key, key("Eb", "major"));
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
    assert_eq!(s.timeline().key, None);
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

#[test]
fn a_scale_that_isnt_major_or_minor_is_refused() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let e = set(&mut s, key("A", "dorian")).unwrap_err().to_string();
    assert!(e.contains("a key's scale is major or minor"), "{e}");
    let e = set(&mut s, key("H", "minor")).unwrap_err().to_string();
    assert!(e.contains("H isn't a key's root"), "{e}");
    assert!(!f.0.join("layout.json").exists());
    // A key written by hand that doesn't read is no key.
    f.write("layout.json", "{\"key\": {\"root\": \"A\", \"scale\": \"lydian\"}}");
    assert_eq!(s.timeline().key, None);
}

#[test]
fn undo_puts_it_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let rev = s.accepted().number;
    set(&mut s, key("E", "minor")).unwrap();
    set(&mut s, key("A", "minor")).unwrap();
    s.undo().unwrap();
    assert_eq!(s.timeline().key, key("E", "minor"));
    s.undo().unwrap();
    assert!(!f.0.join("layout.json").exists());
    assert_eq!((s.timeline().key, s.accepted().number), (None, rev));
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
    assert_eq!(s.timeline().key, None);
}
