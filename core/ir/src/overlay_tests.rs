//! Where overlays go (MON-1, MON-2, MON-5): the checker's sites, and mute in song in the IR.

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
        ("t:pan", &["in: audio<1>"], &["out: audio<2>"]),
    ] {
        m.insert(id.to_string(), KernelSig::parse(id, "", &s(i), &s(o), 0).unwrap());
    }
    m
}

/// A kick track (a saw, its `env` a follower of it) and `song` with `nodes`, `frames` and
/// output `out`.
fn kick_and(song: Value, frames: Value, out: &str) -> Ir {
    serde_json::from_value(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "kick", "track": true, "nodes": [
            {"id": "o", "op": "kernel", "kernel": "t:saw"},
            {"id": "e", "op": "kernel", "kernel": "t:follow", "inputs": {"in": "o"}}],
         "exports": {"out": "o", "env": "e"}},
        {"key": "song", "frames": frames, "nodes": song, "exports": {"out": out}}]}))
    .unwrap()
}

fn sites(ir: &Ir) -> Vec<(String, String, Consumer)> {
    let f = check(ir, &kernels()).unwrap_or_else(|e| panic!("{e:?}"));
    f.overlays.iter().map(|s| (s.module.clone(), s.into.clone(), s.consumer)).collect()
}

fn node(ir: &Ir, name: &str) -> usize {
    check(ir, &kernels()).unwrap().nodes.iter().position(|n| n.name == name).unwrap()
}

// ID-11: an IR without `muted` keeps its JSON, so its hash; `false` reads as absent.
#[test]
fn a_song_without_muted_has_the_same_ir_hash() {
    let text = r#"{"ir":2,"modules":[{"key":"kick","track":true,"lanes":[],"nodes":[],"exports":{}}],"root":{"module":"song","export":"out"}}"#;
    let ir: Ir = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_string(&ir).unwrap(), text);
    let off: Ir = serde_json::from_str(&text.replace(r#""track":true"#, r#""track":true,"muted":false"#)).unwrap();
    assert_eq!(serde_json::to_string(&off).unwrap(), text);
    let on: Ir = serde_json::from_str(&text.replace(r#""track":true"#, r#""track":true,"muted":true"#)).unwrap();
    assert!(on.modules[0].muted && serde_json::to_string(&on).unwrap().contains(r#""muted":true"#));
}

// MON-1: `song(() => lead.out)` reads the track at the output, with nothing in `song` between.
#[test]
fn the_songs_output_reading_a_track_is_a_site() {
    let ir = kick_and(json!([{"id": "k", "op": "import", "module": "kick", "export": "out"}]), json!([]), "k");
    assert_eq!(sites(&ir), [("kick".into(), "song".into(), Consumer::Out)]);
    let f = check(&ir, &kernels()).unwrap();
    assert_eq!((f.overlays[0].src, f.overlays[0].channels), (f.out, 1));
}

// MON-1: every input of `song` that reads the kick's audio is one site, each its own.
#[test]
fn every_input_reading_a_tracks_audio_is_a_site() {
    let ir = kick_and(
        json!([
            {"id": "k", "op": "import", "module": "kick", "export": "out"},
            {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "k", "b": "k"}},
            {"id": "p", "op": "kernel", "kernel": "t:pan", "inputs": {"in": "a"}}]),
        json!([]),
        "p",
    );
    let a = node(&ir, "song/a");
    let want: Vec<(String, String, Consumer)> =
        (0..2).map(|input| ("kick".into(), "song".into(), Consumer::Input { node: a, input })).collect();
    assert_eq!(sites(&ir), want, "song's own nodes are no site, nor the kick's");
}

// MON-2, PKG-13: `follow(kick.out)` in a frame that recorded the kick as its sidechain input
// isn't a site; a mixer reading the same kick is.
#[test]
fn no_site_on_a_sidechain_input() {
    let ir = kick_and(
        json!([
            {"id": "k", "op": "import", "module": "kick", "export": "out"},
            {"id": "f", "op": "kernel", "kernel": "t:follow", "inputs": {"in": "k"}, "frame": 0},
            {"id": "n", "op": "kernel", "kernel": "t:saw"},
            {"id": "d", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "n", "g": "f"}},
            {"id": "m", "op": "kernel", "kernel": "t:add", "inputs": {"a": "d", "b": "k"}}]),
        json!([{"site": 1, "component": "std:follow", "sidechain": [{"input": "in", "ref": "k"}]}]),
        "m",
    );
    assert_eq!(sites(&ir), [("kick".into(), "song".into(), Consumer::Input { node: node(&ir, "song/m"), input: 1 })]);
    // Without the record, the follower's input is a site too.
    let mut plain = ir.clone();
    plain.modules[1].frames[0].sidechain.clear();
    assert_eq!(sites(&plain).len(), 2);
}

// MON-2: a control export (`kick.env`) is no site: it keeps flowing.
#[test]
fn a_control_export_is_no_site() {
    let ir = kick_and(
        json!([
            {"id": "ke", "op": "import", "module": "kick", "export": "env"},
            {"id": "n", "op": "kernel", "kernel": "t:saw"},
            {"id": "d", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "n", "g": "ke"}}]),
        json!([]),
        "d",
    );
    assert_eq!(sites(&ir), []);
}

// MON-1: a voice reading a track's audio from outside reads it through its poly's capture; and a
// feedback reading it is a site too.
#[test]
fn a_voice_capture_and_a_feedback_are_sites() {
    let ir: Ir = serde_json::from_value(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "kick", "track": true, "nodes": [{"id": "o", "op": "kernel", "kernel": "t:saw"}], "exports": {"out": "o"}},
        {"key": "song", "lanes": [{"key": "notes", "kind": "events<note>"}], "nodes": [
            {"id": "k", "op": "import", "module": "kick", "export": "out"},
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "x", "nodes": [
                {"id": "x", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "k"}}]}},
            {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "k"},
            {"id": "a", "op": "kernel", "kernel": "t:add", "inputs": {"a": "v", "b": "fb"}}],
         "exports": {"out": "a"}}]}))
    .unwrap();
    let (v, fb) = (node(&ir, "song/v"), node(&ir, "song/fb"));
    assert_eq!(
        sites(&ir),
        [("kick".into(), "song".into(), Consumer::Capture { poly: v, index: 0 }), ("kick".into(), "song".into(), Consumer::Feedback { node: fb })]
    );
}

// MON-3's edges between tracks: a bus track reading a vocal is a site into the bus, and the bus
// read by the song's output is one into `song`.
#[test]
fn a_track_reading_a_track_is_a_site_into_it() {
    let ir: Ir = serde_json::from_value(json!({"ir": crate::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "vox", "track": true, "nodes": [{"id": "o", "op": "kernel", "kernel": "t:saw"}], "exports": {"out": "o"}},
        {"key": "bus", "track": true, "nodes": [
            {"id": "v", "op": "import", "module": "vox", "export": "out"},
            {"id": "g", "op": "kernel", "kernel": "t:vca", "inputs": {"in": "v"}}], "exports": {"out": "g"}},
        {"key": "song", "nodes": [{"id": "b", "op": "import", "module": "bus", "export": "out"}], "exports": {"out": "b"}}]}))
    .unwrap();
    let g = node(&ir, "bus/g");
    assert_eq!(sites(&ir), [("vox".into(), "bus".into(), Consumer::Input { node: g, input: 0 }), ("bus".into(), "song".into(), Consumer::Out)]);
}

// MON-1, MON-5: only a track is muted in the song; the list is its keys, sorted.
#[test]
fn muted_on_song_is_refused() {
    let mut ir = kick_and(json!([{"id": "k", "op": "import", "module": "kick", "export": "out"}]), json!([]), "k");
    ir.modules[0].muted = true;
    assert_eq!(check(&ir, &kernels()).unwrap().muted, ["kick"]);
    let mut root = ir.clone();
    root.modules[1].muted = true;
    let e = check(&root, &kernels()).unwrap_err();
    assert!(e.len() == 1 && e[0].message.contains("can't be muted"), "{e:?}");
    let mut not_track = ir.clone();
    not_track.modules[0].track = false;
    let e = check(&not_track, &kernels()).unwrap_err();
    assert!(e.len() == 1 && e[0].message.contains("isn't a track"), "{e:?}");
}
