//! Step 3's engine gates, driven through the player as the device would drive it (simulated
//! time): loops that keep their tails and never drift (G2), note edits heard at their frame with
//! no rebuild (G3), no stuck notes (G4), activations with no jump back (G5), no clicks (G6), and
//! tails that decay after stop (G7). The timed versions on p16 run through the server (step 3a's
//! second half).

mod common;

use std::sync::Arc;

use common::*;
use m2_play::{Driver, Player};
use m2_runtime::{LaneSwap, NotesData, Renderer, TraceEntry, TraceKind};
use m2_song::{Arrangement, DataSet, LaneData};
use serde_json::{Value, json};

/// The engine playing in `d`.
fn engine(d: &mut Driver) -> &mut m2_runtime::Engine {
    d.engine().expect("a project is open")
}

fn trace(d: &mut Driver) -> Vec<TraceEntry> {
    engine(d).current().unwrap().trace_entries()
}

/// Fill exactly `frames` frames in device buffers of [`SIZES`], stereo.
fn fill(d: &mut Driver, frames: usize) -> Vec<[f32; 2]> {
    hear(d, frames, &mut |_, _| {})
}

fn lane_data(song: &Song, v: Value) -> LaneSwap {
    let LaneData::Notes(clips) = notes(v) else { unreachable!() };
    LaneSwap::Notes(Arc::new(NotesData::build(&clips, song.data.tempo(), song.rate as f64)))
}

/// A probe for the loop gate: one note at beats 132-136, mid-loop, into `fx`, at 127 bpm.
fn loop_probe(fx: Value) -> Song {
    synth(
        0.1,
        fx,
        "x",
        json!({"clips": [{"id": "a", "start": 128, "length": 16, "notes": [{"pitch": 57, "start": 4, "length": 4}]}]}),
        at_bpm(127.0),
    )
}

/// Loop beats 128-144 at 127 bpm for `wraps` wraps, measuring the 50 ms on each side of every
/// wrap. With `panic`, a panic is sent before the fill that holds each wrap: the negative control.
/// Returns (RMS before, RMS after) per wrap, and the trace.
fn loop_run(song: &Song, wraps: usize, panic: bool) -> (Vec<(f32, f32)>, Vec<TraceEntry>, u64) {
    let mut r = song.renderer();
    r.trace(4 * wraps + 16);
    let (player, mut d) = Player::driven(SR);
    player.open(r).unwrap();
    player.seek(128.0).unwrap();
    player.set_loop(Some((128.0, 144.0))).unwrap();
    player.play();
    // Beats 128 and 144 at 127 bpm; the transport starts at the loop's start at engine frame 0.
    let (start, end) = (2_902_677u64, 3_265_512u64);
    let lap = end - start;
    let w = (0.05 * SR as f64) as u64;
    let mut sums = vec![(0.0f64, 0.0f64); wraps];
    let mut data = vec![0.0f32; 2 * 4096];
    let (mut done, mut k) = (0u64, 0usize);
    let total = lap * wraps as u64 + w;
    while done < total {
        let mut n = SIZES[k % SIZES.len()] as u64;
        k += 1;
        // The next wrap: cut the fill there, so a panic lands just after it.
        let next = (done / lap + 1) * lap;
        if done < next && done + n > next {
            n = next - done;
        }
        if panic && done % lap == 0 && done > 0 {
            player.panic().unwrap();
        }
        let n = n.min(total - done) as usize;
        d.fill(&mut data[..2 * n], 2);
        for (i, f) in data[..2 * n].chunks(2).enumerate() {
            let at = done + i as u64;
            let e = 0.5 * (f[0] as f64 * f[0] as f64 + f[1] as f64 * f[1] as f64);
            let (q, r) = (at / lap, at % lap);
            if q >= 1 && (q as usize) <= wraps && r < w {
                sums[q as usize - 1].1 += e;
            } else if (q as usize) < wraps && r >= lap - w {
                sums[q as usize].0 += e;
            }
        }
        done += n as u64;
        drop(d.drain_retired());
    }
    let resets = engine(&mut d).resets();
    assert_eq!(engine(&mut d).activations(), 0);
    let rms: Vec<(f32, f32)> = sums.iter().map(|(b, a)| ((b / w as f64).sqrt() as f32, (a / w as f64).sqrt() as f32)).collect();
    (rms, trace(&mut d), resets)
}

// Step 3 gate (G2), RENDER-9, RENDER-9a: a 4-bar loop at 127 bpm runs 10 minutes (79 wraps) with
// no tail cut and no drift. The probe note is mid-loop, so nothing starts within 50 ms of a wrap;
// its reverb (or delay) tail must ring across every wrap. A panic at each wrap must fail this.
#[test]
fn a_loop_keeps_its_tails_for_10_minutes() {
    let reverb = loop_probe(json!([
        {"id": "sz", "op": "const", "value": 0.98}, {"id": "dm", "op": "const", "value": 0}, {"id": "mx", "op": "const", "value": 1},
        {"id": "x", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": "v", "size": "sz", "damping": "dm", "mix": "mx"}}]));
    let delay = loop_probe(json!([
        {"id": "t", "op": "const", "value": 0.25, "unit": "seconds"}, {"id": "f", "op": "const", "value": 0.9}, {"id": "mx", "op": "const", "value": 1},
        {"id": "x", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v", "time": "t", "feedback": "f", "mix": "mx"}}]));
    let lap = 3_265_512 - 2_902_677;
    // Beat 132 is 4 beats into the loop.
    let note = 2_993_386u64 - 2_902_677;
    for (name, song) in [("reverb", &reverb), ("delay", &delay)] {
        let (rms, trace, resets) = loop_run(song, 79, false);
        assert_eq!(resets, 0);
        for (k, (before, after)) in rms.iter().enumerate() {
            assert!(db(*before) > -60.0, "{name}, wrap {k}: the tail must sound before the wrap ({} dB)", db(*before));
            assert!(after >= &(0.5 * before), "{name}, wrap {k}: cut from {} to {} dB", db(*before), db(*after));
        }
        // No drift: each lap's On is at the same transport frame, and lap k's at E_k plus it.
        let ons: Vec<(u64, u64)> =
            trace.iter().filter(|e| matches!(e.kind, TraceKind::On { .. })).map(|e| (e.engine, e.transport)).collect();
        // The last lap is heard only for 50 ms, before its note.
        assert_eq!(ons.len(), 79, "{name}");
        for (k, (engine, transport)) in ons.iter().enumerate() {
            assert_eq!((*engine, *transport), (k as u64 * lap + note, 2_902_677 + note), "{name}, lap {k}");
        }
    }
    // The negative control: a panic at each wrap cuts the tail, and the detector sees it.
    let (rms, _, resets) = loop_run(&reverb, 5, true);
    assert_eq!(resets, 5);
    assert!(rms.iter().all(|(b, a)| *a < 0.5 * b), "{rms:?}");
}

/// A pad: one long note per clip at the given pitches, into a release of 50 ms.
fn pad(clips: Value) -> Song {
    synth(0.05, json!([]), "v", clips, at_bpm(120.0))
}

/// `song` playing from 0 with its trace on.
fn play(song: &Song) -> (Player, Driver) {
    let mut r = song.renderer();
    r.trace(1024);
    playing(r, SR)
}

/// After stop and the release: nothing held, and silence.
fn settles(player: &Player, d: &mut Driver, what: &str) {
    player.stop();
    fill(d, SR as usize / 4);
    let tail = fill(d, SR as usize / 10);
    let cur = engine(d).current().unwrap();
    assert_eq!((cur.held_notes(), cur.sounding_notes()), (0, 0), "{what}: a note is stuck");
    assert!(db(peak(&tail)) < -80.0, "{what}: {} dB after the release", db(peak(&tail)));
}

/// The engine frames of the Offs in the trace.
fn offs(d: &mut Driver) -> Vec<u64> {
    trace(d).iter().filter(|e| e.kind == TraceKind::Off).map(|e| e.engine).collect()
}

// Step 3 gate (G4), RENDER-26a: deleting or shortening a sounding note, stopping, wrapping,
// seeking and moving its clip each end it at the right frame, and nothing stays stuck.
#[test]
fn no_edit_or_jump_leaves_a_note_stuck() {
    let long = |len: f64, start: f64| json!({"clips": [{"id": "a", "start": start, "length": 32, "notes": [{"pitch": 60, "start": 0, "length": len}]}]});
    let song = pad(long(16.0, 0.0));
    // From one second in, with the note sounding: (what, the change, where its Off must be; None:
    // the frame the change applied at).
    type Change = Box<dyn Fn(&Player, u64) -> u64>;
    let swap = |v: Value| -> Change {
        let data = lane_data(&song, v);
        Box::new(move |p, g| p.swap_lane(g, 0, data.clone()).unwrap())
    };
    let cases: Vec<(&str, Change, Option<u64>)> = vec![
        ("deleted", swap(json!({"clips": [{"id": "a", "start": 0, "length": 32, "notes": []}]})), None),
        ("shortened behind the playhead", swap(long(1.0, 0.0)), None),
        ("shortened ahead", swap(long(4.0, 0.0)), Some(96_000)),
        ("its clip moved", swap(long(16.0, 0.25)), None),
        ("stopped", Box::new(|p, _| p.transport(m2_runtime::Cmd::Stop).unwrap()), None),
        ("seeked into it", Box::new(|p, _| p.seek(3.0).unwrap()), None),
        ("wrapped", Box::new(|p, _| p.set_loop(Some((0.5, 2.5))).unwrap()), Some(60_000)),
    ];
    for (what, change, at) in cases {
        let (player, mut d) = play(&song);
        fill(&mut d, SR as usize);
        let generation = engine(&mut d).current().unwrap().generation();
        let seq = change(&player, generation);
        fill(&mut d, SR as usize * 2);
        let applied = player.applied(seq).unwrap_or_else(|| panic!("{what}: applied")).clock;
        let expect = at.unwrap_or(applied);
        assert_eq!(offs(&mut d).first(), Some(&expect), "{what}");
        assert_eq!(engine(&mut d).resets(), 0);
        if what == "wrapped" {
            player.set_loop(None).unwrap();
        }
        settles(&player, &mut d, what);
    }
    // Lengthened: it sounds past its old end, until stop.
    let (player, mut d) = play(&song);
    fill(&mut d, SR as usize);
    let g = engine(&mut d).current().unwrap().generation();
    player.swap_lane(g, 0, lane_data(&song, long(24.0, 0.0))).unwrap();
    fill(&mut d, SR as usize * 8);
    assert!(offs(&mut d).is_empty() && engine(&mut d).current().unwrap().held_notes() == 1, "lengthened");
    settles(&player, &mut d, "lengthened");
}

// RENDER-26a: notes with equal keys (duplicates, layered clips with one id, anonymous clips at one
// start) are told apart by their place among equals: deleting one ends one, shortening one moves
// one Off, and a no-op swap changes no bit.
#[test]
fn equal_notes_are_told_apart() {
    let lane = |a: &[f64], layered: usize, anon: usize| {
        let dup: Vec<Value> = a.iter().map(|len| json!({"pitch": 60, "start": 0, "length": len})).collect();
        let mut clips = vec![json!({"id": "a", "start": 0, "length": 32, "notes": dup})];
        clips
            .extend((0..layered).map(|_| json!({"id": "l", "start": 0, "length": 32, "notes": [{"pitch": 64, "start": 0, "length": 12}]})));
        clips.extend((0..anon).map(|_| json!({"start": 0.5, "length": 32, "notes": [{"pitch": 67, "start": 0, "length": 12}]})));
        json!({"clips": clips})
    };
    let song = pad(lane(&[12.0, 14.0], 2, 2));
    let run = |change: Option<Value>| {
        let (player, mut d) = play(&song);
        let mut out = fill(&mut d, SR as usize);
        assert_eq!(engine(&mut d).current().unwrap().held_notes(), 6);
        if let Some(v) = change {
            let g = engine(&mut d).current().unwrap().generation();
            player.swap_lane(g, 0, lane_data(&song, v)).unwrap();
        }
        // To 3 s: beat 6, before any of them ends.
        out.extend(fill(&mut d, SR as usize * 2));
        let held = engine(&mut d).current().unwrap().held_notes();
        let o = offs(&mut d);
        settles(&player, &mut d, "duplicates");
        (out, held, o)
    };
    let (plain, held, _) = run(None);
    assert_eq!(held, 6);
    let (same, held, o) = run(Some(lane(&[12.0, 14.0], 2, 2)));
    assert_eq!((first_diff(&same, &plain), held, o.len()), (None, 6, 0), "a no-op swap");
    let (_, held, o) = run(Some(lane(&[14.0], 1, 1)));
    assert_eq!((held, o.len()), (3, 3), "one of each pair deleted");
    let (_, held, o) = run(Some(lane(&[3.0, 14.0], 2, 2)));
    assert_eq!((held, o), (5, vec![72_000]), "the shorter duplicate shortened to 3 beats");
}

// RENDER-26a, RENDER-9, RENDER-25: over 2000 random blocks of seeks, stops, plays, loops, swaps and
// activations (an effect added and removed, the voices kept), some with a jump on the activation's
// block line, every voice whose gate is open plays a note its scheduler holds, and nothing is held
// after stop.
#[test]
fn random_edits_and_jumps_never_strand_a_gate() {
    let notes_of = |seed: &mut u64| {
        let n: Vec<Value> = (0..12)
            .map(|_| {
                let start = (lcg(seed) % 64) as f64 / 4.0;
                json!({"pitch": 48 + lcg(seed) % 24, "start": start, "length": 0.25 + (lcg(seed) % 32) as f64 / 4.0})
            })
            .collect();
        json!({"clips": [{"id": "a", "start": 0, "length": 32, "notes": n}, {"id": "a", "start": 0, "length": 32, "notes": [n[0].clone()]}]})
    };
    let mut seed = 11;
    let first = notes_of(&mut seed);
    let song = pad(first.clone());
    let delayed = synth(0.05, json!([{"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v"}}]), "d", first, at_bpm(120.0));
    let songs = [&song, &delayed];
    let r = song.renderer();
    let mut sent = (0, r.info());
    let (player, mut d) = playing(r, SR);
    for _ in 0..2000 {
        let jump = |seed: &mut u64| match lcg(seed) % 6 {
            0 => drop(player.seek((lcg(seed) % 64) as f64 / 4.0)),
            1 => player.stop(),
            2 => player.play(),
            3 => drop(player.set_loop(Some(((lcg(seed) % 8) as f64, 8.0 + (lcg(seed) % 8) as f64)))),
            4 => drop(player.set_loop(None)),
            _ => {}
        };
        match lcg(&mut seed) % 40 {
            0..=5 => jump(&mut seed),
            6 | 7 => {
                let g = engine(&mut d).current().unwrap().generation();
                player.swap_lane(g, 0, lane_data(&song, notes_of(&mut seed))).unwrap();
            }
            8 | 9 => {
                // An edit, prepared against the plan sent last, and a jump on its block line.
                let next = 1 - sent.0;
                let r = songs[next].after(songs[sent.0], &sent.1);
                sent = (next, r.info());
                player.activate(r).unwrap();
                jump(&mut seed);
            }
            _ => {}
        }
        fill(&mut d, 128);
        assert!(engine(&mut d).current().unwrap().gates_are_owned(), "a gate is open with no note to close it");
    }
    assert_eq!(engine(&mut d).current().unwrap().dropped_ons(), 0);
    assert!(engine(&mut d).activations() > 50 && engine(&mut d).refused_activations() == 0);
    player.set_loop(None).unwrap();
    settles(&player, &mut d, "after 2000 random blocks");
}

// Step 3 gate (G3), RENDER-26: a note added while playing is heard at its frame with no rebuild.
// Two engines fed the same messages, one also swapping in the note, play the same bits until the
// note starts; nothing was reset or rebuilt.
#[test]
fn a_swapped_in_note_plays_at_its_frame_and_changes_nothing_before() {
    let base = |extra: bool| {
        let mut n = vec![json!({"pitch": 48, "start": 0, "length": 8}), json!({"pitch": 55, "start": 1, "length": 0.5})];
        if extra {
            n.push(json!({"pitch": 67, "start": 2.5, "length": 1}));
        }
        json!({"clips": [{"id": "a", "start": 0, "length": 16, "notes": n}]})
    };
    let song =
        synth(0.2, json!([{"id": "x", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": "v"}}]), "x", base(false), at_bpm(120.0));
    let (pa, mut a) = play(&song);
    let (pb, mut b) = play(&song);
    let mut x = fill(&mut a, SR as usize);
    let mut y = fill(&mut b, SR as usize);
    let g = engine(&mut b).current().unwrap().generation();
    let seq = pb.swap_lane(g, 0, lane_data(&song, base(true))).unwrap();
    x.extend(fill(&mut a, SR as usize));
    y.extend(fill(&mut b, SR as usize));
    // Beat 2.5 at 120 bpm. A sine starts at 0, so the first sample may not differ yet.
    let m = 60_000;
    let diff = first_diff(&x, &y).expect("the new note sounds");
    assert!((m..m + 2).contains(&diff), "identical until the new note starts at {m}, then it sounds: {diff}");
    drop(pa);
    let ons: Vec<u64> = trace(&mut b).iter().filter(|e| matches!(e.kind, TraceKind::On { .. })).map(|e| e.engine).collect();
    assert!(ons.contains(&(m as u64)));
    assert!(pb.applied(seq).is_some_and(|s| s.clock <= m as u64));
    let e = engine(&mut b);
    assert_eq!((e.resets(), e.activations(), e.current().unwrap().generation()), (0, 0, g), "no rebuild, no reset");
}

/// A project at `rate` whose song is one audio lane: a 441 Hz sine at half scale, from beat 0 to
/// 16 at 120 bpm.
fn audio_lane_song(rate: u32) -> Song {
    let dir = temp("clicks");
    std::fs::write(
        dir.join("music2.toml"),
        format!("[project]\nname = \"t\"\nformat = 1\nsample_rate = {rate}\n\n[dependencies]\n\"@m2/core\" = \"1\"\n"),
    )
    .unwrap();
    std::fs::write(dir.join("song.ts"), "").unwrap();
    std::fs::create_dir_all(dir.join("data/song")).unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("data/arrangement.json"), r#"{"tempo": [{"beat": 0, "bpm": 120}]}"#).unwrap();
    std::fs::write(dir.join("data/song/clips.json"), r#"{"clips": [{"id": "c", "start": 0, "length": 16, "asset": "s.wav"}]}"#).unwrap();
    let wave: Vec<f32> = (0..10 * SR as usize).map(|i| (i as f32 * std::f32::consts::TAU * 441.0 / SR as f32).sin() * 0.5).collect();
    m2_runtime::audio::write_wav(&dir.join("assets/s.wav"), &[wave], SR).unwrap();
    let ir: m2_ir::Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "lanes": [{"key": "clips", "kind": "audio<1>"}], "nodes": [{"id": "a", "op": "lane", "lane": "clips"}],
        "exports": {"out": "a"}}]}))
    .unwrap();
    let packages = std_packages();
    let flat = m2_ir::check(&ir, &packages).unwrap();
    let (data, _) = m2_song::Project::open(&dir).unwrap().data(&m2_runtime::decls(&ir), None);
    Song { flat, data, packages, rate, dir }
}

// Step 3 gate (G6), RENDER-9 item 5: stopping and playing again a block later, two seeks a block
// apart, and playing from inside a clip make no step larger than twice the lane material's own.
#[test]
fn audio_lanes_never_click() {
    let song = audio_lane_song(SR);
    let own = max_step(&song.renderer().render(1.0).unwrap());
    let check = |what: &str, steps: &dyn Fn(&Player, &mut Driver) -> Vec<[f32; 2]>| {
        let (player, mut d) = play(&song);
        let mut out = fill(&mut d, SR as usize / 2);
        out.extend(steps(&player, &mut d));
        out.extend(fill(&mut d, SR as usize / 10));
        assert!(max_step(&out) <= 2.0 * own, "{what}: a step of {} against the material's own {own}", max_step(&out));
        assert!(peak(&out[out.len() - 1000..]) > 0.4, "{what}: it plays on");
    };
    check("stop, then play a block later", &|p, d| {
        p.stop();
        let mut o = fill(d, 128);
        p.play();
        o.extend(fill(d, 1000));
        o
    });
    check("two seeks a block apart", &|p, d| {
        p.seek(2.3).unwrap();
        let mut o = fill(d, 128);
        p.seek(5.7).unwrap();
        o.extend(fill(d, 1000));
        o
    });
    check("play from inside a clip", &|p, d| {
        p.stop();
        let mut o = fill(d, 2000);
        p.seek(7.1).unwrap();
        o.extend(fill(d, 2000));
        p.play();
        o.extend(fill(d, 1000));
        o
    });
}

// RENDER-9 item 5, RENDER-28: jumps as close together as the engine takes them never run an audio
// lane out of fades, at any project rate. A seek to 50 frames before a loop's end on every block
// line, each followed by its wrap, makes two fades a block; without a loop, a seek on every block
// line. Before the fix, two such blocks at 48 kHz, or four seeks at 96 kHz (a fade spans almost
// four blocks there), tripped an assert on the audio thread and crashed the player in debug
// builds; release builds cut a fade short.
#[test]
fn close_jumps_never_run_an_audio_lane_out_of_fades() {
    for (rate, looped) in [(48_000, true), (96_000, false), (96_000, true), (192_000, true), (384_000, true)] {
        let song = audio_lane_song(rate);
        let (player, mut d) = playing(song.renderer(), rate);
        let mut buf = vec![0.0f32; 2 * 128];
        let mut out = vec![];
        let mut block = |d: &mut Driver, out: &mut Vec<[f32; 2]>| {
            d.fill(&mut buf, 2);
            out.extend(buf.chunks(2).map(|f| [f[0], f[1]]));
            drop(d.drain_retired());
        };
        if looped {
            player.set_loop(Some((2.0, 3.0))).unwrap();
        }
        // Device buffers of one block, so each seek lands on a block line of its own.
        for _ in 0..64 {
            block(&mut d, &mut out);
        }
        // 50 frames before the loop's end (beat 3: 1.5 s at 120 bpm).
        let target = 3.0 - 50.0 / (rate as f64 / 2.0);
        let fade_blocks = (rate as usize * 5 / 1000).div_ceil(128);
        for _ in 0..2 * fade_blocks + 4 {
            player.seek(target).unwrap();
            block(&mut d, &mut out);
        }
        for _ in 0..4 * fade_blocks {
            block(&mut d, &mut out);
        }
        let what = format!("{rate} Hz{}", if looped { ", looping" } else { "" });
        assert!(!player.crashed(), "{what}: the player crashed");
        let dropped = d.engine().expect("still playing").current().unwrap().dropped_taps();
        assert_eq!(dropped, 0, "{what}: fades cut short");
        assert!(out.iter().all(|f| f[0].is_finite() && f[1].is_finite()) && peak(&out[out.len() - 128..]) > 0.1, "{what}: it plays on");
    }
}

/// After stop: the tail over 3 s.
struct Tail {
    before: f32,
    after: f32,
    out: Vec<[f32; 2]>,
}

/// Play `song` (a note at 0, and nothing after) for `stop_at` seconds, stop, and hear 3 more
/// seconds: exactly what it plays without stopping, as stop resets nothing and starts nothing.
fn tail_after_stop(song: &Song, stop_at: f64, mut each_fill: impl FnMut(&mut Driver)) -> Tail {
    let heard = |stop: bool, each_fill: &mut dyn FnMut(&mut Driver)| {
        let (player, mut d) = play(song);
        let lead = fill(&mut d, (stop_at * SR as f64) as usize);
        let seq = stop.then(|| player.transport(m2_runtime::Cmd::Stop).unwrap());
        let mut out = vec![];
        for _ in 0..3 * SR as usize / 480 {
            out.extend(fill(&mut d, 480));
            each_fill(&mut d);
        }
        (player, d, lead, seq, out)
    };
    let (_, _, _, _, playing_on) = heard(false, &mut |_| {});
    let (player, mut d, lead, seq, out) = heard(true, &mut each_fill);
    // Bit for bit, until the stopped engine idles: then it is silence where playing on is below
    // -100 dBFS (RENDER-6a).
    let differs = out.iter().zip(&playing_on).position(|(s, p)| s != p && !(s == &[0.0; 2] && peak(&[*p]) < 1e-5));
    assert_eq!(differs, None, "a tail after stop is the tail playing on");
    let seq = seq.unwrap();
    // Where the stop landed, in `out`.
    let at = (player.applied(seq).unwrap().clock as usize).saturating_sub(lead.len());
    let pos = engine(&mut d).status().pos;
    let w = SR as usize / 20;
    let mut all = lead;
    all.extend(&out);
    let s = all.len() - out.len() + at;
    let tail = Tail { before: rms(&all[s - w..s]), after: rms(&all[s..s + w]), out: out[at..].to_vec() };
    fill(&mut d, 4800);
    assert_eq!(engine(&mut d).status().pos, pos, "the transport is frozen");
    assert_eq!(engine(&mut d).resets(), 0);
    tail
}

/// Peak over consecutive windows of `n` frames.
fn peaks(x: &[[f32; 2]], n: usize) -> Vec<f32> {
    x.chunks(n).map(peak).collect()
}

// Step 3 gate (G7), RENDER-6a: after stop a tail decays instead of freezing: into a reverb, into
// a delay with feedback, and into a 500 ms delay inside the voice, which also keeps the engine from
// idling between its echoes.
#[test]
fn tails_decay_after_stop() {
    let note = |len: f64| json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 69, "start": 0, "length": len}]}]});
    // A noise burst: a sine through a reverb beats by ±10 dB from one 50 ms window to the next.
    let reverb = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "a", "nodes": [
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "o", "op": "kernel", "kernel": "std:noise"},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]}},
            {"id": "mx", "op": "const", "value": 1},
            {"id": "x", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": "v", "mix": "mx"}}]),
        "x",
        json!([{"key": "notes", "kind": "events<note>"}]),
        DataSet::with_lanes(Arrangement::default(), [(addr("notes"), notes(note(0.2)))]),
    );
    let delay = synth(
        0.02,
        json!([{"id": "t", "op": "const", "value": 0.1, "unit": "seconds"}, {"id": "f", "op": "const", "value": 0.6},
            {"id": "x", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v", "time": "t", "feedback": "f"}}]),
        "x",
        note(0.2),
        at_bpm(120.0),
    );
    for (what, song, period) in [("reverb", &reverb, 0.1), ("delay", &delay, 0.1)] {
        let t = tail_after_stop(song, 0.3, |_| {});
        assert!(t.after >= 0.5 * t.before, "{what}: {} dB, then {} dB", db(t.before), db(t.after));
        let p = peaks(&t.out, (period * SR as f64) as usize);
        let rising = p.windows(2).position(|w| db(w[1]) > db(w[0]) + 1.0 && db(w[1]) > -80.0);
        assert_eq!(rising, None, "{what} doesn't decay: {:?}", &p[..10]);
        assert!(db(*p.last().unwrap()) < -80.0, "{what}: {} dB after 3 s", db(*p.last().unwrap()));
        let frozen = t.out.chunks(128).collect::<Vec<_>>().windows(2).any(|w| w[0] == w[1] && peak(w[0]) > 0.0);
        assert!(!frozen, "{what}: a block repeats");
    }
    // A 500 ms echo inside the voice: stopped mid-echo, the next echoes come after silent gaps.
    let voice = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "d", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}},
                {"id": "t", "op": "const", "value": 0.5, "unit": "seconds"},
                {"id": "f", "op": "const", "value": 0.2},
                {"id": "m", "op": "const", "value": 1},
                {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "a", "time": "t", "feedback": "f", "mix": "m"}}]}}]),
        "v",
        json!([{"key": "notes", "kind": "events<note>"}]),
        DataSet::with_lanes(Arrangement::default(), [(addr("notes"), notes(note(0.2)))]),
    );
    let mut idled = false;
    let t = tail_after_stop(&voice, 0.55, |d| idled |= engine(d).is_idle());
    assert!(t.after >= 0.5 * t.before, "voice delay: {} dB, then {} dB", db(t.before), db(t.after));
    assert!(!idled, "echoes still to come: never idle");
    // Stopped at 0.55 s: echoes at 1.0, 1.5 and 2.0 s, after silent gaps far longer than 8 blocks.
    let at = |s: f64| ((s - 0.55) * SR as f64) as usize;
    assert!(peak(&t.out[at(0.75)..at(0.95)]) < 1e-6, "a silent gap");
    let p = peaks(&t.out[at(1.0)..], SR as usize / 2);
    assert!(db(p[0]) > -20.0 && db(p[1]) > -35.0, "the echoes after the gaps: {p:?}");
    assert!(p.windows(2).all(|w| db(w[1]) <= db(w[0]) + 1.0), "decaying: {p:?}");
}

// RENDER-6a, RENDER-9, O3: std's tremolo follows the song. On a steady level (a square at 0 Hz,
// which holds at 1), stopped, its level holds still, at the export's level where it stopped; Play
// goes on from there as if it never stopped: the export from that frame, bit for bit. Through loop
// wraps mid-block and a seek, the wobble keeps its phase and speed: from the first frame to the
// last, no step is bigger than twice the wobble's own largest (G6's rule).
#[test]
fn tremolo_holds_while_stopped_and_never_jumps() {
    let song = project(
        "tremolo",
        &[
            (
                "song.ts",
                "import { kernel, song } from \"@m2/core\";\nimport { tremolo } from \"std/fx\";\n\n\
                 export default song(() => tremolo(kernel(\"std:square\", { freq: 0 }), { rate: 3, depth: 1 }));\n",
            ),
            ("data/arrangement.json", &json!({"tempo": [{"beat": 0, "bpm": 120}], "meter": []}).to_string()),
        ],
    );
    let mut export = vec![[0.0; 2]; 4 * SR as usize];
    song.renderer().process(&mut export).unwrap();
    let own = max_step(&export);
    assert!(peak(&export) > 0.99 && own > 0.0 && own < 1e-3, "a slow wobble from 0 to 1: steps up to {own}");
    let (player, mut d) = playing(song.renderer(), SR);
    let lead = fill(&mut d, SR as usize * 6 / 10 + 333);
    assert!(lead == export[..lead.len()], "playing, the export");
    let seq = player.transport(m2_runtime::Cmd::Stop).unwrap();
    let stopped = fill(&mut d, SR as usize / 2);
    let stop_at = player.applied(seq).unwrap().clock as usize - lead.len();
    let pos = player.status().pos as usize;
    assert_eq!(pos, lead.len() + stop_at, "stopped at the block line after the stop was sent");
    assert!(stopped[..stop_at] == export[lead.len()..pos], "playing until the stop lands");
    let level = export[pos];
    assert!(level[0] > 0.05 && level[0] < 0.95, "stopped mid-wobble: {level:?}");
    let still = stopped[stop_at..].iter().position(|f| *f != level);
    assert_eq!(still, None, "stopped, the level holds at {level:?}");
    let seq = player.transport(m2_runtime::Cmd::Play).unwrap();
    let played = fill(&mut d, SR as usize);
    let play_at = player.applied(seq).unwrap().clock as usize - lead.len() - stopped.len();
    assert!(played[..play_at].iter().all(|f| *f == level), "held until the play lands");
    let resumed = first_diff(&played[play_at..], &export[pos..pos + played.len() - play_at]);
    assert_eq!(resumed, None, "Play goes on as if it never stopped");
    // A loop of 1.3 beats at 120 bpm: 31 200 frames, so each wrap falls mid-block. Then a seek.
    player.seek(0.5).unwrap();
    player.set_loop(Some((0.0, 1.3))).unwrap();
    let (mut wraps, mut last) = (0, u64::MAX);
    let looped = hear(&mut d, 2 * SR as usize, &mut |_, _| {
        let p = player.status().pos;
        wraps += (p < last && last != u64::MAX) as usize;
        last = p;
    });
    assert!(wraps >= 3, "{wraps} wraps");
    player.set_loop(None).unwrap();
    player.seek(5.1).unwrap();
    let sought = fill(&mut d, SR as usize / 2);
    let all: Vec<[f32; 2]> = [lead, stopped, played, looped, sought].concat();
    assert!(max_step(&all) <= 2.0 * own, "a step of {} against the wobble's own {own}", max_step(&all));
}

// RENDER-25a, step 3 gate (G5): an activation plays on from where the engine is: the position
// read after every device buffer never goes back, and the new renderer is heard within a fade.
#[test]
fn an_activation_never_jumps_back() {
    let song = pad(json!({"clips": [{"start": 0, "length": 32, "notes": [{"pitch": 60, "start": 0, "length": 32}]}]}));
    let r0 = song.renderer();
    let info = r0.info();
    let (player, mut d) = playing(r0, SR);
    let mut last = 0;
    let mut sent = None;
    let mut generation = 0;
    hear(&mut d, 2 * SR as usize, &mut |d, done| {
        let s = player.status();
        assert!(s.pos >= last, "went back from {last} to {}", s.pos);
        last = s.pos;
        if done >= SR as usize && sent.is_none() {
            let r: Renderer = song.after(&song, &info);
            generation = r.generation();
            sent = Some(player.activate(r).unwrap().1);
        }
        let _ = d;
    });
    let applied = player.applied(sent.unwrap()).unwrap();
    assert!(applied.clock >= SR as u64 && applied.clock < SR as u64 + 4096 + 128, "applied at {}", applied.clock);
    assert_eq!(player.status().generation, generation);
    assert!(last >= 2 * SR as u64 - 4096);
}

// RENDER-6a's one exception, RENDER-1 (step 7 design §10.5; owner question Q6, a tape pause): a hold
// fades the output out linearly over 5 ms from the block line after it is sent, then the engine
// processes nothing: silence, its clock and its transport still, which it says (`Player::held`)
// only after the callback that held it. A resume fades back in where it held: the output is that
// of a twin that never held, from where the hold froze it, times the ramp, bit for bit. No step
// across either ramp is larger than twice the music's own (G6's rule).
#[test]
fn a_hold_freezes_and_resumes_without_a_step() {
    let delay = json!([
        {"id": "t", "op": "const", "value": 0.25, "unit": "seconds"},
        {"id": "f", "op": "const", "value": 0.6},
        {"id": "m", "op": "const", "value": 0.5},
        {"id": "x", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v", "time": "t", "feedback": "f", "mix": "m"}},
    ]);
    let notes = json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 69, "start": 0, "length": 4}]}]});
    let song = synth(0.05, delay, "x", notes, at_bpm(120.0));
    let (player, mut d) = playing(song.renderer(), SR);
    let (_twin, mut twin) = playing(song.renderer(), SR);
    // 5 ms at 48 kHz; the engine's block lines are every 128 device frames here.
    const FADE: usize = 240;
    let line = |frames: usize| frames.div_ceil(128) * 128;
    let scaled = |x: [f32; 2], level: usize| {
        let g = level as f32 / FADE as f32;
        [x[0] * g, x[1] * g]
    };
    let mut out = fill(&mut d, 24_000);
    let seq = player.hold(true).unwrap();
    assert!(!player.held(), "not before a callback took it");
    let from = line(out.len());
    out.extend(fill(&mut d, 4_096));
    // The fade's two blocks rendered, then nothing.
    let frozen = from + 2 * 128;
    assert!(player.applied(seq).is_some() && player.held() && engine(&mut d).is_held());
    let (clock, pos) = (player.clock(), player.status().pos);
    assert_eq!((clock, pos), (frozen as u64, frozen as u64), "the clock and the transport stopped once the fade was out");
    out.extend(fill(&mut d, 10_000));
    assert_eq!((player.clock(), player.status().pos), (clock, pos), "nothing moves while held");
    let resumed = line(out.len());
    player.hold(false).unwrap();
    out.extend(fill(&mut d, 24_000));
    assert!(!player.held() && !engine(&mut d).is_held());
    assert_eq!(player.status().pos, (frozen + out.len() - resumed) as u64, "the transport goes on from where it held");

    let want = fill(&mut twin, frozen + 24_000);
    assert_eq!(first_diff(&out[..from], &want[..from]), None, "the same before the hold");
    for k in 0..frozen - from {
        assert_eq!(out[from + k], scaled(want[from + k], FADE.saturating_sub(k + 1)), "fading out, frame {k}");
    }
    assert!(out[frozen..resumed].iter().all(|f| *f == [0.0; 2]), "silent while held");
    for k in 0..FADE {
        assert_eq!(out[resumed + k], scaled(want[frozen + k], k + 1), "fading in, frame {k}");
    }
    let after = out.len() - resumed;
    assert_eq!(first_diff(&out[resumed + FADE..], &want[frozen + FADE..frozen + after]), None, "then the twin's own, where it held");
    let own = max_step(&want[from - 9_600..from]);
    let (down, up) = (max_step(&out[from - 1..frozen + 1]), max_step(&out[resumed - 1..resumed + FADE + 1]));
    assert!(own > 0.01 && down <= 2.0 * own && up <= 2.0 * own, "steps: {down} out, {up} in; the music's own {own}");
}

// RENDER-1, RENDER-28 (step 7 design §10.5: while held, an activation waits for the resume): an
// activation sent while held waits, and the resume after it still applies, then the activation,
// and everything sent after them. Before, the resume queued behind the activation, which waited
// for it: the engine stayed held and silent for good and took no message again, a seek, a stop,
// a panic or a play included, while the player never looked stalled.
#[test]
fn a_resume_never_waits_behind_an_activation_waiting_for_it() {
    let notes = json!([{"pitch": 60, "start": 0, "length": 32}, {"pitch": 64, "start": 3, "length": 4}]);
    let song = pad(json!({"clips": [{"start": 0, "length": 32, "notes": notes}]}));
    let r = song.renderer();
    let info = r.info();
    let (player, mut d) = playing(r, SR);
    fill(&mut d, 4_800);
    player.hold(true).unwrap();
    fill(&mut d, 4_800);
    assert!(player.held());
    let (_, activation) = player.activate(song.after(&song, &info)).unwrap();
    let sent = [activation, player.hold(false).unwrap(), player.seek(2.0).unwrap(), player.transport(m2_runtime::Cmd::Stop).unwrap()];
    fill(&mut d, 10 * SR as usize);
    for seq in sent {
        assert!(player.applied(seq).is_some_and(|a| !a.refused), "message {seq} applied");
    }
    assert!(!player.held() && !engine(&mut d).is_held());
    assert_eq!(engine(&mut d).activations(), 1);
    let s = player.status();
    assert!(!s.playing && s.pending.is_none(), "{s:?}");
    let (panic, _) = (player.panic().unwrap(), player.play());
    // From beat 2: the note at beat 3.
    let out = fill(&mut d, SR as usize);
    assert!(player.applied(panic).is_some() && player.is_playing() && peak(&out) > 0.1, "it plays again");
    assert!(!player.stalled());
}

// RENDER-1, RENDER-9: a seek while held moves the frozen transport, and the status says so at
// once, as it does for a seek while playing: the position is the target from the next callback
// on, the whole hold long, and playback resumes there. Before, the status kept the frozen
// position until the resume, though the seek no longer counted as pending.
#[test]
fn a_seek_while_held_is_where_playback_is() {
    let song = pad(json!({"clips": [{"start": 0, "length": 32, "notes": [{"pitch": 60, "start": 0, "length": 32}]}]}));
    let (player, mut d) = play(&song);
    fill(&mut d, 24_000);
    player.hold(true).unwrap();
    fill(&mut d, 4_800);
    assert!(player.held());
    player.seek(6.0).unwrap();
    assert_eq!(player.status().pending, Some(6.0));
    for _ in 0..2 {
        fill(&mut d, 24_000);
        let s = player.status();
        assert_eq!((s.pending, s.pos, s.beats), (None, 6 * 24_000, 6.0), "while held");
    }
    player.hold(false).unwrap();
    fill(&mut d, 4_800);
    let s = player.status();
    assert!(!player.held() && s.beats > 6.0 && s.beats < 6.3, "resumed there: {s:?}");
}

// RENDER-9: a seek takes effect at the next block line: its target is the position at once, and
// the engine plays from there.
#[test]
fn a_seek_lands_on_the_next_block_line() {
    let song = pad(json!({"clips": [{"start": 0, "length": 32, "notes": [{"pitch": 60, "start": 4, "length": 1}]}]}));
    let (player, mut d) = play(&song);
    fill(&mut d, 1000);
    let seq = player.seek(4.0).unwrap();
    assert_eq!(player.status().pending, Some(4.0));
    fill(&mut d, 1000);
    let a = player.applied(seq).unwrap();
    assert_eq!(a.clock, 1024, "the block line after the 1000 frames heard");
    let ons: Vec<(u64, u64)> =
        trace(&mut d).iter().filter(|e| matches!(e.kind, TraceKind::On { .. })).map(|e| (e.engine, e.transport)).collect();
    assert_eq!(ons, [(1024, 96_000)], "the note at the target starts there");
    assert_eq!(player.status().pending, None);
}

// RENDER-28: a panic in the engine is caught at the callback: it never unwinds into the device.
// What played is retired, the callback plays silence, and opening a project plays again.
#[test]
fn a_panic_in_the_engine_is_caught() {
    if !cfg!(debug_assertions) {
        return;
    }
    let song = pad(json!({"clips": [{"start": 0, "length": 32, "notes": [{"pitch": 60, "start": 0, "length": 32}]}]}));
    let (player, mut d) = play(&song);
    assert!(peak(&fill(&mut d, 4800)) > 0.5);
    // An activation at another rate is a bug (a rate change replaces the engine): debug builds
    // assert, in the engine.
    let wrong = Renderer::new(&song.flat, &song.data, &song.packages, 44_100).unwrap();
    player.activate(wrong).unwrap();
    let out = fill(&mut d, 4800);
    assert!(player.crashed());
    assert!(peak(&out[4800 - 1000..]) == 0.0, "silence after the panic");
    assert!(d.engine().is_none(), "what played was retired");
    player.open(song.renderer()).unwrap();
    player.play();
    assert!(peak(&fill(&mut d, 4800)) > 0.5, "a new project plays");
}

// HOST-6, RENDER-9, RENDER-6a: a song hosting plugins plays live through mid-block wraps (each
// run a plugin call of its own, with its own transport) and a stop, and never idles: a plugin's
// tail can't be seen until plugins report sleep (step 3b). Skipped without Surge XT and
// Airwindows (`m2 plugins`).
#[test]
fn a_song_with_plugins_plays_live_and_never_idles() {
    let song = fixture("examples/with-plugins");
    let missing: Vec<&str> = song
        .flat
        .nodes
        .iter()
        .filter_map(|n| match &n.op {
            m2_ir::FOp::Plugin { plugin, .. } if song.packages.plugin_missing(plugin) => Some(plugin.as_str()),
            _ => None,
        })
        .collect();
    if !missing.is_empty() {
        eprintln!("skipping: {missing:?} not installed");
        return;
    }
    let mut r = song.renderer();
    assert!(r.hosts_plugins() && !r.can_idle());
    r.trace(4096);
    let (player, mut d) = playing(r, SR);
    let first = fill(&mut d, 2 * SR as usize);
    assert!(peak(&first) > 0.01);
    // 2.3 beats from the start: wraps fall mid-block.
    player.seek(0.0).unwrap();
    player.set_loop(Some((0.0, 2.3))).unwrap();
    let looped = fill(&mut d, 3 * SR as usize);
    assert!(peak(&looped[SR as usize..]) > 0.01, "it plays on through the wraps");
    let wraps = trace(&mut d).iter().filter(|e| e.kind == TraceKind::Jump(m2_runtime::Jump::Wrap)).count();
    assert!(wraps >= 2, "{wraps} wraps");
    player.stop();
    fill(&mut d, 3 * SR as usize);
    let e = engine(&mut d);
    assert!(!e.is_idle(), "a renderer with plugins never idles");
    assert_eq!((e.crashes(), e.current().unwrap().held_notes()), (0, 0));
    assert!(!player.crashed());
    // RENDER-10, RENDER-26a: a panic while the lead's chord sounds lets go of its notes in the
    // plugin too, which may keep its voices through a reset: its tail dies away, nothing hangs.
    player.set_loop(None).unwrap();
    player.seek(0.0).unwrap();
    player.play();
    fill(&mut d, SR as usize);
    assert!(engine(&mut d).current().unwrap().held_notes() > 0, "the chord sounds");
    player.panic().unwrap();
    player.stop();
    let tail = fill(&mut d, 8 * SR as usize);
    assert_eq!(engine(&mut d).current().unwrap().held_notes(), 0);
    let last = db(rms(&tail[tail.len() - SR as usize / 5..]));
    assert!(last < -60.0, "a note hangs in the plugin after a panic: {last:.1} dBFS 8 s on");
}

// RENDER-33 and RENDER-9a on p16 (recorded, step 3 gate G2): ten minutes of a 4-bar loop at
// 127 bpm from 1:00. Every lap plays the same notes at the same transport frames, each lap's
// at its wrap plus its place in the loop: no drift. About 3 CPU minutes in release:
// `cargo test -p m2-play --release --test live -- --ignored --nocapture`.
#[test]
#[ignore = "about 3 minutes in release"]
fn p16_loops_ten_minutes_without_drift() {
    let dir = temp("p16-loop");
    std::fs::remove_dir(&dir).unwrap();
    m2_session::reference::write(&dir, &m2_session::reference::Reference::P16).unwrap();
    let song = compile(dir);
    let mut r = song.renderer();
    r.trace(400_000);
    let (player, mut d) = Player::driven(SR);
    player.open(r).unwrap();
    // 1:00 is beat 127; the loop is beats 128-144.
    player.seek(127.0).unwrap();
    player.set_loop(Some((128.0, 144.0))).unwrap();
    player.play();
    let t = std::time::Instant::now();
    let mut data = vec![0.0f32; 2 * 512];
    for _ in 0..(600 * SR as usize / 512) {
        d.fill(&mut data, 2);
        drop(d.drain_retired());
    }
    let took = t.elapsed();
    let (start, end) = (2_902_677u64, 3_265_512u64);
    let lap = end - start;
    let entries = trace(&mut d);
    let wraps: Vec<u64> = entries.iter().filter(|e| e.kind == TraceKind::Jump(m2_runtime::Jump::Wrap)).map(|e| e.engine).collect();
    assert!(wraps.len() >= 78, "{} wraps", wraps.len());
    for (k, w) in wraps.iter().enumerate() {
        assert_eq!(*w, wraps[0] + k as u64 * lap, "wrap {k}");
    }
    let ons = |k: usize| -> Vec<(u64, u64, u32)> {
        let (from, to) = (wraps[k], wraps[k] + lap);
        let mut v: Vec<(u64, u64, u32)> = entries
            .iter()
            .filter(|e| e.engine >= from && e.engine < to)
            .filter_map(|e| match e.kind {
                TraceKind::On { pitch } => Some((e.engine - from, e.transport, pitch.to_bits())),
                _ => None,
            })
            .collect();
        v.sort_unstable();
        v
    };
    let first = ons(0);
    assert!(!first.is_empty());
    for (engine, transport, _) in &first {
        assert_eq!(*engine, transport - start, "an On's engine frame is its wrap plus its place in the loop");
    }
    for k in 1..wraps.len() - 1 {
        assert_eq!(ons(k), first, "lap {k}");
    }
    let e = engine(&mut d);
    assert_eq!((e.resets(), e.activations(), e.current().unwrap().dropped_ons()), (0, 0, 0));
    println!("p16, 10 minutes of a loop: {} laps, {} Ons each, rendered in {took:.1?} ({})", wraps.len(), first.len(), profile());
}

/// Which build measured it.
fn profile() -> &'static str {
    if cfg!(debug_assertions) { "debug assertions on: the dev profile" } else { "release" }
}
