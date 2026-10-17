//! A song opens with no sound device (gaps #13, plan C1): the silent player stands in and says so,
//! plays on its clock, and an output that comes back is used from the next action.

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
