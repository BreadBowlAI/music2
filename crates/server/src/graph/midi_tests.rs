//! MIDI keyboards on the server (8b.10): a keyboard's keys play the lane a window named, at their
//! velocity, by channel; Offs, the sustain pedal and All Notes Off; unplugging lets go; the watchdog
//! never does; no target plays nothing; a new target leaves held notes where they sound; the target
//! is its song's; nothing waits for a compile; an export has no live note. The pitch wheel and the
//! mod wheel reach the target lane, and go back to rest when the keyboard lets go. Keys come as
//! `m2_os::midi` would give them: here MIDI can't be opened (`/dev/snd/seq` is EACCES).

use std::time::{Duration, Instant};

use super::super::audition::{Audition, Strike};
use super::super::tests::{Fixture, eventually};
use super::*;
use m2_editor::render::Length;

/// Two kick tracks: `kick` plays its `hits`, `snare` its own `hits`.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { kick } from '@m2/synths';\nimport { sum } from 'std/math';\n\
                    const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\n\
                    const s = track('snare', ({ lane }) => kick(lane.notes('hits')));\n\
                    export default song(() => sum(k.out, s.out));\n";

/// The song open, a keyboard named "Keys" plugged in, playing `kick.hits`.
fn played() -> (Fixture, Graph) {
    let fixture = Fixture::new();
    fixture.put("song.ts", SONG);
    let graph = fixture.open();
    assert_eq!(graph.midi(Event::Plugged("Keys")).as_deref(), Some("Keys is connected. It plays the selected track."));
    graph.midi_target(Some(("kick".into(), "hits".into()))).unwrap();
    (fixture, graph)
}

fn send(graph: &Graph, bytes: &[u8]) {
    assert_eq!(graph.midi(Event::Bytes("Keys", bytes)), None);
}

fn live(graph: &Graph) -> u32 {
    graph.activity().live
}

/// Nothing changes for a while: what sounds stays.
fn stays(graph: &Graph, n: u32, why: &str) {
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(live(graph), n, "{why}");
}

#[test]
fn a_midi_key_plays_the_selected_track() {
    let (_f, graph) = played();
    send(&graph, &[0x90, 36, 100]);
    eventually("the key sounds", || live(&graph) == 1);
    let state = graph.state().unwrap();
    assert_eq!(state["midi"], json!({"devices": ["Keys"], "target": {"module": "kick", "lane": "hits"}}));
    assert_eq!(state["live"], json!(1));
}

#[test]
fn note_off_and_velocity_zero_end_it() {
    let (_f, graph) = played();
    send(&graph, &[0x90, 36, 100]);
    eventually("the key sounds", || live(&graph) == 1);
    send(&graph, &[0x80, 36, 64]);
    eventually("a Note Off ends it", || live(&graph) == 0);
    send(&graph, &[0x90, 38, 90]);
    eventually("another sounds", || live(&graph) == 1);
    send(&graph, &[0x90, 38, 0]);
    eventually("a Note On at velocity 0 ends it", || live(&graph) == 0);
}

// The key's velocity, out of 127; each channel's keys its own; the rest is nothing.
#[test]
fn velocity_is_the_keys() {
    assert_eq!(read(&[0x90, 60, 127]), Said::On { h: 60, pitch: 60.0, velocity: 1.0 });
    assert_eq!(read(&[0x90, 60, 32]), Said::On { h: 60, pitch: 60.0, velocity: 32.0 / 127.0 });
    assert_eq!(read(&[0x9F, 0, 1]), Said::On { h: 15 * 128, pitch: 0.0, velocity: 1.0 / 127.0 });
    assert_eq!(read(&[0x83, 60, 0]), Said::Off { h: 3 * 128 + 60 });
    assert_eq!(read(&[0xB0, 64, 63]), Said::Pedal(false));
    assert_eq!(read(&[0xB2, 64, 64]), Said::Pedal(true));
    assert_eq!(read(&[0xB0, 123, 0]), Said::AllOff);
    assert_eq!(read(&[0xB0, 120, 0]), Said::AllOff);
    for ignored in [&[0xB0, 7, 64][..], &[0xE0, 64], &[0xD0, 30], &[0xC0, 5], &[0xF8], &[0x90, 60], &[]] {
        assert_eq!(read(ignored), Said::Nothing, "{ignored:?}");
    }
}

#[test]
fn channels_dont_share_a_key() {
    let (_f, graph) = played();
    send(&graph, &[0x90, 36, 100]);
    send(&graph, &[0x91, 36, 100]);
    eventually("one key on each of two channels: two notes", || live(&graph) == 2);
    send(&graph, &[0x80, 36, 0]);
    eventually("channel 1's Off ends its own", || live(&graph) == 1);
    send(&graph, &[0x81, 36, 0]);
    eventually("and channel 2's its own", || live(&graph) == 0);
}

#[test]
fn the_sustain_pedal_holds_offs_until_it_lifts() {
    let (_f, graph) = played();
    send(&graph, &[0x90, 36, 100]);
    eventually("the key sounds", || live(&graph) == 1);
    send(&graph, &[0xB0, 64, 127]);
    send(&graph, &[0x80, 36, 0]);
    stays(&graph, 1, "the pedal holds it after its key is up");
    // Struck again under the pedal: one note, not two, and the pedal's lift doesn't end the new one
    // while its key is down.
    send(&graph, &[0x90, 36, 80]);
    stays(&graph, 1, "struck again: one note");
    send(&graph, &[0x90, 38, 80]);
    send(&graph, &[0x80, 38, 0]);
    eventually("two notes under the pedal", || live(&graph) == 2);
    send(&graph, &[0xB0, 64, 0]);
    eventually("the pedal's lift ends the one let go", || live(&graph) == 1);
    send(&graph, &[0x80, 36, 0]);
    eventually("the held key ends with its Off", || live(&graph) == 0);
}

#[test]
fn all_notes_off_lets_go() {
    let (_f, graph) = played();
    for cc in [123, 120] {
        send(&graph, &[0x90, 36, 100]);
        send(&graph, &[0x92, 40, 100]);
        eventually("two keys sound", || live(&graph) == 2);
        send(&graph, &[0xB5, cc, 0]);
        eventually("CC {cc} lets go of every note", || live(&graph) == 0);
    }
}

#[test]
fn unplugging_a_keyboard_lets_go_of_its_notes() {
    let (_f, graph) = played();
    send(&graph, &[0x90, 36, 100]);
    eventually("the key sounds", || live(&graph) == 1);
    assert_eq!(graph.midi(Event::Unplugged("Keys")).as_deref(), Some("Keys was unplugged. Its notes stopped."));
    eventually("its note stops", || live(&graph) == 0);
    assert_eq!(graph.state().unwrap()["midi"]["devices"], json!([]));
    // A keyboard not plugged in plays nothing, and going twice says nothing.
    send(&graph, &[0x90, 36, 100]);
    stays(&graph, 0, "a keyboard that went plays nothing");
    assert_eq!(graph.midi(Event::Unplugged("Keys")), None);
    assert_eq!(graph.midi(Event::Plugged("Keys")).as_deref(), Some("Keys is connected. It plays the selected track."));
    assert_eq!(graph.midi(Event::Plugged("Keys")), None, "plugged in once");
}

// Q6: a key held over 10 s says nothing (no keepalive), and the watchdog leaves it alone; a
// window's quiet note beside it is let go.
#[test]
fn a_held_midi_key_outlives_the_watchdog() {
    let (_f, graph) = played();
    send(&graph, &[0x90, 36, 100]);
    let window = Audition {
        module: Some("snare".into()),
        lane: Some("hits".into()),
        on: vec![Strike { h: 1, pitch: 38.0, velocity: 0.8 }],
        ..Audition::default()
    };
    graph.audition("w1", window).unwrap();
    eventually("both sound", || live(&graph) == 2);
    graph.expire_auditions_at(Instant::now() + Duration::from_secs(11));
    eventually("the quiet window is let go", || live(&graph) == 1);
    stays(&graph, 1, "the keyboard's key sounds on");
    send(&graph, &[0x80, 36, 0]);
    eventually("until its Off", || live(&graph) == 0);
}

#[test]
fn no_track_selected_plays_nothing() {
    let fixture = Fixture::new();
    fixture.put("song.ts", SONG);
    let graph = fixture.open();
    graph.midi(Event::Plugged("Keys"));
    send(&graph, &[0x90, 36, 100]);
    stays(&graph, 0, "no window said what plays");
    assert_eq!(graph.state().unwrap()["midi"]["target"], Value::Null);
    graph.midi_target(Some(("kick".into(), "hits".into()))).unwrap();
    graph.midi_target(None).unwrap();
    send(&graph, &[0x90, 38, 100]);
    stays(&graph, 0, "a window said nothing plays (the Song row)");
    // A lane the song doesn't have: nothing, and no error.
    graph.midi_target(Some(("kick".into(), "gone".into()))).unwrap();
    send(&graph, &[0x90, 40, 100]);
    stays(&graph, 0, "a lane that has gone plays nothing");
}

#[test]
fn a_new_target_leaves_held_notes_where_they_sound() {
    let (_f, graph) = played();
    send(&graph, &[0x90, 36, 100]);
    eventually("the key sounds on the kick", || live(&graph) == 1);
    graph.midi_target(Some(("snare".into(), "hits".into()))).unwrap();
    send(&graph, &[0x90, 38, 100]);
    eventually("the next key sounds on the snare", || live(&graph) == 2);
    send(&graph, &[0x80, 36, 0]);
    eventually("the first key's Off finds it on the kick", || live(&graph) == 1);
    send(&graph, &[0x80, 38, 0]);
    eventually("and the second's on the snare", || live(&graph) == 0);
}

// The target is its song's: another song opened plays nothing until a window names a lane of it.
#[test]
fn the_target_belongs_to_its_song() {
    let (_f, graph) = played();
    let other = Fixture::new();
    other.put("song.ts", SONG);
    graph.close();
    graph.open(&other.0).unwrap();
    assert_eq!(graph.state().unwrap()["midi"], json!({"devices": ["Keys"], "target": null}));
    send(&graph, &[0x90, 36, 100]);
    stays(&graph, 0, "the old song's target plays nothing here");
}

// RENDER-29: while the session compiles a slow save, a key is heard before the compile ends.
#[test]
fn midi_never_waits_for_a_compile() {
    let slow = |n: u64| {
        format!(
            "import {{ hz, track }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\nfor (let i = 0; i < {n}; i++) {{}}\n\
             export const b = track('b', () => osc.tri(hz(300)));\n"
        )
    };
    let fixture = Fixture::new();
    fixture.put("tracks/b.ts", &slow(0));
    fixture.put(
        "song.ts",
        "import { song, track } from '@m2/core';\nimport { kick } from '@m2/synths';\nimport { sum } from 'std/math';\n\
         import { b } from './tracks/b';\nconst k = track('kick', ({ lane }) => kick(lane.notes('hits')));\n\
         export default song(() => sum(k.out, b.out));\n",
    );
    let graph = std::sync::Arc::new(fixture.open());
    graph.midi(Event::Plugged("Keys"));
    graph.midi_target(Some(("kick".into(), "hits".into()))).unwrap();
    let saver = {
        let (g, dir) = (graph.clone(), fixture.0.clone());
        std::thread::spawn(move || {
            std::fs::write(dir.join("tracks/b.ts"), slow(100_000_000)).unwrap();
            g.poll()
        })
    };
    eventually("the session compiles", || graph.busy().is_some());
    let busy_from = graph.busy().unwrap();
    let t = Instant::now();
    send(&graph, &[0x90, 36, 100]);
    let sent_ms = t.elapsed().as_secs_f64() * 1e3;
    eventually("the key sounds", || live(&graph) == 1);
    let heard = graph.now_ns();
    assert!(saver.join().unwrap(), "the slow save was taken in");
    // The actor keeps a request's span once its answer is sent: wait for it.
    eventually("the compile's span is kept", || graph.busy_spans().iter().any(|s| s.0 == busy_from));
    let (start, end) = *graph.busy_spans().iter().rev().find(|s| s.0 == busy_from).expect("the compile's span");
    let compile_ms = (end - start) as f64 / 1e6;
    if compile_ms < 500.0 {
        eprintln!("void: the compile took only {compile_ms:.0} ms");
        return;
    }
    eprintln!("midi: sent in {sent_ms:.2} ms, heard {:.0} ms before a {compile_ms:.0} ms compile ended", (end - heard) as f64 / 1e6);
    assert!(sent_ms < 20.0, "sending took {sent_ms:.1} ms");
    assert!(heard < end, "heard only after the compile ended");
}

// RENDER-1 (a guard): a key held through an export is not in it: exports render the song offline.
#[test]
fn an_export_has_no_midi_note() {
    let (fixture, graph) = played();
    let export = |name: &str| {
        let e = graph.export(Length::Seconds(0.5)).unwrap();
        let path = fixture.0.join(name);
        e.render.write(&path, |_| {}).unwrap();
        std::fs::read(&path).unwrap()
    };
    let plain = export("a.wav");
    send(&graph, &[0x90, 36, 127]);
    eventually("the key sounds", || live(&graph) == 1);
    let held = export("b.wav");
    assert!(plain == held, "the same bytes");
}

fn wheels(graph: &Graph) -> u32 {
    graph.activity().wheels
}

// RENDER-14's Midi, D8: a keyboard's pitch wheel and mod wheel reach the lane it plays, live, on any
// channel; centred, or reset by CC 121, the lane is at rest again. Read as they come.
#[test]
fn a_keyboards_bend_reaches_its_lane() {
    assert_eq!(read(&[0xE5, 0x10, 0x50]), Said::Wheel([0xE5, 0x10, 0x50]));
    assert_eq!(read(&[0xB2, 1, 0xFF]), Said::Wheel([0xB2, 1, 0x7F]));
    assert_eq!(read(&[0xB0, 121, 0]), Said::Wheel([0xB0, 121, 0]));
    let (_f, graph) = played();
    send(&graph, &[0xE0, 0x7F, 0x7F]);
    eventually("the bend reaches the kick's lane", || wheels(&graph) == 1);
    send(&graph, &[0xE3, 0, 0x40]);
    eventually("centred, on channel 4", || wheels(&graph) == 0);
    send(&graph, &[0xB0, 1, 100]);
    eventually("the mod wheel", || wheels(&graph) == 1);
    send(&graph, &[0xB0, 121, 0]);
    eventually("reset all controllers", || wheels(&graph) == 0);
    graph.midi_target(None).unwrap();
    send(&graph, &[0xE0, 0x7F, 0x7F]);
    stays_still(&graph, "no target: no lane takes it");
}

/// Nothing changes for a while: no lane's wheels move.
fn stays_still(graph: &Graph, why: &str) {
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(wheels(graph), 0, "{why}");
}

// D8: when a keyboard lets go, the lane whose wheels it moved is back at rest: All Notes Off,
// another target (its held key sounds on where it is), ⌘. (a panic), turned off in Settings, and
// unplugged.
#[test]
fn a_keyboard_let_go_centres_its_bend() {
    let (_f, graph) = played();
    let bend = |why: &str| {
        send(&graph, &[0xE0, 0x7F, 0x7F]);
        send(&graph, &[0xB0, 1, 127]);
        eventually(why, || wheels(&graph) == 1);
    };
    bend("bent");
    send(&graph, &[0xB0, 123, 0]);
    eventually("All Notes Off", || wheels(&graph) == 0);
    bend("bent again");
    send(&graph, &[0x90, 36, 100]);
    eventually("a key held", || live(&graph) == 1);
    graph.midi_target(Some(("snare".into(), "hits".into()))).unwrap();
    eventually("another target: the kick's wheels at rest", || wheels(&graph) == 0);
    assert_eq!(live(&graph), 1, "the held key sounds on");
    bend("the snare's");
    graph.panic().unwrap();
    eventually("a panic", || wheels(&graph) == 0);
    bend("the snare's again");
    graph.set_midi_off(&["Keys".into()]);
    eventually("turned off in Settings", || wheels(&graph) == 0);
    graph.set_midi_off(&[]);
    bend("on again");
    assert_eq!(graph.midi(Event::Unplugged("Keys")).as_deref(), Some("Keys was unplugged. Its notes stopped."));
    eventually("unplugged", || wheels(&graph) == 0);
}
