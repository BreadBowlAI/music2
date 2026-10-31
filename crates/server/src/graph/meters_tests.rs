//! The meters on the server (8b.8, TAP-4 to TAP-6, TAP-9): sent while a window watches and the
//! engine runs, not while it idles, moved to each new plan, after the fader and the mute; the
//! master's loudness as `m2 listen` measures it; and the DSP load in the status.

use m2_play::{Driver, Player};

use super::super::overlay::Change;
use super::super::tests::Fixture;
use super::*;

/// A kick on every beat, from std's drum.
const KICK: &str = "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
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
        let graph = Graph::with_player(p);
        graph.open(&fixture.0).unwrap();
        // A window watches (TAP-4); the first read after the taps start sends nothing.
        graph.watch_meters(true);
        assert_eq!(graph.meters(), None, "the taps heard no block yet");
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

// 8b.8, MON-1, Q2, TAP-5: a muted kick's meters read after the mute: the floor, its track's and
// its channel's. The bass is still heard, and the song's output still moves with the duck the
// kick's sidechain drives (the bass's meter is its channel's, after the duck in its strip).
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
        assert_eq!((&m["tracks"]["kick"], &m["channels"]["kick"]), (&json!([0.0]), &json!([0.0])), "{m}");
        assert!(peak(&m, "bass") > 0.1, "{m}");
        outs.push(m["out"][0].as_f64().unwrap());
    }
    let (lo, hi) = (outs.iter().cloned().fold(f64::MAX, f64::min), outs.iter().cloned().fold(0.0, f64::max));
    assert!(lo > 0.0 && hi > 1.5 * lo, "the output dips at each kick: {lo} to {hi}");
}

/// The meter of `key` in `m`'s `field` (`tracks` or `channels`), its first channel, in dB.
fn db_of(m: &Value, field: &str, key: &str) -> f64 {
    20.0 * m[field][key][0].as_f64().unwrap_or_else(|| panic!("no {field}.{key} in {m}")).log10()
}

/// A meters message after `blocks` more blocks, the one before it read and dropped.
fn settled(rig: &mut Rig, blocks: usize) -> Value {
    rig.fill(blocks);
    rig.graph.meters();
    rig.fill(16);
    rig.graph.meters().unwrap()
}

// TAP-5, gaps #20: a channel's meter, and its track's when the channel is its own, read after the
// fader. The kick's fader moved to −12 dB (a value change, which plays as a cell write) moves both
// its meters down by 12 dB once the fader's glide is over; the bass, in a channel of its own too,
// stays where it was; and a track sharing a channel with another reads before it.
#[test]
fn a_fader_at_minus_12_db_moves_its_tracks_meter_by_12_db() {
    let song = |db: i32| {
        kick_and_bass(&format!(
            "const hats = track('hats', () => osc.sine(880));
const rim = track('rim', () => osc.sine(440));
\
             export default song(() => mixer({{ kick: {{ in: kick.out, db: db({db}) }}, bass: bass.out, both: {{ in: sum(hats.out, rim.out), db: db({db}) }} }}));\n"
        ))
        .replace("import { osc }", "import { db } from '@m2/core';\nimport { sum } from 'std/math';\nimport { osc }")
    };
    let mut rig = Rig::new(&song(0), &[]);
    rig.graph.transport("play").unwrap();
    let before = settled(&mut rig, 16);
    assert_eq!(keys(&before), ["bass", "hats", "kick", "rim"], "{before}");
    let channels: Vec<&str> = before["channels"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(channels, ["bass", "both", "kick"], "{before}");
    assert_eq!(before["tracks"]["kick"], before["channels"]["kick"], "the kick's channel is its own");
    rig.graph.write_file(Some(rig.token()), "song.ts", &song(0), &song(-12)).unwrap();
    // The fader's glide.
    let after = settled(&mut rig, 64);
    for (field, key) in [("tracks", "kick"), ("channels", "kick"), ("channels", "both")] {
        let moved = db_of(&after, field, key) - db_of(&before, field, key);
        assert!((moved + 12.0).abs() < 0.05, "{field}.{key} moved {moved:.3} dB: {before} then {after}");
    }
    for (field, key) in [("tracks", "bass"), ("tracks", "hats"), ("tracks", "rim")] {
        let moved = db_of(&after, field, key) - db_of(&before, field, key);
        assert!(moved.abs() < 0.05, "{field}.{key} moved {moved:.3} dB, though its fader didn't, or it has none of its own");
    }
}

// TAP-4: while no window watches, the taps of what plays are off and nothing is sent; watched
// again, the meters come from the read after the next: the first would read the silence of taps
// that heard no block yet.
#[test]
fn unwatched_meters_send_nothing() {
    let mut rig = Rig::new(&kick_and_bass("export default song(() => mixer({ kick: kick.out, bass: bass.out }));\n"), &[]);
    rig.graph.transport("play").unwrap();
    rig.fill(8);
    assert!(rig.graph.meters().is_some());
    rig.graph.watch_meters(false);
    let taps = {
        let cp = rig.graph.inner.control.lock();
        cp.open.as_ref().unwrap().engine.plays.as_ref().unwrap().info.taps.clone().unwrap()
    };
    assert!(!taps.watched(), "the taps are off");
    rig.fill(8);
    assert_eq!(rig.graph.meters(), None, "nothing is sent");
    assert_eq!(taps.take().meters, vec![vec![0.0]; 2], "nothing was raised");
    rig.graph.watch_meters(true);
    rig.fill(8);
    assert_eq!(rig.graph.meters(), None, "the first read after they start sends nothing, not silence");
    rig.fill(8);
    let m = rig.graph.meters().unwrap();
    assert!(peak(&m, "kick") > 0.1, "{m}");
}

/// A 1 kHz sine at −20 dBFS, the whole song.
const SINE: &str = "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { scale } from 'std/math';\n\
                    export default song(() => scale(osc.sine(hz(1000)), 0.1));\n";

// TAP-6: the master's short-term loudness, worked out from the output's ring while the meters are
// read, is `m2 listen`'s within 0.1 LU, and its true peak too; under 3 s heard there is none.
#[test]
fn the_masters_loudness_matches_m2_listen() {
    let mut rig = Rig::new(SINE, &[]);
    rig.graph.transport("play").unwrap();
    let mut last = Value::Null;
    // 4 s, read about every 85 ms.
    for n in 0..47 {
        rig.fill(32);
        last = rig.graph.meters().unwrap();
        if n < 30 {
            assert_eq!(last["loudness"]["short"], Value::Null, "under 3 s: {last}");
        }
    }
    let short = last["loudness"]["short"].as_f64().unwrap_or_else(|| panic!("no loudness in {last}"));
    let true_peak = last["loudness"]["true_peak"].as_f64().unwrap();
    let mut packages = m2_runtime::PackageSet::default();
    packages.load(&crate::resources::std_dir()).unwrap();
    let session = m2_session::Session::open(&rig._fixture.0, packages.clone()).unwrap_or_else(|d| panic!("{d:?}"));
    let ask = m2_editor::listen::Ask {
        from: 0.0,
        to: 4.0,
        preroll: 0.0,
        tracks: None,
        overlays: None,
        picture: false,
        threads: m2_editor::render::Threads::Exactly(1),
    };
    let heard = m2_editor::listen::hear(session.accepted_arc(), &packages, &ask).unwrap().mix;
    let listened = heard.lufs_short_max.unwrap();
    assert!((short - listened).abs() < 0.1, "live {short} LUFS, m2 listen {listened}");
    assert!((true_peak - heard.true_peak.unwrap()).abs() < 0.1, "live {true_peak} dBTP, m2 listen {:?}", heard.true_peak);
    assert!((short + 20.0).abs() < 0.2, "{short}");
}
