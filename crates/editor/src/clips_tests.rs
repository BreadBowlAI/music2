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
        LaneData::Steps { clips, .. } => clips.iter().map(|c| (c.start, c.length)).collect(),
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
    let done =
        go(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: -32.0, to: Some(addr("hats/hits")), copy: false });
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
    let duplicated = go(&mut s, Gesture::Duplicate { clips: vec![by_id("kick/hits", "my53vvkc")], range: None, grid: None, times: None });
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
        let later: Vec<AbsNote> = n
            .iter()
            .filter(|x| x.start >= from && x.start < to)
            .map(|x| AbsNote { start: x.start - from, end: x.end - from, ..*x })
            .collect();
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
// stops at the asset's start. A pitched clip's left edge doesn't move (A8); a reversed clip's left
// edge keeps its offset (A3).
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
    let why = refused(&mut s, Gesture::Resize { clip: by_id("chops/clips", "x5175mvv"), start: 4.5, length: 1.0 });
    assert!(why.contains("pitch"), "{why}");
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
    go(&mut s, Gesture::Duplicate { clips: vec![kick()], range: None, grid: None, times: None });
    assert_eq!(starts(&s), [0.0, 32.0], "8 bars after 8 bars");
    s.undo().unwrap();
    for (length, grid, lands) in [(31.0, None, 32.0), (33.0, None, 36.0), (1.3, Some(0.25), 1.5), (1.3, None, 1.3)] {
        go(&mut s, Gesture::Resize { clip: kick(), start: 0.0, length });
        go(&mut s, Gesture::Duplicate { clips: vec![kick()], range: None, grid, times: None });
        assert_eq!(starts(&s), [0.0, lands], "{length} beats, grid {grid:?}");
        s.undo().unwrap();
        s.undo().unwrap();
    }
    // Two lanes at once: one span, so they stay together.
    go(&mut s, Gesture::Resize { clip: by_id("hats/hits", "92gr2959"), start: 4.0, length: 4.0 });
    go(&mut s, Gesture::Duplicate { clips: vec![kick(), by_id("hats/hits", "92gr2959")], range: None, grid: None, times: None });
    assert_eq!(starts(&s), [0.0, 32.0]);
    assert_eq!(clips(&s, "hats/hits").into_iter().map(|c| c.1).collect::<Vec<_>>(), [4.0, 36.0]);
}

// A marquee's own range is the span: its empty bars are kept.
#[test]
fn a_marquee_duplicate_keeps_its_empty_bars() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    go(&mut s, Gesture::Duplicate { clips: vec![by_id("kick/hits", "my53vvkc")], range: Some((0.0, 64.0)), grid: None, times: None });
    assert_eq!(clips(&s, "kick/hits").into_iter().map(|c| c.1).collect::<Vec<_>>(), [0.0, 64.0]);
}

// Repeat… (Q12): three copies, one after another, each with a fresh id, as one undo step.
#[test]
fn duplicate_three_times_puts_three_copies_after() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let bass = by_id("bass/notes", "db9f9t5s");
    go(&mut s, Gesture::Resize { clip: bass.clone(), start: 0.0, length: 4.0 });
    let done = go(&mut s, Gesture::Duplicate { clips: vec![bass], range: None, grid: None, times: Some(3) });
    let now = clips(&s, "bass/notes");
    assert_eq!(now.iter().map(|c| (c.1, c.2)).collect::<Vec<_>>(), [(0.0, 4.0), (4.0, 4.0), (8.0, 4.0), (12.0, 4.0)]);
    let ids: std::collections::BTreeSet<_> = now.iter().map(|c| c.0.clone().unwrap()).collect();
    assert_eq!(ids.len(), 4, "each copy has an id of its own: {now:?}");
    assert_eq!(done.made.len(), 3);
    // The copies play the first one's notes.
    let n = notes(&s, "bass/notes");
    let first: Vec<_> = n.iter().filter(|x| x.start < 4.0).map(|x| (x.start, x.pitch)).collect();
    let third: Vec<_> = n.iter().filter(|x| (8.0..12.0).contains(&x.start)).map(|x| (x.start - 8.0, x.pitch)).collect();
    assert_eq!(first, third);
    s.undo().unwrap();
    assert_eq!(clips(&s, "bass/notes").len(), 1, "one undo step takes all three back");
}

// A marquee's bars repeat as a block, its empty bars kept.
#[test]
fn repeat_of_a_marquee_range_keeps_its_bars() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let kick = by_id("kick/hits", "my53vvkc");
    go(&mut s, Gesture::Resize { clip: kick.clone(), start: 0.0, length: 4.0 });
    go(&mut s, Gesture::Duplicate { clips: vec![kick], range: Some((0.0, 8.0)), grid: None, times: Some(2) });
    assert_eq!(clips(&s, "kick/hits").into_iter().map(|c| c.1).collect::<Vec<_>>(), [0.0, 8.0, 16.0]);
}

#[test]
fn sixty_five_times_is_refused() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let kick = || by_id("kick/hits", "my53vvkc");
    for times in [0, 65] {
        let e = refused(&mut s, Gesture::Duplicate { clips: vec![kick()], range: None, grid: None, times: Some(times) });
        assert!(e.contains("Repeat 1 to 64 times"), "{e}");
    }
    go(&mut s, Gesture::Resize { clip: kick(), start: 0.0, length: 1.0 });
    go(&mut s, Gesture::Duplicate { clips: vec![kick()], range: None, grid: None, times: Some(64) });
    assert_eq!(clips(&s, "kick/hits").len(), 65);
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
    let why =
        refused(&mut s, Gesture::Paste { clips: vec![Copied { from: addr("bass/cutoff"), clip }], to: Some(addr("kick/hits")), at: 0.0 });
    assert!(why.contains("An automation clip can't go on a notes lane"), "{why}");
    let why = refused(&mut s, Gesture::Move { clips: kick(), by: -1.0, to: None, copy: false });
    assert!(why.contains("before the song starts"), "{why}");
    // What isn't there is said whole, as the user reads it.
    for (clip, says) in [
        ("kick/hits:nothere", "Kick's hits lane has no clip `nothere` now."),
        ("nosuch/notes:abc", "There's no track `nosuch`."),
        ("bass/nolane:abc", "Bass has no lane `nolane`."),
    ] {
        let (lane, id) = clip.split_once(':').unwrap();
        assert_eq!(refused(&mut s, Gesture::Move { clips: vec![by_id(lane, id)], by: 1.0, to: None, copy: false }), says);
    }
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

/// A notes clip `id` from `start` for `length`, with notes `(pitch, start, length)`.
fn notes_clip(id: &str, start: f64, length: f64, notes: &[(f64, f64, f64)]) -> serde_json::Value {
    let notes: Vec<_> = notes.iter().map(|(p, s, l)| serde_json::json!({"pitch": p, "start": s, "length": l, "velocity": 0.8})).collect();
    serde_json::json!({"id": id, "start": start, "length": length, "notes": notes})
}

/// A notes clip as the tests compare it: its id, start and length, and its notes `(pitch, start,
/// length)`.
type Shown = (String, f64, f64, Vec<(f64, f64, f64)>);

/// The notes clips of `lane`, in order.
fn notes_by_id(s: &Session, lane: &str) -> Vec<Shown> {
    let Some(LaneData::Notes(c)) = data(s, lane) else { panic!("{lane} isn't a notes lane") };
    c.into_iter().map(|c| (c.id.unwrap(), c.start, c.length, c.notes.iter().map(|n| (n.pitch, n.start, n.length)).collect())).collect()
}

// A split ends the clip at the split and shortens the notes that cross it, as Live does; the notes
// after it go to a new clip, in place.
#[test]
fn a_split_shortens_the_crossing_notes() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let clip = notes_clip("a", 0.0, 8.0, &[(60.0, 0.0, 3.0), (62.0, 3.0, 2.0), (64.0, 4.0, 2.0), (65.0, 7.0, 1.0)]);
    go(&mut s, Gesture::Set { lane: addr("keys/chords"), put: vec![clip], delete: vec!["6ryexzev".into()] });
    let done = go(&mut s, Gesture::Split { clips: vec![by_id("keys/chords", "a")], at: 4.0 });
    let [(_, second)] = &done.made[..] else { panic!("{:?}", done.made) };
    assert_eq!(
        notes_by_id(&s, "keys/chords"),
        [
            ("a".to_string(), 0.0, 4.0, vec![(60.0, 0.0, 3.0), (62.0, 3.0, 1.0)]),
            (second.clone(), 4.0, 4.0, vec![(64.0, 0.0, 2.0), (65.0, 3.0, 1.0)]),
        ]
    );
    // Only clips the split crosses; when none does, it says so.
    let why = refused(&mut s, Gesture::Split { clips: vec![by_id("keys/chords", "a")], at: 6.0 });
    assert!(why.contains("bar 2"), "{why}");
}

// A split automation clip plays the same curve: the value at the split ends the first and starts
// the second.
#[test]
fn a_split_control_clip_stays_continuous() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let lane = |s: &Session| s.accepted().compiled.data.control("bass", "cutoff");
    let before = lane(&s);
    go(&mut s, Gesture::Split { clips: vec![by_id("bass/cutoff", "adp0sw76")], at: 4.0 });
    let LaneData::Control { clips, .. } = data(&s, "bass/cutoff").unwrap() else { unreachable!() };
    let points: Vec<_> = clips.iter().map(|c| (c.start, c.length, c.points.clone())).collect();
    assert_eq!(points, [(0.0, 4.0, vec![(0.0, 300.0), (4.0, 775.0)]), (4.0, 12.0, vec![(0.0, 775.0), (12.0, 2200.0)])]);
    let after = lane(&s);
    for i in 0..80 {
        let beat = f64::from(i) * 0.25;
        assert_eq!(after.value_at(beat, 500.0), before.value_at(beat, 500.0), "at beat {beat}");
    }
}

// A split audio clip plays the same samples: the second half starts later in the asset. On a
// sample line, at the asset's own rate, the render is the same to the bit (A8).
#[test]
fn a_split_audio_clip_plays_the_same_samples() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    let render = |s: &Session| {
        let rev = s.accepted();
        m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(2.5).unwrap()
    };
    let sound = render(&s);
    assert!(sound.iter().any(|x| x[0] != 0.0));
    let done = go(&mut s, Gesture::Split { clips: vec![by_id("chops/clips", "a3s9vpxb")], at: 1.0 });
    let LaneData::Audio(c) = data(&s, "chops/clips").unwrap() else { unreachable!() };
    let second = c.iter().find(|c| c.id.as_deref() == Some(done.made[0].1.as_str())).unwrap();
    assert_eq!((second.start, second.length, second.offset), (1.0, 1.0, 0.5));
    assert_eq!(render(&s), sound, "the same samples");
    let why = refused(&mut s, Gesture::Split { clips: vec![by_id("chops/clips", "x5175mvv")], at: 5.5 });
    assert!(why.contains("pitch"), "{why}");
}

// Join makes one clip of a lane's notes clips, from the earliest start to the latest end, with the
// earliest's id; every note plays where it did, gaps stay empty, and a note a clip's edge hid
// stays silent.
#[test]
fn join_makes_one_clip_and_keeps_every_note_in_time() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let heard = notes(&s, "bass/notes");
    let done = go(&mut s, Gesture::Split { clips: vec![by_id("bass/notes", "db9f9t5s")], at: 16.0 });
    let second = done.made[0].1.clone();
    go(&mut s, Gesture::Join { clips: vec![by_id("bass/notes", &second), by_id("bass/notes", "db9f9t5s")] });
    assert_eq!(clips(&s, "bass/notes"), [(Some("db9f9t5s".into()), 0.0, 32.0)]);
    assert_eq!(notes(&s, "bass/notes"), heard, "the split and the join play what played");

    // Two clips with a gap, one hiding a note past its end that the joined clip would reach.
    let a = notes_clip("a", 0.0, 4.0, &[(60.0, 0.0, 1.0), (61.0, 3.5, 1.0), (62.0, 5.0, 1.0)]);
    let b = notes_clip("b", 8.0, 4.0, &[(64.0, -1.0, 2.0), (65.0, 2.0, 1.0)]);
    go(&mut s, Gesture::Set { lane: addr("keys/chords"), put: vec![a, b], delete: vec!["6ryexzev".into()] });
    let heard = notes(&s, "keys/chords");
    go(&mut s, Gesture::Join { clips: vec![by_id("keys/chords", "a"), by_id("keys/chords", "b")] });
    assert_eq!(notes(&s, "keys/chords"), heard, "every note where it played, and the hidden ones silent");
    assert_eq!(
        notes_by_id(&s, "keys/chords"),
        [("a".to_string(), 0.0, 12.0, vec![(60.0, 0.0, 1.0), (61.0, 3.5, 0.5), (64.0, 8.0, 1.0), (65.0, 10.0, 1.0)])]
    );
    let one = refused(&mut s, Gesture::Join { clips: vec![by_id("keys/chords", "a")] });
    assert!(one.contains("two"), "{one}");
}

// Automation clips don't join (the fallback would stop playing in their gaps), nor do audio clips,
// nor clips of two lanes.
#[test]
fn control_and_audio_clips_dont_join() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    go(&mut s, Gesture::Split { clips: vec![by_id("bass/cutoff", "adp0sw76")], at: 4.0 });
    let ids: Vec<ClipRef> = clips(&s, "bass/cutoff").into_iter().map(|c| by_id("bass/cutoff", &c.0.unwrap())).collect();
    let why = refused(&mut s, Gesture::Join { clips: ids });
    assert!(why.contains("Automation clips can't be joined"), "{why}");
    let why = refused(&mut s, Gesture::Join { clips: vec![by_id("kick/hits", "my53vvkc"), by_id("hats/hits", "92gr2959")] });
    assert!(why.contains("one lane"), "{why}");
    let audio = fixture("examples/resample");
    let mut a = audio.open();
    let why = refused(&mut a, Gesture::Join { clips: vec![by_id("chops/clips", "a3s9vpxb"), by_id("chops/clips", "j6xncyg7")] });
    assert!(why.contains("Audio clips can't be joined"), "{why}");
}

// Quantize moves note starts to the nearest grid line, and ends too when asked: sixteenths, and
// triplets at two thirds of that; only the notes picked, when some are.
#[test]
fn quantize_to_sixteenths_and_triplets() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let clip = notes_clip("a", 4.0, 4.0, &[(60.0, 0.1, 0.3), (62.0, 0.3, 0.2), (64.0, 1.13, 0.5)]);
    go(&mut s, Gesture::Set { lane: addr("keys/chords"), put: vec![clip], delete: vec!["6ryexzev".into()] });
    let q =
        |notes: Option<Vec<u32>>, grid: f64, ends: bool| Gesture::Quantize { clips: vec![by_id("keys/chords", "a")], notes, grid, ends };
    go(&mut s, q(None, 0.25, false));
    assert_eq!(notes_by_id(&s, "keys/chords")[0].3, [(60.0, 0.0, 0.3), (62.0, 0.25, 0.2), (64.0, 1.25, 0.5)]);
    s.undo().unwrap();
    go(&mut s, q(None, 0.25, true));
    assert_eq!(notes_by_id(&s, "keys/chords")[0].3, [(60.0, 0.0, 0.5), (62.0, 0.25, 0.25), (64.0, 1.25, 0.5)]);
    s.undo().unwrap();
    go(&mut s, q(Some(vec![2]), 1.0 / 6.0, false));
    let third = 1.0 / 6.0;
    assert_eq!(notes_by_id(&s, "keys/chords")[0].3, [(60.0, 0.1, 0.3), (62.0, 0.3, 0.2), (64.0, 7.0 * third, 0.5)]);
    let why = refused(&mut s, Gesture::Quantize { clips: vec![by_id("bass/cutoff", "adp0sw76")], notes: None, grid: 0.25, ends: false });
    assert!(why.contains("notes"), "{why}");
    assert_eq!(refused(&mut s, q(Some(vec![3]), 0.25, false)), "The clip has 3 notes, so it has no note 4.");
}

// The piano roll edits one clip's notes, and automation is drawn as one clip's points.
#[test]
fn notes_and_points_edit_one_clip() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let n = |pitch: f64, start: f64| Note { pitch, start, length: 0.5, velocity: 0.8 };
    let done =
        go(&mut s, Gesture::Notes { clip: by_id("kick/hits", "my53vvkc"), notes: vec![n(36.0, 1.0), n(36.0, 0.0)], length: Some(8.0) });
    assert_eq!(notes_by_id(&s, "kick/hits"), [("my53vvkc".into(), 0.0, 8.0, vec![(36.0, 0.0, 0.5), (36.0, 1.0, 0.5)])]);
    assert!(done.edited.after.contains("bar 1"), "{:?}", done.edited);
    go(&mut s, Gesture::Points { clip: by_id("bass/cutoff", "adp0sw76"), points: vec![(8.0, 1000.0), (0.0, 200.0)] });
    let LaneData::Control { clips, .. } = data(&s, "bass/cutoff").unwrap() else { unreachable!() };
    assert_eq!(clips[0].points, [(0.0, 200.0), (8.0, 1000.0)]);
    let why = refused(&mut s, Gesture::Points { clip: by_id("kick/hits", "my53vvkc"), points: vec![(0.0, 1.0)] });
    assert!(why.contains("automation"), "{why}");
    let why = refused(&mut s, Gesture::Notes { clip: by_id("bass/cutoff", "adp0sw76"), notes: vec![n(36.0, 0.0)], length: None });
    assert!(why.contains("notes"), "{why}");
    let bad = Note { pitch: f64::NAN, start: 0.0, length: 1.0, velocity: 0.8 };
    assert!(edit(&mut s, &Gesture::Notes { clip: by_id("kick/hits", "my53vvkc"), notes: vec![bad], length: None }).is_err());
}

/// The `sha` the timeline shows for clip `id` of `lane`.
fn sha_of(s: &Session, lane: &str, id: &str) -> String {
    sha(&data(s, lane).unwrap().lines_of(id)[0])
}

// WRITE-12: a gesture names each clip with the hash the window drew it with. A clip changed since,
// on disk or in another window, is refused in words naming it, and nothing is written; with the
// hash it has now, the gesture goes ahead.
#[test]
fn a_gesture_on_a_changed_clip_is_changed() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let seen = sha_of(&s, "kick/hits", "my53vvkc");
    let kick = |sha: &str| ClipRef { sha: Some(sha.to_string()), ..by_id("kick/hits", "my53vvkc") };
    let changed = f.read("data/kick/hits.json").replacen("\"velocity\": 0.8", "\"velocity\": 0.5", 1);
    f.write("data/kick/hits.json", &changed);
    assert!(s.refresh());
    match edit(&mut s, &Gesture::Move { clips: vec![kick(&seen)], by: 4.0, to: None, copy: false }) {
        Err(EditError::Changed(files)) => {
            let said = EditError::Changed(files.clone()).to_string();
            assert_eq!((files[0].file.as_str(), files[0].clip.as_deref()), ("data/kick/hits.json", Some("my53vvkc")));
            assert!(said.contains("the Kick clip at bar 1") && said.contains("changed in another window or by an agent"), "{said}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(f.read("data/kick/hits.json"), changed, "nothing written");
    let now = sha_of(&s, "kick/hits", "my53vvkc");
    assert_ne!(now, seen);
    go(&mut s, Gesture::Move { clips: vec![kick(&now)], by: 4.0, to: None, copy: false });
    // A clip that is gone since is changed too.
    let gone = ClipRef { sha: Some(now), ..by_id("kick/hits", "nothere") };
    assert!(matches!(edit(&mut s, &Gesture::Delete { clips: vec![gone] }), Err(EditError::Changed(_))));
}

// WRITE-3: a gesture is planned on the accepted lane; an outside edit to another clip of the lane,
// not taken in yet, merges with it.
#[test]
fn an_outside_edit_to_another_clip_merges() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    go(&mut s, Gesture::Set { lane: addr("kick/hits"), put: vec![notes_clip("other", 64.0, 4.0, &[(36.0, 0.0, 1.0)])], delete: vec![] });
    let outside = f.read("data/kick/hits.json").replacen("\"start\": 64", "\"start\": 72", 1);
    f.write("data/kick/hits.json", &outside);
    go(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: 4.0, to: None, copy: false });
    let now = f.read("data/kick/hits.json");
    assert!(now.contains("\"id\": \"my53vvkc\", \"start\": 4") && now.contains("\"id\": \"other\", \"start\": 72"), "{now}");
}

// WRITE-21: undoing a gesture after an outside change to its clip is a conflict naming the clip.
#[test]
fn undoing_after_an_outside_change_to_the_clip_conflicts() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    go(&mut s, Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: 4.0, to: None, copy: false });
    let outside = f.read("data/kick/hits.json").replacen("\"velocity\": 0.8", "\"velocity\": 0.5", 1);
    f.write("data/kick/hits.json", &outside);
    assert!(s.refresh());
    match crate::undo(&mut s) {
        Err(EditError::Changed(files)) => assert_eq!(files[0].clip.as_deref(), Some("my53vvkc")),
        other => panic!("{other:?}"),
    }
    assert_eq!(f.read("data/kick/hits.json"), outside);
}

// The timeline's clips, each with its `sha`, as the server sends them.
#[test]
fn with_sha_gives_each_clip_its_hash() {
    let f = fixture("examples/first-song");
    let s = f.open();
    let d = data(&s, "bass/notes").unwrap();
    let shown = serde_json::json!({"kind": "notes", "key": "notes", "clips": [{"id": "db9f9t5s"}]});
    let with = with_sha(shown, Some(&d));
    assert_eq!(with["clips"][0]["sha"], serde_json::json!(sha_of(&s, "bass/notes", "db9f9t5s")));
    assert_eq!(with["clips"][0]["sha"].as_str().unwrap().len(), 16);
    // Clips that don't match the data, one for one, get none.
    let odd = with_sha(serde_json::json!({"clips": [{}, {}]}), Some(&d));
    assert!(odd["clips"][0].get("sha").is_none());
}

// WRITE-22a, RENDER-26a (Q21): a lane lease previews with `apply_as` and commits with `edit_as`
// under one Tx id, so a copy gets the same id in the preview and in the file, and that Tx id is
// the commit's. Another Tx id mints another.
#[test]
fn edit_as_uses_the_ids_it_is_given() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let copy = Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: 64.0, to: None, copy: true };
    let previewed = apply_as(s.accepted(), &copy, "tx-lease1").unwrap();
    let [lane] = &previewed[..] else { panic!("{previewed:?}") };
    let done = edit_as(&mut s, &copy, "tx-lease1").unwrap();
    let want = ClipId::derive("tx-lease1", &addr("kick/hits"), 0);
    assert_eq!(done.made, vec![(addr("kick/hits"), want.clone())]);
    assert_eq!(lane.made, vec![want.clone()], "the preview's copy has the commit's id");
    assert_eq!(Some(&lane.data), data(&s, "kick/hits").as_ref(), "the preview is what the commit wrote");
    let other = apply_as(s.accepted(), &copy, "tx-lease2").unwrap();
    assert_ne!(other[0].made, vec![want]);
}

// WRITE-22a: a lane lease pins each clip its gesture names to the hash it saw, so an outside
// change to that clip refuses the gesture as changed; a hash the gesture carries is kept.
#[test]
fn pinned_gives_each_named_clip_its_hash() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let drag = Gesture::Move { clips: vec![by_id("kick/hits", "my53vvkc")], by: 4.0, to: None, copy: false };
    let pin = pinned(s.accepted(), &drag).unwrap();
    let Gesture::Move { clips, .. } = &pin else { panic!() };
    assert_eq!(clips[0].sha.as_deref(), Some(sha_of(&s, "kick/hits", "my53vvkc").as_str()));
    assert_eq!(pinned(s.accepted(), &pin).unwrap(), pin, "a hash it carries stays");
    let outside = f.read("data/kick/hits.json").replacen("\"velocity\": 0.8", "\"velocity\": 0.5", 1);
    f.write("data/kick/hits.json", &outside);
    assert!(s.refresh());
    assert!(matches!(apply_as(s.accepted(), &pin, "tx-x"), Err(EditError::Changed(_))));
    assert!(apply_as(s.accepted(), &drag, "tx-x").is_ok(), "unpinned, the changed clip moves");
}

// ---------------------------------------------------------------- audio clips (lane A's A3)

/// The resample example's audio clip `id`.
fn audio(s: &Session, id: &str) -> m2_song::AudioClip {
    let Some(LaneData::Audio(c)) = data(s, "chops/clips") else { panic!("an audio lane") };
    c.into_iter().find(|c| c.id.as_deref() == Some(id)).unwrap_or_else(|| panic!("no clip {id}"))
}

/// What an audio gesture sets.
#[derive(Default)]
struct Settings {
    gain_db: Option<f64>,
    fade_in: Option<f64>,
    fade_out: Option<f64>,
    reverse: Option<bool>,
    follow: Option<bool>,
}

fn set_on(lane: &str, id: &str, o: Settings) -> Gesture {
    let clip = by_id(lane, id);
    Gesture::Audio {
        clip,
        gain_db: o.gain_db,
        fade_in: o.fade_in,
        fade_out: o.fade_out,
        reverse: o.reverse,
        follow: o.follow,
        offset: None,
    }
}

fn set(id: &str, o: Settings) -> Gesture {
    set_on("chops/clips", id, o)
}

/// The resample example's whole song, 3 s of it, at 48 kHz.
fn rendered(s: &Session) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(3.0).unwrap()
}

/// keys-bounce.wav's length, in seconds.
fn keys_seconds(f: &Fixture) -> f64 {
    asset_seconds(&f.0.join("assets/keys-bounce.wav")).unwrap()
}

// Q10, Q11, Q12: a clip's fades, its volume and its direction are each one Tx, one undo step, and
// each changes only that.
#[test]
fn fades_gain_and_reverse_are_one_tx_each() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    let was = audio(&s, "a3s9vpxb");
    let steps = [
        set("a3s9vpxb", Settings { fade_in: Some(0.25), ..Settings::default() }),
        set("a3s9vpxb", Settings { fade_out: Some(0.4), ..Settings::default() }),
        set("a3s9vpxb", Settings { gain_db: Some(-6.0), ..Settings::default() }),
        set("a3s9vpxb", Settings { reverse: Some(true), ..Settings::default() }),
    ];
    for g in &steps {
        let before = s.accepted().number;
        go(&mut s, g.clone());
        assert_eq!(s.accepted().number, before + 1, "one revision: {g:?}");
    }
    let now = audio(&s, "a3s9vpxb");
    assert_eq!((now.fade_in, now.fade_out, now.gain_db, now.reverse), (0.25, 0.4, -6.0, true));
    assert_eq!((now.start, now.length, now.offset, &now.asset), (was.start, was.length, was.offset, &was.asset), "nothing else");
    let line = f.read("data/chops/clips.json");
    assert!(line.contains(r#""gain_db": -6, "fade_in": 0.25, "fade_out": 0.4, "reverse": true"#), "{line}");
    for _ in &steps {
        crate::undo(&mut s).unwrap();
    }
    assert_eq!(audio(&s, "a3s9vpxb"), was, "four undos take the four back");
    // Remove fades: both at once, one step.
    go(&mut s, set("a3s9vpxb", Settings { fade_in: Some(0.1), fade_out: Some(0.1), ..Settings::default() }));
    go(&mut s, set("a3s9vpxb", Settings { fade_in: Some(0.0), fade_out: Some(0.0), ..Settings::default() }));
    assert_eq!((audio(&s, "a3s9vpxb").fade_in, audio(&s, "a3s9vpxb").fade_out), (0.0, 0.0));
}

// Q10: fades are seconds, 0 or more, and together at most the clip (1 s here: 2 beats at 120 BPM).
#[test]
fn fades_never_pass_the_clip() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    go(&mut s, set("a3s9vpxb", Settings { fade_in: Some(0.6), ..Settings::default() }));
    let why = refused(&mut s, set("a3s9vpxb", Settings { fade_out: Some(0.5), ..Settings::default() }));
    assert!(why.contains("at most the clip: 1 seconds"), "{why}");
    go(&mut s, set("a3s9vpxb", Settings { fade_out: Some(0.4), ..Settings::default() }));
    for bad in [-0.1, f64::NAN, f64::INFINITY] {
        let why = refused(&mut s, set("a3s9vpxb", Settings { fade_in: Some(bad), ..Settings::default() }));
        assert!(why.contains("0 or more"), "{bad}: {why}");
    }
    assert_eq!((audio(&s, "a3s9vpxb").fade_in, audio(&s, "a3s9vpxb").fade_out), (0.6, 0.4));
}

// Q11: a clip's volume is −48 to +12 dB.
#[test]
fn gain_stays_in_its_range() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    for ok in [-48.0, 12.0, -3.5, 0.0] {
        go(&mut s, set("a3s9vpxb", Settings { gain_db: Some(ok), ..Settings::default() }));
        assert_eq!(audio(&s, "a3s9vpxb").gain_db, ok);
    }
    for bad in [-48.5, 12.1, f64::NAN] {
        let why = refused(&mut s, set("a3s9vpxb", Settings { gain_db: Some(bad), ..Settings::default() }));
        assert_eq!(why, "edit rejected: A clip's volume goes from −48 to +12 dB.", "{bad}");
    }
}

// Notes and automation clips have no volume, fades or direction to set.
#[test]
fn only_an_audio_clip_takes_audio_settings() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let why = refused(&mut s, set_on("bass/notes", "db9f9t5s", Settings { gain_db: Some(-6.0), ..Settings::default() }));
    assert!(why.contains("Only an audio clip"), "{why}");
}

/// Frames `a..b` (in seconds) of a render.
fn frames(r: &[[f32; 2]], a: f64, b: f64) -> Vec<[f32; 2]> {
    r[(a * 48_000.0).round() as usize..(b * 48_000.0).round() as usize].to_vec()
}

// Q12, D3: a reversed clip trims from either edge, what is left playing where it did, to the bit:
// its left edge keeps the offset, its right edge moves it. Clip j6xncyg7: beats 2 to 4 (1 to 2 s),
// reversed, a 0.02 s fade in.
#[test]
fn a_reversed_clip_trims_from_either_edge_and_plays_the_same_samples() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    let lane = f.read("data/chops/clips.json");
    let whole = rendered(&s);
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "j6xncyg7"), start: 2.5, length: 1.5 });
    let c = audio(&s, "j6xncyg7");
    assert_eq!((c.start, c.length, c.offset), (2.5, 1.5, 0.0), "the left edge keeps the offset");
    assert_eq!(frames(&rendered(&s), 1.25 + 0.02, 2.0), frames(&whole, 1.25 + 0.02, 2.0), "what is left plays as it did");
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "j6xncyg7"), start: 2.0, length: 2.0 });
    assert_eq!(f.read("data/chops/clips.json"), lane, "and back");
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "j6xncyg7"), start: 2.0, length: 1.0 });
    let c = audio(&s, "j6xncyg7");
    assert_eq!((c.start, c.length, c.offset), (2.0, 1.0, 0.5), "the right edge moves it: what played last is cut");
    assert_eq!(frames(&rendered(&s), 1.0 + 0.02, 1.5), frames(&whole, 1.0 + 0.02, 1.5), "what is left plays as it did");
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "j6xncyg7"), start: 2.0, length: 2.0 });
    assert_eq!(f.read("data/chops/clips.json"), lane, "and back");
}

// Q12: a reversed clip splits into two that play what it played: the first part starts later in
// the asset, the second keeps the offset.
#[test]
fn a_reversed_clip_splits_into_the_same_samples() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    let whole = rendered(&s);
    let done = go(&mut s, Gesture::Split { clips: vec![by_id("chops/clips", "j6xncyg7")], at: 3.0 });
    let (first, second) = (audio(&s, "j6xncyg7"), audio(&s, &done.made[0].1));
    assert_eq!((first.start, first.length, first.offset, first.reverse), (2.0, 1.0, 0.5, true));
    assert_eq!((second.start, second.length, second.offset, second.reverse), (3.0, 1.0, 0.0, true));
    assert_eq!(frames(&rendered(&s), 1.02, 2.0), frames(&whole, 1.02, 2.0), "the same samples");
}

// Q13, D3: the edges stop at the file, both ways: forwards, the left edge at its start and the
// right edge at its end; backwards, the right edge at its start and the left edge at its end. With
// 1 ms of slack, and a clip already past its end keeps its other edge free.
#[test]
fn the_edges_stop_at_the_file_both_ways() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    let file = keys_seconds(&f);
    let beats = |secs: f64| secs * 2.0;
    // Forwards, a3s9vpxb from 0 with offset 0: its right edge reaches the file's end, and no further.
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "a3s9vpxb"), start: 0.0, length: beats(file) });
    let why = refused(&mut s, Gesture::Resize { clip: by_id("chops/clips", "a3s9vpxb"), start: 0.0, length: beats(file) + 0.01 });
    assert!(why.contains("ends where keys-bounce.wav ends"), "{why}");
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "a3s9vpxb"), start: 0.0, length: beats(file) + 0.0015 });
    assert!((audio(&s, "a3s9vpxb").length - beats(file)).abs() < 0.002, "1 ms of slack");
    // Backwards, j6xncyg7 (offset 0), moved to beats 18 to 20: its right edge can't go on (the
    // file's start)…
    go(&mut s, Gesture::Move { clips: vec![by_id("chops/clips", "j6xncyg7")], by: 16.0, to: None, copy: false });
    let why = refused(&mut s, Gesture::Resize { clip: by_id("chops/clips", "j6xncyg7"), start: 18.0, length: 2.5 });
    assert!(why.contains("ends where keys-bounce.wav starts"), "{why}");
    // …its left edge reaches back to the file's end, and no further.
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "j6xncyg7"), start: 20.0 - beats(file), length: beats(file) });
    let why = refused(
        &mut s,
        Gesture::Resize { clip: by_id("chops/clips", "j6xncyg7"), start: 20.0 - beats(file) - 0.01, length: beats(file) + 0.01 },
    );
    assert!(why.contains("starts where keys-bounce.wav ends"), "{why}");
    // Forwards, the left edge stops at the file's start (the old words).
    let why = refused(&mut s, Gesture::Resize { clip: by_id("chops/clips", "nwskkw0g"), start: 3.9, length: 1.1 });
    assert!(why.contains("pitch"), "a pitched clip's left edge, as before: {why}");
}

// A trim or a split that leaves a clip shorter than its fades fits them in (the fade in first), so
// a volume or a direction never fails on them after.
#[test]
fn a_trim_keeps_the_fades_inside_the_clip() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    // a3s9vpxb is 1 s (2 beats at 120 BPM).
    go(&mut s, set("a3s9vpxb", Settings { fade_in: Some(0.4), fade_out: Some(0.4), ..Settings::default() }));
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "a3s9vpxb"), start: 0.0, length: 0.5 });
    let c = audio(&s, "a3s9vpxb");
    assert_eq!((c.fade_in, c.fade_out), (0.25, 0.0), "the fade in fits; nothing is left for the fade out");
    go(&mut s, set("a3s9vpxb", Settings { gain_db: Some(-3.0), ..Settings::default() }));
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "a3s9vpxb"), start: 0.0, length: 2.0 });
    go(&mut s, set("a3s9vpxb", Settings { fade_in: Some(0.8), ..Settings::default() }));
    let done = go(&mut s, Gesture::Split { clips: vec![by_id("chops/clips", "a3s9vpxb")], at: 1.0 });
    assert_eq!(audio(&s, "a3s9vpxb").fade_in, 0.5, "a split's first part keeps what fits");
    assert_eq!(audio(&s, &done.made[0].1).fade_in, 0.0);
}

// As before A3: a warped clip's left edge refuses a pitched clip.
#[test]
fn a_warped_pitched_clips_left_edge_still_refuses() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    let why = refused(&mut s, Gesture::Resize { clip: by_id("chops/clips", "cdayrg4g"), start: 13.0, length: 3.0 });
    assert!(why.contains("pitch"), "{why}");
    go(&mut s, Gesture::Resize { clip: by_id("chops/clips", "cdayrg4g"), start: 12.0, length: 3.0 });
}

/// How loud `a` is over how far `b` is from it, in dB, over frames `from..to` of both channels.
fn clean_db(a: &[[f32; 2]], b: &[[f32; 2]], from: usize, to: usize) -> f64 {
    let (mut sig, mut err) = (0.0f64, 0.0f64);
    for (x, y) in a[from..to].iter().zip(&b[from..to]) {
        for c in 0..2 {
            sig += (x[c] as f64).powi(2);
            err += (x[c] as f64 - y[c] as f64).powi(2);
        }
    }
    10.0 * (sig / err.max(1e-30)).log10()
}

// The sx plan, D11: Follow tempo. On, a clip gets warp markers at its start, its end and each tempo
// change inside it, from the tempo now: each beat plays the moment of its file it played, so it
// sounds the same (a stretch at its own speed is its file, within 40 dB). The markers are in beats,
// so after a tempo change it keeps them: it no longer plays at its own speed. Again on, nothing
// changes; one step each, and undo takes them back; off, it plays at its own speed again.
#[test]
fn follow_tempo_keeps_the_clip_where_it_plays_and_undo_takes_it_back() {
    let f = fixture("examples/resample");
    let mut s = f.open();
    // The first clip is 2 beats from 0: at 120 BPM, then 90 from beat 1, it plays 0 s to 1.17 s.
    crate::song::tempo_change(&mut s, 1.0, 90.0).unwrap();
    let was = audio(&s, "a3s9vpxb");
    let (plain, n) = (rendered(&s), s.accepted().number);
    go(&mut s, set("a3s9vpxb", Settings { follow: Some(true), ..Settings::default() }));
    assert_eq!(s.accepted().number, n + 1, "one step");
    let on = audio(&s, "a3s9vpxb");
    let want = [(0.0, 0.0), (0.5, 1.0), (0.5 + 60.0 / 90.0, 2.0)];
    assert!(on.warp.len() == 3 && on.warp.iter().zip(want).all(|(m, w)| (m.0 - w.0).abs() < 1e-9 && m.1 == w.1), "{:?}", on.warp);
    assert_eq!(audio(&s, "a3s9vpxb"), m2_song::AudioClip { warp: on.warp.clone(), ..was.clone() }, "only its markers");
    let followed = rendered(&s);
    let end = (1.1667 * 48_000.0) as usize;
    let clean = clean_db(&plain, &followed, 2_400, end - 2_400);
    assert!(clean > 40.0, "on, the clip sounds as it did: {clean:.1} dB");
    // Again on: nothing changes.
    go(&mut s, set("a3s9vpxb", Settings { follow: Some(true), ..Settings::default() }));
    assert_eq!(s.accepted().number, n + 1, "on again is no change");
    // The song slows to 60 BPM: the clip keeps its markers, so its beats.
    crate::song::tempo(&mut s, 0.0, 60.0).unwrap();
    assert_eq!(audio(&s, "a3s9vpxb").warp, on.warp, "its markers are in beats");
    let slow = rendered(&s);
    go(&mut s, set("a3s9vpxb", Settings { follow: Some(false), ..Settings::default() }));
    assert_eq!(audio(&s, "a3s9vpxb"), m2_song::AudioClip { warp: vec![], ..was.clone() }, "off: no markers");
    let own = rendered(&s);
    let end = (1.667 * 48_000.0) as usize;
    assert!(clean_db(&slow, &own, 2_400, end) < 10.0, "slowed, the clip followed the tempo, not its own speed");
    for _ in 0..3 {
        crate::undo(&mut s).unwrap();
    }
    assert_eq!(audio(&s, "a3s9vpxb"), was, "three undos: off, the tempo and on, taken back");
    assert_eq!(s.accepted().compiled.data.tempo().segments().len(), 2);
}

/// A take's clip for [`Gesture::Record`]: notes as (pitch, start, length) from its start.
fn take(start: f64, length: f64, notes: &[(f64, f64, f64)]) -> Value {
    let notes: Vec<Value> =
        notes.iter().map(|(p, s, l)| serde_json::json!({"pitch": p, "start": s, "length": l, "velocity": 0.5})).collect();
    serde_json::json!({"start": start, "length": length, "notes": notes})
}

// MON-10: a punched take replaces only its range. The clip under it is cut at both edges, as Split
// cuts it: the part before keeps its id, the part after gets one of its own, and what lay inside
// goes. One undo puts every byte back.
#[test]
fn a_take_replaces_its_range_as_split_cuts() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = f.read("data/kick/hits.json");
    let lane = addr("kick/hits");
    let t = Taken { lane: lane.clone(), clip: take(8.5, 4.0, &[(28.0, 0.25, 0.5)]), how: How::Replace };
    let done = go(&mut s, Gesture::Record { takes: vec![t] });
    let c = clips(&s, "kick/hits");
    assert_eq!(c.len(), 3, "{c:?}");
    assert_eq!((c[0].0.as_deref(), c[0].1, c[0].2), (Some("my53vvkc"), 0.0, 8.5), "the part before keeps its id");
    assert_eq!((c[1].1, c[1].2, c[2].1, c[2].2), (8.5, 4.0, 12.5, 19.5), "the take, then the part after: {c:?}");
    let [(made_lane, id)] = &done.made[..] else { panic!("{:?}", done.made) };
    assert!(made_lane == &lane && c[1].0.as_deref() == Some(id.as_str()) && c[2].0.as_deref() != Some("my53vvkc"));
    let starts: Vec<(f64, f64)> = notes(&s, "kick/hits").iter().map(|n| (n.start, n.pitch)).collect();
    let want: Vec<(f64, f64)> = (0..=8).map(|b| (b as f64, 24.0)).chain([(8.75, 28.0)]).chain((13..32).map(|b| (b as f64, 24.0))).collect();
    assert_eq!(starts, want, "the kicks before and after, and the take's one note between");
    s.undo().unwrap();
    assert_eq!(f.read("data/kick/hits.json"), before, "one undo");
}

// MON-12: with Overdub, a take's notes go into the clip that holds its first note, which grows to
// hold them; a take over no clip makes its own.
#[test]
fn overdub_grows_the_clip_to_hold_the_take() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let lane = addr("kick/hits");
    let into = |clip| Gesture::Record { takes: vec![Taken { lane: lane.clone(), clip, how: How::Into }] };
    let done = go(&mut s, into(take(28.0, 8.0, &[(26.0, 0.5, 0.25), (26.0, 6.0, 0.25)])));
    assert_eq!(clips(&s, "kick/hits"), [(Some("my53vvkc".into()), 0.0, 36.0)], "one clip, grown");
    assert!(done.made.is_empty(), "nothing new: {:?}", done.made);
    let n = notes(&s, "kick/hits");
    assert_eq!(n.len(), 34);
    assert!(n.iter().any(|x| x.start == 28.5 && x.pitch == 26.0) && n.iter().any(|x| x.start == 34.0 && x.pitch == 26.0), "{n:?}");
    let done = go(&mut s, into(take(40.0, 4.0, &[(26.0, 1.0, 0.25)])));
    assert_eq!(clips(&s, "kick/hits").len(), 2, "over no clip: one of its own");
    assert_eq!(done.made.len(), 1);
}

// MON-10 (review Q9): the take's gesture is the server's own: the clips and drag routes can't be
// asked for it.
#[test]
fn the_takes_gesture_is_off_the_wire() {
    let asked =
        serde_json::json!({"op": "record", "takes": [{"module": "kick", "lane": "hits", "clip": take(0.0, 4.0, &[]), "how": "replace"}]});
    assert!(serde_json::from_value::<Gesture>(asked).is_err(), "refused");
}
