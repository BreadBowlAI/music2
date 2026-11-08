//! A node's key (CARD-25): the key its call had in the view before, found again by ID-13 through
//! each edit, as a scope follows its call (TAP-9); a new one for a call no view had.

use std::collections::HashSet;

use serde_json::Value;

use super::super::Graph;
use super::super::tests::Fixture;

/// A saw bass, a sine kick under an LFO and a square pad, into std's mixer.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { lfo } from 'std/control';\nimport { osc } from 'std/dsp';\n\
                    import { scale } from 'std/math';\nimport { mixer } from 'std/mix';\n\
                    const bass = track('bass', () => osc.saw(110));\n\
                    const kick = track('kick', () => scale(osc.sine(55), lfo(2)));\n\
                    const pad = track('pad', () => osc.square(880));\n\
                    export default song(() => mixer({ bass: bass.out, kick: kick.out, pad: pad.out }));\n";

struct Rig {
    _fixture: Fixture,
    graph: Graph,
}

impl Rig {
    fn new(song: &str) -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", song);
        let graph = Graph::silent().stored();
        graph.open(&fixture.0).unwrap();
        Rig { _fixture: fixture, graph }
    }

    fn token(&self) -> u64 {
        self.graph.state().unwrap()["session"].as_u64().unwrap()
    }

    fn nodes(&self) -> Vec<Value> {
        let state = self.graph.state().unwrap();
        state["view"]["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap().clone()).collect()
    }

    /// The id and the key of the one node labelled `label`.
    fn node(&self, label: &str) -> (String, String) {
        let found: Vec<Value> = self.nodes().into_iter().filter(|n| n["label"] == label).collect();
        assert_eq!(found.len(), 1, "one {label}: {found:?}");
        let n = &found[0];
        (n["id"].as_str().unwrap().to_string(), n["key"].as_str().expect("every node has a key").to_string())
    }

    fn write(&self, from: &str, to: &str) {
        self.graph.write_file(Some(self.token()), "song.ts", from, to).unwrap();
    }
}

/// `SONG` with a hats track written before the bass: every call after it in the file gets another id.
fn hats_first() -> String {
    SONG.replace("const bass", "const hats = track('hats', () => osc.tri(880));\nconst bass")
        .replace("pad: pad.out })", "pad: pad.out, hats: hats.out })")
}

// CARD-25: an edit that writes a call before the saw gives the saw's node another id, never
// another key; an edit of the saw's own call keeps its key; and an undo of it too.
#[test]
fn a_nodes_key_follows_its_call_across_an_edit() {
    let rig = Rig::new(SONG);
    let (id, key) = rig.node("osc.saw");
    let more = hats_first();
    rig.write(SONG, &more);
    let (moved, kept) = rig.node("osc.saw");
    assert_ne!(moved, id, "the edit gave the saw another id");
    assert_eq!(kept, key, "and kept its key");
    // The call itself changed: the same call, the same key.
    let square = more.replace("osc.saw(110)", "osc.square(110)");
    rig.write(&more, &square);
    let bass: Vec<Value> = rig.nodes().into_iter().filter(|n| n["label"] == "osc.square" && n["id"] == moved).collect();
    assert_eq!(bass.len(), 1, "the bass's call, now a square, at the saw's place");
    assert_eq!(bass[0]["key"], key.as_str(), "the same call keeps its key");
    // Back as it was, by the editor's undo.
    rig.graph.undo(rig.token()).unwrap();
    rig.graph.undo(rig.token()).unwrap();
    let (back, again) = rig.node("osc.saw");
    assert_eq!(back, id, "undone, its first id");
    assert_eq!(again, key, "and its key still");
}

// CARD-25: a call no view had gets a key no node of the song has had, even where it takes another
// call's old id; a node that isn't a call has its id as its key.
#[test]
fn a_new_call_gets_a_key_no_node_had() {
    let rig = Rig::new(SONG);
    let before: HashSet<String> = rig.nodes().iter().map(|n| n["key"].as_str().unwrap().to_string()).collect();
    let (saw_id, _) = rig.node("osc.saw");
    let more = hats_first();
    rig.write(SONG, &more);
    let (hats_id, hats_key) = rig.node("osc.tri");
    assert_eq!(hats_id, saw_id, "the hats' call took the saw's old id");
    assert!(!before.contains(&hats_key), "a key no node had: {hats_key} in {before:?}");
    for n in rig.nodes() {
        let key = n["key"].as_str().unwrap();
        if n["kind"] != "call" {
            assert_eq!(key, n["id"].as_str().unwrap(), "a {} node's key is its id", n["kind"]);
        }
    }
    let keys: Vec<String> = rig.nodes().iter().filter(|n| n["kind"] == "call").map(|n| n["key"].as_str().unwrap().to_string()).collect();
    let unique: HashSet<&String> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len(), "no two calls share a key: {keys:?}");
}

/// Two filters written around a saw, the Lowpass outside.
const FILTERS: &str = "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { highpass, lowpass } from 'std/filter';\n\
                       import { mixer } from 'std/mix';\n\
                       const bass = track('bass', () => lowpass(highpass(osc.saw(110), { cutoff: 80 }), { cutoff: 900 }));\n\
                       export default song(() => mixer({ bass: bass.out }));\n";

impl Rig {
    fn revision(&self) -> u64 {
        self.graph.state().unwrap()["view"]["revision"].as_u64().unwrap()
    }
}

// CARD-25 (review Z4): ⋯ ▸ Move swaps two calls, `inner(outer(x))` for `outer(inner(x))`; each
// keeps its key, though its `(` doesn't survive the diff.
#[test]
fn a_moved_effect_keeps_its_key() {
    let rig = Rig::new(FILTERS);
    let (lp, lp_key) = rig.node("lowpass");
    let (_, hp_key) = rig.node("highpass");
    rig.graph.move_effect(rig.token(), rig.revision(), &lp, true).unwrap();
    let text = std::fs::read_to_string(rig._fixture.0.join("song.ts")).unwrap();
    assert!(text.contains("highpass(lowpass(osc.saw(110)"), "moved: {text}");
    assert_eq!(rig.node("lowpass").1, lp_key, "the Lowpass keeps its key");
    assert_eq!(rig.node("highpass").1, hp_key, "and the Highpass its own");
}

// CARD-25 (review Z4): Remove, then undo, brings the call back as the code it was: its key comes
// back with it; and redo, then undo again, the same.
#[test]
fn an_undone_remove_brings_the_key_back() {
    let rig = Rig::new(FILTERS);
    let (lp, lp_key) = rig.node("lowpass");
    rig.graph.remove_effect(rig.token(), rig.revision(), &lp).unwrap();
    assert!(rig.nodes().iter().all(|n| n["label"] != "lowpass"), "removed");
    rig.graph.undo(rig.token()).unwrap();
    assert_eq!(rig.node("lowpass").1, lp_key, "undone, the Lowpass has its key again");
    rig.graph.redo(rig.token()).unwrap();
    rig.graph.undo(rig.token()).unwrap();
    assert_eq!(rig.node("lowpass").1, lp_key, "after a redo and an undo too");
}
