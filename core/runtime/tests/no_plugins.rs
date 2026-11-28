//! HOST-1, FMT-18a: a runtime built without its `plugins` feature has no CLAP host, as the web
//! player's will be (docs/designs/live/web-engine.md). A song that uses a plugin still builds and
//! plays: each plugin is `plugin.missing`, in words that say plugins can't play here, and its node
//! plays as a missing one does. Run by `cargo test -p m2-runtime --no-default-features --features
//! wasmtime --test no_plugins` (`ci/pr.sh rust`, `ci/check.sh`); with the feature on, this file is empty.
#![cfg(not(feature = "plugins"))]

use std::path::Path;

use m2_ir::{Ir, Severity};
use m2_runtime::Renderer;
use m2_runtime::packages::PackageSet;
use m2_song::DataSet;

/// A song whose output is plugin `id`: an effect on a sine when `effect`, else an instrument.
fn song(id: &str, effect: bool) -> Ir {
    let mut nodes = vec![serde_json::json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {}})];
    let mut inputs = serde_json::json!({"mix_3": "c"});
    if effect {
        inputs["in"] = serde_json::json!("o");
    }
    nodes.push(serde_json::json!({"id": "c", "op": "const", "value": 0.5, "origin": {"site": 0}}));
    nodes.push(serde_json::json!({"id": "p", "op": "plugin", "plugin": id, "inputs": inputs, "src": "song.ts:3:5"}));
    let exports = if effect { serde_json::json!({"out": "p", "dry": "o"}) } else { serde_json::json!({"out": "p"}) };
    serde_json::from_value(serde_json::json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "nodes": nodes, "exports": exports}]}))
    .unwrap()
}

fn std_set() -> PackageSet {
    let mut set = PackageSet::default();
    set.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    set
}

fn render(ir: &Ir, set: &PackageSet) -> Vec<[f32; 2]> {
    let flat = m2_ir::check(ir, set).unwrap_or_else(|e| panic!("{e:?}"));
    Renderer::new(&flat, &DataSet::empty(), set, 48_000).unwrap().render(0.1).unwrap()
}

#[test]
fn a_plugin_song_is_told_plugins_cant_play_here() {
    for (effect, plays) in [(true, "passes its audio through unchanged"), (false, "plays silence")] {
        let mut set = std_set();
        let mut ir = song("clap:org.example.reverb", effect);
        let diags = set.bind_plugins(&mut ir);
        let said: Vec<_> = diags.iter().map(|d| (d.code, d.severity, d.message.as_str())).collect();
        let words = format!(
            "this song uses the CLAP plugin `org.example.reverb`, and plugins can't play here: this build of music2 has no \
             plugin host. Its node {plays}; its code and saved state are kept as they are"
        );
        assert_eq!(said, [("plugin.missing", Severity::Error, words.as_str())]);
        assert!(set.plugin_missing("clap:org.example.reverb"));

        // It plays as a missing plugin does (FMT-18a): the effect lets the sine through, the
        // instrument is silent.
        let out = render(&ir, &set);
        if effect {
            let mut dry = ir.clone();
            dry.root.export = "dry".into();
            let dry = render(&dry, &set);
            assert!(dry.iter().any(|f| f[0] != 0.0), "the sine is silent");
            assert_eq!(out, dry);
        } else {
            assert!(out.iter().all(|f| *f == [0.0, 0.0]));
        }
    }
}
