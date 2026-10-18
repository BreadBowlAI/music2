//! Record's routes through a running server (lane B): a take started, played into and stopped over
//! HTTP is written by the take thread, and every window hears it; a read-only window can't record
//! (423, WRITE-18's words).

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::{ServeOptions, start_with};

const SONG: &str = "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
                    const b = track('bass', { name: 'Bass' }, ({ lane }) => kick(lane.notes('notes')));\nexport default song(() => b.out);\n";

/// A project of [`SONG`] in a folder of its own, named `name`.
fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-routes-record-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.3\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(dir.join("song.ts"), SONG).unwrap();
    dir
}

/// `POST path` with `body` and the token: the status and the JSON answer.
fn post(url: &str, path: &str, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = body.to_string();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    let json = answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null));
    (status, json)
}

// Lane B: start, a key, stop, through the routes; the take thread writes the take and every window
// hears it, then the graph event; "Undo recording on Bass".
#[test]
fn a_take_through_the_routes() {
    let dir = project("rw");
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let state = running.state.clone();
    let session = state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let mut events = state.events.subscribe();
    use m2_os::midi::Event;
    state.midi(Event::Plugged("Keys"));
    let (status, take) = post(&running.url, "/api/graph/record/start", &json!({"session": session, "module": "bass", "lane": "notes", "count_in": false}));
    assert_eq!(status, 200, "{take:#}");
    assert_eq!((&take["phase"], &take["module"], &take["lane"]), (&json!("take"), &json!("bass"), &json!("notes")));
    assert_eq!(state.graph.state().unwrap()["record"]["phase"], json!("take"));
    std::thread::sleep(Duration::from_millis(300));
    state.midi(Event::Bytes("Keys", &[0x90, 60, 100]));
    std::thread::sleep(Duration::from_millis(250));
    state.midi(Event::Bytes("Keys", &[0x80, 60, 0]));
    let (status, ended) = post(&running.url, "/api/graph/record/stop", &json!({"session": session}));
    assert_eq!((status, &ended), (200, &json!({"ended": true})));
    assert!(state.graph.status().playing, "R ends the take; play goes on");
    let t = Instant::now();
    let (mut written, mut graph) = (None, false);
    while (written.is_none() || !graph) && t.elapsed() < Duration::from_secs(5) {
        match events.try_recv() {
            Ok(m) => {
                let v: Value = serde_json::from_str(&m).unwrap();
                if v["t"] == "take" && v.get("written").is_some() {
                    written = Some(v);
                } else if v["t"] == "graph" && written.is_some() {
                    graph = true;
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let written = written.expect("the windows hear the take written");
    assert!(graph, "then the graph event");
    assert_eq!((&written["written"]["module"], &written["written"]["lane"]), (&json!("bass"), &json!("notes")));
    assert_eq!(state.graph.undo_names().0.as_deref(), Some("recording on Bass"));
    let lane: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap()).unwrap();
    assert_eq!(lane["clips"][0]["notes"].as_array().map(Vec::len), Some(1), "{lane}");
    assert_eq!(state.graph.state().unwrap()["record"], Value::Null);
    let _ = std::fs::remove_dir_all(&dir);
}

// WRITE-17: in a window open read-only, Record answers 423 with WRITE-18's message, and nothing
// starts.
#[test]
fn a_read_only_window_cant_record() {
    let dir = project("ro");
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let (status, answer) = post(&running.url, "/api/graph/record/start", &json!({"session": session, "module": "bass", "lane": "notes"}));
    assert_eq!(status, 423, "{answer:#}");
    assert!(answer["error"].as_str().unwrap().contains("nothing was changed"), "{answer:#}");
    assert_eq!(post(&running.url, "/api/graph/record/stop", &json!({"session": session})).0, 423);
    assert!(!running.state.graph.recording());
    assert!(!running.state.graph.status().playing, "nothing plays");
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

// The review's F3: Settings names the MIDI keyboards plugged in with no song open (the start
// screen), as the server hears them.
#[test]
fn settings_name_the_keyboards_with_no_song_open() {
    let dir = project("kb");
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    use m2_os::midi::Event;
    running.state.midi(Event::Plugged("Keys"));
    let addr = running.url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /api/settings HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let shown: Value = serde_json::from_str(answer.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(shown["midi"]["devices"], json!(["Keys"]), "{shown:#}");
    let _ = std::fs::remove_dir_all(&dir);
}

const VOX: &str = "import { song, track } from '@m2/core';\n\
                   const vox = track('vox', { name: 'Vox' }, ({ lane }) => lane.audio('clips'));\nexport default song(() => vox.out);\n";

/// `GET path` with the token: the status and the JSON answer.
fn get(url: &str, path: &str) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    (status, answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null)))
}

// The io plan's slice 3: an audio take through the routes. Settings name the input and turn on
// hearing yourself; R on the audio lane records what is injected (`/api/graph/input/inject`); the
// meters say the input's level; every window hears the take start and be written; "Undo recording
// on Vox"; the file holds the tone.
#[test]
fn an_audio_take_through_the_routes() {
    let dir = project("audio");
    std::fs::write(dir.join("song.ts"), VOX).unwrap();
    std::fs::create_dir_all(dir.join("data/vox")).unwrap();
    std::fs::write(dir.join("data/vox/clips.json"), "{\"clips\": []}\n").unwrap();
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let state = running.state.clone();
    state.keep_settings(dir.join("settings.json"));
    let (status, inputs) = get(&running.url, "/api/audio/inputs");
    assert_eq!(status, 200, "{inputs:#}");
    let first = &inputs["inputs"][0];
    assert!(first["id"].is_string() && first["name"].is_string() && first["default"] == json!(true), "{inputs:#}");
    let (status, set) = post(&running.url, "/api/settings", &json!({"input": {"device": first["id"], "monitor": true}}));
    assert_eq!(status, 200, "{set:#}");
    assert_eq!((&set["input"]["device"], &set["input"]["name"], &set["input"]["monitor"]), (&first["id"], &first["name"], &json!(true)), "{set:#}");
    let session = state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let mut events = state.events.subscribe();
    let (status, take) = post(&running.url, "/api/graph/record/start", &json!({"session": session, "module": "vox", "lane": "clips", "count_in": false}));
    assert_eq!(status, 200, "{take:#}");
    assert_eq!((&take["phase"], &take["kind"], &take["hear"]), (&json!("take"), &json!("audio"), &json!(true)), "Settings' Hear yourself reaches the take: {take:#}");
    let tone = json!({"tone": {"hz": 440.0, "seconds": 0.5, "gain": 0.5}});
    let (status, injected) = post(&running.url, "/api/graph/input/inject", &tone);
    assert_eq!(status, 200, "{injected:#}");
    // The meters say the input's level while it records.
    let t = Instant::now();
    let mut level = 0.0;
    while level < 0.4 && t.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(30));
        level = state.graph.meters().and_then(|m| m["input"][0].as_f64()).unwrap_or(0.0).max(level);
    }
    assert!((0.4..=0.51).contains(&level), "the input's level: {level}");
    std::thread::sleep(Duration::from_millis(300));
    let (status, ended) = post(&running.url, "/api/graph/record/stop", &json!({"session": session}));
    assert_eq!((status, &ended), (200, &json!({"ended": true})));
    let t = Instant::now();
    let (mut started, mut written, mut graph) = (false, None, false);
    while (written.is_none() || !graph) && t.elapsed() < Duration::from_secs(5) {
        match events.try_recv() {
            Ok(m) => {
                let v: Value = serde_json::from_str(&m).unwrap();
                started |= v["t"] == "take" && v["kind"] == "audio";
                if v["t"] == "take" && v.get("written").is_some() {
                    written = Some(v);
                } else if v["t"] == "graph" && written.is_some() {
                    graph = true;
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    assert!(started, "every window hears the audio take start");
    let written = written.expect("the windows hear the take written");
    assert!(graph, "then the graph event");
    assert_eq!(written["written"]["asset"], json!("Vox take.wav"), "{written:#}");
    assert_eq!(state.graph.undo_names().0.as_deref(), Some("recording on Vox"));
    let lane: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("data/vox/clips.json")).unwrap()).unwrap();
    assert_eq!(lane["clips"].as_array().map(Vec::len), Some(1), "{lane}");
    let mut wav = hound::WavReader::open(dir.join("assets/Vox take.wav")).unwrap();
    let loudest = wav.samples::<f32>().map(Result::unwrap).fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((0.45..=0.51).contains(&loudest), "the tone is in the take: {loudest}");
    let _ = std::fs::remove_dir_all(&dir);
}

// The review's io-r2 through the routes: `{"unplug": true}` plays a microphone unplugged. Every
// window hears the take written with what it has, in words, and the song plays on.
#[test]
fn the_inject_api_unplugs_the_input() {
    let dir = project("unplug");
    std::fs::write(dir.join("song.ts"), VOX).unwrap();
    std::fs::create_dir_all(dir.join("data/vox")).unwrap();
    std::fs::write(dir.join("data/vox/clips.json"), "{\"clips\": []}\n").unwrap();
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let state = running.state.clone();
    let session = state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let mut events = state.events.subscribe();
    let (status, take) = post(&running.url, "/api/graph/record/start", &json!({"session": session, "module": "vox", "lane": "clips", "count_in": false}));
    assert_eq!(status, 200, "{take:#}");
    post(&running.url, "/api/graph/input/inject", &json!({"tone": {"hz": 440.0, "seconds": 0.5}}));
    std::thread::sleep(Duration::from_millis(300));
    let (status, answer) = post(&running.url, "/api/graph/input/inject", &json!({"unplug": true}));
    assert_eq!(status, 200, "{answer:#}");
    let t = Instant::now();
    let mut written = None;
    while written.is_none() && t.elapsed() < Duration::from_secs(5) {
        match events.try_recv() {
            Ok(m) => written = Some(serde_json::from_str::<Value>(&m).unwrap()).filter(|v| v["t"] == "take" && v.get("written").is_some()),
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let written = written.expect("the windows hear the take written");
    assert_eq!(written["written"]["note"], json!("The sound input was unplugged: the take was kept up to here."), "{written:#}");
    assert!(state.graph.status().playing, "the song plays on");
    let _ = std::fs::remove_dir_all(&dir);
}

// D24: the inject API takes samples or a tone, and says when no take has the input open.
#[test]
fn the_inject_api_needs_an_open_input() {
    let dir = project("inject");
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    running.state.graph.open(&dir).unwrap();
    let (status, answer) = post(&running.url, "/api/graph/input/inject", &json!({"samples": [0.0, 0.5]}));
    assert_eq!(status, 400, "{answer:#}");
    assert_eq!(answer["error"], json!("Nothing is recording from the sound input."));
    let _ = std::fs::remove_dir_all(&dir);
}
