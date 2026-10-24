//! What an agent sees over HTTP and the socket (AGENT-6; track 3 of the agents design): the tools
//! key reads the focus record but can't post it, and can't flip or close Compare; a socket that
//! falls behind is told how many events it missed.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::socket::Socket;

/// A server on examples/first-song with both keys, the page's `t0k` and the tools key `t00ls`.
fn serving() -> (std::path::PathBuf, crate::Running, u64) {
    let dir = std::env::temp_dir().join(format!("m2-see-{}-{}", std::process::id(), rand_part()));
    copy(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir);
    let options = crate::ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() };
    let running = crate::start_with(options, Graph::silent()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    (dir, running, session)
}

fn rand_part() -> u64 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A copy of `from` at `to`, but its audio.
fn copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() {
            copy(&f, &t);
        } else if f.extension().is_none_or(|x| x != "wav") {
            std::fs::copy(&f, &t).unwrap();
        }
    }
}

/// `method path` with `key` and `body`: the status and the JSON answer.
fn ask(url: &str, method: &str, path: &str, key: &str, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = if body.is_null() { String::new() } else { body.to_string() };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    (status, answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null)))
}

// AGENT-6, AGENT-3: the focus record is the page's to post; the tools key reads it. Compare's flips
// and close are the musician's: the tools key gets 403 and nothing changes.
#[test]
fn the_tools_key_reads_focus_but_cant_post_it_or_compare() {
    let (dir, running, session) = serving();
    let url = &running.url;
    let focus = json!({"session": session, "view": "mix", "track": "kick"});
    let (status, _) = ask(url, "POST", "/api/focus", "t00ls", &focus);
    assert_eq!(status, 403, "an agent can't post the focus");
    let (status, body) = ask(url, "POST", "/api/focus", "t0k", &focus);
    assert_eq!((status, &body["track"]), (200, &json!("kick")), "{body}");
    let (status, body) = ask(url, "GET", "/api/focus", "t00ls", &Value::Null);
    assert_eq!((status, &body["view"], &body["track"]), (200, &json!("mix"), &json!("kick")), "an agent reads it: {body}");
    // An agent's step, which only the page may compare.
    let notes = std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap();
    let sha = format!("sha256:{}", m2_song::Sha::of(notes.as_bytes()).hex());
    let tx = json!({"id": "tx-agent", "base": {"data/bass/notes.json": sha},
        "ops": [{"op": "clips", "lane": ["bass", "notes"], "put": [{"id": "a1", "start": 0, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 0.25}]}], "delete": []}],
        "expect": {"confined": {"ranges": {}, "lanes": [["bass", "notes"]], "modules": []}}});
    let (status, body) = ask(url, "POST", "/api/tx/commit", "t00ls", &json!({"session": session, "tx": tx, "as": "Claude"}));
    assert_eq!(status, 200, "{body}");
    let step = body["step"].as_u64().unwrap();
    let (status, _) =
        ask(url, "POST", "/api/graph/undo", "t00ls", &json!({"session": session, "step": step, "compare": true, "as": "Claude"}));
    assert_eq!(status, 403, "an agent can't open Compare");
    assert_eq!(running.state.graph.comparing(), Value::Null);
    let (status, body) = ask(url, "POST", "/api/graph/undo", "t0k", &json!({"session": session, "step": step, "compare": true}));
    assert_eq!(status, 200, "{body}");
    assert_eq!(running.state.graph.comparing()["step"], json!(step));
    let (status, _) = ask(url, "POST", "/api/graph/compare", "t00ls", &json!({"session": session, "open": false}));
    assert_eq!(status, 403, "nor close it");
    assert_eq!(running.state.graph.comparing()["step"], json!(step), "still open");
    let (status, body) = ask(url, "POST", "/api/graph/compare", "t0k", &json!({"session": session, "open": false}));
    assert_eq!((status, &body["comparing"]), (200, &Value::Null), "{body}");
    let _ = std::fs::remove_dir_all(dir);
}

// AGENT-6: a socket that falls behind (it reads nothing while far more is sent than its buffers
// and the event channel hold) is told how many events it missed, then hears the rest.
#[test]
fn a_socket_that_falls_behind_hears_lagged() {
    let (dir, running, _) = serving();
    let mut socket = Socket::open(&running.url, "/api/ws?token=t00ls").unwrap();
    socket.wait_at_most(Some(Duration::from_secs(10))).unwrap();
    // Let the server's side subscribe before the flood.
    std::thread::sleep(Duration::from_millis(300));
    let big = "x".repeat(64 << 10);
    for i in 0..1000 {
        running.state.toast(format!("{i} {big}"));
    }
    let (t, mut missed, mut after) = (Instant::now(), None, 0);
    while t.elapsed() < Duration::from_secs(60) {
        let Some(m) = socket.message().expect("a message within 10 s") else { break };
        let v: Value = serde_json::from_str(&m).unwrap();
        match (&missed, v["t"].as_str()) {
            (None, Some("lagged")) => missed = v["missed"].as_u64(),
            (Some(_), Some("toast")) => {
                after += 1;
                if v["text"].as_str().is_some_and(|t| t.starts_with("999 ")) {
                    break;
                }
            }
            _ => {}
        }
    }
    let missed = missed.expect("a lagged event");
    assert!(missed > 0 && after > 0, "it missed {missed}, then heard {after} more");
    socket.close();
    let _ = std::fs::remove_dir_all(dir);
}
