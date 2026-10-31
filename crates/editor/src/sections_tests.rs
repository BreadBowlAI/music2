//! Sections (ARR-1 to ARR-9) on one song: the first song with an audio track, notes clips inside,
//! across and after bars 9–16, automation across both edges, an audio clip across the first, a
//! tempo change inside and after, a 3/4 from bar 21, and the markers Verse, Chorus and Bridge.

use std::collections::BTreeMap;

use m2_session::Session;
use m2_song::{LaneAddr, LaneData};

use super::*;
use crate::import::{self, Source, Target};
use crate::testing::{Fixture, files_under, fixture};

fn addr(lane: &str) -> LaneAddr {
    let (m, l) = lane.split_once('/').unwrap();
    LaneAddr::new(m, l).unwrap()
}

/// A 16-bit mono WAV, `seconds` long.
fn wav(seconds: f64) -> Vec<u8> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
    for i in 0..(seconds * 48_000.0) as usize {
        w.write_sample(((i as f32 * 0.01).sin() * 16_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    out.into_inner()
}

// The lanes in their canonical form, as music2 writes them: undo puts back the bytes a gesture
// replaced.
const KICK: &str = r#"{"clips": [
  {"id": "k0000001", "start": 0, "length": 32, "notes": [
    {"pitch": "C1", "start": 0, "length": 1, "velocity": 0.8},
    {"pitch": "C1", "start": 16, "length": 1, "velocity": 0.8}
  ]},
  {"id": "k0000002", "start": 32, "length": 32, "notes": [
    {"pitch": "C1", "start": 0, "length": 1, "velocity": 0.8},
    {"pitch": "C1", "start": 8, "length": 1, "velocity": 0.8}
  ]},
  {"id": "k0000003", "start": 64, "length": 16, "notes": [
    {"pitch": "C1", "start": 0, "length": 1, "velocity": 0.8}
  ]}
]}
"#;

const BASS: &str = r#"{"clips": [
  {"id": "b0000001", "start": 24, "length": 16, "notes": [
    {"pitch": "E2", "start": 0, "length": 2, "velocity": 0.8},
    {"pitch": "E2", "start": 6, "length": 4, "velocity": 0.8},
    {"pitch": "E2", "start": 10, "length": 2, "velocity": 0.8}
  ]},
  {"id": "b0000002", "start": 56, "length": 16, "notes": [
    {"pitch": "G2", "start": 0, "length": 2, "velocity": 0.8},
    {"pitch": "G2", "start": 6, "length": 4, "velocity": 0.8},
    {"pitch": "G2", "start": 12, "length": 2, "velocity": 0.8}
  ]}
]}
"#;

const CUTOFF: &str = r#"{"unit": "hz", "clips": [
  {"id": "c0000001", "start": 16, "length": 64, "points": [[0, 300], [64, 2200]]}
]}
"#;

const ARRANGEMENT: &str = "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 124},\n    {\"beat\": 48, \"bpm\": 140},\n    {\"beat\": 80, \"bpm\": 100}\n  ],\n  \"meter\": [\n    {\"beat\": 0, \"beats\": 4, \"unit\": 4},\n    {\"beat\": 80, \"beats\": 3, \"unit\": 4}\n  ]\n}\n";

/// The ids the song starts with: any other is a new one.
const OLD: [&str; 7] = ["k0000001", "k0000002", "k0000003", "b0000001", "b0000002", "c0000001", "v0000001"];

/// The song, opened. Bars 9–16 are beats 32 to 64.
fn song() -> (Fixture, Session) {
    let f = fixture("examples/first-song");
    {
        let mut s = f.open();
        let a = import::audio(&f.0, Source::Bytes { name: "vox.wav".into(), bytes: wav(30.0) }, &mut |_| {}).unwrap();
        import::place(&mut s, &[a], &Target::Track { module: "keys".into(), beat: 0.0 }).unwrap();
    }
    let placed: serde_json::Value = serde_json::from_str(&f.read("data/vox/clips.json")).unwrap();
    let hash = placed["clips"][0]["hash"].as_str().unwrap().to_string();
    f.write("data/kick/hits.json", KICK);
    f.write("data/bass/notes.json", BASS);
    f.write("data/bass/cutoff.json", CUTOFF);
    f.write("data/bass/drop.json", "{\"unit\": \"unit01\", \"clips\": []}\n");
    f.write("data/hats/hits.json", "{\"clips\": []}\n");
    f.write("data/keys/chords.json", "{\"clips\": []}\n");
    let vox = format!(
        "{{\"clips\": [\n  {{\"id\": \"v0000001\", \"start\": 28, \"length\": 8, \"asset\": \"vox.wav\", \"hash\": \"{hash}\", \"offset\": 0, \"gain_db\": 0, \"fade_in\": 0.01, \"fade_out\": 0.02, \"reverse\": false, \"pitch\": 0, \"warp\": []}}\n]}}\n"
    );
    f.write("data/vox/clips.json", vox);
    f.write("data/arrangement.json", ARRANGEMENT);
    let mut s = f.open();
    assert!(
        s.draft_errors.is_empty() && s.accepted().compiled.problems.iter().all(|d| !d.code.starts_with("lane.")),
        "{:?}",
        s.accepted().compiled.problems
    );
    let marks = [(0.0, "Verse"), (32.0, "Chorus"), (64.0, "Bridge")];
    markers::set(&mut s, marks.iter().map(|(beat, name)| Marker { beat: *beat, name: name.to_string() }).collect()).unwrap();
    (f, s)
}

fn n(x: f64) -> String {
    m2_song::lane::number(x)
}

/// Each clip of `lane` in order, as `id start+length […]`: its notes as `pitch@start+length`, its
/// points as `beat:value`, its audio as its offset and fades. An id the song didn't start with is `*`.
fn lane(s: &Session, lane: &str) -> Vec<String> {
    let a = addr(lane);
    let Some(d) = s.accepted().compiled.data.lane(a.module.as_str(), a.lane.as_str()) else { return vec![] };
    let id = |id: &Option<String>| match id.as_deref() {
        Some(i) if OLD.contains(&i) => i.to_string(),
        Some(i) if m2_song::ClipId::valid(i) => "*".into(),
        other => format!("{other:?}"),
    };
    let join = |v: Vec<String>| v.join(" ");
    match &**d {
        LaneData::Notes(cs) => cs
            .iter()
            .map(|c| {
                let notes = c.notes.iter().map(|x| format!("{}@{}+{}", n(x.pitch), n(x.start), n(x.length))).collect();
                format!("{} {}+{} [{}]", id(&c.id), n(c.start), n(c.length), join(notes))
            })
            .collect(),
        LaneData::Control { clips, .. } => clips
            .iter()
            .map(|c| {
                let points = c.points.iter().map(|(b, v)| format!("{}:{}", n(*b), n(*v))).collect();
                format!("{} {}+{} [{}]", id(&c.id), n(c.start), n(c.length), join(points))
            })
            .collect(),
        LaneData::Steps { clips, .. } => clips
            .iter()
            .map(|c| {
                let points = c.points.iter().map(|(b, v)| format!("{}:{}", n(*b), serde_json::to_string(v).unwrap())).collect();
                format!("{} {}+{} [{}]", id(&c.id), n(c.start), n(c.length), join(points))
            })
            .collect(),
        LaneData::Audio(cs) => cs
            .iter()
            .map(|c| {
                format!("{} {}+{} offset {} fades {}/{}", id(&c.id), n(c.start), n(c.length), n(c.offset), n(c.fade_in), n(c.fade_out))
            })
            .collect(),
    }
}

/// Every file of the song's data, and its layout, by path.
fn files(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    let mut out: BTreeMap<String, Vec<u8>> = files_under(&f.0.join("data"))
        .into_iter()
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect();
    if let Ok(bytes) = std::fs::read(f.0.join("layout.json")) {
        out.insert("layout.json".into(), bytes);
    }
    out
}

/// The markers as `layout.json` has them: `name@beat`.
fn marks(f: &Fixture) -> Vec<String> {
    markers::on_disk(&f.0).iter().map(|m| format!("{}@{}", m.name, n(m.beat))).collect()
}

fn go(s: &mut Session, g: Gesture) -> ClipsEdited {
    edit(s, &g).unwrap_or_else(|e| panic!("{g:?}: {e}"))
}

/// [`go`] as the Tx with id `tx_id`: the ids it mints are known (FMT-11).
fn go_as(s: &mut Session, g: Gesture, tx_id: &str) -> ClipsEdited {
    edit_as(s, &g, tx_id).unwrap_or_else(|e| panic!("{g:?}: {e}"))
}

fn refused(s: &mut Session, g: Gesture) -> String {
    match edit(s, &g) {
        Err(e) => e.to_string(),
        Ok(done) => panic!("{g:?} was accepted: {done:?}"),
    }
}

/// The seconds `beats` last at 124 BPM.
fn at124(beats: f64) -> f64 {
    beats * 60.0 / 124.0
}

// ARR-1: a section is whole bars, from the song's start on; anything else is refused in words, and
// nothing is written.
#[test]
fn a_span_off_the_bar_lines_is_refused() {
    let (f, mut s) = song();
    let before = files(&f);
    // Places as a person counts them, from 1 (R14): beat 33 is bar 9's second beat.
    let words = refused(&mut s, Gesture::Delete { start: 33.0, end: 64.0 });
    assert!(words.ends_with("A section starts at the start of a bar, not at bar 9 beat 2."), "{words}");
    let words = refused(&mut s, Gesture::Delete { start: 32.0, end: 62.0 });
    assert!(words.ends_with("A section ends at the end of a bar, not at bar 16 beat 3."), "{words}");
    let words = refused(&mut s, Gesture::Delete { start: 32.0, end: 32.0 });
    assert!(words.contains("ends after it starts"), "{words}");
    let words = refused(&mut s, Gesture::Delete { start: f64::NAN, end: 32.0 });
    assert!(words.ends_with("A section starts at the start of a bar, and NaN isn't a place in the song."), "{words}");
    // In 3/4 from bar 21 (beat 80), bar 22 starts at beat 83.
    let words = refused(&mut s, Gesture::InsertBars { at: 84.0, bars: 1 });
    assert!(words.ends_with("New bars go at the start of a bar, not at bar 22 beat 2."), "{words}");
    let words = refused(&mut s, Gesture::Move { start: 32.0, end: 64.0, to: 97.0 });
    assert!(words.ends_with("The bars go at the start of a bar, not at bar 26 beat 3."), "{words}");
    assert!(refused(&mut s, Gesture::InsertBars { at: 32.0, bars: 0 }).contains("Insert 1 to 999 bars"));
    assert!(refused(&mut s, Gesture::InsertBars { at: 32.0, bars: 1000 }).contains("Insert 1 to 999 bars"));
    assert_eq!(files(&f), before);
    // On a bar line of the 3/4: taken (nothing is there to move).
    go(&mut s, Gesture::InsertBars { at: 83.0, bars: 1 });
}

// ARR-2, ARR-8, WRITE-21: each gesture is one Tx and one undo step, which puts every byte of the
// data and the layout back. Paste, duplicate, move and delete change clips only; insert and delete
// bars move the arrangement and the markers too.
#[test]
fn each_gesture_is_one_undo_step_byte_for_byte() {
    let (f, mut s) = song();
    let copy = section(s.accepted(), 32.0, 64.0).unwrap();
    let gestures = [
        (Gesture::Paste { section: copy, at: 0.0 }, false),
        (Gesture::Duplicate { start: 32.0, end: 64.0, to: Some(0.0) }, false),
        (Gesture::Move { start: 32.0, end: 64.0, to: 0.0 }, false),
        (Gesture::Delete { start: 32.0, end: 64.0 }, false),
        (Gesture::InsertBars { at: 32.0, bars: 2 }, true),
        (Gesture::DeleteBars { start: 32.0, end: 64.0 }, true),
    ];
    for (g, time) in gestures {
        let before = files(&f);
        let steps = s.last_step();
        let done = go(&mut s, g.clone());
        assert_eq!(s.last_step(), steps + 1, "{g:?}: one undo step");
        let after = files(&f);
        let changed: Vec<&String> = after.keys().filter(|k| before.get(*k) != after.get(*k)).collect();
        assert!(changed.contains(&&"data/kick/hits.json".to_string()), "{g:?}: {changed:?}");
        for file in ["data/arrangement.json", "layout.json"] {
            assert_eq!(changed.contains(&&file.to_string()), time, "{g:?}: {file} (ARR-8): {changed:?}");
        }
        assert!(done.edited.before != done.edited.after, "{g:?}: {done:?}");
        s.undo().unwrap();
        assert_eq!(files(&f), before, "{g:?}: undo puts every byte back");
    }
}

// ARR-3: a clip that crosses an edge is split there: notes that cross are cut to it, automation
// takes its value there on both sides, and an audio clip's later part starts later in its file.
// Delete leaves a gap: what is inside goes, what is outside stays where it is, with its id.
#[test]
fn a_clip_crossing_each_edge_is_split_there() {
    let (f, mut s) = song();
    let done = go(&mut s, Gesture::Delete { start: 32.0, end: 64.0 });
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("bars 9–16", "empty bars 9–16"));
    assert_eq!(lane(&s, "kick/hits"), ["k0000001 0+32 [24@0+1 24@16+1]", "k0000003 64+16 [24@0+1]"]);
    assert_eq!(lane(&s, "bass/notes"), ["b0000001 24+8 [40@0+2 40@6+2]", "* 64+8 [43@4+2]"]);
    assert_eq!(lane(&s, "bass/cutoff"), ["c0000001 16+16 [0:300 16:775]", "* 64+16 [0:1725 16:2200]"]);
    assert_eq!(lane(&s, "vox/clips"), ["v0000001 28+4 offset 0 fades 0.01/0"]);
    assert_eq!(f.read("data/arrangement.json"), ARRANGEMENT);
    assert_eq!(marks(&f), ["Verse@0", "Chorus@32", "Bridge@64"]);
    s.undo().unwrap();
    // The files exactly, with the ids a Tx of a known id gives (FMT-11): counted across the lanes,
    // in their order, the cutoff's first.
    go_as(&mut s, Gesture::Delete { start: 32.0, end: 64.0 }, "t1");
    let new = |lane: &str, n: u32| m2_song::ClipId::derive("t1", &addr(lane), n);
    let kick = KICK.replace(
        "  {\"id\": \"k0000002\", \"start\": 32, \"length\": 32, \"notes\": [\n    {\"pitch\": \"C1\", \"start\": 0, \"length\": 1, \"velocity\": 0.8},\n    {\"pitch\": \"C1\", \"start\": 8, \"length\": 1, \"velocity\": 0.8}\n  ]},\n",
        "",
    );
    assert_eq!(f.read("data/kick/hits.json"), kick);
    let bass = format!(
        r#"{{"clips": [
  {{"id": "b0000001", "start": 24, "length": 8, "notes": [
    {{"pitch": "E2", "start": 0, "length": 2, "velocity": 0.8}},
    {{"pitch": "E2", "start": 6, "length": 2, "velocity": 0.8}}
  ]}},
  {{"id": "{}", "start": 64, "length": 8, "notes": [
    {{"pitch": "G2", "start": 4, "length": 2, "velocity": 0.8}}
  ]}}
]}}
"#,
        new("bass/notes", 1)
    );
    assert_eq!(f.read("data/bass/notes.json"), bass);
    let cutoff = format!(
        "{{\"unit\": \"hz\", \"clips\": [\n  {{\"id\": \"c0000001\", \"start\": 16, \"length\": 16, \"points\": [[0, 300], [16, 775]]}},\n  {{\"id\": \"{}\", \"start\": 64, \"length\": 16, \"points\": [[0, 1725], [16, 2200]]}}\n]}}\n",
        new("bass/cutoff", 0)
    );
    assert_eq!(f.read("data/bass/cutoff.json"), cutoff);
    s.undo().unwrap();
    // A copy of the bars takes the audio clip's later part: it starts 4 beats later in its file.
    let copy = section(s.accepted(), 32.0, 64.0).unwrap();
    go(&mut s, Gesture::Paste { section: copy, at: 0.0 });
    let later = format!("offset {} fades 0/0.02", n(at124(4.0)));
    assert_eq!(
        lane(&s, "vox/clips"),
        [format!("* 0+4 {later}"), format!("* 32+4 {later}")],
        "the copy at bar 1, and the clip's part after bar 9"
    );
    s.undo().unwrap();
    // An audio clip Split can't split refuses the gesture, in words.
    let pitched = f.read("data/vox/clips.json").replace("\"pitch\": 0", "\"pitch\": 2");
    f.write("data/vox/clips.json", pitched);
    drop(s);
    let mut s = f.open();
    let words = refused(&mut s, Gesture::Delete { start: 32.0, end: 64.0 });
    assert!(
        words.ends_with(
            "Vox's audio clip at bar 8 has its pitch changed, so it can't be cut at bar 9. Set its pitch back to 0, or move it off bar 9."
        ),
        "{words}"
    );
}

// ARR-4: a paste replaces what the bars it lands on held, lane by lane: split at their edges, the
// inside cleared, the copy placed with fresh ids. Duplicate is a paste right after itself.
#[test]
fn a_paste_replaces_what_was_there() {
    let (f, mut s) = song();
    let copy = section(s.accepted(), 32.0, 64.0).unwrap();
    let kick = copy.lanes.iter().find(|l| l.lane == addr("kick/hits")).unwrap();
    assert_eq!(kick.clips.len(), 1, "only the clips that overlap the bars, whole: {kick:?}");
    assert!(copy.lanes.iter().any(|l| l.lane == addr("hats/hits") && l.clips.is_empty()), "every lane, an empty one too");
    let cutoff = copy.lanes.iter().find(|l| l.lane == addr("bass/cutoff")).unwrap();
    assert_eq!(cutoff.kind, Holds::Control { unit: Some("hz".into()) }, "each lane with its kind (R12)");
    let done = go(&mut s, Gesture::Paste { section: copy, at: 0.0 });
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("bars 1–8", "a copy of bars 9–16"));
    let pasted = [
        ("kick/hits", vec!["* 0+32 [24@0+1 24@8+1]", "k0000002 32+32 [24@0+1 24@8+1]", "k0000003 64+16 [24@0+1]"]),
        ("bass/notes", vec!["* 0+8 [40@2+2]", "* 24+8 [43@0+2 43@6+2]", "* 32+8 [40@2+2]", "b0000002 56+16 [43@0+2 43@6+4 43@12+2]"]),
        ("bass/cutoff", vec!["* 0+32 [0:775 32:1725]", "* 32+48 [0:775 48:2200]"]),
    ];
    for (l, want) in &pasted {
        assert_eq!(lane(&s, l), *want, "{l}");
    }
    let later = format!("offset {} fades 0/0.02", n(at124(4.0)));
    assert_eq!(lane(&s, "vox/clips"), [format!("* 0+4 {later}"), format!("* 32+4 {later}")]);
    // The app selects what was placed.
    assert_eq!(done.made.iter().filter(|(l, _)| *l == addr("bass/notes")).count(), 2, "{:?}", done.made);
    assert_eq!(f.read("data/arrangement.json"), ARRANGEMENT);
    s.undo().unwrap();
    go(&mut s, Gesture::Duplicate { start: 32.0, end: 64.0, to: Some(0.0) });
    for (l, want) in &pasted {
        assert_eq!(lane(&s, l), *want, "duplicate: {l}");
    }
}

// ARR-4, FMT-12: a move puts the bars at the bar line asked, keeps the id of a clip that was inside
// whole, and leaves a gap.
#[test]
fn a_move_leaves_a_gap_and_keeps_ids() {
    let (_f, mut s) = song();
    let done = go(&mut s, Gesture::Move { start: 32.0, end: 64.0, to: 0.0 });
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("bars 9–16", "bars 1–8"));
    assert_eq!(lane(&s, "kick/hits"), ["k0000002 0+32 [24@0+1 24@8+1]", "k0000003 64+16 [24@0+1]"]);
    assert_eq!(lane(&s, "bass/notes"), ["* 0+8 [40@2+2]", "b0000002 24+8 [43@0+2 43@6+2]", "* 64+8 [43@4+2]"]);
}

// ARR-5: a copy keeps what was copied: cut (copied, then emptied) and pasted back, the bars play as
// they did. A lane the song doesn't have, or one of another kind, refuses the paste in words.
#[test]
fn a_cut_then_a_paste_keeps_the_copy() {
    let (_f, mut s) = song();
    let kick = |s: &Session| s.accepted().compiled.data.notes("kick", "hits");
    let was = kick(&s);
    let copy = section(s.accepted(), 32.0, 64.0).unwrap();
    go(&mut s, Gesture::Delete { start: 32.0, end: 64.0 });
    assert_ne!(kick(&s), was);
    go(&mut s, Gesture::Paste { section: copy.clone(), at: 32.0 });
    assert_eq!(kick(&s), was, "the kick plays as it did");
    // A lane the song no longer has is skipped while the copy holds nothing for it (R7).
    let kick_lane = copy.lanes.iter().find(|l| l.lane == addr("kick/hits")).unwrap().clone();
    let mut gone = copy.clone();
    gone.lanes.push(SectionLane { lane: addr("lead/notes"), kind: Holds::Notes, clips: vec![] });
    go(&mut s, Gesture::Paste { section: gone.clone(), at: 0.0 });
    s.undo().unwrap();
    // One it holds clips for refuses the paste, naming every such lane, and never asks for a copy
    // again: after a cut there is none to make.
    gone.lanes.push(SectionLane { lane: addr("lead/notes"), kind: Holds::Notes, clips: kick_lane.clips.clone() });
    gone.lanes.push(SectionLane { lane: addr("pad/chords"), kind: Holds::Notes, clips: kick_lane.clips.clone() });
    let words = refused(&mut s, Gesture::Paste { section: gone, at: 0.0 });
    assert!(
        words.ends_with(
            "The copy has clips for lead's notes lane and pad's chords lane, which the song doesn't have now, so it can't be pasted."
        ),
        "{words}"
    );
    assert!(!words.contains("again"), "{words}");
    // A lane that takes another kind of clip (R12): checked by the copy's kind, as a clip paste is.
    let mut other = copy.clone();
    let cutoff = other.lanes.iter_mut().find(|l| l.lane == addr("bass/cutoff")).unwrap();
    (cutoff.kind, cutoff.clips) = (Holds::Notes, kick_lane.clips.clone());
    let words = refused(&mut s, Gesture::Paste { section: other, at: 0.0 });
    assert!(words.ends_with("Bass's cutoff lane: a notes clip can't go on an automation lane, so the copy can't be pasted."), "{words}");
    let mut hz = copy.clone();
    hz.lanes.iter_mut().find(|l| l.lane == addr("bass/cutoff")).unwrap().kind = Holds::Control { unit: Some("db".into()) };
    let words = refused(&mut s, Gesture::Paste { section: hz, at: 0.0 });
    assert!(words.contains("their units differ"), "{words}");
    let words = refused(&mut s, Gesture::Paste { section: copy, at: 33.0 });
    assert!(words.ends_with("A paste goes at the start of a bar, not at bar 9 beat 2."), "{words}");
}

// ARR-6: insert bars opens bars at a bar line, in the time signature before it: every clip, marker,
// tempo change and time signature change at or after it moves later; a clip across it is split.
#[test]
fn insert_bars_moves_everything_after() {
    let (f, mut s) = song();
    let done = go(&mut s, Gesture::InsertBars { at: 32.0, bars: 2 });
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("bar 9", "2 empty bars at bar 9"));
    assert!(done.made.is_empty(), "nothing to select: {:?}", done.made);
    assert_eq!(lane(&s, "kick/hits"), ["k0000001 0+32 [24@0+1 24@16+1]", "k0000002 40+32 [24@0+1 24@8+1]", "k0000003 72+16 [24@0+1]"]);
    assert_eq!(lane(&s, "bass/notes"), ["b0000001 24+8 [40@0+2 40@6+2]", "* 40+8 [40@2+2]", "b0000002 64+16 [43@0+2 43@6+4 43@12+2]"]);
    // The automation across bar 9 holds its value there over the new bars (R11); notes and audio
    // leave them empty.
    assert_eq!(lane(&s, "bass/cutoff"), ["c0000001 16+24 [0:300 16:775 24:775]", "* 40+48 [0:775 48:2200]"]);
    assert_eq!(
        lane(&s, "vox/clips"),
        ["v0000001 28+4 offset 0 fades 0.01/0".to_string(), format!("* 40+4 offset {} fades 0/0.02", n(at124(4.0)))]
    );
    assert_eq!(
        f.read("data/arrangement.json"),
        "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 124},\n    {\"beat\": 56, \"bpm\": 140},\n    {\"beat\": 88, \"bpm\": 100}\n  ],\n  \"meter\": [\n    {\"beat\": 0, \"beats\": 4, \"unit\": 4},\n    {\"beat\": 88, \"beats\": 3, \"unit\": 4}\n  ]\n}\n"
    );
    assert_eq!(marks(&f), ["Verse@0", "Chorus@40", "Bridge@72"]);
    s.undo().unwrap();
    // At bar 1: the song's own tempo and time signature stay at beat 0; the rest moves.
    go(&mut s, Gesture::InsertBars { at: 0.0, bars: 1 });
    let arr = s.accepted().project.arrangement.clone();
    assert_eq!(arr.tempo.iter().map(|t| (t.beat, t.bpm)).collect::<Vec<_>>(), [(0.0, 124.0), (52.0, 140.0), (84.0, 100.0)]);
    assert_eq!(arr.meter.iter().map(|m| (m.beat, m.beats)).collect::<Vec<_>>(), [(0.0, 4), (84.0, 3)]);
    assert_eq!(marks(&f), ["Verse@4", "Chorus@36", "Bridge@68"]);
    s.undo().unwrap();
    // At the 3/4's bar line: the new bars are in the 4/4 before it, and the 3/4 moves after them.
    // No clip is there or after, so only the arrangement changes.
    let before = files(&f);
    go(&mut s, Gesture::InsertBars { at: 80.0, bars: 2 });
    let arr = s.accepted().project.arrangement.clone();
    assert_eq!(arr.meter.iter().map(|m| (m.beat, m.beats)).collect::<Vec<_>>(), [(0.0, 4), (88.0, 3)]);
    let after = files(&f);
    let changed: Vec<&String> = after.keys().filter(|k| before.get(*k) != after.get(*k)).collect();
    assert_eq!(changed, ["data/arrangement.json"]);
}

// ARR-6, ARR-3: bars inserted inside a clip split it there; its second part moves on.
#[test]
fn insert_bars_inside_a_clip_splits_it() {
    let (_f, mut s) = song();
    go(&mut s, Gesture::InsertBars { at: 40.0, bars: 1 });
    assert_eq!(
        lane(&s, "kick/hits"),
        ["k0000001 0+32 [24@0+1 24@16+1]", "k0000002 32+8 [24@0+1]", "* 44+24 [24@0+1]", "k0000003 68+16 [24@0+1]"]
    );
}

// ARR-7: delete bars takes the bars out and moves what follows earlier; of the changes inside,
// the one in effect at their end stays, at their start, so what follows plays and is named as
// before. One that would repeat the tempo before it is left out.
#[test]
fn delete_bars_keeps_what_follows() {
    let (f, mut s) = song();
    let done = go(&mut s, Gesture::DeleteBars { start: 32.0, end: 64.0 });
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("bars 9–16", "bars 9–16 taken out"));
    assert_eq!(lane(&s, "kick/hits"), ["k0000001 0+32 [24@0+1 24@16+1]", "k0000003 32+16 [24@0+1]"]);
    assert_eq!(lane(&s, "bass/notes"), ["b0000001 24+8 [40@0+2 40@6+2]", "* 32+8 [43@4+2]"]);
    assert_eq!(lane(&s, "bass/cutoff"), ["c0000001 16+16 [0:300 16:775]", "* 32+16 [0:1725 16:2200]"]);
    assert_eq!(lane(&s, "vox/clips"), ["v0000001 28+4 offset 0 fades 0.01/0"]);
    assert_eq!(
        f.read("data/arrangement.json"),
        "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 124},\n    {\"beat\": 32, \"bpm\": 140},\n    {\"beat\": 48, \"bpm\": 100}\n  ],\n  \"meter\": [\n    {\"beat\": 0, \"beats\": 4, \"unit\": 4},\n    {\"beat\": 48, \"beats\": 3, \"unit\": 4}\n  ]\n}\n"
    );
    assert_eq!(marks(&f), ["Verse@0", "Bridge@32"], "the Chorus went with its bars; the Bridge follows them");
    s.undo().unwrap();
    // A change back to the song's tempo inside the bars would say what plays there already.
    let back = ARRANGEMENT.replace("{\"beat\": 48, \"bpm\": 140}", "{\"beat\": 40, \"bpm\": 140},\n    {\"beat\": 48, \"bpm\": 124}");
    f.write("data/arrangement.json", back);
    drop(s);
    let mut s = f.open();
    go(&mut s, Gesture::DeleteBars { start: 32.0, end: 64.0 });
    let arr = s.accepted().project.arrangement.clone();
    assert_eq!(arr.tempo.iter().map(|t| (t.beat, t.bpm)).collect::<Vec<_>>(), [(0.0, 124.0), (48.0, 100.0)]);
}

// ARR-2, WRITE-14: no code changes, so a song whose draft doesn't build still takes a section
// gesture, checked against the last accepted lanes.
#[test]
fn a_song_that_doesnt_build_takes_them() {
    let (f, mut s) = song();
    f.write("tracks/hats.ts", "export default (;\n");
    assert!(s.refresh() && !s.draft_errors.is_empty());
    let (code, data) = (s.accepted().code_rev, s.accepted().data_rev);
    go(&mut s, Gesture::DeleteBars { start: 32.0, end: 64.0 });
    assert_eq!((s.accepted().code_rev, s.accepted().data_rev), (code, data + 1), "a data revision");
    assert!(!s.draft_errors.is_empty(), "the code is still broken");
    assert_eq!(lane(&s, "kick/hits"), ["k0000001 0+32 [24@0+1 24@16+1]", "k0000003 32+16 [24@0+1]"]);
    assert_eq!(f.read("tracks/hats.ts"), "export default (;\n", "code never moves");
}

// ARR-2: a gesture that changes nothing writes nothing.
#[test]
fn a_gesture_that_changes_nothing_writes_nothing() {
    let (f, mut s) = song();
    let before = files(&f);
    let steps = s.last_step();
    let done = go(&mut s, Gesture::Delete { start: 200.0, end: 212.0 });
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("no change", "no change"));
    assert_eq!((s.last_step(), files(&f)), (steps, before.clone()));
    // A move to where the bars are, and a duplicate onto themselves, change nothing either (R9).
    for g in [Gesture::Move { start: 32.0, end: 64.0, to: 32.0 }, Gesture::Duplicate { start: 32.0, end: 64.0, to: Some(32.0) }] {
        let done = go(&mut s, g.clone());
        assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("no change", "no change"), "{g:?}");
        assert_eq!((s.last_step(), files(&f)), (steps, before.clone()), "{g:?}");
    }
}

// ARR-4 (review R3): a section lands on whole bars. Bars 9–16 are 32 beats; from bar 17 they would
// end one beat into bar 26 of the 3/4, so a paste, a duplicate or a move there is refused in words,
// and a copy whose bars aren't bars of the song is too. Nothing is written.
#[test]
fn a_section_lands_on_whole_bars() {
    let (f, mut s) = song();
    let before = files(&f);
    let copy = section(s.accepted(), 32.0, 64.0).unwrap();
    let inside =
        "At bar 17, bars 9–16 would end inside bar 26, where the bars are another length: put them where they end at the end of a bar.";
    let words = refused(&mut s, Gesture::Paste { section: copy.clone(), at: 64.0 });
    assert!(words.ends_with(inside), "{words}");
    assert!(refused(&mut s, Gesture::Duplicate { start: 32.0, end: 64.0, to: None }).ends_with(inside));
    assert!(refused(&mut s, Gesture::Move { start: 32.0, end: 64.0, to: 64.0 }).ends_with(inside));
    let mut off = copy.clone();
    (off.start, off.end) = (33.0, 34.5);
    let words = refused(&mut s, Gesture::Paste { section: off, at: 0.0 });
    assert!(words.ends_with("A section starts at the start of a bar, not at bar 9 beat 2."), "{words}");
    let mut odd = copy;
    odd.end = f64::INFINITY;
    let words = refused(&mut s, Gesture::Paste { section: odd, at: 0.0 });
    assert!(words.ends_with("The copy's bars aren't a span of the song: copy them again."), "{words}");
    assert_eq!(files(&f), before);
    // Twelve bars of 4/4 from bar 9 end at beat 80, where the 3/4 starts: they fit.
    go(&mut s, Gesture::Duplicate { start: 32.0, end: 48.0, to: Some(64.0) });
}

/// A song of one track whose tone takes a choice and a switch from lanes of names (CARD-15): the
/// shape is a saw from beat 16, a square from beat 28; the sub is on from bar 17 (beat 64).
fn names_song() -> (Fixture, Session) {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write(
        "lib/tone.ts",
        "import { component, control, port } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\
         export const tone = component(\n  { id: \"tone\", controls: { shape: control.choice([\"saw\", \"square\", \"sine\"], \"saw\"), sub: control.bool(false) }, output: port.audio() },\n  () => osc.saw(110),\n);\n",
    );
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { tone } from \"./lib/tone\";\n\
         const lead = track(\"lead\", ({ lane }) => tone({ shape: lane.control(\"shape\", \"choice\", \"saw\"), sub: lane.control(\"sub\", \"switch\", false) }));\n\
         export default song(() => lead.out);\n",
    );
    f.write(
        "data/lead/shape.json",
        "{\"unit\": \"choice\", \"clips\": [\n  {\"id\": \"s0000001\", \"start\": 16, \"length\": 32, \"points\": [[0, \"saw\"], [12, \"square\"]]}\n]}\n",
    );
    f.write(
        "data/lead/sub.json",
        "{\"unit\": \"switch\", \"clips\": [\n  {\"id\": \"w0000001\", \"start\": 64, \"length\": 16, \"points\": [[0, true]]}\n]}\n",
    );
    let s = f.open();
    assert!(
        s.draft_errors.is_empty() && s.accepted().compiled.problems.iter().all(|d| !d.code.starts_with("lane.")),
        "{:?}",
        s.draft_errors
    );
    (f, s)
}

/// A lane of names' clips as `lane` writes them, its points `beat:value`.
fn steps(s: &Session, lane: &str) -> Vec<String> {
    let a = addr(lane);
    let Some(d) = s.accepted().compiled.data.lane(a.module.as_str(), a.lane.as_str()) else { return vec![] };
    let LaneData::Steps { clips, .. } = &**d else { panic!("{lane} isn't a lane of names") };
    let old = ["s0000001", "w0000001"];
    clips
        .iter()
        .map(|c| {
            let id = c.id.as_deref().filter(|i| old.contains(i)).unwrap_or("*");
            let points: Vec<String> = c.points.iter().map(|(b, v)| format!("{}:{}", n(*b), serde_json::to_string(v).unwrap())).collect();
            format!("{id} {}+{} [{}]", n(c.start), n(c.length), points.join(" "))
        })
        .collect()
}

// ARR-1, ARR-3, ARR-6 (review R1): lanes of names are part of every section. Insert bars moves
// the switch on with the rest, and splits the choice at the edge: its later part starts with the
// name in effect there, and its first part holds it over the new bars. Delete bars takes them out;
// a copy carries their clips and kinds, and a paste puts them in place of what was there.
#[test]
fn lanes_of_names_move_with_the_bars() {
    let (f, mut s) = names_song();
    let before = files(&f);
    go(&mut s, Gesture::InsertBars { at: 32.0, bars: 2 });
    assert_eq!(steps(&s, "lead/shape"), ["s0000001 16+24 [0:\"saw\" 12:\"square\"]", "* 40+16 [0:\"square\"]"]);
    assert_eq!(steps(&s, "lead/sub"), ["w0000001 72+16 [0:true]"]);
    s.undo().unwrap();
    assert_eq!(files(&f), before);
    go(&mut s, Gesture::DeleteBars { start: 32.0, end: 48.0 });
    assert_eq!(steps(&s, "lead/shape"), ["s0000001 16+16 [0:\"saw\" 12:\"square\"]"]);
    assert_eq!(steps(&s, "lead/sub"), ["w0000001 48+16 [0:true]"]);
    s.undo().unwrap();
    let copy = section(s.accepted(), 32.0, 48.0).unwrap();
    let kinds: Vec<&Holds> = copy.lanes.iter().map(|l| &l.kind).collect();
    let options = ["saw", "square", "sine"].map(String::from).to_vec();
    assert_eq!(kinds, [&Holds::Choice { options }, &Holds::Switch]);
    go(&mut s, Gesture::Paste { section: copy, at: 64.0 });
    assert_eq!(steps(&s, "lead/shape"), ["s0000001 16+32 [0:\"saw\" 12:\"square\"]", "* 64+16 [0:\"square\"]"]);
    assert!(steps(&s, "lead/sub").is_empty(), "the copy's sub was off there: {:?}", steps(&s, "lead/sub"));
    s.undo().unwrap();
    assert_eq!(files(&f), before);
}

// ARR-2 (review R2): a section reaches every lane, so while a lane file doesn't read (it plays the
// clips it had before), every section gesture is refused, naming the file, and nothing moves.
#[test]
fn a_lane_file_that_doesnt_read_refuses_them() {
    let (f, mut s) = song();
    f.write("data/hats/hits.json", "{\"clips\": [ {\"start\": 40, \"length\": 4, \"notes\": []} ,]}\n");
    s.refresh();
    let before = files(&f);
    let words = "data/hats/hits.json doesn't read right now, so its clips can't move with the bars: fix the file first.";
    for g in [
        Gesture::InsertBars { at: 32.0, bars: 2 },
        Gesture::Delete { start: 32.0, end: 64.0 },
        Gesture::Duplicate { start: 0.0, end: 32.0, to: None },
    ] {
        let said = refused(&mut s, g.clone());
        assert!(said.ends_with(words), "{g:?}: {said}");
    }
    assert_eq!(files(&f), before);
}

// ARR-6 (review R2, R6): while data/arrangement.json or layout.json doesn't read, insert and delete
// bars are refused in words naming it, and change nothing; the gestures that move clips only are
// still taken (WRITE-14).
#[test]
fn a_file_that_doesnt_read_refuses_the_time_gestures() {
    let (f, mut s) = song();
    f.write("data/arrangement.json", ARRANGEMENT.replace("\"bpm\": 140", "\"bpm\": 1400"));
    s.refresh();
    let before = files(&f);
    for g in [Gesture::InsertBars { at: 32.0, bars: 1 }, Gesture::DeleteBars { start: 32.0, end: 64.0 }] {
        let said = refused(&mut s, g.clone());
        assert!(said.ends_with("data/arrangement.json doesn't read right now, so its tempo and time signature changes can't move with the bars: fix the file first."), "{g:?}: {said}");
        assert!(!said.contains("compile"), "the code compiles: {said}");
    }
    assert_eq!(files(&f), before);
    go(&mut s, Gesture::Delete { start: 32.0, end: 64.0 });
    s.undo().unwrap();
    f.write("data/arrangement.json", ARRANGEMENT);
    s.refresh();
    f.write("layout.json", "{ not json");
    let before = files(&f);
    for g in [Gesture::InsertBars { at: 32.0, bars: 1 }, Gesture::DeleteBars { start: 32.0, end: 64.0 }] {
        let said = refused(&mut s, g.clone());
        assert!(
            said.ends_with("layout.json doesn't read right now, so its markers can't move with the bars: fix it, or reset the layout."),
            "{g:?}: {said}"
        );
    }
    assert_eq!(files(&f), before);
    let copy = section(s.accepted(), 32.0, 64.0).unwrap();
    go(&mut s, Gesture::Paste { section: copy, at: 0.0 });
    assert_eq!(f.read("layout.json"), "{ not json", "a paste never writes the layout");
}

/// The test song with `arrangement` as its tempo and time signatures.
fn with_arrangement(arrangement: &str) -> (Fixture, Session) {
    let (f, s) = song();
    drop(s);
    f.write("data/arrangement.json", arrangement);
    let s = f.open();
    (f, s)
}

/// An arrangement in its canonical form: `(beat, bpm)` and `(beat, beats, unit)`.
fn arrangement(tempo: &[(f64, f64)], meter: &[(f64, u32, u32)]) -> String {
    let a = m2_song::Arrangement {
        tempo: tempo.iter().map(|&(beat, bpm)| m2_song::Tempo { beat, bpm }).collect(),
        meter: meter.iter().map(|&(beat, beats, unit)| m2_song::Meter { beat, beats, unit }).collect(),
    };
    a.canonical()
}

// ARR-7 (review R6): the time signature half. Of the changes inside the deleted bars, the one in
// effect at their end lands at their start, unless it repeats what plays before it; one at their
// end edge moves to their start; and a 7/8 bar keeps its place in the count.
#[test]
fn delete_bars_keeps_what_follows_in_time() {
    // Three bars of 3/4 between 4/4s (bars 9 to 11): taking them out leaves the 4/4 alone.
    let (f, mut s) = with_arrangement(&arrangement(&[(0.0, 124.0)], &[(0.0, 4, 4), (32.0, 3, 4), (41.0, 4, 4)]));
    go(&mut s, Gesture::DeleteBars { start: 32.0, end: 41.0 });
    assert_eq!(f.read("data/arrangement.json"), arrangement(&[(0.0, 124.0)], &[(0.0, 4, 4)]));
    drop(s);
    // Bar 4 (4/4) and bar 5 (the first 7/8): the 7/8 and its tempo land at bar 4, and the 4/4 after
    // the second 7/8 bar follows it.
    let (f, mut s) = with_arrangement(&arrangement(&[(0.0, 124.0), (16.0, 90.0)], &[(0.0, 4, 4), (16.0, 7, 8), (23.0, 4, 4)]));
    go(&mut s, Gesture::DeleteBars { start: 12.0, end: 19.5 });
    assert_eq!(f.read("data/arrangement.json"), arrangement(&[(0.0, 124.0), (12.0, 90.0)], &[(0.0, 4, 4), (12.0, 7, 8), (15.5, 4, 4)]));
    drop(s);
    // A change at the end edge: bars 17 to 20 out, the 3/4 from bar 21 starts at bar 17.
    let (f, mut s) = with_arrangement(ARRANGEMENT);
    go(&mut s, Gesture::DeleteBars { start: 64.0, end: 80.0 });
    assert_eq!(f.read("data/arrangement.json"), arrangement(&[(0.0, 124.0), (48.0, 140.0), (64.0, 100.0)], &[(0.0, 4, 4), (64.0, 3, 4)]));
    assert_eq!(marks(&f), ["Verse@0", "Chorus@32", "Bridge@64"]);
}

// ARR-7 (review R10): when nothing follows the deleted bars, the changes and markers inside them go,
// and the song's own first tempo and time signature stay: the song isn't left at its last tempo.
#[test]
fn delete_bars_to_the_end() {
    let (f, mut s) = song();
    go(&mut s, Gesture::DeleteBars { start: 0.0, end: 200.0 });
    assert_eq!(f.read("data/arrangement.json"), arrangement(&[(0.0, 124.0)], &[(0.0, 4, 4)]));
    assert!(marks(&f).is_empty(), "{:?}", marks(&f));
    assert!(lane(&s, "kick/hits").is_empty() && lane(&s, "bass/cutoff").is_empty());
    s.undo().unwrap();
    // From bar 17 to the end: what plays before bar 17 stays as it was.
    go(&mut s, Gesture::DeleteBars { start: 64.0, end: 200.0 });
    assert_eq!(f.read("data/arrangement.json"), arrangement(&[(0.0, 124.0), (48.0, 140.0)], &[(0.0, 4, 4)]));
    assert_eq!(marks(&f), ["Verse@0", "Chorus@32"]);
}
