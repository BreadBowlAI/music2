//! Feeds (FEED-1 to FEED-8): a track's notes written ahead of the playhead by an agent, played as
//! the lane's scheduler's second source, told as `feed` events, and kept into the song only when
//! asked. The test's thread is the audio thread: it fills a driven player.

use std::time::{Duration, Instant};

use m2_play::{Driver, Player};
use m2_runtime::TraceKind;
use m2_song::Note;
use serde_json::{Value, json};

use super::super::tests::{Fixture, eventually, revs};
use super::*;
use crate::caller::{Caller, as_caller};

/// A bass playing its `notes` lane (no clips), and keys playing theirs, into std's mixer.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { bass, pluck } from '@m2/synths';\nimport { mixer } from 'std/mix';\n\
                    const b = track('bass', ({ lane }) => bass(lane.notes('notes')));\n\
                    const k = track('keys', ({ lane }) => pluck(lane.notes('notes')));\n\
                    export default song(() => mixer({ bass: b.out, keys: k.out }));\n";

/// Frames in a beat at 120 BPM and 48 kHz.
const BEAT: u64 = 24_000;

/// The song open in a driven player, its renderer traced, its events heard.
struct Rig {
    fixture: Fixture,
    graph: Graph,
    driver: Driver,
    token: u64,
    events: tokio::sync::broadcast::Receiver<String>,
}

impl Rig {
    fn new() -> Rig {
        Rig::with(&[])
    }

    /// With these files written first, each `(path, text)`.
    fn with(files: &[(&str, &str)]) -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", SONG);
        for (path, text) in files {
            fixture.put(path, text);
        }
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p).stored();
        let (to, events) = tokio::sync::broadcast::channel(4096);
        graph.tell_steps(to);
        graph.open(&fixture.0).unwrap();
        let token = graph.state().unwrap()["session"].as_u64().unwrap();
        let mut rig = Rig { fixture, graph, driver, token, events };
        rig.fill(2);
        rig.trace();
        rig
    }

    /// Trace what the renderer that plays now turns on and off.
    fn trace(&mut self) {
        self.driver.engine().unwrap().current_mut().unwrap().trace(8192);
    }

    /// `n` blocks of 128 frames; the left channel.
    fn fill(&mut self, n: usize) -> Vec<f32> {
        let mut out = vec![];
        for _ in 0..n {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
            out.extend(buf.iter().step_by(2));
        }
        out
    }

    /// Fill until the transport passes `beat`, ticking the feeds every 16 blocks (about 43 ms).
    fn until(&mut self, beat: f64) -> Vec<f32> {
        let mut out = vec![];
        let mut n = 0;
        while self.graph.status().beat < beat {
            out.extend(self.fill(1));
            n += 1;
            if n % 16 == 0 {
                self.graph.feeds_tick();
            }
            assert!(n < 200_000, "the transport never reached beat {beat}");
        }
        self.graph.feeds_tick();
        out
    }

    fn play(&mut self) {
        self.graph.transport("play").unwrap();
        self.fill(1);
    }

    /// The Ons the renderer played: transport frame and pitch.
    fn ons(&mut self) -> Vec<(u64, f32)> {
        let entries = self.driver.engine().unwrap().current().unwrap().trace_entries();
        entries.iter().filter_map(|e| if let TraceKind::On { pitch } = e.kind { Some((e.transport, pitch)) } else { None }).collect()
    }

    fn offs(&mut self) -> Vec<u64> {
        let entries = self.driver.engine().unwrap().current().unwrap().trace_entries();
        entries.iter().filter(|e| e.kind == TraceKind::Off).map(|e| e.transport).collect()
    }

    fn sounding(&mut self) -> usize {
        self.driver.engine().unwrap().current().unwrap().sounding_notes()
    }

    /// Open a feed on `track` as the agent Claude.
    fn open(&self, track: &str, low_bars: Option<f64>) -> Result<Value> {
        let open = Open { track: track.into(), lane: None, low_bars };
        as_caller(Caller::agent(Some("Claude"), None), || self.graph.feed_open(open))
    }

    /// Put `notes` (pitch, start, length) from `from` to `to` on `feed`, as the agent Claude.
    fn put(&self, feed: &str, from: f64, to: f64, notes: &[(f64, f64, f64)]) -> Result<Value> {
        let notes = notes.iter().map(|&(pitch, start, length)| Note { pitch, start, length, velocity: 0.8 }).collect();
        as_caller(Caller::agent(Some("Claude"), None), || self.graph.feed_put(Put { feed: feed.into(), from, to, notes }))
    }

    /// The feed events heard since the last call, each as JSON.
    fn heard(&mut self) -> Vec<Value> {
        let mut out = vec![];
        while let Ok(m) = self.events.try_recv() {
            let v: Value = serde_json::from_str(&m).unwrap();
            if v["t"] == "feed" {
                out.push(v);
            }
        }
        out
    }

    /// The `why` of each feed event heard since the last call, those with one.
    fn whys(&mut self) -> Vec<String> {
        self.heard().iter().filter_map(|v| v["why"].as_str().map(str::to_string)).collect()
    }

    fn state(&self, feed: &str) -> Value {
        let all = self.graph.feeds();
        all["feeds"].as_array().unwrap().iter().find(|f| f["feed"] == feed).cloned().unwrap_or(Value::Null)
    }
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

// FEED-1: a feed opens on a track that plays a notes lane, one a track, by the agent's name, and
// the state lists it; a track that isn't there, or plays no notes lane, is refused in words.
#[test]
fn a_feed_opens_on_a_notes_track_and_one_a_track() {
    let mut rig = Rig::new();
    let opened = rig.open("bass", None).unwrap();
    assert_eq!(
        (&opened["feed"], &opened["lane"], &opened["writer"], &opened["low_bars"]),
        (&json!("bass"), &json!("notes"), &json!("Claude"), &json!(4.0))
    );
    assert_eq!(rig.whys(), ["open"]);
    let again = rig.open("bass", None).unwrap_err().to_string();
    assert!(again.contains("already"), "{again}");
    let gone = rig.open("drums", None).unwrap_err().to_string();
    assert!(gone.contains("drums"), "{gone}");
    let song = rig.open("song", None).unwrap_err().to_string();
    assert!(song.contains("notes"), "{song}");
    // The state carries it for the page's Live mark (FEED-1), with its writer.
    let state = rig.graph.state().unwrap();
    assert_eq!(state["feeds"][0]["feed"], "bass", "{}", state["feeds"]);
    assert_eq!(state["feeds"][0]["writer"], "Claude");
    // The musician's open has no writer until an agent writes.
    let page = rig.graph.feed_open(Open { track: "keys".into(), lane: None, low_bars: None }).unwrap();
    assert_eq!(page["writer"], Value::Null);
    rig.put("keys", 0.0, 4.0, &[]).unwrap();
    assert_eq!(rig.state("keys")["writer"], "Claude");
}

// FEED-2: a put lands on its beats, sample-accurate, written while stopped and written ahead
// while playing: each On is on its beat's frame.
#[test]
fn a_put_lands_on_its_beats_sample_accurate() {
    let mut rig = Rig::new();
    rig.open("bass", None).unwrap();
    let taken = rig.put("bass", 0.0, 4.0, &[(36.0, 1.0, 0.5), (40.0, 2.5, 0.25), (43.0, 3.25, 0.5)]).unwrap();
    assert_eq!((&taken["taken"], &taken["late"]), (&json!(3), &json!(0)), "{taken}");
    rig.play();
    rig.until(4.5);
    let ahead = rig.put("bass", 4.0, 12.0, &[(45.0, 6.0, 1.0), (47.0, 7.125, 0.5)]).unwrap();
    assert_eq!(ahead["taken"], 2, "{ahead}");
    rig.until(8.0);
    let at = |beat: f64| (beat * BEAT as f64).round() as u64;
    assert_eq!(rig.ons(), [(at(1.0), 36.0), (at(2.5), 40.0), (at(3.25), 43.0), (at(6.0), 45.0), (at(7.125), 47.0)]);
    assert!(rig.offs().contains(&at(1.5)) && rig.offs().contains(&at(7.0)), "{:?}", rig.offs());
    // Nothing was written: the song is as it was.
    assert_eq!(revs(&rig.graph).0, 1);
}

// FEED-4: while playing, a put before the lock takes only what is past it: a note before the
// playhead, and one inside the lock's margin, are left out and counted late; a sounding note the
// put leaves out isn't cut, and ends where it was written to.
#[test]
fn a_put_before_the_lock_takes_only_what_is_past_it() {
    let mut rig = Rig::new();
    rig.open("bass", None).unwrap();
    rig.put("bass", 0.0, 8.0, &[(36.0, 1.5, 2.0)]).unwrap();
    rig.play();
    rig.until(2.0);
    assert_eq!(rig.sounding(), 1, "the long note sounds");
    let st = rig.graph.status();
    let answer = rig.put("bass", 0.0, 8.0, &[(38.0, 1.0, 0.5), (40.0, st.beat + 0.02, 0.5), (43.0, 5.0, 0.5)]).unwrap();
    assert_eq!((&answer["taken"], &answer["late"]), (&json!(1), &json!(2)), "{answer}");
    let (lock, playhead) = (answer["lock"].as_f64().unwrap(), answer["playhead"].as_f64().unwrap());
    // Two of the driven player's buffers are far less than 50 ms: the lock is 50 ms on (0.1 beats).
    assert!((lock - playhead - 0.1).abs() < 1e-6, "lock {lock}, playhead {playhead}");
    assert_eq!(answer["written_to"], 8.0);
    rig.fill(4);
    assert_eq!(rig.sounding(), 1, "the sounding note isn't cut by the put");
    rig.until(6.0);
    let at = |beat: f64| (beat * BEAT as f64).round() as u64;
    assert!(rig.offs().contains(&at(3.5)), "it ends where it was written to: {:?}", rig.offs());
    assert_eq!(rig.ons().iter().map(|o| o.1).collect::<Vec<_>>(), [36.0, 43.0]);
    assert_eq!(rig.state("bass")["late"], 2);
    // Stopped, nothing is late: a put waits for Play.
    rig.graph.transport("stop").unwrap();
    rig.fill(2);
    let stopped = rig.put("bass", 0.0, 8.0, &[(38.0, 1.0, 0.5)]).unwrap();
    assert_eq!((&stopped["taken"], &stopped["late"]), (&json!(1), &json!(0)), "{stopped}");
}

// FEED-4, FEED-2: a rewrite past the lock replaces what was there: the old note never sounds, the
// new one does. Notes outside the range stay.
#[test]
fn a_rewrite_past_the_lock_replaces_what_was_there() {
    let mut rig = Rig::new();
    rig.open("bass", None).unwrap();
    rig.put("bass", 0.0, 8.0, &[(36.0, 5.0, 0.5), (48.0, 7.0, 0.5)]).unwrap();
    rig.play();
    rig.until(2.0);
    rig.put("bass", 4.0, 6.0, &[(40.0, 5.5, 0.5)]).unwrap();
    rig.until(8.0);
    let at = |beat: f64| (beat * BEAT as f64).round() as u64;
    assert_eq!(rig.ons(), [(at(5.5), 40.0), (at(7.0), 48.0)]);
    let bad = rig.put("bass", 9.0, 10.0, &[(40.0, 11.0, 0.5)]).unwrap_err().to_string();
    assert!(bad.contains("11"), "a note outside its range is refused: {bad}");
}

// FEED-4: a `to` more than 64 bars past the playhead is refused in words, and changes nothing.
#[test]
fn a_put_too_far_ahead_is_refused_in_words() {
    let rig = Rig::new();
    rig.open("bass", None).unwrap();
    let far = rig.put("bass", 0.0, 64.0 * 4.0 + 1.0, &[(36.0, 1.0, 1.0)]).unwrap_err().to_string();
    assert!(far.contains("64 bars"), "{far}");
    assert_eq!(rig.state("bass")["notes"], 0);
    rig.put("bass", 0.0, 64.0 * 4.0, &[(36.0, 1.0, 1.0)]).unwrap();
}

// FEED-4, FEED-7: notes that end more than 16 bars behind the playhead are dropped from the feed,
// and still in what played.
#[test]
fn notes_far_behind_are_dropped_and_still_kept() {
    let mut rig = Rig::new();
    rig.open("bass", None).unwrap();
    rig.put("bass", 0.0, 4.0, &[(36.0, 1.0, 1.0), (38.0, 2.0, 1.0)]).unwrap();
    rig.graph.seek(60.0).unwrap();
    rig.play();
    rig.until(61.0);
    assert_eq!(rig.state("bass")["notes"], 2, "15 bars behind: kept");
    // Played from the start this time.
    rig.graph.seek(0.0).unwrap();
    rig.until(70.5);
    assert_eq!(rig.state("bass")["notes"], 0, "more than 16 bars behind: dropped");
    let kept = rig.graph.feed_keep("bass", Some(As::Clip)).unwrap();
    assert_eq!(kept["kept"], 2, "{kept}");
}

// FEED-5: low, empty and jump are each told once, at the right moment: low when the buffer drops
// under the low mark, empty when the playhead reaches written to (counted), jump at a seek, a stop
// and a loop's wrap; and the state at most every quarter second while playing.
#[test]
fn low_empty_and_jump_are_each_told_once() {
    let mut rig = Rig::new();
    rig.open("bass", Some(1.0)).unwrap();
    rig.put("bass", 0.0, 8.0, &[(36.0, 0.0, 1.0)]).unwrap();
    rig.heard();
    rig.play();
    rig.until(3.9);
    assert!(rig.whys().is_empty(), "the buffer is over a bar");
    rig.until(4.1);
    let heard = rig.heard();
    let low: Vec<&Value> = heard.iter().filter(|v| v["why"] == "low").collect();
    assert_eq!(low.len(), 1, "{heard:?}");
    let b = low[0]["buffer"]["beats"].as_f64().unwrap();
    assert!(b < 4.0 && b > 3.8, "told as it crossed: {b}");
    assert_eq!(low[0]["buffer"]["bars"].as_f64().unwrap(), b / 4.0);
    assert!((low[0]["buffer"]["seconds"].as_f64().unwrap() - b / 2.0).abs() < 1e-9);
    rig.until(7.9);
    assert!(rig.whys().is_empty(), "low once");
    rig.until(8.1);
    let heard = rig.heard();
    let empty: Vec<&Value> = heard.iter().filter(|v| v["why"] == "empty").collect();
    assert_eq!(empty.len(), 1, "{heard:?}");
    assert_eq!(empty[0]["empties"], 1);
    // While playing, the state comes without a why, no more than four times a second.
    let t = Instant::now();
    rig.heard();
    while t.elapsed() < Duration::from_millis(600) {
        rig.fill(4);
        rig.graph.feeds_tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    let plain = rig.heard().iter().filter(|v| v["why"].is_null()).count();
    assert!((2..=3).contains(&plain), "{plain} in 600 ms");
    rig.until(9.0);
    assert!(rig.whys().is_empty(), "empty once");
    // A seek back: written to is 8 again, above the low mark: jump alone.
    rig.graph.seek(0.0).unwrap();
    rig.fill(2);
    rig.graph.feeds_tick();
    assert_eq!(rig.whys(), ["jump"]);
    rig.graph.transport("stop").unwrap();
    rig.fill(2);
    rig.graph.feeds_tick();
    assert_eq!(rig.whys(), ["jump"]);
    // A loop's wrap.
    rig.graph.set_loop(Some((0.0, 2.0)), Some(true)).unwrap();
    rig.play();
    rig.until(1.9);
    rig.heard();
    let mut n = 0;
    while rig.graph.status().beat > 1.0 {
        rig.fill(1);
        n += 1;
        assert!(n < 10_000);
    }
    rig.graph.feeds_tick();
    assert_eq!(rig.whys(), ["jump"], "the wrap");
}

// FEED-1, FEED-2: close ends the feed's sounding notes at once (each gets its Off), and what played
// is kept for Keep.
#[test]
fn close_ends_the_feeds_notes_at_once() {
    let mut rig = Rig::new();
    rig.open("bass", None).unwrap();
    rig.put("bass", 0.0, 16.0, &[(36.0, 0.5, 8.0)]).unwrap();
    rig.play();
    rig.until(1.0);
    assert_eq!(rig.sounding(), 1);
    let closed = rig.graph.feed_close("bass").unwrap();
    assert_eq!(closed["feed"], "bass");
    rig.fill(2);
    assert_eq!(rig.sounding(), 0, "its note ends at once");
    assert_eq!(rig.offs().len(), 1);
    assert!(rig.graph.state().unwrap()["feeds"].as_array().unwrap().is_empty());
    assert!(rig.whys().contains(&"closed".to_string()));
    let kept = rig.graph.feed_keep("bass", None).unwrap();
    assert_eq!(kept["kept"], 1, "{kept}");
    // Its note, as long as it played.
    let lane = rig.fixture.read("data/bass/notes.json");
    let v: Value = serde_json::from_str(&lane).unwrap();
    let length = v["clips"][0]["notes"][0]["length"].as_f64().unwrap();
    assert!(length > 0.5 && length < 0.6, "{lane}");
}

// FEED-2: mute and solo silence a feed, as any track's notes.
#[test]
fn mute_and_solo_silence_a_feed() {
    let mut rig = Rig::new();
    rig.open("bass", None).unwrap();
    rig.put("bass", 0.0, 64.0, &[(36.0, 0.0, 60.0)]).unwrap();
    rig.play();
    assert!(rms(&rig.until(1.0)) > 1e-3, "the feed sounds");
    let mute = |rig: &Rig, on: bool| {
        rig.graph.overlay(rig.token, super::super::overlay::Change { mute: [("bass".to_string(), on)].into(), ..Default::default() })
    };
    mute(&rig, true).unwrap();
    rig.until(2.0);
    assert_eq!(rms(&rig.until(3.0)), 0.0, "muted");
    mute(&rig, false).unwrap();
    rig.until(4.0);
    assert!(rms(&rig.until(5.0)) > 1e-3, "unmuted");
    rig.graph
        .overlay(rig.token, super::super::overlay::Change { solo: [("keys".to_string(), true)].into(), ..Default::default() })
        .unwrap();
    rig.until(6.0);
    assert_eq!(rms(&rig.until(7.0)), 0.0, "another track soloed");
}

// FEED-3: a feed never reaches the song's files, a revision, an export or a bounce.
#[test]
fn a_feed_never_reaches_a_file_an_export_or_a_bounce() {
    let mut rig = Rig::new();
    let files = |rig: &Rig| {
        let mut out = vec![];
        let mut stack = vec![rig.fixture.0.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.file_name().is_some_and(|n| n == ".music2") {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((p.clone(), std::fs::read(&p).unwrap()));
                }
            }
        }
        out.sort();
        out
    };
    let export = |rig: &Rig, name: &str| {
        let (_, _, r, _) = rig.graph.render(m2_editor::render::Length::Seconds(3.0)).unwrap();
        let out = rig.fixture.0.join(".music2").join(name);
        r.write(&out, |_| {}).unwrap();
        std::fs::read(out).unwrap()
    };
    let bounce = |rig: &Rig, name: &str| {
        let plan = rig.graph.bounce_plan(rig.token, "bass".into(), None).unwrap();
        let out = rig.fixture.0.join(".music2").join(name);
        plan.render.write(&out, |_| {}).unwrap();
        std::fs::read(out).unwrap()
    };
    std::fs::create_dir_all(rig.fixture.0.join(".music2")).unwrap();
    let (before, revision) = (files(&rig), revs(&rig.graph));
    let (exported, bounced) = (export(&rig, "a.wav"), bounce(&rig, "a-bounce.wav"));
    rig.open("bass", None).unwrap();
    rig.put("bass", 0.0, 8.0, &[(36.0, 0.0, 2.0), (40.0, 1.0, 2.0)]).unwrap();
    rig.play();
    assert!(rms(&rig.until(1.5)) > 1e-3, "the feed sounds live");
    assert_eq!(export(&rig, "b.wav"), exported, "the export is the song's");
    assert_eq!(bounce(&rig, "b-bounce.wav"), bounced, "the bounce is the song's");
    assert_eq!(files(&rig), before, "no file changed");
    assert_eq!(revs(&rig.graph), revision, "no revision");
    assert!(rig.graph.state().unwrap()["undo_name"].is_null(), "no undo step");
}

// FEED-7, MON-14: Keep writes exactly what played as one undo step: a clip over the whole bars it
// covers, the notes the playhead passed (not one written ahead), and takes them out of the feed so
// nothing sounds twice. Undo takes it back.
#[test]
fn keep_makes_one_undo_step_with_exactly_what_played() {
    let mut rig = Rig::new();
    rig.open("bass", None).unwrap();
    rig.put("bass", 0.0, 16.0, &[(36.0, 1.0, 0.5), (38.0, 2.0, 0.5), (40.0, 5.25, 0.5), (43.0, 12.0, 1.0)]).unwrap();
    rig.play();
    rig.until(6.0);
    let before = revs(&rig.graph).0;
    let kept = as_caller(Caller::agent(Some("Claude"), None), || rig.graph.feed_keep("bass", None)).unwrap();
    assert_eq!((&kept["kept"], &kept["as"]), (&json!(3), &json!("clip")), "{kept}");
    assert_eq!(revs(&rig.graph).0, before + 1, "one revision");
    let state = rig.graph.state().unwrap();
    assert_eq!(state["undo_name"]["step"], kept["step"], "one undo step: {}", state["undo_name"]);
    let lane: Value = serde_json::from_str(&rig.fixture.read("data/bass/notes.json")).unwrap();
    let clip = &lane["clips"][0];
    assert_eq!((clip["start"].as_f64(), clip["length"].as_f64()), (Some(0.0), Some(8.0)), "two whole bars: {clip}");
    let notes: Vec<(f64, f64)> =
        clip["notes"].as_array().unwrap().iter().map(|n| (n["start"].as_f64().unwrap(), n["length"].as_f64().unwrap())).collect();
    assert_eq!(notes, [(1.0, 0.5), (2.0, 0.5), (5.25, 0.5)]);
    assert_eq!(rig.state("bass")["notes"], 1, "the kept notes left the feed; the one ahead stays");
    // Nothing more played since: a second Keep has nothing.
    let again = rig.graph.feed_keep("bass", None).unwrap_err().to_string();
    assert!(again.contains("Nothing"), "{again}");
    rig.graph.undo(rig.token).unwrap();
    assert!(!rig.fixture.0.join("data/bass/notes.json").exists(), "undo takes it back: the lane had no file before");
}

// FEED-7, VER-3: with a clip with versions under what played, Keep adds what played inside it as a
// version that waits, by the agent's name; what plays doesn't change.
#[test]
fn keep_as_a_version_adds_one_to_the_clip_under_it() {
    let lane = r#"{"clips": [{"id": "v1", "start": 0, "length": 16, "notes": [{"pitch": 48, "start": 12, "length": 1}],
      "versions": [{"name": "original"}, {"name": "other", "notes": [{"pitch": 50, "start": 0, "length": 1}]}]}]}"#;
    let mut rig = Rig::with(&[("data/bass/notes.json", lane)]);
    rig.open("bass", None).unwrap();
    rig.put("bass", 0.0, 24.0, &[(36.0, 1.0, 0.5), (40.0, 2.0, 0.5)]).unwrap();
    rig.play();
    rig.until(3.0);
    let kept = as_caller(Caller::agent(Some("Claude"), None), || rig.graph.feed_keep("bass", None)).unwrap();
    assert_eq!((&kept["kept"], &kept["as"], &kept["version"]), (&json!(2), &json!("version"), &json!("live")), "{kept}");
    let lane: Value = serde_json::from_str(&rig.fixture.read("data/bass/notes.json")).unwrap();
    let clip = &lane["clips"][0];
    assert_eq!(clip["notes"].as_array().unwrap().len(), 1, "what plays is the same: {clip}");
    let live = clip["versions"].as_array().unwrap().iter().find(|v| v["name"] == "live").cloned().unwrap();
    assert_eq!(live["by"], "Claude");
    assert_eq!(live["notes"].as_array().unwrap().len(), 2, "{live}");
    assert_eq!(rig.state("bass")["notes"], 2, "a version waits: the feed keeps playing them");
}

// FEED-8, RENDER-29: while the session compiles a slow save, a put is taken and reaches the engine:
// it never waits for the session.
#[test]
fn a_put_never_waits_for_a_compile() {
    let slow = |n: u64| {
        format!(
            "import {{ track }} from '@m2/core';\nimport {{ bass }} from '@m2/synths';\nfor (let i = 0; i < {n}; i++) {{}}\n\
             export const b = track('bass', ({{ lane }}) => bass(lane.notes('notes')));\n"
        )
    };
    let fixture = Fixture::new();
    fixture.put("tracks/b.ts", &slow(0));
    fixture.put("song.ts", "import { song } from '@m2/core';\nimport { b } from './tracks/b';\nexport default song(() => b.out);\n");
    let graph = std::sync::Arc::new(fixture.open());
    graph.feed_open(Open { track: "bass".into(), lane: None, low_bars: None }).unwrap();
    graph.transport("play").unwrap();
    let saved = graph.now_ns();
    let saver = {
        let (g, dir) = (graph.clone(), fixture.0.clone());
        std::thread::spawn(move || {
            std::fs::write(dir.join("tracks/b.ts"), slow(100_000_000)).unwrap();
            g.poll()
        })
    };
    eventually("the session compiles", || graph.busy().is_some_and(|since| since >= saved));
    let busy_from = graph.busy().unwrap();
    let t = Instant::now();
    let beat = graph.status().beat;
    graph
        .feed_put(Put {
            feed: "bass".into(),
            from: beat + 0.5,
            to: beat + 4.0,
            notes: vec![Note { pitch: 36.0, start: beat + 0.5, length: 2.0, velocity: 0.8 }],
        })
        .unwrap();
    let sent_ms = t.elapsed().as_secs_f64() * 1e3;
    let seq = graph.feed_seq("bass").expect("sent to the engine");
    eventually("the engine takes it", || graph.applied(seq).is_some());
    let heard = graph.now_ns();
    assert!(saver.join().unwrap(), "the slow save was taken in");
    eventually("the compile's span is kept", || graph.busy_spans().iter().any(|s| s.0 == busy_from));
    let (start, end) = *graph.busy_spans().iter().rev().find(|s| s.0 == busy_from).expect("the compile's span");
    let compile_ms = (end - start) as f64 / 1e6;
    if compile_ms < 1500.0 {
        eprintln!("void: the compile took only {compile_ms:.0} ms");
        return;
    }
    eprintln!("feed: put in {sent_ms:.2} ms, taken {:.0} ms into a {compile_ms:.0} ms compile", (heard - start) as f64 / 1e6);
    assert!(sent_ms < 20.0, "the put took {sent_ms:.1} ms");
    assert!(heard < end, "taken only after the compile ended");
}
