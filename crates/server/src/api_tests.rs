//! The one route table through a running server (AGENT-8, AGENT-11; track 4 of the agents design):
//! every route is declared once, in the table the router, the read-only window's 423 and the
//! tools key's 403 are made from; every example body reads; every write is 423 in a window open
//! read-only; and the tools key gets 403 on every route not for agents.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::api::{self, Route};
use crate::graph::Graph;
use crate::{Running, ServeOptions, start_with};

/// The page's key, and the tools key, of the servers here.
const PAGE: &str = "t0k";
const TOOLS: &str = "t00ls";

const MANIFEST: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.9\"\n\"@m2/drums\" = \"2\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"2\"\n";
const SONG: &str = "import { song, track } from '@m2/core';\nimport { drums } from '@m2/drums';\n\
                    const k = track('kick', ({ lane }) => drums.drum(lane.notes('hits')));\nexport default song(() => k.out);\n";

/// What AGENT-11 names as the musician's: each is in the table, and not for agents.
const THE_MUSICIANS: [(&str, &str); 46] = [
    ("POST", "/api/graph/transport"),
    ("POST", "/api/graph/arm"),
    ("POST", "/api/graph/seek"),
    ("POST", "/api/graph/loop"),
    ("POST", "/api/graph/panic"),
    ("POST", "/api/graph/record/start"),
    ("POST", "/api/graph/record/stop"),
    ("POST", "/api/graph/record/cancel"),
    ("POST", "/api/graph/input/inject"),
    ("POST", "/api/graph/overlay"),
    ("POST", "/api/graph/view"),
    ("POST", "/api/graph/plugin/window"),
    ("POST", "/api/graph/plugin/latch"),
    ("POST", "/api/plugins/scan"),
    ("POST", "/api/graph/audition"),
    ("POST", "/api/graph/midi/inject"),
    ("POST", "/api/graph/redo"),
    ("POST", "/api/graph/undo/skip"),
    ("POST", "/api/graph/step"),
    ("POST", "/api/graph/unsaved"),
    ("POST", "/api/settings"),
    ("POST", "/api/focus"),
    ("POST", "/api/graph/compare"),
    ("POST", "/api/graph/export"),
    ("POST", "/api/graph/stems"),
    ("POST", "/api/graph/preset/save"),
    ("POST", "/api/graph/open"),
    ("POST", "/api/graph/close"),
    ("POST", "/api/project/new"),
    ("POST", "/api/project/save-as"),
    ("POST", "/api/project/rename"),
    ("POST", "/api/project/recent/forget"),
    ("POST", "/api/graph/lock/retry"),
    ("POST", "/api/graph/preview"),
    ("POST", "/api/graph/preview/cancel"),
    ("POST", "/api/graph/lease/begin"),
    ("POST", "/api/graph/lease/update"),
    ("POST", "/api/graph/lease/keepalive"),
    ("POST", "/api/graph/lease/commit"),
    ("POST", "/api/graph/lease/cancel"),
    ("POST", "/api/graph/lane/lease/begin"),
    ("POST", "/api/graph/lane/lease/update"),
    ("POST", "/api/graph/lane/lease/commit"),
    ("POST", "/api/graph/tempo/lease/begin"),
    ("POST", "/api/graph/tempo/lease/update"),
    ("POST", "/api/graph/tempo/lease/commit"),
];

/// Every song change the page makes, which stays open to agents (AGENT-11), and is a write.
const SONG_CHANGES: [(&str, &str); 42] = [
    ("POST", "/api/tx/commit"),
    ("POST", "/api/graph/take"),
    ("POST", "/api/graph/set"),
    ("POST", "/api/graph/connect"),
    ("POST", "/api/graph/disconnect"),
    ("POST", "/api/graph/effect/add"),
    ("POST", "/api/graph/effect/remove"),
    ("POST", "/api/graph/effect/bypass"),
    ("POST", "/api/graph/effect/move"),
    ("POST", "/api/graph/effect/sidechain"),
    ("POST", "/api/graph/lane"),
    ("POST", "/api/graph/relink"),
    ("POST", "/api/graph/data/delete"),
    ("POST", "/api/graph/fix"),
    ("POST", "/api/graph/label"),
    ("POST", "/api/graph/automate"),
    ("POST", "/api/graph/automate/remove"),
    ("POST", "/api/graph/track/add"),
    ("POST", "/api/graph/track/duplicate"),
    ("POST", "/api/graph/track/delete"),
    ("POST", "/api/graph/track/mute"),
    ("POST", "/api/graph/track/instrument"),
    ("POST", "/api/graph/track/rekey"),
    ("POST", "/api/graph/mix/insert"),
    ("POST", "/api/graph/mix/bus"),
    ("POST", "/api/graph/mix/send"),
    ("POST", "/api/graph/mix/route"),
    ("POST", "/api/graph/layout"),
    ("POST", "/api/graph/tempo"),
    ("POST", "/api/graph/meter"),
    ("POST", "/api/graph/title"),
    ("POST", "/api/graph/clips"),
    ("POST", "/api/graph/sections"),
    ("POST", "/api/graph/song-key"),
    ("POST", "/api/graph/markers"),
    ("POST", "/api/graph/audio/import"),
    ("POST", "/api/graph/audio/track"),
    ("POST", "/api/graph/sample"),
    ("POST", "/api/graph/midifile/import"),
    ("POST", "/api/graph/preset/load"),
    ("POST", "/api/graph/undo"),
    ("PUT", "/api/graph/file"),
];

/// A new folder `name` for one test, with a song of one track in it, `song/`.
fn song(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let song = dir.join("song");
    std::fs::create_dir_all(&song).unwrap();
    std::fs::write(song.join("music2.toml"), MANIFEST).unwrap();
    std::fs::write(song.join("song.ts"), SONG).unwrap();
    song
}

/// A server with `song` open, and its session.
fn serve(song: &Path) -> (Running, u64) {
    let options = ServeOptions { port: 0, web_dir: song.join("no-web"), token: TOOLS.into(), page_token: PAGE.into() };
    let running = start_with(options, Graph::silent().stored()).unwrap();
    let session = running.state.graph.open(song).unwrap()["session"].as_u64().unwrap();
    (running, session)
}

/// Every file of the song but its `.music2/`, with what it holds.
fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut all = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if e.file_name() != ".music2" {
                    stack.push(p);
                }
            } else {
                all.insert(p.strip_prefix(dir).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    all
}

/// `example` with the session `session`, and every path on this computer (a string that starts
/// with `/`) moved under `elsewhere`, so nothing outside the test's folder is touched.
fn with(example: &Value, session: u64, elsewhere: &Path) -> Value {
    fn moved(v: &Value, elsewhere: &Path) -> Value {
        match v {
            Value::String(s) if s.starts_with('/') => json!(elsewhere.join(s.trim_start_matches('/'))),
            Value::Array(a) => Value::Array(a.iter().map(|v| moved(v, elsewhere)).collect()),
            Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), moved(v, elsewhere))).collect()),
            v => v.clone(),
        }
    }
    let mut v = moved(example, elsewhere);
    if let Some(o) = v.as_object_mut().filter(|o| o.contains_key("session")) {
        o.insert("session".into(), json!(session));
    }
    v
}

/// `s` as a URL's query keeps it.
fn escaped(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// `method path` with `body` and `key` (a GET's keys in the query): the status, the answer's
/// content type, and its body as text.
fn ask(url: &str, method: &str, path: &str, key: &str, body: &Value) -> (u16, String, String) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let req = if method == "GET" {
        let keys: Vec<String> = body
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| format!("{}={}", escaped(k), escaped(&v.as_str().map_or_else(|| v.to_string(), str::to_string))))
            .collect();
        let query = if keys.is_empty() { String::new() } else { format!("?{}", keys.join("&")) };
        format!("GET {path}{query} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {key}\r\nConnection: close\r\n\r\n")
    } else {
        let body = body.to_string();
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = vec![];
    s.read_to_end(&mut answer).unwrap();
    let answer = String::from_utf8_lossy(&answer).into_owned();
    let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((&answer, ""));
    let status = head[9..12].parse().unwrap();
    let kind =
        head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-type:").map(|k| k.trim().to_string())).unwrap_or_default();
    (status, kind, body.to_string())
}

// AGENT-8: every route is declared once, with its method, its path, the largest body it takes,
// whether it writes, whether agents may call it, words and an example; the router, the 423 and the
// 403 come from that table, so no file of the server keeps a list of paths but through it.
#[test]
fn every_route_is_declared_once() {
    let all = api::all();
    assert!(all.len() > 100, "every route of the app is in the table: {} are", all.len());
    let mut seen = BTreeSet::new();
    for r in &all {
        let at = format!("{} {}", r.method, r.path);
        assert!(seen.insert((r.method, r.path)), "{at} is declared twice");
        assert!(r.path.starts_with("/api/") && ["GET", "POST", "PUT"].contains(&r.method), "{at}");
        assert!(!(r.writes && r.method == "GET"), "{at}: a GET never writes");
        assert!(!r.words.is_empty() && !r.words.ends_with('.'), "{at}: its words, with no full stop: {:?}", r.words);
        assert!(r.example().is_object(), "{at}: its example is a JSON object: {}", r.example);
        assert!(r.limit >= api::BODY, "{at}: the largest body it takes: {}", r.limit);
        assert_eq!(r.json()["limit"], json!(r.limit), "{at}: m2 routes --json lists it");
        assert!(std::ptr::eq(api::find(r.method, r.path).unwrap(), *r), "{at} is found");
    }
    assert_eq!(api::find("HEAD", "/api/graph").map(|r| r.method), Some("GET"), "a HEAD is a GET's");
    assert!(api::find("POST", "/api/graph/nope").is_none());

    // No hand list is left: outside `api.rs`, no file of the server keeps a list of the routes that
    // write or are the musician's. (A route added outside the table answers no one:
    // `a_route_outside_the_table_answers_no_one`.)
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut rust = vec![];
    let mut stack = vec![src.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                rust.push(p);
            }
        }
    }
    let mut found = vec![];
    for file in rust {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if name == "api.rs" || name == "tests.rs" || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for pattern in ["const WRITES", "MUSICIANS"] {
                if code.contains(pattern) {
                    found.push(format!("{}:{}: {}", file.strip_prefix(&src).unwrap().display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "lists of routes outside the table:\n{}", found.join("\n"));
}

/// The routes that close the open song or open another: sent last, so the rest meet the song open.
const ENDS_THE_SONG: [&str; 3] = ["/api/graph/close", "/api/graph/open", "/api/project/new"];

// AGENT-8: each route's example body, sent to a running server by who may send it, reads: it never
// gets axum's plain-text "Failed to deserialize" (a Tx's own refusal is JSON), and the router has
// every route of the table. Each goes with a session that isn't the open song's, so a route that
// reads its body refuses after reading it, and with paths on this computer moved into the test's
// folder.
#[test]
fn every_example_body_reads() {
    let song = song("examples");
    let elsewhere = song.parent().unwrap().join("elsewhere");
    let (running, session) = serve(&song);
    let mut all = api::all();
    assert!(all.len() > 100, "every route of the app is in the table: {} are", all.len());
    all.sort_by_key(|r| ENDS_THE_SONG.contains(&r.path));
    for r in &all {
        let key = if r.agents { TOOLS } else { PAGE };
        let (status, kind, text) = ask(&running.url, r.method, r.path, key, &with(&r.example(), session + 1000, &elsewhere));
        let at = format!("{} {} ({}): {status} {kind}: {text}", r.method, r.path, r.example);
        assert!(!text.contains("Failed to deserialize") && !text.contains("Failed to parse"), "its example doesn't read: {at}");
        assert!(text != r#"{"error":"no such API"}"# && status != 405, "the router hasn't got it: {at}");
        assert!(status != 401 && status != 403, "who may send it is let in: {at}");
    }
    let _ = std::fs::remove_dir_all(song.parent().unwrap());
}

// WRITE-17, AGENT-8: in a window open read-only, every route the table marks as a write answers
// 423 with WRITE-18's words, whoever sends it, before anything is planned; nothing is written. And
// only those: every other route goes on.
#[test]
fn a_read_only_window_answers_every_write_423() {
    let song = song("read-only");
    let elsewhere = song.parent().unwrap().join("elsewhere");
    let held = m2_session::lock::acquire(&song, "m2", m2_session::lock::RETRY).unwrap();
    let (running, session) = serve(&song);
    assert!(running.state.graph.read_only().is_some(), "open read-only");
    let before = files(&song);
    let writes: Vec<&Route> = api::all().into_iter().filter(|r| r.writes).collect();
    for (method, path) in SONG_CHANGES {
        assert!(writes.iter().any(|r| r.method == method && r.path == path), "{method} {path} is a write");
    }
    for r in &writes {
        // The open song's own session: only the gate stands between the request and the song.
        let body = with(&r.example(), session, &elsewhere);
        for key in [PAGE, TOOLS].into_iter().filter(|k| *k == PAGE || r.agents) {
            let (status, _, text) = ask(&running.url, r.method, r.path, key, &body);
            assert_eq!(status, 423, "{} {} with {key}: {text}", r.method, r.path);
            assert!(text.contains("nothing was changed"), "WRITE-18's words: {} {}: {text}", r.method, r.path);
        }
    }
    assert_eq!(files(&song), before, "nothing was written");
    // Only a write: the rest goes on here, as reads, listening and the transport do. (Opening,
    // closing or making a song would end this window; their 403 is tested below.)
    for r in api::all().into_iter().filter(|r| !r.writes) {
        if ENDS_THE_SONG.contains(&r.path) {
            continue;
        }
        let key = if r.agents { TOOLS } else { PAGE };
        let (status, _, text) = ask(&running.url, r.method, r.path, key, &with(&r.example(), session + 1000, &elsewhere));
        assert!(!(status == 423 && text.contains("nothing was changed")), "{} {} isn't a write: {text}", r.method, r.path);
    }
    drop(held);
    let _ = std::fs::remove_dir_all(song.parent().unwrap());
}

// AGENT-11 (D6): the tools key gets 403, with the route's words, on every route the table doesn't
// mark for agents: the transport, seek, the loop, a take, listening mute and solo, the view, plugin
// windows, redo, settings and the songs themselves among them. Nothing they would do happens.
// Every read, and every song change the page makes, stays open to agents.
#[test]
fn the_tools_key_gets_403_on_every_route_not_for_agents() {
    let song = song("musicians");
    let elsewhere = song.parent().unwrap().join("elsewhere");
    let (running, session) = serve(&song);
    let graph = &running.state.graph;
    let all = api::all();
    let musicians: Vec<&&Route> = all.iter().filter(|r| !r.agents).collect();
    for (method, path) in THE_MUSICIANS {
        assert!(musicians.iter().any(|r| r.method == method && r.path == path), "{method} {path} is the musician's");
    }
    for (method, path) in SONG_CHANGES {
        assert!(all.iter().any(|r| r.method == method && r.path == path && r.agents), "{method} {path} is for agents");
    }
    assert!(all.iter().filter(|r| r.method == "GET").all(|r| r.agents), "every read is for agents");
    let before = files(&song);
    for r in &musicians {
        // The open song's own session: only the 403 stands between the agent and what it asks.
        let (status, _, text) = ask(&running.url, r.method, r.path, TOOLS, &with(&r.example(), session, &elsewhere));
        assert_eq!(status, 403, "{} {}: {text}", r.method, r.path);
        let said: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(said["error"], json!(r.refusal()), "{} {}", r.method, r.path);
        assert!(r.refusal().contains("is the musician's"), "{}", r.refusal());
    }
    // Nothing happened: the song is still open, stopped where it was, nothing muted, soloed or
    // recorded, and its files as they were.
    let state = graph.state().unwrap();
    assert_eq!(state["session"], json!(session), "the song is still open");
    assert_eq!((&state["playing"], &state["beat"], &state["loop"]), (&json!(false), &json!(0.0), &Value::Null), "{state:#}");
    assert_eq!((&state["overlays"]["mute"], &state["overlays"]["solo"]), (&json!([]), &json!([])));
    assert_eq!(state["record"], Value::Null, "no take");
    assert_eq!(files(&song), before, "nothing was written");
    let _ = std::fs::remove_dir_all(song.parent().unwrap());
}

// AGENT-8 (the review of ag-routes' F7): the router takes each route's body up to the limit the
// table gives it, and refuses one byte more, 413.
#[test]
fn each_route_takes_bodies_up_to_its_limit() {
    let song = song("limits");
    let (running, session) = serve(&song);
    let route = api::find("POST", "/api/graph/title").unwrap();
    assert_eq!(route.limit, api::BODY, "a route with no `up to`: 2 MiB");
    let body = json!({"session": session, "name": "Night drive"}).to_string();
    // Padded with spaces to the limit, it reads; a byte more is refused.
    for (size, status) in [(route.limit, 200), (route.limit + 1, 413)] {
        let padded = format!("{body}{}", " ".repeat(size - body.len()));
        let addr = running.url.trim_start_matches("http://");
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        let head = format!(
            "POST /api/graph/title HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {PAGE}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            padded.len()
        );
        s.write_all(head.as_bytes()).unwrap();
        let _ = s.write_all(padded.as_bytes());
        let mut answer = vec![];
        let _ = s.read_to_end(&mut answer);
        let answer = String::from_utf8_lossy(&answer);
        assert_eq!(answer.get(9..12), Some(status.to_string().as_str()), "a body of {size} bytes: {}", answer.lines().next().unwrap_or(""));
    }
    let _ = std::fs::remove_dir_all(song.parent().unwrap());
}

/// A server of `routes`, as the app serves them (`routes::served`), with no song open: its url.
fn serving_routes(routes: axum::Router<std::sync::Arc<crate::AppState>>) -> String {
    let mut state = crate::AppState::new(TOOLS.into(), Graph::silent().stored());
    state.page_token = PAGE.into();
    let app = crate::routes::served(std::sync::Arc::new(state), routes, Path::new("no-web"));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
            let _ = axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app).await;
        })
    });
    url
}

// AGENT-8 (the review of ag-routes' F8): only the table's routes answer. Routes added to the router
// outside the table, however they are added, answer no one, whichever key asks: an `/api` path the
// table hasn't got is 404, "no such API"; a method the table hasn't got on a path it has, 405; any
// other path is the page's. (Every route of the table reaches its handler: `every_example_body_reads`.)
#[test]
fn a_route_outside_the_table_answers_no_one() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let hits = std::sync::Arc::new(AtomicUsize::new(0));
    let stray = |hits: &std::sync::Arc<AtomicUsize>| {
        let hits = hits.clone();
        move || {
            hits.fetch_add(1, Ordering::SeqCst);
            async { "a stray route" }
        }
    };
    let service = axum::routing::any(stray(&hits)).with_state::<()>(());
    let routes = api::router()
        .route("/api/graph/stray", axum::routing::any(stray(&hits)))
        .route("/stray", axum::routing::get(stray(&hits)))
        .route_service("/api/stray/service", service)
        .merge(axum::Router::new().route("/api/graph/merged", axum::routing::post(stray(&hits))))
        .nest("/api/nested", axum::Router::new().route("/x", axum::routing::get(stray(&hits))));
    let url = serving_routes(routes);
    let asked = [
        ("GET", "/api/graph/stray", 404),
        ("POST", "/api/graph/stray", 404),
        ("POST", "/api/graph/merged", 404),
        ("GET", "/api/nested/x", 404),
        ("GET", "/api/stray/service", 404),
        ("GET", "/api/graph/transport", 405),
        ("PUT", "/api/graph/title", 405),
    ];
    for key in [PAGE, TOOLS] {
        for (method, path, status) in asked {
            let (got, _, text) = ask(&url, method, path, key, &json!({}));
            assert_eq!(got, status, "{method} {path} with {key}: {text}");
            if status == 404 {
                assert_eq!(text, r#"{"error":"no such API"}"#, "{method} {path}");
            }
        }
        let (_, _, text) = ask(&url, "GET", "/stray", key, &json!({}));
        assert!(!text.contains("a stray route"), "GET /stray is the page's: {text}");
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0, "no route outside the table answered");
}

// AGENT-11 (the review of ag-routes' F9): each refusal reads as one sentence, its route's words
// after one colon, and the contract quotes the transport's as the app says it.
#[test]
fn the_refusals_read_as_the_contract_quotes_them() {
    for r in api::all().into_iter().filter(|r| !r.agents) {
        let said = r.refusal();
        assert_eq!(said.matches(':').count(), 1, "{} {}: {said}", r.method, r.path);
        assert_eq!(said.matches(". ").count(), 1, "{} {}: {said}", r.method, r.path);
    }
    let play = api::find("POST", "/api/graph/transport").unwrap().refusal();
    let contract = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/architecture.md")).unwrap();
    let quoted = contract.lines().find(|l| l.contains("`Route::refusal`")).unwrap_or_default();
    assert!(quoted.contains(&format!("\"{play}\"")), "AGENT-11's Today quotes {play:?}:\n{quoted}");
}

/// Every `.ts` and `.tsx` file of the page (`web/src`), with what it holds, by its path there.
fn page_sources() -> BTreeMap<String, String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src");
    let mut all = BTreeMap::new();
    let mut stack = vec![src.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "ts" || x == "tsx") {
                let name = p.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
                all.insert(name, std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    all
}

// AGENT-2: the window keeps the step of every change it makes as its own as soon as the change's
// answer comes, whatever the route. The page sends every request through one function, `http`
// (web/src/lib/http.ts), which keeps the step of each answer that names one: so a route the page
// starts to call can't be forgotten. Only the answers that name the step they took back (an undo, a
// redo, a skip) are left out. This lists the routes that write that the page calls, and checks that
// none of its requests goes another way.
#[test]
fn the_page_keeps_the_step_of_every_route_it_calls() {
    let page = page_sources();
    assert!(page.len() > 50, "the page's sources: {} files", page.len());
    let way = "lib/http.ts";
    let http = page.get(way).expect("web/src/lib/http.ts: the page's one way to the server");
    // No request goes another way: no `fetch` but in http.ts, no XHR, no beacon.
    let mut other = vec![];
    for (name, text) in &page {
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            let fetches = code.contains("fetch(") && !code.contains("prefetch(");
            if (fetches && name != way) || code.contains("XMLHttpRequest") || code.contains("sendBeacon") || code.contains("new EventSource") {
                other.push(format!("web/src/{name}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(other.is_empty(), "requests that don't go through http (web/src/{way}):\n{}", other.join("\n"));

    // The routes whose answers are kept by no window: exactly those that name the step they took
    // back, each a write.
    let line = http.lines().find(|l| l.contains("const TAKES_BACK")).expect("http.ts names the routes that take a step back: TAKES_BACK");
    let named: BTreeSet<&str> = line.split('"').skip(1).step_by(2).collect();
    assert_eq!(named, BTreeSet::from(["/api/graph/undo", "/api/graph/redo", "/api/graph/undo/skip"]), "{line}");
    for path in &named {
        assert!(api::find("POST", path).is_some_and(|r| r.writes), "{path} is a write of the table");
    }

    // The routes that write which the page calls: each by its whole path, or, in graph.ts, as its
    // `/api/graph` call ("/set").
    let all: String = page.values().map(String::as_str).collect();
    let graph = page.get("lib/graph.ts").unwrap();
    let calls = |r: &Route| {
        let whole = [format!("\"{}\"", r.path), format!("`{}`", r.path), format!("\"{}?", r.path), format!("`{}?", r.path)];
        let short = r.path.strip_prefix("/api/graph").filter(|p| !p.is_empty()).map(|p| format!("(\"{p}\""));
        whole.iter().any(|w| all.contains(w.as_str())) || short.is_some_and(|s| graph.contains(&s))
    };
    let written: Vec<String> = api::all().into_iter().filter(|r| r.writes && calls(r)).map(|r| format!("{} {}", r.method, r.path)).collect();
    for known in ["POST /api/graph/midifile/import", "POST /api/graph/audio/import", "POST /api/graph/sample", "POST /api/library/sample", "POST /api/graph/clips"] {
        assert!(written.iter().any(|w| w == known), "the page calls {known}: the list found {written:?}");
    }
    println!("the routes that write which the page calls, each answer's step kept by http:\n{}", written.join("\n"));
}
