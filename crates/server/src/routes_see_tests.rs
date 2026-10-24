//! What an agent sees over HTTP and the socket (AGENT-6; track 3 of the agents design and its
//! review): the tools key reads the focus record but can't post it, can't open, flip or close
//! Compare, and can't end it by closing or opening a song; every event says its session; a socket
//! that falls behind is told how many events it missed.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::socket::Socket;

/// A server on examples/first-song with both keys, the page's `t0k` and the tools key `t00ls`.
fn serving() -> (std::path::PathBuf, crate::Running, u64) {
    serving_example("first-song")
}

/// A server on a copy of example `name` (without its audio) with both keys.
fn serving_example(name: &str) -> (std::path::PathBuf, crate::Running, u64) {
    let dir = std::env::temp_dir().join(format!("m2-see-{}-{}", std::process::id(), rand_part()));
    copy(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name), &dir);
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

// AGENT-6, AGENT-3: the focus record is the page's to post; the tools key reads it. Compare is the
// musician's: the tools key gets 403 opening, flipping or closing it, and nothing changes.
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
    let v = json!({"session": session, "step": step, "before": true});
    let (status, _) = ask(url, "POST", "/api/graph/compare", "t00ls", &v);
    assert_eq!(status, 403, "an agent can't open Compare");
    assert_eq!(running.state.graph.comparing(), Value::Null);
    let (status, body) = ask(url, "POST", "/api/graph/compare", "t0k", &v);
    assert_eq!(status, 200, "{body}");
    assert_eq!(running.state.graph.comparing()["step"], json!(step));
    let (status, _) = ask(url, "POST", "/api/graph/compare", "t00ls", &json!({"session": session, "step": step, "before": false}));
    assert_eq!(status, 403, "nor flip it");
    let (status, _) = ask(url, "POST", "/api/graph/compare", "t00ls", &json!({"session": session, "open": false}));
    assert_eq!(status, 403, "nor close it");
    assert_eq!(running.state.graph.comparing()["step"], json!(step), "still open");
    let (status, body) = ask(url, "POST", "/api/graph/compare", "t0k", &json!({"session": session, "open": false}));
    assert_eq!((status, &body["comparing"]), (200, &Value::Null), "{body}");
    let _ = std::fs::remove_dir_all(dir);
}

/// An agent's step on the bass notes, by the tools key: its id.
fn agent_step(dir: &std::path::Path, url: &str, session: u64, id: &str) -> u64 {
    let notes = std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap();
    let sha = format!("sha256:{}", m2_song::Sha::of(notes.as_bytes()).hex());
    let tx = json!({"id": format!("tx-{id}"), "base": {"data/bass/notes.json": sha},
        "ops": [{"op": "clips", "lane": ["bass", "notes"], "put": [{"id": id, "start": 0, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 0.25}]}], "delete": []}],
        "expect": {"confined": {"ranges": {}, "lanes": [["bass", "notes"]], "modules": []}}});
    let (status, body) = ask(url, "POST", "/api/tx/commit", "t00ls", &json!({"session": session, "tx": tx, "as": "Claude"}));
    assert_eq!(status, 200, "{body}");
    body["step"].as_u64().unwrap()
}

// AGENT-3 (the review's F4), AGENT-11: while the pill is open, an agent can't end it by closing
// the song, opening it or another, or making a new song. Since track 4 each is the musician's, so
// the tools key gets 403 for them always, not only while the pill is open (it was busy, 423), and
// Compare stays open. The page can.
#[test]
fn while_comparing_an_agent_cant_close_or_open_a_song() {
    let (dir, running, session) = serving();
    let url = &running.url;
    let step = agent_step(&dir, url, session, "a1");
    let (status, body) = ask(url, "POST", "/api/graph/compare", "t0k", &json!({"session": session, "step": step, "before": true}));
    assert_eq!(status, 200, "{body}");
    let (status, body) = ask(url, "POST", "/api/graph/close", "t00ls", &json!({}));
    assert_eq!(status, 403, "closing the song: {body}");
    let (status, body) = ask(url, "POST", "/api/graph/open", "t00ls", &json!({"path": dir}));
    assert_eq!(status, 403, "opening it again: {body}");
    let new = json!({"template": "empty", "name": "Agent's song"});
    let (status, body) = ask(url, "POST", "/api/project/new", "t00ls", &new);
    assert_eq!(status, 403, "a new song: {body}");
    assert_eq!(running.state.graph.comparing()["step"], json!(step), "Compare stays open");
    assert_eq!(running.state.graph.dir().as_deref(), Some(dir.canonicalize().unwrap().as_path()), "on the same song");
    let (status, body) = ask(url, "POST", "/api/graph/close", "t0k", &json!({}));
    assert_eq!(status, 200, "the musician can: {body}");
    assert_eq!(running.state.graph.comparing(), Value::Null);
    let _ = std::fs::remove_dir_all(dir);
}

// AGENT-6 (F6): every event a socket hears says the session of the song it is about, and so does the
// transport's status: an agent never takes another song's event for its own.
#[test]
fn every_event_says_its_session() {
    let (dir, running, session) = serving();
    let url = &running.url;
    let mut socket = Socket::open(url, "/api/ws?token=t00ls").unwrap();
    socket.wait_at_most(Some(Duration::from_secs(10))).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let step = agent_step(&dir, url, session, "a1");
    running.state.toast("a toast");
    running.state.graph_changed();
    running.state.midi_changed();
    let (status, _) = ask(url, "POST", "/api/focus", "t0k", &json!({"session": session, "view": "mix"}));
    assert_eq!(status, 200);
    let (status, _) = ask(url, "POST", "/api/point", "t00ls", &json!({"session": session, "target": "track:bass", "words": "here"}));
    assert_eq!(status, 200);
    let (status, _) = ask(url, "POST", "/api/point", "t0k", &json!({"session": session, "clear": true}));
    assert_eq!(status, 200);
    ask(url, "POST", "/api/graph/compare", "t0k", &json!({"session": session, "step": step, "before": true}));
    ask(url, "POST", "/api/graph/compare", "t0k", &json!({"session": session, "open": false}));
    let (mut seen, t) = (vec![], Instant::now());
    let kinds = ["step", "toast", "graph", "midi", "focus", "point", "compare"];
    while t.elapsed() < Duration::from_secs(10) && !kinds.iter().all(|k| seen.iter().any(|e: &Value| e["t"] == *k)) {
        let Some(m) = socket.message().expect("a message within 10 s") else { break };
        let v: Value = serde_json::from_str(&m).unwrap();
        if v["t"] != "meters" {
            seen.push(v);
        }
    }
    for k in kinds {
        assert!(seen.iter().any(|e| e["t"] == k), "heard a {k}: {seen:?}");
    }
    let without: Vec<&Value> = seen.iter().filter(|e| e["session"] != json!(session)).collect();
    assert!(without.is_empty(), "every event names session {session}: {without:?}");
    let (status, body) = ask(url, "GET", "/api/graph/status", "t00ls", &Value::Null);
    assert_eq!((status, &body["session"]), (200, &json!(session)), "{body}");
    running.state.graph.close();
    let (_, body) = ask(url, "GET", "/api/graph/status", "t00ls", &Value::Null);
    assert_eq!(body["session"], Value::Null, "none with no song open: {body}");
    socket.close();
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

/// The leases live now, as an agent reads them: (kind, lease).
fn live(url: &str) -> Vec<(String, u64)> {
    let (status, leases) = ask(url, "GET", "/api/graph/leases", "t00ls", &Value::Null);
    assert_eq!(status, 200, "{leases}");
    let mut live: Vec<(String, u64)> = leases
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["state"] == "live" || l["state"].is_null())
        .map(|l| (l["kind"].as_str().unwrap_or("knob").to_string(), l["lease"].as_u64().unwrap()))
        .collect();
    live.sort();
    live
}

// AGENT-3, AGENT-11 (the review of ag-routes' F1): a WebSocket opened with the tools key names no
// window. One that gives the id of the window the musician drags in, closed, ends none of its
// drags (a knob's, a clip's, the tempo's), and the musician's next update is heard. The closing of
// the window's own socket ends them (C6).
#[test]
fn a_tools_socket_naming_a_window_ends_none_of_its_drags() {
    let (dir, running, session) = serving();
    let url = &running.url;
    let page = Socket::open(url, "/api/ws?client=w2&token=t0k").unwrap();
    let revision = running.state.graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let knob = json!({"session": session, "revision": revision, "id": "tracks/kick.ts#1/@tune", "client": "w2", "gesture": "d1"});
    let (status, knob) = ask(url, "POST", "/api/graph/lease/begin", "t0k", &knob);
    assert_eq!(status, 200, "a knob drag: {knob}");
    let (status, tempo) = ask(url, "POST", "/api/graph/tempo/lease/begin", "t0k", &json!({"session": session, "at": 0, "client": "w2"}));
    assert_eq!(status, 200, "the tempo's drag: {tempo}");
    let clip = json!({"module": "kick", "lane": "hits", "id": "my53vvkc"});
    let lane = json!({"session": session, "client": "w2", "gesture": {"op": "move", "clips": [clip], "by": 1}});
    let (status, lane) = ask(url, "POST", "/api/graph/lane/lease/begin", "t0k", &lane);
    assert_eq!(status, 200, "a clip's drag: {lane}");
    let held = live(url);
    assert_eq!(held.len(), 3, "{held:?}");
    let agent = Socket::open(url, "/api/ws?client=w2&token=t00ls").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    agent.close();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(live(url), held, "the agent's socket ended none of the window's drags");
    let update = json!({"lease": knob["lease"], "seq": 2, "value": 0.6});
    let (status, body) = ask(url, "POST", "/api/graph/lease/update", "t0k", &update);
    assert_eq!(status, 200, "the musician's next update is heard: {body}");
    // The window's own socket closing ends them.
    page.close();
    let t = Instant::now();
    while !live(url).is_empty() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(live(url), vec![], "the window's own socket ended its drags");
    let _ = std::fs::remove_dir_all(dir);
}

/// What the song's `assets/` holds, hidden part files too; None when there is no `assets/`.
fn assets(dir: &std::path::Path) -> Option<Vec<String>> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir.join("assets")).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    Some(names)
}

/// A mono WAV of a quarter second at `path`.
fn wav(path: &std::path::Path) {
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..12_000 {
        w.write_sample(((i as f32 * 0.03).sin() * 8_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// Whether the Compare pill stays open on `step` for `long`: the watcher took nothing in meanwhile.
fn stays_open(graph: &Graph, step: u64, long: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < long {
        if graph.comparing()["step"] != json!(step) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

// AGENT-3 (the review of ag-routes' F2): while the musician compares, an agent's audio import and a
// Sampler's file are busy, and nothing of them reaches the song: its `assets/` is as it was, with
// no part file either, and the pill stays open (the watcher took nothing in). Once the pill closes,
// the same import lands, as one step named on the server.
#[test]
fn an_agents_import_while_comparing_copies_nothing() {
    let (dir, running, session) = serving();
    let url = &running.url;
    let outside = dir.with_extension("files");
    std::fs::create_dir_all(&outside).unwrap();
    let vox = outside.join("vox.wav");
    wav(&vox);
    let step = agent_step(&dir, url, session, "a1");
    let (status, body) = ask(url, "POST", "/api/graph/compare", "t0k", &json!({"session": session, "step": step, "before": true}));
    assert_eq!(status, 200, "{body}");
    let before = assets(&dir);
    let import = json!({"session": session, "paths": [vox], "to": {"kind": "below", "beat": 0}});
    let (status, body) = ask(url, "POST", "/api/graph/audio/import", "t00ls", &import);
    assert_eq!((status, &body["busy"]["what"]), (423, &json!("compare")), "{body}");
    assert_eq!(assets(&dir), before, "nothing was copied into the song");
    assert!(
        stays_open(&running.state.graph, step, Duration::from_millis(2500)),
        "the pill stays open: {}",
        running.state.graph.comparing()
    );
    assert_eq!(assets(&dir), before);
    // Closed, the import lands.
    let (status, body) = ask(url, "POST", "/api/graph/compare", "t0k", &json!({"session": session, "open": false}));
    assert_eq!(status, 200, "{body}");
    let (status, body) = ask(url, "POST", "/api/graph/audio/import", "t00ls", &import);
    assert_eq!(status, 200, "{body}");
    assert_eq!(assets(&dir), Some(vec!["vox.wav".to_string()]), "{body}");
    assert_eq!((&body["undo_name"]["name"], &body["undo_name"]["by"]), (&json!("importing vox.wav"), &json!("agent")), "{body}");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(outside);

    // A Sampler's file, the same.
    let (dir, running, session) = serving_example("sampler");
    let url = &running.url;
    let outside = dir.with_extension("files");
    std::fs::create_dir_all(&outside).unwrap();
    let clap = outside.join("clap.wav");
    wav(&clap);
    let (status, body) = ask(url, "POST", "/api/graph/title", "t00ls", &json!({"session": session, "name": "Claps"}));
    assert_eq!(status, 200, "{body}");
    let step = body["step"].as_u64().unwrap();
    let (status, body) = ask(url, "POST", "/api/graph/compare", "t0k", &json!({"session": session, "step": step, "before": true}));
    assert_eq!(status, 200, "{body}");
    let revision = running.state.graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let before = assets(&dir);
    let sample = json!({"session": session, "at": revision, "call": "tracks/drums.ts#1", "file": clap});
    let (status, body) = ask(url, "POST", "/api/graph/sample", "t00ls", &sample);
    assert_eq!((status, &body["busy"]["what"]), (423, &json!("compare")), "{body}");
    assert_eq!(assets(&dir), before, "nothing was copied into the song");
    assert!(
        stays_open(&running.state.graph, step, Duration::from_millis(2500)),
        "the pill stays open: {}",
        running.state.graph.comparing()
    );
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(outside);
}
