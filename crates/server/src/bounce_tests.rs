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

/// Hooks that hold the render at its first word of progress until `go` says, telling `held`, and
/// note the furthest it got.
fn held(furthest: Arc<std::sync::Mutex<f64>>) -> (Hooks, std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (tell, held) = std::sync::mpsc::channel();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let mut first = true;
    let rendering = move |p: f64| {
        let mut f = furthest.lock().unwrap();
        *f = f.max(p);
        drop(f);
        if std::mem::take(&mut first) {
            tell.send(()).unwrap();
            wait.recv().unwrap();
        }
    };
    (Hooks { rendering: Some(Box::new(rendering)), written: None }, held, go)
}

// RENDER-40: a bounce stopped while it renders adds nothing and leaves no file, not even the
// `assets/` folder it made; the slot is free. The render is held until the stop is asked: no race.
#[test]
fn a_stopped_bounce_leaves_nothing() {
    let temp = Temp::new();
    let (state, token, song) = first_song(&temp, 4000);
    let revision = state.graph.state().unwrap()["view"]["revision"].clone();
    let furthest = Arc::new(std::sync::Mutex::new(0.0));
    let (hooks, rendering, go) = held(furthest.clone());
    let running = {
        let state = state.clone();
        std::thread::spawn(move || run_with(&state, token, "keys".into(), None, hooks))
    };
    rendering.recv().unwrap();
    assert!(ask_stop(&state), "a bounce renders");
    go.send(()).unwrap();
    assert!(stop(&state), "it stopped");
    let e = format!("{:#}", running.join().unwrap().unwrap_err());
    assert_eq!(e, STOPPED);
    assert!(*furthest.lock().unwrap() < 0.5, "it stopped at once, at {}", furthest.lock().unwrap());
    assert!(!song.join("assets").exists(), "no file is left, nor its folder");
    assert_eq!(state.graph.state().unwrap()["view"]["revision"], revision, "nothing was added");
    assert!(!state.exports.running());
    assert!(stop(&state), "with nothing rendering, a stop answers at once");
}

// RENDER-40: a change to the song while it renders stops it at once: nothing is added and nothing
// is left, and it says why. The change is made while the render is held: no race.
#[test]
fn a_change_while_bouncing_refuses_and_leaves_nothing() {
    let temp = Temp::new();
    let (state, token, song) = first_song(&temp, 4000);
    let furthest = Arc::new(std::sync::Mutex::new(0.0));
    let (hooks, rendering, go) = held(furthest.clone());
    let running = {
        let state = state.clone();
        std::thread::spawn(move || run_with(&state, token, "keys".into(), None, hooks))
    };
    rendering.recv().unwrap();
    state.graph.label(token, "hats", "Hi hats").unwrap();
    go.send(()).unwrap();
    let e = running.join().unwrap().unwrap_err().to_string();
    assert_eq!(e, "The song changed while it bounced, so nothing was added. Bounce it again.");
    assert!(*furthest.lock().unwrap() < 0.5, "it stopped at once, at {}", furthest.lock().unwrap());
    assert!(!song.join("assets").exists(), "its files went");
    assert!(!std::fs::read_to_string(song.join("song.ts")).unwrap().contains("keys-audio"));
    assert!(!state.exports.running());
}

// RENDER-40, the review's B1: the watcher taking in the bounce's own files, after they are named
// and before the bounce is placed, is no change to the song: the bounce lands.
#[test]
fn the_watcher_taking_in_its_own_files_is_no_change() {
    let temp = Temp::new();
    let (state, token, song) = first_song(&temp, 32);
    let before = state.graph.shown_revision().unwrap();
    let watcher = state.clone();
    let written = move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while watcher.graph.shown_revision() == Some(before) && std::time::Instant::now() < deadline {
            watcher.graph.watch();
            watcher.graph.watch_wait(std::time::Duration::from_millis(50));
        }
    };
    let done = run_with(&state, token, "keys".into(), None, Hooks { rendering: None, written: Some(Box::new(written)) });
    let after = state.graph.state().unwrap();
    assert!(done.is_ok(), "{done:?}");
    assert!(after["view"]["revision"].as_u64().unwrap() > before + 1, "the watcher took the files in first");
    assert!(std::fs::read_to_string(song.join("song.ts")).unwrap().contains("keys-audio"));
}
