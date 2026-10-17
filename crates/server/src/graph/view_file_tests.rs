//! The view settings kept per song in `.music2/view.json` (polish Q16): merged, never the loop,
//! read again at the next open, and kept in memory only in a read-only window.

use serde_json::{Value, json};

use super::super::tests::Fixture;
use super::*;

fn token(graph: &Graph) -> u64 {
    graph.state().unwrap()["session"].as_u64().unwrap()
}

fn file_of(fixture: &Fixture) -> Value {
    serde_json::from_str(&fixture.read(".music2/view.json")).unwrap()
}

fn map(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn view_settings_merge_and_keep_the_loop() {
    let fixture = Fixture::new();
    let graph = Graph::silent();
    graph.open(&fixture.0).unwrap();
    assert_eq!(graph.state().unwrap()["local"], json!({}), "a new song starts with the defaults");
    graph.set_loop(Some((4.0, 8.0)), Some(true)).unwrap();
    let loop_ = json!({"start": 4.0, "end": 8.0, "on": true});
    let grid = json!({"notes": {"pick": 0.125, "triplet": false, "on": true}});

    // A set merges, beside the loop.
    let local = graph.set_view(token(&graph), map(json!({"grid": grid, "audition": false}))).unwrap();
    assert_eq!(local, json!({"grid": grid, "audition": false}));
    assert_eq!(file_of(&fixture), json!({"loop": loop_, "grid": grid, "audition": false}));
    assert_eq!(graph.state().unwrap()["local"], json!({"grid": grid, "audition": false}), "the state has them, not the loop");

    // A null removes one; the loop's own route keeps the settings.
    graph.set_view(token(&graph), map(json!({"audition": null}))).unwrap();
    graph.set_loop(Some((8.0, 16.0)), Some(false)).unwrap();
    assert_eq!(file_of(&fixture), json!({"loop": {"start": 8.0, "end": 16.0, "on": false}, "grid": grid}));

    // The loop, a name it doesn't know, and another song's session are refused, and change nothing.
    let e = graph.set_view(token(&graph), map(json!({"loop": loop_}))).unwrap_err().to_string();
    assert!(e.contains("its own route"), "{e}");
    let e = graph.set_view(token(&graph), map(json!({"zoom": 48, "colour": "red"}))).unwrap_err().to_string();
    assert!(e.contains("\"colour\" isn't a view setting"), "{e}");
    assert!(graph.set_view(token(&graph) + 1, map(json!({"zoom": 48}))).is_err());
    assert_eq!(graph.state().unwrap()["local"], json!({"grid": grid}));

    // Opened again: read from the file; what the page doesn't keep stays in the file only.
    graph.close();
    let mut on_disk = file_of(&fixture);
    on_disk["colour"] = json!("red");
    fixture.put(".music2/view.json", &on_disk.to_string());
    graph.open(&fixture.0).unwrap();
    assert_eq!(graph.state().unwrap()["local"], json!({"grid": grid}));
    graph.set_view(token(&graph), map(json!({"zoom": 36}))).unwrap();
    assert_eq!(file_of(&fixture)["colour"], json!("red"), "entries it doesn't know are kept");

    // A read-only window keeps them in memory: nothing is written.
    graph.close();
    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    graph.open(&fixture.0).unwrap();
    assert!(graph.read_only().is_some());
    let before = fixture.read(".music2/view.json");
    let local = graph.set_view(token(&graph), map(json!({"zoom": 48}))).unwrap();
    assert_eq!((&local["zoom"], &graph.state().unwrap()["local"]["zoom"]), (&json!(48), &json!(48)));
    assert_eq!(fixture.read(".music2/view.json"), before);
    drop(held);
}
