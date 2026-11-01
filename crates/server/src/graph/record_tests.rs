//! Recording on the server (lane B): a take on a driven player, whose callbacks the test runs, so
//! every note's place is known: the engine clock the speakers play is the published one less two
//! buffers of 480 frames (D4). 120 bpm: a beat is 24,000 frames, 50 callbacks.

use std::time::Duration;

use m2_editor::clips::{ClipRef, Gesture};
use m2_play::{Driver, Player};
use m2_song::LaneAddr;

use super::super::audition::{Audition, Strike};
use super::super::midi::Event;
use super::super::tests::Fixture;
use super::*;

const SONG: &str = "import { song, track } from '@m2/core';\nimport { drums } from '@m2/drums';\nimport { sum } from 'std/math';\n\
                    const b = track('bass', { name: 'Bass' }, ({ lane }) => drums.drum(lane.notes('notes')));\n\
                    const k = track('keys', { name: 'Keys' }, ({ lane }) => drums.drum(lane.notes('notes')));\n\
                    export default song(() => sum(b.out, k.out));\n";
/// Frames a beat lasts at 120 bpm, and a callback's.
const BEAT: f64 = 24_000.0;
const CALLBACK: usize = 480;
/// What a note's place may be off by besides when it was struck: a frame's rounding.
const SLACK: f64 = 2.0 / BEAT;

struct Rig {
    fixture: Fixture,
    graph: Graph,
    d: Driver,
    token: u64,
    /// The engine clock where the transport was at a beat: the take's anchor, as the test knows it.
    anchor: (u64, f64),
    /// When the last callback began, on `m2_play::now_ns`'s clock, or a little before.
    filled: u64,
}

/// When a key was heard: from where the speakers were as the last callback began, to where they
/// were once the key was struck (the test's own time between them counts, as `heard_frame` counts
/// it).
#[derive(Clone, Copy, Debug)]
struct Heard {
    lo: f64,
    hi: f64,
}

impl Heard {
    fn has(&self, beat: f64) -> bool {
        self.lo - SLACK <= beat && beat <= self.hi + SLACK
    }

    /// The window shifted by `by` beats: a place in a clip that starts there.
    fn less(self, by: f64) -> Heard {
        Heard { lo: self.lo - by, hi: self.hi - by }
    }
}

impl Rig {
    fn new() -> Rig {
        Rig::with(&[])
    }

    /// A rig whose song also has `files` (a path in the project, and its text).
    fn with(files: &[(&str, &str)]) -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", SONG);
        for (path, text) in files {
            fixture.put(path, text);
        }
        let (p, d) = Player::driven(48_000);
        let graph = Graph::with_player(p).stored();
        let token = graph.open(&fixture.0).unwrap()["session"].as_u64().unwrap();
        let mut rig = Rig { fixture, graph, d, token, anchor: (0, 0.0), filled: 0 };
        rig.fill(4);
        assert_eq!(rig.graph.midi(Event::Plugged("Keys")).as_deref(), Some("Keys is connected. It plays the selected track."));
        rig.graph.midi_target(Some(("bass".into(), "notes".into()))).unwrap();
        rig
    }

    /// Run `n` device callbacks.
    fn fill(&mut self, n: usize) {
        let mut buf = [0.0f32; 2 * CALLBACK];
        for _ in 0..n {
            self.filled = m2_play::now_ns();
            self.d.fill(&mut buf, 2);
            drop(self.d.drain_retired());
        }
    }

    /// From the next block on, the transport is at `from` beats: a Play or a count sent now.
    fn anchored(&mut self, plus: u64, from: f64) {
        self.anchor = (self.graph.engine_clock().unwrap() + plus, from);
    }

    /// The beat the speakers play as the last callback began, by the engine clock, which never
    /// wraps (it counts on through a loop's end and through a count).
    fn by_clock(&self) -> f64 {
        let heard = self.graph.engine_clock().unwrap() as f64 - 2.0 * CALLBACK as f64;
        self.anchor.1 + (heard - self.anchor.0 as f64) / BEAT
    }

    /// The same, exact: by the transport's frame while it plays.
    fn at(&self) -> f64 {
        let st = self.graph.status();
        if st.playing { st.beat - 2.0 * CALLBACK as f64 / BEAT } else { self.by_clock() }
    }

    /// Callbacks until the speakers play `beat` or later, by the clock.
    fn to(&mut self, beat: f64) {
        while self.by_clock() < beat {
            self.fill(1);
        }
    }

    fn start(&self, count_in: bool, quantize: Option<f64>) -> Value {
        self.graph.record_start(self.token, "bass", "notes", Options { count_in, quantize, ..Default::default() }).unwrap()
    }

    /// Keyboard "Keys" sends `bytes`: when it was heard.
    fn key(&self, bytes: &[u8]) -> Heard {
        let lo = self.at();
        assert_eq!(self.graph.midi(Event::Bytes("Keys", bytes)), None);
        let waited = m2_play::now_ns().saturating_sub(self.filled) as f64 * 48_000.0 / 1e9;
        Heard { lo, hi: lo + waited / BEAT }
    }

    /// The take thread's tick, until a take is written or found empty: what it said.
    fn written(&self) -> Value {
        let t = std::time::Instant::now();
        loop {
            if let Some(v) = self
                .graph
                .record_tick()
                .into_iter()
                .find(|v| v.get("written").is_some() || v.get("empty").is_some() || v.get("error").is_some())
            {
                return v;
            }
            assert!(t.elapsed() < Duration::from_secs(5), "no take was written");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The lane's clips as written.
    fn clips(&self) -> Vec<Value> {
        let text = std::fs::read_to_string(self.fixture.0.join("data/bass/notes.json")).unwrap_or_else(|_| "{\"clips\": []}".into());
        serde_json::from_str::<Value>(&text).unwrap()["clips"].as_array().unwrap().clone()
    }

    fn revision(&self) -> u64 {
        self.graph.state().unwrap()["view"]["revision"].as_u64().unwrap()
    }
}

/// A note of a clip: its pitch, start, length and velocity.
fn note(c: &Value, i: usize) -> (f64, f64, f64, f64) {
    let n = &c["notes"][i];
    let pitch = n["pitch"].as_f64().or_else(|| m2_song::parse_pitch(n["pitch"].as_str()?)).unwrap();
    (pitch, n["start"].as_f64().unwrap(), n["length"].as_f64().unwrap(), n["velocity"].as_f64().unwrap())
}

#[test]
fn a_take_records_midi_notes_where_they_were_heard() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    let shown = r.start(false, None);
    assert_eq!((&shown["phase"], &shown["start"], &shown["from"]), (&json!("take"), &json!(0.0), &json!(0.0)));
    r.to(1.0);
    let on = r.key(&[0x90, 60, 100]);
    r.to(1.5);
    let off = r.key(&[0x80, 60, 0]);
    r.to(2.25);
    let on2 = r.key(&[0x90, 64, 64]);
    r.to(2.5);
    let off2 = r.key(&[0x90, 64, 0]);
    r.to(3.0);
    r.graph.transport("stop").unwrap();
    let said = r.written();
    assert!(said["written"]["revision"].is_u64(), "{said}");
    let clips = r.clips();
    assert_eq!(clips.len(), 1);
    let c = &clips[0];
    assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(0.0), Some(4.0)), "one bar");
    let (pitch, start, length, velocity) = note(c, 0);
    assert_eq!(pitch, 60.0);
    assert!(on.has(start) && off.has(start + length), "heard at {on:?} to {off:?}, written {start} for {length}");
    assert_eq!(velocity, 100.0 / 127.0, "velocity as played");
    let (pitch, start, length, velocity) = note(c, 1);
    assert_eq!((pitch, velocity), (64.0, 64.0 / 127.0));
    assert!(on2.has(start) && off2.has(start + length), "heard at {on2:?} to {off2:?}, written {start} for {length}");
    assert_eq!(said["written"]["made"][0]["id"], c["id"], "the clip made, for the window to select");
    assert!(!r.graph.status().playing, "the stop stopped");
}

// RENDER-36: the pedal isn't recorded, but a take keeps each note as it sounded: a key let go under
// the pedal ends where the pedal lifts, or where the key is struck again; one let go with the pedal
// up ends at its Off.
#[test]
fn a_take_keeps_a_note_the_pedal_held() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(0.5);
    let on = r.key(&[0x90, 60, 100]);
    r.key(&[0xB0, 64, 127]);
    r.to(1.0);
    r.key(&[0x80, 60, 0]);
    r.to(1.5);
    let up = r.key(&[0xB0, 64, 0]);
    r.to(2.0);
    r.key(&[0xB0, 64, 127]);
    let on2 = r.key(&[0x90, 62, 100]);
    r.to(2.25);
    r.key(&[0x80, 62, 0]);
    r.to(2.75);
    let again = r.key(&[0x90, 62, 90]);
    r.to(3.0);
    r.key(&[0xB0, 64, 0]);
    let off3 = r.key(&[0x80, 62, 0]);
    r.to(3.5);
    r.graph.transport("stop").unwrap();
    r.written();
    let c = &r.clips()[0];
    let (pitch, start, length, _) = note(c, 0);
    assert_eq!(pitch, 60.0);
    assert!(on.has(start) && up.has(start + length), "held to the pedal's lift: {on:?} to {up:?}, written {start} for {length}");
    let (pitch, start, length, _) = note(c, 1);
    assert_eq!(pitch, 62.0);
    assert!(
        on2.has(start) && again.has(start + length),
        "held to the key struck again: {on2:?} to {again:?}, written {start} for {length}"
    );
    let (_, start, length, velocity) = note(c, 2);
    assert!(again.has(start) && off3.has(start + length) && velocity == 90.0 / 127.0, "the new one, to its Off: {start} for {length}");
    assert_eq!(c["notes"].as_array().unwrap().len(), 3, "no pedal in the lane");
}

// RENDER-36 (review E8): a Stop lets the pedal up for the take as for the lane. The foot still down,
// a key let go after it ends at its Off in the next take, as the voices end it there, not where the
// foot at last lifts.
#[test]
fn a_stop_lets_the_takes_pedal_up_too() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(0.5);
    r.key(&[0xB0, 64, 127]);
    r.to(1.0);
    r.graph.transport("stop").unwrap();
    assert!(r.written().get("empty").is_some(), "the first take holds no note");
    r.fill(10);
    let from = r.graph.status().beat;
    r.anchored(0, from);
    r.start(false, None);
    r.to(from + 0.5);
    let on = r.key(&[0x90, 60, 100]);
    r.to(from + 1.0);
    let off = r.key(&[0x80, 60, 0]);
    r.to(from + 2.0);
    r.key(&[0xB0, 64, 0]);
    r.to(from + 2.5);
    r.graph.transport("stop").unwrap();
    assert!(r.written()["written"].is_object());
    let c = &r.clips()[0];
    let (pitch, start, length, _) = note(c, 0);
    let at = c["start"].as_f64().unwrap();
    assert_eq!(pitch, 60.0);
    assert!(on.less(at).has(start) && off.less(at).has(start + length), "to its Off: {on:?} to {off:?}, written {start} for {length}");
}

// D4: a note goes where it was heard between two callbacks, not on the callback's line.
#[test]
fn notes_land_between_callbacks() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    let line = r.at();
    std::thread::sleep(Duration::from_millis(5));
    r.key(&[0x90, 60, 100]);
    r.fill(10);
    r.key(&[0x80, 60, 0]);
    r.graph.record_stop(r.token).unwrap();
    r.written();
    let start = note(&r.clips()[0], 0).1;
    assert!(start - line >= 4.5e-3 * 48_000.0 / BEAT, "5 ms after the callback's line {line}: written at {start}");
}

#[test]
fn a_quick_tap_still_records_a_note() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    r.key(&[0x90, 60, 100]);
    r.key(&[0x80, 60, 100]);
    r.fill(10);
    r.graph.transport("stop").unwrap();
    r.written();
    assert_eq!(note(&r.clips()[0], 0).2, SHORTEST, "struck and let go in no time: 1/64 beat");
}

#[test]
fn piano_notes_marked_record_are_recorded_and_audition_is_not() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    let strike = |client: &str, h: u32, pitch: f64, record: bool| {
        let a = Audition {
            module: Some("bass".into()),
            lane: Some("notes".into()),
            on: vec![Strike { h, pitch, velocity: 0.5 }],
            record,
            ..Default::default()
        };
        r.graph.audition(client, a).unwrap();
    };
    strike("piano", 1, 62.0, true);
    strike("notes-click", 1, 67.0, false);
    r.fill(20);
    r.graph.audition("piano", Audition { off: vec![1], ..Default::default() }).unwrap();
    r.graph.audition("notes-click", Audition { off: vec![1], ..Default::default() }).unwrap();
    r.fill(5);
    r.graph.transport("stop").unwrap();
    r.written();
    let c = &r.clips()[0];
    assert_eq!(c["notes"].as_array().unwrap().len(), 1, "{c}");
    assert_eq!(note(c, 0).0, 62.0, "the piano's note, not Notes' click");
}

#[test]
fn stop_writes_one_tx_named_recording_on_bass() {
    let mut r = Rig::new();
    let before = r.revision();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(0.5);
    r.key(&[0x90, 48, 90]);
    r.to(1.0);
    r.key(&[0x80, 48, 0]);
    r.graph.transport("stop").unwrap();
    let said = r.written();
    assert_eq!(said["written"]["revision"].as_u64(), Some(before + 1), "one Tx");
    assert_eq!(r.graph.undo_names().0.as_deref(), Some("recording on Bass"));
    assert_eq!(r.clips().len(), 1);
    r.graph.undo(r.token).unwrap();
    assert!(r.clips().is_empty(), "one Undo takes it back");
}

#[test]
fn held_notes_end_at_the_stop() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    let on = r.key(&[0x90, 60, 100]);
    r.to(2.5);
    let lo = r.at();
    r.graph.transport("stop").unwrap();
    let stop = Heard { lo, hi: r.key(&[]).hi };
    r.written();
    let (_, start, length, _) = note(&r.clips()[0], 0);
    assert!(on.has(start) && stop.has(start + length), "held to the stop at {stop:?}: {start} for {length}");
}

#[test]
fn the_take_spans_whole_bars() {
    let mut r = Rig::new();
    r.graph.seek(6.0).unwrap();
    r.fill(2);
    r.anchored(0, 6.0);
    let shown = r.start(false, None);
    assert_eq!((&shown["start"], &shown["from"]), (&json!(4.0), &json!(6.0)), "from bar 2's line");
    r.to(7.0);
    let on = r.key(&[0x90, 60, 100]);
    r.to(7.5);
    r.key(&[0x80, 60, 100]);
    r.to(9.5);
    r.graph.transport("stop").unwrap();
    r.written();
    let c = &r.clips()[0];
    assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(4.0), Some(8.0)), "bars 2 and 3: to the line after the stop");
    assert!(on.less(4.0).has(note(c, 0).1), "a note's start is from the clip's");
}

#[test]
fn an_empty_take_writes_nothing() {
    let mut r = Rig::new();
    let before = r.revision();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(2.0);
    r.graph.transport("stop").unwrap();
    assert_eq!(r.written(), json!({"t": "take", "empty": true}));
    assert_eq!(r.revision(), before);
    assert!(r.clips().is_empty());
}

// Q26: snapped as Quantize snaps (ends: false): starts to the grid, lengths kept.
#[test]
fn quantize_puts_starts_on_the_grid() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    for (on, off, pitch) in [(1.07, 1.3, 60u8), (1.6, 1.8, 62), (2.95, 3.4, 64)] {
        r.to(on);
        r.key(&[0x90, pitch, 100]);
        r.to(off);
        r.key(&[0x80, pitch, 100]);
    }
    r.graph.transport("stop").unwrap();
    r.written();
    let c = r.clips()[0].clone();
    let mut mine: Vec<(f64, f64)> = (0..3).map(|i| (note(&c, i).1, note(&c, i).2)).collect();
    quantized(&mut mine, 0.0, 0.25);
    let lane = LaneAddr::new("bass", "notes").unwrap();
    let clip = ClipRef { lane: lane.clone(), id: c["id"].as_str().map(String::from), at: None, sha: None };
    r.graph.clips(r.token, Gesture::Quantize { clips: vec![clip], notes: None, grid: 0.25, ends: false }).unwrap();
    let q = r.clips()[0].clone();
    let theirs: Vec<(f64, f64)> = (0..3).map(|i| (note(&q, i).1, note(&q, i).2)).collect();
    assert_eq!(mine, theirs, "the take's rounding is Quantize's");
    assert_eq!(theirs.iter().map(|n| n.0).collect::<Vec<_>>(), [1.0, 1.5, 3.0]);
    // Recorded with the grid on: written so at once.
    r.graph.undo(r.token).unwrap();
    r.graph.undo(r.token).unwrap();
    r.graph.seek(0.0).unwrap();
    r.fill(2);
    r.anchored(0, 0.0);
    r.graph.transport("play").unwrap();
    r.fill(2);
    r.start(false, Some(0.25));
    r.to(1.07);
    let on = r.key(&[0x90, 60, 100]);
    r.to(1.3);
    let off = r.key(&[0x80, 60, 100]);
    r.graph.transport("stop").unwrap();
    r.written();
    let (_, start, length, _) = note(&r.clips()[0], 0);
    assert_eq!(start, 1.0);
    assert!(length >= off.lo - on.hi - SLACK && length <= off.hi - on.lo + SLACK, "its length kept: {length}");
}

// MON-11 (was Q27's first pass only): at the loop's end the first pass is written, the loop plays
// on, and the take goes round, recording the next pass.
#[test]
fn a_loop_wrap_writes_the_first_pass() {
    let mut r = Rig::new();
    r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
    r.anchored(0, 0.0);
    let shown = r.start(false, None);
    assert_eq!(shown["until"], json!(4.0));
    r.to(1.0);
    r.key(&[0x90, 60, 100]);
    r.to(2.0);
    r.key(&[0x80, 60, 100]);
    r.to(4.2);
    let said = r.written();
    assert!(said["written"].is_object(), "{said}");
    assert!(r.graph.status().playing, "play goes on");
    assert!(r.graph.status().beat < 1.0, "round the loop");
    let c = &r.clips()[0];
    assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(0.0), Some(4.0)));
    assert!(r.graph.recording(), "recording the next pass");
    assert_eq!(r.graph.state().unwrap()["record"]["pass"], json!(1));
}

// D5: a note struck just after the wrap is no part of the take, and never lands at its start.
#[test]
fn a_note_after_the_wrap_is_not_put_at_the_start() {
    let mut r = Rig::new();
    r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(2.0);
    r.key(&[0x90, 60, 100]);
    r.key(&[0x80, 60, 100]);
    r.to(4.1);
    // Before the take thread looks: the key comes first.
    r.key(&[0x90, 72, 100]);
    r.key(&[0x80, 72, 100]);
    r.written();
    let c = &r.clips()[0];
    assert_eq!(c["notes"].as_array().unwrap().len(), 1, "{c}");
    assert_eq!(note(c, 0).0, 60.0);
}

#[test]
fn a_note_held_over_the_wrap_ends_at_the_loop_end() {
    let mut r = Rig::new();
    r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(3.0);
    let on = r.key(&[0x90, 60, 100]);
    r.to(4.5);
    r.written();
    r.key(&[0x80, 60, 100]);
    let (_, start, length, _) = note(&r.clips()[0], 0);
    assert!(on.has(start), "{on:?}: {start}");
    assert!((start + length - 4.0).abs() < 1e-9, "ends at the loop's end: {start} + {length}");
}

// D5: a click on the ruler is a jump: the take ends where it was heard, and play goes on there.
#[test]
fn a_click_on_the_ruler_ends_the_take() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    let on = r.key(&[0x90, 60, 100]);
    r.to(2.0);
    let lo = r.at();
    r.graph.seek(12.0).unwrap();
    let jump = Heard { lo, hi: r.key(&[]).hi };
    r.fill(5);
    r.key(&[0x80, 60, 100]);
    r.written();
    let (_, start, length, _) = note(&r.clips()[0], 0);
    assert!(on.has(start) && jump.has(start + length), "held to the jump at {jump:?}: {start} + {length}");
    assert!(r.graph.status().playing && r.graph.status().beat >= 12.0);
}

// Q20: stopped at bar 2, a bar of clicks, then recording from bar 2 with the song.
#[test]
fn a_count_in_then_record_starts_at_the_start_point() {
    let mut r = Rig::new();
    r.graph.seek(4.0).unwrap();
    r.fill(2);
    // The song starts a bar after the count: 96,000 frames, a whole number of blocks.
    r.anchored(96_000, 4.0);
    let shown = r.start(true, None);
    assert_eq!((&shown["phase"], &shown["count_in"]), (&json!("count"), &json!({"beats": 4, "ms": 2000.0})));
    assert_eq!(r.graph.state().unwrap()["record"]["phase"], json!("count"));
    r.fill(100);
    assert!(!r.graph.status().playing, "counting: nothing plays");
    let ticks = r.graph.record_tick();
    assert!(ticks.iter().all(|t| t["phase"] != json!("take")), "{ticks:?}");
    r.to(4.1);
    let said = r.graph.record_tick();
    assert!(said.iter().any(|t| t["phase"] == json!("take")), "the windows hear it records: {said:?}");
    assert!(r.graph.status().playing);
    r.to(4.5);
    let on = r.key(&[0x90, 60, 100]);
    r.to(5.0);
    r.key(&[0x80, 60, 100]);
    r.graph.transport("stop").unwrap();
    r.written();
    let c = &r.clips()[0];
    assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(4.0), Some(4.0)));
    assert!(on.less(4.0).has(note(c, 0).1), "{on:?}: {c}");
}

// D27: a note struck in the count's last beat starts on the take's first beat; earlier ones are
// heard, not kept.
#[test]
fn a_note_in_the_counts_last_beat_lands_on_the_first_beat() {
    let mut r = Rig::new();
    r.anchored(96_000, 0.0);
    r.start(true, None);
    r.to(-2.5);
    r.key(&[0x90, 50, 100]);
    r.key(&[0x80, 50, 100]);
    r.to(-0.5);
    r.key(&[0x90, 60, 100]);
    r.to(0.5);
    let off = r.key(&[0x80, 60, 100]);
    r.graph.transport("stop").unwrap();
    r.written();
    let c = &r.clips()[0];
    assert_eq!(c["notes"].as_array().unwrap().len(), 1, "the early one isn't kept: {c}");
    let (pitch, start, length, _) = note(c, 0);
    assert_eq!((pitch, start), (60.0, 0.0), "on the first beat");
    assert!(off.has(length), "to its Off at {off:?}: {length}");
}

#[test]
fn a_keyboard_turned_off_neither_plays_nor_records() {
    let mut r = Rig::new();
    r.graph.set_midi_off(&["Keys".into()]);
    assert_eq!(r.graph.midi_state()["off"], json!(["Keys"]));
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    r.key(&[0x90, 60, 100]);
    r.fill(5);
    assert_eq!(r.graph.activity().live, 0, "it plays nothing");
    r.key(&[0x80, 60, 100]);
    r.graph.transport("stop").unwrap();
    assert_eq!(r.written(), json!({"t": "take", "empty": true}), "and records nothing");
    r.graph.set_midi_off(&[]);
    assert_eq!(r.graph.midi_state().get("off"), None);
    r.key(&[0x90, 60, 100]);
    r.fill(5);
    assert_eq!(r.graph.activity().live, 1, "on again, it plays");
    // Plugged in while off, it says so.
    r.graph.set_midi_off(&["Pads".into()]);
    assert_eq!(r.graph.midi(Event::Plugged("Pads")).as_deref(), Some("Pads is connected. It is turned off in Settings."));
}

#[test]
fn record_needs_a_notes_lane() {
    let r = Rig::new();
    let e = r.graph.record_start(r.token, "bass", "nope", Options::default()).unwrap_err();
    assert_eq!(e.to_string(), "Select a notes track or an audio track to record on it.");
    assert!(!r.graph.recording());
    r.start(false, None);
    let e = r.graph.record_start(r.token, "bass", "notes", Options::default()).unwrap_err();
    assert_eq!(e.to_string(), "A take is recording already.");
}

#[test]
fn closing_the_song_drops_the_take() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    r.key(&[0x90, 60, 100]);
    r.fill(10);
    r.graph.close();
    let said = r.graph.record_tick();
    assert_eq!(said, vec![json!({"t": "take", "phase": null})], "nothing written");
    assert!(!r.graph.recording());
    assert!(r.clips().is_empty());
}

// D8: while a take records, a window that selects another track doesn't move the keys.
#[test]
fn the_keys_stay_on_the_takes_lane() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.graph.midi_target(Some(("keys".into(), "notes".into()))).unwrap();
    r.to(1.0);
    r.key(&[0x90, 60, 100]);
    r.to(1.5);
    r.key(&[0x80, 60, 100]);
    r.graph.record_stop(r.token).unwrap();
    r.written();
    assert_eq!(r.clips().len(), 1, "recorded on Bass");
    // After the take, the window's choice plays.
    assert_eq!(r.graph.midi_state()["target"], json!({"module": "keys", "lane": "notes"}));
}

// D6: Space while counting in cancels the count, and nothing plays; R or Esc too (cancel).
#[test]
fn space_while_counting_cancels_and_nothing_plays() {
    let mut r = Rig::new();
    r.start(true, None);
    r.fill(30);
    r.graph.transport("toggle").unwrap();
    r.fill(300);
    assert!(!r.graph.status().playing, "nothing plays");
    assert!(!r.graph.recording());
    r.start(true, None);
    r.fill(30);
    r.graph.record_cancel(r.token).unwrap();
    r.fill(300);
    assert!(!r.graph.status().playing && !r.graph.recording());
}

// R while playing: from now, with no count; R again ends it and play goes on.
#[test]
fn r_while_playing_records_from_now() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.graph.transport("play").unwrap();
    r.to(5.0);
    let shown = r.start(true, None);
    assert_eq!((&shown["phase"], &shown["start"]), (&json!("take"), &json!(4.0)), "no count; from bar 2's line");
    r.to(6.0);
    let on = r.key(&[0x90, 60, 100]);
    r.to(6.5);
    r.key(&[0x80, 60, 100]);
    r.graph.record_stop(r.token).unwrap();
    r.written();
    assert!(r.graph.status().playing, "play goes on");
    let c = &r.clips()[0];
    assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(4.0), Some(4.0)));
    assert!(on.less(4.0).has(note(c, 0).1), "heard at {on:?}: {c}");
}

// Guard (D7): ending a take never waits for the session: the actor held by a slow job, a Stop
// answers at once, and the take is written once the actor is free.
#[test]
fn stopping_never_waits_for_a_compile() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    r.key(&[0x90, 60, 100]);
    r.fill(10);
    r.key(&[0x80, 60, 100]);
    let (held, release) = std::sync::mpsc::channel::<()>();
    let jobs = r.graph.jobs.clone();
    jobs.send(Box::new(move |_| {
        let _ = release.recv_timeout(Duration::from_secs(10));
    }))
    .unwrap();
    let t = std::time::Instant::now();
    r.graph.transport("stop").unwrap();
    assert!(t.elapsed() < Duration::from_millis(200), "the stop waited {:?}", t.elapsed());
    assert!(!r.graph.status().playing);
    drop(held);
    r.written();
    assert_eq!(r.clips().len(), 1);
}

// A jump the audio thread hasn't taken yet: R says to press again, rather than record from where
// the song was.
#[test]
fn r_right_after_a_jump_asks_again() {
    let mut r = Rig::new();
    r.graph.transport("play").unwrap();
    r.fill(10);
    r.graph.seek(8.0).unwrap();
    let e = r.graph.record_start(r.token, "bass", "notes", Options::default()).unwrap_err();
    assert_eq!(e.to_string(), "The song is moving to another place. Press Record again.");
    assert!(!r.graph.recording());
    // Once the speakers play the new place (two buffers on), R records from there.
    r.fill(5);
    let shown = r.start(false, None);
    assert_eq!(shown["start"], json!(8.0), "taken: from bar 3");
}

// The review's R1: with the code broken, R is refused in the page's words, before anything plays.
#[test]
fn record_is_refused_while_the_code_doesnt_compile() {
    let r = Rig::new();
    r.fixture.put("song.ts", "(");
    assert!(r.graph.poll());
    let e = r.graph.record_start(r.token, "bass", "notes", Options { count_in: true, quantize: None, ..Default::default() }).unwrap_err();
    assert_eq!(e.to_string(), "Fix the code first: it doesn't compile right now.");
    assert!(!r.graph.recording() && !r.graph.status().playing, "nothing counts in, nothing plays");
}

// The review's R1: the code broken while a take records. The take waits, said in words (never "Bass
// is gone"), and is written once the code compiles again.
#[test]
fn a_take_that_ends_while_the_code_is_broken_waits_for_the_code() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    let on = r.key(&[0x90, 60, 100]);
    r.to(1.5);
    r.key(&[0x80, 60, 0]);
    r.fixture.put("song.ts", "(");
    assert!(r.graph.poll());
    r.graph.transport("stop").unwrap();
    let said = r.graph.record_tick();
    assert!(said.iter().all(|v| v.get("error").is_none() && v.get("written").is_none()), "{said:?}");
    let waits: Vec<&str> = said.iter().filter_map(|v| v["waiting"].as_str()).collect();
    assert_eq!(waits, ["The recording waits for the code. Fix the code, and it goes on Bass."], "{said:?}");
    assert!(r.graph.record_tick().iter().all(|v| v.get("waiting").is_none()), "said once");
    assert!(r.clips().is_empty());
    r.fixture.put("song.ts", SONG);
    assert!(r.graph.poll());
    let said = r.written();
    assert!(said["written"].is_object(), "{said}");
    let c = &r.clips()[0];
    assert_eq!(c["notes"].as_array().unwrap().len(), 1, "{c}");
    assert!(on.has(note(c, 0).1), "heard at {on:?}: {c}");
}

// The review's R2: the loop turned on while a take records. The first pass ends at the loop's end,
// and a note after the wrap is never put past it.
#[test]
fn a_loop_turned_on_during_a_take_ends_it_at_the_loops_end() {
    let mut r = Rig::new();
    r.graph.set_loop(Some((0.0, 4.0)), Some(false)).unwrap();
    r.anchored(0, 0.0);
    let shown = r.start(false, None);
    assert_eq!(shown["until"], Value::Null, "no loop yet");
    r.to(1.0);
    r.graph.set_loop(None, Some(true)).unwrap();
    r.to(2.0);
    r.key(&[0x90, 60, 100]);
    r.key(&[0x80, 60, 100]);
    // By the clock, beat 4.5: the song is half a beat into its second pass.
    r.to(4.5);
    assert!(r.graph.status().beat < 1.0, "round the loop");
    r.key(&[0x90, 72, 100]);
    r.key(&[0x80, 72, 100]);
    let said = r.written();
    assert!(said["written"].is_object(), "{said}");
    let c = &r.clips()[0];
    assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(0.0), Some(4.0)), "the first pass: {c}");
    assert_eq!(c["notes"].as_array().unwrap().len(), 1, "the note after the wrap isn't kept: {c}");
    assert!(r.graph.status().playing, "the loop plays on");
}

// The review's R2: the loop turned off while a take records. The take goes on past the old end.
#[test]
fn a_loop_turned_off_during_a_take_lets_it_go_on() {
    let mut r = Rig::new();
    r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
    r.anchored(0, 0.0);
    assert_eq!(r.start(false, None)["until"], json!(4.0));
    r.to(1.0);
    r.graph.set_loop(None, Some(false)).unwrap();
    r.to(4.5);
    let said = r.graph.record_tick();
    assert!(
        r.graph.recording() && said.iter().all(|v| v.get("written").is_none() && v.get("empty").is_none()),
        "still recording past the old end: {said:?}"
    );
    let on = r.key(&[0x90, 60, 100]);
    r.to(5.0);
    r.key(&[0x80, 60, 100]);
    r.graph.transport("stop").unwrap();
    r.written();
    let c = &r.clips()[0];
    assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(0.0), Some(8.0)), "{c}");
    assert!(on.has(note(c, 0).1), "heard at {on:?}: {c}");
}

// The review's R4: the count's last beat is one beat of the count's own clicks, even where the tempo
// changes at the start point. 120 BPM before beat 8, 60 from it: the count clicks once a second.
#[test]
fn the_counts_last_beat_is_measured_in_time() {
    let tempo = "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}, {\"beat\": 8, \"bpm\": 60}], \"meter\": []}\n";
    let mut r = Rig::with(&[("data/arrangement.json", tempo)]);
    r.graph.seek(8.0).unwrap();
    r.fill(2);
    // The count: four clicks of 48,000 frames. The rig's beats are 24,000 frames (120 BPM), so the
    // downbeat is "beat 8" and a second before it "beat 6".
    r.anchored(192_000, 8.0);
    r.start(true, None);
    // 1.1 s before the downbeat: in the third click. Heard, not kept.
    r.to(8.0 - 2.2);
    r.key(&[0x90, 50, 100]);
    r.key(&[0x80, 50, 100]);
    // 0.9 s before: in the last click. On the take's first beat.
    r.to(8.0 - 1.8);
    r.key(&[0x90, 60, 100]);
    r.key(&[0x80, 60, 100]);
    r.to(8.5);
    r.graph.transport("stop").unwrap();
    r.written();
    let c = &r.clips()[0];
    assert_eq!(c["notes"].as_array().unwrap().len(), 1, "only the last click's note: {c}");
    let (pitch, start, _, _) = note(c, 0);
    assert_eq!((pitch, start), (60.0, 0.0), "on the first beat: {c}");
}

// The review's R5: a new player (another output, a fallback) ends the take where it was last heard,
// not at its start: a note held then keeps its length.
#[test]
fn a_new_player_ends_the_take_where_it_was_last_heard() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    let on = r.key(&[0x90, 60, 100]);
    r.to(2.5);
    let last = r.at();
    // The take thread looks, as it does every 100 ms; then the output changes.
    r.graph.record_tick();
    let (player, _driver) = Player::driven(48_000);
    *r.graph.inner.player.lock() = Some(Device { player, epoch: 1, resume: 0.0 });
    let said = r.written();
    assert!(said["written"].is_object(), "{said}");
    let (_, start, length, _) = note(&r.clips()[0], 0);
    assert!(on.has(start), "{on:?}: {start}");
    assert!((start + length - last).abs() < 0.05, "held to where it was last heard ({last}): {start} + {length}");
}

// The review's R6: snapped to the grid, a held note moved later still ends inside the clip: the
// clip reaches the bar line after the note's end.
#[test]
fn a_note_the_grid_moves_later_still_ends_inside_the_clip() {
    let mut r = Rig::new();
    r.anchored(0, 0.0);
    r.start(false, Some(1.0));
    r.to(6.6);
    r.key(&[0x90, 60, 100]);
    r.to(7.9);
    r.graph.transport("stop").unwrap();
    r.written();
    let c = &r.clips()[0];
    let (_, start, length, _) = note(c, 0);
    assert_eq!(start, 7.0, "on the grid: {c}");
    assert!(length > 1.2, "its length kept: {c}");
    assert_eq!(c["length"].as_f64(), Some(12.0), "to the bar line after its end: {c}");
}

// The review's R7: a cancel that lands just after the count's bar, before the take thread has seen
// it turn into playing, still stops the song (D6).
#[test]
fn a_cancel_just_after_the_count_still_stops_the_song() {
    let mut r = Rig::new();
    r.anchored(96_000, 0.0);
    r.start(true, None);
    r.to(0.03);
    assert!(r.graph.status().playing, "the count turned into playing");
    assert_eq!(r.graph.record_cancel(r.token).unwrap(), json!({"cancelled": true}));
    r.fill(10);
    assert!(!r.graph.status().playing, "nothing plays");
    assert!(!r.graph.recording());
}

/// Recording on an audio lane (the io plan's slice 2): a driven player at 48 kHz whose silent input
/// the test feeds one callback at a time, so every sample's place is known. A callback of 480
/// frames hands over 480 input frames, heard from two buffers (960 frames) before the first frame
/// it renders (`m2_play`'s injected input).
mod audio {
    use super::*;
    use m2_play::{Channels, InputChoice};

    const VOX: &str = "import { song, track } from '@m2/core';\n\
                       const vox = track('vox', { name: 'Vox' }, ({ lane }) => lane.audio('clips'));\n\
                       export default song(() => vox.out);\n";
    /// How long before the first frame a callback renders its input buffer was heard.
    const LATE: u64 = 2 * CALLBACK as u64;

    struct Rig {
        fixture: Fixture,
        graph: Graph,
        player: Player,
        d: Driver,
        token: u64,
        /// The engine clock where the transport is at beat 0 (or the take's start).
        anchor: u64,
        /// What the input hears at each engine clock, on every channel; or on each, by channel.
        sound: Box<dyn Fn(u64) -> f32>,
        each: Option<Box<dyn Fn(u64, usize) -> f32>>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig::with(VOX, &[], &[])
        }

        /// A rig whose song is `song`, with `files` beside it, and whose silent inputs are `inputs`
        /// (`Desk:4`: one called Desk, with 4 channels).
        fn with(song: &str, files: &[(&str, &str)], inputs: &[&str]) -> Rig {
            // Through a link, as on a Mac: the song opens by its real path, not this one.
            let fixture = Fixture::linked();
            fixture.put("song.ts", song);
            fixture.put("data/vox/clips.json", "{\"clips\": []}\n");
            for (path, text) in files {
                fixture.put(path, text);
            }
            let (player, d) = Player::driven(48_000);
            if !inputs.is_empty() {
                player.name_inputs(inputs);
            }
            let graph = Graph::with_player(player.clone()).stored();
            let token = graph.open(&fixture.0).unwrap()["session"].as_u64().unwrap();
            let mut rig = Rig { fixture, graph, player, d, token, anchor: 0, sound: Box::new(|_| 0.0), each: None };
            rig.fill(4);
            rig
        }

        /// `n` device callbacks, the input's buffer queued before each once a take opened it.
        fn fill(&mut self, n: usize) {
            let mut buf = [0.0f32; 2 * CALLBACK];
            for k in 0..n {
                if let Some(listen) = self.player.input() {
                    let first = self.d.engine().map_or(0, |e| e.out_clock());
                    let heard = first.saturating_sub(LATE);
                    match &self.each {
                        Some(f) => {
                            let each: Vec<Vec<f32>> =
                                (0..listen.channels as usize).map(|c| (0..CALLBACK as u64).map(|j| f(heard + j, c)).collect()).collect();
                            self.player.inject_channels(&each).unwrap();
                        }
                        None => {
                            let samples: Vec<f32> = (0..CALLBACK as u64).map(|j| (self.sound)(heard + j)).collect();
                            self.player.inject_input(&samples).unwrap();
                        }
                    }
                }
                self.d.fill(&mut buf, 2);
                drop(self.d.drain_retired());
                if k % 50 == 49 {
                    self.graph.record_settle();
                }
            }
            self.graph.record_settle();
        }

        /// The beat heard as the last callback began, by the engine clock.
        fn by_clock(&self) -> f64 {
            (self.graph.engine_clock().unwrap() as f64 - LATE as f64 - self.anchor as f64) / BEAT
        }

        fn to(&mut self, beat: f64) {
            while self.by_clock() < beat {
                self.fill(1);
            }
        }

        fn start(&self, count_in: bool, hear: bool) -> Value {
            let opts = Options { count_in, quantize: None, input: InputChoice::default(), hear, ..Default::default() };
            self.graph.record_start(self.token, "vox", "clips", opts).unwrap()
        }

        fn written(&mut self) -> Value {
            let t = std::time::Instant::now();
            // The input hands over the take's last frames two buffers after they were heard.
            self.fill(4);
            loop {
                let said = self.graph.record_tick();
                if let Some(v) =
                    said.into_iter().find(|v| v.get("written").is_some() || v.get("empty").is_some() || v.get("error").is_some())
                {
                    return v;
                }
                assert!(t.elapsed() < Duration::from_secs(10), "no take was written");
                // The input delivers the take's last frames as the song plays on.
                self.fill(2);
            }
        }

        fn clips(&self) -> Vec<Value> {
            let v: Value = serde_json::from_str(&self.fixture.read("data/vox/clips.json")).unwrap();
            v["clips"].as_array().unwrap().clone()
        }

        /// What the notices thread toasts at its next read of the fault ring.
        fn told(&self) -> Vec<String> {
            self.graph.hear_faults().0
        }

        /// The problems the news lists that faults made, as (code, module, message), and the
        /// input's severities.
        fn problems(&self) -> Listed {
            let problems = &self.graph.news()["problems"];
            let input = problems.as_array().unwrap().iter().filter(|p| p["code"].as_str().unwrap().starts_with("input."));
            (crate::graph::faults::tests::listed(problems), input.map(|p| p["severity"].as_str().unwrap().to_string()).collect())
        }

        /// The samples of the file clip `c` plays, its rate and channels.
        fn samples(&self, c: &Value) -> (Vec<f32>, u32, u16) {
            let path = self.fixture.0.join("assets").join(c["asset"].as_str().unwrap());
            let mut r = hound::WavReader::open(&path).unwrap();
            let spec = r.spec();
            (r.samples::<f32>().map(Result::unwrap).collect(), spec.sample_rate, spec.channels)
        }
    }

    /// The problems faults made, as (code, module, message), and the input's severities.
    type Listed = (Vec<(String, Option<String>, String)>, Vec<String>);

    /// A click on the clocks `at`, silence elsewhere.
    fn clicks(at: Vec<u64>) -> Box<dyn Fn(u64) -> f32> {
        Box::new(move |c| if at.contains(&c) { 1.0 } else { 0.0 })
    }

    // D20: a click train played at known engine clocks lands in the file where it was heard, to the
    // sample: the file's first sample is the one heard at the take's start.
    #[test]
    fn an_injected_input_records_where_it_was_heard() {
        let mut r = Rig::new();
        r.anchor = r.graph.engine_clock().unwrap();
        let at: Vec<u64> = (0..3).map(|b| r.anchor + b * BEAT as u64 + 37).collect();
        r.sound = clicks(at);
        let shown = r.start(false, false);
        assert_eq!((&shown["phase"], &shown["start"]), (&json!("take"), &json!(0.0)));
        r.to(2.5);
        r.graph.transport("stop").unwrap();
        let said = r.written();
        assert_eq!(said["written"]["asset"], json!("Vox take.wav"), "{said}");
        let c = &r.clips()[0];
        assert_eq!(c["start"].as_f64(), Some(0.0), "{c}");
        let (s, rate, channels) = r.samples(c);
        assert_eq!((rate, channels), (48_000, 1), "the song's rate; a mono input makes a mono take");
        let heard: Vec<usize> = s.iter().enumerate().filter(|(_, x)| **x != 0.0).map(|(i, _)| i).collect();
        assert_eq!(heard, [37, 24_037, 48_037], "each click where it was heard");
        let length = c["length"].as_f64().unwrap();
        assert!((length - s.len() as f64 / BEAT).abs() < 1e-9, "as long as the take: {length} beats, {} frames", s.len());
    }

    // WRITE-1, FMT-14: the take's file is pushed, named and its folder synced before the Tx names it.
    #[test]
    fn a_take_is_durable_before_the_song_names_it() {
        let mut r = Rig::new();
        assert!(m2_editor::take::steps(&r.fixture.0).is_empty());
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|c| (c % 100) as f32 / 100.0);
        r.start(false, false);
        r.to(1.0);
        r.graph.record_stop(r.token).unwrap();
        assert!(r.written()["written"].is_object());
        assert_eq!(m2_editor::take::steps(&r.fixture.0), ["push", "name", "sync", "tx"]);
        let parts: Vec<String> =
            std::fs::read_dir(r.fixture.0.join("assets")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(parts, ["Vox take.wav"], "no part file is left");
    }

    // Q20 and D20: stopped at bar 2, a bar of clicks, then the take from bar 2. What was played
    // during the count isn't in the file.
    #[test]
    fn a_count_in_keeps_nothing_before_the_start() {
        let mut r = Rig::new();
        r.graph.seek(4.0).unwrap();
        r.fill(2);
        // The song starts a bar after the count: 96,000 frames on.
        r.anchor = r.graph.engine_clock().unwrap() + 96_000;
        let anchor = r.anchor;
        r.sound = Box::new(move |c| if c < anchor { 1.0 } else { 0.25 });
        let shown = r.start(true, false);
        assert_eq!(shown["phase"], json!("count"));
        r.fill(150);
        r.anchor -= (4.0 * BEAT) as u64;
        r.to(5.0);
        r.graph.transport("stop").unwrap();
        r.written();
        let c = &r.clips()[0];
        assert_eq!(c["start"].as_f64(), Some(4.0), "on the start bar: {c}");
        let (s, _, _) = r.samples(c);
        assert!(s.len() >= BEAT as usize - 2 * 128, "to where the stop was heard: {} frames", s.len());
        assert!(s.iter().all(|x| *x == 0.25), "nothing from the count: first {:?}", &s[..4]);
    }

    // D19: one Undo takes the recording back; the file stays (music2 never deletes an asset).
    #[test]
    fn one_undo_takes_the_recording_back() {
        let mut r = Rig::new();
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        let before = r.graph.undo_names().0;
        r.start(false, false);
        r.to(1.5);
        r.graph.transport("stop").unwrap();
        let said = r.written();
        assert_eq!(said["written"]["revision"].as_u64(), r.graph.state().unwrap()["view"]["revision"].as_u64(), "{said}");
        assert_eq!(r.graph.undo_names().0.as_deref(), Some("recording on Vox"));
        assert_eq!(r.clips().len(), 1);
        r.graph.undo(r.token).unwrap();
        assert!(r.clips().is_empty(), "one Undo takes it back");
        assert_eq!(r.graph.undo_names().0, before, "one step");
        assert!(r.fixture.0.join("assets/Vox take.wav").is_file(), "the file stays");
    }

    // MON-11 (was Q27's first pass only): at the loop's end the take goes round, and its file keeps
    // the first pass to the sample, then the second. Stopped just into the second, the clip covers
    // the loop and plays the first, the newest whole pass.
    #[test]
    fn a_take_over_the_loop_keeps_its_first_pass_to_the_sample() {
        let mut r = Rig::new();
        r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        r.start(false, false);
        r.to(4.2);
        assert!(r.graph.recording(), "round the loop, still recording");
        assert!(r.graph.status().playing && r.graph.status().beat < 1.0, "round the loop, playing on");
        r.graph.transport("stop").unwrap();
        let said = r.written();
        assert!(said["written"].is_object(), "{said}");
        let c = &r.clips()[0];
        assert_eq!((c["start"].as_f64(), c["length"].as_f64(), c["offset"].as_f64()), (Some(0.0), Some(4.0), Some(0.0)), "{c}");
        let (s, _, _) = r.samples(c);
        assert!(s.len() > 4 * BEAT as usize && s[..4 * BEAT as usize].iter().all(|x| *x == 0.5), "the first pass, to the sample");
    }

    // D22: a writer that falls more than the ring (4 s) behind writes silence for what the ring lost,
    // so what comes after stays where it was heard, and the take says so.
    #[test]
    fn a_full_take_ring_writes_silence_and_keeps_time() {
        let mut r = Rig::new();
        r.anchor = r.graph.engine_clock().unwrap();
        let at: Vec<u64> = (0..12).map(|b| r.anchor + b * BEAT as u64 + 5).collect();
        r.sound = clicks(at);
        r.start(false, false);
        r.graph.record_pause(true);
        // 6 s of input while the writer moves nothing: the ring keeps the last 4.
        r.to(12.0);
        r.graph.record_pause(false);
        r.graph.record_stop(r.token).unwrap();
        let said = r.written();
        assert_eq!(said["written"]["note"], json!("The disk was too slow: part of the take is silent."), "{said}");
        let (s, _, _) = r.samples(&r.clips()[0]);
        let heard: Vec<usize> = s.iter().enumerate().filter(|(_, x)| **x != 0.0).map(|(i, _)| i).collect();
        let kept: Vec<usize> = (0..12).map(|b| b * BEAT as usize + 5).filter(|i| heard.contains(i)).collect();
        assert_eq!(heard, kept, "every click still where it was heard");
        assert!(!heard.contains(&5) && heard.contains(&(11 * BEAT as usize + 5)), "the first lost, the last kept: {heard:?}");
        assert!(s.len() >= 12 * BEAT as usize, "time kept: {} frames", s.len());
    }

    // The review's io-r1: a new audio lane's wait for the disk (up to 500 ms, D9) runs before the
    // control plane's lock is taken. Live notes, the piano, the meters and Record take that lock, so
    // they never wait for the disk.
    #[test]
    fn a_new_audio_lane_never_waits_for_the_disk_under_the_lock() {
        let mut r = Rig::new();
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        r.start(false, false);
        // Longer than the 4 s a lane keeps whole in memory: it streams.
        r.to(12.0);
        r.graph.transport("stop").unwrap();
        r.written();
        r.graph.undo(r.token).unwrap();
        r.graph.seek(0.0).unwrap();
        r.fill(4);
        // A disk that never answers: the clip's new lane waits the whole 500 ms for it.
        r.player.pause_disk(true);
        let mut longest = Duration::ZERO;
        std::thread::scope(|s| {
            let redo = s.spawn(|| r.graph.redo(r.token));
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(900) {
                let asked = Instant::now();
                drop(r.graph.inner.control.lock());
                longest = longest.max(asked.elapsed());
                std::thread::sleep(Duration::from_millis(1));
            }
            redo.join().unwrap().unwrap();
        });
        r.player.pause_disk(false);
        assert_eq!(r.clips().len(), 1, "the redo put the clip back");
        assert!(longest < Duration::from_millis(150), "the control plane's lock was held for {longest:?}");
    }

    // The review's io-r2, and the ex plan's slice 3: the input's device going away (a USB
    // microphone unplugged) is the input's, never the output's. The take ends with what it has. It
    // is one problem, `input.lost`, naming the input, told once (the take's own words say nothing
    // more), and listed until the next take. The song plays on through the same player; the next R
    // opens the input again.
    #[test]
    fn an_input_unplugged_mid_take_is_one_problem() {
        let mut r = Rig::new();
        r.player.name_inputs(&["Mic A"]);
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        r.start(false, false);
        r.to(2.0);
        r.player.input_error(m2_play::DeviceErrorKind::DeviceNotAvailable.into());
        let said = r.written();
        assert!(said["written"].is_object() && said["written"].get("note").is_none(), "written, with nothing more said: {said}");
        let words = "Mic A was unplugged, so the take ended there. It was kept up to then. The song plays on.";
        assert_eq!(r.told(), [words]);
        r.fill(4);
        r.graph.record_tick();
        assert!(r.told().is_empty(), "told once");
        assert_eq!(r.problems(), (vec![("input.lost".into(), None, words.into())], vec!["warning".into()]), "one problem");
        assert!(!r.player.stalled(), "the output isn't gone");
        assert_eq!(r.player.device_errors(), m2_play::DeviceErrors::default(), "nothing counted as the output's");
        assert!(r.graph.status().playing, "the song plays on");
        let (s, _, _) = r.samples(&r.clips()[0]);
        assert!(s.len() >= 2 * BEAT as usize - 2 * CALLBACK, "kept up to where it was unplugged: {} frames", s.len());
        r.graph.transport("stop").unwrap();
        r.fill(2);
        r.start(false, false);
        assert!(!r.player.input_gone(), "R opened the input again");
        assert!(r.problems().0.is_empty(), "the next take: it isn't listed");
        r.graph.record_cancel(r.token).unwrap();
    }

    // io r7, the ex plan's D8: an input chosen in Settings that isn't there records from one that
    // is, and says so, naming both, as outputs do: `input.missing`, a Warning, told once and listed
    // until the next take. That take is told again: listed again, with no second toast. A choice
    // that is there says nothing.
    #[test]
    fn a_chosen_input_that_isnt_there_is_told_naming_the_one_used() {
        let mut r = Rig::new();
        // As M2_AUDIO_INPUTS="Mic A,Mic B" lists them.
        r.player.name_inputs(&["Mic A", "Mic B"]);
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        let chosen =
            |name: &str| Options { input: InputChoice { device: Some(name.into()), name: Some(name.into()) }, ..Default::default() };
        r.graph.record_start(r.token, "vox", "clips", chosen("Mic C")).unwrap();
        assert_eq!(r.player.input().map(|l| l.name).as_deref(), Some("Mic A"), "it records from the one there");
        let words =
            "Mic C isn't plugged in, so this take records from Mic A. To record from another input, choose it in Settings ▸ Sound input.";
        assert_eq!(r.told(), [words]);
        let listed = (vec![("input.missing".to_string(), None, words.to_string())], vec!["warning".to_string()]);
        assert_eq!(r.problems(), listed);
        r.to(1.0);
        r.graph.transport("stop").unwrap();
        assert!(r.written()["written"].is_object(), "the take is kept");
        assert_eq!(r.problems(), listed, "listed until the next take");
        r.fill(2);
        r.graph.record_start(r.token, "vox", "clips", chosen("Mic C")).unwrap();
        assert!(r.told().is_empty(), "told once in the song");
        assert_eq!(r.problems(), listed, "listed again for the next take");
        r.graph.record_cancel(r.token).unwrap();
        r.graph.transport("stop").unwrap();
        r.fill(2);
        r.graph.record_start(r.token, "vox", "clips", chosen("Mic B")).unwrap();
        assert_eq!(r.player.input().map(|l| l.name).as_deref(), Some("Mic B"));
        assert!(r.told().is_empty() && r.problems().0.is_empty(), "Mic B is there: nothing is said");
        r.graph.record_cancel(r.token).unwrap();
    }

    // A Mac's Don't Allow during a take (the ex plan's D11): the OS refuses the microphone, and
    // the input gives silence. The take thread asks at each tick: the take is dropped, so nothing
    // is written and no file is left, and it is said once, in NO_MICROPHONE's words, as an Error.
    // The song plays on, and the next R says why.
    #[test]
    fn a_take_the_os_refuses_is_dropped_and_said() {
        let mut r = Rig::new();
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        r.start(false, false);
        r.to(1.0);
        r.graph.record_tick();
        r.player.refuse_input(true);
        let said = r.graph.record_tick();
        assert!(said.contains(&json!({"t": "take", "phase": null})), "the take ended: {said:?}");
        assert!(!r.graph.recording(), "nothing waits to be written");
        let words = format!("{} The take was only silence, so it wasn't kept.", m2_play::NO_MICROPHONE);
        assert_eq!(r.told(), std::slice::from_ref(&words));
        assert_eq!(r.problems(), (vec![("input.refused".into(), None, words)], vec!["error".into()]));
        r.fill(4);
        assert!(r.graph.status().playing, "the song plays on");
        assert!(r.clips().is_empty(), "no clip");
        // Its part file goes with its writer's thread.
        let files = || std::fs::read_dir(r.fixture.0.join("assets")).map_or(0, |d| d.count());
        let t = Instant::now();
        while files() > 0 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(files(), 0, "no file is left");
        let again = r.graph.record_start(r.token, "vox", "clips", Options::default()).map(|_| ());
        assert_eq!(again.map_err(|e| e.to_string()), Err(m2_play::NO_MICROPHONE.to_string()), "the next R says why");
        assert!(r.told().is_empty(), "said once");
    }

    // The review's io-r3: a write that fails (a full disk) ends the take with every frame written
    // before it, as a whole WAV, said in words. Before, the whole take was lost.
    #[test]
    fn a_full_disk_keeps_the_take_up_to_there() {
        let mut r = Rig::new();
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        r.start(false, false);
        // The take thread finds the take's start: the writer writes as it records.
        r.fill(2);
        r.graph.record_tick();
        r.to(1.0);
        // Room for about a quarter of a second more of the mono take.
        r.graph.record_leave_room(12_000 * 4 + 100);
        r.to(3.0);
        let said = r.written();
        assert_eq!(said["written"]["note"], json!(take_audio::DISK_FULL), "{said}");
        assert!(r.graph.status().playing, "the song plays on");
        let (s, _, _) = r.samples(&r.clips()[0]);
        assert!((BEAT as usize..2 * BEAT as usize).contains(&s.len()), "up to where the disk was full: {} frames", s.len());
        assert!(s.iter().all(|x| *x == 0.5), "every frame whole");
        let names: Vec<String> =
            std::fs::read_dir(r.fixture.0.join("assets")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Vox take.wav"], "no part file is left");
        r.graph.transport("stop").unwrap();
    }

    // The review's io-r3: a WAV counts its bytes in 32 bits, so a take ends before 4 GiB, at about
    // 3.1 hours of 48 kHz stereo. Past that its header would wrap, and the clip would play its first
    // minutes only.
    #[test]
    fn a_take_ends_before_its_wav_runs_out() {
        let f = Fixture::new();
        let stereo = m2_editor::take::Growing::new(&f.0, "Vox", 2, 48_000).unwrap();
        let mono = m2_editor::take::Growing::new(&f.0, "Bass", 1, 48_000).unwrap();
        let most = stereo.room();
        assert!(most * 8 < u32::MAX as u64 - 64, "its data and header fit 32 bits: {most} frames");
        assert!((most as f64 / 48_000.0 / 3600.0 - 3.1).abs() < 0.05, "about 3.1 hours: {most} frames");
        assert!(mono.room().abs_diff(2 * most) < 2, "a mono take twice as long");
    }

    // Stop then R at once on the same track: the take before is still being written (its last
    // frames, then the disk), or, let go, its thread hasn't yet taken its part file away. The next
    // take makes a part file of its own, and each is named as it finishes.
    #[test]
    fn a_take_starts_while_the_one_before_is_still_written() {
        let f = Fixture::new();
        let mut first = m2_editor::take::Growing::new(&f.0, "Vox", 2, 48_000).unwrap();
        let mut second = m2_editor::take::Growing::new(&f.0, "Vox", 2, 48_000).unwrap();
        let let_go = m2_editor::take::Growing::new(&f.0, "Vox", 2, 48_000).unwrap();
        first.append(&[[0.5; 2]; 4]).unwrap();
        second.append(&[[0.25; 2]; 4]).unwrap();
        assert_eq!(first.finish("Vox take.wav").unwrap().asset.as_str(), "Vox take.wav");
        assert_eq!(second.finish("Vox take.wav").unwrap().asset.as_str(), "Vox take 2.wav");
        drop(let_go);
        let mut names: Vec<String> =
            std::fs::read_dir(f.0.join("assets")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["Vox take 2.wav", "Vox take.wav"], "no part file is left");
    }

    // MON-8: hearing yourself plays the input on the take's lane, live only: an export made while the
    // take records has none of it.
    #[test]
    fn hearing_yourself_never_reaches_an_export() {
        let mut r = Rig::new();
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        r.start(false, true);
        r.to(1.0);
        let mut buf = [0.0f32; 2 * CALLBACK];
        let first = r.d.engine().unwrap().out_clock();
        r.player.inject_input(&[0.5; CALLBACK]).unwrap();
        r.d.fill(&mut buf, 2);
        assert!(r.d.engine().unwrap().out_clock() > first);
        assert!(buf.iter().all(|x| (*x - 0.5).abs() < 1e-6), "heard live: {:?}", &buf[..4]);
        let e = r.graph.export(m2_editor::render::Length::Seconds(0.5)).unwrap();
        let out = r.fixture.0.join("out.wav");
        e.render.write_float(&out, |_| {}).unwrap();
        let mut w = hound::WavReader::open(&out).unwrap();
        assert!(w.samples::<f32>().map(Result::unwrap).all(|x| x == 0.0), "the export has none of it");
        r.graph.transport("stop").unwrap();
        r.written();
    }

    /// The file's note beside clip `c`'s asset: its takes, in seconds (MON-11).
    fn takes_of(r: &Rig, c: &Value) -> Vec<f64> {
        let note: Value = serde_json::from_str(&r.fixture.read(&format!("assets/{}.json", c["asset"].as_str().unwrap()))).unwrap();
        note["takes"].as_array().unwrap().iter().map(|t| t.as_f64().unwrap()).collect()
    }

    fn arm(r: &Rig, module: &str, lane: Option<&str>, armed: Option<bool>, channels: Option<Channels>, hear: bool) -> Result<Value> {
        r.graph.arm(r.token, module, lane, armed, channels, InputChoice::default(), hear)
    }

    /// One callback, the input hearing `x` on every channel: what came out.
    fn heard(r: &mut Rig, x: f32) -> Vec<f32> {
        r.sound = Box::new(move |_| x);
        r.fill(8);
        let mut buf = [0.0f32; 2 * CALLBACK];
        r.player.inject_input(&[x; CALLBACK]).unwrap();
        r.d.fill(&mut buf, 2);
        buf.to_vec()
    }

    // MON-9, MON-8: an armed audio track hears its input through the track, with no take, while
    // Settings' Hear yourself is on; off, or disarmed, it doesn't. Nothing is written.
    #[test]
    fn an_armed_track_monitors_without_a_take() {
        let mut r = Rig::new();
        let armed = arm(&r, "vox", Some("clips"), Some(true), None, true).unwrap();
        assert_eq!(armed, json!({"armed": {"vox": "clips"}, "channels": {}}));
        let out = heard(&mut r, 0.5);
        assert!(out.iter().all(|x| (*x - 0.5).abs() < 1e-6), "heard through the track: {:?}", &out[..4]);
        assert!(!r.graph.recording() && r.graph.state().unwrap()["record"].is_null(), "no take");
        assert!(!r.graph.status().playing, "the song is stopped");
        assert_eq!(r.graph.state().unwrap()["arm"]["armed"], json!({"vox": "clips"}), "the state says it");
        r.graph.set_input(InputChoice::default(), false);
        assert!(heard(&mut r, 0.5).iter().all(|x| *x == 0.0), "Hear yourself off: not heard");
        r.graph.set_input(InputChoice::default(), true);
        assert!(heard(&mut r, 0.25).iter().all(|x| (*x - 0.25).abs() < 1e-6), "on again: heard");
        arm(&r, "vox", None, Some(false), None, true).unwrap();
        assert!(heard(&mut r, 0.5).iter().all(|x| *x == 0.0), "disarmed: not heard");
        assert!(r.clips().is_empty(), "nothing written");
        assert_eq!(r.graph.undo_names().0, None, "no undo step");
    }

    // MON-13: a track armed on channels 3–4 of a four-channel input records them, as a stereo take,
    // each where it was heard; one armed on channel 2 records it alone, as a mono take. Channels the
    // input hasn't are refused in words.
    #[test]
    fn input_three_and_four_record_their_own_channels() {
        let mut r = Rig::with(VOX, &[], &["Desk:4"]);
        arm(&r, "vox", Some("clips"), Some(true), Some(Channels::Pair(2)), false).unwrap();
        r.anchor = r.graph.engine_clock().unwrap();
        r.each = Some(Box::new(|_, c| [0.1, 0.2, 0.3, 0.4][c]));
        r.start(false, false);
        r.to(1.0);
        r.graph.transport("stop").unwrap();
        r.written();
        let (s, _, channels) = r.samples(&r.clips()[0]);
        assert_eq!(channels, 2, "a pair is a stereo take");
        assert!(s.len() >= 2 * (BEAT as usize - 2 * CALLBACK), "{} samples", s.len());
        assert!(s.chunks(2).all(|f| f == [0.3, 0.4]), "channel 3 left, channel 4 right: {:?}", &s[..4]);
        let refused = arm(&r, "vox", None, None, Some(Channels::Pair(4)), false).unwrap_err().to_string();
        assert_eq!(refused, "Desk has no input 5–6: it has 1 to 4.");
        arm(&r, "vox", None, None, Some(Channels::One(1)), false).unwrap();
        r.graph.seek(4.0).unwrap();
        r.fill(2);
        r.start(false, false);
        r.fill(60);
        r.graph.transport("stop").unwrap();
        r.written();
        let c = r.clips().into_iter().find(|c| c["start"].as_f64() >= Some(4.0)).unwrap();
        let (s, _, channels) = r.samples(&c);
        assert_eq!(channels, 1, "one channel is a mono take");
        assert!(!s.is_empty() && s.iter().all(|x| *x == 0.2), "channel 2 alone: {:?}", &s[..4]);
    }

    const BAND: &str = "import { song, track } from '@m2/core';\nimport { drums } from '@m2/drums';\nimport { sum } from 'std/math';\n\
                        const vox = track('vox', { name: 'Vox' }, ({ lane }) => lane.audio('clips'));\n\
                        const gtr = track('gtr', { name: 'Gtr' }, ({ lane }) => lane.audio('clips'));\n\
                        const keys = track('keys', { name: 'Keys' }, ({ lane }) => drums.drum(lane.notes('notes')));\n\
                        export default song(() => sum(sum(vox.out, gtr.out), keys.out));\n";

    // MON-9, MON-14: Record records into every armed track at once: a microphone on channel 1, a
    // guitar on 3–4, and the keys the MIDI keyboard plays (the armed notes track, whatever is
    // selected). One Tx, one undo step, which takes all three back.
    #[test]
    fn record_records_into_every_armed_track() {
        let mut r = Rig::with(BAND, &[("data/gtr/clips.json", "{\"clips\": []}\n")], &["Desk:4"]);
        assert_eq!(r.graph.midi(Event::Plugged("Keys")).as_deref(), Some("Keys is connected. It plays the selected track."));
        arm(&r, "vox", Some("clips"), Some(true), Some(Channels::One(0)), false).unwrap();
        arm(&r, "gtr", Some("clips"), Some(true), Some(Channels::Pair(2)), false).unwrap();
        let armed = arm(&r, "keys", Some("notes"), Some(true), None, false).unwrap();
        assert_eq!(armed["armed"], json!({"gtr": "clips", "keys": "notes", "vox": "clips"}));
        r.anchor = r.graph.engine_clock().unwrap();
        r.each = Some(Box::new(|_, c| [0.1, 0.2, 0.3, 0.4][c]));
        let before = r.graph.undo_names().0;
        let shown = r.start(false, false);
        assert_eq!(shown["lanes"].as_array().map(Vec::len), Some(3), "{shown}");
        r.to(1.0);
        assert_eq!(r.graph.midi(Event::Bytes("Keys", &[0x90, 60, 100])), None);
        r.to(1.5);
        assert_eq!(r.graph.midi(Event::Bytes("Keys", &[0x80, 60, 0])), None);
        r.to(2.0);
        let refused = arm(&r, "vox", None, Some(false), None, false).unwrap_err().to_string();
        assert_eq!(refused, "A take is recording: arm tracks before you record.");
        r.graph.transport("stop").unwrap();
        let said = r.written();
        assert!(said["written"].is_object(), "{said}");
        let read = |path: &str| serde_json::from_str::<Value>(&r.fixture.read(path)).unwrap()["clips"].as_array().unwrap().clone();
        let (vox, gtr, keys) = (read("data/vox/clips.json"), read("data/gtr/clips.json"), read("data/keys/notes.json"));
        assert_eq!((vox.len(), gtr.len(), keys.len()), (1, 1, 1), "a clip on each");
        let (v, _, vc) = r.samples(&vox[0]);
        let (g, _, gc) = r.samples(&gtr[0]);
        assert!(vc == 1 && v.iter().all(|x| *x == 0.1), "the microphone: channel 1, mono");
        assert!(gc == 2 && g.chunks(2).all(|f| f == [0.3, 0.4]), "the guitar: channels 3–4");
        assert_eq!(keys[0]["notes"].as_array().unwrap().len(), 1, "the keys: the note played");
        assert_eq!(r.graph.undo_names().0.as_deref(), Some("recording on Keys, Gtr, Vox"));
        r.graph.undo(r.token).unwrap();
        assert!(read("data/vox/clips.json").is_empty() && read("data/gtr/clips.json").is_empty(), "one undo takes all three back");
        let keys = r.fixture.0.join("data/keys/notes.json");
        assert!(!keys.exists() || read("data/keys/notes.json").is_empty(), "and the keys' clip");
        assert_eq!(r.graph.undo_names().0, before, "one step");
    }

    // MON-11, MON-14: three passes over the loop and a stop just into a fourth: one file of every
    // pass, one after another, its note listing them; one clip over the loop playing the third, the
    // newest whole pass. Choosing the first is one undo step, which takes it back.
    #[test]
    fn three_loop_passes_make_three_takes() {
        let mut r = Rig::new();
        r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
        r.anchor = r.graph.engine_clock().unwrap();
        let (a, pass) = (r.anchor, 4 * BEAT as u64);
        r.sound = Box::new(move |c| if c < a { 0.0 } else { 0.1 * ((c - a) / pass + 1) as f32 });
        r.start(false, false);
        // The take thread finds the take's start: the writer writes as it records.
        r.fill(2);
        r.graph.record_tick();
        r.to(12.5);
        r.graph.transport("stop").unwrap();
        let said = r.written();
        assert!(said["written"].is_object(), "{said}");
        let clips = r.clips();
        assert_eq!(clips.len(), 1, "one clip: {clips:?}");
        let c = &clips[0];
        assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(0.0), Some(4.0)), "over the loop: {c}");
        assert_eq!(takes_of(&r, c), [0.0, 2.0, 4.0, 6.0], "every pass, where it starts in the file");
        assert_eq!(c["offset"].as_f64(), Some(4.0), "the third plays: the fourth was cut short");
        let (s, _, _) = r.samples(c);
        for k in 0..3 {
            let pass = &s[k * 4 * BEAT as usize..(k + 1) * 4 * BEAT as usize];
            let off: Vec<(usize, f32)> = pass.iter().copied().enumerate().filter(|(_, x)| *x != 0.1 * (k + 1) as f32).take(4).collect();
            assert!(off.is_empty(), "pass {} to the sample: {off:?}", k + 1);
        }
        // The Takes menu: the view lists them; the first chosen.
        let state = r.graph.state().unwrap();
        let lane = state["timeline"]["tracks"].as_array().unwrap().iter().find(|t| t["module"] == "vox").unwrap()["lanes"][0].clone();
        assert_eq!(lane["takes"][c["asset"].as_str().unwrap()], json!([0.0, 2.0, 4.0, 6.0]));
        let clip = ClipRef { lane: LaneAddr::new("vox", "clips").unwrap(), id: c["id"].as_str().map(String::from), at: None, sha: None };
        r.graph.choose_take(r.token, clip, 0).unwrap();
        assert_eq!(r.clips()[0]["offset"].as_f64(), Some(0.0), "the first plays");
        assert_eq!(r.graph.undo_names().0.as_deref(), Some("Vox take 1"));
        r.graph.undo(r.token).unwrap();
        assert_eq!(r.clips()[0]["offset"].as_f64(), Some(4.0), "one undo: the third again");
        assert_eq!(r.graph.undo_names().0.as_deref(), Some("recording on Vox"));
    }

    // MON-10: a punch on beats 4 to 6, from beat 2: the song plays up to 4, records to 6 and plays
    // on. The clip there is cut at both edges, as Split cuts it, and the take covers exactly the
    // range.
    #[test]
    fn an_audio_punch_splits_what_was_there() {
        let mut r = Rig::new();
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.25);
        r.start(false, false);
        r.to(8.2);
        r.graph.transport("stop").unwrap();
        r.written();
        let first = r.clips()[0].clone();
        r.graph.seek(2.0).unwrap();
        r.fill(2);
        r.graph.set_loop(Some((4.0, 6.0)), Some(false)).unwrap();
        r.anchor = r.graph.engine_clock().unwrap() - (2.0 * BEAT) as u64;
        r.sound = Box::new(|_| 0.75);
        let opts = Options { punch: true, ..Default::default() };
        r.graph.record_start(r.token, "vox", "clips", opts).unwrap();
        r.to(3.0);
        assert!(r.clips().len() == 1, "nothing yet on the way in");
        // The take thread finds the take's start, so its writer writes as it records: past the
        // range's end too, before the next tick sees the take end there.
        r.graph.record_tick();
        r.to(7.0);
        let said = r.written();
        assert!(said["written"].is_object(), "{said}");
        assert!(r.graph.status().playing && r.graph.status().beat > 6.0, "the song plays on");
        let c = r.clips();
        let span = |c: &Value| (c["start"].as_f64().unwrap(), c["length"].as_f64().unwrap(), c["offset"].as_f64().unwrap());
        assert_eq!(c.len(), 3, "{c:?}");
        assert_eq!((span(&c[0]), c[0]["id"].clone()), ((0.0, 4.0, 0.0), first["id"].clone()), "the part before keeps its id");
        assert_eq!(span(&c[1]), (4.0, 2.0, 0.0), "the take covers the range");
        let after = span(&c[2]);
        assert!(after.0 == 6.0 && (after.2 - 3.0).abs() < 1e-9, "the part after plays on from 3 s into its file: {after:?}");
        assert!(c[0]["asset"] == first["asset"] && c[2]["asset"] == first["asset"] && c[1]["asset"] != first["asset"]);
        let (s, _, _) = r.samples(&c[1]);
        assert_eq!(s.len(), BEAT as usize * 2, "two beats to the sample");
        assert!(s.iter().all(|x| *x == 0.75), "what was played in the range");
    }

    /// Vox's song with a Gtr track before it: Vox's lane moves to another slot.
    const GTR_FIRST: &str = "import { song, track } from '@m2/core';\nimport { sum } from 'std/math';\n\
                             const gtr = track('gtr', { name: 'Gtr' }, ({ lane }) => lane.audio('clips'));\n\
                             const vox = track('vox', { name: 'Vox' }, ({ lane }) => lane.audio('clips'));\n\
                             export default song(() => sum(gtr.out, vox.out));\n";

    // MON-8 (review Q6): an armed track hears its input across a new plan in which its lane has
    // another slot, once; disarmed it stops; and two armed tracks hear at once.
    #[test]
    fn an_armed_track_hears_across_a_new_plan() {
        let mut r = Rig::new();
        arm(&r, "vox", Some("clips"), Some(true), None, true).unwrap();
        assert!(heard(&mut r, 0.5).iter().all(|x| (*x - 0.5).abs() < 1e-6), "armed: heard");
        r.fixture.put("data/gtr/clips.json", "{\"clips\": []}\n");
        r.fixture.put("song.ts", GTR_FIRST);
        assert!(r.graph.poll());
        r.fill(8);
        r.graph.record_tick();
        let out = heard(&mut r, 0.5);
        assert!(out.iter().all(|x| (*x - 0.5).abs() < 1e-6), "still heard, once, through Vox: {:?}", &out[..4]);
        arm(&r, "vox", None, Some(false), None, true).unwrap();
        let out = heard(&mut r, 0.5);
        assert!(out.iter().all(|x| *x == 0.0), "disarmed: silent {:?}", &out[..4]);
        arm(&r, "gtr", Some("clips"), Some(true), None, true).unwrap();
        arm(&r, "vox", Some("clips"), Some(true), None, true).unwrap();
        let out = heard(&mut r, 0.25);
        assert!(out.iter().all(|x| (*x - 0.5).abs() < 1e-6), "both hear: twice 0.25 {:?}", &out[..4]);
    }

    // MON-8 (review Q6): disarmed right after a new plan, before the take thread's tick, a track
    // stops hearing: the lane that moved into the new plan hearing is told to stop.
    #[test]
    fn a_track_disarmed_right_after_a_new_plan_stops_hearing() {
        let mut r = Rig::new();
        arm(&r, "vox", Some("clips"), Some(true), None, true).unwrap();
        assert!(heard(&mut r, 0.5).iter().all(|x| (*x - 0.5).abs() < 1e-6), "armed: heard");
        r.fixture.put("data/gtr/clips.json", "{\"clips\": []}\n");
        r.fixture.put("song.ts", GTR_FIRST);
        assert!(r.graph.poll());
        r.fill(8);
        arm(&r, "vox", None, Some(false), None, true).unwrap();
        let out = heard(&mut r, 0.5);
        assert!(out.iter().all(|x| *x == 0.0), "disarmed: silent {:?}", &out[..4]);
    }

    // MON-10 (review Q4): a punch whose writer ran past its end before the take thread saw it: the
    // waveform is made from the file as kept, never from the frames cut off.
    #[test]
    fn a_punchs_waveform_is_the_file_it_kept() {
        let mut r = Rig::new();
        r.fill(220);
        r.graph.seek(2.0).unwrap();
        r.fill(2);
        r.graph.set_loop(Some((4.0, 6.0)), Some(false)).unwrap();
        r.anchor = r.graph.engine_clock().unwrap() - (2.0 * BEAT) as u64;
        r.sound = Box::new(|_| 0.75);
        r.graph.record_start(r.token, "vox", "clips", Options { punch: true, ..Default::default() }).unwrap();
        r.to(3.0);
        r.graph.record_tick();
        // The take thread ticks late: the writer has written a beat past the range's end.
        r.to(7.0);
        assert!(r.written()["written"].is_object());
        let path = r.fixture.0.join("assets").join(r.clips()[0]["asset"].as_str().unwrap());
        let frames = hound::WavReader::open(&path).unwrap().duration() as u64;
        let peaks = crate::peaks::Store::global().of(&path).unwrap();
        assert_eq!((frames, peaks.frames), (2 * BEAT as u64, 2 * BEAT as u64), "the waveform is the file's");
    }

    // MON-11 (review Q3): on a clip that follows the tempo, choosing a take plays it: its warp
    // markers move with its offset, and an export of the song plays the take chosen.
    #[test]
    fn choosing_a_take_on_a_clip_that_follows_the_tempo_plays_it() {
        let mut r = Rig::new();
        r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
        r.anchor = r.graph.engine_clock().unwrap();
        let (a, pass) = (r.anchor, 4 * BEAT as u64);
        r.sound = Box::new(move |c| if c < a { 0.0 } else { 0.1 * ((c - a) / pass + 1) as f32 });
        r.start(false, false);
        r.fill(2);
        r.graph.record_tick();
        r.to(12.5);
        r.graph.transport("stop").unwrap();
        assert!(r.written()["written"].is_object());
        let c = r.clips()[0].clone();
        let clip = || ClipRef { lane: LaneAddr::new("vox", "clips").unwrap(), id: c["id"].as_str().map(String::from), at: None, sha: None };
        let follow =
            Gesture::Audio { clip: clip(), gain_db: None, fade_in: None, fade_out: None, reverse: None, follow: Some(true), offset: None };
        r.graph.clips(r.token, follow).unwrap();
        r.graph.choose_take(r.token, clip(), 0).unwrap();
        let after = r.clips()[0].clone();
        assert_eq!((after["offset"].as_f64(), after["warp"][0][0].as_f64()), (Some(0.0), Some(0.0)), "the markers moved with it: {after}");
        r.graph.set_loop(None, Some(false)).unwrap();
        let e = r.graph.export(m2_editor::render::Length::Seconds(2.0)).unwrap();
        let out = r.fixture.0.join("out.wav");
        e.render.write_float(&out, |_| {}).unwrap();
        let s: Vec<f32> = hound::WavReader::open(&out).unwrap().samples::<f32>().map(Result::unwrap).collect();
        let mut mid: Vec<f32> = s[s.len() / 4..3 * s.len() / 4].iter().map(|x| x.abs()).collect();
        mid.sort_by(f32::total_cmp);
        let median = mid[mid.len() / 2];
        assert!((median - 0.1).abs() < 0.02, "the first pass plays, not the third: {median}");
    }
}

// AGENT-3: while a take records, every agent's write is busy, whatever it touches: the take's
// notes follow the tempo map, and the musician may hear themselves through the track's effects.
// Nothing changes, and once the take ended the same write goes ahead.
#[test]
fn while_a_take_records_every_agents_write_is_busy() {
    use crate::caller::{Caller, as_caller};
    let mut r = Rig::new();
    let agent = Caller::agent(Some("Claude"), None);
    // A step of the agent's own, made before the take, which its undo would take back.
    let layout = serde_json::json!({"op": "write", "file": "layout.json", "text": "{}\n"});
    let made =
        serde_json::json!({"id": "tx-mine", "base": {"layout.json": null}, "label": ["a", "b"], "ops": [layout], "expect": "identical"});
    assert_eq!(as_caller(agent.clone(), || r.graph.tx_commit(Some(r.token), &made, None, &[])).unwrap().0, 200);
    r.start(false, None);
    r.fill(10);
    let song = r.fixture.read("song.ts");
    let lane = LaneAddr::new("keys", "notes").unwrap();
    let put = serde_json::json!({"id": "agent1", "start": 0, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 0.25}]});
    let mut tx = m2_song::Tx::new(
        "tx-agent",
        ("before".into(), "after".into()),
        vec![m2_song::tx::Op::Clips { lane: lane.clone(), put: vec![put], delete: vec![] }],
        m2_song::tx::Expect::Confined(m2_song::tx::Confined { lanes: vec![lane.clone()], ..Default::default() }),
    );
    let file = r.fixture.0.join("data/keys/notes.json");
    let base = std::fs::read_to_string(&file).map_or(m2_song::tx::Base::Absent, |t| m2_song::tx::Base::text(&t));
    tx.base.insert(m2_song::tx::TxPath::lane(&lane), base);
    let (status, body) = as_caller(agent.clone(), || r.graph.tx_commit(Some(r.token), &tx.to_json(), None, &[])).unwrap();
    assert_eq!((status, &body["busy"]["what"]), (423, &serde_json::json!("take")), "{body}");
    assert_eq!(body["busy"]["words"], serde_json::json!("you're recording on Bass"), "{body}");
    assert!(!file.exists(), "nothing changed");
    assert_eq!(r.fixture.read("song.ts"), song);
    let e = as_caller(agent.clone(), || r.graph.undo(r.token)).unwrap_err();
    assert!(e.downcast_ref::<crate::graph::busy::Busy>().is_some(), "an undo too: {e:#}");
    r.graph.record_stop(r.token).unwrap();
    r.written();
    let (status, body) = as_caller(agent, || r.graph.tx_commit(Some(r.token), &tx.to_json(), None, &[])).unwrap();
    assert_eq!(status, 200, "{body}");
}

// AGENT-3 (the review of ag-routes' F2): a take the musician begins while an agent's import copies
// its file, once its first check passed, makes the import busy when it lands, and its copy goes:
// nothing is left in the song, no file in `assets/`, no part file, not the folder either.
#[test]
fn a_take_begun_while_an_agents_import_copies_leaves_nothing() {
    use crate::caller::{Caller, as_caller};
    use m2_editor::import::{Source, Target};
    // The rig's song, with an audio track to bring the file onto.
    let song = "import { song, track } from '@m2/core';\nimport { drums } from '@m2/drums';\nimport { sum } from 'std/math';\n\
                const b = track('bass', { name: 'Bass' }, ({ lane }) => drums.drum(lane.notes('notes')));\n\
                const k = track('keys', { name: 'Keys' }, ({ lane }) => drums.drum(lane.notes('notes')));\n\
                const v = track('vox', { name: 'Vox' }, ({ lane }) => lane.audio('clips'));\n\
                export default song(() => sum(sum(b.out, k.out), v.out));\n";
    let r = Rig::with(&[("song.ts", song), ("data/vox/clips.json", "{\"clips\": []}\n")]);
    let (arrived, at) = std::sync::mpsc::channel::<()>();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    // The first check an agent's write passes waits here while the take begins.
    let mut once = Some((arrived, wait));
    r.graph.inspect(move |a| {
        a.after_arrival = Some(Box::new(move |_| {
            if let Some((arrived, wait)) = once.take() {
                let _ = arrived.send(());
                let _ = wait.recv();
            }
        }))
    });
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut wav = std::io::Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut wav, spec).unwrap();
    for i in 0..4_800 {
        w.write_sample(((i as f32 * 0.03).sin() * 8_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    let source = Source::Bytes { name: "vox.wav".into(), bytes: wav.into_inner() };
    let clips = r.fixture.read("data/vox/clips.json");
    std::thread::scope(|s| {
        let (graph, token) = (&r.graph, r.token);
        let onto = Target::Lane { module: "vox".into(), lane: "clips".into(), beat: 0.0 };
        let import = s.spawn(move || as_caller(Caller::agent(Some("Claude"), None), || graph.import_audio(token, vec![source], onto)));
        at.recv().unwrap();
        r.start(false, None);
        go.send(()).unwrap();
        let e = import.join().unwrap().unwrap_err();
        let busy = e.downcast_ref::<crate::graph::busy::Busy>().unwrap_or_else(|| panic!("busy: {e:#}"));
        assert_eq!(busy.what, "take", "{e:#}");
    });
    r.graph.inspect(|a| a.after_arrival = None);
    let assets = r.fixture.0.join("assets");
    let left: Vec<String> =
        std::fs::read_dir(&assets).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    assert_eq!(left, Vec::<String>::new(), "nothing was left in assets/");
    assert!(!assets.exists(), "not even the folder, which the import made");
    assert_eq!(r.fixture.read("data/vox/clips.json"), clips, "no clip was placed");
}

/// Bass's notes lane holding clip `base0001` from 0, `beats` long, a C3 on each beat.
fn base(beats: u32) -> String {
    let notes: Vec<String> =
        (0..beats).map(|b| format!("{{\"pitch\": 48, \"start\": {b}, \"length\": 0.25, \"velocity\": 0.5}}")).collect();
    format!("{{\"clips\": [{{\"id\": \"base0001\", \"start\": 0, \"length\": {beats}, \"notes\": [{}]}}]}}\n", notes.join(", "))
}

/// The lane's notes in song time, as (pitch, start), in order.
fn played(r: &Rig) -> Vec<(f64, f64)> {
    let mut out = vec![];
    for c in r.clips() {
        let at = c["start"].as_f64().unwrap();
        for i in 0..c["notes"].as_array().unwrap().len() {
            let (p, s, _, _) = note(&c, i);
            out.push((p, at + s));
        }
    }
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

impl Rig {
    /// The take thread's tick until `n` takes are written: what each said.
    fn writes(&self, n: usize) -> Vec<Value> {
        let t = std::time::Instant::now();
        let mut out = vec![];
        while out.len() < n {
            out.extend(self.graph.record_tick().into_iter().filter(|v| v.get("written").is_some()));
            assert!(t.elapsed() < Duration::from_secs(5), "{} of {n} takes written", out.len());
            std::thread::sleep(Duration::from_millis(5));
        }
        out
    }
}

// MON-10: a punch on beats 4 to 8 with the loop off, from beat 0: the song plays from 0, recording
// starts at 4 and ends at 8, where the song plays on. What was there in the range goes: the clip is
// cut at both edges, its notes before and after stay where they were. Notes before and after the
// range aren't kept.
#[test]
fn a_punch_replaces_only_its_range() {
    let mut r = Rig::with(&[("data/bass/notes.json", &base(16))]);
    r.graph.set_loop(Some((4.0, 8.0)), Some(false)).unwrap();
    r.anchored(0, 0.0);
    let shown = r.graph.record_start(r.token, "bass", "notes", Options { punch: true, ..Default::default() }).unwrap();
    assert_eq!((&shown["from"], &shown["until"], &shown["start"]), (&json!(4.0), &json!(8.0), &json!(4.0)), "{shown}");
    r.to(2.0);
    r.key(&[0x90, 60, 100]);
    r.to(2.25);
    r.key(&[0x80, 60, 0]);
    r.to(5.0);
    let on = r.key(&[0x90, 64, 100]);
    r.to(5.5);
    r.key(&[0x80, 64, 0]);
    r.to(8.5);
    let said = r.written();
    assert!(said["written"].is_object(), "{said}");
    r.key(&[0x90, 67, 100]);
    r.key(&[0x80, 67, 0]);
    assert!(r.graph.status().playing, "the song plays on");
    assert!(!r.graph.recording(), "the take ended at the range's end");
    let clips = r.clips();
    let spans: Vec<(&str, f64, f64)> =
        clips.iter().map(|c| (c["id"].as_str().unwrap(), c["start"].as_f64().unwrap(), c["length"].as_f64().unwrap())).collect();
    assert_eq!(spans.len(), 3, "{spans:?}");
    assert_eq!((spans[0].0, spans[0].1, spans[0].2), ("base0001", 0.0, 4.0), "the part before keeps its id");
    assert_eq!((spans[1].1, spans[1].2, spans[2].1, spans[2].2), (4.0, 4.0, 8.0, 8.0), "the take over the range, then the part after");
    let notes = played(&r);
    let (before, inside, after): (Vec<_>, Vec<_>, Vec<_>) = (
        notes.iter().filter(|n| n.1 < 4.0).copied().collect(),
        notes.iter().filter(|n| (4.0..8.0).contains(&n.1)).copied().collect(),
        notes.iter().filter(|n| n.1 >= 8.0).copied().collect(),
    );
    assert_eq!(before, (0..4).map(|b| (48.0, b as f64)).collect::<Vec<_>>(), "the notes before stay");
    assert_eq!(after, (8..16).map(|b| (48.0, b as f64)).collect::<Vec<_>>(), "the notes after stay");
    assert_eq!(inside.len(), 1, "only the take's note inside: {inside:?}");
    assert!(inside[0].0 == 64.0 && on.has(inside[0].1), "where it was heard: {on:?}, {inside:?}");
}

// MON-11: over the loop each pass is its own clip, written as the pass ends, so the next pass plays
// it: three passes, three clips over the loop, three undo steps.
#[test]
fn each_pass_over_a_loop_is_its_own_clip() {
    let mut r = Rig::new();
    r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
    r.anchored(0, 0.0);
    r.start(false, None);
    for (k, pitch) in [60u8, 62, 64].into_iter().enumerate() {
        r.to(k as f64 * 4.0 + 1.0);
        r.key(&[0x90, pitch, 100]);
        r.to(k as f64 * 4.0 + 1.5);
        r.key(&[0x80, pitch, 0]);
    }
    r.to(12.5);
    r.writes(3);
    assert!(r.graph.recording(), "still going round");
    r.graph.transport("stop").unwrap();
    assert_eq!(r.written(), json!({"t": "take", "empty": true}), "the fourth pass: nothing played");
    let clips = r.clips();
    assert_eq!(clips.len(), 3, "{clips:?}");
    let mut pitches = vec![];
    for c in &clips {
        assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(0.0), Some(4.0)), "over the loop: {c}");
        assert_eq!(c["notes"].as_array().unwrap().len(), 1);
        let (pitch, start, _, _) = note(c, 0);
        assert!((start - 1.0).abs() < 0.05, "a beat in: {start}");
        pitches.push(pitch);
    }
    pitches.sort_by(f64::total_cmp);
    assert_eq!(pitches, [60.0, 62.0, 64.0], "one note a pass");
    for _ in 0..3 {
        assert_eq!(r.graph.undo_names().0.as_deref(), Some("recording on Bass"));
        r.graph.undo(r.token).unwrap();
    }
    assert!(r.clips().is_empty(), "one undo step a pass");
}

// MON-12: with Overdub, a take over a clip adds its notes into it: one clip, both notes.
#[test]
fn overdub_merges_into_the_clip_under_it() {
    let mut r = Rig::with(&[("data/bass/notes.json", &base(1))]);
    r.anchored(0, 0.0);
    r.graph.record_start(r.token, "bass", "notes", Options { overdub: true, ..Default::default() }).unwrap();
    r.to(0.5);
    let on = r.key(&[0x90, 60, 100]);
    r.to(0.75);
    r.key(&[0x80, 60, 0]);
    r.to(1.0);
    r.graph.transport("stop").unwrap();
    assert!(r.written()["written"].is_object());
    let clips = r.clips();
    assert_eq!(clips.len(), 1, "{clips:?}");
    assert_eq!((clips[0]["id"].as_str(), clips[0]["length"].as_f64()), (Some("base0001"), Some(4.0)), "the clip under it, grown to a bar");
    let notes = played(&r);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes[0] == (48.0, 0.0) && notes[1].0 == 60.0 && on.has(notes[1].1), "both: {notes:?}");
}

/// A song with a kit: a Drums bus, and Kick, Snare and Hats going to it, each from its lane `hits`;
/// and Bass, which the rig selects.
const KIT: &str = "import { bus, song, track } from '@m2/core';\nimport { drums } from '@m2/drums';\nimport { gain } from 'std/dsp';\n\
                   import { master, mixer } from 'std/mix';\n\
                   const d = bus('drums', { name: 'Drums' }, ({ input }) => gain(input));\n\
                   const k = track('kick', { name: 'Kick' }, ({ lane }) => drums.drum(lane.notes('hits')));\n\
                   const s = track('snare', { name: 'Snare' }, ({ lane }) => drums.drum(lane.notes('hits'), { drum: 'snare' }));\n\
                   const h = track('hats', { name: 'Hats' }, ({ lane }) => drums.drum(lane.notes('hits'), { drum: 'hats' }));\n\
                   const b = track('bass', { name: 'Bass' }, ({ lane }) => drums.drum(lane.notes('notes')));\n\
                   export default song(() => master(mixer({ drums: d.out, kick: { in: k.out, to: 'drums' }, snare: { in: s.out, to: 'drums' }, \
                   hats: { in: h.out, to: 'drums' }, bass: b.out })));\n";

/// The kit's Drum tracks, each with what lane `hits` holds: its notes in song time, as (pitch, start).
fn kit_notes(r: &Rig) -> [(&'static str, Vec<(f64, f64)>); 3] {
    ["kick", "snare", "hats"].map(|m| {
        let text = std::fs::read_to_string(r.fixture.0.join(format!("data/{m}/hits.json"))).unwrap_or_else(|_| "{\"clips\": []}".into());
        let v: Value = serde_json::from_str(&text).unwrap();
        let mut out = vec![];
        for c in v["clips"].as_array().unwrap() {
            let at = c["start"].as_f64().unwrap();
            for i in 0..c["notes"].as_array().unwrap().len() {
                let (p, s, _, _) = note(c, i);
                out.push((p, ((at + s) * 4.0).round() / 4.0));
            }
        }
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        (m, out)
    })
}

/// The lane `hits` of `module`'s clips, as (start, length).
fn kit_clips(r: &Rig, module: &str) -> Vec<(f64, f64)> {
    let text = std::fs::read_to_string(r.fixture.0.join(format!("data/{module}/hits.json"))).unwrap_or_else(|_| "{\"clips\": []}".into());
    let v: Value = serde_json::from_str(&text).unwrap();
    v["clips"].as_array().unwrap().iter().map(|c| (c["start"].as_f64().unwrap(), c["length"].as_f64().unwrap())).collect()
}

/// `pitch` struck at `beat` and let go a quarter beat later.
fn hit(r: &mut Rig, pitch: u8, beat: f64) {
    r.to(beat);
    r.key(&[0x90, pitch, 100]);
    r.to(beat + 0.25);
    r.key(&[0x80, pitch, 0]);
}

/// Every beat of 16 on lane `hits` of the kit's every track, each at `pitch` 48.
fn kit_base() -> Vec<(String, String)> {
    ["kick", "snare", "hats"].iter().map(|m| (format!("data/{m}/hits.json"), base(16))).collect()
}

// KIT-26: a punch on a kit replaces every Drum track's lane between the loop's edges once any is
// played there: the Kick's hit is its lane's one note there, and the Snare's and the Hats' lanes,
// not played, are left empty there; the notes before and after stay. One undo puts it all back.
#[test]
fn a_punch_on_a_kit_replaces_every_drums_lane_in_its_range() {
    let files = kit_base();
    let mut files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    files.push(("song.ts", KIT));
    let mut r = Rig::with(&files);
    let before = kit_notes(&r);
    r.graph.set_loop(Some((4.0, 8.0)), Some(false)).unwrap();
    r.anchored(0, 0.0);
    r.graph.record_start(r.token, "kick", "hits", Options { punch: true, ..Default::default() }).unwrap();
    hit(&mut r, 36, 5.0);
    r.to(8.5);
    assert!(r.written()["written"].is_object());
    let outside = |b: f64| !(4.0..8.0).contains(&b);
    let base: Vec<(f64, f64)> = (0..16).map(f64::from).filter(|b| outside(*b)).map(|b| (48.0, b)).collect();
    let mut kick = base.clone();
    kick.insert(4, (36.0, 5.0));
    assert_eq!(kit_notes(&r), [("kick", kick), ("snare", base.clone()), ("hats", base)]);
    assert_eq!(kit_clips(&r, "snare"), [(0.0, 4.0), (8.0, 8.0)], "no clip of its own there");
    assert_eq!(r.graph.undo_names().0.as_deref(), Some("recording on Drums"));
    r.graph.undo(r.token).unwrap();
    assert_eq!(kit_notes(&r), before, "one undo");
}

// KIT-26: Overdub on a kit puts each hit into the clip under it on its own Drum track's lane; a lane
// not played is left as it was.
#[test]
fn overdub_on_a_kit_puts_each_hit_into_its_lanes_clip() {
    let files = kit_base();
    let mut files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    files.push(("song.ts", KIT));
    let mut r = Rig::with(&files);
    r.anchored(0, 0.0);
    r.graph.record_start(r.token, "snare", "hits", Options { overdub: true, ..Default::default() }).unwrap();
    hit(&mut r, 36, 0.5);
    hit(&mut r, 38, 1.5);
    r.to(2.0);
    r.graph.transport("stop").unwrap();
    assert!(r.written()["written"].is_object());
    let base: Vec<(f64, f64)> = (0..16).map(|b| (48.0, f64::from(b))).collect();
    let with = |p: f64, at: f64| {
        let mut v = base.clone();
        v.push((p, at));
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    };
    assert_eq!(kit_notes(&r), [("kick", with(36.0, 0.5)), ("snare", with(38.0, 1.5)), ("hats", base.clone())]);
    for m in ["kick", "snare", "hats"] {
        assert_eq!(kit_clips(&r, m), [(0.0, 16.0)], "{m}: one clip, the one under it");
    }
}

// KIT-26: over a loop, each pass of a kit's take is one Tx of every lane played: the first pass's
// Kick and Snare in one undo step, the second's Hats in another.
#[test]
fn each_pass_over_a_loop_on_a_kit_is_one_step() {
    let mut r = Rig::with(&[("song.ts", KIT)]);
    r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
    r.anchored(0, 0.0);
    r.graph.record_start(r.token, "hats", "hits", Options::default()).unwrap();
    hit(&mut r, 36, 1.0);
    hit(&mut r, 38, 2.0);
    hit(&mut r, 42, 5.0);
    r.to(8.5);
    r.writes(2);
    r.graph.transport("stop").unwrap();
    assert_eq!(r.written(), json!({"t": "take", "empty": true}), "the third pass: nothing played");
    assert_eq!(kit_notes(&r), [("kick", vec![(36.0, 1.0)]), ("snare", vec![(38.0, 2.0)]), ("hats", vec![(42.0, 1.0)])]);
    r.graph.undo(r.token).unwrap();
    assert_eq!(kit_notes(&r), [("kick", vec![(36.0, 1.0)]), ("snare", vec![(38.0, 2.0)]), ("hats", vec![])], "the second pass");
    r.graph.undo(r.token).unwrap();
    assert_eq!(kit_notes(&r), [("kick", vec![]), ("snare", vec![]), ("hats", vec![])], "the first pass, both lanes");
}

// KIT-25, KIT-26: a take on a kit records the keyboard piano's keys on the Drum tracks that answer
// to them, as a pad's.
#[test]
fn a_take_on_a_kit_records_the_keyboard_pianos_keys_on_their_drums() {
    let mut r = Rig::with(&[("song.ts", KIT)]);
    r.anchored(0, 0.0);
    r.graph.record_start(r.token, "kick", "hits", Options::default()).unwrap();
    let piano = |h: u32, key: f64| Audition {
        module: Some("kick".into()),
        lane: Some("hits".into()),
        on: vec![Strike { h, pitch: key, velocity: 0.8 }],
        record: true,
        ..Audition::default()
    };
    for (h, key, at) in [(1, 38.0, 1.0), (2, 42.0, 2.0)] {
        r.to(at);
        r.graph.audition("w1", piano(h, key)).unwrap();
        r.to(at + 0.25);
        r.graph.audition("w1", Audition { off: vec![h], ..Audition::default() }).unwrap();
    }
    r.to(3.0);
    r.graph.transport("stop").unwrap();
    assert!(r.written()["written"].is_object());
    assert_eq!(kit_notes(&r), [("kick", vec![]), ("snare", vec![(38.0, 1.0)]), ("hats", vec![(42.0, 2.0)])]);
}

// MON-9: one notes track is armed at a time; while one is, the keyboards play it and Record records
// it, whatever a window selects.
#[test]
fn arming_a_notes_track_disarms_the_other() {
    let mut r = Rig::new();
    let arm = |r: &Rig, module: &str| {
        r.graph.arm(r.token, module, Some("notes"), Some(true), None, m2_play::InputChoice::default(), false).unwrap()
    };
    assert_eq!(arm(&r, "keys")["armed"], json!({"keys": "notes"}));
    r.anchored(0, 0.0);
    let shown = r.start(false, None);
    assert_eq!(shown["lanes"], json!([{"module": "keys", "lane": "notes", "kind": "notes", "start": 0.0}]), "the armed track, not bass");
    r.to(1.0);
    r.key(&[0x90, 60, 100]);
    r.to(1.5);
    r.key(&[0x80, 60, 0]);
    r.graph.transport("stop").unwrap();
    assert!(r.written()["written"].is_object());
    let keys: Value = serde_json::from_str(&std::fs::read_to_string(r.fixture.0.join("data/keys/notes.json")).unwrap()).unwrap();
    assert_eq!(keys["clips"].as_array().map(Vec::len), Some(1), "recorded on Keys");
    assert!(r.clips().is_empty(), "nothing on Bass");
    assert_eq!(arm(&r, "bass")["armed"], json!({"bass": "notes"}), "arming Bass disarms Keys");
}

/// Every note of the lane in song time, as (pitch, start, end), in order.
fn spans(r: &Rig) -> Vec<(f64, f64, f64)> {
    let mut out = vec![];
    for c in r.clips() {
        let at = c["start"].as_f64().unwrap();
        for i in 0..c["notes"].as_array().unwrap().len() {
            let (p, s, l, _) = note(&c, i);
            assert!(s >= 0.0, "a note starts inside its clip: {c}");
            out.push((p, at + s, at + s + l));
        }
    }
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

// MON-11 (review Q1): over a loop that starts at beat 4, a key struck while the speakers play the
// pass's last moments, the engine already round, stays in its pass, held to the loop's end; one
// struck just after the wrap starts the next pass. Neither lands before the loop's start.
#[test]
fn keys_either_side_of_a_wrap_stay_in_their_pass() {
    let mut r = Rig::new();
    r.graph.set_loop(Some((4.0, 8.0)), Some(true)).unwrap();
    r.graph.seek(4.0).unwrap();
    r.fill(2);
    r.anchored(0, 4.0);
    r.start(false, None);
    r.to(7.97);
    let before = r.by_clock();
    assert!(r.graph.status().beat < 5.0, "the engine is round already, the speakers not yet");
    r.key(&[0x90, 72, 100]);
    r.to(8.01);
    let after = r.by_clock();
    r.key(&[0x90, 74, 100]);
    r.to(8.5);
    r.key(&[0x80, 72, 0]);
    r.key(&[0x80, 74, 0]);
    r.to(9.0);
    r.graph.transport("stop").unwrap();
    // The first pass, then the second.
    r.writes(2);
    let notes = spans(&r);
    assert_eq!(notes.len(), 2, "{notes:?}");
    let (pitch, start, end) = notes.iter().copied().find(|n| n.0 == 72.0).unwrap();
    assert!(pitch == 72.0 && (start - before).abs() < 0.03 && (end - 8.0).abs() < 1e-9, "in the first pass, to the loop's end: {notes:?}");
    let (_, start, _) = notes.iter().copied().find(|n| n.0 == 74.0).unwrap();
    assert!(start >= 4.0 && (start + 4.0 - after).abs() < 0.03, "at the start of the next pass: {notes:?}");
}

// MON-9 (review Q2): an armed track that leaves the song is disarmed at once: the state says so,
// Record records the selected track again, and the MIDI keyboard plays it.
#[test]
fn an_armed_track_that_leaves_the_song_is_disarmed() {
    let mut r = Rig::new();
    r.graph.arm(r.token, "keys", Some("notes"), Some(true), None, m2_play::InputChoice::default(), false).unwrap();
    let only_bass = "import { song, track } from '@m2/core';\nimport { drums } from '@m2/drums';\n\
                     const b = track('bass', { name: 'Bass' }, ({ lane }) => drums.drum(lane.notes('notes')));\n\
                     export default song(() => b.out);\n";
    r.fixture.put("song.ts", only_bass);
    assert!(r.graph.poll());
    r.fill(8);
    assert_eq!(r.graph.state().unwrap()["arm"]["armed"], json!({}), "Keys is gone: disarmed");
    let mut buf = [0.0f32; 2 * CALLBACK];
    r.graph.midi(Event::Bytes("Keys", &[0x90, 40, 120]));
    let mut peak = 0.0f32;
    for _ in 0..20 {
        r.d.fill(&mut buf, 2);
        peak = buf.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    r.graph.midi(Event::Bytes("Keys", &[0x80, 40, 0]));
    assert!(peak > 0.1, "the keyboard plays the selected Bass: {peak}");
    r.fill(10);
    let shown = r.graph.record_start(r.token, "bass", "notes", Options::default()).unwrap();
    assert_eq!(shown["module"], json!("bass"), "Record records the selected track");
    r.graph.record_cancel(r.token).unwrap();
}

// MON-10, MON-11 (review Q5): a key struck a moment before a take starts after its way in (the
// song played up to the loop's start) lands on the take's first beat, as one in a count's last
// beat does (D27).
#[test]
fn a_key_struck_just_before_a_way_in_ends_lands_on_the_first_beat() {
    let mut r = Rig::new();
    r.graph.set_loop(Some((4.0, 8.0)), Some(true)).unwrap();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(3.97);
    r.key(&[0x90, 72, 100]);
    r.to(4.5);
    let off = r.key(&[0x80, 72, 0]);
    r.to(5.0);
    r.graph.transport("stop").unwrap();
    assert!(r.written()["written"].is_object(), "kept");
    let notes = spans(&r);
    assert_eq!(notes.len(), 1, "{notes:?}");
    let (_, start, end) = notes[0];
    assert!(start == 4.0 && off.has(end), "from the take's first beat to where it was let go: {notes:?}");
}

// MON-14 (review): undo of a punch and of an overdub gives the lane file back byte for byte.
#[test]
fn undo_puts_a_punch_and_an_overdub_back_byte_for_byte() {
    let take = |r: &mut Rig, opts: &Options| {
        r.graph.seek(0.0).unwrap();
        r.fill(4);
        r.anchored(0, 0.0);
        r.graph.record_start(r.token, "bass", "notes", opts.clone()).unwrap();
        r.to(5.0);
        r.key(&[0x90, 64, 100]);
        r.to(5.5);
        r.key(&[0x80, 64, 0]);
        r.to(8.5);
        if r.graph.recording() {
            r.graph.transport("stop").unwrap();
        }
        assert!(r.written()["written"].is_object());
        r.graph.transport("stop").unwrap();
        r.fill(4);
    };
    for opts in [Options { punch: true, ..Default::default() }, Options { overdub: true, ..Default::default() }] {
        let mut r = Rig::with(&[("data/bass/notes.json", &base(16))]);
        r.graph.set_loop(Some((4.0, 8.0)), Some(false)).unwrap();
        take(&mut r, &opts);
        r.graph.undo(r.token).unwrap();
        let before = std::fs::read_to_string(r.fixture.0.join("data/bass/notes.json")).unwrap();
        take(&mut r, &opts);
        assert_ne!(std::fs::read_to_string(r.fixture.0.join("data/bass/notes.json")).unwrap(), before, "{opts:?}");
        r.graph.undo(r.token).unwrap();
        assert_eq!(std::fs::read_to_string(r.fixture.0.join("data/bass/notes.json")).unwrap(), before, "{opts:?}");
    }
}

// MON-9, MON-11, RENDER-36: the pedal works on an armed track, pass by pass. A key let go under the
// pedal is kept to the pedal's lift in its pass; one the pedal holds over the wrap ends at the
// loop's end, its pass's clip on the armed Keys.
#[test]
fn the_pedal_holds_notes_on_an_armed_track_in_each_pass() {
    let mut r = Rig::new();
    r.graph.arm(r.token, "keys", Some("notes"), Some(true), None, m2_play::InputChoice::default(), false).unwrap();
    r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
    r.anchored(0, 0.0);
    r.start(false, None);
    r.to(1.0);
    r.key(&[0x90, 60, 100]);
    r.key(&[0xB0, 64, 127]);
    r.to(1.5);
    r.key(&[0x80, 60, 0]);
    r.to(2.5);
    r.key(&[0xB0, 64, 0]);
    r.to(5.0);
    r.key(&[0x90, 62, 100]);
    r.key(&[0xB0, 64, 127]);
    r.to(5.5);
    r.key(&[0x80, 62, 0]);
    r.to(8.5);
    r.key(&[0xB0, 64, 0]);
    r.writes(2);
    r.graph.transport("stop").unwrap();
    r.written();
    let keys: Value = serde_json::from_str(&std::fs::read_to_string(r.fixture.0.join("data/keys/notes.json")).unwrap()).unwrap();
    let clips = keys["clips"].as_array().unwrap();
    assert_eq!(clips.len(), 2, "a clip a pass, on the armed Keys: {keys}");
    let mut ends: Vec<(f64, f64, f64)> = clips.iter().map(|c| note(c, 0)).map(|(p, s, l, _)| (p, s, s + l)).collect();
    ends.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!((ends[0].0, ends[1].0) == (60.0, 62.0), "{ends:?}");
    assert!((ends[0].2 - 2.5).abs() < 0.05, "held by the pedal to its lift: {ends:?}");
    assert!((ends[1].2 - 4.0).abs() < 1e-9, "held over the wrap: to the loop's end: {ends:?}");
    assert!(r.clips().is_empty(), "nothing on the selected Bass");
}
