//! Bounce to audio in the app (RENDER-40): its route puts the bounce in the song as one undo step;
//! a stopped bounce, and one whose song changed while it rendered, leave nothing.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use serde_json::Value;

use super::*;
use crate::graph::Graph;

struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("m2-bounce-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

/// The app's state with a copy of first-song open in `temp`, its Keys clip `beats` long; its
/// session, and the song's folder.
fn first_song(temp: &Temp, beats: u32) -> (Arc<AppState>, u64, PathBuf) {
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let chords = std::fs::read_to_string(song.join("data/keys/chords.json")).unwrap();
    let long = chords.replacen("\"start\": 0, \"length\": 32, \"notes\"", &format!("\"start\": 0, \"length\": {beats}, \"notes\""), 1);
    assert!(beats == 32 || long != chords, "the clip's length is written as it was");
    std::fs::write(song.join("data/keys/chords.json"), long).unwrap();
    let state = Arc::new(AppState::new(String::new(), Graph::silent().stored()));
    let token = state.graph.open(&song).unwrap()["session"].as_u64().unwrap();
    (state, token, song)
}

/// The names in the song's `assets/`, sorted: none when it isn't there.
fn assets(song: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(song.join("assets"))
        .into_iter()
        .flatten()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    n.sort();
    n
}

/// Wait for the first `bounce` event that says how far it is.
fn until_rendering(events: &mut tokio::sync::broadcast::Receiver<String>) {
    loop {
        let e: Value = serde_json::from_str(&events.blocking_recv().unwrap()).unwrap();
        if e["t"] == "bounce" && e.get("progress").is_some() {
            return;
        }
    }
}

// RENDER-40: the route bounces Keys into a free name, adds Keys audio after it and mutes Keys, as
// one step named "bouncing Keys"; a second bounce of it is refused in words (it is muted now).
#[test]
fn the_route_bounces_in_place_as_one_step() {
    let temp = Temp::new();
    let (state, token, song) = first_song(&temp, 32);
    let mut events = state.events.subscribe();
    let done = run(&state, token, "keys".into(), None).unwrap();
    let said = (&done["module"], &done["label"], &done["muted"], &done["asset"]);
    assert_eq!(said, (&json!("keys-audio"), &json!("Keys audio"), &json!("keys"), &json!("keys-bounce.wav")), "{done}");
    assert_eq!(assets(&song), ["keys-bounce.wav", "keys-bounce.wav.json"]);
    let now = state.graph.state().unwrap();
    assert_eq!(now["undo_name"]["name"], json!("bouncing Keys"), "{}", now["undo_name"]);
    let mut heard = vec![];
    while let Ok(e) = events.try_recv() {
        let e: Value = serde_json::from_str(&e).unwrap();
        if e["t"] == "bounce" {
            heard.push(e);
        }
    }
    assert!(heard.first().is_some_and(|e| e["progress"] == json!(0.0) && e["label"] == "Keys"), "{heard:?}");
    assert!(heard.last().is_some_and(|e| e["done"] == json!(true) && e["module"] == "keys-audio"), "{heard:?}");
    let e = run(&state, token, "keys".into(), None).unwrap_err().to_string();
    assert!(e.contains("Keys is muted in the song"), "{e}");
    assert!(!state.exports.running(), "the slot is free");
}

// RENDER-40: a bounce stopped while it renders adds nothing and leaves no file; the slot is free.
#[test]
fn a_stopped_bounce_leaves_nothing() {
    let temp = Temp::new();
    let (state, token, song) = first_song(&temp, 4000);
    let mut events = state.events.subscribe();
    let revision = state.graph.state().unwrap()["view"]["revision"].clone();
    let running = {
        let state = state.clone();
        std::thread::spawn(move || run(&state, token, "keys".into(), None))
    };
    until_rendering(&mut events);
    assert!(stop(&state), "it stopped");
    let e = format!("{:#}", running.join().unwrap().unwrap_err());
    assert!(e.contains("stopped"), "{e}");
    assert_eq!(assets(&song), Vec::<String>::new(), "no file is left");
    assert_eq!(state.graph.state().unwrap()["view"]["revision"], revision, "nothing was added");
    assert!(!state.exports.running());
}

// RENDER-40: when the song changes while it renders, nothing is added and its files go: the
// bounce is of a song that isn't there now.
#[test]
fn a_change_while_bouncing_refuses_and_leaves_nothing() {
    let temp = Temp::new();
    let (state, token, song) = first_song(&temp, 4000);
    let mut events = state.events.subscribe();
    let running = {
        let state = state.clone();
        std::thread::spawn(move || run(&state, token, "keys".into(), None))
    };
    until_rendering(&mut events);
    state.graph.label(token, "hats", "Hi hats").unwrap();
    let e = running.join().unwrap().unwrap_err().to_string();
    assert!(e.contains("The song changed while it bounced"), "{e}");
    assert_eq!(assets(&song), Vec::<String>::new(), "its files went");
    assert!(!std::fs::read_to_string(song.join("song.ts")).unwrap().contains("keys-audio"));
    assert!(!state.exports.running());
}
