//! The live engine at the runtime's level (step 3a): two clocks, jumps that reset nothing,
//! events, the range render, activation and the engine's message order.

use std::collections::VecDeque;

use m2_song::{Arrangement, LaneData, LaneKind};

use super::alloc_count::during;
use super::goldens::addr;
use super::*;
use crate::engine::{CallbackStamp, Engine, Envelope, Msg, Retired};

fn notes(v: Value) -> LaneData {
    LaneData::read(&v.to_string(), &LaneKind::Notes).unwrap()
}

pub(super) fn with_notes(v: Value, arr: Arrangement) -> DataSet {
    DataSet::with_lanes(arr, [(addr("notes"), notes(v))])
}

pub(super) fn at_bpm(bpm: f64) -> Arrangement {
    serde_json::from_value(json!({"tempo": [{"beat": 0, "bpm": bpm}]})).unwrap()
}

/// A poly of sine voices at their notes' pitches, through an envelope that releases over
/// `release` seconds (0: the gate alone), then the nodes `after`, with output `out`.
fn synth(release: f64, after: Value, out: &str) -> Ir {
    let mut body = vec![
        json!({"id": "p", "op": "lane", "lane": "pitch"}),
        json!({"id": "g", "op": "lane", "lane": "gate"}),
        json!({"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}}),
        json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}}),
    ];
    if release > 0.0 {
        body.extend([
            json!({"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g", "sustain": "s", "release": "r"}}),
            json!({"id": "s", "op": "const", "value": 1}),
            json!({"id": "r", "op": "const", "value": release, "unit": "seconds"}),
            json!({"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}}),
        ]);
    } else {
        body.push(json!({"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}));
    }
    let mut nodes = vec![
        json!({"id": "n", "op": "lane", "lane": "notes"}),
        json!({"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "amp", "nodes": body}}),
    ];
    nodes.extend(after.as_array().unwrap().iter().cloned());
    song(Value::Array(nodes), out, json!([{"key": "notes", "kind": "events<note>"}]))
}

/// A renderer as the live engine gets it: not strict, stopped at 0.
pub(super) fn live(ir: &Ir, data: &DataSet) -> Renderer {
    let pk = std_packages();
    let flat = check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::new(&flat, data, &pk, SR).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    r
}

/// An engine driven by hand: messages in, what it retires out.
pub(super) struct Rig {
    pub(super) e: Engine,
    pub(super) inbox: VecDeque<Envelope>,
    retired: Vec<Retired>,
    /// Refuse to take retired objects, as a full channel does.
    refuse: bool,
    seq: u64,
}

impl Rig {
    pub(super) fn new(r: Renderer) -> Rig {
        Rig { e: Engine::with(Box::new(r)), inbox: VecDeque::new(), retired: vec![], refuse: false, seq: 0 }
    }

    fn send(&mut self, msg: Msg) -> u64 {
        self.seq += 1;
        self.inbox.push_back(Envelope { seq: self.seq, msg });
        self.seq
    }

    pub(super) fn cmd(&mut self, c: Cmd) -> u64 {
        self.send(Msg::Transport(c))
    }

    pub(super) fn run(&mut self, frames: usize) -> Vec<[f32; 2]> {
        let mut out = vec![[0.0; 2]; frames];
        let Rig { e, inbox, retired, refuse, .. } = self;
        let mut retire = |r: Retired| {
            if *refuse {
                return Err(r);
            }
            retired.push(r);
            Ok(())
        };
        e.process(&mut out, &mut || inbox.pop_front(), &mut retire, CallbackStamp::default());
        out
    }

    /// The renderers retired so far.
    fn retired_renderers(&self) -> Vec<&Renderer> {
        self.retired.iter().filter_map(|r| if let Retired::Renderer(r) = r { Some(&**r) } else { None }).collect()
    }
}

/// The largest step from one sample to the next.
fn max_step(x: &[[f32; 2]]) -> f32 {
    x.windows(2).map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs())).fold(0.0, f32::max)
}

/// A small deterministic generator, for "random" clocks that reproduce.
fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

// RENDER-9 item 4, RENDER-10: a seek is a transport event. The delay's echoes ring on through it
// exactly as if nothing had happened, while a panic cuts them.
#[test]
fn a_jump_never_resets_a_kernel() {
    let ir = synth(
        0.0,
        json!([
            {"id": "t", "op": "const", "value": 0.25, "unit": "seconds"},
            {"id": "f", "op": "const", "value": 0.9},
            {"id": "m", "op": "const", "value": 1},
            {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v", "time": "t", "feedback": "f", "mix": "m"}}]),
        "d",
    );
    let data =
        with_notes(json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 69, "start": 0, "length": 0.1}]}]}), at_bpm(120.0));
    let run = |jump: Option<Msg>| {
        let mut rig = Rig::new(live(&ir, &data));
        rig.cmd(Cmd::Play);
        let mut out = rig.run(SR as usize * 6 / 10);
        if let Some(m) = jump {
            rig.send(m);
        }
        out.extend(rig.run(SR as usize / 2));
        (out, rig.e.resets())
    };
    let (plain, _) = run(None);
    assert!(peak(&plain[SR as usize * 7 / 10..SR as usize * 8 / 10]) > 0.1, "the third echo");
    let (seeked, resets) = run(Some(Msg::Transport(Cmd::Seek(16.0))));
    assert!(seeked == plain, "a seek leaves the echoes exactly as they were");
    let (stopped, _) = run(Some(Msg::Transport(Cmd::Stop)));
    assert!(stopped == plain, "so does a stop");
    assert_eq!(resets, 0);
    let (panicked, resets) = run(Some(Msg::Panic));
    assert_eq!(resets, 1);
    assert!(peak(&panicked[SR as usize * 61 / 100..]) == 0.0, "a panic resets everything");
}

// RENDER-10: every reset of a renderer's graph is counted, wherever it comes from: an offline seek
// replays from one, and a panic resets the playing renderer and, during a fade, the fading one;
// the count outlives the renderer retired. Jumps, a swap and an activation reset nothing. The
// gates' "no kernel reset" checks read this count.
#[test]
fn every_graph_reset_is_counted() {
    let ir = synth(0.0, json!([]), "v");
    let data = with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 1}]}]}), at_bpm(120.0));
    let mut offline = live(&ir, &data);
    assert_eq!(offline.resets(), 0, "building isn't a reset");
    offline.seek(1000).unwrap();
    assert_eq!(offline.resets(), 1, "an offline seek replays from a reset (RENDER-1)");
    let mut rig = Rig::new(live(&ir, &data));
    rig.cmd(Cmd::Play);
    rig.cmd(Cmd::Loop(Some((0.0, 1.0))));
    rig.run(SR as usize);
    rig.cmd(Cmd::Seek(0.5));
    rig.run(BLOCK);
    rig.cmd(Cmd::Stop);
    rig.run(BLOCK);
    rig.cmd(Cmd::Play);
    let generation = rig.e.current().unwrap().generation();
    rig.send(Msg::SwapLane { generation, slot: 0, data: LaneSwap::Notes(Arc::new(notes_data(&data, "song", "notes", SR as f64))) });
    rig.send(Msg::Activate { renderer: Box::new(live(&ir, &data)) });
    rig.run(SR as usize / 10);
    assert_eq!((rig.e.resets(), rig.e.activations(), rig.e.seeks()), (0, 1, 1), "wraps, a seek, a stop, a swap and an activation");
    rig.send(Msg::Activate { renderer: Box::new(live(&ir, &data)) });
    rig.send(Msg::Panic);
    rig.run(BLOCK);
    assert!(rig.e.fading().is_some());
    assert_eq!(rig.e.resets(), 2, "a panic during a fade resets both renderers");
    rig.run(SR as usize / 10);
    assert!(rig.e.fading().is_none() && rig.retired_renderers().len() == 2);
    assert_eq!(rig.e.resets(), 2, "counted after the faded renderer is retired");
}

// RENDER-13, RENDER-11: a voice's gate closes at its note's Off. A seek into the middle of a
// note ends it there, and it isn't chased, though the transport is inside it.
#[test]
fn voices_gate_on_events_not_on_note_times() {
    let ir = synth(0.0, json!([]), "v");
    let data = with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 69, "start": 0, "length": 4}]}]}), at_bpm(120.0));
    let mut r = live(&ir, &data);
    r.trace(64);
    let mut rig = Rig::new(r);
    rig.cmd(Cmd::Play);
    assert!(peak(&rig.run(SR as usize / 2)) > 0.9);
    rig.cmd(Cmd::Seek(2.0));
    // The seek lands at the next block line: 24 064.
    let after = rig.run(SR as usize / 2);
    assert!(peak(&after[64..]) == 0.0, "silent from the seek on, inside the note");
    let cur = rig.e.current().unwrap();
    let offs: Vec<TraceEntry> = cur.trace_entries().into_iter().filter(|e| e.kind == TraceKind::Off).collect();
    assert_eq!(offs.iter().map(|e| (e.engine, e.transport)).collect::<Vec<_>>(), [(24_064, 48_000)]);
    assert_eq!((cur.held_notes(), cur.sounding_notes()), (0, 0));
}

// RENDER-6a, RENDER-7: stopped, the engine keeps running. The transport, control lanes and time
// hold at the stop position, while kernels go on: a free-running sine keeps moving.
#[test]
fn stopped_lanes_and_time_hold_while_kernels_run() {
    let ir = song(
        json!([
            {"id": "c", "op": "lane", "lane": "level"},
            {"id": "t", "op": "time", "of": "beats"},
            {"id": "l", "op": "kernel", "kernel": "std:lfo", "inputs": {"beats": "t"}},
            {"id": "o", "op": "kernel", "kernel": "std:sine"},
            {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "c"}},
            {"id": "b", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "a", "by": "l"}}]),
        "b",
        json!([{"key": "level", "kind": "control<gain>", "default": 0}]),
    );
    let (tl, dir) = timeline_for(
        &ir,
        &[("data/song/level.json", json!({"unit": "gain", "clips": [{"start": 0, "length": 8, "points": [[0, 0], [8, 1]]}]}))],
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let node = |name: &str| flat.nodes.iter().position(|n| n.name == name).unwrap();
    let (c, t, l) = (node("song/c"), node("song/t"), node("song/l"));
    let mut rig = Rig::new(live(&ir, &tl));
    rig.cmd(Cmd::Play);
    // Off the beat, where the lfo isn't 0.
    rig.run(SR as usize * 11 / 10);
    rig.cmd(Cmd::Stop);
    rig.run(256);
    let stop = rig.e.status();
    assert!(!stop.playing);
    let held = |rig: &Rig, i: usize| rig.e.current().unwrap().graph.bufs[i][0].clone();
    let (c0, t0, l0) = (held(&rig, c), held(&rig, t), held(&rig, l));
    assert!((t0[0] as f64 - stop.beats).abs() < 1e-4, "time holds the stop position: {} vs {}", t0[0], stop.beats);
    assert!(c0[0] > 0.2, "the lane holds its value there");
    let mut blocks = vec![];
    for _ in 0..4 {
        blocks.push(rig.run(BLOCK));
        for (i, first) in [(c, c0[0]), (t, t0[0]), (l, l0[0])] {
            assert!(held(&rig, i).iter().all(|&x| x == first), "node {i} holds");
        }
    }
    assert_eq!(rig.e.status().pos, stop.pos, "the transport is frozen");
    assert!(blocks.windows(2).all(|w| w[0] != w[1]) && peak(&blocks[3]) > 0.05, "the sine runs on");
    std::fs::remove_dir_all(dir).ok();
}

// RENDER-9 item 5: an audio lane crossfades over 5 ms at a loop wrap, which falls on its exact
// frame mid-block; once the fade is done it plays the material bit for bit.
#[test]
fn an_audio_lane_crossfades_at_a_wrap() {
    let (ir, tl, dir) =
        audio_song(&[("a441.wav", vec![sine(441.0, 3.0, SR)], SR)], json!([{"start": 0, "length": 4, "asset": "a441.wav"}]));
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let material = Renderer::new(&flat, &tl, &pk, SR).unwrap().render(1.0).unwrap();
    let own = max_step(&material);
    // 0.9 beats at 120 bpm: the wrap is at 21 600, mid-block, where a cut would step.
    let wrap = 21_600;
    assert!((material[wrap][0] - material[0][0]).abs() > 2.0 * own, "a hard cut would click");
    let mut rig = Rig::new(live(&ir, &tl));
    rig.cmd(Cmd::Play);
    rig.cmd(Cmd::Loop(Some((0.0, 0.9))));
    let out = rig.run(SR as usize);
    assert!(out[..wrap] == material[..wrap], "exact before the wrap");
    assert!(max_step(&out) <= 2.0 * own, "{} against the material's own {own}", max_step(&out));
    assert!(out[wrap + 240..wrap + 2000] == material[240..2000], "exact once the fade is done");
    assert_eq!(rig.e.resets(), 0);
    std::fs::remove_dir_all(dir).ok();
}

// RENDER-14, RENDER-28, HOST-6: a plugin hears the lane's notes and its sampled parameters in the
// order a stable sort by time gave before: parameters after notes at one frame. Built without
// allocating.
#[test]
fn plugin_events_merge_as_a_stable_sort_did() {
    use m2_plugins::Event as P;
    let on = |frame, id, pitch| Event::On { frame, id, pitch, velocity: 0.5, from_pitch: None };
    let off = |frame, id| Event::Off { frame, id, velocity: 0.0 };
    // Two notes held from before (1, 2), three starting here, one of which ends here.
    let notes = [off(0, 1), on(0, 5, 60.5), off(16, 2), on(16, 6, 62.0), off(40, 6), on(40, 7, 64.0)];
    let ramp: Vec<f32> = (0..BLOCK).map(|f| (f / 32) as f32).collect();
    let bufs = vec![vec![ramp]];
    let port = Input::Port { src: PortRef { node: 0, port: 0 }, src_channels: 1, channels: 1 };
    let mut params = vec![(11, port, None), (10, Input::Const(0.5), None)];
    let mut keys: Vec<(u32, i16)> = Vec::with_capacity(16);
    keys.extend([(1, 60), (2, 61)]);
    let mut out = m2_plugins::Events::with_capacity(64);
    let mut next = 0;
    let (_, allocs) = during(|| plugin_events(&[], &[], &notes, &mut next, &mut params, &bufs, (0, BLOCK), &mut keys, &mut out));
    assert_eq!((allocs, out.capacity(), out.dropped(), next), (0, 64, 0, notes.len()));
    // Before: held notes' offs, then each new note's events, then parameters, stably sorted.
    let mut before = vec![P::NoteOff { time: 0, id: 1, key: 60 }, P::NoteOff { time: 16, id: 2, key: 61 }];
    before.extend([
        P::NoteOn { time: 0, id: 5, key: 61, velocity: 0.5 },
        P::Tuning { time: 0, id: 5, key: 61, semitones: -0.5 },
        P::NoteOn { time: 16, id: 6, key: 62, velocity: 0.5 },
        P::NoteOff { time: 40, id: 6, key: 62 },
        P::NoteOn { time: 40, id: 7, key: 64, velocity: 0.5 },
    ]);
    for f in [0, 32, 64, 96] {
        before.push(P::Param { time: f, id: 11, value: (f / 32) as f64 });
    }
    before.push(P::Param { time: 0, id: 10, value: 0.5 });
    let time = |e: &P| match *e {
        P::NoteOn { time, .. } | P::NoteOff { time, .. } | P::NoteChoke { time, .. } | P::Tuning { time, .. } | P::Param { time, .. } => {
            time
        }
    };
    before.sort_by_key(time);
    assert_eq!(out.iter().collect::<Vec<_>>(), before);
    assert_eq!(keys, [(5, 61), (7, 64)], "held: the notes that started and didn't end");
    // RENDER-10, RENDER-26a: what a plugin held when a panic reset it is choked and let go first,
    // as a plugin may keep its voices through a reset. C7: what a carried instance held that the
    // new data dropped is let go, with no choke, after them.
    let (mut next, mut none) = (0, Vec::with_capacity(4));
    let release = [(3, 70)];
    let (sent, allocs) = during(|| plugin_events(&keys, &release, &[], &mut next, &mut [], &bufs, (0, BLOCK), &mut none, &mut out));
    assert_eq!((allocs, sent), (0, (3, Some(0))));
    assert_eq!(
        out.iter().collect::<Vec<_>>(),
        [
            P::NoteChoke { time: 0, id: 5, key: 61 },
            P::NoteOff { time: 0, id: 5, key: 61 },
            P::NoteChoke { time: 0, id: 7, key: 64 },
            P::NoteOff { time: 0, id: 7, key: 64 },
            P::NoteOff { time: 0, id: 3, key: 70 }
        ]
    );
    // A full list counts what it can't hold, and never grows.
    let mut small = m2_plugins::Events::with_capacity(2);
    let (_, allocs) = during(|| plugin_events(&keys, &[], &[], &mut next, &mut [], &bufs, (0, BLOCK), &mut none, &mut small));
    assert_eq!((allocs, small.len(), small.dropped()), (0, 2, 2));
}

// RENDER-4, RENDER-5: a range render starts from init, the transport at its start, with jump
// semantics. From 0 it is the offline render; from later it is deterministic, and a note sounding
// at its start isn't chased.
#[test]
fn range_renders_start_fresh_and_dont_chase() {
    let pk = std_packages();
    let (ir, data) = goldens::g2();
    let flat = check(&ir, &pk).unwrap();
    let full = Renderer::new(&flat, &data, &pk, SR).unwrap().render(1.0).unwrap();
    assert!(Renderer::starting_at(&flat, &data, &pk, SR, 0).unwrap().render(1.0).unwrap() == full, "from 0: exact");
    let range = |t0| Renderer::starting_at(&flat, &data, &pk, SR, t0).unwrap().render(0.5).unwrap();
    assert!(range(12_345) == range(12_345), "deterministic");
    let held = synth(0.0, json!([]), "v");
    let data = with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 69, "start": 0, "length": 4}]}]}), at_bpm(120.0));
    let flat = check(&held, &pk).unwrap();
    assert!(peak(&Renderer::starting_at(&flat, &data, &pk, SR, 24_000).unwrap().render(0.5).unwrap()) == 0.0, "not chased");
    let mut replay = Renderer::new(&flat, &data, &pk, SR).unwrap();
    replay.seek(24_000).unwrap();
    assert!(peak(&replay.render(0.5).unwrap()) > 0.9, "the exact render has it");
}

/// Delays and reverbs inside voices that are stolen again and again, with a cell in the voices
/// (the reverb's mix) and a smooth one at the top (the level).
fn retriggered() -> (Ir, DataSet) {
    let ir = cell::framed(song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "r", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}},
                {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "a"}},
                {"id": "wet", "op": "const", "value": 0.25, "origin": {"site": 2, "arg": ["0"]}},
                {"id": "r", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": "d", "mix": "wet"}}]}},
            cell::smooth_const("lvl", 0.8, 3),
            {"id": "out", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "v", "by": "lvl"}}]),
        "out",
        json!([{"key": "notes", "kind": "events<note>"}]),
    ));
    let ns: Vec<Value> = (0..8).map(|k| json!({"pitch": 48 + k, "start": k as f64 * 0.25, "length": 0.2})).collect();
    (ir, with_notes(json!({"clips": [{"start": 0, "length": 4, "notes": ns}]}), at_bpm(120.0)))
}

// RENDER-28, §1.5 of the step 3a design: a kernel's first reset may grow its memory (a delay's
// from 896 to 1664 KiB); building resets every kernel once, so stealing voices on the audio
// thread later never grows memory.
#[test]
fn prewarming_keeps_kernel_memory_flat() {
    let (ir, data) = retriggered();
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let mut r = Renderer::new(&flat, &data, &pk, SR).unwrap();
    let before = r.kernel_memory_bytes();
    r.render(2.5).unwrap();
    assert_eq!(r.kernel_memory_bytes(), before, "eight steals later");
    // What pre-warming spares the audio thread: the first reset grows a fresh graph's memory.
    let mut store = Store::new(&pk.engine, StoreLimitsBuilder::new().build());
    let slots = lane_slots(&flat);
    let lanes = vec![LaneSlot::Notes(NoteScheduler::new(Arc::new(NotesData::empty())))];
    let (owner, mut fresh) = (m2_plugins::Owner::discard(), crate::Fresh);
    let mut g = Builder {
        store: &mut store,
        packages: &pk,
        data: &data,
        sample_rate: SR as f64,
        slots: &slots,
        lanes: &lanes,
        source: &mut fresh,
        owner: &owner,
        ordinals: Default::default(),
    }
    .graph(&flat.nodes, &flat.order, false)
        .unwrap();
    let fresh = g.kernel_memory(&store);
    g.prewarm(&mut store).unwrap();
    assert!(g.kernel_memory(&store) > fresh);
}

// RENDER-28: once a thread has rendered a block, rendering, the engine's messages and cell
// writes (a step in a voice, a ramp at the top) allocate nothing. (core/play's trap checks the
// audio callback from its very first fill.)
#[test]
fn rendering_live_allocates_nothing() {
    let (ir, data) = retriggered();
    let mut rig = Rig::new(live(&ir, &data));
    rig.cmd(Cmd::Play);
    rig.run(BLOCK);
    let flat = check(&ir, &std_packages()).unwrap();
    let next = live(&ir, &data);
    let generation = rig.e.current().unwrap().generation();
    let lane = LaneSwap::Notes(Arc::new(notes_data(&data, "song", "notes", SR as f64)));
    let msgs = [
        Msg::Transport(Cmd::Seek(0.5)),
        Msg::Transport(Cmd::Loop(Some((0.0, 1.0)))),
        Msg::SwapLane { generation, slot: 0, data: lane },
        Msg::Activate { renderer: Box::new(next) },
        Msg::Transport(Cmd::Stop),
        Msg::Panic,
        Msg::Transport(Cmd::Play),
    ];
    for m in msgs {
        rig.send(m);
    }
    let mut out = vec![[0.0; 2]; 300];
    let mut kept: Vec<Retired> = Vec::with_capacity(64);
    let (slots, map) = (rig.e.current().unwrap().cell_slots(), rig.e.current().unwrap().cell_map());
    let ids: Vec<CellId> = map.cells(&cell::origin(2)).iter().chain(map.cells(&cell::origin(3))).copied().collect();
    assert_eq!(ids.len(), 2, "a cell in the voices and one at the top");
    let Rig { e, inbox, .. } = &mut rig;
    let ((), allocs) = during(|| {
        for k in 0..200 {
            slots.set(&ids, 0.1 + (k % 7) as f32 * 0.1, k + 1);
            let mut retire = |r: Retired| {
                if kept.len() == kept.capacity() {
                    return Err(r);
                }
                kept.push(r);
                Ok(())
            };
            e.process(&mut out, &mut || inbox.pop_front(), &mut retire, CallbackStamp::default());
        }
    });
    assert_eq!(allocs, 0);
    // The swap's old data and the replaced renderer; cell writes retire nothing.
    assert!(inbox.is_empty() && kept.len() == 2, "{} retired", kept.len());
    // The panic came during the activation's fade: it reset both renderers.
    assert_eq!((e.resets(), e.activations()), (2, 1));
    drop(flat);
}

// RENDER-25a: an activation crossfades the whole output over 5 ms, at the engine's position
// (no jump back), whichever block it lands on. Step 3 gate (G6): adding or removing an effect
// while a sine plays makes no step larger than twice the sine's own largest.
#[test]
fn activations_crossfade_without_jumping_back() {
    let sine_ir = song(
        json!([{"id": "f", "op": "const", "value": 440, "unit": "hz"}, {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}]),
        "o",
        json!([]),
    );
    let delayed = song(
        json!([{"id": "f", "op": "const", "value": 440, "unit": "hz"}, {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}}]),
        "d",
        json!([]),
    );
    let own = max_step(&render(&sine_ir, &DataSet::empty(), 0.1));
    let mut seed = 7;
    let mut rig = Rig::new(live(&sine_ir, &DataSet::empty()));
    rig.cmd(Cmd::Play);
    let mut out = vec![];
    let mut last_pos = 0;
    for k in 0..12 {
        out.extend(rig.run(500 + lcg(&mut seed) as usize % 6000));
        let next = if k % 2 == 0 { &delayed } else { &sine_ir };
        rig.send(Msg::Activate { renderer: Box::new(live(next, &DataSet::empty())) });
        for _ in 0..8 {
            out.extend(rig.run(97));
            let pos = rig.e.status().pos;
            assert!(pos > last_pos, "the position only moves on: {pos} after {last_pos}");
            last_pos = pos;
        }
    }
    assert_eq!(rig.e.activations(), 12);
    assert!(max_step(&out) <= 2.0 * own, "{} against the sine's own {own}", max_step(&out));
    assert_eq!(rig.e.status().pos, out.len() as u64, "playing all along, never jumping back");
    assert_eq!(rig.retired_renderers().len(), 12, "replaced renderers are retired, off the audio thread");
}

// RENDER-25a, RENDER-27, step 3 gate (G6): a renderer built for another tempo takes over at the
// same beat. No frame math underflows (debug builds check), the output doesn't step, and the old
// renderer starts nothing after the switch.
#[test]
fn an_activation_across_a_tempo_change_keeps_the_beat() {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g", "release": "r"}},
                {"id": "r", "op": "const", "value": 0.05, "unit": "seconds"},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}}]}},
            {"id": "a", "op": "lane", "lane": "clips"},
            {"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "v", "b": "a"}}]),
        "s",
        json!([{"key": "notes", "kind": "events<note>"}, {"key": "clips", "kind": "audio<1>"}]),
    );
    let data = |bpm: f64| {
        let ns: Vec<Value> = (0..16).map(|k| json!({"pitch": 57 + (k % 5) * 2, "start": k, "length": 0.9})).collect();
        let dir = project(&[
            ("data/arrangement.json", json!({"tempo": [{"beat": 0, "bpm": bpm}]})),
            ("data/song/notes.json", json!({"clips": [{"start": 0, "length": 16, "notes": ns}]})),
            ("data/song/clips.json", json!({"clips": [{"start": 0, "length": 16, "asset": "s.wav"}]})),
        ]);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        audio::write_wav(&dir.join("assets/s.wav"), &[sine(220.0, 20.0, SR)], SR).unwrap();
        (self::data(&ir, &dir), dir)
    };
    let (d120, dir1) = data(120.0);
    let mut seed = 3;
    for bpm in [90.0, 150.0, 90.0, 150.0] {
        let (other, dir2) = data(bpm);
        let mut r = live(&ir, &d120);
        r.trace(4096);
        let mut rig = Rig::new(r);
        rig.cmd(Cmd::Play);
        let before = rig.run(SR as usize + lcg(&mut seed) as usize % 40_000);
        // The transport at the next block line, where the activation lands.
        let old = rig.e.current().unwrap();
        let beat = old.transport().beat(old.tempo(), SR as f64);
        let clock = rig.e.clock();
        let mut next = live(&ir, &other);
        next.trace(64);
        rig.send(Msg::Activate { renderer: Box::new(next) });
        let after = rig.run(4096);
        let new = rig.e.current().unwrap();
        let enter = new.trace_entries().into_iter().find(|e| e.kind == TraceKind::Jump(Jump::Enter)).unwrap();
        assert_eq!(enter.engine, clock, "at the block line");
        let b = new.tempo().beat_at(enter.transport as f64 / SR as f64);
        assert!((b - beat).abs() < 1e-4, "the beat is kept: {beat} then {b} at {bpm} bpm");
        let tail = &before[before.len() - SR as usize..];
        let own = max_step(tail);
        let mut joined = tail[tail.len() - 1000..].to_vec();
        joined.extend(&after);
        assert!(max_step(&joined) <= 2.0 * own, "{} against {own} at {bpm} bpm", max_step(&joined));
        let old = rig.retired_renderers();
        assert_eq!(old.len(), 1);
        assert!(
            !old[0].trace_entries().iter().any(|e| matches!(e.kind, TraceKind::On { .. }) && e.engine >= clock),
            "the old one starts nothing"
        );
        std::fs::remove_dir_all(dir2).ok();
    }
    std::fs::remove_dir_all(dir1).ok();
}

// RENDER-28: structural messages apply in order; one that can't apply yet waits, and the ones
// after it wait in the inbox. Only consecutive activations coalesce: a plan that never played is
// retired unrun. A swap reaches only the renderer it was made for.
#[test]
fn structural_messages_keep_their_order() {
    let ir = synth(0.0, json!([]), "v");
    let data = with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 1}]}]}), at_bpm(120.0));
    let mut rig = Rig::new(live(&ir, &data));
    rig.cmd(Cmd::Play);
    rig.run(1000);
    let first = rig.e.current().unwrap().generation();
    let a1 = live(&ir, &data);
    let g1 = a1.generation();
    rig.send(Msg::Activate { renderer: Box::new(a1) });
    // Then, in the same block line and so during its fade: two more plans, then a swap for the
    // second, and one for the first renderer.
    let (a2, a3) = (live(&ir, &data), live(&ir, &data));
    let (g2, g3) = (a2.generation(), a3.generation());
    rig.send(Msg::Activate { renderer: Box::new(a2) });
    rig.send(Msg::Activate { renderer: Box::new(a3) });
    let lane = || LaneSwap::Notes(Arc::new(notes_data(&data, "song", "notes", SR as f64)));
    rig.send(Msg::SwapLane { generation: g3, slot: 0, data: lane() });
    rig.send(Msg::SwapLane { generation: first, slot: 0, data: lane() });
    rig.cmd(Cmd::Seek(0.0));
    rig.run(BLOCK);
    assert_eq!(rig.e.current().unwrap().generation(), g1);
    assert_eq!(rig.inbox.len(), 2, "the swap waits behind the activation, and what follows it waits too");
    assert_eq!(rig.e.seeks(), 0);
    rig.run(1000);
    assert!(rig.inbox.is_empty());
    assert_eq!(rig.e.current().unwrap().generation(), g3);
    assert_eq!(rig.e.activations(), 2, "the second plan never played");
    let unrun: Vec<&Renderer> = rig.retired_renderers().into_iter().filter(|r| r.generation() == g2).collect();
    assert_eq!(unrun.len(), 1);
    assert_eq!(unrun[0].clock(), 0, "retired without rendering a block");
    // The swap for the third renderer applied (its old data retired); the one for the first,
    // long gone, was retired unapplied.
    let lanes = rig.retired.iter().filter(|r| matches!(r, Retired::Lane(_))).count();
    assert_eq!(lanes, 2);
    assert_eq!(rig.e.refused_swaps(), 0);
    assert_eq!(rig.e.seeks(), 1);
}

// RENDER-28: with the retire channel full, retired objects wait in a fixed stash; once it is full
// too, structural messages wait (transport commands don't), and nothing is dropped.
#[test]
fn a_full_retire_channel_holds_messages_never_drops_them() {
    let ir = synth(0.0, json!([]), "v");
    let data = with_notes(json!({"clips": []}), at_bpm(120.0));
    let mut rig = Rig::new(live(&ir, &data));
    rig.refuse = true;
    // Swaps for a renderer that never came: each is retired unapplied.
    let stale = || Msg::SwapLane { generation: 0, slot: 0, data: LaneSwap::Notes(Arc::new(NotesData::empty())) };
    // Sixteen fill the stash.
    for _ in 0..16 {
        rig.send(stale());
    }
    rig.cmd(Cmd::Play);
    rig.send(stale());
    rig.cmd(Cmd::Seek(2.0));
    rig.send(stale());
    rig.cmd(Cmd::Stop);
    rig.run(BLOCK * 4);
    let s = rig.e.status();
    assert!(s.playing && s.pos >= 48_000, "transport commands apply while a structural message waits: {s:?}");
    assert_eq!(rig.inbox.len(), 1, "what follows a second waiting message waits in the inbox, in order");
    rig.refuse = false;
    rig.run(BLOCK * 2);
    assert!(rig.inbox.is_empty() && !rig.e.status().playing);
    assert_eq!(rig.retired.iter().filter(|r| matches!(r, Retired::Lane(_))).count(), 18, "every one retired, none dropped");
}

// RENDER-6a: stopped and silent for 8 blocks, an engine with nothing that can sound idles, and
// any message wakes it. Echoes inside a voice keep it from idling between them (G7).
#[test]
fn the_engine_idles_only_when_nothing_can_sound() {
    let ir = synth(0.0, json!([]), "v");
    let data =
        with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 0.2}]}]}), at_bpm(120.0));
    let mut rig = Rig::new(live(&ir, &data));
    rig.run(BLOCK * 20);
    assert!(rig.e.is_idle(), "stopped and silent");
    let clock = rig.e.clock();
    rig.run(BLOCK);
    assert_eq!(rig.e.clock(), clock + BLOCK as u64, "the clock goes on");
    rig.cmd(Cmd::Play);
    let out = rig.run(SR as usize / 4);
    assert!(!rig.e.is_idle() && peak(&out) > 0.5, "a message wakes it");
    // A 500 ms delay inside the voice: silent between echoes, but never idle.
    let echo = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "d", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}},
                {"id": "t", "op": "const", "value": 0.5, "unit": "seconds"},
                {"id": "f", "op": "const", "value": 0.5},
                {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "a", "time": "t", "feedback": "f", "mix": "m"}},
                {"id": "m", "op": "const", "value": 1}]}}]),
        "v",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let data =
        with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 69, "start": 0, "length": 0.1}]}]}), at_bpm(120.0));
    let mut rig = Rig::new(live(&echo, &data));
    rig.cmd(Cmd::Play);
    rig.run(SR as usize / 10);
    rig.cmd(Cmd::Stop);
    let out = rig.run(SR as usize * 12 / 10);
    let at = |s: f64| (s * SR as f64) as usize;
    assert!(peak(&out[at(0.15)..at(0.38)]) < 1e-6, "silent before the echo");
    assert!(peak(&out[at(0.40)..at(0.46)]) > 0.1, "the echo after the gap is there");
    assert!(peak(&out[at(0.90)..at(0.96)]) > 0.05, "and the next");
    assert_eq!(rig.e.resets(), 0);
}

// RENDER-25a, step 3 gate (G6), the poly variant: activations while a note plays through an
// envelope. The replaced renderer releases it, the new one doesn't chase it, and no step is larger
// than twice the sine's own.
#[test]
fn activations_while_a_voice_sounds_dont_click() {
    let held = |after: Value, out: &str| synth(0.05, after, out);
    let plain = held(json!([]), "v");
    let delayed = held(json!([{"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v"}}]), "d");
    let data = with_notes(
        json!({"clips": [{"start": 0, "length": 64, "notes": [
        {"pitch": 69, "start": 0, "length": 64}, {"pitch": 69, "start": 16, "length": 48}, {"pitch": 69, "start": 32, "length": 32}]}]}),
        at_bpm(120.0),
    );
    let sine = song(
        json!([{"id": "f", "op": "const", "value": 440, "unit": "hz"}, {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}]),
        "o",
        json!([]),
    );
    let own = max_step(&render(&sine, &DataSet::empty(), 0.1));
    let mut seed = 5;
    for k in 0..6 {
        let mut rig = Rig::new(live(&plain, &data));
        rig.cmd(Cmd::Play);
        let mut out = rig.run(4000 + lcg(&mut seed) as usize % 20_000);
        assert!(peak(&out[out.len() - 100..]) > 0.5, "the note sounds");
        rig.send(Msg::Activate { renderer: Box::new(live(if k % 2 == 0 { &delayed } else { &plain }, &data)) });
        out.extend(rig.run(SR as usize / 5));
        assert!(max_step(&out) <= 2.0 * own, "{} against the sine's own {own}", max_step(&out));
        assert!(peak(&out[out.len() - 1000..]) < 1e-3, "released, and not chased");
        assert_eq!(rig.e.current().unwrap().held_notes(), 0);
    }
}

mod carry;
