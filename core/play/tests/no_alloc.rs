//! RENDER-28: the audio callback never allocates or frees, from its very first fill on a fresh
//! thread, through everything the engine does live: play, seek, a loop and its wraps, voices
//! stolen, a lane swap, an activation across a tempo change that keeps kernels, voices, a ring and
//! an audio player, swaps the audio lane's renders, grows a lane, and plays out a ghost reading a
//! ghost lane while its edges fade, a panic in that window, the window's end and its leftovers, a
//! stale activation refused, stop, idle, play, panic, and a full retire channel; and, from step 5,
//! cell writes from another thread all along (a step, a ramp and a cell in voices), with no
//! rebuild; from 8b, overlay cell writes, live notes, and the meters' taps and the callback's timer. Kernel memory never grows either: a reset on the audio thread would grow a delay's
//! (§1.5 of the step 3a design).
//!
//! Everything is built and sent on the test's thread; only the callback runs under the trap.
//! Only debug builds' callbacks mark their thread for it (a release callback touches no
//! thread-local), so these count in debug builds, as ci/check.sh runs them; in release they count
//! nothing, which `an_allocation_in_the_callback_is_counted` checks.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use common::*;
use m2_ir::Origin;
use m2_play::{DeviceError, DeviceErrorKind, Driver, Gone, Player, alloc};
use m2_runtime::{BLOCK, CellTarget, Continues, LaneSwap, LiveEv, NotesData, Renderer, Retired};
use m2_song::{DataSet, Project};
use serde_json::{Value, json};

// The app's debug counter (`m2_play::alloc`): it counts what the callback allocates or frees, as
// the callback marks its own thread.
#[global_allocator]
static TRAP: alloc::Counting = alloc::Counting;

/// A device callback's fill under the trap: what it allocated or freed.
fn trapped(f: impl FnOnce()) -> u64 {
    alloc::trace(std::env::var_os("M2_TRAP_TRACE").is_some());
    let before = alloc::here();
    f();
    alloc::here() - before
}

/// Where the song's cells are written: the feedback gain (a step), the reverb's mix in the voices
/// (a step in every voice), and the level at the end (declared smooth, so a ramp).
fn cells() -> [Origin; 3] {
    [11, 12, 13].map(|site| Origin { site, arg: vec!["0".into()], ..Origin::default() })
}

/// Where gestures on [`cells`] write in `r`: each value's cells, through `r`'s slots (RENDER-23).
fn targets(r: &Renderer) -> Vec<CellTarget> {
    let map = r.cell_map();
    let t: Vec<CellTarget> = cells().iter().map(|o| CellTarget { slots: r.cell_slots(), cells: map.cells(o).into() }).collect();
    assert!(t.iter().all(|t| t.cells.len() == 1), "each value is one cell");
    t
}

/// The song: voices that hold a delay and a reverb and are stolen, notes with equal keys, a
/// control lane, time, a feedback loop and a delay at the top, an audio lane, and the cells of
/// [`cells`]. `fx` adds a reverb at the end, and scales the voices by a constant where the control
/// lane was (whose node then plays out the window as a ghost, reading the lane as a ghost lane).
fn nodes(fx: bool) -> (Value, &'static str) {
    let mut n = vec![
        json!({"id": "n", "op": "lane", "lane": "notes"}),
        json!({"id": "v", "op": "poly", "events": "n", "voices": 3, "body": {"out": "r", "nodes": [
            {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
            {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
            {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
            {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g"}},
            {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}},
            {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "a"}},
            {"id": "wet", "op": "const", "value": 0.25, "origin": {"site": 12, "arg": ["0"]}},
            {"id": "r", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": "d", "mix": "wet"}}]}}),
        json!({"id": "c", "op": "lane", "lane": "level"}),
        json!({"id": "t", "op": "time", "of": "beats"}),
        json!({"id": "l", "op": "kernel", "kernel": "std:lfo", "inputs": {"beats": "t"}}),
        json!({"id": "half", "op": "const", "value": 0.5}),
        json!({"id": "vc", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "v", "by": if fx { "half" } else { "c" }}}),
        json!({"id": "vl", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "vc", "by": "l"}}),
        json!({"id": "a", "op": "lane", "lane": "clips"}),
        json!({"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "vl", "b": "a"}}),
        json!({"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "sum"}),
        json!({"id": "k", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}}),
        json!({"id": "h", "op": "const", "value": 0.3, "origin": {"site": 11, "arg": ["0"]}}),
        json!({"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "x", "b": "k"}}),
        json!({"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "sum"}}),
    ];
    let last = if fx {
        n.push(json!({"id": "fx", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": "d"}}));
        "fx"
    } else {
        "d"
    };
    n.push(json!({"id": "lvl", "op": "const", "value": 0.9,
        "origin": {"site": 13, "arg": ["0"], "control": {"frame": 0, "slot": "level", "smooth": true}}}));
    n.push(json!({"id": "out", "op": "kernel", "kernel": "std:scale", "inputs": {"in": last, "by": "lvl"}}));
    (Value::Array(n), "out")
}

/// The song's project at `bpm`, in a temp folder, compiled from `nodes` without TypeScript. With
/// `fx`, 300 notes more at beat 12 outgrow the notes lane's scheduler.
fn build(fx: bool, bpm: f64) -> Song {
    let dir = temp("trap");
    let mut ns: Vec<Value> = (0..64).map(|k| json!({"pitch": 48 + (k % 7) * 3, "start": k as f64 * 0.25, "length": 0.4})).collect();
    if fx {
        ns.extend((0..300).map(|k| json!({"pitch": k % 120, "start": 12, "length": 1})));
    }
    let files = [
        ("music2.toml", "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\n".to_string()),
        ("song.ts", String::new()),
        ("data/arrangement.json", json!({"tempo": [{"beat": 0, "bpm": bpm}]}).to_string()),
        // Equal keys: the first note twice.
        (
            "data/song/notes.json",
            json!({"clips": [{"id": "a", "start": 0, "length": 16, "notes": ns},
            {"id": "a", "start": 0, "length": 16, "notes": [{"pitch": 48, "start": 0, "length": 0.4}]}]})
            .to_string(),
        ),
        ("data/song/level.json", json!({"unit": "gain", "clips": [{"start": 0, "length": 16, "points": [[0, 0.2], [16, 1]]}]}).to_string()),
        ("data/song/clips.json", json!({"clips": [{"start": 1, "length": 8, "asset": "s.wav"}]}).to_string()),
    ];
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let wave: Vec<f32> = (0..96_000).map(|i| (i as f32 * 0.03).sin() * 0.3).collect();
    m2_runtime::audio::write_wav(&dir.join("assets/s.wav"), &[wave], SR).unwrap();
    let (n, out) = nodes(fx);
    let ir: m2_ir::Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "nodes": n, "exports": {"out": out}, "frames": [{"site": 1, "component": "test:level"}], "lanes": [
            {"key": "notes", "kind": "events<note>"}, {"key": "level", "kind": "control<gain>", "default": 0},
            {"key": "clips", "kind": "audio<1>"}]}]}))
    .unwrap();
    let packages = std_packages();
    let flat = m2_ir::check(&ir, &packages).unwrap_or_else(|e| panic!("{e:?}"));
    let (data, diags): (DataSet, _) = Project::open(&dir).unwrap().data(&m2_runtime::decls(&ir), None);
    let codes: Vec<&str> = diags.iter().map(|d| d.code).collect();
    assert!(codes.iter().all(|c| !c.starts_with("lane.") && *c != "asset.missing"), "{codes:?}");
    Song { flat, data, packages, rate: SR, dir }
}

enum Step {
    /// Fill this many device frames; then take what was retired, or leave it in the channel.
    Fill(usize, bool),
    Done,
}

/// What the audio thread saw after a step.
struct Report {
    allocs: u64,
    idle: bool,
    pos: u64,
}

fn trap_run(device_rate: u32) {
    let a = build(false, 120.0);
    let b = build(true, 100.0);
    let mut ra = a.renderer();
    ra.set_strict(false);
    let (gen_a, mem_a) = (ra.generation(), ra.kernel_memory_bytes());
    // Prepared against A as it plays: the kept instances move into it at the block line.
    let (info_a, stale_from) = (ra.info(), a.renderer());
    let rb = b.after(&a, &info_a);
    let (gen_b, mem_b) = (rb.generation(), rb.kernel_memory_bytes());
    let s = rb.stats().clone();
    assert!(s.kept.kernels > 5 && s.kept.polys == 1 && s.kept.rings == 1 && s.ghosts == 1 && s.fades >= 2, "{s:?}");
    // A plan prepared against A again, sent once B plays: refused.
    let stale = a.after(&a, &stale_from.info());
    drop(stale_from);
    // Where gestures write in each, taken before they are sent.
    let both = Arc::new([targets(&ra), targets(&rb)]);
    // A swap of A's notes: the same notes and one more.
    let more = NotesData::build(
        &match &**a.data.lane("song", "notes").unwrap() {
            m2_song::LaneData::Notes(c) => {
                let mut c = c.clone();
                c[0].notes.push(m2_song::Note { pitch: 72.0, start: 5.0, length: 1.0, velocity: 0.8 });
                c
            }
            _ => unreachable!(),
        },
        a.data.tempo(),
        SR as f64,
    );
    // A's notes as they are: what a preview's swap back puts back (PLANE-5).
    let notes_a = || match &**a.data.lane("song", "notes").unwrap() {
        m2_song::LaneData::Notes(c) => NotesData::build(c, a.data.tempo(), SR as f64),
        _ => unreachable!(),
    };
    let (player, d) = Player::driven(device_rate);
    player.open(ra).unwrap();
    let (steps, rx) = mpsc::channel::<Step>();
    let (reports, back) = mpsc::channel::<Report>();
    // Cells are written from another thread until the stop (below): whether it writes yet, and
    // the rounds it wrote so far.
    let writing = Arc::new(AtomicBool::new(true));
    let wrote = Arc::new(AtomicUsize::new(0));
    let (writes, rounds) = (writing.clone(), wrote.clone());
    // The audio thread: fresh, under the trap from its very first fill.
    let audio = std::thread::spawn(move || {
        let mut d = d;
        let mut kept: Vec<Gone> = vec![];
        let mut buf = vec![0.0f32; 2 * 4096];
        while let Ok(Step::Fill(frames, take)) = rx.recv() {
            let mut done = 0;
            let from = rounds.load(Ordering::Acquire);
            let allocs = trapped(|| {
                for n in SIZES.iter().cycle() {
                    let n = (*n).min(frames - done);
                    if n == 0 {
                        break;
                    }
                    d.fill(&mut buf[..2 * n], 2);
                    done += n;
                    // While cells are written, the first buffer waits for two rounds more: they
                    // land between this step's block lines, however little the writer runs.
                    while done == n && writes.load(Ordering::Acquire) && rounds.load(Ordering::Acquire) < from + 2 {
                        std::thread::yield_now();
                    }
                }
            });
            if take {
                kept.extend(d.drain_retired());
            }
            let e = d.engine().unwrap();
            reports.send(Report { allocs, idle: e.is_idle(), pos: e.status().pos }).unwrap();
        }
        kept.extend(d.drain_retired());
        (d, kept)
    });
    let step = |frames: usize, take: bool| -> Report {
        steps.send(Step::Fill(frames, take)).unwrap();
        let r = back.recv().unwrap();
        assert_eq!(r.allocs, 0, "the audio thread allocated (at {device_rate} Hz); M2_TRAP_TRACE=1 shows where");
        r
    };
    let s = |x: f64| (x * device_rate as f64) as usize;
    // Cell writes from another thread all along, into whichever renderer plays, until the stop:
    // the feedback gain, the reverb's mix in every voice and the smooth level, as gestures do.
    let target = Arc::new(AtomicUsize::new(0));
    let writer = {
        let (player, both, target, writing, wrote) = (player.clone(), both.clone(), target.clone(), writing.clone(), wrote.clone());
        std::thread::spawn(move || {
            let mut k = 0;
            while writing.load(Ordering::Relaxed) {
                for t in &both[target.load(Ordering::Relaxed)] {
                    player.set_cells(t, 0.2 + (k % 5) as f64 * 0.1);
                }
                k += 1;
                wrote.store(k, Ordering::Release);
                std::thread::yield_now();
            }
            for t in &both[target.load(Ordering::Relaxed)] {
                player.set_cells(t, 0.42);
            }
            k
        })
    };
    player.play();
    step(s(0.5), true);
    player.seek(2.0).unwrap();
    player.set_loop(Some((2.0, 3.0))).unwrap();
    // Three wraps, voices stolen all along.
    step(s(1.6), true);
    player.set_loop(None).unwrap();
    let more = Arc::new(more);
    player.swap_lane(gen_a, 0, LaneSwap::Notes(more.clone())).unwrap();
    step(s(0.3), true);
    // A preview whose commit failed is swapped back (PLANE-5): once it played, and once in the
    // same block it was taken in.
    player.swap_lane(gen_a, 0, LaneSwap::Notes(Arc::new(notes_a()))).unwrap();
    step(s(0.3), true);
    player.swap_lane(gen_a, 0, LaneSwap::Notes(more)).unwrap();
    player.swap_lane(gen_a, 0, LaneSwap::Notes(Arc::new(notes_a()))).unwrap();
    step(s(0.3), true);
    player.activate(rb).unwrap();
    // In the window: its ghost and fades reset with the plan.
    player.panic().unwrap();
    target.store(1, Ordering::Relaxed);
    step(s(0.5), true);
    player.activate(stale).unwrap();
    step(s(0.1), true);
    writing.store(false, Ordering::Relaxed);
    // Seven steps took two rounds or more each, waited for above.
    assert!(writer.join().unwrap() > 10, "it wrote while the callback ran");
    step(s(0.1), true);
    player.stop();
    let mut idle = false;
    for _ in 0..40 {
        idle = step(s(0.5), true).idle;
        if idle {
            break;
        }
    }
    assert!(idle, "stopped, the engine went idle once nothing could sound");
    player.play();
    step(s(0.3), true);
    player.panic().unwrap();
    step(s(0.3), true);
    // The retired channel fills (64), then the engine's stash (16); the rest wait their turn.
    // Swaps for the renderer that left are each retired unapplied.
    for _ in 0..120 {
        player.swap_lane(gen_a, 0, LaneSwap::Notes(Arc::new(NotesData::empty()))).unwrap();
    }
    step(s(0.2), false);
    let pos = step(s(0.2), false).pos;
    step(s(0.3), true);
    let last = step(s(0.3), true);
    assert!(last.pos > pos, "it played on throughout");
    steps.send(Step::Done).unwrap();
    let (mut d, kept) = audio.join().unwrap();
    let lanes = kept.iter().filter(|g| matches!(g, Gone::Engine(Retired::Lane(_)))).count();
    assert_eq!(lanes, 124, "every swap was taken and retired, none dropped");
    let old: Vec<&Renderer> =
        kept.iter().filter_map(|g| if let Gone::Engine(Retired::Renderer(r)) = g { Some(&**r) } else { None }).collect();
    assert_eq!(old.iter().map(|r| r.generation()).collect::<Vec<_>>()[..1], [gen_a]);
    assert_eq!(old.len(), 2, "the plan B replaced, and the stale one, refused");
    assert!(kept.iter().any(|g| matches!(g, Gone::Engine(Retired::Nodes(_)))), "the window's leftovers left");
    let old_mem = old[0].kernel_memory_bytes();
    let e = d.engine().unwrap();
    assert_eq!((e.resets(), e.activations(), e.refused_swaps(), e.refused_activations(), e.leaked()), (2, 1, 0, 1, 0));
    let cur = e.current().unwrap();
    assert_eq!(cur.generation(), gen_b, "cell writes rebuilt nothing");
    // Every kept instance moved, none grew: what A held is in A and B, as it was.
    assert_eq!(cur.kernel_memory_bytes() + old_mem, mem_a + mem_b, "no kernel grew its memory on the audio thread, nor after the panics");
    let map = cur.cell_map();
    for o in cells() {
        assert_eq!(map.cells(&o).iter().map(|c| cur.cell_value(*c)).collect::<Vec<_>>(), [Some(0.42)], "the last write plays");
    }
    assert!(!player.crashed());
}

#[test]
fn the_audio_thread_never_allocates() {
    trap_run(SR);
}

// The same through the rate converter, on a 44.1 kHz device.
#[test]
fn the_audio_thread_never_allocates_through_the_resampler() {
    trap_run(44_100);
}

// RENDER-28, MON-1: overlay cells written while a song plays (mutes, unmutes, a glide cut short),
// and an activation whose overlays were set before it was sent, allocate and free nothing.
#[test]
fn overlay_writes_never_allocate() {
    let song = project(
        "trap-overlays",
        &[(
            "song.ts",
            "import { song, track } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { mixer } from \"std/mix\";\n\
             const kick = track(\"kick\", () => osc.sine(55));\nconst bass = track(\"bass\", () => osc.saw(110));\n\
             export default song(() => mixer({ kick: kick.out, bass: bass.out }));\n",
        )],
    );
    let r = song.renderer();
    let info = r.info();
    assert_eq!(info.overlays.len(), 2);
    let kick = CellTarget { slots: info.cell_slots.clone(), cells: info.overlays.iter().filter(|o| o.0 == "kick").map(|o| o.2).collect() };
    let (player, mut d) = Player::driven(SR);
    player.open(r).unwrap();
    player.play();
    let mut data = vec![0.0f32; 2 * 4096];
    let mut allocs = 0;
    for (k, n) in [64usize, 37, 128, 1000, 3, 512, 4096, 77].iter().cycle().take(64).enumerate() {
        if k % 3 == 0 {
            player.set_cells(&kick, if k % 2 == 0 { 0.0 } else { 1.0 });
        }
        if k == 32 {
            let mut next = song.after(&song, &info);
            next.set_overlays(&[0.0, 1.0]);
            player.activate(next).unwrap();
        }
        allocs += trapped(|| d.fill(&mut data[..2 * n], 2));
        drop(d.drain_retired());
    }
    assert_eq!(allocs, 0, "the callback allocated or freed");
}

// RENDER-28, MON-7: live notes allocate and free nothing: struck and let go while the song plays
// (into voices and into a plugin), through a wrap and a stop, waking the idle engine, a full
// table, a source's release, a panic, and messages for a renderer that is gone.
#[test]
fn live_notes_never_allocate() {
    for song in [build(false, 120.0), probes(1.0)] {
        let r = song.renderer();
        let (generation, info) = (r.generation(), r.info());
        let slot = info.lanes.iter().position(|l| l.lane.lane == "notes").unwrap() as u32;
        let (player, mut d) = Player::driven(SR);
        player.open(r).unwrap();
        let mut data = vec![0.0f32; 2 * 4096];
        let mut allocs = 0;
        let mut fill = |d: &mut m2_play::Driver, frames: usize| {
            let mut done = 0;
            for n in SIZES.iter().cycle() {
                let n = (*n).min(frames - done);
                if n == 0 {
                    break;
                }
                allocs += trapped(|| d.fill(&mut data[..2 * n], 2));
                done += n;
            }
            drop(d.drain_retired());
        };
        let on = |h: u32| LiveEv::On { source: 1, handle: h, pitch: 40.0 + (h % 40) as f32, velocity: 0.8 };
        let off = |h: u32| LiveEv::Off { source: 1, handle: h };
        fill(&mut d, 4096);
        player.play();
        player.set_loop(Some((0.0, 1.0))).unwrap();
        for h in 0..40 {
            player.live(generation, slot, on(h)).unwrap();
            fill(&mut d, 1000);
            player.live(generation, slot, off(h)).unwrap();
            fill(&mut d, 300);
        }
        player.set_loop(None).unwrap();
        player.stop();
        // Idle, then woken by a key.
        fill(&mut d, 48_000);
        player.live(generation, slot, on(1)).unwrap();
        fill(&mut d, 2000);
        assert_eq!(player.live_notes(), 1);
        // A full table, a release, a panic, and a renderer that is gone.
        for h in 0..20 {
            player.live(generation, slot, on(100 + h)).unwrap();
        }
        fill(&mut d, 1000);
        assert_eq!(player.live_notes(), 16);
        player.live(generation, slot, LiveEv::Release { source: 1 }).unwrap();
        fill(&mut d, 1000);
        assert_eq!(player.live_notes(), 0);
        player.live(generation, slot, on(7)).unwrap();
        player.panic().unwrap();
        player.live(generation + 1000, slot, on(8)).unwrap();
        fill(&mut d, 2000);
        assert_eq!(player.live_notes(), 0);
        assert_eq!(allocs, 0, "the callback allocated or freed");
        assert!(!player.crashed());
    }
}

// RENDER-30a: a sound input's callback allocates and frees nothing: a device's at 44.1 kHz, stereo,
// converted to the song's 48 kHz, in buffers of odd sizes; and the output's callback, as it hands
// the silent input what was injected and the engine hears it on the audio lane (MON-8), across a
// stop, the idle engine kept awake, a loop's wraps, and hearing stopped and started again.
#[test]
fn the_input_callback_never_allocates() {
    let song = build(false, 120.0);
    let r = song.renderer();
    let (generation, info) = (r.generation(), r.info());
    let slot = info.lanes.iter().position(|l| l.lane.lane == "clips").unwrap() as u32;
    let (player, mut d) = Player::driven(SR);
    player.open(r).unwrap();
    player.open_input(&m2_play::InputChoice::default()).unwrap();
    let (mut device, engine_ring, take_ring) = player.driven_input(44_100, 2);
    let wave: Vec<f32> = (0..2 * 4096).map(|i| (i as f32 * 0.01).sin() * 0.25).collect();
    let mut data = vec![0.0f32; 2 * 4096];
    let (mut allocs, mut heard) = (0, 0.0f32);
    let mut fill = |d: &mut m2_play::Driver, frames: usize| {
        let mut done = 0;
        for n in SIZES.iter().cycle() {
            let n = (*n).min(frames - done);
            if n == 0 {
                break;
            }
            player.inject_input(&wave[..n]).unwrap();
            allocs += trapped(|| device.fill(&wave[..2 * n], std::time::Duration::from_micros(700)));
            allocs += trapped(|| d.fill(&mut data[..2 * n], 2));
            heard = heard.max(peak(&data[..2 * n].chunks(2).map(|f| [f[0], f[1]]).collect::<Vec<_>>()));
            done += n;
        }
        drop(d.drain_retired());
    };
    fill(&mut d, 4096);
    player.hear(generation, slot, true).unwrap();
    player.play();
    player.set_loop(Some((0.0, 1.0))).unwrap();
    fill(&mut d, 60_000);
    player.stop();
    fill(&mut d, 48_000);
    assert!(!player.idle(), "never idle while it hears");
    player.hear(generation, slot, false).unwrap();
    fill(&mut d, 4096);
    player.hear(generation, slot, true).unwrap();
    fill(&mut d, 4096);
    assert_eq!(allocs, 0, "an input's callback or the engine's hearing allocated or freed");
    assert!(heard > 0.1, "the input was heard: {heard}");
    assert!(engine_ring.head() > 100_000 && take_ring.head() == engine_ring.head(), "the device's input reached both rings");
    assert!(player.input_frames() > 100_000, "the silent input handed over what was injected");
    assert!(!player.crashed());
}

// RENDER-28, 8b.8: a live plan's taps and the callback's timer allocate and free nothing, while
// another thread reads and resets them as the server's meters do.
#[test]
fn taps_and_the_load_never_allocate() {
    let song = project(
        "trap-taps",
        &[(
            "song.ts",
            "import { song, track } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { mixer } from \"std/mix\";\n\
             const kick = track(\"kick\", () => osc.sine(55));\nconst bass = track(\"bass\", () => osc.saw(110));\n\
             export default song(() => mixer({ kick: kick.out, bass: bass.out }));\n",
        )],
    );
    let owner = m2_plugins::Owner::discard();
    let mut cache = m2_runtime::audio::Cache::default();
    let r = Renderer::prepare(&song.flat, &song.data, &song.packages, SR, &mut cache, &owner, m2_runtime::Basis::Fresh).unwrap();
    let taps = r.info().taps.clone().expect("a live plan has taps");
    let (player, mut d) = Player::driven(SR);
    player.open(r).unwrap();
    player.play();
    let reading = Arc::new(AtomicBool::new(true));
    let reader = {
        let (player, reading) = (player.clone(), reading.clone());
        std::thread::spawn(move || {
            let (mut seen, mut longest) = (vec![0.0f32; 3], 0.0f32);
            // Once more after the last fill: what it left is read too.
            loop {
                let last = !reading.load(Ordering::Relaxed);
                let p = taps.take();
                for (s, x) in seen.iter_mut().zip([p.out[0], p.track("kick").unwrap()[0], p.track("bass").unwrap()[0]]) {
                    *s = s.max(x);
                }
                longest = longest.max(player.load().longest);
                if last {
                    return (seen, longest);
                }
                std::thread::yield_now();
            }
        })
    };
    let mut data = vec![0.0f32; 2 * 4096];
    let mut allocs = 0;
    for n in SIZES.iter().cycle().take(40) {
        allocs += trapped(|| d.fill(&mut data[..2 * n], 2));
    }
    reading.store(false, Ordering::Relaxed);
    let (seen, longest) = reader.join().unwrap();
    assert_eq!(allocs, 0, "the callback allocated or freed");
    assert!(seen.iter().all(|&x| x > 0.1), "the output, the kick and the bass were read: {seen:?}");
    assert!(longest > 0.0);
}

/// A probe instrument on 64 notes into a probe effect (step 3b, §6.5), `gain` on the instrument,
/// a cell (written at [`cells`]'s first site).
fn probes(gain: f64) -> Song {
    probes_on(gain, None)
}

/// [`probes`], on `host` when given (the host process, HOST-3).
fn probes_on(gain: f64, host: Option<Arc<dyn m2_plugins::PluginHost>>) -> Song {
    let dir = temp("trap-plugins");
    let ns: Vec<Value> = (0..64).map(|k| json!({"pitch": 48 + (k % 7) * 3, "start": k as f64 * 0.25, "length": 1.5})).collect();
    for (name, text) in [
        ("music2.toml", "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\n".to_string()),
        ("song.ts", String::new()),
        ("data/song/notes.json", json!({"clips": [{"id": "a", "start": 0, "length": 16, "notes": ns}]}).to_string()),
    ] {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let mut ir: m2_ir::Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "exports": {"out": "fx"}, "lanes": [{"key": "notes", "kind": "events<note>"}], "nodes": [
            {"id": "n", "op": "lane", "lane": "notes"}, {"id": "g", "op": "const", "value": gain, "origin": {"site": 11, "arg": ["0"]}},
            {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"notes": "n", "gain_1": "g"}},
            {"id": "fx", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"in": "p"}}]}]}))
    .unwrap();
    let mut packages = std_packages();
    if let Some(h) = host {
        packages = packages.with_host(h);
    }
    let diags = packages.bind_plugins(&mut ir);
    assert!(diags.is_empty(), "{diags:?}");
    let flat = m2_ir::check(&ir, &packages).unwrap_or_else(|e| panic!("{e:?}"));
    let (data, _) = Project::open(&dir).unwrap().data(&m2_runtime::decls(&ir), None);
    Song { flat, data, packages, rate: SR, dir }
}

/// What an export does with the live instances `lent` lends while the engine holds (RENDER-1,
/// `Lent` in `crates/server/src/export.rs`): each taken offline, `song`'s renderer playing them
/// through the song from 0 (its first call resets each), each given back, active again.
fn export_with(song: &Song, lent: &std::collections::HashMap<usize, Arc<m2_runtime::PluginInstance>>) {
    let cycle = |inst: &m2_runtime::PluginInstance| {
        let (host, h) = (inst.host(), inst.handle());
        host.deactivate(h);
        host.activate(h, SR, BLOCK as u32).unwrap();
    };
    lent.values().for_each(|i| cycle(i));
    let mut r = Renderer::lending(&song.flat, &song.data, &song.packages, SR, lent).unwrap();
    for k in 0..(song.seconds() * SR as f64) as u64 / BLOCK as u64 {
        r.render_block(k * BLOCK as u64).unwrap();
    }
    drop(r);
    for i in lent.values() {
        cycle(i);
        i.mark_reloaded();
    }
}

// RENDER-28, step 3b (§6.5): the callback never allocates with plugins either: two probe instances
// played, then an activation that moves both while notes sound into one (its notes and last-sent
// values with it), a hold for an export and its resume with a panic deferred to it (RENDER-1), a
// cell write into a parameter, a panic, a hold an export renders in and its resume, which starts
// the notes the export ended again, stop, the plugins asleep and the engine idle, then woken
// by a plugin's request from a thread of its own. The probe
// asks for its main-thread callback from `process` every 10 calls. Only the host glue is trapped
// here: a real plugin's own allocations are its own (step 3a §13).
#[test]
fn the_audio_thread_never_allocates_with_plugins() {
    let (a, b) = (probes(1.0), probes(0.5));
    let owner = m2_plugins::Owner::discard();
    let mut ra =
        Renderer::prepare(&a.flat, &a.data, &a.packages, SR, &mut m2_runtime::audio::Cache::default(), &owner, m2_runtime::Basis::Fresh)
            .unwrap();
    ra.set_strict(false);
    let handles = ra.plugin_handles();
    let rb = b.after(&a, &ra.info());
    let info_b = rb.info();
    let gain = CellTarget { slots: rb.cell_slots(), cells: rb.cell_map().cells(&cells()[0]).into() };
    assert_eq!(gain.cells.len(), 1, "the gain parameter's value is a cell");
    assert_eq!(rb.stats().kept.plugins, 2, "both kept");
    let stats: Vec<_> = handles.iter().map(|h| m2_plugins::probe::stats(*h).unwrap()).collect();
    let (player, d) = Player::driven(SR);
    player.open(ra).unwrap();
    let (steps, rx) = mpsc::channel::<usize>();
    let (reports, back) = mpsc::channel::<(u64, bool)>();
    let audio = std::thread::spawn(move || {
        let mut d = d;
        let mut kept: Vec<Gone> = vec![];
        let mut buf = vec![0.0f32; 2 * 4096];
        while let Ok(frames) = rx.recv() {
            let mut done = 0;
            let allocs = trapped(|| {
                for n in SIZES.iter().cycle() {
                    let n = (*n).min(frames - done);
                    if n == 0 {
                        break;
                    }
                    d.fill(&mut buf[..2 * n], 2);
                    done += n;
                }
            });
            kept.extend(d.drain_retired());
            reports.send((allocs, d.engine().unwrap().is_idle())).unwrap();
        }
        (d, kept)
    });
    let step = |frames: usize| -> bool {
        steps.send(frames).unwrap();
        let (allocs, idle) = back.recv().unwrap();
        assert_eq!(allocs, 0, "the audio thread allocated with plugins; M2_TRAP_TRACE=1 shows where");
        idle
    };
    player.play();
    step(SR as usize / 2);
    assert!(stats[0].sounding.load(std::sync::atomic::Ordering::Relaxed) > 0, "notes sound into the instrument");
    player.activate(rb).unwrap();
    step(SR as usize / 2);
    player.hold(true).unwrap();
    step(SR as usize / 10);
    assert!(player.held(), "held");
    let calls = stats[0].process.load(std::sync::atomic::Ordering::Relaxed);
    player.panic().unwrap();
    step(SR as usize / 10);
    assert_eq!(stats[0].process.load(std::sync::atomic::Ordering::Relaxed), calls, "nothing processed while held");
    assert_eq!(stats[0].resets.load(std::sync::atomic::Ordering::Relaxed), 0, "the panic waits for the resume");
    player.hold(false).unwrap();
    step(SR as usize / 10);
    assert!(!player.held() && stats[0].resets.load(std::sync::atomic::Ordering::Relaxed) == 1, "resumed, then the panic");
    player.set_cells(&gain, 0.5);
    player.panic().unwrap();
    step(SR as usize / 4);
    // RENDER-1: an export renders with the instances while held, here on this thread as on the
    // export's, which ends the notes playback left in them; at the resume the instrument starts
    // again the notes the song still holds, on the audio thread.
    player.hold(true).unwrap();
    step(SR as usize / 10);
    assert!(player.held());
    export_with(&b, &info_b.lend(&b.flat, &b.data, &b.packages, &Continues::identity(&b.flat)));
    let ons = |s: &m2_plugins::probe::Stats| s.notes().iter().filter(|e| e.1 == m2_plugins::probe::Note::On).count();
    let (before, sounding) = (ons(&stats[0]), stats[0].sounding.load(std::sync::atomic::Ordering::Relaxed));
    player.hold(false).unwrap();
    step(BLOCK);
    assert_eq!(sounding, 0, "the export ended them");
    assert!(ons(&stats[0]) > before && stats[0].sounding.load(std::sync::atomic::Ordering::Relaxed) > 0, "they start again");
    player.stop();
    step(SR as usize / 4);
    for s in &stats {
        s.sleep.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let mut idle = false;
    for _ in 0..20 {
        idle = step(SR as usize / 10);
        if idle {
            break;
        }
    }
    assert!(idle, "every plugin asleep: the engine idles");
    let calls = stats[1].process.load(std::sync::atomic::Ordering::Relaxed);
    stats[1].sleep.store(false, std::sync::atomic::Ordering::Relaxed);
    let h = handles[1];
    std::thread::spawn(move || assert!(m2_plugins::probe::request_process(h))).join().unwrap();
    assert!(!step(SR as usize / 10), "woken");
    assert!(stats[1].process.load(std::sync::atomic::Ordering::Relaxed) > calls);
    drop(steps);
    let (_d, kept) = audio.join().unwrap();
    for s in &stats {
        let n = |a: &std::sync::atomic::AtomicU64| a.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!((n(&s.doubled), n(&s.callbacks_in_process), n(&s.process_says_main), n(&s.process_not_audio)), (0, 0, 0, 0));
        assert!(n(&s.requested) > 0);
    }
    assert_eq!(kept.iter().filter(|g| matches!(g, Gone::Engine(Retired::Renderer(_)))).count(), 1, "the old renderer was retired");
    assert!(!player.crashed());
}

// RENDER-28, HOST-3: the callback never allocates or locks with plugins in the host process
// either: two probes played through their shared-memory slots, an activation that moves both, a
// hold and its resume, a cell write into a parameter, a panic (each probe reset there), and stop.
// Red with an allocation in the slot's copy.
#[test]
fn the_audio_thread_never_allocates_with_plugins_in_the_host_process() {
    let host: Arc<dyn m2_plugins::PluginHost> = Arc::new(m2_plugins::Remote::for_tests());
    let (a, b) = (probes_on(1.0, Some(host.clone())), probes_on(0.5, Some(host)));
    let owner = m2_plugins::Owner::discard();
    let mut ra =
        Renderer::prepare(&a.flat, &a.data, &a.packages, SR, &mut m2_runtime::audio::Cache::default(), &owner, m2_runtime::Basis::Fresh)
            .unwrap();
    ra.set_strict(false);
    let rb = b.after(&a, &ra.info());
    let gain = CellTarget { slots: rb.cell_slots(), cells: rb.cell_map().cells(&cells()[0]).into() };
    assert_eq!(rb.stats().kept.plugins, 2, "both kept");
    let (player, d) = Player::driven(SR);
    player.open(ra).unwrap();
    let (steps, rx) = mpsc::channel::<usize>();
    let (reports, back) = mpsc::channel::<(u64, f32)>();
    let audio = std::thread::spawn(move || {
        let mut d = d;
        let mut kept: Vec<Gone> = vec![];
        let mut buf = vec![0.0f32; 2 * 4096];
        while let Ok(frames) = rx.recv() {
            let (mut done, mut peak) = (0, 0.0f32);
            let allocs = trapped(|| {
                for n in SIZES.iter().cycle() {
                    let n = (*n).min(frames - done);
                    if n == 0 {
                        break;
                    }
                    d.fill(&mut buf[..2 * n], 2);
                    peak = buf[..2 * n].iter().fold(peak, |m, x| m.max(x.abs()));
                    done += n;
                }
            });
            kept.extend(d.drain_retired());
            reports.send((allocs, peak)).unwrap();
        }
        kept
    });
    let step = |frames: usize| -> f32 {
        steps.send(frames).unwrap();
        let (allocs, peak) = back.recv().unwrap();
        assert_eq!(allocs, 0, "the audio thread allocated with plugins in the host process; M2_TRAP_TRACE=1 shows where");
        peak
    };
    player.play();
    assert!(step(SR as usize / 2) > 0.01, "the probes play");
    player.activate(rb).unwrap();
    assert!(step(SR as usize / 2) > 0.01, "and play on, moved");
    player.hold(true).unwrap();
    step(SR as usize / 10);
    player.hold(false).unwrap();
    step(SR as usize / 10);
    player.set_cells(&gain, 0.25);
    player.panic().unwrap();
    step(SR as usize / 4);
    player.stop();
    step(SR as usize / 4);
    drop(steps);
    let kept = audio.join().unwrap();
    assert_eq!(kept.iter().filter(|g| matches!(g, Gone::Engine(Retired::Renderer(_)))).count(), 1, "the old renderer was retired");
    assert!(!player.crashed());
}

// RENDER-28: what a callback allocates is counted, a plugin's included, as the callback marks its
// own thread: what the app's debug counter logs. A release build's callback marks nothing, so it
// touches no thread-local, whose first touch may allocate on macOS: nothing is counted there.
#[test]
fn an_allocation_in_the_callback_is_counted() {
    let a = probes(1.0);
    let owner = m2_plugins::Owner::discard();
    let r =
        Renderer::prepare(&a.flat, &a.data, &a.packages, SR, &mut m2_runtime::audio::Cache::default(), &owner, m2_runtime::Basis::Fresh)
            .unwrap();
    let probe = m2_plugins::probe::stats(r.plugin_handles()[0]).unwrap();
    let (player, mut d) = Player::driven(SR);
    player.open(r).unwrap();
    player.play();
    let mut buf = vec![0.0f32; 2 * 512];
    assert_eq!(trapped(|| d.fill(&mut buf, 2)), 0);
    probe.allocate.store(true, Ordering::Relaxed);
    let counted = if cfg!(debug_assertions) { 2 } else { 0 };
    assert_eq!(trapped(|| d.fill(&mut buf, 2)), counted, "the probe's box, made and freed, counted in debug builds only");
    drop(d.drain_retired());
}

// The trap itself: an allocation under it is counted (or the tests above prove nothing), and
// outside a callback nothing is.
#[test]
fn the_trap_counts() {
    let before = alloc::here();
    alloc::set_in_callback(true);
    let b = std::hint::black_box(Box::new(7u64));
    drop(b);
    alloc::set_in_callback(false);
    drop(std::hint::black_box(Box::new(7u64)));
    assert_eq!(alloc::here() - before, 2);
}

// RENDER-28 (platform survey-later's bug 1): cpal's error callback runs on the audio thread on
// ALSA, and CoreAudio's overloads on its realtime thread. Counting an error there allocates and
// frees nothing, for errors as cpal makes them: an xrun with no text, or with a static one (an
// ALSA suspend), a change, or a loss of the device.
#[test]
fn the_device_error_callback_never_allocates() {
    let (player, d) = Player::driven(SR);
    let errors: [DeviceError; 5] = [
        DeviceErrorKind::Xrun.into(),
        DeviceError::with_message(DeviceErrorKind::Xrun, "Device does not support suspend/resume"),
        DeviceErrorKind::DeviceChanged.into(),
        DeviceErrorKind::StreamInvalidated.into(),
        DeviceErrorKind::DeviceNotAvailable.into(),
    ];
    let n = trapped(|| {
        alloc::set_in_callback(true);
        for e in errors {
            d.error(e);
        }
        alloc::set_in_callback(false);
    });
    assert_eq!(n, 0);
    let e = player.device_errors();
    assert_eq!((e.xruns, e.changes, e.gone, e.other), (2, 1, 2, 0), "every one counted");
}

/// Reads made on a thread while it runs a callback (debug builds mark it).
static CALLBACK_READS: AtomicUsize = AtomicUsize::new(0);

fn on_read(_: &std::fs::File, _: usize) -> std::io::Result<()> {
    if alloc::in_callback() {
        CALLBACK_READS.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

// RENDER-30, RENDER-28: a lane that streams from the disk is only copied on the sound thread. Play,
// a seek, a loop's wraps and a tempo activation over a long clip, under the trap: no callback
// allocates, and none reads a file (the disk thread does).
#[test]
fn the_sound_thread_never_reads_a_file() {
    m2_os::fs::watch_reads(on_read);
    let (a, b) = (long_audio(30.0, 120.0), long_audio(30.0, 100.0));
    let r = a.renderer();
    let info = r.info();
    let (player, mut d) = Player::driven(SR);
    player.open(r).unwrap();
    let mut buf = vec![0.0f32; 2 * 480];
    let run = |d: &mut Driver, callbacks: usize, buf: &mut Vec<f32>| {
        let mut n = 0;
        for _ in 0..callbacks {
            n += trapped(|| d.fill(buf, 2));
            drop(d.drain_retired());
        }
        n
    };
    player.play();
    let mut allocs = run(&mut d, 100, &mut buf);
    player.seek(30.0).unwrap();
    allocs += run(&mut d, 100, &mut buf);
    player.set_loop(Some((32.0, 34.0))).unwrap();
    player.seek(32.0).unwrap();
    allocs += run(&mut d, 1000, &mut buf);
    player.activate(b.after(&a, &info)).unwrap();
    allocs += run(&mut d, 200, &mut buf);
    assert!(player.status().pos > 0 && peak(&buf.chunks(2).map(|f| [f[0], f[1]]).collect::<Vec<_>>()) > 0.0, "it plays");
    assert_eq!(player.disk_misses().iter().map(|m| m.1).sum::<u64>(), 0, "the disk kept up");
    // A disk that never answers: a seek far off finds nothing, and the callback plays silence
    // rather than read for itself.
    player.set_loop(None).unwrap();
    player.pause_disk(true);
    player.seek(10.0).unwrap();
    allocs += run(&mut d, 50, &mut buf);
    if cfg!(debug_assertions) {
        assert_eq!(CALLBACK_READS.load(Ordering::Relaxed), 0, "a callback read a file");
    }
    assert_eq!(allocs, 0, "the callback allocated");
    assert!(player.disk_misses().iter().map(|m| m.1).sum::<u64>() > 0, "the lane missed");
}
