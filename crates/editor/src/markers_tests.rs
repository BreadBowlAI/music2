//! Markers (Q13): written sorted into `layout.json`, one a beat, names of one line, every other key
//! kept; one undo step each; refused while the file doesn't read.

use super::*;
use crate::Gestures;
use crate::testing::fixture;

fn m(beat: f64, name: &str) -> Marker {
    Marker { beat, name: name.into() }
}

// Q13, FMT-19: the whole list, sorted, in `layout.json`'s canonical form; the timeline shows it.
#[test]
fn markers_are_written_sorted_into_layout_json() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (rev, song) = (s.accepted().number, f.read("song.ts"));
    let done = set(&mut s, vec![m(64.0, "Chorus"), m(0.0, "Intro"), m(32.5, "Verse")]).unwrap();
    assert_eq!((done.revision, done.file.as_str()), (rev, "layout.json"), "no revision: layout only");
    assert_eq!(
        f.read("layout.json"),
        "{\n  \"order\": [],\n  \"tracks\": {},\n  \"markers\": [\n    { \"beat\": 0, \"name\": \"Intro\" },\n    \
         { \"beat\": 32.5, \"name\": \"Verse\" },\n    { \"beat\": 64, \"name\": \"Chorus\" }\n  ]\n}\n"
    );
    assert_eq!(s.timeline().markers, [m(0.0, "Intro"), m(32.5, "Verse"), m(64.0, "Chorus")]);
    assert_eq!(f.read("song.ts"), song);
    // Each change says what it did, for Undo's words.
    let label = |s: &mut Session, list: Vec<Marker>| set(s, list).map(|d| (d.before, d.after)).unwrap();
    let three = vec![m(0.0, "Intro"), m(32.5, "Verse"), m(64.0, "Chorus")];
    let added = [three.clone(), vec![m(96.0, "Outro")]].concat();
    assert_eq!(label(&mut s, added.clone()), ("".into(), "Outro at bar 25".into()));
    let renamed = [three.clone(), vec![m(96.0, "End")]].concat();
    assert_eq!(label(&mut s, renamed), ("Outro".into(), "End".into()));
    let moved = [three.clone(), vec![m(100.0, "End")]].concat();
    assert_eq!(label(&mut s, moved), ("End at bar 25".into(), "End at bar 26".into()));
    assert_eq!(label(&mut s, three.clone()), ("End".into(), "".into()));
    // Nothing changed: no step.
    let r = s.accepted().number;
    let undoable = s.can_undo();
    assert_eq!(set(&mut s, three).unwrap().revision, r);
    assert_eq!(s.can_undo(), undoable);
    // None left: no `markers` key.
    set(&mut s, vec![]).unwrap();
    assert!(!f.read("layout.json").contains("markers"));
}

#[test]
fn a_second_marker_on_a_beat_is_refused() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let e = set(&mut s, vec![m(16.0, "Verse"), m(64.0, "Chorus"), m(16.0, "Drop")]).unwrap_err().to_string();
    assert!(e.contains("A marker is at bar 5 already"), "{e}");
    assert!(!f.0.join("layout.json").exists(), "nothing written");
}

#[test]
fn a_name_of_two_lines_is_refused() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    for (list, words) in [
        (vec![m(0.0, "Intro\nVerse")], "A marker's name is one line"),
        (vec![m(0.0, "  ")], "A marker needs a name"),
        (vec![m(0.0, &"x".repeat(41))], "A marker's name is 40 characters at most"),
        (vec![m(-4.0, "Pickup")], "A marker goes on a beat from the song's start on"),
    ] {
        let e = set(&mut s, list).unwrap_err().to_string();
        assert!(e.contains(words), "{e}");
    }
    set(&mut s, vec![m(0.0, &"é".repeat(40))]).unwrap();
    assert!(f.read("layout.json").contains(&"é".repeat(40)), "40 characters, not bytes");
}

// FMT-19: a key it doesn't know, the song's key, the order and colours: all kept as read.
#[test]
fn markers_keep_unknown_keys_and_the_key() {
    let f = fixture("examples/first-song");
    let text = "{\n  \"order\": [\"bass\", \"kick\", \"hats\", \"keys\"],\n  \"tracks\": {\n    \"bass\": { \"colour\": \"plum\", \"height\": 3 }\n  },\n  \
                \"markers\": [{ \"beat\": 8, \"name\": \"Old\", \"colour\": \"rose\" }, { \"beat\": \"x\" }],\n  \"key\": { \"root\": \"A\", \"scale\": \"minor\" },\n  \"zoomies\": 1\n}\n";
    f.write("layout.json", text);
    let mut s = f.open();
    assert_eq!(s.timeline().markers, [m(8.0, "Old")], "the entry that doesn't read is left out");
    set(&mut s, vec![m(8.0, "Old"), m(16.0, "Verse")]).unwrap();
    let after = Layout::read(&f.read("layout.json")).unwrap();
    assert_eq!(after.order, ["bass", "kick", "hats", "keys"]);
    assert_eq!(after.colour("bass"), Some("plum"));
    let other: Vec<&str> = after.other.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(other, ["key", "zoomies"]);
    assert!(f.read("layout.json").contains("\"key\": { \"root\": \"A\", \"scale\": \"minor\" }"));
    assert_eq!(read(&after), [m(8.0, "Old"), m(16.0, "Verse")]);
}

// One undo step each, every byte back.
#[test]
fn undo_puts_the_old_list_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    set(&mut s, vec![m(0.0, "Intro")]).unwrap();
    let one = f.read("layout.json");
    set(&mut s, vec![m(0.0, "Intro"), m(64.0, "Chorus")]).unwrap();
    s.undo().unwrap();
    assert_eq!(f.read("layout.json"), one);
    assert_eq!(s.timeline().markers, [m(0.0, "Intro")]);
    s.undo().unwrap();
    assert!(!f.0.join("layout.json").exists());
    assert!(s.timeline().markers.is_empty());
    s.redo().unwrap();
    assert_eq!(f.read("layout.json"), one);
}

// design-8b §2.1, R15: a file that doesn't read is never written over by a marker.
#[test]
fn an_unreadable_layout_refuses() {
    let f = fixture("examples/first-song");
    f.write("layout.json", "{ \"order\": [\n<<<<<<< ours\n");
    let mut s = f.open();
    assert!(s.timeline().markers.is_empty());
    let e = set(&mut s, vec![m(0.0, "Intro")]).unwrap_err().to_string();
    assert!(e.contains("layout.json doesn't read: fix it, or reset the layout"), "{e}");
    assert_eq!(f.read("layout.json"), "{ \"order\": [\n<<<<<<< ours\n");
}
