//! The page's view settings (Q16): merged into `.music2/view.json` beside the loop, a null taking
//! a key out, only the known keys, in memory only in a read-only window.

use serde_json::{Map, Value, json};

use crate::graph::Graph;

/// A one-track song in a fresh folder, with `view` as its `.music2/view.json`.
fn song(name: &str, view: &Value) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-view-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".music2")).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.2\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
         const kick = track('kick', () => osc.sine(55));\nexport default song(() => kick.out);\n",
    )
    .unwrap();
    std::fs::write(dir.join(".music2/view.json"), view.to_string()).unwrap();
    dir
}

fn file(dir: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join(".music2/view.json")).unwrap()).unwrap()
}

fn set(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn view_settings_merge_and_keep_the_loop() {
    let looped = json!({"start": 0.0, "end": 4.0, "on": false});
    let dir = song("merge", &json!({"loop": looped, "zoom": 36.0, "newer": 1}));
    let graph = Graph::silent();
    let state = graph.open(&dir).unwrap();
    let session = state["session"].as_u64().unwrap();
    // As read when the song opened: the known keys only, not the loop.
    assert_eq!(state["local"], json!({"zoom": 36.0}), "{:#}", state["local"]);

    let answer = graph.set_view(session, set(json!({"zoom": 54.0, "heights": {"kick": 72}}))).unwrap();
    assert_eq!(answer["local"], json!({"zoom": 54.0, "heights": {"kick": 72}}));
    assert_eq!(graph.state().unwrap()["local"], answer["local"]);
    assert_eq!(file(&dir), json!({"loop": looped, "zoom": 54.0, "newer": 1, "heights": {"kick": 72}}), "the loop and a newer version's key stay");

    // A null takes a key out.
    graph.set_view(session, set(json!({"zoom": null}))).unwrap();
    assert_eq!(graph.state().unwrap()["local"], json!({"heights": {"kick": 72}}));
    assert_eq!(file(&dir), json!({"loop": looped, "newer": 1, "heights": {"kick": 72}}));

    // The loop has its own route; a key it doesn't know is refused, and nothing is written.
    for bad in [json!({"loop": {"start": 1, "end": 2}}), json!({"zoom": 24, "colour": "red"})] {
        let e = graph.set_view(session, set(bad)).unwrap_err().to_string();
        assert!(e.contains("isn't a view setting"), "{e}");
    }
    assert_eq!(file(&dir), json!({"loop": looped, "newer": 1, "heights": {"kick": 72}}));
    // Another session's settings never land here.
    assert!(graph.set_view(session + 1, set(json!({"zoom": 24}))).is_err());

    // Opened again: as kept.
    graph.close();
    assert_eq!(graph.open(&dir).unwrap()["local"], json!({"heights": {"kick": 72}}));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_read_only_window_keeps_view_settings_in_memory() {
    let dir = song("ro", &json!({"zoom": 36.0}));
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let graph = Graph::silent();
    let session = graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    assert!(graph.read_only().is_some());
    let answer = graph.set_view(session, set(json!({"zoom": 81.0, "follow": false}))).unwrap();
    assert_eq!(answer["local"], json!({"zoom": 81.0, "follow": false}));
    assert_eq!(graph.state().unwrap()["local"], answer["local"]);
    assert_eq!(file(&dir), json!({"zoom": 36.0}), "the file is the writer's");
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}
