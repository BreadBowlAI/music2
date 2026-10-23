//! What a step changed (AGENT-5): tracks, lanes and knobs, by the names the views use.

use crate::Gestures;
use crate::changed::{Changed, changed};
use crate::testing::fixture;

// AGENT-5: an outside change to a mixer's pan, a bass clip and a spare call inserted above them
// names the song's module and Bass, Bass's notes lane, and the pan's knob by its id now: the call
// inserted above it renumbers the ids, but not what changed.
#[test]
fn changed_names_tracks_lanes_and_knobs() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = s.accepted_arc();
    let song = f.read("song.ts");
    let at = song.find("export default").unwrap();
    let panned = song.replacen("pan: 0.3", "pan: 0.45", 1);
    assert_ne!(panned, song, "the hats' pan is 0.3");
    let spare = format!("{}const spare = db(-3);\n{}", &panned[..at], &panned[at..]);
    f.write("song.ts", &spare);
    let notes = f.read("data/bass/notes.json");
    let moved = notes.replacen("\"start\": 0", "\"start\": 0.5", 1);
    assert_ne!(moved, notes, "a bass note starts at 0");
    f.write("data/bass/notes.json", &moved);
    assert!(s.refresh());
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let pan = s.controls().into_iter().find(|c| c.file == "song.ts" && c.id.ends_with("hats.pan")).expect("the hats' pan knob");
    assert!((pan.value - 0.45).abs() < 1e-9);
    let got = changed(&before, s.accepted());
    assert_eq!(
        got,
        Changed { tracks: vec!["bass".into(), "song".into()], lanes: vec![("bass".into(), "notes".into())], knobs: vec![pan.id.clone()] },
    );
    // The same revision on both sides: nothing changed.
    assert_eq!(changed(s.accepted(), s.accepted()), Changed::default());
}

// AGENT-5: a knob that showed its declared default, written now (the commonest agent edit, "set
// the ceiling to -0.5"), is the same knob under its new id, and glows.
#[test]
fn an_inherited_knob_written_is_named() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (id, v) = s.controls().into_iter().find(|c| c.id.contains("/@")).map(|c| (c.id, c.value)).expect("a knob left out of its call");
    let before = s.accepted_arc();
    let rev = s.accepted().number;
    let to = if v == 0.0 { 0.5 } else { v * 0.5 };
    crate::value::set(&mut s, rev, &id, to, None).unwrap();
    let now: Vec<String> = s.controls().into_iter().filter(|c| (c.value - to).abs() < 1e-9 && c.written).map(|c| c.id).collect();
    let [written] = &now[..] else { panic!("one knob has the new value: {now:?}") };
    assert_ne!(written, &id, "written, it has an id of its own");
    assert_eq!(changed(&before, s.accepted()).knobs, vec![written.clone()]);
}
