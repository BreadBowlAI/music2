//! A song opens with no sound device (gaps #13, plan C1): the silent player stands in and says so,
//! plays on its clock, and an output that comes back is used from the next action. Settings'
//! choice of output (gaps #31, C2) makes a new player where the song was.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use serde_json::json;

use super::super::tests::{Fixture, eventually};
use super::*;

/// An output that doesn't start until `there` is set, and the OS lists as the default while
/// `listed` is (one that won't open is listed and not there); `starts` counts the tries.
#[derive(Clone, Default)]
struct Plug {
    there: Arc<AtomicBool>,
    listed: Arc<AtomicBool>,
    starts: Arc<AtomicU32>,
}

impl Plug {
    fn graph(&self) -> Graph {
        let (there, starts) = (self.there.clone(), self.starts.clone());
        let ready = self.listed.clone();
        Graph::starting(
            Box::new(move |_| {
                starts.fetch_add(1, Ordering::Relaxed);
                match there.load(Ordering::Relaxed) {
                    true => Player::silent(),
                    false => Err(anyhow::anyhow!("no audio output device")),
                }
            }),
            Box::new(move || ready.load(Ordering::Relaxed).then(|| "Headphones".to_string())),
        )
    }

    fn starts(&self) -> u32 {
        self.starts.load(Ordering::Relaxed)
    }
}

fn opened(plug: &Plug) -> (Fixture, Graph) {
    let fixture = Fixture::new();
    let graph = plug.graph();
    graph.open(&fixture.0).expect("the song opens");
    (fixture, graph)
}

// gaps #13: with no output, a song opens all the same, and the status says nothing sounds.
#[test]
fn a_song_opens_without_an_audio_device() {
    let plug = Plug::default();
    let (_f, graph) = opened(&plug);
    assert!(graph.no_output());
    let a = graph.activity();
    assert_eq!((a.no_output, a.output), (true, None));
    let state = graph.state().unwrap();
    assert_eq!((&state["no_output"], &state["output"]), (&json!(true), &json!(null)));
    // An output that starts says its name, and nothing is missing.
    let (_g, fine) = opened(&Plug { there: Arc::new(AtomicBool::new(true)), listed: Arc::new(AtomicBool::new(true)), ..Plug::default() });
    let a = fine.activity();
    assert_eq!((a.no_output, a.output.as_deref()), (false, Some("none")));
}

// X23: the start isn't tried at every action, and nothing is toasted over and over: the status
// says it. Tried again once the default output changes, or after 30 s.
#[test]
fn the_silent_player_says_so_once() {
    // The OS lists a default output that won't open.
    let plug = Plug::default();
    plug.listed.store(true, Ordering::Relaxed);
    let (_f, graph) = opened(&plug);
    assert_eq!(plug.starts(), 1);
    for _ in 0..3 {
        graph.transport("play").unwrap();
        graph.seek(1.0).unwrap();
        graph.transport("stop").unwrap();
        graph.look_for_output();
        graph.poll();
    }
    assert_eq!(plug.starts(), 1, "the same default output: not tried again yet");
    assert!(graph.expire().is_empty(), "the status says it: no toast");
    // Thirty seconds on, still none: tried again once, and it stands in still.
    graph.age_failed_start(31);
    graph.look_for_output();
    graph.poll();
    graph.look_for_output();
    graph.poll();
    assert_eq!(plug.starts(), 2);
    assert!(graph.no_output() && graph.expire().is_empty());
}

// gaps #13: the silent player plays on its clock: the playhead moves.
#[test]
fn a_fallback_player_plays_on_its_clock() {
    let plug = Plug::default();
    let (_f, graph) = opened(&plug);
    graph.transport("play").unwrap();
    eventually("the playhead moves", || graph.status().beat > 0.5);
    assert!(graph.status().playing && graph.no_output());
}

// gaps #13: an output that comes back is used from the next action, where the song was, stopped,
// and the user is told once. (`poll` is an action the gates and tests run; the server's own is `catch_up`, every 300 ms, which waits while the actor is busy.)
#[test]
fn a_device_that_comes_back_is_used_from_the_next_action() {
    let plug = Plug::default();
    let (_f, graph) = opened(&plug);
    graph.transport("play").unwrap();
    eventually("the playhead moves", || graph.status().beat > 0.5);
    let before = graph.inner.current_device().unwrap().epoch;
    plug.there.store(true, Ordering::Relaxed);
    plug.listed.store(true, Ordering::Relaxed);
    // Not yet looked for: the stand-in plays on.
    graph.poll();
    assert!(graph.no_output() && graph.status().playing);
    graph.look_for_output();
    assert!(graph.poll(), "the next action loads the song into the new player");
    let now = graph.inner.current_device().unwrap();
    assert!(now.epoch > before && !graph.no_output());
    assert_eq!(graph.activity().output.as_deref(), Some("none"));
    let st = graph.status();
    assert!(!st.playing && st.beat > 0.5, "where it was, stopped: {st:?}");
    assert_eq!(graph.expire(), [BACK]);
    // Said once: nothing more to say.
    graph.look_for_output();
    graph.poll();
    assert!(graph.expire().is_empty());
    assert_eq!(plug.starts(), 2);
    std::thread::sleep(Duration::from_millis(20));
    assert!(graph.generation().is_some(), "the song plays in it");
}

// gaps #31: Settings' output and buffer size make a new player at once, opened with them, and the
// song plays in it where it was, stopped; the window is told play stopped. The same choice again
// changes nothing. Before a song opens, the choice waits for its player.
#[test]
fn choosing_an_output_makes_a_new_player_where_it_was() {
    let asked: Arc<parking_lot::Mutex<Vec<Choice>>> = Arc::default();
    let starts = asked.clone();
    let graph = Graph::starting(
        Box::new(move |c| {
            starts.lock().push(c.clone());
            Player::silent()
        }),
        Box::new(|| Some("Speakers".into())),
    );
    let headphones = Choice { device: Some("Headphones".into()), frames: Some(256) };
    assert!(!graph.set_output(headphones.clone()).unwrap(), "no player yet: nothing stops");
    assert!(graph.output_now().is_none());
    let fixture = Fixture::new();
    graph.open(&fixture.0).unwrap();
    assert_eq!(asked.lock().as_slice(), std::slice::from_ref(&headphones), "the song's player opens with the choice");
    graph.transport("play").unwrap();
    eventually("the playhead moves", || graph.status().beat > 0.5);
    let before = graph.inner.current_device().unwrap().epoch;
    let speakers = Choice { device: None, frames: Some(128) };
    assert!(graph.set_output(speakers.clone()).unwrap(), "it was playing");
    assert_eq!(asked.lock().last(), Some(&speakers));
    let epoch = graph.inner.current_device().unwrap().epoch;
    assert!(epoch > before, "a new player, at once");
    let loaded = graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().map(|p| p.epoch));
    assert_eq!(loaded, Some(epoch), "the song is loaded into it at once");
    let st = graph.status();
    assert!(!st.playing && st.beat > 0.5, "where it was, stopped: {st:?}");
    eventually("its first callback", || graph.output_now().is_some_and(|n| n.frames.is_some()));
    let now = graph.output_now().unwrap();
    assert_eq!((now.device.as_str(), now.rate, now.fell_back, now.no_output), ("none", 48_000, false, false));
    assert_eq!(graph.output_choice(), speakers);
    // The same choice again: the same player.
    let epoch = graph.inner.current_device().unwrap().epoch;
    assert!(!graph.set_output(speakers).unwrap());
    assert_eq!(graph.inner.current_device().unwrap().epoch, epoch);
}

// gaps #31: a change of output keeps where the song was, though another action (the actor's
// catch-up, a settle, the transport) asks for the player the moment the old one is out of its
// slot: the new player starts the song where it was, never at beat 0 (NX-S6).
#[test]
fn an_output_change_keeps_the_beat_beside_another_action() {
    let graph = Graph::starting(Box::new(|_| Player::silent()), Box::new(|| Some("Speakers".into())));
    let fixture = Fixture::new();
    graph.open(&fixture.0).unwrap();
    graph.transport("play").unwrap();
    eventually("the playhead moves", || graph.status().beat > 0.5);
    let inner = Arc::downgrade(&graph.inner);
    *graph.inner.output.changing.lock() = Some(Box::new(move || {
        let inner = inner.upgrade().expect("the graph");
        // On a thread of its own, as another action is: it waits while the player is locked.
        std::thread::spawn(move || drop(inner.device())).join().unwrap();
    }));
    let before = graph.inner.current_device().unwrap().epoch;
    assert!(graph.set_output(Choice { device: None, frames: Some(128) }).unwrap(), "it was playing");
    *graph.inner.output.changing.lock() = None;
    assert!(graph.inner.current_device().unwrap().epoch > before, "a new player");
    let st = graph.status();
    assert!(!st.playing && st.beat > 0.5, "where it was, stopped: {st:?}");
}
