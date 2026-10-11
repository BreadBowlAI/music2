use std::path::{Path, PathBuf};

use m2_ir::{Ir, check};
use m2_song::Timeline;
use serde_json::{Value, json};

use super::*;

const SR: u32 = 48_000;

fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).expect("std loads");
    p
}

fn song(nodes: Value, out: &str, lanes: Value) -> Ir {
    serde_json::from_value(json!({"ir": 0, "root": {"module": "song", "export": "out"},
        "modules": [{"name": "song", "lanes": lanes, "nodes": nodes, "exports": {"out": out}}]}))
    .unwrap()
}

/// A timeline folder with the given files, in a fresh temp dir.
fn timeline(files: &[(&str, Value)]) -> (Timeline, PathBuf) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("m2-runtime-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, v) in files {
        std::fs::write(dir.join(name), v.to_string()).unwrap();
    }
    (Timeline::load(&dir).unwrap(), dir)
}

fn render(ir: &Ir, tl: &Timeline, seconds: f64) -> Vec<[f32; 2]> {
    let pk = std_packages();
    let flat = check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    Renderer::new(&flat, tl, &pk, SR).unwrap().render(seconds).unwrap()
}

fn peak(x: &[[f32; 2]]) -> f32 {
    x.iter().fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

#[test]
fn sine_has_the_right_pitch() {
    let ir = song(
        json!([
            {"id": "f", "op": "const", "value": 440, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
        ]),
        "o",
        json!([]),
    );
    let out = render(&ir, &Timeline::default(), 1.0);
    assert_eq!(out.len(), SR as usize);
    let crossings = out.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count();
    assert!((439..=441).contains(&crossings), "{crossings} upward zero crossings");
    assert!((peak(&out) - 1.0).abs() < 0.01);
    // Mono output is copied to both channels.
    assert!(out.iter().all(|f| f[0] == f[1]));
}

#[test]
fn poly_plays_notes_from_the_timeline_and_is_deterministic() {
    let lanes = json!([{"id": "notes", "kind": "events<note>"}, {"id": "cutoff", "kind": "control<hz>", "default": 2000}]);
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "c", "op": "lane", "lane": "cutoff"},
            {"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                {"id": "f", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "o", "cutoff": "c"}},
                {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g", "release": "r"}},
                {"id": "r", "op": "const", "value": 0.05, "unit": "seconds"},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "f.lp", "by": "e"}}]}},
        ]),
        "v",
        lanes,
    );
    // 120 bpm: one beat is half a second. A C major chord on beat 0 for one beat, then silence.
    let (tl, dir) = timeline(&[
        ("song.json", json!({"tempo": [{"beat": 0, "bpm": 120}]})),
        ("song.notes.json", json!({"clips": [{"start": 0, "length": 4, "notes": [
            {"pitch": "C4", "start": 0, "length": 1}, {"pitch": "E4", "start": 0, "length": 1}, {"pitch": "G4", "start": 0, "length": 1}]}]})),
    ]);
    let a = render(&ir, &tl, 2.0);
    let b = render(&ir, &tl, 2.0);
    assert!(a == b, "two renders differ");
    let half = SR as usize / 2;
    assert!(peak(&a[..half]) > 0.3, "chord is audible");
    assert!(peak(&a[half + SR as usize / 4..]) < 1e-3, "released and silent after the note");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn voice_stealing_keeps_playing() {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]}},
        ]),
        "v",
        json!([{"id": "notes", "kind": "events<note>"}]),
    );
    let (tl, dir) = timeline(&[("song.notes.json", json!({"clips": [{"start": 0, "length": 8, "notes": [
        {"pitch": 60, "start": 0, "length": 4}, {"pitch": 72, "start": 1, "length": 1}]}]}))]);
    // Default tempo 120: the second note (beat 1..2) steals the only voice.
    let out = render(&ir, &tl, 1.0);
    let window = &out[SR as usize / 2 + 1000..SR as usize - 1000];
    let crossings = window.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count() as f32;
    let hz = crossings / (window.len() as f32 / SR as f32);
    assert!((hz - 523.25).abs() < 10.0, "the newer note (C5) plays, got {hz} Hz");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn control_lanes_follow_automation() {
    let ir = song(
        json!([
            {"id": "l", "op": "lane", "lane": "level"},
            {"id": "o", "op": "kernel", "kernel": "std:sine"},
            {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "l"}},
        ]),
        "a",
        json!([{"id": "level", "kind": "control<gain>", "default": 0}]),
    );
    let (tl, dir) = timeline(&[("song.level.json", json!({"clips": [{"start": 0, "length": 2, "points": [[0, 0], [2, 1]]}]}))]);
    let out = render(&ir, &tl, 1.5);
    let q = SR as usize / 4;
    assert!(peak(&out[..q]) < 0.3);
    assert!(peak(&out[3 * q..4 * q]) > 0.7);
    assert!(peak(&out[5 * q..]) == 0.0, "after the clip the lane is back at its default (0)");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn feedback_delays_one_block() {
    // out = noise + 0.5 × (out one block ago): stays bounded and differs from plain noise.
    let ir = song(
        json!([
            {"id": "x", "op": "kernel", "kernel": "std:noise"},
            {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "sum"},
            {"id": "half", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "x", "b": "half"}},
        ]),
        "sum",
        json!([]),
    );
    let out = render(&ir, &Timeline::default(), 0.5);
    assert!(peak(&out) < 2.5 && peak(&out) > 1.0);
}

#[test]
fn std_manifest_matches_its_kernels() {
    // Instantiating every kernel checks its port counts against the manifest.
    let pk = std_packages();
    let mut store = Store::new(&pk.engine, StoreLimitsBuilder::new().build());
    let mut ids: Vec<String> = pk.kernel_sigs().map(|s| s.id.clone()).collect();
    ids.sort();
    assert!(ids.len() >= 20, "{ids:?}");
    for id in ids {
        let sig = m2_ir::Kernels::kernel(&pk, &id).unwrap().clone();
        let inputs: Vec<Input> = sig.inputs.iter().map(|_| Input::Const(0.0)).collect();
        let node = FNode {
            name: id.clone(),
            module: "t".into(),
            src: None,
            origin: None,
            op: FOp::Const(0.0),
            outputs: sig.outputs.iter().map(|_| Kind::Audio(1)).collect(),
        };
        let tempo = TempoMap::constant(120.0);
        let tl = Timeline::default();
        let mut b = Builder { store: &mut store, packages: &pk, timeline: &tl, tempo: &tempo, sample_rate: 48_000.0, assets: None, audio: Default::default() };
        b.kernel(&node, &id, 1, &inputs).unwrap_or_else(|e| panic!("{e:#}"));
    }
}

// Regression tests for bugs reported in review (2026-10-11).

#[test]
fn output_does_not_depend_on_the_callers_buffer_size() {
    let ir = song(
        json!([
            {"id": "x", "op": "kernel", "kernel": "std:noise"},
            {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "sum"},
            {"id": "half", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "x", "b": "half"}},
        ]),
        "sum",
        json!([]),
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let mut one = vec![[0.0; 2]; 512];
    Renderer::new(&flat, &Timeline::default(), &pk, SR).unwrap().process(&mut one).unwrap();
    let mut r = Renderer::new(&flat, &Timeline::default(), &pk, SR).unwrap();
    let mut pieces = vec![[0.0; 2]; 512];
    for chunk in pieces.chunks_mut(64) {
        r.process(chunk).unwrap();
    }
    let first_diff = one.iter().zip(&pieces).position(|(a, b)| a != b);
    assert_eq!(first_diff, None, "renders differ from frame {first_diff:?}");
}

#[test]
fn a_stolen_voice_plays_until_the_new_note_starts() {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]}},
        ]),
        "v",
        json!([{"id": "notes", "kind": "events<note>"}]),
    );
    // 120 bpm at 48 kHz: 96 samples is 0.004 beats.
    let (tl, dir) = timeline(&[("song.notes.json", json!({"clips": [{"start": 0, "length": 1, "notes": [
        {"pitch": 69, "start": 0, "length": 1}, {"pitch": 81, "start": 0.004, "length": 0.5}]}]}))]);
    let out = render(&ir, &tl, 0.01);
    assert!(peak(&out[1..96]) > 0.0, "the first note is silent before the second one starts");
    std::fs::remove_dir_all(dir).ok();
}

/// A voice with a delay and a feedback loop inside, notes at odd sample positions,
/// and a stolen voice: everything that depends on how a block is split.
fn tricky() -> (Ir, Timeline, PathBuf) {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "mix", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}},
                {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "mix"},
                {"id": "q", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
                {"id": "h", "op": "const", "value": 0.3},
                {"id": "mix", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "q"}}]}},
            {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v"}},
        ]),
        "d",
        json!([{"id": "notes", "kind": "events<note>"}]),
    );
    let (tl, dir) = timeline(&[("song.notes.json", json!({"clips": [{"start": 0, "length": 4, "notes": [
        {"pitch": 57, "start": 0.0013, "length": 0.3}, {"pitch": 64, "start": 0.0071, "length": 0.2},
        {"pitch": 69, "start": 0.0109, "length": 0.5}, {"pitch": 60, "start": 0.61, "length": 0.05}]}]}))]);
    (ir, tl, dir)
}

#[test]
fn every_buffer_size_gives_the_same_samples() {
    let (ir, tl, dir) = tricky();
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let total = 24_000;
    let mut reference = vec![[0.0; 2]; total];
    Renderer::new(&flat, &tl, &pk, SR).unwrap().process(&mut reference).unwrap();
    assert!(peak(&reference) > 0.1);
    for size in [1, 7, 64, 100, 129, 1000] {
        let mut r = Renderer::new(&flat, &tl, &pk, SR).unwrap();
        let mut out = vec![[0.0; 2]; total];
        for chunk in out.chunks_mut(size) {
            r.process(chunk).unwrap();
        }
        let diff = reference.iter().zip(&out).position(|(a, b)| a != b);
        assert_eq!(diff, None, "buffer size {size} differs from frame {diff:?}");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn seeking_gives_the_same_samples_as_a_full_render() {
    let (ir, tl, dir) = tricky();
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let mut r = Renderer::new(&flat, &tl, &pk, SR).unwrap();
    let full = r.render(1.0).unwrap();
    for from in [0u64, 1, 5_000, 12_345] {
        r.seek(from).unwrap();
        assert_eq!(r.position(), from);
        let mut part = vec![[0.0; 2]; 6_000];
        r.process(&mut part).unwrap();
        assert!(part[..] == full[from as usize..from as usize + 6_000], "seek to {from} differs");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn echoes_inside_a_voice_outlive_its_note() {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "d", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}},
                {"id": "t", "op": "const", "value": 0.25, "unit": "seconds"},
                {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "a", "time": "t", "mix": "m"}},
                {"id": "m", "op": "const", "value": 0.5}]}},
        ]),
        "v",
        json!([{"id": "notes", "kind": "events<note>"}]),
    );
    // A 50 ms note at 120 bpm (0.1 beats); the echo comes 250 ms after it starts.
    let (tl, dir) = timeline(&[("song.notes.json", json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 69, "start": 0, "length": 0.1}]}]}))]);
    let out = render(&ir, &tl, 1.0);
    let at = |s: f64| (s * SR as f64) as usize;
    assert!(peak(&out[at(0.1)..at(0.24)]) < 1e-3, "silent between the note and its echo");
    assert!(peak(&out[at(0.25)..at(0.30)]) > 0.1, "the first echo is heard");
    assert!(peak(&out[at(0.50)..at(0.55)]) > 0.02, "the second echo is heard");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn strict_mode_names_the_kernel_that_broke() {
    let ir = song(
        json!([
            {"id": "p", "op": "const", "value": 100000, "unit": "semitones"},
            {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
        ]),
        "o",
        json!([]),
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let e = Renderer::new(&flat, &Timeline::default(), &pk, SR).unwrap().render(0.01).unwrap_err();
    assert!(e.to_string().contains("song/hz produced inf"), "{e}");
}

/// A package folder holding one hand-written kernel module.
fn package(wat: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("m2-pkg-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("k.wasm"), wat).unwrap();
    std::fs::write(
        dir.join("m2pkg.toml"),
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\nwasm = \"k.wasm\"\n\n[[kernel]]\nname = \"k\"\noutputs = [\"out: audio<1>\"]\n",
    )
    .unwrap();
    dir
}

const KERNEL_EXPORTS: &str = r#"
  (memory (export "memory") 1)
  (func (export "k_init") (param f32 i32 i32))
  (func (export "k_ports") (result i32) (i32.const 1))
  (func (export "k_port") (param i32) (result i32) (i32.const 1024))
  (func (export "k_reset"))
  (func (export "k_process") (param i32))
  (func (export "k_active") (result i32) (i32.const 0))"#;

#[test]
fn packages_must_be_sandboxed_and_speak_our_abi() {
    let ok = package(&format!("(module (func (export \"m2_abi\") (result i32) (i32.const 0)) {KERNEL_EXPORTS})"));
    PackageSet::default().load(&ok).unwrap();

    let wrong_abi = package(&format!("(module (func (export \"m2_abi\") (result i32) (i32.const 7)) {KERNEL_EXPORTS})"));
    let e = PackageSet::default().load(&wrong_abi).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("ABI v7"), "{e:#}");

    let imports = package(&format!("(module (import \"env\" \"now\" (func)) (func (export \"m2_abi\") (result i32) (i32.const 0)) {KERNEL_EXPORTS})"));
    let e = PackageSet::default().load(&imports).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("can't import anything"), "{e:#}");

    let missing = package("(module (func (export \"m2_abi\") (result i32) (i32.const 0)) (memory (export \"memory\") 1))");
    let e = PackageSet::default().load(&missing).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("missing export `k_init`"), "{e:#}");

    for d in [ok, wrong_abi, imports, missing] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn voices_end_when_their_envelope_closes_but_not_while_echoes_remain() {
    // saw → filter → amp: the oscillator never stops, but the voice is over once the
    // envelope has released.
    let synth = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                {"id": "f", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "o"}},
                {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g"}},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "f.lp", "by": "e"}}]}},
        ]),
        "v",
        json!([{"id": "notes", "kind": "events<note>"}]),
    );
    let (tl, dir) = timeline(&[("song.notes.json", json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 0.2}]}]}))]);
    let pk = std_packages();
    let mut r = Renderer::new(&check(&synth, &pk).unwrap(), &tl, &pk, SR).unwrap();
    r.render(0.05).unwrap();
    assert_eq!(r.active_voices(), 1);
    r.render(1.0).unwrap();
    assert_eq!(r.active_voices(), 0, "the voice ended after its release");
    std::fs::remove_dir_all(dir).ok();

    // The same short note through a delay inside the voice stays alive between echoes.
    let echo = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "d", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}},
                {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "a", "time": "t"}},
                {"id": "t", "op": "const", "value": 0.5, "unit": "seconds"}]}},
        ]),
        "v",
        json!([{"id": "notes", "kind": "events<note>"}]),
    );
    let (tl, dir) = timeline(&[("song.notes.json", json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 0.1}]}]}))]);
    let mut r = Renderer::new(&check(&echo, &pk).unwrap(), &tl, &pk, SR).unwrap();
    r.render(0.3).unwrap();
    assert_eq!(r.active_voices(), 1, "silent between echoes, but echoes are coming");
    r.render(20.0).unwrap();
    assert_eq!(r.active_voices(), 0, "the echoes died away");
    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------- audio lanes

/// A song that plays one stereo audio lane, with the given assets and clips.
fn audio_song(assets: &[(&str, Vec<Vec<f32>>, u32)], clips: Value) -> (Ir, Timeline, PathBuf) {
    let ir = song(json!([{"id": "a", "op": "lane", "lane": "clips"}]), "a", json!([{"id": "clips", "kind": "audio<2>"}]));
    let (tl, dir) = timeline(&[("song.json", json!({"tempo": [{"beat": 0, "bpm": 120}]})), ("song.clips.json", json!({"clips": clips}))]);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    for (name, data, sr) in assets {
        audio::write_wav(&dir.join("assets").join(name), data, *sr).unwrap();
    }
    (ir, tl, dir)
}

fn render_audio(ir: &Ir, tl: &Timeline, dir: &Path, seconds: f64) -> Vec<[f32; 2]> {
    let pk = std_packages();
    let flat = check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    Renderer::build(&flat, tl, &pk, SR, Some(&dir.join("assets"))).unwrap().render(seconds).unwrap()
}

fn sine(hz: f64, seconds: f64, sr: u32) -> Vec<f32> {
    (0..(seconds * sr as f64) as usize).map(|i| (i as f64 / sr as f64 * hz * std::f64::consts::TAU).sin() as f32 * 0.5).collect()
}

/// Frequency from upward zero crossings.
fn freq(x: &[[f32; 2]]) -> f64 {
    let n = x.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count();
    n as f64 / (x.len() as f64 / SR as f64)
}

#[test]
fn clips_land_on_their_sample_with_gain_and_fades() {
    let dc = vec![vec![0.5f32; 48_000]];
    let (ir, tl, dir) = audio_song(&[("dc.wav", dc, SR)], json!([{"start": 1, "length": 1, "asset": "dc.wav", "gain_db": -6, "fade_out": 0.1}]));
    let out = render_audio(&ir, &tl, &dir, 1.5);
    // 120 bpm: beat 1 is sample 24 000; the clip lasts one beat (24 000 samples).
    assert_eq!(out[23_999], [0.0, 0.0]);
    let g = 0.5 * 10f32.powf(-6.0 / 20.0);
    assert!((out[24_000][0] - g).abs() < 1e-6 && out[24_000][0] == out[24_000][1], "mono assets play on both channels");
    assert!((out[30_000][0] - g).abs() < 1e-6);
    assert!(out[47_990][0] < 0.01 * g, "faded out by the end");
    assert_eq!(out[48_000], [0.0, 0.0]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn pitch_without_warp_is_varispeed_and_with_warp_keeps_timing() {
    let (ir, tl, dir) = audio_song(
        &[("a440.wav", vec![sine(440.0, 3.0, SR)], SR)],
        json!([
            {"start": 0, "length": 2, "asset": "a440.wav", "pitch": 12},
            {"start": 4, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [2, 2]]},
            {"start": 8, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [2, 2]], "pitch": 12}
        ]),
    );
    let out = render_audio(&ir, &tl, &dir, 5.0);
    let s = |a: f64, b: f64| &out[(a * SR as f64) as usize..(b * SR as f64) as usize];
    // Varispeed: an octave up is twice the frequency.
    assert!((freq(s(0.1, 0.9)) - 880.0).abs() < 5.0, "{}", freq(s(0.1, 0.9)));
    // Warped: two seconds of source squeezed into one (two beats at 120 bpm), same pitch.
    assert!((freq(s(2.1, 2.9)) - 440.0).abs() < 8.0, "{}", freq(s(2.1, 2.9)));
    assert!(s(2.1, 2.9).iter().all(|f| f[0].abs() <= 0.6));
    // Warped and transposed: same timing, an octave up.
    assert!((freq(s(4.1, 4.9)) - 880.0).abs() < 12.0, "{}", freq(s(4.1, 4.9)));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn reverse_and_other_sample_rates() {
    let ramp: Vec<f32> = (0..24_000).map(|i| i as f32 / 24_000.0).collect();
    let (ir, tl, dir) = audio_song(
        &[("ramp.wav", vec![ramp], SR), ("a441.wav", vec![sine(441.0, 2.0, 44_100)], 44_100)],
        json!([
            {"start": 0, "length": 1, "asset": "ramp.wav", "reverse": true},
            {"start": 2, "length": 2, "asset": "a441.wav"}
        ]),
    );
    let out = render_audio(&ir, &tl, &dir, 2.0);
    assert!(out[100][0] > 0.99 && out[23_900][0] < 0.01, "plays backwards");
    assert!((freq(&out[48_000 + 4_800..96_000 - 4_800]) - 441.0).abs() < 3.0, "a 44.1 kHz asset keeps its pitch at 48 kHz");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_bounce_placed_as_a_clip_reproduces_the_original() {
    // Render a synth, write it as an asset, play it back from an audio lane: identical.
    let synth = song(
        json!([
            {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "f"}},
            {"id": "f", "op": "const", "value": 110, "unit": "hz"},
            {"id": "x", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "o"}},
        ]),
        "x.lp",
        json!([]),
    );
    let original = render(&synth, &Timeline::default(), 1.0);
    let channels = vec![original.iter().map(|f| f[0]).collect::<Vec<f32>>(), original.iter().map(|f| f[1]).collect()];
    let (ir, tl, dir) = audio_song(&[("bounce.wav", channels, SR)], json!([{"start": 0, "length": 2, "asset": "bounce.wav"}]));
    let played = render_audio(&ir, &tl, &dir, 1.0);
    assert!(played == original, "the bounce plays back bit for bit");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn audio_lanes_seek_exactly() {
    let (ir, tl, dir) = audio_song(
        &[("a440.wav", vec![sine(440.0, 3.0, SR)], SR)],
        json!([{"start": 0.5, "length": 3, "asset": "a440.wav", "warp": [[0, 0], [3, 3]], "fade_in": 0.05}]),
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let mut r = Renderer::build(&flat, &tl, &pk, SR, Some(&dir.join("assets"))).unwrap();
    let full = r.render(2.0).unwrap();
    r.seek(30_011).unwrap();
    let part = r.render(0.5).unwrap();
    assert!(part[..] == full[30_011..30_011 + part.len()]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ignore = "prints measurements; run with --ignored --nocapture"]
fn stretch_quality_report() {
    let (ir, tl, dir) = audio_song(
        &[("a440.wav", vec![sine(440.0, 3.0, SR)], SR)],
        json!([
            {"start": 0, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [2, 2]]},
            {"start": 4, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [1, 2]]},
            {"start": 8, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [2, 2]], "pitch": 12}
        ]),
    );
    let out = render_audio(&ir, &tl, &dir, 5.0);
    for (name, a) in [("2x faster", 0.1), ("2x slower", 2.1), ("faster + octave", 4.1)] {
        let s = &out[(a * SR as f64) as usize..((a + 0.8) * SR as f64) as usize];
        // Level per 10 ms window: a clean stretch of a sine keeps a constant level.
        let w: Vec<f32> = s.chunks(480).map(|c| c.iter().fold(0.0f32, |m, f| m.max(f[0].abs()))).collect();
        let (lo, hi) = (w.iter().cloned().fold(f32::MAX, f32::min), w.iter().cloned().fold(0.0, f32::max));
        println!("{name}: {:.1} Hz, level {lo:.3}..{hi:.3} ({:.1} dB ripple)", freq(s), 20.0 * (hi / lo).log10());
    }
    std::fs::remove_dir_all(dir).ok();
}
