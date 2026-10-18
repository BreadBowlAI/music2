//! The `poly` op's allocator (RENDER-16): `mode`, `retrigger`, `steal` and `glide`, on any voice
//! graph. The voice here is a probe that plays its note fields as sound (left: the pitch while the
//! gate is open, right: the gate), so what these tests hold is the allocator's, whatever the voice.
//! They port std's `notes/tests.rs` (`std:mono`), which they replace.

use std::collections::HashSet;

use super::live::{Rig, at_bpm, with_notes};
use super::*;

/// Frames in a beat at 120 bpm.
const BEAT: usize = 24_000;

/// `t:probe`: inputs `a` and `b`, a stereo output of `a` on the left and `b` on the right.
fn probe_package() -> PathBuf {
    let at = |port: u32| format!("(i32.add (i32.load offset={} (local.get $io)) (i32.shl (local.get $i) (i32.const 2)))", 4 * port);
    let wat = module_v1(&format!(
        r#"(func (export "probe_new") (param i32) (result i32) (i32.const 4096))
           (func (export "probe_ports") (result i32) (i32.const 513))
           (func (export "probe_reset") (param i32))
           (func (export "probe_active") (param i32) (result i32) (i32.const 0))
           (func (export "probe_process") (param $s i32) (param $io i32) (param $i i32) (param $n i32)
             (block $done (loop $l
               (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
               (f32.store {l} (f32.load {a}))
               (f32.store {r} (f32.load {b}))
               (local.set $i (i32.add (local.get $i) (i32.const 1)))
               (br $l))))"#,
        a = at(0),
        b = at(1),
        l = at(2),
        r = at(3),
    ));
    let dir = package_with(&wat, "inputs = [\"a: control<*>\", \"b: control<unit01>\"]\noutputs = [\"out: audio<2>\"]");
    // `package_with` names its kernel `k`.
    let manifest = std::fs::read_to_string(dir.join("m2pkg.toml")).unwrap().replace("name = \"k\"", "name = \"probe\"");
    std::fs::write(dir.join("m2pkg.toml"), manifest).unwrap();
    dir
}

fn packages() -> PackageSet {
    let mut pk = std_packages();
    pk.load(&probe_package()).unwrap();
    pk
}

/// A poly of probes over the notes lane, its options `opts` (`alloc`, `voices`, `glide`), its glide
/// (if any) the cell `gl` of `glide` seconds. The probe's left is the pitch times the gate, or the
/// velocity times the gate when `velocity`.
fn probed(opts: Value, glide: f64, velocity: bool) -> Flat {
    let field = if velocity { "velocity" } else { "pitch" };
    let mut v = json!({"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "o", "nodes": [
        {"id": "f", "op": "lane", "lane": field},
        {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "fg", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "f", "by": "g"}},
        {"id": "o", "op": "kernel", "kernel": "t:probe", "inputs": {"a": "fg", "b": "g"}}]}});
    v.as_object_mut().unwrap().extend(opts.as_object().unwrap().clone());
    let gl = json!({"id": "gl", "op": "const", "value": glide, "unit": "seconds", "origin": {"site": 7, "arg": ["0"]}});
    let ir = song(json!([{"id": "n", "op": "lane", "lane": "notes"}, gl, v]), "v", json!([{"key": "notes", "kind": "events<note>"}]));
    check(&ir, &packages()).unwrap_or_else(|e| panic!("{e:?}"))
}

/// Notes `(pitch, start beat, length in beats, velocity)` at 120 bpm.
fn notes(ns: &[(f64, f64, f64, f64)]) -> DataSet {
    let ns: Vec<Value> = ns.iter().map(|&(p, s, l, v)| json!({"pitch": p, "start": s, "length": l, "velocity": v})).collect();
    with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 16, "notes": ns}]}), at_bpm(120.0))
}

/// `flat` over `data`, offline, for `beats`.
fn play(flat: &Flat, data: &DataSet, beats: f64) -> Vec<[f32; 2]> {
    Renderer::new(flat, data, &packages(), SR).unwrap().render(beats / 2.0).unwrap()
}

/// The frame of beat `b`.
fn at(b: f64) -> usize {
    (b * BEAT as f64).round() as usize
}

fn legato(glide: f64) -> Flat {
    probed(json!({"alloc": {"mode": "legato", "retrigger": false}, "glide": "gl"}), glide, false)
}

/// The probe's pitch over `range`, where its gate is open.
fn pitches(out: &[[f32; 2]], range: std::ops::Range<usize>) -> Vec<f32> {
    out[range].iter().map(|f| f[0]).collect()
}

/// Whether the gate is open on every frame of `range`.
fn open(out: &[[f32; 2]], range: std::ops::Range<usize>) -> bool {
    out[range].iter().all(|f| f[1] == 1.0)
}

// A note that starts while another is held slides to its pitch over the glide, read where the
// slide starts, and the gate stays open: nothing starts again. The slide's first frame plays the
// pitch it leaves, as std's `mono` did, and its last the pitch it reaches.
#[test]
fn legato_slides_without_starting_again() {
    let out = play(&legato(0.01), &notes(&[(60.0, 0.0, 1.0, 0.8), (64.0, 0.5, 1.0, 0.8)]), 2.0);
    let b = at(0.5);
    assert!(open(&out, 0..at(1.5)), "the gate never falls");
    assert_eq!(out[b - 1][0], 60.0);
    assert_eq!(out[b][0], 60.0, "the slide's first frame plays the pitch it leaves");
    assert!((out[b + 240][0] - 62.0).abs() < 1e-3, "halfway at 5 ms: {}", out[b + 240][0]);
    assert_eq!(out[b + 480][0], 64.0, "there after 10 ms (480 frames)");
    assert!(pitches(&out, b..b + 480).windows(2).all(|w| w[1] >= w[0]), "it rises all the way");
    // Without a glide it jumps, still without starting again.
    let out = play(&legato(0.0), &notes(&[(60.0, 0.0, 1.0, 0.8), (64.0, 0.5, 1.0, 0.8)]), 2.0);
    assert!(open(&out, 0..at(1.5)));
    assert_eq!((out[b - 1][0], out[b][0]), (60.0, 64.0));
    // And a legato note brings its own velocity.
    let vel = probed(json!({"alloc": {"mode": "legato", "retrigger": false}}), 0.0, true);
    let out = play(&vel, &notes(&[(60.0, 0.0, 1.0, 0.5), (64.0, 0.5, 1.0, 0.25)]), 2.0);
    assert_eq!((out[b - 1][0], out[b][0]), (0.5, 0.25));
}

// Notes that only touch (one ends on the frame the next starts) start again: no slide, and
// without retrigger the gate falls for that one frame, so envelopes start again from where they
// are. With retrigger, the voice starts fresh instead, its gate open.
#[test]
fn touching_notes_start_again() {
    let data = notes(&[(60.0, 0.0, 1.0, 0.8), (67.0, 1.0, 1.0, 0.8)]);
    let b = at(1.0);
    let out = play(&legato(0.05), &data, 2.5);
    assert!(open(&out, 0..b) && open(&out, b + 1..at(2.0)));
    assert_eq!(out[b][1], 0.0, "the gate falls for one frame");
    assert_eq!(out[b + 1][0], 67.0, "at its own pitch, no slide");
    let fresh = probed(json!({"alloc": {"mode": "legato"}, "glide": "gl"}), 0.05, false);
    let out = play(&fresh, &data, 2.5);
    assert!(open(&out, 0..at(2.0)), "retriggered: a fresh voice, its gate open");
    assert_eq!(out[b][0], 67.0);
}

// When the note sounding ends while an older one is still held, the older one sounds again,
// sliding back to it: an 808's slide up and back.
#[test]
fn a_held_note_sounds_again_when_the_top_one_ends() {
    let out = play(&legato(0.01), &notes(&[(48.0, 0.0, 2.0, 0.8), (60.0, 0.5, 0.5, 0.8)]), 2.5);
    let back = at(1.0);
    assert!(open(&out, 0..at(2.0)), "the gate stays open throughout");
    assert_eq!(out[back - 1][0], 60.0);
    assert_eq!(out[back][0], 60.0, "the slide back starts where the top note was");
    assert!((out[back + 240][0] - 54.0).abs() < 1e-3, "{}", out[back + 240][0]);
    assert_eq!(out[back + 480][0], 48.0, "back at the held note");
    assert_eq!(out[at(1.9)][0], 48.0);
}

// A trill over a held note: each note of it overlaps the held one, so every change slides, both
// ways, and the gate never falls, though the trill's notes only touch each other.
#[test]
fn a_trill_over_a_held_note_glides_each_way() {
    let mut ns = vec![(60.0, 0.0, 3.0, 0.8)];
    for k in 0..8 {
        ns.push((if k % 2 == 0 { 62.0 } else { 64.0 }, 0.5 + 0.25 * k as f64, 0.25, 0.8));
    }
    let out = play(&legato(0.002), &notes(&ns), 3.5);
    assert!(open(&out, 0..at(3.0)), "the gate never falls");
    for k in 0..8 {
        let s = at(0.5 + 0.25 * k as f64);
        let (to, from) = if k % 2 == 0 { (62.0, if k == 0 { 60.0 } else { 64.0 }) } else { (64.0, 62.0) };
        assert_eq!((out[s][0], out[s + 96][0]), (from, to), "note {k} slides from {from} to {to} over 96 frames");
        assert!(out[s + 48][0] != from && out[s + 48][0] != to, "note {k} is halfway");
    }
    // After the trill, back to the held note.
    assert_eq!(out[at(2.5) + 96][0], 60.0);
}

// Mono: every note starts again (the gate falls for a frame where it was open), and with a glide
// every note slides from the one before, overlapping or not.
#[test]
fn mono_starts_every_note_and_glides_from_the_last() {
    let mono = probed(json!({"alloc": {"mode": "mono", "retrigger": false}, "glide": "gl"}), 0.01, false);
    let out = play(&mono, &notes(&[(60.0, 0.0, 1.0, 0.8), (64.0, 0.5, 1.0, 0.8), (67.0, 2.0, 0.5, 0.8)]), 3.0);
    let (b, c) = (at(0.5), at(2.0));
    assert_eq!(out[b][1], 0.0, "an overlapping note starts again: the gate falls for one frame");
    assert!(open(&out, b + 1..at(1.5)));
    assert_eq!((out[b + 1][0], out[b + 480][0]), (60.0 + 4.0 / 480.0, 64.0), "and slides from the last");
    assert_eq!(out[c - 1][1], 0.0, "silence between");
    assert!(open(&out, c..at(2.5)), "a note after silence: the gate opens");
    assert_eq!((out[c][0], out[c + 480][0]), (64.0, 67.0), "it slides from the last note too");
}

// retrigger: false in poly mode: a stolen voice isn't reset; its gate falls for one frame. With
// retrigger (the default) the gate stays open: the voice starts fresh.
#[test]
fn retrigger_false_drops_the_gate_one_sample() {
    let data = notes(&[(60.0, 0.0, 1.0, 0.8), (64.0, 0.5, 1.0, 0.8)]);
    let b = at(0.5);
    let carry = probed(json!({"alloc": {"retrigger": false}}), 0.0, false);
    let out = play(&carry, &data, 2.0);
    assert_eq!((out[b - 1][1], out[b][1], out[b + 1][1]), (1.0, 0.0, 1.0));
    assert_eq!(out[b + 1][0], 64.0);
    let reset = probed(json!({}), 0.0, false);
    let out = play(&reset, &data, 2.0);
    assert!(open(&out, 0..at(1.5)));
}

/// Two held notes in two voices (70, then 40), and a third (50) at beat 0.5, stealing as `steal`
/// says: what sounds at beat 0.75, the voices' pitches summed.
fn third_note(steal: &str) -> f32 {
    let two = probed(json!({"voices": 2, "alloc": {"steal": steal}}), 0.0, false);
    let out = play(&two, &notes(&[(70.0, 0.0, 2.0, 0.8), (40.0, 0.25, 2.0, 0.8), (50.0, 0.5, 2.0, 0.8)]), 1.0);
    out[at(0.75)][0]
}

// steal: quietest takes the voice with the lowest peak over its last block (40 is quieter than
// 70, as the probe plays it); oldest takes the first held.
#[test]
fn steal_quietest_takes_the_quietest() {
    assert_eq!(third_note("oldest"), 40.0 + 50.0);
    assert_eq!(third_note("quietest"), 70.0 + 50.0);
}

// steal: none plays a note only on a free voice: the third isn't played, and its Off finds nothing.
#[test]
fn steal_none_drops_the_note() {
    assert_eq!(third_note("none"), 70.0 + 40.0);
}

/// A rig playing `f` over `data` from 0 for `frames`, with `next` activated at `edit`.
fn edited(f: &Flat, data: &DataSet, frames: usize, edit: usize, next: &Flat) -> (Vec<[f32; 2]>, Rig) {
    let pk = packages();
    let mut r = Renderer::prepare(f, data, &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let mut rig = Rig::of(f, data, r);
    rig.cmd(Cmd::Play);
    let mut out = rig.run(edit);
    let (old, old_data, info) = rig.last.as_ref().unwrap();
    let cont = Continues::named(old, next);
    let restarts = HashSet::new();
    let keep = Keep { old, old_data, info, cont: &cont, restarts: &restarts };
    let mut r = Renderer::prepare(next, data, &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Keep(keep)).unwrap();
    r.set_strict(false);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    out.extend(rig.run(frames - edit));
    (out, rig)
}

/// The cell `gl` set to `v` in `f`, as a knob's turn writes it.
fn with_glide(f: &Flat, v: f64) -> Flat {
    let mut f = f.clone();
    let gl = f.nodes.iter_mut().find(|n| n.name == "song/gl").unwrap();
    gl.op = FOp::Const(v);
    f
}

// glide is a Value: a knob, whose turn is a cell write that keeps the pool, and the next slide
// takes the new time.
#[test]
fn glide_is_a_cell_not_a_new_pool() {
    let f = legato(0.01);
    let data = notes(&[(60.0, 0.0, 2.0, 0.8), (64.0, 0.5, 0.5, 0.8), (67.0, 1.5, 0.25, 0.8)]);
    let (out, rig) = edited(&f, &data, at(2.0), at(1.0), &with_glide(&f, 0.02));
    let s = rig.e.current().unwrap().stats().clone();
    assert_eq!((s.kept.polys, s.fresh.total(), s.ghosts), (1, 0, 0), "{s:?}");
    let (b, c) = (at(0.5), at(1.5));
    assert_eq!(out[b + 480][0], 64.0, "before the turn: 10 ms");
    assert!(out[c + 480][0] < 67.0 && out[c + 960][0] == 67.0, "after it: 20 ms");
}

// A legato pool moves whole into a plan of the same shape, its held notes and its slide with it:
// the activation changes no bit. Another mode is another shape: a new pool.
#[test]
fn a_mono_pool_moves_whole_across_an_activation() {
    let f = legato(0.01);
    let data = notes(&[(48.0, 0.0, 2.0, 0.8), (60.0, 0.5, 0.5, 0.8)]);
    let frames = at(2.5);
    let plain = play(&f, &data, 2.5);
    // Mid-slide, with the top note held over the held one: the return to it needs the memory.
    let (out, rig) = edited(&f, &data, frames, at(0.5) + 200, &f);
    let s = rig.e.current().unwrap().stats().clone();
    assert_eq!(s.kept, Count { polys: 1, ..Count::default() }, "{s:?}");
    assert!(out == plain, "the activation changed no bit");
    assert_eq!(out[at(1.0) + 480][0], 48.0, "back to the held note");
    let mono = probed(json!({"alloc": {"mode": "mono", "retrigger": false}, "glide": "gl"}), 0.01, false);
    let (_, rig) = edited(&f, &data, frames, at(0.75), &mono);
    let s = rig.e.current().unwrap().stats().clone();
    assert_eq!(s.fresh, Count { polys: 1, ..Count::default() }, "{s:?}");
}
