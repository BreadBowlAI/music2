//! The card's menus, switches and views through a running server (CARD-21, CARD-22): a choice
//! written by its name, a switch by true or false, and the values one drag moved written with one
//! `POST /api/graph/set` as one undo step, each one's lease ended by it.

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::routes_8b::tests::post;
use crate::{ServeOptions, start_with};

/// A sound with a choice, a switch and an envelope's controls, and a song that plays it.
const WOBBLE: &str = "import { component, control, port, view } from '@m2/core';\nimport { sampler } from 'std/sampler';\n\
    export const wobble = component({ id: 'wobble', label: 'Wobble', inputs: { notes: port.notes() },\n\
      controls: { shape: control.choice(['saw', 'square', 'sine'], 'saw'), sub: control.bool(false), attack: control.secs(0.005),\n\
        decay: control.secs(0.2), sustain: control.frac(0.6), release: control.secs(0.3) },\n\
      output: port.audio(), editor: { views: { amp: view.envelope('attack', 'decay', 'sustain', 'release') } } },\n\
      (notes, { attack, release }) => sampler(notes, { attack, release }));\n";
const SONG: &str = "import { song, track } from '@m2/core';\nimport { wobble } from './lib/wobble';\n\
    const lead = track('lead', ({ lane }) => wobble(lane.notes('notes'), { shape: 'saw', decay: 0.4 }));\nexport default song(() => lead.out);\n";

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-routes-views-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.7\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(dir.join("lib/wobble.ts"), WOBBLE).unwrap();
    std::fs::write(dir.join("song.ts"), SONG).unwrap();
    dir
}

/// Wobble's knob of `slot`, as the view sends it, and the view's revision.
fn knob(graph: &Graph, slot: &str) -> (u64, Value) {
    let view = graph.state().unwrap()["view"].clone();
    let nodes: Vec<Value> = view["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap().clone()).collect();
    let card = nodes.into_iter().find(|n| n["title"] == "Wobble").expect("Wobble's card");
    let k = card["knobs"].as_array().unwrap().iter().find(|k| k["slot"] == slot).cloned();
    (view["revision"].as_u64().unwrap(), k.unwrap_or_else(|| panic!("no {slot}: {card:#}")))
}

// CARD-21 over HTTP: a choice's menu writes its name and a switch writes true, each one undo step;
// a name it doesn't have is refused in words and nothing is written. CARD-22: a drag of two knobs
// (each previewed under one gesture) is written by one `/set` with `with`: one undo step, both leases
// ended; a value refused refuses both, and ends both leases.
#[test]
fn a_choice_a_switch_and_a_drag_of_two_knobs_through_the_routes() {
    let dir = project("rw");
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let (url, graph) = (running.url.clone(), &running.state.graph);
    let session = graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let read = || std::fs::read_to_string(dir.join("song.ts")).unwrap();
    let set = |body: Value| post(&url, "/api/graph/set", Some("t0k"), &body);

    let (at, shape) = knob(graph, "shape");
    assert_eq!((&shape["kind"], &shape["value"]), (&json!("choice"), &json!("saw")));
    let (status, done) = set(json!({"session": session, "revision": at, "id": shape["id"], "gesture": "set", "value": "square"}));
    assert_eq!(status, 200, "{done:#}");
    assert!(done["step"].is_number(), "one undo step: {done:#}");
    assert_eq!(read(), SONG.replace("shape: 'saw'", "shape: 'square'"));
    assert_eq!(knob(graph, "shape").1["value"], "square");
    let (at, sub) = knob(graph, "sub");
    let (status, done) = set(json!({"session": session, "revision": at, "id": sub["id"], "gesture": "set", "value": true}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!(read(), SONG.replace("shape: 'saw', decay: 0.4", "shape: 'square', decay: 0.4, sub: true"));
    let before = read();
    let (at, shape) = knob(graph, "shape");
    let (status, why) = set(json!({"session": session, "revision": at, "id": shape["id"], "gesture": "set", "value": "squar"}));
    assert!(
        status == 400 && why["error"].as_str().unwrap().contains("shape is one of \"saw\", \"square\" or \"sine\", not \"squar\""),
        "{why:#}"
    );
    assert_eq!(read(), before, "nothing written");

    // A drag of the decay's corner: decay and sustain, heard as they move, written at release.
    let (at, decay) = knob(graph, "decay");
    let (_, sustain) = knob(graph, "sustain");
    let preview = |id: &Value, v: f64| {
        let (status, why) = post(
            &url,
            "/api/graph/preview",
            Some("t0k"),
            &json!({"session": session, "revision": at, "id": id, "gesture": "corner-1", "value": v}),
        );
        assert_eq!(status, 200, "{why:#}");
    };
    preview(&decay["id"], 0.3);
    preview(&sustain["id"], 0.45);
    assert_eq!(graph.leases().len(), 2, "a lease on each control");
    let body = json!({"session": session, "revision": at, "id": decay["id"], "gesture": "corner-1", "value": 0.25, "with": [{"id": sustain["id"], "value": 0.5}]});
    let (status, done) = set(body);
    assert_eq!(status, 200, "{done:#}");
    assert!(done["step"].is_number(), "one undo step: {done:#}");
    assert_eq!(read(), before.replace("decay: 0.4, sub: true", "decay: 0.25, sub: true, sustain: 0.5"));
    assert!(graph.leases().is_empty(), "both leases ended");
    let (status, undone) = post(&url, "/api/graph/undo", Some("t0k"), &json!({"session": session}));
    assert_eq!(status, 200, "{undone:#}");
    assert_eq!(read(), before, "one undo takes both back");

    // Refused: both leases end, nothing is written.
    let (at, decay) = knob(graph, "decay");
    let (_, sustain) = knob(graph, "sustain");
    for (id, v) in [(&decay["id"], 0.3), (&sustain["id"], 0.45)] {
        let (status, why) = post(
            &url,
            "/api/graph/preview",
            Some("t0k"),
            &json!({"session": session, "revision": at, "id": id, "gesture": "corner-2", "value": v}),
        );
        assert_eq!(status, 200, "{why:#}");
    }
    let body = json!({"session": session, "revision": at, "id": decay["id"], "gesture": "corner-2", "value": 0.25, "with": [{"id": sustain["id"], "value": "loud"}]});
    let (status, why) = set(body);
    assert!(status == 400 && why["error"].as_str().unwrap().contains("sustain is a number, not \"loud\""), "{why:#}");
    assert_eq!(read(), before, "nothing written");
    assert!(graph.leases().is_empty(), "both leases ended");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// CARD-22 (review V1, V4): a gesture's write that is refused before it is written ends every lease
// of the gesture together, its controls playing the code's values again, and leaves none closing:
// a knob named twice, and a lease that ended between the drag and its release. After each, the
// knob takes a plain write and a new drag.
#[test]
fn a_refused_gesture_ends_all_its_leases() {
    let dir = project("refused");
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let (url, graph) = (running.url.clone(), &running.state.graph);
    let session = graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let read = || std::fs::read_to_string(dir.join("song.ts")).unwrap();
    let call = |path: &str, body: Value| post(&url, path, Some("t0k"), &body);

    // One knob named twice: refused in words, no lease left closing.
    let (at, decay) = knob(graph, "decay");
    let (status, why) =
        call("/api/graph/preview", json!({"session": session, "revision": at, "id": decay["id"], "gesture": "g1", "value": 0.3}));
    assert_eq!(status, 200, "{why:#}");
    let body = json!({"session": session, "revision": at, "id": decay["id"], "gesture": "g1", "value": 0.25, "with": [{"id": decay["id"], "value": 0.35}]});
    let (status, why) = call("/api/graph/set", body);
    assert!(status == 400 && why["error"].as_str().unwrap().contains("decay is given twice"), "{status} {why:#}");
    assert!(graph.leases().is_empty(), "its lease ended: {:?}", graph.leases());
    assert_eq!(read(), SONG, "nothing written");
    let (status, done) =
        call("/api/graph/set", json!({"session": session, "revision": at, "id": decay["id"], "gesture": "set", "value": 0.5}));
    assert_eq!(status, 200, "a plain write takes it: {done:#}");
    assert!(read().contains("decay: 0.5"));

    // A lease that ended between the drag and its release (its own 200 ms went by): the gesture's
    // other lease ends with it.
    let (at, decay) = knob(graph, "decay");
    let (_, sustain) = knob(graph, "sustain");
    let (status, why) =
        call("/api/graph/lease/begin", json!({"session": session, "revision": at, "id": sustain["id"], "gesture": "g2", "ttl_ms": 200}));
    assert_eq!(status, 200, "{why:#}");
    let (status, why) =
        call("/api/graph/preview", json!({"session": session, "revision": at, "id": decay["id"], "gesture": "g2", "value": 0.3}));
    assert_eq!(status, 200, "{why:#}");
    std::thread::sleep(std::time::Duration::from_millis(400));
    graph.expire();
    assert_eq!(graph.leases().len(), 1, "the sustain's lease ended by its time, the decay's lives");
    let before = read();
    let body = json!({"session": session, "revision": at, "id": decay["id"], "gesture": "g2", "value": 0.25, "with": [{"id": sustain["id"], "value": 0.5}]});
    let (status, why) = call("/api/graph/set", body);
    assert!(status == 400 && why["error"].as_str().unwrap().contains("this gesture ended"), "{status} {why:#}");
    assert!(graph.leases().is_empty(), "the decay's lease ended with it: {:?}", graph.leases());
    assert_eq!(read(), before, "nothing written");
    let (status, why) =
        call("/api/graph/preview", json!({"session": session, "revision": at, "id": decay["id"], "gesture": "g3", "value": 0.3}));
    assert_eq!(status, 200, "a new drag takes it: {why:#}");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
