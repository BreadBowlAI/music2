//! RENDER-28: the audio callback never allocates, from its very first fill on a fresh thread,
//! through everything step 3a does live: play, seek, a loop and its wraps, voices stolen, a lane
//! swap, an activation across a tempo change (a crossfade with a retime), a value set, stop,
//! idle, play, panic, and a full retire channel. Kernel memory never grows either: a reset on
//! the audio thread would grow a delay's (§1.5 of the step 3a design).
//!
//! Everything is built and sent on the test's thread; only the callback runs under the trap.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::mpsc;

use common::*;
use m2_ir::Origin;
use m2_play::{Gone, Player};
use m2_runtime::{LaneSwap, NotesData, Renderer, Retired};
use m2_song::{DataSet, Project};
use serde_json::{Value, json};

struct Trap;

thread_local! {
    static IN_AUDIO: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn count(what: &str, size: usize) {
    let _ = IN_AUDIO.try_with(|on| {
        if on.get() {
            let _ = COUNT.try_with(|n| n.set(n.get() + 1));
            if std::env::var_os("M2_TRAP_TRACE").is_some() {
                on.set(false);
                eprintln!("{what} of {size} bytes on the audio thread:\n{}", std::backtrace::Backtrace::force_capture());
                on.set(true);
            }
        }
    });
}

unsafe impl GlobalAlloc for Trap {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count("alloc", l.size());
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        count("dealloc", l.size());
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count("realloc", n);
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static TRAP: Trap = Trap;

/// The song: voices that hold a delay and a reverb and are stolen, notes with equal keys, a
/// control lane, time, a feedback loop and a delay at the top, and an audio lane. `fx` adds a
/// reverb at the end.
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
            {"id": "r", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": "d"}}]}}),
        json!({"id": "c", "op": "lane", "lane": "level"}),
        json!({"id": "t", "op": "time", "of": "beats"}),
        json!({"id": "l", "op": "kernel", "kernel": "std:lfo", "inputs": {"beats": "t"}}),
        json!({"id": "vc", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "v", "by": "c"}}),
        json!({"id": "vl", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "vc", "by": "l"}}),
        json!({"id": "a", "op": "lane", "lane": "clips"}),
        json!({"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "vl", "b": "a"}}),
        json!({"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "sum"}),
        json!({"id": "k", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}}),
        json!({"id": "h", "op": "const", "value": 0.3}),
        json!({"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "x", "b": "k"}}),
        json!({"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "sum"}}),
    ];
    if fx {
        n.push(json!({"id": "fx", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": "d"}}));
    }
    (Value::Array(n), if fx { "fx" } else { "d" })
}

/// The song's project at `bpm`, in a temp folder, compiled from `nodes` without TypeScript.
fn build(fx: bool, bpm: f64) -> Song {
    let dir = temp("trap");
    let ns: Vec<Value> = (0..64).map(|k| json!({"pitch": 48 + (k % 7) * 3, "start": k as f64 * 0.25, "length": 0.4})).collect();
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
        "modules": [{"key": "song", "nodes": n, "exports": {"out": out}, "lanes": [
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
    allocs: usize,
    idle: bool,
    pos: u64,
}

fn trap_run(device_rate: u32) {
    let a = build(false, 120.0);
    let b = build(true, 100.0);
    let mut ra = a.renderer();
    ra.set_strict(false);
    let (gen_a, mem_a) = (ra.generation(), ra.kernel_memory_bytes());
    let rb = b.renderer();
    let (gen_b, mem_b) = (rb.generation(), rb.kernel_memory_bytes());
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
            COUNT.with(|n| n.set(0));
            for n in SIZES.iter().cycle() {
                let n = (*n).min(frames - done);
                if n == 0 {
                    break;
                }
                IN_AUDIO.with(|on| on.set(true));
                d.fill(&mut buf[..2 * n], 2);
                IN_AUDIO.with(|on| on.set(false));
                done += n;
            }
            let allocs = COUNT.with(|n| n.get());
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
    player.play();
    step(s(0.5), true);
    player.seek(2.0).unwrap();
    player.set_loop(Some((2.0, 3.0))).unwrap();
    // Three wraps, voices stolen all along.
    step(s(1.6), true);
    player.set_loop(None).unwrap();
    player.swap_lane(gen_a, 0, LaneSwap::Notes(Arc::new(more))).unwrap();
    step(s(0.3), true);
    player.commit_const(Origin { site: 1, arg: vec!["0".into()], scale: None }, 0.5).unwrap();
    player.activate(rb).unwrap();
    step(s(0.5), true);
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
    for k in 0..120 {
        player.commit_const(Origin { site: 1000 + k, arg: vec![], scale: None }, 0.5).unwrap();
    }
    step(s(0.2), false);
    let pos = step(s(0.2), false).pos;
    step(s(0.3), true);
    let last = step(s(0.3), true);
    assert!(last.pos > pos, "it played on throughout");
    steps.send(Step::Done).unwrap();
    let (mut d, kept) = audio.join().unwrap();
    let origins = kept.iter().filter(|g| matches!(g, Gone::Engine(Retired::Origin(_)))).count();
    assert_eq!(origins, 121, "every value was applied and retired, none dropped");
    let old: Vec<&Renderer> =
        kept.iter().filter_map(|g| if let Gone::Engine(Retired::Renderer(r)) = g { Some(&**r) } else { None }).collect();
    assert_eq!(old.iter().map(|r| r.generation()).collect::<Vec<_>>(), [gen_a]);
    assert_eq!(old[0].kernel_memory_bytes(), mem_a, "no kernel grew its memory on the audio thread");
    let e = d.engine().unwrap();
    assert_eq!((e.resets(), e.activations(), e.refused_swaps()), (1, 1, 0));
    let cur = e.current().unwrap();
    assert_eq!(cur.generation(), gen_b);
    assert_eq!(cur.kernel_memory_bytes(), mem_b, "nor after the panic reset every kernel");
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

// The trap itself: an allocation under it is counted (or the tests above prove nothing).
#[test]
fn the_trap_counts() {
    COUNT.with(|n| n.set(0));
    IN_AUDIO.with(|on| on.set(true));
    let b = std::hint::black_box(Box::new(7u64));
    drop(b);
    IN_AUDIO.with(|on| on.set(false));
    assert_eq!(COUNT.with(|n| n.get()), 2);
}
