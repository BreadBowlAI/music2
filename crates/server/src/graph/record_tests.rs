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

const SONG: &str = "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\nimport { sum } from 'std/math';\n\
                    const b = track('bass', { name: 'Bass' }, ({ lane }) => kick(lane.notes('notes')));\n\
                    const k = track('keys', { name: 'Keys' }, ({ lane }) => kick(lane.notes('notes')));\n\
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
        let graph = Graph::with_player(p);
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
            if let Some(v) = self.graph.record_tick().into_iter().find(|v| v.get("written").is_some() || v.get("empty").is_some() || v.get("error").is_some()) {
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
        let a = Audition { module: Some("bass".into()), lane: Some("notes".into()), on: vec![Strike { h, pitch, velocity: 0.5 }], record, ..Default::default() };
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

// Q27: the first pass only. At the loop's end the take is written, and the loop plays on.
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
    assert!(!r.graph.recording(), "recording ended");
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
    assert!(r.graph.recording() && said.iter().all(|v| v.get("written").is_none() && v.get("empty").is_none()), "still recording past the old end: {said:?}");
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
    use m2_play::InputChoice;

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
        /// What the input hears at each engine clock.
        sound: Box<dyn Fn(u64) -> f32>,
    }

    impl Rig {
        fn new() -> Rig {
            let fixture = Fixture::new();
            fixture.put("song.ts", VOX);
            fixture.put("data/vox/clips.json", "{\"clips\": []}\n");
            let (player, d) = Player::driven(48_000);
            let graph = Graph::with_player(player.clone());
            let token = graph.open(&fixture.0).unwrap()["session"].as_u64().unwrap();
            let mut rig = Rig { fixture, graph, player, d, token, anchor: 0, sound: Box::new(|_| 0.0) };
            rig.fill(4);
            rig
        }

        /// `n` device callbacks, the input's buffer queued before each once a take opened it.
        fn fill(&mut self, n: usize) {
            let mut buf = [0.0f32; 2 * CALLBACK];
            for k in 0..n {
                if self.player.input().is_some() {
                    let first = self.d.engine().map_or(0, |e| e.out_clock());
                    let heard = first.saturating_sub(LATE);
                    let samples: Vec<f32> = (0..CALLBACK as u64).map(|j| (self.sound)(heard + j)).collect();
                    self.player.inject_input(&samples).unwrap();
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
            let opts = Options { count_in, quantize: None, input: InputChoice::default(), hear };
            self.graph.record_start(self.token, "vox", "clips", opts).unwrap()
        }

        fn written(&mut self) -> Value {
            let t = std::time::Instant::now();
            // The input hands over the take's last frames two buffers after they were heard.
            self.fill(4);
            loop {
                let said = self.graph.record_tick();
                if let Some(v) = said.into_iter().find(|v| v.get("written").is_some() || v.get("empty").is_some() || v.get("error").is_some()) {
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

        /// The samples of the file clip `c` plays, its rate and channels.
        fn samples(&self, c: &Value) -> (Vec<f32>, u32, u16) {
            let path = self.fixture.0.join("assets").join(c["asset"].as_str().unwrap());
            let mut r = hound::WavReader::open(&path).unwrap();
            let spec = r.spec();
            (r.samples::<f32>().map(Result::unwrap).collect(), spec.sample_rate, spec.channels)
        }
    }

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
        let parts: Vec<String> = std::fs::read_dir(r.fixture.0.join("assets")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
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

    // Q27, D20: the first pass only, as a notes take: written at the loop's end, as long as the loop.
    #[test]
    fn a_take_ends_at_the_loop_end() {
        let mut r = Rig::new();
        r.graph.set_loop(Some((0.0, 4.0)), Some(true)).unwrap();
        r.anchor = r.graph.engine_clock().unwrap();
        r.sound = Box::new(|_| 0.5);
        r.start(false, false);
        r.to(4.2);
        let said = r.written();
        assert!(said["written"].is_object(), "{said}");
        assert!(r.graph.status().playing && r.graph.status().beat < 1.0, "round the loop, playing on");
        let c = &r.clips()[0];
        assert_eq!((c["start"].as_f64(), c["length"].as_f64()), (Some(0.0), Some(4.0)), "{c}");
        assert_eq!(r.samples(c).0.len(), 4 * BEAT as usize, "the first pass, to the sample");
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
}
