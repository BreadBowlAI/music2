//! Live notes in the runtime (MON-7, 8b.6's audition): a note played on a lane, not from its
//! data. It sounds at the next block, stopped or playing; its id is the engine's; it survives
//! stop, seek, wrap and an activation that keeps its consumer; it ends by its Off, its source's
//! release or a panic; and what doesn't fit is refused and counted.

use m2_plugins::Owner;
use m2_plugins::probe::{self, Note};
use std::sync::atomic::Ordering;

use super::live::{Rig, at_bpm, flat, live, synth, with_notes};
use super::*;
use crate::engine::Msg;

/// 120 bpm: a beat is 24 000 frames.
const BEAT: usize = 24_000;

fn on(generation: u64, source: u32, handle: u32, pitch: f32) -> Msg {
    Msg::Live { generation, slot: 0, ev: LiveEv::On { source, handle, pitch, velocity: 0.8 } }
}

fn off(generation: u64, source: u32, handle: u32) -> Msg {
    Msg::Live { generation, slot: 0, ev: LiveEv::Off { source, handle } }
}

/// An engine playing `ir` with `data`, stopped at 0, and its renderer's generation.
fn rig(ir: &Ir, data: &DataSet) -> (Rig, u64) {
    let rig = Rig::of(&flat(ir), data, live(ir, data));
    let g = rig.e.current().unwrap().generation();
    (rig, g)
}

/// The live notes the engine says it holds, and the notes its voices and plugins hold.
fn held(rig: &Rig) -> (u32, usize) {
    (rig.e.status().live, rig.e.current().unwrap().held_notes())
}

// MON-7: a live note sounds from the next block, stopped, and its Off ends it there.
#[test]
fn a_live_note_sounds_and_ends() {
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &DataSet::empty());
    assert!(peak(&rig.run(4 * BLOCK)) == 0.0);
    rig.send(on(g, 1, 7, 69.0));
    let out = rig.run(40 * BLOCK);
    assert!(peak(&out[..BLOCK]) > 0.1 && peak(&out[out.len() - BLOCK..]) > 0.5, "it sounds at once, stopped");
    assert_eq!(held(&rig), (1, 1));
    rig.send(off(g, 1, 7));
    assert!(peak(&rig.run(40 * BLOCK)) == 0.0, "its Off ends it at the block line");
    assert_eq!(held(&rig), (0, 0));
    assert_eq!(rig.e.refused_live(), 0);
}

/// The probe on the notes lane: what it is sent is recorded.
fn probed(data: &DataSet) -> (Rig, u64, m2_plugins::Handle) {
    m2_plugins::test_main_thread();
    probe::install();
    let mut pk = std_packages();
    let mut ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"}, {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"notes": "n"}}]),
        "p",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    assert!(pk.bind_plugins(&mut ir).is_empty());
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::prepare(&flat, data, &pk, SR, &mut audio::Cache::default(), &Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let (g, h) = (r.generation(), r.plugin_handles()[0]);
    (Rig::of(&flat, data, r), g, h)
}

// MON-7, RENDER-26a: live notes take the engine's ids, so a plugin, which matches its Offs by id,
// never sees one twice, beside the song's notes through loop wraps, and every Off finds its note.
#[test]
fn live_ids_never_meet_scheduled_ids() {
    let ns: Vec<Value> = (0..4).map(|k| json!({"pitch": 60 + k, "start": k as f64 * 0.25, "length": 0.2})).collect();
    let data = with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}), at_bpm(120.0));
    let (mut rig, g, h) = probed(&data);
    rig.cmd(Cmd::Play);
    rig.cmd(Cmd::Loop(Some((0.0, 1.0))));
    for k in 0..24 {
        rig.send(on(g, 1, k, 70.0 + (k % 5) as f32));
        rig.run(29 * BLOCK);
        rig.send(off(g, 1, k));
        rig.run(7 * BLOCK);
    }
    rig.cmd(Cmd::Stop);
    rig.run(4 * BLOCK);
    let stats = probe::stats(h).unwrap();
    let n = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
    assert_eq!((n(&stats.repeated_ids), n(&stats.unknown_offs)), (0, 0));
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 0, "every note ended");
    let ons = stats.notes().iter().filter(|x| x.1 == Note::On).count();
    assert!(ons >= 24 + 4 * 4, "the live notes and the song's: {ons}");
    assert_eq!(held(&rig), (0, 0));
}

// MON-7, RENDER-9, RENDER-26a: a held key is a hand, not the song: stop, seek and a loop wrap end
// the song's notes and leave a live one sounding.
#[test]
fn a_live_note_survives_stop_seek_and_wrap() {
    let data = with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 4}]}]}), at_bpm(120.0));
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &data);
    rig.cmd(Cmd::Play);
    rig.send(on(g, 1, 7, 69.0));
    rig.run(BLOCK);
    assert_eq!(held(&rig), (1, 2), "the song's note and the live one");
    let sounds = |rig: &mut Rig| peak(&rig.run(16 * BLOCK)) > 0.5 && held(rig) == (1, 1);
    rig.cmd(Cmd::Seek(6.0));
    assert!(sounds(&mut rig), "through a seek, which ended the song's note");
    rig.cmd(Cmd::Loop(Some((6.0, 7.0))));
    rig.run(2 * BEAT);
    assert!(sounds(&mut rig), "through two wraps");
    rig.cmd(Cmd::Stop);
    assert!(sounds(&mut rig), "through a stop");
    rig.cmd(Cmd::Loop(None));
    rig.cmd(Cmd::Seek(3.0));
    assert!(sounds(&mut rig), "through a seek while stopped");
    rig.cmd(Cmd::Play);
    assert!(sounds(&mut rig), "through a play inside the song's note, which isn't chased");
    assert!(rig.e.current().unwrap().gates_are_owned());
    rig.send(off(g, 1, 7));
    rig.run(BLOCK);
    assert_eq!(held(&rig), (0, 0));
}

// MON-7, RENDER-26a: a live note moves with its lane into the next plan, with its consumer kept,
// and sounds on: into a plan that keeps the scheduler, and into one that grows it; its Off ends it
// there.
#[test]
fn a_live_note_moves_with_its_lane_across_an_activation() {
    let plain = synth(0.05, json!([]), "v");
    let delayed = synth(0.05, json!([{"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v"}}]), "d");
    let data = DataSet::empty();
    // Notes enough to outgrow the scheduler: 300 at beat 64.
    let many: Vec<Value> = (0..300).map(|k| json!({"pitch": k % 100, "start": 64, "length": 1})).collect();
    let grown = with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 128, "notes": many}]}), at_bpm(120.0));
    let pk = std_packages();
    let (p, d) = (flat(&plain), flat(&delayed));
    let (mut rig, g) = rig(&plain, &data);
    rig.cmd(Cmd::Play);
    rig.send(on(g, 1, 7, 69.0));
    rig.run(8 * BLOCK);
    let caps = rig.e.current().unwrap().info().lanes[0].caps.unwrap();
    for (f, data) in [(&d, &data), (&p, &grown)] {
        let next = rig.prepare(f, data, &pk);
        rig.send(Msg::Activate { renderer: Box::new(next) });
        let out = rig.run(SR as usize / 10);
        assert_eq!(rig.e.current().unwrap().stats().kept.polys, 1);
        assert!(peak(&out[out.len() - 1000..]) > 0.5, "the kept voice sounds on");
        assert_eq!(held(&rig), (1, 1));
        assert!(rig.e.current().unwrap().gates_are_owned());
    }
    assert!(rig.e.current().unwrap().info().lanes[0].caps.unwrap().sounding > caps.sounding, "the lane grew");
    let g = rig.e.current().unwrap().generation();
    rig.send(off(g, 1, 7));
    rig.run(SR as usize / 5);
    assert_eq!(held(&rig), (0, 0));
    assert!(peak(&rig.run(BLOCK)) < 1e-4, "released");
}

// MON-7, RENDER-26a (S5): an activation that replaces the lane's consumer ends the live note in
// it, as it ends every note sounding into it; the note's Off later reaches the new consumer, which
// never had it, and is ignored. Nothing is left on.
#[test]
fn a_replaced_consumer_ends_its_live_notes() {
    let (gated, enveloped) = (synth(0.0, json!([]), "v"), synth(0.05, json!([]), "v"));
    let pk = std_packages();
    let (mut rig, g) = rig(&gated, &DataSet::empty());
    rig.send(on(g, 1, 7, 69.0));
    assert!(peak(&rig.run(8 * BLOCK)) > 0.5);
    let next = rig.prepare(&flat(&enveloped), &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(next) });
    let out = rig.run(SR as usize / 10);
    assert_eq!(rig.e.current().unwrap().stats().fresh.polys, 1, "a new poly");
    assert!(peak(&out[out.len() - 1000..]) == 0.0, "the old voice ended with its plan");
    assert_eq!(held(&rig), (1, 0), "the lane still holds it, and nothing sounds it");
    let g = rig.e.current().unwrap().generation();
    rig.send(off(g, 1, 7));
    rig.run(BLOCK);
    assert_eq!(held(&rig), (0, 0));
    assert!(peak(&rig.run(8 * BLOCK)) == 0.0);
}

// MON-7, RENDER-14: a key struck and let go at one block line sounds its On then, and its Off at
// the next block: at one frame an Off sorts before an On, which would leave the note on for good.
#[test]
fn a_key_struck_and_let_go_at_once_still_ends() {
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &DataSet::empty());
    rig.e.current_mut().unwrap().trace(16);
    rig.send(on(g, 1, 7, 69.0));
    rig.send(off(g, 1, 7));
    assert!(peak(&rig.run(BLOCK)) > 0.1, "it sounds for a block");
    rig.run(BLOCK);
    assert_eq!(held(&rig), (0, 0));
    let kinds: Vec<(u64, TraceKind)> = rig.e.current().unwrap().trace_entries().into_iter().map(|e| (e.engine, e.kind)).collect();
    assert_eq!(kinds, [(0, TraceKind::On { pitch: 69.0 }), (BLOCK as u64, TraceKind::Off)]);
}

// MON-7: a lane holds 16 live notes; the 17th On is refused and counted with the dropped Ons.
#[test]
fn a_full_table_refuses_and_counts() {
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &DataSet::empty());
    for h in 0..17 {
        rig.send(on(g, 1, h, 60.0 + h as f32));
    }
    rig.run(BLOCK);
    let s = rig.e.status();
    assert_eq!((s.live, s.dropped_ons), (16, 1));
    // An Off frees a place.
    rig.send(off(g, 1, 0));
    rig.send(on(g, 1, 16, 76.0));
    rig.run(2 * BLOCK);
    assert_eq!((rig.e.status().live, rig.e.status().dropped_ons), (16, 1));
}

// MON-7: a release ends every note of one source, on every lane, whatever slot it names, and
// leaves another source's.
#[test]
fn release_ends_one_sources_notes() {
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &DataSet::empty());
    rig.send(on(g, 1, 1, 60.0));
    rig.send(on(g, 1, 2, 64.0));
    rig.send(on(g, 2, 1, 67.0));
    rig.run(BLOCK);
    assert_eq!(held(&rig), (3, 3));
    rig.send(Msg::Live { generation: g, slot: 9, ev: LiveEv::Release { source: 1 } });
    rig.run(BLOCK);
    assert_eq!(held(&rig), (1, 1));
    // The same handle from the other source is its own note.
    rig.send(off(g, 2, 1));
    rig.run(BLOCK);
    assert_eq!(held(&rig), (0, 0));
    assert_eq!(rig.e.refused_live(), 0);
}

// MON-7, RENDER-10 (A10): a panic ends live notes, without Offs, as it ends the song's: their voices
// are reset.
#[test]
fn panic_ends_live_notes() {
    let (mut rig, g) = rig(&synth(0.05, json!([]), "v"), &DataSet::empty());
    rig.send(on(g, 1, 7, 69.0));
    rig.run(8 * BLOCK);
    rig.e.current_mut().unwrap().trace(64);
    rig.send(Msg::Panic);
    assert!(peak(&rig.run(8 * BLOCK)) == 0.0, "cut, not released");
    assert_eq!(held(&rig), (0, 0));
    let offs = rig.e.current().unwrap().trace_entries().into_iter().filter(|e| e.kind == TraceKind::Off).count();
    assert_eq!(offs, 0, "no Off is sent");
    // Its handle is free again.
    rig.send(on(g, 1, 7, 69.0));
    rig.run(BLOCK);
    assert_eq!(held(&rig), (1, 1));
}

// MON-7, RENDER-6a: a live note wakes an idle engine, which doesn't idle again while it is held.
#[test]
fn a_live_note_wakes_an_idle_engine() {
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &DataSet::empty());
    rig.run(20 * BLOCK);
    assert!(rig.e.is_idle());
    rig.send(on(g, 1, 7, 69.0));
    assert!(peak(&rig.run(BLOCK)) > 0.1, "heard in the block it wakes");
    rig.run(40 * BLOCK);
    assert!(!rig.e.is_idle());
    rig.send(off(g, 1, 7));
    rig.run(20 * BLOCK);
    assert!(rig.e.is_idle(), "idle again once it ends");
}

// MON-7, R2: a live note for another plan, for a slot this one doesn't have, or with
// a pitch or velocity out of range, is refused and counted, and nothing sounds.
#[test]
fn another_generations_live_is_refused() {
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &DataSet::empty());
    let bad = [
        on(g + 1000, 1, 1, 60.0),
        Msg::Live { generation: g, slot: 7, ev: LiveEv::On { source: 1, handle: 2, pitch: 60.0, velocity: 0.5 } },
        Msg::Live { generation: g, slot: 0, ev: LiveEv::On { source: 1, handle: 3, pitch: f32::NAN, velocity: 0.5 } },
        Msg::Live { generation: g, slot: 0, ev: LiveEv::On { source: 1, handle: 4, pitch: 128.5, velocity: 0.5 } },
        Msg::Live { generation: g, slot: 0, ev: LiveEv::On { source: 1, handle: 5, pitch: 60.0, velocity: 1.5 } },
    ];
    let n = bad.len() as u64;
    for m in bad {
        rig.send(m);
    }
    assert!(peak(&rig.run(8 * BLOCK)) == 0.0);
    assert_eq!((rig.e.refused_live(), held(&rig)), (n, (0, 0)));
}

// MON-7, RENDER-1 (M4): held for an export, nothing renders, so an On is refused (it would sound
// only when the export ends); an Off is kept, and sent at the resume.
#[test]
fn held_for_an_export_an_on_is_refused_and_an_off_waits() {
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &DataSet::empty());
    rig.send(on(g, 1, 7, 69.0));
    rig.run(8 * BLOCK);
    rig.send(Msg::Hold(true));
    rig.run(4 * BLOCK);
    assert!(rig.e.is_held());
    rig.send(off(g, 1, 7));
    rig.send(on(g, 1, 8, 72.0));
    rig.run(4 * BLOCK);
    assert_eq!(rig.e.refused_live(), 1, "the On");
    assert_eq!(rig.e.status().live, 1, "the Off waits");
    rig.send(Msg::Hold(false));
    rig.run(BLOCK);
    assert_eq!(held(&rig), (0, 0), "sent at the resume");
}

fn wheel(generation: u64, source: u32, bytes: [u8; 3]) -> Msg {
    Msg::Live { generation, slot: 0, ev: LiveEv::Midi { source, bytes } }
}

/// The lane's events in the last block.
fn events(rig: &Rig) -> Vec<Event> {
    rig.e.current().unwrap().lane_events(0).to_vec()
}

// RENDER-14, MON-7, D8: the wheels merge into their lane's buffer at the block line, at its first
// frame, before every note there: a live On, the song's On and an Off. Each is sent once, on channel
// 1 whatever channel moved it; the latest of each wins. Anything but the wheels is refused.
#[test]
fn a_live_bend_merges_before_the_notes() {
    let data =
        with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 0.5}]}]}), at_bpm(120.0));
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &data);
    rig.cmd(Cmd::Play);
    rig.send(wheel(g, 1, [0xE4, 0, 0x60]));
    rig.send(wheel(g, 1, [0xE4, 0x7F, 0x7F]));
    rig.send(wheel(g, 1, [0xB2, 1, 90]));
    rig.send(on(g, 1, 7, 69.0));
    rig.run(BLOCK);
    let ev = events(&rig);
    assert_eq!(ev[..2], [Event::Midi { frame: 0, bytes: [0xE0, 0x7F, 0x7F] }, Event::Midi { frame: 0, bytes: [0xB0, 1, 90] }], "{ev:?}");
    assert!(matches!(ev[2..], [Event::On { frame: 0, pitch: 69.0, .. }, Event::On { frame: 0, pitch: 60.0, .. }]), "{ev:?}");
    rig.run(BLOCK);
    assert!(events(&rig).is_empty(), "sent once");
    rig.send(off(g, 1, 7));
    rig.send(wheel(g, 1, [0xE0, 0, 0x40]));
    rig.run(BLOCK);
    assert!(
        matches!(events(&rig)[..], [Event::Midi { frame: 0, bytes: [0xE0, 0, 0x40] }, Event::Off { frame: 0, .. }]),
        "{:?}",
        events(&rig)
    );
    for bad in [[0xB0, 7, 100], [0x90, 60, 100], [0xE0, 0x80, 0], [0xF8, 0, 0]] {
        rig.send(wheel(g, 1, bad));
    }
    rig.send(Msg::Live { generation: g, slot: 3, ev: LiveEv::Midi { source: 1, bytes: [0xE0, 0, 0] } });
    rig.run(BLOCK);
    assert!(events(&rig).is_empty() && rig.e.refused_live() == 5);
}

// MON-7, RENDER-1 (M4), D8: held for an export, a wheel is not refused, as an On is: the latest
// waits in its lane's slot and is sent at the resume. Refused, a wheel let go while an export ran
// would stay bent. A source's release puts back at rest the wheels it moved, and only those.
#[test]
fn held_for_an_export_a_wheel_waits_for_the_resume() {
    let (mut rig, g) = rig(&synth(0.0, json!([]), "v"), &DataSet::empty());
    rig.send(wheel(g, 1, [0xE0, 0x7F, 0x7F]));
    rig.run(BLOCK);
    rig.send(Msg::Hold(true));
    rig.run(4 * BLOCK);
    assert!(rig.e.is_held());
    rig.send(wheel(g, 1, [0xE0, 0, 0x40]));
    rig.send(wheel(g, 2, [0xB0, 1, 50]));
    rig.run(4 * BLOCK);
    assert_eq!((rig.e.refused_live(), rig.e.status().wheels), (0, 1), "taken: the bend is centred, the mod wheel up");
    rig.send(Msg::Hold(false));
    rig.run(BLOCK);
    assert_eq!(
        events(&rig),
        [Event::Midi { frame: 0, bytes: [0xE0, 0, 0x40] }, Event::Midi { frame: 0, bytes: [0xB0, 1, 50] }],
        "sent at the resume"
    );
    rig.send(Msg::Live { generation: g, slot: 0, ev: LiveEv::Release { source: 1 } });
    rig.run(BLOCK);
    assert!(events(&rig).is_empty() && rig.e.status().wheels == 1, "source 1's bend was at rest already, and 2's wheel is 2's");
    rig.send(Msg::Live { generation: g, slot: 0, ev: LiveEv::Release { source: 2 } });
    rig.run(BLOCK);
    assert_eq!((events(&rig), rig.e.status().wheels), (vec![Event::Midi { frame: 0, bytes: [0xB0, 1, 0] }], 0));
}

// RENDER-14, D8: a plugin on the lane hears its wheels as CLAP MIDI events, on channel 1, at the
// block's first frame and before the notes there.
#[test]
fn a_plugin_hears_the_bend_as_midi() {
    let (mut rig, g, h) = probed(&DataSet::empty());
    rig.send(wheel(g, 1, [0xE2, 0x7F, 0x7F]));
    rig.send(wheel(g, 1, [0xB0, 1, 100]));
    rig.send(on(g, 1, 7, 60.0));
    rig.run(BLOCK);
    let notes = probe::stats(h).unwrap().notes();
    let midi = |b: [u8; 3]| i32::from_le_bytes([b[0], b[1], b[2], 0]);
    let kinds: Vec<(i64, Note)> = notes.iter().map(|n| (n.0, n.1)).collect();
    assert_eq!(kinds, [(0, Note::Midi), (0, Note::Midi), (0, Note::On)], "{notes:?}");
    assert_eq!((notes[0].2, notes[1].2), (midi([0xE0, 0x7F, 0x7F]), midi([0xB0, 1, 100])));
}
