//! Plugin windows on the server (HOST-4, HOME-5, HOME-5a, HOME-6, MON-6): what the probe's
//! scripted window does (`gesture-at`, `preset-at`, `dirty-at`, see `m2_plugins::probe`) reaches
//! a literal Tx, a latch, a set-back or an autosave. The probe runs in this process, so its
//! records can be read; the test is the device's thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use m2_play::{Driver, Player};
use m2_plugins::probe::{self, GAIN, LEVEL};
use m2_plugins::{Handle, MainCall, Reply};
use serde_json::Value;

use super::super::tests::Fixture;
use crate::graph::Graph;

/// Two notes held for 60 beats, and a control lane `g` at 0.2.
const NOTES: &str = r#"{"clips": [{"id": "a", "start": 0, "length": 64, "notes": [{"pitch": "C4", "start": 0, "length": 60}, {"pitch": "G4", "start": 0, "length": 60}]}]}"#;
const LANE: &str = r#"{"unit": "scalar", "clips": [{"id": "r", "start": 0, "length": 64, "points": [[0, 0.2], [64, 0.2]]}]}"#;

/// A song whose keyed probe (key `lead`, state saved with `tag`, gain 1, level 0.1) has its gain
/// set by `gain` (code), open in the app's state on a driven player, playing, four blocks in.
struct Rig {
    fixture: Fixture,
    state: Arc<crate::AppState>,
    driver: Driver,
}

fn song(gain: &str) -> String {
    format!(
        "import {{ plugin, song, track }} from '@m2/core';\n\
         const p = track('p', ({{ lane }}) =>\n  plugin('clap:m2.probe', {{ notes: lane.notes('n'), key: 'lead', gain_1: {gain} }}),\n);\n\
         export default song(() => p.out);\n"
    )
}

impl Rig {
    fn new(gain: &str, tag: &str) -> Rig {
        probe::install();
        let fixture = Fixture::new();
        fixture.put("song.ts", &song(gain));
        fixture.put("data/p/n.json", NOTES);
        fixture.put("data/p/g.json", LANE);
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

    /// `n` device buffers of a block: a call of the probe each.
    fn fill(&mut self, n: usize) {
        for _ in 0..n {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
        }
    }

    /// Fill until the probe has made `calls` calls.
    fn fill_to(&mut self, calls: u64) {
        let s = self.stats();
        while s.process.load(std::sync::atomic::Ordering::Relaxed) < calls {
            self.fill(1);
        }
    }

    /// A tick of the session: what the watcher's catch-up does every 300 ms.
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

    /// A parameter's value as the plugin reads it, as the engine's f32 samples carry it.
    fn param(&mut self, id: u32) -> f32 {
        match m2_plugins::host().main_call(self.handle(), MainCall::Value(id)) {
            Ok(Reply::Value(Some(v))) => v as f32,
            other => panic!("{other:?}"),
        }
    }

    fn session(&self) -> u64 {
        self.graph().state().unwrap()["session"].as_u64().unwrap()
    }

    fn song(&self) -> String {
        std::fs::read_to_string(self.fixture.0.join("song.ts")).unwrap()
    }

    fn saved(&self) -> Vec<u8> {
        std::fs::read(self.fixture.0.join("data/p/lead.state")).unwrap()
    }

    /// The probe's node in the graph state's `plugins`, and its name there.
    fn plugin(&self) -> (String, Value) {
        let state = self.graph().state().unwrap();
        let node = state["view"]["modules"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["nodes"].as_array().unwrap())
            .find_map(|n| n["plugin"].as_str())
            .expect("the probe's view node names its plugin node")
            .to_string();
        (node.clone(), state["plugins"][&node].clone())
    }

    fn loads(&mut self) -> usize {
        self.stats().main_calls().iter().filter(|c| c.what == "load").count()
    }
}

// HOME-5, HOME-5a, HOST-4: a gesture in the window on a parameter written in code holds it: the
// node never sends the code's value meanwhile, though the code still says 0.5; its end commits one
// Tx with the window's value, one Undo step, which plays from then on.
#[test]
fn a_gesture_on_a_literal_commits_one_tx() {
    let mut r = Rig::new("0.5", "gesture-at-20-60:1=0.8");
    assert_eq!(r.param(GAIN), 0.5f32);
    r.fill_to(30);
    r.tick();
    assert!(r.song().contains("gain_1: 0.5"), "nothing written during the gesture");
    let leases = r.graph().state().unwrap()["leases"].as_array().unwrap().len();
    assert_eq!(leases, 1, "the literal is leased while the gesture lasts");
    for _ in 0..5 {
        r.fill(4);
        assert_eq!(r.param(GAIN), 0.8f32, "the code's 0.5 isn't sent during the gesture");
    }
    r.fill_to(70);
    r.tick();
    assert!(r.song().contains("gain_1: 0.8"), "the gesture's end wrote the literal: {}", r.song());
    assert_eq!(r.graph().state().unwrap()["leases"].as_array().unwrap().len(), 0);
    r.fill(8);
    assert_eq!(r.param(GAIN), 0.8f32, "the new literal plays");
    let session = r.session();
    r.graph().undo(session).unwrap();
    assert!(r.song().contains("gain_1: 0.5"), "one Undo step puts it back");
    r.fill(8);
    assert_eq!(r.param(GAIN), 0.5f32, "and the code's value plays again");
}

// MON-6, HOST-4: a gesture on a parameter a lane drives latches it: the window's value stays after
// the gesture, the box says so, an export never hears it, and Re-enable plays the lane again.
#[test]
fn a_gesture_on_a_lane_driven_parameter_latches_it() {
    let mut r = Rig::new("lane.control('g', 'scalar', 0.5)", "gesture-at-20-30:1=0.9");
    assert_eq!(r.param(GAIN), 0.2f32, "the lane plays");
    // The first tick publishes the boxes; then there's nothing new to tell.
    r.tick();
    r.graph().plugins_changed();
    r.tick();
    assert!(!r.graph().plugins_changed(), "nothing new to tell");
    r.fill_to(60);
    r.tick();
    assert!(r.graph().plugins_changed(), "the watcher tells the windows the box changed");
    assert_eq!(r.param(GAIN), 0.9f32, "latched at the window's value after the gesture");
    let (node, plugin) = r.plugin();
    let latched = plugin["latched"].as_array().unwrap();
    assert!(latched.len() == 1 && latched[0]["id"] == GAIN && latched[0]["name"] == "Gain", "the box says so: {plugin}");
    assert!(r.song().contains("lane.control('g'"), "nothing written");
    // An export doesn't borrow it, and plays the lane.
    let out = export_now(&mut r);
    let (_, _, fresh, _) = r.graph().render(m2_editor::render::Length::Song).unwrap();
    let fresh_wav = r.fixture.0.join("fresh.wav");
    fresh.write(&fresh_wav, |_| {}).unwrap();
    let samples = |p: &PathBuf| hound::WavReader::open(p).unwrap().into_samples::<i32>().map(Result::unwrap).collect::<Vec<i32>>();
    assert!(samples(&out) == samples(&fresh_wav), "the export plays the lane, as m2 render does");
    r.fill(8);
    assert_eq!(r.param(GAIN), 0.9f32, "still latched live");
    let session = r.session();
    r.graph().plugin_reenable(session, &node, None).unwrap();
    r.fill(8);
    assert_eq!(r.param(GAIN), 0.2f32, "Re-enable: the lane plays again");
    assert_eq!(r.plugin().1["latched"].as_array().unwrap().len(), 0);
}

// HOME-6, FMT-17: a storm of mark_dirty and a change to a parameter code doesn't set make one Tx,
// after a second of quiet: the blob and its sidecar, one Undo step. The save isn't loaded back
// into the instance; an undo loads the old blob.
#[test]
fn a_dirty_plugin_autosaves_its_state_once() {
    let mut r = Rig::new("0.5", "dirty-at-10-40 preset-at-15:2=0.3");
    let before = r.saved();
    r.fill_to(45);
    r.tick();
    assert_eq!(r.saved(), before, "not saved before a second of quiet");
    let loads = r.loads();
    std::thread::sleep(super::QUIET + Duration::from_millis(100));
    let t = Instant::now();
    while r.saved() == before {
        assert!(t.elapsed() < Duration::from_secs(10), "never saved");
        r.tick();
        r.fill(1);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(r.saved(), probe::state_with("dirty-at-10-40 preset-at-15:2=0.3", 0.5, 0.3), "its state now");
    assert!(r.fixture.0.join("data/p/lead.state.json").exists(), "with its sidecar");
    r.fill(8);
    r.tick();
    r.fill(8);
    assert_eq!(r.loads(), loads, "the save isn't loaded back into the instance");
    let session = r.session();
    r.graph().undo(session).unwrap();
    assert_eq!(r.saved(), before, "one Undo step");
    assert_eq!(r.graph().state().unwrap()["can_undo"], false, "and only one");
    r.fill(8);
    assert_eq!(r.loads(), loads + 1, "the undo loads the old blob");
    assert_eq!(r.param(LEVEL), 0.1f32);
}

// HOME-5a: a change the plugin makes outside a gesture (a preset loaded in its own browser) to a
// parameter the code sets is set back at the next block, and said once.
#[test]
fn a_preset_change_outside_a_gesture_is_set_back() {
    let mut r = Rig::new("0.5", "preset-at-20:1=0.9");
    r.fill_to(21);
    r.fill(2);
    assert_eq!(r.param(GAIN), 0.5f32, "set back");
    r.tick();
    let told: Vec<String> = r.graph().expire().into_iter().filter(|t| t.contains("plugin.param-reverted")).collect();
    assert!(told.len() == 1 && told[0].contains("Gain") && told[0].contains("m2 probe (p)"), "{told:?}");
    r.fill(8);
    r.tick();
    assert!(r.graph().expire().iter().all(|t| !t.contains("plugin.param-reverted")), "said once");
    assert!(r.song().contains("gain_1: 0.5"));
}

// RENDER-1, HOME-6: an export first saves a state changed in the window that waits for its quiet
// second, so the file holds it and the export borrows the instance that plays it.
#[test]
fn an_export_commits_a_pending_autosave_first() {
    let mut r = Rig::new("0.5", "preset-at-10:2=0.3");
    let before = r.saved();
    r.fill_to(15);
    r.tick();
    assert_eq!(r.saved(), before, "waiting for its quiet second");
    let made = r.graph().inner.owner.made();
    export_now(&mut r);
    assert_eq!(r.saved(), probe::state_with("preset-at-10:2=0.3", 0.5, 0.3), "saved before the export");
    assert_eq!(r.graph().inner.owner.made(), made, "the export borrowed the instance");
}

/// Export the song as the app does, filling as the device does until it is done.
fn export_now(r: &mut Rig) -> PathBuf {
    let out = r.fixture.0.join("out.wav");
    crate::export::start(&r.state, Some(out.clone())).unwrap();
    let t = Instant::now();
    while r.state.exports.running() {
        assert!(t.elapsed() < Duration::from_secs(30), "the export never ended");
        r.fill(1);
        std::thread::sleep(Duration::from_micros(200));
    }
    out
}

// AGENT-3, HOME-6: a plugin whose window changed its sound, not saved yet, holds its state: an
// agent's write of that state is busy, and says so; its other files are free.
#[test]
fn a_plugin_window_not_yet_saved_makes_its_state_busy() {
    use crate::caller::{Caller, as_caller};
    use m2_song::tx::{Base, Expect, Op, Payload, Tx, TxPath};
    let mut r = Rig::new("0.5", "dirty-at-10-400");
    r.fill_to(20);
    let t = Instant::now();
    while r.graph().inspect(|a| a.open.as_ref().unwrap().windows.unsaved().is_empty()) {
        assert!(t.elapsed() < Duration::from_secs(5), "the window's change was heard");
        r.fill(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    let state = r.saved();
    let path = TxPath::new("data/p/lead.state").unwrap();
    let mut tx = Tx::new(
        "tx-agent",
        ("a".into(), "b".into()),
        vec![Op::Write { file: path.clone(), bytes: Payload::Bytes(probe::state_with("agent", 1.0, 0.2).into()) }],
        Expect::Identical,
    );
    tx.base.insert(path, Base::Sha(m2_song::Sha::of(&state)));
    let agent = Caller::agent(Some("Claude"), None);
    let session = r.session();
    let (status, body) = as_caller(agent.clone(), || r.graph().tx_commit(Some(session), &tx.to_json(), None, &[])).unwrap();
    assert_eq!(
        (status, &body["busy"]["what"], &body["busy"]["file"]),
        (423, &serde_json::json!("plugin"), &serde_json::json!("data/p/lead.state")),
        "{body}"
    );
    assert!(body["busy"]["words"].as_str().unwrap().contains("in its window"), "{body}");
    assert_eq!(r.saved(), state, "nothing changed");
    let notes = TxPath::new("data/p/n.json").unwrap();
    let lane = m2_song::LaneAddr::new("p", "n").unwrap();
    let confined = Expect::Confined(m2_song::tx::Confined { lanes: vec![lane], ..Default::default() });
    let mut other = Tx::new(
        "tx-agent-2",
        ("a".into(), "b".into()),
        vec![Op::Write { file: notes.clone(), bytes: Payload::Text(NOTES.replace("60}", "30}").into()) }],
        confined,
    );
    other.base.insert(notes, Base::text(NOTES));
    let (status, body) = as_caller(agent, || r.graph().tx_commit(Some(session), &other.to_json(), None, &[])).unwrap();
    assert_eq!(status, 200, "{body}");
}

/// Fill until the session heard the window's change: it changed the plugin's state, not saved yet.
fn until_unsaved(r: &mut Rig) {
    let t = Instant::now();
    while r.graph().inspect(|a| a.open.as_ref().unwrap().windows.unsaved().is_empty()) {
        assert!(t.elapsed() < Duration::from_secs(5), "the window's change was heard");
        r.fill(1);
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// An agent's Tx writing the probe's state at `file`, over `was`, as `m2 plugin-state` sends it.
fn state_tx(file: &str, was: &[u8]) -> m2_song::tx::Tx {
    use m2_song::tx::{Base, Confined, Expect, Op, Payload, Tx, TxPath};
    let path = TxPath::new(file).unwrap();
    let mut tx = Tx::new(
        "tx-agent",
        ("a".into(), "b".into()),
        vec![Op::Write { file: path.clone(), bytes: Payload::Bytes(probe::state_with("agent", 1.0, 0.2).into()) }],
        Expect::Confined(Confined::default()),
    );
    tx.base.insert(path, Base::Sha(m2_song::Sha::of(was)));
    tx
}

// AGENT-3 (the review's F3): a plugin's state is held by its window under the name the song gives
// its file: a plugin keyed `aux` (a Windows device name) keeps its state in `aux-.state`, and an
// agent's write of that file is busy while the window's changes aren't saved.
#[test]
fn a_plugin_keyed_aux_is_held_by_its_window() {
    use crate::caller::{Caller, as_caller};
    probe::install();
    let fixture = Fixture::new();
    fixture.put("song.ts", &song("0.5").replace("key: 'lead'", "key: 'aux'"));
    fixture.put("data/p/n.json", NOTES);
    fixture.put("data/p/g.json", LANE);
    std::fs::write(fixture.0.join("data/p/aux-.state"), probe::state_with("dirty-at-10-400", 1.0, 0.1)).unwrap();
    fixture.put("data/p/aux-.state.json", r#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#);
    let (p, driver) = Player::driven(48_000);
    let state = Arc::new(crate::AppState::new(String::new(), Graph::with_player(p)));
    state.graph.open(&fixture.0).unwrap();
    state.graph.transport("play").unwrap();
    let mut r = Rig { fixture, state, driver };
    r.fill(4);
    r.fill_to(20);
    until_unsaved(&mut r);
    let saved = std::fs::read(r.fixture.0.join("data/p/aux-.state")).unwrap();
    let tx = state_tx("data/p/aux-.state", &saved);
    let session = r.session();
    let (status, body) =
        as_caller(Caller::agent(Some("Claude"), None), || r.graph().tx_commit(Some(session), &tx.to_json(), None, &[])).unwrap();
    assert_eq!(
        (status, &body["busy"]["what"], &body["busy"]["file"]),
        (423, &serde_json::json!("plugin"), &serde_json::json!("data/p/aux-.state")),
        "{body}"
    );
    assert_eq!(std::fs::read(r.fixture.0.join("data/p/aux-.state")).unwrap(), saved, "nothing changed");
}

// AGENT-3 (the review's F4): the check at the settle hears what the plugin windows did up to then:
// a change the musician makes in the window while an agent's write of that plugin's state is on
// its way takes the write back, and the window's change stays theirs, not saved yet.
#[test]
fn a_window_change_made_while_an_agents_state_lands_takes_it_back() {
    use crate::caller::{Caller, as_caller};
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
    use std::sync::mpsc;
    let mut r = Rig::new("0.5", "dirty-at-10-400");
    assert!(r.graph().inspect(|a| a.open.as_ref().unwrap().windows.unsaved().is_empty()), "not changed yet");
    let state = r.saved();
    let tx = state_tx("data/p/lead.state", &state);
    let handle = r.handle();
    let session = r.session();
    let (arrived, at) = mpsc::channel::<()>();
    let (filled, wait) = mpsc::channel::<()>();
    r.graph().inspect(move |a| {
        a.after_arrival = Some(Box::new(move |_| {
            let _ = arrived.send(());
            let _ = wait.recv();
        }))
    });
    let stop = AtomicBool::new(false);
    let Rig { state: app, driver, .. } = &mut r;
    let (status, body) = std::thread::scope(|s| {
        let stop = &stop;
        s.spawn(move || {
            if at.recv().is_err() {
                return;
            }
            // The musician changes the plugin's sound in its window as the agent's write lands.
            let stats = probe::stats(handle).unwrap();
            while stats.process.load(Relaxed) < 20 {
                let mut buf = [0.0f32; 256];
                driver.fill(&mut buf, 2);
                drop(driver.drain_retired());
            }
            // And the plugin host has told the app of it, as its tick does within 5 ms: the check
            // hears what the windows told the app up to then (a change not told yet: gaps).
            m2_plugins::sweep_now().unwrap();
            let _ = filled.send(());
            while !stop.load(Relaxed) {
                let mut buf = [0.0f32; 256];
                driver.fill(&mut buf, 2);
                drop(driver.drain_retired());
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        let agent = Caller::agent(Some("Claude"), Some("a warmer lead"));
        let out = as_caller(agent, || app.graph.tx_commit(Some(session), &tx.to_json(), None, &[])).unwrap();
        stop.store(true, Relaxed);
        out
    });
    r.graph().inspect(|a| a.after_arrival = None);
    assert_eq!(
        (status, &body["busy"]["what"], &body["busy"]["taken_back"]),
        (423, &serde_json::json!("plugin"), &serde_json::json!(true)),
        "{body}"
    );
    assert_eq!(r.saved(), state, "taken back");
    assert!(
        !r.graph().inspect(|a| a.open.as_ref().unwrap().windows.unsaved().is_empty()),
        "the window's change is still theirs, not saved yet"
    );
}

// AGENT-3 (the review's F1): an agent's change that would end a plugin window's changes not saved
// yet is taken back, whatever file it touched: here song.ts, playing the track with an instrument
// of std in the plugin's place, which would drop the instance the window changed.
#[test]
fn an_agents_change_that_would_drop_a_window_not_saved_yet_is_taken_back() {
    use crate::caller::{Caller, as_caller};
    use m2_song::tx::{Base, Edit, Expect, Op, Pos, Tx, TxPath};
    let mut r = Rig::new("0.5", "dirty-at-10-400");
    r.fill_to(20);
    until_unsaved(&mut r);
    let session = r.session();
    let text = r.song();
    let old = "plugin('clap:m2.probe', { notes: lane.notes('n'), key: 'lead', gain_1: 0.5 })";
    let at = text.find(old).expect("the plugin's call");
    let path = TxPath::new("song.ts").unwrap();
    let edits = vec![
        Edit { start: Pos::Byte(0), end: Pos::Byte(0), expected: String::new(), text: "import { pluck } from 'std/instruments';\n".into() },
        Edit {
            start: Pos::Byte(at as u32),
            end: Pos::Byte((at + old.len()) as u32),
            expected: old.into(),
            text: "pluck(lane.notes('n'))".into(),
        },
    ];
    let mut tx = Tx::new(
        "tx-agent",
        ("a".into(), "b".into()),
        vec![Op::TextEdit { file: path.clone(), edits }],
        Expect::Seen(m2_song::Sha::of(b"")),
    );
    tx.base.insert(path, Base::text(&text));
    let mut json = tx.to_json();
    let (status, checked) = r.graph().tx_check(Some(session), &json).unwrap();
    assert_eq!(status, 200, "{checked}");
    json["expect"] = serde_json::json!({"seen": checked["checked"]["hash"]});
    let (status, body) =
        as_caller(Caller::agent(Some("Claude"), Some("a plucked lead")), || r.graph().tx_commit(Some(session), &json, None, &[])).unwrap();
    assert_eq!(
        (status, &body["busy"]["what"], &body["busy"]["taken_back"]),
        (423, &serde_json::json!("plugin"), &serde_json::json!(true)),
        "{body}"
    );
    assert!(body["busy"]["words"].as_str().unwrap().contains("in its window"), "{body}");
    assert_eq!(r.song(), text, "taken back");
    assert!(!r.graph().inspect(|a| a.open.as_ref().unwrap().windows.unsaved().is_empty()), "the window's change is still theirs");
}

// The review of ag-listen, F17: the graph state's `plugins` says which plugin's window changed its
// sound, not saved yet (`unsaved`), so an agent's listen, which hears the saved state, can say so.
#[test]
fn the_state_says_a_plugin_window_has_changes_not_saved_yet() {
    let mut r = Rig::new("0.5", "dirty-at-10-400");
    r.tick();
    assert_eq!(r.plugin().1["unsaved"], serde_json::json!(false), "{:?}", r.plugin());
    r.fill_to(20);
    until_unsaved(&mut r);
    r.tick();
    assert_eq!(r.plugin().1["unsaved"], serde_json::json!(true), "{:?}", r.plugin());
}
