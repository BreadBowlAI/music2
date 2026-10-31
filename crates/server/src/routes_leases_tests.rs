//! The lease routes through a running server (WRITE-22a): begin, update, the listing, commit and
//! a changed clip's 409; a read-only window can't begin or commit a drag on clips (423, WRITE-18's
//! words).

use std::io::{Read, Write};

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::{ServeOptions, start_with};

const SONG: &str = "import { song, track } from '@m2/core';\nimport { bass } from '@m2/synths';\n\
                    const b = track('bass', ({ lane }) => bass(lane.notes('notes')));\nexport default song(() => b.out);\n";
const NOTES: &str = r#"{"clips": [{"id": "a", "start": 0, "length": 16, "notes": [{"pitch": "E2", "start": 4, "length": 1}]}]}"#;

/// A project of [`SONG`] in a folder of its own, named `name`.
fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-routes-leases-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data/bass")).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.9\"\n\"@m2/drums\" = \"2\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"2\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(dir.join("song.ts"), SONG).unwrap();
    std::fs::write(dir.join("data/bass/notes.json"), NOTES).unwrap();
    dir
}

/// `method path` with `body` and the token: the status and the JSON answer.
fn call(url: &str, method: &str, path: &str, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = if method == "GET" { String::new() } else { body.to_string() };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    let json = answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null));
    (status, json)
}

/// Clip `a` moved by `by`, named as the timeline names it, with `sha` when given.
fn moved(by: f64, sha: Option<&str>) -> Value {
    let mut clip = json!({"module": "bass", "lane": "notes", "id": "a"});
    if let Some(sha) = sha {
        clip["sha"] = json!(sha);
    }
    json!({"op": "move", "clips": [clip], "by": by})
}

// WRITE-22a: begin, update (an older one dropped), the listing with `kind: "lane"`, and the commit,
// answered as `/api/graph/clips` answers; a clip changed since is 409 with `changed`.
#[test]
fn lane_leases_through_the_routes() {
    let dir = project("rw");
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let url = &running.url;
    let (status, begun) =
        call(url, "POST", "/api/graph/lane/lease/begin", &json!({"session": session, "gesture": moved(1.0, None), "client": "w1"}));
    assert_eq!(status, 200, "{begun:#}");
    assert_eq!((&begun["previewable"], &begun["lanes"]), (&json!(true), &json!([{"module": "bass", "lane": "notes"}])));
    let lease = begun["lease"].clone();
    assert_eq!(call(url, "POST", "/api/graph/lane/lease/update", &json!({"lease": lease, "seq": 2, "gesture": moved(2.0, None)})).0, 200);
    let (_, stale) = call(url, "POST", "/api/graph/lane/lease/update", &json!({"lease": lease, "seq": 2, "gesture": moved(3.0, None)}));
    assert_eq!(stale, json!({"seq": null}), "an older update is dropped");
    assert_eq!(call(url, "POST", "/api/graph/lane/lease/keepalive", &json!({"lease": lease})).0, 200);
    let (_, listed) = call(url, "GET", "/api/graph/leases", &Value::Null);
    assert_eq!((&listed[0]["kind"], &listed[0]["lease"], &listed[0]["client"]), (&json!("lane"), &lease, &json!("w1")), "{listed:#}");
    let (status, done) = call(url, "POST", "/api/graph/lane/lease/commit", &json!({"lease": lease, "gesture": moved(2.0, None)}));
    assert_eq!(status, 200, "{done:#}");
    assert!(done["revision"].is_u64() && done["made"].is_array(), "{done:#}");
    assert!(std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap().contains("\"start\": 2"));
    assert_eq!(call(url, "GET", "/api/graph/leases", &Value::Null).1, json!([]));
    let (status, why) =
        call(url, "POST", "/api/graph/lane/lease/begin", &json!({"session": session, "gesture": moved(1.0, Some("0000000000000000"))}));
    assert_eq!((status, &why["changed"]), (409, &json!(true)), "{why:#}");
    let _ = std::fs::remove_dir_all(&dir);
}

// RENDER-27a: the tempo field's drag through the routes: begin names the entry and its tempo,
// an update is heard (the status follows), the listing says `kind: "tempo"`, and the commit
// answers as `/api/graph/tempo` does, writing data/arrangement.json once.
#[test]
fn a_tempo_lease_through_the_routes() {
    let dir = project("tempo");
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let url = &running.url;
    let (status, begun) = call(url, "POST", "/api/graph/tempo/lease/begin", &json!({"session": session, "at": 3.5, "client": "w1"}));
    assert_eq!((status, &begun["at"], &begun["bpm"], &begun["heard"]), (200, &json!(0.0), &json!(120.0), &json!(true)), "{begun:#}");
    let lease = begun["lease"].clone();
    assert_eq!(call(url, "POST", "/api/graph/tempo/lease/update", &json!({"lease": lease, "seq": 1, "bpm": 130})).1, json!({"seq": 1}));
    let t = std::time::Instant::now();
    while running.state.graph.status().bpm != Some(130.0) {
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "the status never said 130");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (_, listed) = call(url, "GET", "/api/graph/leases", &Value::Null);
    assert_eq!((&listed[0]["kind"], &listed[0]["bpm"]), (&json!("tempo"), &json!(130.0)), "{listed:#}");
    let (status, bad) = call(url, "POST", "/api/graph/tempo/lease/update", &json!({"lease": lease, "seq": 2, "bpm": 5000}));
    assert!(status == 400 && bad["error"].as_str().unwrap().contains("20 to 999"), "{bad:#}");
    let (status, done) = call(url, "POST", "/api/graph/tempo/lease/commit", &json!({"lease": lease, "bpm": 130}));
    assert_eq!(status, 200, "{done:#}");
    assert!(done["revision"].is_u64() && done["after"] == json!("130 BPM"), "{done:#}");
    assert!(std::fs::read_to_string(dir.join("data/arrangement.json")).unwrap().contains("130"));
    assert_eq!(call(url, "GET", "/api/graph/leases", &Value::Null).1, json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}

// WRITE-17: in a window open read-only, a drag's begin and commit answer 423 with WRITE-18's
// message, the tempo's too, and nothing begins.
#[test]
fn a_read_only_window_begins_no_lane_lease() {
    let dir = project("ro");
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let (status, answer) =
        call(&running.url, "POST", "/api/graph/lane/lease/begin", &json!({"session": session, "gesture": moved(1.0, None)}));
    assert_eq!(status, 423, "{answer:#}");
    assert!(answer["error"].as_str().unwrap().contains("nothing was changed"), "{answer:#}");
    assert_eq!(call(&running.url, "POST", "/api/graph/lane/lease/commit", &json!({"lease": 1, "gesture": moved(1.0, None)})).0, 423);
    assert_eq!(call(&running.url, "POST", "/api/graph/tempo/lease/begin", &json!({"session": session, "at": 0})).0, 423);
    assert_eq!(call(&running.url, "POST", "/api/graph/tempo/lease/commit", &json!({"lease": 1, "bpm": 130})).0, 423);
    assert!(running.state.graph.lane_leases().is_empty() && running.state.graph.tempo_leases().is_empty());
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}
