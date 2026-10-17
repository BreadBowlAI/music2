//! The DSP load and the engine's idling, as the player publishes them for the meters (8b.8).

use m2_runtime::BLOCK;
use serde_json::json;

use super::*;

/// A renderer of `nodes` at 48 kHz, its output `out`.
fn song(nodes: serde_json::Value, out: &str) -> Renderer {
    let ir: m2_ir::Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "nodes": nodes, "exports": {"out": out}}]}))
    .unwrap();
    let mut packages = m2_runtime::PackageSet::default();
    packages.load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    let flat = m2_ir::check(&ir, &packages).unwrap();
    Renderer::new(&flat, &m2_song::DataSet::empty(), &packages, 48_000).unwrap()
}

/// A 1 kHz sine, times `level`.
fn sine(level: f64) -> Renderer {
    song(
        json!([
            {"id": "f", "op": "const", "value": 1000, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "l", "op": "const", "value": level},
            {"id": "x", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "l"}}]),
        "x",
    )
}

/// `frames` frames through the callback, in buffers of `n`.
fn fill(d: &mut Driver, frames: usize, n: usize) {
    let mut data = vec![0.0f32; 2 * n];
    for _ in 0..frames.div_ceil(n) {
        d.fill(&mut data, 2);
    }
}

// 8b.8: the load is the longest callback since it was last read, as a share of its period; a
// callback longer than its period is late, and counted for good. A device at 1 GHz (a frame is a
// nanosecond) makes every callback late; nothing plays, so nothing is converted to its rate.
#[test]
fn the_load_is_the_longest_callback() {
    let (player, mut d) = Player::driven(1_000_000_000);
    assert_eq!(player.load(), Load::default());
    fill(&mut d, 5, 1);
    let l = player.load();
    assert!(l.longest > 1.0, "{l:?}");
    assert_eq!(l.late, 5);
    assert_eq!(player.load(), Load { longest: 0.0, late: 5 }, "read: the longest starts again; late counts on");
    // At 48 kHz, playing: never more than the longest share the test's own clock saw.
    let (player, mut d) = Player::driven(48_000);
    player.open(sine(0.5)).unwrap();
    player.play();
    let mut most = 0.0f64;
    for n in [64usize, 480, 1, 4096, 128, 37] {
        let mut data = vec![0.0f32; 2 * n];
        let t = now_ns();
        d.fill(&mut data, 2);
        most = most.max((now_ns() - t) as f64 / (n as f64 * 1e9 / 48_000.0));
    }
    let l = player.load();
    assert!(l.longest > 0.0 && l.longest as f64 <= most * 1.0001, "{l:?}, the test saw {most}");
}

// RENDER-6a, 8b.8: the player says when the engine idles, so nothing reads meters then: with no
// project, stopped and silent, and never while it plays or a sound rings on stopped.
#[test]
fn the_engine_says_when_it_idles() {
    let (player, mut d) = Player::driven(48_000);
    assert!(player.idle(), "nothing plays");
    player.open(sine(0.0)).unwrap();
    fill(&mut d, BLOCK, BLOCK);
    assert!(!player.idle(), "loaded: it renders until it has been silent a while");
    fill(&mut d, 4096, 256);
    assert!(player.idle(), "stopped and silent");
    player.play();
    fill(&mut d, BLOCK, BLOCK);
    assert!(!player.idle(), "playing");
    fill(&mut d, 4096, 256);
    assert!(!player.idle(), "playing silence still runs");
    player.stop();
    fill(&mut d, 4096, 256);
    assert!(player.idle(), "stopped again");
    player.open(sine(0.5)).unwrap();
    fill(&mut d, 48_000, 480);
    assert!(!player.idle(), "a sine sounds on while stopped");
    player.unload().unwrap();
    fill(&mut d, 4096, 256);
    assert!(player.idle(), "unloaded");
}
