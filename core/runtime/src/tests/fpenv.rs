//! One floating-point mode on every thread that renders (RENDER-2a).

use super::*;

/// A sine through `std:scale` by 1e-30, then 1e-10 (about 1e-40: subnormal), then 1e30 and 1e10:
/// near the sine again where subnormals are kept, silence where they are flushed to zero.
pub(super) fn subnormal_chain() -> Ir {
    let mut nodes = vec![
        json!({"id": "f", "op": "const", "value": 440, "unit": "hz"}),
        json!({"id": "x0", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}),
    ];
    for (k, by) in [1e-30, 1e-10, 1e30, 1e10].into_iter().enumerate() {
        nodes.push(json!({"id": format!("by{k}"), "op": "const", "value": by}));
        nodes.push(json!({"id": format!("x{}", k + 1), "op": "kernel", "kernel": "std:scale", "inputs": {"in": format!("x{k}"), "by": format!("by{k}")}}));
    }
    song(Value::Array(nodes), "x4", json!([]))
}

// RENDER-2a: a thread that flushes subnormals to zero (a host's, a device's) renders the same bits
// as one that keeps them, and flushes again afterwards: the engine sets the one mode for the
// block and puts the thread's own back.
#[test]
fn a_flush_to_zero_set_by_the_caller_changes_no_bit() {
    let pk = std_packages();
    let f = check(&subnormal_chain(), &pk).unwrap();
    let kept = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(0.1).unwrap();
    assert!(peak(&kept) > 0.5, "the chain gives the sine back: {}", peak(&kept));
    let (flushed, after) = std::thread::scope(|s| {
        s.spawn(|| {
            crate::fpenv::flush_to_zero();
            let out = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(0.1).unwrap();
            (out, crate::fpenv::flushes())
        })
        .join()
        .unwrap()
    });
    assert!(flushed == kept, "the same bits on a thread that flushes");
    assert!(after, "the thread flushes again afterwards");
}

/// One note on a synth and an fm voice, then nothing, through every std kernel that keeps state:
/// a filter, a delay and a reverb on long feedback, chorus, phaser, tremolo, a compressor, an eq,
/// a limiter, drive, a ducker following the fm voice, and a widener, and `@m2/effects`' own
/// bitcrusher (PACK-21), all summed.
fn long_tail() -> (Ir, DataSet) {
    let voice = |kernel: &str| {
        json!({"op": "poly", "events": "n", "voices": 2, "body": {"out": "x", "nodes": [
            {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
            {"id": "x", "op": "kernel", "kernel": kernel, "inputs": {"pitch": "p", "gate": "g"}}]}})
    };
    let c = |id: &str, v: f64| json!({"id": id, "op": "const", "value": v});
    let fx = |id: &str, kernel: &str, inputs: Value| json!({"id": id, "op": "kernel", "kernel": kernel, "inputs": inputs});
    let mut nodes = vec![json!({"id": "n", "op": "lane", "lane": "notes"})];
    let (mut s, mut f) = (voice("std:synth"), voice("std:fm"));
    s["id"] = json!("s");
    f["id"] = json!("f");
    nodes.extend([s, f]);
    nodes.extend([
        c("hi", 0.9),
        c("big", 0.98),
        c("low", 0.1),
        c("one", 1.0),
        c("t", 0.13),
        c("q", 4.0),
        c("cut", 800.0),
        c("deep", -30.0),
        c("ratio", 8.0),
    ]);
    nodes.extend([
        fx("fs", "std:pan", json!({"in": "f"})),
        fx("svf", "std:svf", json!({"in": "s", "cutoff": "cut", "q": "q"})),
        fx("delay", "std:delay", json!({"in": "s", "time": "t", "feedback": "hi", "mix": "one"})),
        fx("reverb", "std:reverb", json!({"in": "s", "size": "big", "damping": "low", "mix": "one"})),
        fx("verb2", "std:reverb", json!({"in": "fs", "size": "big", "damping": "low", "mix": "one"})),
        fx("chorus", "std:chorus", json!({"in": "s"})),
        fx("phaser", "std:phaser", json!({"in": "s", "feedback": "hi"})),
        fx("tremolo", "std:tremolo", json!({"in": "s"})),
        fx("compress", "std:compress", json!({"in": "s", "threshold": "deep", "ratio": "ratio"})),
        fx("eq3", "std:eq3", json!({"in": "s"})),
        fx("bitcrush", "@m2/effects:bitcrush", json!({"in": "s"})),
        fx("limit", "std:limit", json!({"in": "s"})),
        fx("drive", "std:drive", json!({"in": "s"})),
        fx("follow", "std:follow", json!({"in": "f"})),
        fx("duck", "std:duck", json!({"in": "s", "side": "follow"})),
        fx("widen", "std:widen", json!({"in": "s"})),
    ]);
    let sum = [
        "svf.lp", "svf.bp", "svf.hp", "delay", "reverb", "verb2", "chorus", "phaser", "tremolo", "compress", "eq3", "bitcrush", "limit",
        "drive", "duck", "widen",
    ];
    let mut last = "fs".to_string();
    for (k, x) in sum.iter().enumerate() {
        nodes.push(fx(&format!("sum{k}"), "std:add", json!({"a": last, "b": x})));
        last = format!("sum{k}");
    }
    let ir = song(Value::Array(nodes), &last, json!([{"key": "notes", "kind": "events<note>"}]));
    let notes = json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 57, "start": 0, "length": 0.5}]}]});
    let data = DataSet::with_lanes(
        super::live::at_bpm(120.0),
        [(m2_song::LaneAddr::new("song", "notes").unwrap(), {
            m2_song::LaneData::read(&notes.to_string(), &m2_song::LaneKind::Notes).unwrap()
        })],
    );
    (ir, data)
}

// OS-49 (8a, Decision 12): subnormal numbers are slow on x86, so std flushes its recursive state
// below about 1e-15 (`fx.rs`). The bitcrusher, `@m2/effects`' own kernel since std 0.8, makes none
// either: the only state of it that settles is its mix's smoothing, which lands exactly on its
// target. One note, then five minutes of silence, through every std kernel that keeps state and the
// bitcrusher, counting the subnormal samples in every node's output: counted, not timed.
// std 0.2's filter, delay and envelope follower still let theirs through (a sum of subnormals is
// one too, so the adds after them are left out). This test names them: when a std bump flushes
// them (OS-49's deferred fix), it fails, and the list shrinks to none.
#[test]
fn which_std_kernels_make_subnormals_in_a_five_minute_tail() {
    let pk = own_packages();
    let (ir, data) = long_tail();
    let f = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::new(&f, &data, &pk, SR).unwrap();
    let blocks = (300.5 * SR as f64 / BLOCK as f64) as usize;
    let mut subnormal = vec![0u64; f.nodes.len()];
    let mut heard = 0.0f32;
    for k in 0..blocks {
        let out = r.render_block((k * BLOCK) as u64).unwrap();
        heard = heard.max(out.iter().fold(0.0, |m, x| m.max(x[0].abs())));
        for (i, n) in subnormal.iter_mut().enumerate() {
            *n += r.plan.graph.bufs[i].iter().flatten().filter(|x| x.is_subnormal()).count() as u64;
        }
    }
    assert!(heard > 0.1, "the note sounds: {heard}");
    let mut kernels: Vec<&str> = (0..f.nodes.len())
        .filter(|&i| subnormal[i] > 0)
        .filter_map(|i| if let m2_ir::FOp::Kernel { kernel, .. } = &f.nodes[i].op { Some(kernel.as_str()) } else { None })
        .filter(|k| *k != "std:add")
        .collect();
    kernels.sort_unstable();
    kernels.dedup();
    let counts: Vec<String> =
        (0..f.nodes.len()).filter(|&i| subnormal[i] > 0).map(|i| format!("{}: {}", f.nodes[i].name, subnormal[i])).collect();
    assert_eq!(kernels, ["std:delay", "std:follow", "std:svf"], "the kernels whose outputs held subnormal samples: {counts:?}");
}
