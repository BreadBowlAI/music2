//! Scopes on the server (TAP-8, TAP-9): a box's last frames in every meters message while its
//! client asks, found again in each new plan, refused in words for what isn't sound, and ended by
//! a stop, a window that closes, or 10 s without an ask.

use m2_play::{Driver, Player};

use super::super::tests::Fixture;
use super::*;

/// A saw bass, a sine kick under an LFO and a square pad, into std's mixer.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { lfo } from 'std/control';\nimport { osc } from 'std/dsp';\n\
                    import { scale } from 'std/math';\nimport { mixer } from 'std/mix';\n\
                    const bass = track('bass', () => osc.saw(110));\n\
                    const kick = track('kick', () => scale(osc.sine(55), lfo(2)));\n\
                    const pad = track('pad', () => osc.square(880));\n\
                    export default song(() => mixer({ bass: bass.out, kick: kick.out, pad: pad.out }));\n";

/// How many of `x`'s frames go the same way as the one before, the most of up or down: nearly all
/// of a saw's.
fn one_way(x: &[f64]) -> usize {
    let up = x.windows(2).filter(|w| w[1] > w[0]).count();
    up.max(x.windows(2).filter(|w| w[1] < w[0]).count())
}

struct Rig {
    _fixture: Fixture,
    graph: Graph,
    driver: Driver,
}

impl Rig {
    fn new(song: &str) -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", song);
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p);
        graph.open(&fixture.0).unwrap();
        graph.watch_meters(true);
        graph.meters();
        let mut rig = Rig { _fixture: fixture, graph, driver };
        rig.fill(1);
        rig
    }

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

    /// The view's id for the call of `callee` in `song.ts`.
    fn id(&self, callee: &str) -> String {
        let state = self.graph.state().unwrap();
        let nodes = state["view"]["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap().clone());
        let ids: Vec<String> = nodes.filter(|n| n["label"] == callee).map(|n| n["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids.len(), 1, "one {callee}: {ids:?}");
        ids[0].clone()
    }

    /// The next meters message, after `blocks` blocks.
    fn read(&mut self, blocks: usize) -> Value {
        self.fill(blocks);
        self.graph.meters().unwrap()
    }
}

/// The scopes of message `m`: each one's module, node, and its frames per channel.
fn scopes(m: &Value) -> Vec<(String, String, Vec<Vec<f64>>)> {
    let Some(list) = m["scopes"].as_array() else { return vec![] };
    list.iter()
        .map(|s| {
            let frames = s["frames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect())
                .collect();
            (s["module"].as_str().unwrap().into(), s["node"].as_str().unwrap().into(), frames)
        })
        .collect()
}

// TAP-8: while a window asks, every meters message carries the box's last 1,024 frames, its
// channels: the bass's saw, as loud as its meter says, and nothing of another box. A stop ends it.
#[test]
fn a_scopes_frames_arrive_while_asked() {
    let mut rig = Rig::new(SONG);
    let saw = rig.id("osc.saw");
    let asked = rig.graph.scope(rig.token(), "w1", "bass", &saw, true).unwrap();
    assert_eq!(asked, json!({"scope": {"module": "bass", "node": saw, "channels": 1}}));
    rig.graph.transport("play").unwrap();
    rig.read(16);
    let m = rig.read(16);
    let got = scopes(&m);
    assert_eq!(got.len(), 1, "{m}");
    let (module, node, frames) = &got[0];
    assert_eq!((module.as_str(), node.as_str(), frames.len(), frames[0].len()), ("bass", saw.as_str(), 1, SCOPE));
    let top = frames[0].iter().fold(0.0f64, |m, x| m.max(x.abs()));
    let ramp = one_way(&frames[0]);
    assert!(top > 0.5 && ramp > SCOPE * 9 / 10, "a saw, a ramp nearly every frame: top {top}, {ramp} one way");
    assert!((top - m["tracks"]["bass"][0].as_f64().unwrap()).abs() < 0.05, "as loud as its meter: {top} {m}");
    // Asked again: the same scope, kept.
    assert_eq!(rig.graph.scope(rig.token(), "w1", "bass", &saw, true).unwrap(), asked);
    assert_eq!(rig.graph.scope(rig.token(), "w1", "bass", &saw, false).unwrap(), json!({}));
    rig.read(4);
    assert!(scopes(&rig.read(4)).is_empty(), "stopped");
}

// TAP-9: a scope follows its call into each new plan: an edit that adds a track before it (a new
// plan, and a new id for its box in the view) keeps its frames coming, from the same call, under
// its new id; and an edit of the call keeps it on that call.
#[test]
fn a_scope_follows_its_box_into_each_new_plan() {
    let mut rig = Rig::new(SONG);
    let saw = rig.id("osc.saw");
    rig.graph.scope(rig.token(), "w1", "bass", &saw, true).unwrap();
    rig.graph.transport("play").unwrap();
    assert_eq!(scopes(&rig.read(16)).len(), 1);
    let more = SONG.replace("const bass", "const hats = track('hats', () => osc.sine(880));\nconst bass").replace(
        "mixer({ bass: bass.out, kick: kick.out, pad: pad.out })",
        "mixer({ bass: bass.out, kick: kick.out, pad: pad.out, hats: hats.out })",
    );
    rig.graph.write_file(Some(rig.token()), "song.ts", SONG, &more).unwrap();
    rig.read(16);
    let got = scopes(&rig.read(16));
    assert_eq!(got.len(), 1, "still shown");
    let moved = rig.id("osc.saw");
    assert_ne!(moved, saw, "the edit gave the saw's box another id");
    assert_eq!(got[0].1, moved, "the scope follows its call, under the id the view gives it now");
    assert_eq!(
        rig.graph.scope(rig.token(), "w1", "bass", &moved, true).unwrap()["scope"]["channels"],
        1,
        "asked by it, it is the same scope"
    );
    let ramp = one_way(&got[0].2[0]);
    assert!(ramp > SCOPE * 9 / 10, "the saw still, not the sine added: {ramp} one way");
    // The saw made a sine: the scope's box is that call still.
    let sine = more.replace("osc.saw(110)", "osc.square(110)");
    rig.graph.write_file(Some(rig.token()), "song.ts", &more, &sine).unwrap();
    rig.read(16);
    let got = scopes(&rig.read(16));
    let flat = got[0].2[0].windows(2).filter(|w| w[1] == w[0]).count();
    assert!(flat > SCOPE / 2, "the same call, now a square: {flat} flat");
}

// TAP-8: what isn't one sound at the top of its track is refused in words; a fifth box is refused
// while four are shown; a window that closes ends its scopes, and so do 10 s without an ask.
#[test]
fn a_scope_is_refused_in_words_and_ends_with_its_client() {
    let mut rig = Rig::new(SONG);
    let token = rig.token();
    let lfo = rig.id("lfo");
    let no = rig.graph.scope(token, "w1", "kick", &lfo, true).unwrap_err();
    assert_eq!(format!("{no:#}"), "lfo gives a control or notes, not sound: a scope shows sound.");
    let boxes = [("bass", rig.id("osc.saw")), ("kick", rig.id("osc.sine")), ("kick", rig.id("scale")), ("song", rig.id("mixer"))];
    for (module, node) in &boxes {
        rig.graph.scope(token, "w1", module, node, true).unwrap();
    }
    rig.graph.transport("play").unwrap();
    rig.read(16);
    assert_eq!(scopes(&rig.read(16)).len(), 4, "four shown");
    let fifth = rig.graph.scope(token, "w2", "pad", &rig.id("osc.square"), true).unwrap_err();
    assert_eq!(format!("{fifth:#}"), "4 scopes are showing already: close one, then try again.");
    // Another window asks for one shown: it shares it, and keeps it when the first closes.
    rig.graph.scope(token, "w2", "bass", &boxes[0].1, true).unwrap();
    rig.graph.client_gone("w1");
    let left = scopes(&rig.read(16));
    assert_eq!(left.iter().map(|s| s.1.as_str()).collect::<Vec<_>>(), [boxes[0].1.as_str()], "only w2's");
    // 10 s on, w2 never asked again: it ends.
    rig.graph.inner.control.lock().scopes.expire(std::time::Instant::now() + KEEP, None);
    assert!(scopes(&rig.read(16)).is_empty(), "ended");
    assert!(rig.graph.scope(token + 1, "w2", "bass", &boxes[0].1, true).is_err(), "another session's");
}
