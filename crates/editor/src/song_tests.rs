//! The song bar's builders on the fixtures: what each writes, byte for byte, that it is one undo
//! step, and what is refused.

use std::collections::BTreeMap;

use super::*;
use crate::testing::{Fixture, files_under, fixture};

/// Every file of the project, by path, with its bytes.
fn tree(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_str().unwrap().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

/// The paths whose bytes differ between two trees.
fn changed(a: &BTreeMap<String, Vec<u8>>, b: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter().filter(|k| a.get(*k) != b.get(*k)).cloned().collect()
}

const TWO_TEMPOS: &str = r#"{"tempo": [{"beat": 0, "bpm": 124}, {"beat": 32, "bpm": 100}], "meter": []}"#;

// WRITE-10, FMT-13, FMT-8a: a tempo edit writes data/arrangement.json, in its canonical form, and
// nothing else; the graph is the same (Confined {}), so the code isn't touched; history names it in
// BPM.
#[test]
fn a_tempo_edit_writes_only_the_arrangement() {
    for rel in ["examples/first-song", "templates/starter", "templates/empty"] {
        let f = fixture(rel);
        let mut s = f.open();
        let before = tree(&f);
        let was = s.accepted().project.arrangement.tempo[0].bpm;
        let e = tempo(&mut s, 0.0, 128.0).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!((e.file.as_str(), e.before, e.after.as_str()), (FILE, bpm_words(was), "128 BPM"), "{rel}");
        assert_eq!(changed(&before, &tree(&f)), vec![FILE.to_string()], "{rel}");
        assert_eq!(f.read(FILE), "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 128}\n  ],\n  \"meter\": []\n}\n", "{rel}");
        assert_eq!(s.accepted().project.arrangement.tempo, vec![Tempo { beat: 0.0, bpm: 128.0 }]);
        // The same tempo again writes nothing.
        let n = s.accepted().number;
        let same = tempo(&mut s, 0.0, 128.0).unwrap();
        assert_eq!((same.revision, same.before.as_str(), same.after.as_str()), (n, "128 BPM", "128 BPM"));
    }
}

// The song bar edits the tempo at the playhead: with a change at bar 9, the playhead after it edits
// that change, before it the song's own (design-8b §4.4). A change added between them goes in
// order, and one at a beat that has one sets it.
#[test]
fn the_tempo_at_the_playhead_is_the_one_edited() {
    let f = fixture("examples/first-song");
    f.write(FILE, TWO_TEMPOS);
    let mut s = f.open();
    let e = tempo(&mut s, 40.0, 96.5).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("100 BPM from bar 9", "96.5 BPM from bar 9"));
    let e = tempo(&mut s, 31.9, 126.0).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("124 BPM", "126 BPM"));
    let bpms = |s: &Session| s.accepted().project.arrangement.tempo.iter().map(|t| (t.beat, t.bpm)).collect::<Vec<_>>();
    assert_eq!(bpms(&s), vec![(0.0, 126.0), (32.0, 96.5)]);
    let e = tempo_change(&mut s, 16.0, 110.0).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("126 BPM", "110 BPM from bar 5"));
    assert_eq!(bpms(&s), vec![(0.0, 126.0), (16.0, 110.0), (32.0, 96.5)]);
    let e = tempo_change(&mut s, 18.0, 112.0).unwrap();
    assert_eq!(e.after, "112 BPM from 5.3", "off a bar line: bar and beat");
    tempo_change(&mut s, 16.0, 111.0).unwrap();
    assert_eq!(bpms(&s), vec![(0.0, 126.0), (16.0, 111.0), (18.0, 112.0), (32.0, 96.5)], "a change where there is one sets it");
    let e = remove_tempo(&mut s, 18.0).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("112 BPM from 5.3", "111 BPM from bar 5"));
    assert!(matches!(remove_tempo(&mut s, 17.0), Err(EditError::NotFound(_))));
}

// FMT-13: a tempo outside 20 to 999 BPM is refused with the reader's own words, and nothing is
// written; so is a beat that isn't one.
#[test]
fn a_tempo_out_of_range_is_refused() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = tree(&f);
    assert!(matches!(tempo(&mut s, 0.0, f64::NAN), Err(EditError::Rejected(_))));
    for bpm in [19.9, 1000.0, 0.0, -120.0] {
        match tempo(&mut s, 0.0, bpm) {
            Err(EditError::Rejected(why)) => assert!(why.contains("is outside 20 to 999"), "{bpm}: {why}"),
            other => panic!("{bpm}: {other:?}"),
        }
    }
    assert!(matches!(tempo_change(&mut s, 8.0, 1200.0), Err(EditError::Rejected(_))));
    assert!(matches!(tempo_change(&mut s, f64::INFINITY, 120.0), Err(EditError::Rejected(_))));
    for bpm in [20.0, 999.0] {
        tempo(&mut s, 0.0, bpm).unwrap();
    }
    assert_eq!(changed(&before, &tree(&f)), vec![FILE.to_string()]);
    assert!(f.read(FILE).contains("\"bpm\": 999"));
}

// FMT-13's bar-line rule: a new time signature lands on the bar line at or before where it was
// asked for, so it is never refused for being off one; and a change before it moves it back onto a
// bar line of the new meter, or is refused when it would land on the change before.
#[test]
fn a_meter_change_lands_on_a_bar_line() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let e = meter_change(&mut s, 18.5, 7, 8).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("4/4", "7/8 from bar 5"));
    let meters = |s: &Session| s.accepted().project.arrangement.meter.iter().map(|m| (m.beat, m.beats, m.unit)).collect::<Vec<_>>();
    assert_eq!(meters(&s), vec![(0.0, 4, 4), (16.0, 7, 8)]);
    // Three bars of 7/8 later (bar 8), one bar of 3/4 there.
    let e = meter_change(&mut s, 16.0 + 3.0 * 3.5 + 1.0, 3, 4).unwrap();
    assert_eq!(e.after, "3/4 from bar 8");
    assert_eq!(meters(&s), vec![(0.0, 4, 4), (16.0, 7, 8), (26.5, 3, 4)]);
    // The song's own becomes 3/4: the 7/8 goes back to beat 15, the bar line before 16, and the
    // 3/4 after it moves with the 7/8's bars.
    let e = meter(&mut s, 2.0, 3, 4).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("4/4", "3/4"));
    assert_eq!(meters(&s), vec![(0.0, 3, 4), (15.0, 7, 8), (25.5, 3, 4)]);
    assert_eq!(s.accepted().project.arrangement.bar_at(15.0), (5, 0.0), "still bar 6 of the song's");
    // The meter at the playhead: in the 7/8.
    let e = meter(&mut s, 20.0, 5, 8).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("7/8 from bar 6", "5/8 from bar 6"));
    // Bars of 4/1 from the start are 16 beats long: the change at beat 15 would move back to beat
    // 0, onto the song's own, so it is refused in words.
    match meter(&mut s, 0.0, 4, 1) {
        Err(EditError::Rejected(why)) => assert!(why.contains("would land on the 4/1 before it"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(meter(&mut s, 0.0, 65, 4), Err(EditError::Rejected(_))));
    assert!(matches!(meter(&mut s, 0.0, 4, 3), Err(EditError::Rejected(_))));
    let e = remove_meter(&mut s, 15.0).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("5/8 from bar 6", "3/4"));
    assert_eq!(meters(&s), vec![(0.0, 3, 4), (24.0, 3, 4)], "the 3/4 after it lands on the song's bar line at or before beat 25");
}

// The first tempo and the first time signature are the song's own: changed, never removed.
#[test]
fn the_first_tempo_entry_stays() {
    let f = fixture("examples/first-song");
    f.write(FILE, r#"{"tempo": [{"beat": 0, "bpm": 124}], "meter": [{"beat": 0, "beats": 4, "unit": 4}, {"beat": 16, "beats": 3, "unit": 4}]}"#);
    let mut s = f.open();
    let before = tree(&f);
    for e in [remove_tempo(&mut s, 0.0), remove_meter(&mut s, 0.0)] {
        match e {
            Err(EditError::Rejected(why)) => assert!(why.contains("can't be removed: change it instead"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(changed(&before, &tree(&f)), Vec::<String>::new());
    // An arrangement with no meter written has 4/4: setting it writes nothing.
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let n = s.accepted().number;
    assert_eq!(meter(&mut s, 0.0, 4, 4).unwrap().revision, n);
    assert!(f.read(FILE).contains("\"meter\": []"));
}

// WRITE-21: each edit is one undo step, and undo puts back the arrangement's bytes as they were,
// however they were written (here on one line, not in the canonical form).
#[test]
fn undo_restores_the_arrangement_bytes() {
    let f = fixture("examples/first-song");
    f.write(FILE, TWO_TEMPOS);
    let mut s = f.open();
    tempo(&mut s, 0.0, 90.0).unwrap();
    meter_change(&mut s, 8.0, 6, 8).unwrap();
    tempo_change(&mut s, 4.0, 140.0).unwrap();
    assert_ne!(f.read(FILE), TWO_TEMPOS);
    for _ in 0..3 {
        crate::undo(&mut s).unwrap();
    }
    assert_eq!(f.read(FILE), TWO_TEMPOS, "byte for byte");
    crate::redo(&mut s).unwrap();
    assert_eq!(s.accepted().project.arrangement.tempo[0].bpm, 90.0);
}

// WRITE-6, WRITE-10: the song's title is its manifest's `[project] name`, rewritten in place as a
// TOML basic string; the rest of the file (its comments) stays, the graph is Identical, and the
// folder keeps its name. An empty one is refused in the words the field shows.
#[test]
fn a_title_edit_is_identical() {
    let f = fixture("examples/first-song");
    let manifest = "# The demo.\n[project]\nname = \"first-song\"   # its label\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.1\"\n";
    f.write("music2.toml", manifest);
    let mut s = f.open();
    let before = tree(&f);
    let e = title(&mut s, "  Night \"Drive\" \\ ☕ ").unwrap();
    assert_eq!((e.file.as_str(), e.before.as_str(), e.after.as_str()), ("music2.toml", "first-song", "Night \"Drive\" \\ ☕"));
    assert_eq!(f.read("music2.toml"), manifest.replace("\"first-song\"", r#""Night \"Drive\" \\ ☕""#));
    assert_eq!(changed(&before, &tree(&f)), vec!["music2.toml".to_string()]);
    assert_eq!(s.accepted().project.manifest.project.name, "Night \"Drive\" \\ ☕");
    for bad in ["", "   ", "two\nlines"] {
        assert!(matches!(title(&mut s, bad), Err(EditError::Rejected(_))), "{bad:?}");
    }
    assert!(matches!(title(&mut s, " "), Err(EditError::Rejected(w)) if w == "A song needs a name"));
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read("music2.toml"), manifest);
    // Written however TOML allows: a literal string is replaced by a basic one.
    let f = fixture("templates/empty");
    f.write("music2.toml", "[project]\nname = 'Empty'\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.1\"\n");
    let mut s = f.open();
    title(&mut s, "Sketch").unwrap();
    assert!(f.read("music2.toml").starts_with("[project]\nname = \"Sketch\"\n"));
}
