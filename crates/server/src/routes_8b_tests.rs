//! 8b's routes through a running server: the token check covers them, and mute and solo answer
//! with the overlay state, in words when refused; the server's own thread saves them.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::{ServeOptions, start_with};

/// `POST path` with `body`, with the token or not: the status and the JSON answer.
fn post(url: &str, path: &str, token: Option<&str>, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = body.to_string();
    let auth = token.map_or(String::new(), |t| format!("x-m2-token: {t}\r\n"));
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    let json = answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null));
    (status, json)
}

// MON-1, MON-4, G5: `POST /api/graph/overlay` is behind the token, mutes and solos, refuses the
// song in words, and its change is saved by the server's thread soon after.
#[test]
fn mute_and_solo_answer_with_the_overlay_state() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.1\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
         const kick = track('kick', () => osc.sine(55));\nexport default song(() => kick.out);\n",
    )
    .unwrap();
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let state = running.state.graph.open(&dir).unwrap();
    let session = state["session"].as_u64().unwrap();
    let url = &running.url;
    assert_eq!(post(url, "/api/graph/overlay", None, &json!({"session": session})).0, 401, "behind the token");
    let (status, answer) = post(url, "/api/graph/overlay", Some("t0k"), &json!({"session": session, "mute": {"kick": true}}));
    assert_eq!((status, &answer["mute"], &answer["heard"]["kick"]), (200, &json!(["kick"]), &json!("muted")), "{answer:#}");
    let (status, answer) = post(url, "/api/graph/overlay", Some("t0k"), &json!({"session": session, "solo": {"song": true}}));
    assert_eq!(status, 400);
    assert!(answer["error"].as_str().unwrap().contains("song can't be muted or soloed"), "{answer:#}");
    let file = dir.join(".music2/overlays.json");
    let start = Instant::now();
    while !file.exists() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved, json!({"mute": ["kick"], "solo": []}));
    let _ = std::fs::remove_dir_all(&dir);
}
