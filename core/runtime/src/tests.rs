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
            op: FOp::Const(0.0),
            outputs: sig.outputs.iter().map(|_| Kind::Audio(1)).collect(),
        };
        let tempo = TempoMap::constant(120.0);
        let tl = Timeline::default();
        let mut b = Builder { store: &mut store, packages: &pk, timeline: &tl, tempo: &tempo, sample_rate: 48_000.0 };
        b.kernel(&node, &id, 1, &inputs).unwrap_or_else(|e| panic!("{e:#}"));
    }
}
