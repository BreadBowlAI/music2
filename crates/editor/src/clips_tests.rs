//! Clip gestures by id, on the first song's notes and automation lanes and the resample
//! example's audio lane: ids (FMT-12), kinds, trims, placing, and one Tx each that undo takes
//! back.

use m2_session::Session;
use m2_song::{AbsNote, ClipId, LaneAddr, LaneData};

use super::*;
use crate::testing::{Fixture, fixture};

fn addr(lane: &str) -> LaneAddr {
    let (m, l) = lane.split_once('/').unwrap();
    LaneAddr::new(m, l).unwrap()
}

/// The clip `id` of `lane` (`kick/hits`).
fn by_id(lane: &str, id: &str) -> ClipRef {
    ClipRef { lane: addr(lane), id: Some(id.into()), at: None, sha: None }
}

/// The `rank`th clip of `lane` that starts at `start`.
fn at(lane: &str, start: f64, rank: u32) -> ClipRef {
    ClipRef { lane: addr(lane), id: None, at: Some((start, rank)), sha: None }
}

fn data(s: &Session, lane: &str) -> Option<LaneData> {
    let a = addr(lane);
    s.accepted().compiled.data.lane(a.module.as_str(), a.lane.as_str()).map(|d| (**d).clone())
}

/// Each clip's id and span, in canonical order.
fn clips(s: &Session, lane: &str) -> Vec<(Option<String>, f64, f64)> {
    let Some(d) = data(s, lane) else { return vec![] };
    let spans: Vec<(f64, f64)> = match &d {
        LaneData::Notes(c) => c.iter().map(|c| (c.start, c.length)).collect(),
        LaneData::Control { clips, .. } => clips.iter().map(|c| (c.start, c.length)).collect(),
        LaneData::Audio(c) => c.iter().map(|c| (c.start, c.length)).collect(),
    };
    d.ids().into_iter().zip(spans).map(|(id, (s, l))| (id.map(str::to_string), s, l)).collect()
}

fn notes(s: &Session, lane: &str) -> Vec<AbsNote> {
    let a = addr(lane);
    s.accepted().compiled.data.notes(a.module.as_str(), a.lane.as_str())
}

fn go(s: &mut Session, g: Gesture) -> ClipsEdited {
    edit(s, &g).unwrap_or_else(|e| panic!("{g:?}: {e}"))
}

fn refused(s: &mut Session, g: Gesture) -> String {
    match edit(s, &g) {
        Err(e) => e.to_string(),
        Ok(done) => panic!("{g:?} was accepted: {done:?}"),
    }
}

/// The lines of `after` that `before` doesn't have.
fn changed_lines(before: &str, after: &str) -> Vec<String> {
    after.lines().filter(|l| !before.lines().any(|b| b == *l)).map(str::to_string).collect()
}

// FMT-12: a moved clip keeps its id; only its line changes; undo puts every byte back.
#[test]
fn a_move_keeps_its_id() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = f.read("data/kick/hits.json");
    let done = go(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: 32.0, to: None, copy: false });
    assert_eq!(clips(&s, "kick/hits"), [(Some("my53vvkc".into()), 32.0, 32.0)]);
    assert!(done.made.is_empty(), "nothing new to select: {:?}", done.made);
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("clip at bar 1 (8 bars)", "clip at bar 9 (8 bars)"));
    let after = f.read("data/kick/hits.json");
    assert_eq!(changed_lines(&before, &after), ["  {\"id\": \"my53vvkc\", \"start\": 32, \"length\": 32, \"notes\": ["]);
    // Onto another lane of its kind, it keeps its id there too.
    let done = go(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: -32.0, to: Some(addr("hats/hits")), copy: false });
    assert!(clips(&s, "kick/hits").is_empty());
    assert!(clips(&s, "hats/hits").contains(&(Some("my53vvkc".into()), 0.0, 32.0)), "{:?}", clips(&s, "hats/hits"));
    assert_eq!(done.made, [(addr("hats/hits"), "my53vvkc".to_string())], "moved to another lane: the app selects it there");
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(f.read("data/kick/hits.json"), before);
}

// FMT-12: a clip moved onto a lane that has its id gets a fresh one.
#[test]
fn a_move_onto_a_taken_id_mints_one() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let same = serde_json::json!({"id": "my53vvkc", "start": 64, "length": 4, "notes": []});
    go(&mut s, Gesture::Set { lane: addr("hats/hits"), put: vec![same], delete: vec![] });
    let done = go(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: 0.0, to: Some(addr("hats/hits")), copy: false });
    let [(lane, id)] = &done.made[..] else { panic!("{:?}", done.made) };
    assert!(lane == &addr("hats/hits") && id != "my53vvkc" && ClipId::valid(id), "{id}");
    let hats: Vec<Option<String>> = clips(&s, "hats/hits").into_iter().map(|c| c.0).collect();
    assert_eq!(hats.len(), 3);
    assert!(hats.contains(&Some(id.clone())) && hats.contains(&Some("my53vvkc".into())) && hats.contains(&Some("92gr2959".into())));
}

// FMT-11: a copy, a duplicate and a paste each get a fresh id, never one the lane has.
#[test]
fn every_copy_mints() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let copied = go(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: 64.0, to: None, copy: true });
    let duplicated = go(&mut s, Gesture::Duplicate { clips: vec![by_id("kick/hits", "my53vvkc")], range: None, grid: None });
    let clip = data(&s, "kick/hits").unwrap().clip_value("my53vvkc").unwrap();
    let pasted = go(&mut s, Gesture::Paste { clips: vec![Copied { from: addr("kick/hits"), clip }], to: None, at: 128.0 });
    let ids: Vec<String> = clips(&s, "kick/hits").into_iter().map(|c| c.0.unwrap()).collect();
    assert_eq!(ids.len(), 4, "{ids:?}");
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 4, "every id once: {ids:?}");
    for made in [copied.made, duplicated.made, pasted.made] {
        let [(lane, id)] = &made[..] else { panic!("{made:?}") };
        assert!(lane == &addr("kick/hits") && id != "my53vvkc" && ClipId::valid(id) && ids.contains(id), "{id}");
    }
    // The copies play what the original plays, later.
    let n = notes(&s, "kick/hits");
    let first: Vec<AbsNote> = n.iter().filter(|x| x.start < 32.0).copied().collect();
    for (from, to) in [(32.0, 64.0), (64.0, 96.0), (128.0, 160.0)] {
        let later: Vec<AbsNote> = n.iter().filter(|x| x.start >= from && x.start < to).map(|x| AbsNote { start: x.start - from, end: x.end - from, ..*x }).collect();
        assert_eq!(later, first, "the copy at {from}");
    }
}

// A left trim moves the clip's start; every note keeps its place in time, and the ones now before
// the clip stay in the file, silent. Dragging the edge back brings them back, byte for byte.
#[test]
fn a_left_trim_keeps_notes_in_place_and_trimming_back_brings_them_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (before, heard) = (f.read("data/bass/notes.json"), notes(&s, "bass/notes"));
    let LaneData::Notes(c) = data(&s, "bass/notes").unwrap() else { unreachable!() };
    let written = c[0].notes.len();
    go(&mut s, Gesture::Resize { clip: by_id("bass/notes", "db9f9t5s"), start: 4.0, length: 28.0 });
    assert_eq!(clips(&s, "bass/notes"), [(Some("db9f9t5s".into()), 4.0, 28.0)]);
    let cut: Vec<AbsNote> = heard.iter().filter(|n| n.start >= 4.0).copied().collect();
    assert_eq!(notes(&s, "bass/notes"), cut, "the notes after the edge play where they did");
    let LaneData::Notes(c) = data(&s, "bass/notes").unwrap() else { unreachable!() };
    assert_eq!(c[0].notes.len(), written, "every note is still in the file");
    assert_eq!((c[0].notes[0].start, c[0].notes[0].pitch), (-4.0, 33.0), "the first now starts before its clip");
    go(&mut s, Gesture::Resize { clip: by_id("bass/notes", "db9f9t5s"), start: 0.0, length: 32.0 });
    assert_eq!(f.read("data/bass/notes.json"), before);
    // The right edge: only the length.
    go(&mut s, Gesture::Resize { clip: by_id("bass/notes", "db9f9t5s"), start: 0.0, length: 16.0 });
    assert_eq!(notes(&s, "bass/notes"), heard.iter().filter(|n| n.start < 16.0).copied().collect::<Vec<_>>());
    let short = refused(&mut s, Gesture::Resize { clip: by_id("bass/notes", "db9f9t5s"), start: 0.0, length: 0.0 });
    assert!(short.contains("1/64 of a beat"), "{short}");
}

// An audio clip's left edge moves its offset into the asset by the seconds the edge moved, and
// stops at the asset's start. A pitched or reversed clip's left edge doesn't move (A8).
#[test]
fn an_audio_left_trim_moves_its_offset() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    let before = f.read("data/chops/clips.json");
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "a3s9vpxb"), start: 1.0, length: 1.0 });
    let LaneData::Audio(c) = data(&s, "chops/clips").unwrap() else { unreachable!() };
    assert_eq!((c[0].start, c[0].length, c[0].offset), (1.0, 1.0, 0.5), "120 bpm: half a second a beat");
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "a3s9vpxb"), start: 0.0, length: 2.0 });
    assert_eq!(f.read("data/chops/clips.json"), before);
    // At 0.75 s into the asset: 1.5 beats back is its start, and 2 is past it.
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "84f0shn6"), start: 8.5, length: 1.75 });
    let past = refused(&mut s, Gesture::Resize { clip: by_id("chops/clips", "84f0shn6"), start: 8.0, length: 2.25 });
    assert!(past.contains("start of its audio"), "{past}");
    for (id, start, says) in [("x5175mvv", 4.5, "pitch"), ("j6xncyg7", 1.5, "reversed")] {
        let why = refused(&mut s, Gesture::Resize { clip: by_id("chops/clips", id), start, length: 1.0 });
        assert!(why.contains(says), "{id}: {why}");
    }
}

// A new clip is a bar long at the meter there: empty notes, or automation holding what the lane
// plays at its start. An audio lane takes no empty clip.
#[test]
fn a_new_clip_is_a_bar_long_and_automation_holds_the_lanes_value() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let done = go(&mut s, Gesture::Create { lane: addr("bass/notes"), start: 32.0, length: None });
    let [(_, id)] = &done.made[..] else { panic!("{:?}", done.made) };
    assert!(clips(&s, "bass/notes").contains(&(Some(id.clone()), 32.0, 4.0)));
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("no clip", "clip at bar 9 (1 bar)"));
    // The cutoff's clip rises from 300 to 2200 over beats 0 to 16; after it, the fallback (500).
    for (start, value) in [(8.0, 1250.0), (40.0, 500.0)] {
        let done = go(&mut s, Gesture::Create { lane: addr("bass/cutoff"), start, length: Some(2.0) });
        let id = &done.made[0].1;
        let LaneData::Control { clips, .. } = data(&s, "bass/cutoff").unwrap() else { unreachable!() };
        let c = clips.iter().find(|c| c.id.as_ref() == Some(id)).unwrap();
        assert_eq!((c.start, c.length, &c.points[..]), (start, 2.0, &[(0.0, value), (2.0, value)][..]));
    }
    let audio = fixture("examples/resample");
    let mut a = audio.open();
    let why = refused(&mut a, Gesture::Create { lane: addr("chops/clips"), start: 32.0, length: None });
    assert!(why.contains("audio"), "{why}");
}

// Duplicate lands right after the selection: a span of a bar or more rounds up to whole bars, so
// a verse ending on beat 4 of bar 8 lands on bar 9 and a fill into bar 9 on bar 10; a shorter one
// rounds up to the grid, or not at all with snap off.
#[test]
fn a_duplicated_section_lands_on_the_next_bar() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let kick = || by_id("kick/hits", "my53vvkc");
    let starts = |s: &Session| clips(s, "kick/hits").into_iter().map(|c| c.1).collect::<Vec<_>>();
    go(&mut s, Gesture::Duplicate { clips: vec![kick()], range: None, grid: None });
    assert_eq!(starts(&s), [0.0, 32.0], "8 bars after 8 bars");
    s.undo().unwrap();
    for (length, grid, lands) in [(31.0, None, 32.0), (33.0, None, 36.0), (1.3, Some(0.25), 1.5), (1.3, None, 1.3)] {
        go(&mut s, Gesture::Resize { clip: kick(), start: 0.0, length });
        go(&mut s, Gesture::Duplicate { clips: vec![kick()], range: None, grid });
        assert_eq!(starts(&s), [0.0, lands], "{length} beats, grid {grid:?}");
        s.undo().unwrap();
        s.undo().unwrap();
    }
    // Two lanes at once: one span, so they stay together.
    go(&mut s, Gesture::Resize { clip: by_id("hats/hits", "92gr2959"), start: 4.0, length: 4.0 });
    go(&mut s, Gesture::Duplicate { clips: vec![kick(), by_id("hats/hits", "92gr2959")], range: None, grid: None });
    assert_eq!(starts(&s), [0.0, 32.0]);
    assert_eq!(clips(&s, "hats/hits").into_iter().map(|c| c.1).collect::<Vec<_>>(), [4.0, 36.0]);
}

// A marquee's own range is the span: its empty bars are kept.
#[test]
fn a_marquee_duplicate_keeps_its_empty_bars() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    go(&mut s, Gesture::Duplicate { clips: vec![by_id("kick/hits", "my53vvkc")], range: Some((0.0, 64.0)), grid: None });
    assert_eq!(clips(&s, "kick/hits").into_iter().map(|c| c.1).collect::<Vec<_>>(), [0.0, 64.0]);
}

// Pasted clips keep their offsets from the earliest, each with a fresh id: on the lane named when
// it fits them all, else on the lanes they came from.
#[test]
fn a_paste_mints_and_keeps_offsets() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    go(&mut s, Gesture::Resize { clip: by_id("hats/hits", "92gr2959"), start: 4.0, length: 4.0 });
    let copy = |s: &Session, lane: &str, id: &str| Copied { from: addr(lane), clip: data(s, lane).unwrap().clip_value(id).unwrap() };
    let copied = vec![copy(&s, "kick/hits", "my53vvkc"), copy(&s, "hats/hits", "92gr2959")];
    let done = go(&mut s, Gesture::Paste { clips: copied.clone(), to: None, at: 64.0 });
    assert_eq!(done.made.len(), 2);
    assert!(clips(&s, "kick/hits").iter().any(|c| c.1 == 64.0) && clips(&s, "hats/hits").iter().any(|c| c.1 == 68.0));
    let done = go(&mut s, Gesture::Paste { clips: copied, to: Some(addr("keys/chords")), at: 128.0 });
    let keys: Vec<(f64, f64)> = clips(&s, "keys/chords").into_iter().map(|c| (c.1, c.2)).collect();
    assert_eq!(keys, [(0.0, 32.0), (128.0, 32.0), (132.0, 4.0)]);
    assert!(done.made.iter().all(|(l, id)| l == &addr("keys/chords") && ClipId::valid(id)));
}

// Notes, automation and audio clips never cross, and automation crosses only within its unit;
// each refusal says why and writes nothing.
#[test]
fn kinds_and_units_never_mix() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let kick = || vec![by_id("kick/hits", "my53vvkc")];
    let why = refused(&mut s, Gesture::Move { clips: kick(), by: 0.0, to: Some(addr("bass/cutoff")), copy: false });
    assert!(why.contains("A notes clip can't go on an automation lane"), "{why}");
    let cutoff = vec![by_id("bass/cutoff", "adp0sw76")];
    let why = refused(&mut s, Gesture::Move { clips: cutoff.clone(), by: 0.0, to: Some(addr("bass/notes")), copy: true });
    assert!(why.contains("An automation clip can't go on a notes lane"), "{why}");
    let why = refused(&mut s, Gesture::Move { clips: cutoff, by: 0.0, to: Some(addr("bass/drop")), copy: true });
    assert!(why.contains("hz") && why.contains("unit01") && why.contains("can't share clips"), "{why}");
    let clip = data(&s, "bass/cutoff").unwrap().clip_value("adp0sw76").unwrap();
    let why = refused(&mut s, Gesture::Paste { clips: vec![Copied { from: addr("bass/cutoff"), clip }], to: Some(addr("kick/hits")), at: 0.0 });
    assert!(why.contains("An automation clip can't go on a notes lane"), "{why}");
    let why = refused(&mut s, Gesture::Move { clips: kick(), by: -1.0, to: None, copy: false });
    assert!(why.contains("before the song starts"), "{why}");
    let why = refused(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "nothere")], by: 1.0, to: None, copy: false });
    assert!(why.contains("nothere"), "{why}");
    assert_eq!((s.accepted().number, s.can_undo()), (1, false), "nothing was written");
}

// FMT-12: a lane whose clips have no ids is written whole, and every clip of it gets one; the
// gesture names its clip by where it starts.
#[test]
fn touching_a_lane_gives_its_anonymous_clips_ids() {
    let f = fixture("examples/first-song");
    let text = f.read("data/kick/hits.json").replacen("\"id\": \"my53vvkc\", ", "", 1);
    let second = text.replacen("\n]}", ",\n  {\"start\": 32, \"length\": 4, \"notes\": []}\n]}", 1);
    f.write("data/kick/hits.json", &second);
    let mut s = f.open();
    assert_eq!(clips(&s, "kick/hits").iter().map(|c| c.0.clone()).collect::<Vec<_>>(), [None, None]);
    go(&mut s, Gesture::Move { clips: vec![at("kick/hits", 32.0, 0)], by: 32.0, to: None, copy: false });
    let now = clips(&s, "kick/hits");
    assert_eq!(now.iter().map(|c| c.1).collect::<Vec<_>>(), [0.0, 64.0]);
    assert!(now.iter().all(|c| c.0.as_deref().is_some_and(ClipId::valid)), "{now:?}");
    assert!(data(&s, "kick/hits").unwrap().addressable());
    let why = refused(&mut s, Gesture::Delete { clips: vec![at("kick/hits", 32.0, 0)] });
    assert!(why.contains("bar 9"), "no clip starts there now: {why}");
}

// Delete takes clips out; the generic form puts and deletes by id, minting for a clip with none.
#[test]
fn delete_and_set() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = f.read("data/hats/hits.json");
    go(&mut s, Gesture::Delete { clips: vec![by_id("hats/hits", "92gr2959")] });
    assert_eq!(f.read("data/hats/hits.json"), "{\"clips\": []}\n", "the lane keeps its file");
    s.undo().unwrap();
    assert_eq!(f.read("data/hats/hits.json"), before);
    let put = vec![serde_json::json!({"start": 8, "length": 2, "notes": [{"pitch": "C4", "start": 0, "length": 1}]})];
    let done = go(&mut s, Gesture::Set { lane: addr("hats/hits"), put, delete: vec!["92gr2959".into()] });
    let [(_, id)] = &done.made[..] else { panic!("{:?}", done.made) };
    assert_eq!(clips(&s, "hats/hits"), [(Some(id.clone()), 8.0, 2.0)]);
    let bad = vec![serde_json::json!({"start": 8, "length": 2, "points": [[0, 1]]})];
    let why = refused(&mut s, Gesture::Set { lane: addr("hats/hits"), put: bad, delete: vec![] });
    assert!(why.contains("automation"), "{why}");
}

// The edit is pure: the same revision, gesture and ids make the same lanes.
#[test]
fn apply_is_pure() {
    let f = fixture("examples/first-song");
    let s = f.open();
    let g = Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc"), by_id("hats/hits", "92gr2959")], by: 4.0, to: None, copy: true };
    let run = || {
        let mut n = 0;
        apply(s.accepted(), &g, &mut |lane: &LaneAddr| {
            n += 1;
            ClipId::derive("same", lane, n)
        })
        .unwrap()
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b);
    assert_eq!(a.len(), 2);
}

// A lane with no file yet gets one; undo takes it away.
#[test]
fn a_first_clip_makes_the_lane_file() {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { kick } from \"std/instruments\";\n\
         const k = track(\"kick\", ({ lane }) => kick(lane.notes(\"hits\")));\nexport default song(() => k.out);\n",
    );
    let mut s = f.open();
    go(&mut s, Gesture::Create { lane: addr("kick/hits"), start: 0.0, length: None });
    assert!(f.0.join("data/kick/hits.json").exists());
    s.undo().unwrap();
    assert!(!f.0.join("data/kick/hits.json").exists());
}

// Places are bar.beat.sixteenth, counted from 1 in the song's meter; beats are quarter notes.
#[test]
fn places_are_bar_beat_and_sixteenth() {
    let common = m2_song::Arrangement::default();
    for (text, beats) in [("1", 0.0), ("9", 32.0), ("9.3", 34.0), ("9.3.1", 34.0), ("9.3.2", 34.25), ("2.4.4", 7.75)] {
        assert_eq!(position(&common, text), Ok(beats), "{text}");
    }
    // 3/4 for two bars, then 4/4.
    let mixed = m2_song::Arrangement {
        tempo: vec![m2_song::Tempo { beat: 0.0, bpm: 120.0 }],
        meter: vec![m2_song::Meter { beat: 0.0, beats: 3, unit: 4 }, m2_song::Meter { beat: 6.0, beats: 4, unit: 4 }],
    };
    assert_eq!(position(&mixed, "2.3"), Ok(5.0));
    assert_eq!(position(&mixed, "4.4"), Ok(13.0));
    for bad in ["0", "9.0", "9.3.5", "9.3.1.1", "nine", "", "1.4"] {
        let why = position(&mixed, bad).unwrap_err();
        assert!(why.contains(bad) || why.contains("bar.beat"), "{bad}: {why}");
    }
}

// WRITE-3: a gesture is planned on the accepted lane; an outside edit to another clip of the lane,
// not taken in yet, merges with it.
#[test]
fn an_outside_edit_to_another_clip_merges() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let other = serde_json::json!({"id": "other", "start": 64, "length": 4, "notes": [{"pitch": 36, "start": 0, "length": 1, "velocity": 0.8}]});
    go(&mut s, Gesture::Set { lane: addr("kick/hits"), put: vec![other], delete: vec![] });
    let outside = f.read("data/kick/hits.json").replacen("\"start\": 64", "\"start\": 72", 1);
    f.write("data/kick/hits.json", &outside);
    go(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: 4.0, to: None, copy: false });
    let now = f.read("data/kick/hits.json");
    assert!(now.contains("\"id\": \"my53vvkc\", \"start\": 4") && now.contains("\"id\": \"other\", \"start\": 72"), "{now}");
}
