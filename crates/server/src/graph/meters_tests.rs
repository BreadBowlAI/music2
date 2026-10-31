//! The meters on the server (8b.8): sent while the engine runs and not while it idles, moved to
//! each new plan, after mute; and the DSP load in the status.

use m2_play::{Driver, Player};

use super::super::overlay::Change;
use super::super::tests::Fixture;
use super::*;

/// A kick on every beat, from std's drum.
const KICK: &str = "import { song, track } from '@m2/core';\nimport { kick } from '@m2/synths';\n\
                    const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n";

/// Kick and bass tracks into std's mixer, and `more` after them.
fn kick_and_bass(more: &str) -> String {
    format!(
        "import {{ song, track }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\nimport {{ mixer }} from 'std/mix';\n\
         const kick = track('kick', () => osc.sine(55));\nconst bass = track('bass', () => osc.saw(110));\n{more}"
    )
}

/// A project open with a driven player.
struct Rig {
    _fixture: Fixture,
    graph: Graph,
    driver: Driver,
}

impl Rig {
    /// A project of `song.ts` and `files`, open.
    fn new(song: &str, files: &[(&str, &str)]) -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", song);
        for (path, text) in files {
            fixture.put(path, text);
        }
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p).stored();
        graph.open(&fixture.0).unwrap();
        let mut rig = Rig { _fixture: fixture, graph, driver };
        // The engine takes what was loaded at its first callback.
        rig.fill(1);
        rig
    }

    /// `blocks` callbacks of a block each.
    fn fill(&mut self, blocks: usize) {
        for _ in 0..blocks {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
        }
    }

    fn token(&self) -> u64 {
        self.graph.state().unwrap()["session"].as_u64().unwrap()
    }
}

/// The hits, one on every beat for 8 beats.
fn hits() -> String {
    let notes: Vec<Value> = (0..8).map(|b| json!({"pitch": 36, "start": b, "length": 0.25, "velocity": 0.9})).collect();
    json!({"clips": [{"start": 0, "length": 8, "notes": notes}]}).to_string()
}

fn peak(m: &Value, track: &str) -> f64 {
    m["tracks"][track][0].as_f64().unwrap_or_else(|| panic!("no {track} in {m}"))
}

fn keys(m: &Value) -> Vec<&str> {
    m["tracks"].as_object().unwrap().keys().map(String::as_str).collect()
}

// 8b.8, RENDER-6a: meters come while the engine runs, played or stopped while it rings; as it
// starts to idle, one last message, then none until it runs again.
#[test]
fn meters_are_sent_while_playing_and_stop_when_idle() {
    let mut rig = Rig::new(KICK, &[("data/kick/hits.json", &hits())]);
    // Open and stopped: silent, so it idles at once.
    rig.fill(32);
    let last = rig.graph.meters().expect("one last message as it idles");
    assert_eq!((&last["t"], &last["out"]), (&json!("meters"), &json!([0.0, 0.0])));
    rig.fill(8);
    assert_eq!(rig.graph.meters(), None, "idle: none");
    rig.graph.transport("play").unwrap();
    rig.fill(40);
    let m = rig.graph.meters().expect("playing");
    assert_eq!(keys(&m), ["kick"]);
    assert!(peak(&m, "kick") > 0.1 && m["out"][0].as_f64().unwrap() > 0.1, "{m}");
    assert!(m["dsp"]["load"].as_f64().unwrap() > 0.0 && m["dsp"]["dropouts"].as_u64().is_some(), "{m}");
    rig.fill(4);
    assert!(rig.graph.meters().is_some(), "each read while it runs");
    assert!(rig.graph.activity().load > 0.0, "the status's load is the meters'");
    // Stopped: the kick's tail rings out, then the engine idles.
    rig.graph.transport("stop").unwrap();
    let mut sent = vec![];
    for _ in 0..500 {
        rig.fill(4);
        match rig.graph.meters() {
            Some(m) => sent.push(m),
            None => break,
        }
    }
    assert!(sent.len() > 2 && sent.len() < 500, "it rang a while, then idled ({} messages)", sent.len());
    let last = sent.last().unwrap();
    assert!(peak(last, "kick") < 1e-4 && last["out"][0].as_f64().unwrap() < 1e-4, "the last reads the silence it idled in: {last}");
    rig.fill(8);
    assert_eq!(rig.graph.meters(), None);
}

// 8b.8: each new plan brings its taps: a track added is metered once the engine takes its plan,
// and nothing is sent while the plan waits to be taken.
#[test]
fn a_new_plan_moves_the_taps() {
    let song = kick_and_bass("export default song(() => mixer({ kick: kick.out, bass: bass.out }));\n");
    let mut rig = Rig::new(&song, &[]);
    rig.graph.transport("play").unwrap();
    rig.fill(8);
    let m = rig.graph.meters().unwrap();
    assert_eq!(keys(&m), ["bass", "kick"]);
    let more = kick_and_bass(
        "const hats = track('hats', () => osc.sine(880));\nexport default song(() => mixer({ kick: kick.out, bass: bass.out, hats: hats.out }));\n",
    );
    rig.graph.write_file(Some(rig.token()), "song.ts", &song, &more).unwrap();
    assert_eq!(rig.graph.meters(), None, "the new plan isn't taken yet");
    rig.fill(8);
    let m = rig.graph.meters().unwrap();
    assert_eq!(keys(&m), ["bass", "hats", "kick"]);
    assert!(peak(&m, "hats") > 0.1 && peak(&m, "kick") > 0.1, "{m}");
}

// 8b.8, MON-1, Q2: a muted kick's meter reads its overlay, after the mute: the floor. The bass is
// still heard, and the song's output still moves with the duck the kick's sidechain drives (the
// bass's own meter is read before the duck, which is in the song).
#[test]
fn muting_the_kick_floors_its_meter_and_the_bass_still_ducks() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures/sidechain/env");
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap();
    let files = [("data/kick/hits.json", read("data/kick/hits.json")), ("data/arrangement.json", read("data/arrangement.json"))];
    let mut rig = Rig::new(&read("song.ts"), &files.iter().map(|(p, t)| (*p, t.as_str())).collect::<Vec<_>>());
    rig.graph.transport("play").unwrap();
    rig.graph.overlay(rig.token(), Change { mute: [("kick".to_string(), true)].into(), ..Change::default() }).unwrap();
    // The mute's 10 ms glide.
    rig.fill(8);
    rig.graph.meters().unwrap();
    let mut outs = vec![];
    for _ in 0..60 {
        rig.fill(8);
        let m = rig.graph.meters().unwrap();
        assert_eq!(m["tracks"]["kick"], json!([0.0]), "{m}");
        assert!(peak(&m, "bass") > 0.1, "{m}");
        outs.push(m["out"][0].as_f64().unwrap());
    }
    let (lo, hi) = (outs.iter().cloned().fold(f64::MAX, f64::min), outs.iter().cloned().fold(0.0, f64::max));
    assert!(lo > 0.0 && hi > 1.5 * lo, "the output dips at each kick: {lo} to {hi}");
}
