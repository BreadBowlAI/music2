//! The `poly` op's allocator (RENDER-16): `mode`, `retrigger`, `steal` and `glide`, on any voice
//! graph. The voice here is a probe that plays its note fields as sound (left: the pitch while the
//! gate is open, right: the gate), so what these tests hold is the allocator's, whatever the voice.
//! They port std's `notes/tests.rs` (`std:mono`), which they replace. And the lane's wheels
//! (RENDER-14's `Midi`): the bend on every voice's pitch, the mod wheel a note field.

use super::live::{Rig, at_bpm, with_notes};
use super::*;

/// Frames in a beat at 120 bpm.
const BEAT: usize = 24_000;

/// `@test/t:probe`: inputs `a` and `b`, a stereo output of `a` on the left and `b` on the right.
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
    probed_field(opts, glide, if velocity { "velocity" } else { "pitch" })
}

/// [`probed`], the probe's left `field` times the gate.
fn probed_field(opts: Value, glide: f64, field: &str) -> Flat {
    let mut v = json!({"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "o", "nodes": [
        {"id": "f", "op": "lane", "lane": field},
        {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "fg", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "f", "by": "g"}},
        {"id": "o", "op": "kernel", "kernel": "@test/t:probe", "inputs": {"a": "fg", "b": "g"}}]}});
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

/// A live engine playing `f` over `data` from 0, and its renderer's generation.
fn live_rig(f: &Flat, data: &DataSet) -> (Rig, u64) {
    let mut r =
        Renderer::prepare(f, data, &packages(), SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let g = r.generation();
    let mut rig = Rig::of(f, data, r);
    rig.cmd(Cmd::Play);
    (rig, g)
}

/// `next` activated in `rig`, prepared against what it plays: a pool of the same shape moves whole.
fn activate(rig: &mut Rig, next: &Flat, data: &DataSet) {
    let r = rig.prepare(next, data, &packages());
    rig.send(Msg::Activate { renderer: Box::new(r) });
}

/// A rig playing `f` over `data` from 0 for `frames`, with `next` activated at `edit`.
fn edited(f: &Flat, data: &DataSet, frames: usize, edit: usize, next: &Flat) -> (Vec<[f32; 2]>, Rig) {
    let (mut rig, _) = live_rig(f, data);
    let mut out = rig.run(edit);
    activate(&mut rig, next, data);
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

/// A wheel moved on the notes lane of the plan `g`, by source 1.
fn wheel(g: u64, bytes: [u8; 3]) -> Msg {
    Msg::Live { generation: g, slot: 0, ev: LiveEv::Midi { source: 1, bytes } }
}

/// Whether the probe's left is `v` on every frame of `out`.
fn all(out: &[[f32; 2]], v: f32) -> bool {
    out.iter().all(|f| f[0] == v)
}

// RENDER-14, D8: a live pitch bend moves the pitch of every voice its lane plays, 2 semitones at
// either end, on any channel, from the next block line; centred, each pitch is its note's again.
// A legato pool's pitch, slides and all, hears it the same way.
#[test]
fn a_bend_reaches_every_voice_in_its_lane() {
    let two = probed(json!({"voices": 2}), 0.0, false);
    let (mut rig, g) = live_rig(&two, &notes(&[(60.0, 0.0, 4.0, 0.8), (64.0, 0.0, 4.0, 0.8)]));
    assert!(all(&rig.run(8 * BLOCK)[BLOCK..], 124.0), "two voices: 60 and 64");
    rig.send(wheel(g, [0xE0, 0x7F, 0x7F]));
    assert!(all(&rig.run(8 * BLOCK), 128.0), "each 2 semitones up, from the block line");
    rig.send(wheel(g, [0xE5, 0, 0]));
    assert!(all(&rig.run(8 * BLOCK), 120.0), "and down, on channel 6");
    rig.send(wheel(g, [0xE0, 0, 0x40]));
    assert!(all(&rig.run(8 * BLOCK), 124.0), "centred");
    // Legato, through a slide: each frame the pitch it plays unbent, plus 2.
    let data = notes(&[(60.0, 0.0, 1.0, 0.8), (64.0, 0.5, 1.0, 0.8)]);
    let plain = play(&legato(0.01), &data, 2.0);
    let (mut rig, g) = live_rig(&legato(0.01), &data);
    rig.send(wheel(g, [0xE0, 0x7F, 0x7F]));
    let bent = rig.run(at(2.0));
    let want: Vec<f32> = plain.iter().map(|f| if f[1] == 1.0 { f[0] + 2.0 } else { 0.0 }).collect();
    assert!(bent.iter().map(|f| f[0]).eq(want), "the pool's pitch, bent");
    assert!(plain[at(0.5)..at(0.5) + 480].windows(2).any(|w| w[0][0] != w[1][0]), "it slides");
}

// RENDER-14, D8: the mod wheel is a note field, 0 to 1, the same in every voice of its lane, from
// the next block line and on any channel; CC 121 (reset all controllers) puts it back at 0. In a
// poly of voices and in a legato pool.
#[test]
fn the_mod_wheel_is_a_note_field() {
    // Two notes: in a poly, two voices, each hearing the wheel; legato, one.
    for (opts, voices) in [(json!({"voices": 2}), 2.0), (json!({"alloc": {"mode": "legato", "retrigger": false}}), 1.0)] {
        let f = probed_field(opts, 0.0, "modWheel");
        let (mut rig, g) = live_rig(&f, &notes(&[(60.0, 0.0, 8.0, 0.8), (64.0, 0.0, 8.0, 0.8)]));
        assert!(all(&rig.run(4 * BLOCK), 0.0), "at rest");
        rig.send(wheel(g, [0xB0, 1, 127]));
        assert!(all(&rig.run(4 * BLOCK), voices), "all the way up, in each voice");
        rig.send(wheel(g, [0xB3, 1, 64]));
        assert!(all(&rig.run(4 * BLOCK), voices * (64.0 / 127.0)), "halfway, on channel 4");
        rig.send(wheel(g, [0xB0, 121, 0]));
        assert!(all(&rig.run(4 * BLOCK), 0.0), "reset");
    }
}

// RENDER-14, RENDER-16, RENDER-10: a lane's wheels live in its pool, as its notes do. A pool moved
// whole into the next plan keeps the bend; a new pool hears the lane's bend at its first block; a
// panic resets the pool, and the lane is back at rest.
#[test]
fn bend_and_wheel_reset_with_the_pool() {
    let f = legato(0.0);
    let data = notes(&[(60.0, 0.0, 1.0, 0.8), (60.0, 1.25, 1.0, 0.8), (60.0, 2.5, 1.0, 0.8)]);
    let (mut rig, g) = live_rig(&f, &data);
    rig.send(wheel(g, [0xE0, 0x7F, 0x7F]));
    rig.send(wheel(g, [0xB0, 1, 127]));
    assert!(all(&rig.run(at(0.5)), 62.0));
    assert_eq!(rig.e.status().wheels, 1, "one lane's wheels are away from rest");
    activate(&mut rig, &f, &data);
    let out = rig.run(at(0.5));
    assert_eq!(rig.e.current().unwrap().stats().kept.polys, 1);
    assert!(all(&out, 62.0), "kept with the pool");
    let mono = probed(json!({"alloc": {"mode": "mono", "retrigger": false}}), 0.0, false);
    activate(&mut rig, &mono, &data);
    let out = rig.run(at(1.0));
    assert_eq!(rig.e.current().unwrap().stats().fresh.polys, 1, "a new pool");
    assert!(all(&out[out.len() - at(0.5)..], 62.0), "its next note hears the lane's bend");
    rig.send(Msg::Panic);
    let out = rig.run(at(1.0));
    assert_eq!(rig.e.status().wheels, 0, "the lane is at rest");
    assert!(all(&out[out.len() - at(0.5)..], 60.0), "and so is the pool");
}

/// A poly over the notes lane, its options `opts`, whose voice plays its note's `keyCount` while its
/// gate is open (mono: one voice plays the sum of its notes').
fn counted(opts: Value) -> Flat {
    let mut v = json!({"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "o", "nodes": [
        {"id": "k", "op": "lane", "lane": "keyCount"},
        {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "o", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "k", "by": "g"}}]}});
    v.as_object_mut().unwrap().extend(opts.as_object().unwrap().clone());
    let ir = song(json!([{"id": "n", "op": "lane", "lane": "notes"}, v]), "v", json!([{"key": "notes", "kind": "events<note>"}]));
    check(&ir, &packages()).unwrap_or_else(|e| panic!("{e:?}"))
}

/// What plays in the middle of each beat `beats` names.
fn mids(out: &[[f32; 2]], beats: &[f64]) -> Vec<f32> {
    beats.iter().map(|&b| out[at(b)][0]).collect()
}

// RENDER-39: a note's `keyCount` is how many notes have started on its key in the lane, itself
// included: notes that alternate keys each count on their own key, a chord's notes each on theirs,
// and notes that start together on one key once. Counted from the lane's notes, whether a voice
// takes the note or not; and in a one-note pool too.
#[test]
fn a_note_reads_its_keys_count() {
    // Kick, snare, kick, snare, kick: 1, 1, 2, 2, 3. Then a chord of 36, 38, 40 and 40 again.
    let mut ns: Vec<(f64, f64, f64, f64)> =
        [36.0, 38.0, 36.0, 38.0, 36.0].iter().enumerate().map(|(i, &p)| (p, i as f64 * 0.5, 0.25, 0.8)).collect();
    ns.extend([36.0, 38.0, 40.0, 40.0].map(|p| (p, 3.0, 0.25, 0.8)));
    let out = play(&counted(json!({"voices": 4})), &notes(&ns), 4.0);
    assert_eq!(mids(&out, &[0.1, 0.6, 1.1, 1.6, 2.1]), [1.0, 1.0, 2.0, 2.0, 3.0]);
    assert_eq!(out[at(3.1)][0], 4.0 + 3.0 + 1.0 + 1.0, "36's fourth, 38's third, and 40's first, twice");
    // One voice that takes no note while it plays one: the note it refused still counts.
    let none = counted(json!({"alloc": {"steal": "none"}}));
    let out = play(&none, &notes(&[(36.0, 0.0, 1.0, 0.8), (36.0, 0.5, 0.25, 0.8), (36.0, 1.5, 0.25, 0.8)]), 2.0);
    assert_eq!(mids(&out, &[0.6, 1.6]), [1.0, 3.0]);
    // Legato: the one voice plays its newest note's count.
    let mono = counted(json!({"alloc": {"mode": "legato", "retrigger": false}}));
    let out = play(&mono, &notes(&[(36.0, 0.0, 1.0, 0.8), (38.0, 0.5, 1.0, 0.8), (36.0, 1.25, 0.5, 0.8)]), 2.0);
    assert_eq!(mids(&out, &[0.25, 0.75, 1.5]), [1.0, 1.0, 2.0]);
}

// RENDER-39, RENDER-33: the count is the pool's, from the notes alone: live from 0 it plays the
// export's counts, however long the engine ran first; a jump never resets it (back to the start,
// the next kick is the fourth); a pool moved whole keeps it; a panic resets it.
#[test]
fn the_key_count_is_the_pools() {
    let f = counted(json!({"voices": 2}));
    let data = notes(&[(36.0, 0.0, 0.25, 0.8), (36.0, 0.5, 0.25, 0.8), (36.0, 1.0, 0.25, 0.8)]);
    let export = play(&f, &data, 1.5);
    let (mut rig, _) = live_rig(&f, &data);
    rig.cmd(Cmd::Stop);
    rig.run(10 * BLOCK);
    rig.cmd(Cmd::Seek(0.0));
    rig.cmd(Cmd::Play);
    let live = rig.run(at(1.5));
    assert_eq!(mids(&live, &[0.1, 0.6, 1.1]), mids(&export, &[0.1, 0.6, 1.1]), "live as the export");
    rig.cmd(Cmd::Seek(0.0));
    assert_eq!(mids(&rig.run(at(1.5)), &[0.1, 0.6, 1.1]), [4.0, 5.0, 6.0], "a jump carries on");
    activate(&mut rig, &f, &data);
    rig.cmd(Cmd::Seek(0.0));
    let out = rig.run(at(1.5));
    assert_eq!(rig.e.current().unwrap().stats().kept.polys, 1);
    assert_eq!(mids(&out, &[0.1, 0.6, 1.1]), [7.0, 8.0, 9.0], "kept with the pool");
    rig.send(Msg::Panic);
    rig.cmd(Cmd::Seek(0.0));
    rig.cmd(Cmd::Play);
    assert_eq!(mids(&rig.run(at(1.5)), &[0.1, 0.6, 1.1]), [1.0, 2.0, 3.0], "a panic resets it");
}

/// The sustain pedal on the notes lane of the plan `g`, at `v`, on channel 1 (RENDER-35).
fn pedal(g: u64, v: u8) -> Msg {
    wheel(g, [0xB0, 64, v])
}

// RENDER-38: while the lane's pedal is down, a voice holds its note after its Off comes, until the
// pedal lifts: at its block line, both notes held end at once. A note let go while it is up ends at
// its Off, as always.
#[test]
fn the_pedal_holds_notes_until_it_lifts() {
    let f = probed(json!({"voices": 2}), 0.0, false);
    let data = notes(&[(60.0, 0.0, 0.25, 0.8), (64.0, 0.125, 0.25, 0.8), (67.0, 2.0, 0.25, 0.8)]);
    let (mut rig, g) = live_rig(&f, &data);
    rig.send(pedal(g, 127));
    let held = rig.run(150 * BLOCK);
    assert!(all(&held[at(0.125)..], 124.0), "both held past their Offs at 0.25 and 0.375");
    rig.send(pedal(g, 0));
    assert!(all(&rig.run(4 * BLOCK), 0.0), "the pedal up ends both, from its block line");
    let free = rig.run(at(2.5) - 154 * BLOCK);
    assert!(all(&free[at(2.0) - 154 * BLOCK..at(2.25) - 154 * BLOCK], 67.0) && all(&free[at(2.25) - 154 * BLOCK..], 0.0), "up: at its Off");
}

// RENDER-38: a key struck again under the pedal ends the note held on it where the new one starts:
// one note sounds, not two; another key's held note rings on.
#[test]
fn a_key_struck_again_under_the_pedal_ends_the_held_one() {
    let f = probed(json!({"voices": 4}), 0.0, false);
    let data = notes(&[(60.0, 0.0, 0.25, 0.8), (48.0, 0.0, 0.25, 0.8), (60.0, 0.5, 0.25, 0.8)]);
    let (mut rig, g) = live_rig(&f, &data);
    rig.send(pedal(g, 127));
    let out = rig.run(at(1.0));
    assert!(all(&out[at(0.25)..at(0.5)], 108.0), "both held");
    assert!(all(&out[at(0.5)..], 108.0), "60 struck again: still 60 and 48, not 60 twice");
}

// RENDER-38: in a legato pool the notes let go under the pedal stay in its note memory: the newest
// sounds on, slid to, until the pedal lifts and the gate falls.
#[test]
fn a_mono_pool_holds_under_the_pedal() {
    let data = notes(&[(60.0, 0.0, 0.25, 0.8), (64.0, 0.5, 0.25, 0.8)]);
    let (mut rig, g) = live_rig(&legato(0.0), &data);
    rig.send(pedal(g, 127));
    let out = rig.run(250 * BLOCK);
    assert!(open(&out, 1..250 * BLOCK), "the gate never falls");
    assert_eq!((out[at(0.4)][0], out[at(0.6)][0], out[at(1.2)][0]), (60.0, 64.0, 64.0), "60 held, then 64, held");
    rig.send(pedal(g, 0));
    assert!(all(&rig.run(4 * BLOCK), 0.0), "the pedal up lets both go");
}

// RENDER-36, RENDER-38: a Stop lets the pedal up, so the notes it held end with the song's; a loop
// going round leaves it down, so a note it holds rings on over the wrap; a panic lets it up too.
#[test]
fn a_stop_or_a_panic_lets_the_pedal_up() {
    let f = probed(json!({"voices": 2}), 0.0, false);
    let data = notes(&[(60.0, 0.0, 0.25, 0.8)]);
    let (mut rig, g) = live_rig(&f, &data);
    rig.send(pedal(g, 127));
    assert!(all(&rig.run(150 * BLOCK)[at(0.125)..], 60.0), "held");
    rig.cmd(Cmd::Stop);
    assert!(all(&rig.run(4 * BLOCK), 0.0), "Stop lets it up");
    rig.cmd(Cmd::Seek(0.0));
    rig.cmd(Cmd::Loop(Some((0.0, 1.0))));
    rig.cmd(Cmd::Play);
    rig.send(pedal(g, 127));
    let looped = rig.run(282 * BLOCK);
    assert!(all(&looped[at(0.5)..at(1.5)], 60.0), "a wrap leaves it down: the note rings on into the next lap");
    rig.send(Msg::Panic);
    rig.cmd(Cmd::Loop(None));
    rig.cmd(Cmd::Seek(0.5));
    assert!(all(&rig.run(4 * BLOCK), 0.0), "a panic lets it up");
}

// RENDER-38 (review E4): a note whose key is still down when the pedal lifts sounds on until its own
// Off: the lift ends only the notes the pedal holds.
#[test]
fn a_key_still_down_at_the_lift_sounds_on() {
    let f = probed(json!({"voices": 2}), 0.0, false);
    let data = notes(&[(60.0, 0.0, 2.0, 0.8)]);
    let (mut rig, g) = live_rig(&f, &data);
    rig.send(pedal(g, 127));
    rig.run(40 * BLOCK);
    rig.send(pedal(g, 0));
    let out = rig.run(at(2.5) - 40 * BLOCK);
    let from = |b: f64| at(b) - 40 * BLOCK;
    assert!(all(&out[..from(2.0)], 60.0), "its key is down: it sounds on past the lift");
    assert!(all(&out[from(2.0)..], 0.0), "then ends at its Off");
}

// RENDER-38 (review E4): a note struck and let go within one block under the pedal rings on: the
// notes that start in a block are held by the pedal as those already in voices are.
#[test]
fn a_note_struck_and_let_go_in_one_block_rings_on() {
    let f = probed(json!({"voices": 2}), 0.0, false);
    // From frame 24 to frame 72 of the third block.
    let s = (2.0 * BLOCK as f64 + 24.0) / BEAT as f64;
    let data = notes(&[(60.0, s, 48.0 / BEAT as f64, 0.8)]);
    let (mut rig, g) = live_rig(&f, &data);
    rig.send(pedal(g, 127));
    let out = rig.run(at(0.5));
    assert!(all(&out[2 * BLOCK + 24..], 60.0), "held past its Off at frame 72 of its block");
}

// RENDER-38, RENDER-16: a pool moved whole while the pedal is down keeps the notes it holds, and lets
// them go when the pedal lifts.
#[test]
fn a_kept_pool_keeps_its_pedal() {
    let f = probed(json!({"voices": 2}), 0.0, false);
    let data = notes(&[(60.0, 0.0, 0.25, 0.8)]);
    let (mut rig, g) = live_rig(&f, &data);
    rig.send(pedal(g, 127));
    rig.run(94 * BLOCK);
    activate(&mut rig, &f, &data);
    let held = rig.run(94 * BLOCK);
    assert_eq!(rig.e.current().unwrap().stats().kept.polys, 1);
    assert!(all(&held, 60.0), "held across the activation");
    rig.send(pedal(rig.e.current().unwrap().generation(), 0));
    assert!(all(&rig.run(4 * BLOCK), 0.0), "the lift ends it");
}

// RENDER-38, RENDER-35: a pool made new (its shape changed) while the pedal is down hears the pedal
// again, as a new consumer hears its lane's wheels: a note let go after the rebuild is held.
#[test]
fn a_new_pool_hears_the_pedal() {
    let (two, three) = (probed(json!({"voices": 2}), 0.0, false), probed(json!({"voices": 3}), 0.0, false));
    let data = notes(&[(64.0, 1.0, 0.25, 0.8)]);
    let (mut rig, g) = live_rig(&two, &data);
    rig.send(pedal(g, 127));
    rig.run(94 * BLOCK);
    activate(&mut rig, &three, &data);
    let out = rig.run(at(2.0) - 94 * BLOCK);
    assert_eq!(rig.e.current().unwrap().stats().kept.polys, 0, "a new pool");
    assert!(all(&out[at(1.0) - 94 * BLOCK..], 64.0), "held past its Off at 1.25");
}

// RENDER-38, RENDER-16 (review E7): when the pedal has filled the voices, a new note takes the voice
// of a note only the pedal holds before one whose key is still down. Two voices: a bass note held
// down, a note let go under the pedal, then a third note: the bass sounds on.
#[test]
fn the_pedal_gives_up_its_own_notes_first() {
    for steal in ["oldest", "quietest"] {
        let f = probed(json!({"voices": 2, "alloc": {"steal": steal}}), 0.0, false);
        let data = notes(&[(36.0, 0.0, 4.0, 0.8), (60.0, 0.5, 0.25, 0.8), (64.0, 1.0, 0.25, 0.8)]);
        let (mut rig, g) = live_rig(&f, &data);
        rig.send(pedal(g, 127));
        let out = rig.run(at(1.5));
        assert_eq!(out[at(1.25)][0], 36.0 + 64.0, "{steal}: the held-down bass and the new note, not the pedal's");
    }
}

// RENDER-38 (review E2): an Off is never moved later: a lift ends a held note at the lift only if it
// hasn't ended before (a release at an activation), so its gate never opens again.
#[test]
fn a_lift_never_reopens_an_ended_note() {
    let mut n = VoiceNote { id: 7, pitch: 60.0, velocity: 0.8, on: 0, off: 1_000, count: 1, held: true };
    let lift = [Event::Midi { frame: 0, bytes: [0xB0, 64, 127] }, Event::Midi { frame: 10, bytes: [0xB0, 64, 0] }];
    crate::poly::hold(&lift, true, 5_000, &mut |f: &mut dyn FnMut(&mut VoiceNote)| f(&mut n));
    assert_eq!((n.off, n.held), (1_000, false));
}
