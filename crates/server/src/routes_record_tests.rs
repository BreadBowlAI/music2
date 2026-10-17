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
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.2\"\n";
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
