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
    out.insert("layout.json".into(), std::fs::read(f.0.join("layout.json")).unwrap());
    out
}

/// The markers as `layout.json` has them: `name@beat`.
fn marks(f: &Fixture) -> Vec<String> {
    markers::on_disk(&f.0).iter().map(|m| format!("{}@{}", m.name, n(m.beat))).collect()
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
    let words = refused(&mut s, Gesture::Delete { start: 33.0, end: 64.0 });
    assert!(words.contains("must be on a bar line: beat 33 is inside bar 9"), "{words}");
    let words = refused(&mut s, Gesture::Delete { start: 32.0, end: 32.0 });
    assert!(words.contains("ends after it starts"), "{words}");
    let words = refused(&mut s, Gesture::Delete { start: f64::NAN, end: 32.0 });
    assert!(words.contains("from the song's start on"), "{words}");
    // In 3/4 from bar 21 (beat 80), bar 22 starts at beat 83.
    let words = refused(&mut s, Gesture::InsertBars { at: 84.0, bars: 1 });
    assert!(words.contains("beat 84 is inside bar 22"), "{words}");
    let words = refused(&mut s, Gesture::Move { start: 32.0, end: 64.0, to: 97.0 });
    assert!(words.contains("on a bar line"), "{words}");
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
        (Gesture::Paste { section: copy, at: 64.0 }, false),
        (Gesture::Duplicate { start: 32.0, end: 64.0, to: None }, false),
        (Gesture::Move { start: 32.0, end: 64.0, to: 98.0 }, false),
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
    // A copy of the bars takes the audio clip's later part: it starts 4 beats later in its file.
    let copy = section(s.accepted(), 32.0, 64.0).unwrap();
    go(&mut s, Gesture::Paste { section: copy, at: 80.0 });
    assert_eq!(
        lane(&s, "vox/clips"),
        ["v0000001 28+8 offset 0 fades 0.01/0.02".to_string(), format!("* 80+4 offset {} fades 0/0.02", n(at124(4.0)))]
    );
    s.undo().unwrap();
    // An audio clip Split can't split refuses the gesture, in words.
    let pitched = f.read("data/vox/clips.json").replace("\"pitch\": 0", "\"pitch\": 2");
    f.write("data/vox/clips.json", pitched);
    drop(s);
    let mut s = f.open();
    let words = refused(&mut s, Gesture::Delete { start: 32.0, end: 64.0 });
    assert!(words.contains("its pitch is changed"), "{words}");
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
    let done = go(&mut s, Gesture::Paste { section: copy, at: 64.0 });
    assert_eq!(
        (done.edited.before.as_str(), done.edited.after.as_str()),
        ("bars 17–26", "a copy of bars 9–16"),
        "into the 3/4 from bar 21"
    );
    let pasted = [
        ("kick/hits", vec!["k0000001 0+32 [24@0+1 24@16+1]", "k0000002 32+32 [24@0+1 24@8+1]", "* 64+32 [24@0+1 24@8+1]"]),
        (
            "bass/notes",
            vec!["b0000001 24+16 [40@0+2 40@6+4 40@10+2]", "b0000002 56+8 [43@0+2 43@6+2]", "* 64+8 [40@2+2]", "* 88+8 [43@0+2 43@6+2]"],
        ),
        ("bass/cutoff", vec!["c0000001 16+48 [0:300 48:1725]", "* 64+32 [0:775 32:1725]"]),
    ];
    for (l, want) in &pasted {
        assert_eq!(lane(&s, l), *want, "{l}");
    }
    assert_eq!(
        lane(&s, "vox/clips"),
        ["v0000001 28+8 offset 0 fades 0.01/0.02".to_string(), format!("* 64+4 offset {} fades 0/0.02", n(at124(4.0)))]
    );
    // The app selects what was placed.
    assert_eq!(done.made.iter().filter(|(l, _)| *l == addr("bass/notes")).count(), 2, "{:?}", done.made);
    assert_eq!(f.read("data/arrangement.json"), ARRANGEMENT);
    s.undo().unwrap();
    go(&mut s, Gesture::Duplicate { start: 32.0, end: 64.0, to: None });
    for (l, want) in &pasted {
        assert_eq!(lane(&s, l), *want, "duplicate: {l}");
    }
}

// ARR-4, FMT-12: a move puts the bars at the bar line asked, keeps the id of a clip that was inside
// whole, and leaves a gap.
#[test]
fn a_move_leaves_a_gap_and_keeps_ids() {
    let (_f, mut s) = song();
    // Bar 27, in the 3/4.
    let done = go(&mut s, Gesture::Move { start: 32.0, end: 64.0, to: 98.0 });
    assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), ("bars 9–16", "bars 27–37"));
    assert_eq!(lane(&s, "kick/hits"), ["k0000001 0+32 [24@0+1 24@16+1]", "k0000003 64+16 [24@0+1]", "k0000002 98+32 [24@0+1 24@8+1]"]);
    assert_eq!(
        lane(&s, "bass/notes"),
        ["b0000001 24+8 [40@0+2 40@6+2]", "* 64+8 [43@4+2]", "* 98+8 [40@2+2]", "b0000002 122+8 [43@0+2 43@6+2]"]
    );
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
    let mut gone = copy.clone();
    gone.lanes.push(SectionLane { lane: addr("lead/notes"), clips: vec![] });
    let words = refused(&mut s, Gesture::Paste { section: gone, at: 32.0 });
    assert!(words.contains("The copy has lead's notes lane, which the song doesn't have now"), "{words}");
    let mut other = copy.clone();
    let notes = other.lanes.iter().find(|l| l.lane == addr("kick/hits")).unwrap().clips.clone();
    other.lanes.iter_mut().find(|l| l.lane == addr("bass/cutoff")).unwrap().clips = notes;
    let words = refused(&mut s, Gesture::Paste { section: other, at: 32.0 });
    assert!(words.contains("The copy has notes clips for Bass's cutoff lane, which holds automation now."), "{words}");
    let words = refused(&mut s, Gesture::Paste { section: copy, at: 33.0 });
    assert!(words.contains("on a bar line"), "{words}");
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
    assert_eq!(lane(&s, "bass/cutoff"), ["c0000001 16+16 [0:300 16:775]", "* 40+48 [0:775 48:2200]"]);
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
    assert_eq!((s.last_step(), files(&f)), (steps, before));
}
