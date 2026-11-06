//! The planner's offsets and delays (RENDER-18, RENDER-19, RENDER-20), on checked graphs with each
//! node's latency given by name.

use super::*;
use crate::latency::{At, Delay, Latency, insert, plan};

/// `nodes` checked, and each node's latency from `latent` by its id.
fn planned(nodes: Value, out: &str, lanes: Value, latent: &[(&str, u32)]) -> (Flat, Latency) {
    let flat = check(&song(nodes, out, lanes), &std_packages()).unwrap_or_else(|e| panic!("{e:?}"));
    let lat: Vec<u32> =
        flat.nodes.iter().map(|n| latent.iter().find(|(id, _)| n.name == format!("song/{id}")).map_or(0, |l| l.1)).collect();
    let l = plan(&flat, &lat);
    (flat, l)
}

fn at(flat: &Flat, id: &str) -> usize {
    flat.nodes.iter().position(|n| n.name == format!("song/{id}")).unwrap_or_else(|| panic!("no node {id}"))
}

// A chain adds up, and a lane read after it arrives late by the chain's latency.
#[test]
fn a_chain_adds_up_and_its_lane_is_delayed() {
    let (flat, l) = planned(
        json!([
            {"id": "f", "op": "const", "value": 440.0},
            {"id": "one", "op": "const", "value": 1.0},
            {"id": "neg", "op": "const", "value": -1.0},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "x", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "x", "by": "one"}},
            {"id": "g", "op": "lane", "lane": "gain"},
            {"id": "b", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "a", "by": "g"}},
        ]),
        "b",
        json!([{"key": "gain", "kind": "control<gain>", "default": 1.0}]),
        &[("a", 10), ("b", 5)],
    );
    let (a, b) = (at(&flat, "a"), at(&flat, "b"));
    assert_eq!((l.offset[a], l.offset[b], l.total), (0, 10, 15));
    assert_eq!(l.delays, [Delay { consumer: b, at: At::Input(1), frames: 10 }], "the lane, as late as `a`");
    assert!(l.in_feedback.is_empty());
}

// Two parallel paths: the faster is delayed by the difference.
#[test]
fn the_faster_of_two_paths_is_delayed() {
    let (flat, l) = planned(
        json!([
            {"id": "f", "op": "const", "value": 440.0},
            {"id": "one", "op": "const", "value": 1.0},
            {"id": "neg", "op": "const", "value": -1.0},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "x", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "slow", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "x", "by": "one"}},
            {"id": "fast", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "x", "by": "neg"}},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "slow", "b": "fast"}},
        ]),
        "sum",
        json!([]),
        &[("slow", 100)],
    );
    let sum = at(&flat, "sum");
    assert_eq!((l.offset[sum], l.total), (100, 100));
    assert_eq!(l.delays, [Delay { consumer: sum, at: At::Input(1), frames: 100 }]);
    // The graph that plays: the delay after every checked node, read by `sum`, run right before it.
    let g = insert(&flat, &l);
    let d = flat.nodes.len();
    assert_eq!(g.nodes.len(), d + 1);
    assert!(matches!(g.nodes[d].op, FOp::Delay { input, frames: 100 } if input.node == at(&flat, "fast")));
    let FOp::Kernel { inputs, .. } = &g.nodes[sum].op else { panic!() };
    assert!(matches!(inputs[1], Input::Port { src, .. } if src.node == d));
    let pos = |i: usize| g.order.iter().position(|&x| x == i).unwrap();
    assert_eq!(pos(d) + 1, pos(sum));
}

// A diamond: each input of the join waits for the latest.
#[test]
fn a_diamond_waits_for_its_slowest_side() {
    let (flat, l) = planned(
        json!([
            {"id": "f", "op": "const", "value": 440.0},
            {"id": "one", "op": "const", "value": 1.0},
            {"id": "neg", "op": "const", "value": -1.0},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "x", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "p", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "x", "by": "one"}},
            {"id": "q", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "p", "by": "one"}},
            {"id": "r", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "x", "by": "one"}},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "q", "b": "r"}},
        ]),
        "sum",
        json!([]),
        &[("p", 30), ("q", 20), ("r", 10)],
    );
    let (q, r, sum) = (at(&flat, "q"), at(&flat, "r"), at(&flat, "sum"));
    assert_eq!((l.offset[q], l.offset[r], l.offset[sum], l.total), (30, 0, 50, 50));
    assert_eq!(l.delays, [Delay { consumer: sum, at: At::Input(1), frames: 40 }]);
}

// A latent node on a feedback loop can't be compensated: it is named (RENDER-20). The feedback's
// output arrives at 0, so nothing else waits for the loop.
#[test]
fn latency_on_a_feedback_loop_is_named() {
    let (flat, l) = planned(
        json!([
            {"id": "f", "op": "const", "value": 440.0},
            {"id": "one", "op": "const", "value": 1.0},
            {"id": "neg", "op": "const", "value": -1.0},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "x", "op": "kernel", "kernel": "std:noise"},
            {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "sum"},
            {"id": "half", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "x", "b": "half"}},
        ]),
        "sum",
        json!([]),
        &[("half", 64)],
    );
    let (half, sum) = (at(&flat, "half"), at(&flat, "sum"));
    assert_eq!(l.in_feedback, [half], "the latent node; the noise's delay into the join isn't on the loop");
    assert_eq!((l.total, l.delays), (64, vec![Delay { consumer: sum, at: At::Input(0), frames: 64 }]));
}

// No latency: no delay, and the graph that plays is the checked one.
#[test]
fn no_latency_makes_no_delay() {
    let (flat, l) = planned(
        json!([
            {"id": "f", "op": "const", "value": 440.0},
            {"id": "one", "op": "const", "value": 1.0},
            {"id": "neg", "op": "const", "value": -1.0},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "x", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "x", "by": "one"}},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "x"}},
        ]),
        "sum",
        json!([]),
        &[],
    );
    assert_eq!((l.delays.len(), l.total), (0, 0));
    assert_eq!(insert(&flat, &l), flat);
}

/// A package whose kernel `@test/t:k` passes its mono input on, its manifest saying how late with
/// `latency` (`latency_s = 0.015`, say).
fn late_kernel(latency: &str) -> PathBuf {
    package_with(
        &module_v1(&kernel_v1("k", &out_v1("(local.get $x)"))),
        &format!("inputs = [\"in: audio<1>\"]\noutputs = [\"out: audio<1>\"]\n{latency}"),
    )
}

// VOX-24: a latency in seconds is counted at the song's rate by the kit's one rule, rounded up: the
// song is that many frames late at 44.1, 48 and 96 kHz, and the path beside the kernel is delayed
// to match it (RENDER-18).
#[test]
fn a_latency_in_seconds_is_made_up_for_at_every_rate() {
    let dir = late_kernel("latency_s = 0.015");
    let mut pk = std_packages();
    pk.load(&dir).unwrap();
    let ir = song(
        json!([
            {"id": "f", "op": "const", "value": 440.0},
            {"id": "x", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "late", "op": "kernel", "kernel": "@test/t:k", "inputs": {"in": "x"}},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "late", "b": "x"}},
        ]),
        "sum",
        json!([]),
    );
    let flat = check(&ir, &pk).unwrap();
    for (rate, frames) in [(44_100, 662), (48_000, 720), (96_000, 1_440)] {
        assert_eq!(frames, m2_kernel::latency_frames(0.015, rate as f64), "the kit's rule");
        let r = Renderer::new(&flat, &DataSet::empty(), &pk, rate).unwrap();
        assert_eq!((r.latency(), r.stats().delays), (frames, 1), "{rate} Hz: the song is as late, the path beside it delayed");
    }
    std::fs::remove_dir_all(dir).ok();
}

// VOX-24: a kernel says its latency one way: in frames or in seconds, never both.
#[test]
fn a_latency_in_seconds_and_in_frames_is_refused() {
    let dir = late_kernel("latency = 64\nlatency_s = 0.015");
    let e = std_packages().load(&dir).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("gives its latency twice: say `latency` (frames) or `latency_s` (seconds)"), "{e:#}");
    std::fs::remove_dir_all(dir).ok();
}
