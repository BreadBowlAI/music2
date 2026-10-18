//! Buses (RENDER-34): a bus is a track the song feeds. The checker holds a bus to being a track
//! with no notes or audio lanes, names the modules of a loop through buses, and works out the
//! routes: which tracks reach which bus through the song (MON-3).

use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use crate::{Ir, KernelSig};

fn kernels() -> HashMap<String, KernelSig> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut m = HashMap::new();
    for (id, i, o) in [
        ("t:saw", &["freq: control<hz> = 440"][..], &["out: audio<1>"][..]),
        ("t:add", &["a: *", "b: *"], &["out: *"]),
        ("t:follow", &["in: audio<*>"], &["out: control<unit01>"]),
        ("t:vca", &["in: audio<*>", "g: control<gain> = 1"], &["out: audio<*>"]),
    ] {
        m.insert(id.to_string(), KernelSig::parse(id, "", &s(i), &s(o), 0).unwrap());
    }
    m
}

fn ir(modules: Value) -> Ir {
    serde_json::from_value(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": modules})).unwrap()
}

/// A track `key` that plays a saw.
fn saw(key: &str) -> Value {
    json!({"key": key, "track": true, "nodes": [{"id": "o", "op": "kernel", "kernel": "t:saw"}], "exports": {"out": "o"}})
}

/// A bus `key` that turns down what the song feeds it.
fn bus(key: &str) -> Value {
    json!({"key": key, "track": true, "bus": true, "nodes": [
        {"id": "i", "op": "import", "module": "song", "export": crate::feed_export(key)},
        {"id": "r", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "i"}}],
     "exports": {"out": "r"}})
}

/// kick and vox into the song; vox's strip is sent to `verb`, verb's strip to `delay`; the kick
/// is heard by a follower (a sidechain input) whose level turns vox's send. The master sums the
/// kick, vox and both buses.
fn mix() -> Ir {
    ir(json!([
        saw("kick"),
        saw("vox"),
        bus("verb"),
        bus("delay"),
        {"key": "song", "frames": [{"site": 1, "sidechain": [{"input": "in", "ref": "k"}]}], "nodes": [
            {"id": "k", "op": "import", "module": "kick", "export": "out"},
            {"id": "v", "op": "import", "module": "vox", "export": "out"},
            {"id": "w", "op": "import", "module": "verb", "export": "out"},
            {"id": "d", "op": "import", "module": "delay", "export": "out"},
            {"id": "vs", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "v"}},
            {"id": "fk", "op": "kernel", "kernel": "t:follow", "inputs": {"in": "k"}, "frame": 0},
            {"id": "send", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "vs", "g": "fk"}},
            {"id": "ws", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "w"}},
            {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "k", "b": "vs"}},
            {"id": "b", "op": "kernel", "kernel": "t:add", "inputs": {"a": "a", "b": "ws"}},
            {"id": "m", "op": "kernel", "kernel": "t:add", "inputs": {"a": "b", "b": "d"}}],
         "exports": {"out": "m", "bus:verb": "send", "bus:delay": "ws"}},
    ]))
}

fn routes(ir: &Ir) -> Vec<(String, String)> {
    check(ir, &kernels()).unwrap_or_else(|e| panic!("{e:?}")).routes
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.into(), b.into())
}

// ID-11: an IR without `bus` keeps its JSON, so its hash; `false` reads as absent.
#[test]
fn a_song_without_buses_has_the_same_ir_hash() {
    let text = r#"{"ir":2,"modules":[{"key":"kick","track":true,"lanes":[],"nodes":[],"exports":{}}],"root":{"module":"song","export":"out"}}"#;
    let ir: Ir = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_string(&ir).unwrap(), text);
    let off: Ir = serde_json::from_str(&text.replace(r#""track":true"#, r#""track":true,"bus":false"#)).unwrap();
    assert_eq!(serde_json::to_string(&off).unwrap(), text);
    let on: Ir = serde_json::from_str(&text.replace(r#""track":true"#, r#""track":true,"bus":true"#)).unwrap();
    assert!(on.modules[0].bus && serde_json::to_string(&on).unwrap().contains(r#""bus":true"#));
}

// MON-3, RENDER-34: a route `m → b` is a track whose way into the song reaches bus `b`'s feed. vox
// is sent to verb, and verb to delay; the kick reaches the send only through a sidechain input, so
// it is no route. A song with no bus has none.
#[test]
fn routes_follow_the_song_from_a_track_to_the_buses_it_feeds() {
    assert_eq!(routes(&mix()), [pair("verb", "delay"), pair("vox", "verb")]);
    let f = check(&mix(), &kernels()).unwrap();
    // The bus is a track like any other: the song reads it through a site.
    let sites: Vec<(&str, &str)> = f.overlays.iter().map(|s| (s.module.as_str(), s.into.as_str())).collect();
    assert_eq!(sites, [("vox", "song"), ("verb", "song"), ("kick", "song"), ("delay", "song")]);
    let plain = ir(json!([saw("kick"), {"key": "song", "nodes": [{"id": "k", "op": "import", "module": "kick", "export": "out"}], "exports": {"out": "k"}}]));
    assert!(routes(&plain).is_empty());
}

// RENDER-34: a bus is a track, and it plays what it is fed, so it has no notes or audio lanes. A
// control lane, to automate it, is fine.
#[test]
fn a_bus_is_a_track_with_no_notes_or_audio_lanes() {
    let mut m = mix();
    m.modules[2].lanes = serde_json::from_value(json!([{"key": "mix", "kind": "control<unit01>", "default": 1}])).unwrap();
    assert!(check(&m, &kernels()).is_ok());
    for (kind, word) in [("events<note>", "notes"), ("audio<2>", "audio")] {
        let mut m = mix();
        m.modules[2].lanes = serde_json::from_value(json!([{"key": "x", "kind": kind}])).unwrap();
        let e = check(&m, &kernels()).unwrap_err();
        assert!(e.iter().any(|d| d.module.as_deref() == Some("verb") && d.message.contains(&format!("the bus `verb` declares the {word} lane `x`"))), "{e:?}");
    }
    let mut m = mix();
    m.modules[2].track = false;
    let e = check(&m, &kernels()).unwrap_err();
    assert!(e.iter().any(|d| d.message.contains("`verb` is a bus, so it must be a track")), "{e:?}");
}

// RENDER-34: a bus that feeds itself through code is the checker's loop error, which names the
// modules the loop goes through.
#[test]
fn a_loop_through_a_bus_names_its_modules() {
    let mut m = mix();
    // verb is fed its own strip.
    m.modules[4].exports.insert("bus:verb".into(), "ws".into());
    let e = check(&m, &kernels()).unwrap_err();
    assert!(e[0].message.contains("these nodes form a loop: "), "{}", e[0]);
    assert!(e[0].message.contains("verb/r (t:vca) → song/ws (t:vca) → verb/r (t:vca). It goes through verb → song → verb."), "{}", e[0]);
    // A loop in one module says no more than it did.
    let one = ir(json!([{"key": "song", "nodes": [
        {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "o", "b": "b"}},
        {"id": "b", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "a"}},
        {"id": "o", "op": "kernel", "kernel": "t:saw"}], "exports": {"out": "a"}}]));
    let e = check(&one, &kernels()).unwrap_err();
    assert!(!e[0].message.contains("goes through"), "{}", e[0]);
}
