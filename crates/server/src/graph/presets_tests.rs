//! A plugin's presets (HOST-8, FMT-22, gaps #26): saved from its live instance, loaded as one Undo
//! step that reaches the instance that plays, and its own loaded on an instance made for it. The
//! probe runs in this process, so its records can be read; the test is the device's thread. Your
//! Presets folder is the scratch HOME's: each test names its presets apart.

use std::sync::Arc;
use std::time::{Duration, Instant};

use m2_editor::presets::{self, Preset};
use m2_play::{Driver, Player};
use m2_plugins::probe::{self, GAIN};
use m2_plugins::{Handle, MainCall, Reply};

use super::super::tests::Fixture;
use crate::graph::Graph;

const NOTES: &str = r#"{"clips": [{"id": "a", "start": 0, "length": 64, "notes": [{"pitch": "C4", "start": 0, "length": 60}]}]}"#;

/// A song whose keyed probe (key `lead`, its state saved with `tag`) has its gain written in code
/// (`gain_1: <gain>`), open on a driven player, playing.
struct Rig {
    fixture: Fixture,
    state: Arc<crate::AppState>,
    driver: Driver,
}

impl Rig {
    fn new(gain: &str, tag: &str) -> Rig {
        probe::install();
        let fixture = Fixture::new();
        fixture.put(
            "song.ts",
            &format!(
                "import {{ plugin, song, track }} from '@m2/core';\n\
                 const p = track('p', ({{ lane }}) =>\n  plugin('clap:m2.probe', {{ notes: lane.notes('n'), key: 'lead', gain_1: {gain} }}),\n);\n\
                 export default song(() => p.out);\n"
            ),
        );
        fixture.put("data/p/n.json", NOTES);
        std::fs::write(fixture.0.join("data/p/lead.state"), probe::state_with(tag, 1.0, 0.1)).unwrap();
        fixture.put("data/p/lead.state.json", r#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#);
        let (p, driver) = Player::driven(48_000);
        let state = Arc::new(crate::AppState::new(String::new(), Graph::with_player(p)));
        state.graph.open(&fixture.0).unwrap();
        state.graph.transport("play").unwrap();
        let mut r = Rig { fixture, state, driver };
        r.fill(4);
        r
    }

    fn graph(&self) -> &Graph {
        &self.state.graph
    }

    fn fill(&mut self, n: usize) {
        for _ in 0..n {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
        }
    }

    fn tick(&self) {
        self.graph().inspect(|a| {
            a.catch_up();
        });
    }

    fn handle(&mut self) -> Handle {
        self.driver.engine().unwrap().current().unwrap().plugin_handles()[0]
    }

    fn stats(&mut self) -> Arc<probe::Stats> {
        probe::stats(self.handle()).unwrap()
    }

    fn tag(&mut self) -> String {
        self.stats().tag.lock().unwrap().clone()
    }

    fn gain(&mut self) -> f64 {
        match m2_plugins::host().main_call(self.handle(), MainCall::Value(GAIN)) {
            Ok(Reply::Value(Some(v))) => v,
            other => panic!("{other:?}"),
        }
    }

    fn session(&self) -> u64 {
        self.graph().state().unwrap()["session"].as_u64().unwrap()
    }

    fn revision(&self) -> u64 {
        self.graph().state().unwrap()["view"]["revision"].as_u64().unwrap()
    }

    fn song(&self) -> String {
        std::fs::read_to_string(self.fixture.0.join("song.ts")).unwrap()
    }

    fn saved(&self) -> Vec<u8> {
        std::fs::read(self.fixture.0.join("data/p/lead.state")).unwrap()
    }

    /// The probe's call, as its card names it (`song.ts#…`).
    fn node(&self) -> String {
        let state = self.graph().state().unwrap();
        state["view"]["modules"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["nodes"].as_array().unwrap())
            .find(|n| n["plugin"].is_string())
            .and_then(|n| n["id"].as_str())
            .expect("the probe has a card")
            .to_string()
    }
}

/// Your presets for the probe.
fn yours() -> std::path::PathBuf {
    m2_os::dirs::presets().unwrap().join(presets::folder("clap:m2.probe"))
}

fn store(name: &str, sound: &str, gain: f64, state: &[u8]) {
    let p = Preset {
        name: name.into(),
        sound: sound.into(),
        values: [("1".to_string(), gain)].into(),
        plugin_version: Some("1.0.0".into()),
        state: Some(m2_song::tx::base64(state)),
    };
    let dir = m2_os::dirs::presets().unwrap().join(presets::folder(sound));
    presets::store(&dir, &p, true).unwrap();
}

// HOST-8, FMT-22: a plugin's preset holds its live instance's state and the parameters its code
// writes, by CLAP id, and the plugin's version. No song file changes.
#[test]
fn saving_a_plugin_preset_keeps_its_state_and_code_values() {
    let r = Rig::new("0.5", "warm");
    let before = r.song();
    let saved = r.graph().save_preset(r.session(), None, &r.node(), "Saved warm", true).unwrap();
    assert_eq!(saved["name"], "Saved warm");
    let p = Preset::parse(&std::fs::read_to_string(yours().join("Saved warm.json")).unwrap()).unwrap();
    assert_eq!((p.sound.as_str(), p.plugin_version.as_deref()), ("clap:m2.probe", Some("1.0.0")));
    assert_eq!(p.values, [("1".to_string(), 0.5)].into(), "the gain its code writes");
    assert_eq!(p.blob().unwrap(), probe::state_with("warm", 0.5, 0.1), "the state the probe plays");
    assert_eq!(r.song(), before, "no song file changes");
}

// HOST-8, RENDER-26b, HOME-6: loading one is one Tx: the state, its sidecar and the literal. The
// probe that plays loads it, and marks itself dirty after (`dirty-at`), which saves nothing more.
// One Undo puts all three back, and the old state plays again.
#[test]
fn loading_a_plugin_preset_is_one_undo_step() {
    let mut r = Rig::new("0.5", "warm");
    let (song, state) = (r.song(), r.saved());
    let cold = probe::state_with("cold dirty-at-1-100000", 0.25, 0.2);
    store("Loads cold", "clap:m2.probe", 0.25, &cold);
    let loads = r.stats().main_calls().iter().filter(|c| c.what == "load").count();
    let done = r.graph().load_preset(r.session(), r.revision(), &r.node(), "yours", "Loads cold").unwrap();
    assert_eq!(done["changed"], true);
    assert!(r.song().contains("gain_1: 0.25"), "the literal: {}", r.song());
    assert_eq!(r.saved(), cold, "the state");
    r.fill(8);
    assert_eq!(r.stats().main_calls().iter().filter(|c| c.what == "load").count(), loads + 1, "the probe that plays loads it");
    assert_eq!((r.tag(), r.gain()), ("cold dirty-at-1-100000".to_string(), 0.25));
    // Dirty at every call: a second of quiet after, its save matches the file.
    r.tick();
    std::thread::sleep(crate::graph::plugin_window::QUIET + Duration::from_millis(100));
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(600) {
        r.tick();
        std::thread::sleep(Duration::from_millis(50));
    }
    r.graph().undo(r.session()).unwrap();
    assert_eq!((r.song(), r.saved()), (song, state), "one Undo puts the code and the state back");
    assert_eq!(r.graph().state().unwrap()["can_undo"], false, "one step");
    r.fill(8);
    assert_eq!((r.tag(), r.gain()), ("warm".to_string(), 0.5), "the old state plays again");
}

// HOST-8: the plugin's own presets are listed, and one loads as one Undo step: on an instance made
// for it and destroyed after, never the one that plays, until the Tx commits.
#[test]
fn the_plugins_own_preset_loads_on_a_scratch_instance() {
    let mut r = Rig::new("0.8", "warm");
    let list = r.graph().presets(r.session(), None, &r.node()).unwrap();
    assert_eq!(list["label"], "m2 probe");
    assert_eq!(list["plugin"], serde_json::json!([{"name": "Bright"}, {"name": "Dark"}]));
    let done = r.graph().load_preset(r.session(), r.revision(), &r.node(), "plugin", "Dark").unwrap();
    assert_eq!(done["changed"], true);
    assert!(r.song().contains("gain_1: 0.5"), "Dark's gain, in the code: {}", r.song());
    assert_eq!(r.saved(), probe::state_with("dark", 0.5, 0.05));
    let made: Vec<_> = probe::tagged("dark").into_iter().filter(|s| s.main_calls().iter().any(|c| c.what == "preset-load")).collect();
    assert_eq!(made.len(), 1, "one instance was made for it");
    let t = Instant::now();
    while !made[0].main_calls().iter().any(|c| c.what == "destroy") && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(made[0].main_calls().iter().any(|c| c.what == "destroy"), "and destroyed after");
    assert!(r.stats().main_calls().iter().all(|c| c.what != "preset-load"), "never the one that plays");
    r.fill(8);
    assert_eq!((r.tag(), r.gain()), ("dark".to_string(), 0.5), "the probe that plays has Dark");
}

// HOST-8: a preset for another plugin is refused in words, and nothing is written.
#[test]
fn a_preset_for_another_plugin_is_refused_in_words() {
    let r = Rig::new("0.5", "warm");
    let (song, state) = (r.song(), r.saved());
    let (at, node) = (r.revision(), r.node());
    let other = Preset {
        name: "Other".into(),
        sound: "clap:m2.probe.fails-state".into(),
        values: Default::default(),
        plugin_version: None,
        state: Some(m2_song::tx::base64(b"x")),
    };
    let why = r
        .graph()
        .inspect(move |a| {
            let o = a.open.as_mut().unwrap();
            presets::load_plugin(&mut o.session, at, &node, &other, b"x".to_vec()).map(|_| ()).map_err(|e| e.to_string())
        })
        .unwrap_err();
    assert_eq!(why, "This preset is for m2.probe.fails-state, not m2 probe.");
    assert_eq!((r.song(), r.saved()), (song, state), "nothing written");
    assert_eq!(r.graph().state().unwrap()["can_undo"], false);
}
