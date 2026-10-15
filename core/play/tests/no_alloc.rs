//! RENDER-28: the audio callback never allocates or frees, from its very first fill on a fresh
//! thread, through everything the engine does live: play, seek, a loop and its wraps, voices
//! stolen, a lane swap, an activation across a tempo change that keeps kernels, voices, a ring and
//! an audio player, swaps the audio lane's renders, grows a lane, and plays out a ghost reading a
//! ghost lane while its edges fade, a panic in that window, the window's end and its leftovers, a
//! stale activation refused, stop, idle, play, panic, and a full retire channel; and, from step 5,
//! cell writes from another thread all along (a step, a ramp and a cell in voices), with no
//! rebuild. Kernel memory never grows either: a reset on the audio thread would grow a delay's
//! (§1.5 of the step 3a design).
//!
//! Everything is built and sent on the test's thread; only the callback runs under the trap.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use common::*;
use m2_ir::Origin;
use m2_play::{DeviceError, DeviceErrorKind, Gone, Player, alloc};
use m2_runtime::{CellTarget, LaneSwap, NotesData, Renderer, Retired};
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
    // The audio thread: fresh, under the trap from its very first fill.
    let audio = std::thread::spawn(move || {
        let mut d = d;
        let mut kept: Vec<Gone> = vec![];
        let mut buf = vec![0.0f32; 2 * 4096];
        while let Ok(Step::Fill(frames, take)) = rx.recv() {
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
    let writing = Arc::new(AtomicBool::new(true));
    let writer = {
        let (player, both, target, writing) = (player.clone(), both.clone(), target.clone(), writing.clone());
        std::thread::spawn(move || {
            let mut k = 0;
            while writing.load(Ordering::Relaxed) {
                for t in &both[target.load(Ordering::Relaxed)] {
                    player.set_cells(t, 0.2 + (k % 5) as f64 * 0.1);
                }
                k += 1;
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

/// A probe instrument on 64 notes into a probe effect (step 3b, §6.5), `gain` on the instrument,
/// a cell (written at [`cells`]'s first site).
fn probes(gain: f64) -> Song {
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
    let diags = packages.bind_plugins(&mut ir);
    assert!(diags.is_empty(), "{diags:?}");
    let flat = m2_ir::check(&ir, &packages).unwrap_or_else(|e| panic!("{e:?}"));
    let (data, _) = Project::open(&dir).unwrap().data(&m2_runtime::decls(&ir), None);
    Song { flat, data, packages, rate: SR, dir }
}

// RENDER-28, step 3b (§6.5): the callback never allocates with plugins either: two probe instances
// played, then an activation that moves both while notes sound into one (its notes and last-sent
// values with it), a hold for an export and its resume with a panic deferred to it (RENDER-1), a
// cell write into a parameter, a panic, stop, the plugins asleep and the engine idle, then woken
// by a plugin's request from a thread of its own. The probe
// asks for its main-thread callback from `process` every 10 calls. Only the host glue is trapped
// here: a real plugin's own allocations are its own (step 3a §13).
#[test]
fn the_audio_thread_never_allocates_with_plugins() {
    let (a, b) = (probes(1.0), probes(0.5));
    let owner = m2_plugins::Owner::discard();
    let mut ra = Renderer::prepare(&a.flat, &a.data, &a.packages, SR, &mut m2_runtime::audio::Cache::default(), &owner, m2_runtime::Basis::Fresh).unwrap();
    ra.set_strict(false);
    let handles = ra.plugin_handles();
    let rb = b.after(&a, &ra.info());
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

// RENDER-28: what a callback allocates is counted, a plugin's included, as the callback marks its
// own thread: what the app's debug counter logs.
#[test]
fn an_allocation_in_the_callback_is_counted() {
    let a = probes(1.0);
    let owner = m2_plugins::Owner::discard();
    let r = Renderer::prepare(&a.flat, &a.data, &a.packages, SR, &mut m2_runtime::audio::Cache::default(), &owner, m2_runtime::Basis::Fresh).unwrap();
    let probe = m2_plugins::probe::stats(r.plugin_handles()[0]).unwrap();
    let (player, mut d) = Player::driven(SR);
    player.open(r).unwrap();
    player.play();
    let mut buf = vec![0.0f32; 2 * 512];
    assert_eq!(trapped(|| d.fill(&mut buf, 2)), 0);
    probe.allocate.store(true, Ordering::Relaxed);
    assert_eq!(trapped(|| d.fill(&mut buf, 2)), 2, "the probe's box, made and freed");
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
