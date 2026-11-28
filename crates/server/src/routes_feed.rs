//! Feeds (FEED-1 to FEED-8), in the one route table (AGENT-8): a track's notes an agent writes
//! ahead of the playhead, which music2 plays. Open to agents (AGENT-11, the live note's first
//! decision). Open, put and close are playback, not writes: they work in a read-only window and
//! never wait for the session (RENDER-29). Keep is a write: one undo step.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::graph::feeds::{As, Open, Put};
use crate::routes::{ApiError, blocking};

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/feed/open" => feed_open; reads, agents,
        "Open a feed on a track: an agent writes its notes ahead of the playhead and music2 plays them, with the track's clips, while the musician plays the song; low_bars (4 unless given) is the buffer under which it says low",
        r#"{"session": 7, "track": "bass", "low_bars": 4}"#;
    POST "/api/feed/put" => feed_put; reads, agents,
        "Write a feed's notes from beat `from` to beat `to` (song beats; each note's start too): they replace what it held there past the lock; notes before the lock are left out and counted late. Answers taken, late, lock, written_to and playhead",
        r#"{"session": 7, "feed": "bass", "from": 64, "to": 80, "notes": [{"pitch": "A1", "start": 64, "length": 0.5, "velocity": 0.9}]}"#;
    GET "/api/feed" => feed_get; reads, agents,
        "Every open feed's state, as its feed events say it: the playhead, the lock, written_to, the buffer in beats, bars and seconds, empties and late",
        r#"{}"#;
    POST "/api/feed/keep" => feed_keep; writes, agents,
        "Keep what played on a feed in the song, as one undo step: into a new clip (into: clip), or a version of the notes clip under it (into: version); without into, a version when that clip has versions",
        r#"{"session": 7, "feed": "bass", "into": "clip"}"#;
    POST "/api/feed/close" => feed_close; reads, agents,
        "Close a feed: its notes end at once; what played stays for Keep until the song closes",
        r#"{"session": 7, "feed": "bass"}"#;
}

/// A body as its act takes it, or a refusal in words, `{"error": …}`, as every feed refusal is
/// (`feed/1`, FEED-8): never the extractor's own plain-text answer, which a relay can't read.
fn read<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    body.map(|Json(t)| t).map_err(|e| {
        // The innermost cause says what: `missing field `to` at line 1 column 47`.
        let mut why: &dyn std::error::Error = &e;
        while let Some(inner) = why.source() {
            why = inner;
        }
        ApiError::from(anyhow::anyhow!("The body doesn't read: {why}."))
    })
}

/// The open song's session is `session`, else the request is for a song gone.
fn same(s: &AppState, session: u64) -> Result<(), ApiError> {
    match s.graph.session() {
        Some(open) if open == session => Ok(()),
        _ => Err(ApiError::from(anyhow::anyhow!("the open song changed: read the state again (session {session} isn't open)"))),
    }
}

#[derive(Deserialize)]
struct OpenReq {
    session: u64,
    #[serde(flatten)]
    open: Open,
    /// An agent's name, as the body gives it (else the `x-m2-as` header's).
    #[serde(default, rename = "as")]
    by: Option<String>,
}

/// Open a feed (FEED-1): its state, `why: "open"`; every window hears it, and draws its Live mark.
async fn feed_open(State(s): S, body: Result<Json<OpenReq>, JsonRejection>) -> ApiResult {
    let r = read(body)?;
    same(&s, r.session)?;
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), None);
    let v = blocking(move || crate::caller::as_caller(caller, || state.graph.feed_open(r.open))).await;
    s.graph_changed();
    Ok(Json(v?))
}

#[derive(Deserialize)]
struct PutReq {
    session: u64,
    #[serde(flatten)]
    put: Put,
    #[serde(default, rename = "as")]
    by: Option<String>,
}

/// A put (FEED-4): `{ok, taken, late, lock, written_to, playhead}`. When it changes who wrote last,
/// every window hears the graph changed, so the Live mark names the newest writer (FEED-1). The
/// control plane's lock is taken on a blocking thread, never here.
async fn feed_put(State(s): S, body: Result<Json<PutReq>, JsonRejection>) -> ApiResult {
    let r = read(body)?;
    same(&s, r.session)?;
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), None);
    let (v, named) = blocking(move || {
        crate::caller::as_caller(caller, || {
            let before = state.graph.feed_writer(&r.put.feed);
            let feed = r.put.feed.clone();
            let v = state.graph.feed_put(r.put)?;
            Ok((v, state.graph.feed_writer(&feed) != before))
        })
    })
    .await?;
    if named {
        s.graph_changed();
    }
    Ok(Json(v))
}

/// Every open feed's state (FEED-5).
async fn feed_get(State(s): S) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || Ok(state.graph.feeds())).await?))
}

#[derive(Deserialize)]
struct FeedReq {
    session: u64,
    feed: String,
    /// Keep: into a clip or a version.
    #[serde(default)]
    into: Option<As>,
    #[serde(default, rename = "as")]
    by: Option<String>,
}

/// Keep what played (FEED-7): one Tx, one undo step. `{kept, as, step, revision, …}`.
async fn feed_keep(State(s): S, body: Result<Json<FeedReq>, JsonRejection>) -> ApiResult {
    let r = read(body)?;
    same(&s, r.session)?;
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), None);
    let v = blocking(move || crate::caller::as_caller(caller, || state.graph.feed_keep(&r.feed, r.into))).await;
    s.graph_changed();
    Ok(Json(v?))
}

/// Close a feed (FEED-1): its state, `why: "closed"`.
async fn feed_close(State(s): S, body: Result<Json<FeedReq>, JsonRejection>) -> ApiResult {
    let r = read(body)?;
    same(&s, r.session)?;
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), None);
    let v = blocking(move || crate::caller::as_caller(caller, || state.graph.feed_close(&r.feed))).await;
    s.graph_changed();
    Ok(Json(v?))
}

#[cfg(test)]
mod tests {
    use crate::api;

    use std::io::{Read, Write};

    use serde_json::{Value, json};

    /// `method path` with `body`, the tools key, as the agent `who`: the status and the answer.
    fn ask(url: &str, method: &str, path: &str, who: &str, body: &Value) -> (u16, Value) {
        let addr = url.trim_start_matches("http://");
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        let body = if body.is_null() { String::new() } else { body.to_string() };
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t00ls\r\nx-m2-as: {who}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut answer = String::new();
        s.read_to_string(&mut answer).unwrap();
        let status = answer[9..12].parse().unwrap();
        (status, answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null)))
    }

    fn copy(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (f, t) = (e.path(), to.join(e.file_name()));
            if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
        }
    }

    // FEED-1: a put by another agent than the one that wrote last tells every window the graph
    // changed, so the Live mark names the newest writer; one by the same agent doesn't.
    #[test]
    fn a_new_writer_is_told_to_the_page() {
        let base = std::env::temp_dir().join(format!("m2-feed-writer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("song");
        copy(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir);
        let options = crate::ServeOptions { port: 0, web_dir: base.join("no-web"), token: "t00ls".into(), page_token: "p4ge".into() };
        let running = crate::start_with(options, crate::graph::Graph::silent().stored()).unwrap();
        let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
        let url = running.url.clone();
        assert_eq!(ask(&url, "POST", "/api/feed/open", "Ada", &json!({"session": session, "track": "keys"})).0, 200);
        let put = |who: &str, pitch: u32| {
            let body =
                json!({"session": session, "feed": "keys", "from": 0, "to": 4, "notes": [{"pitch": pitch, "start": 1, "length": 0.5}]});
            let (status, v) = ask(&url, "POST", "/api/feed/put", who, &body);
            assert_eq!(status, 200, "{v}");
        };
        let (_, mut events, _) = running.state.subscribe(false);
        let graph_events = |events: &mut tokio::sync::broadcast::Receiver<String>| {
            let mut n = 0;
            while let Ok(m) = events.try_recv() {
                n += usize::from(serde_json::from_str::<Value>(&m).is_ok_and(|v| v["t"] == "graph"));
            }
            n
        };
        put("Ada", 60);
        assert_eq!(graph_events(&mut events), 0, "the same writer: nothing for the mark to change");
        put("Grace", 62);
        assert_eq!(graph_events(&mut events), 1, "a new writer: the page reads the state again");
        assert_eq!(running.state.graph.state().unwrap()["feeds"][0]["writer"], "Grace");
        let _ = std::fs::remove_dir_all(&base);
    }

    // AGENT-11, FEED-8: the feed routes are declared once, each open to agents; only Keep writes.
    #[test]
    fn the_feed_routes_are_open_to_agents() {
        let all = api::all();
        for (method, path, writes) in [
            ("POST", "/api/feed/open", false),
            ("POST", "/api/feed/put", false),
            ("GET", "/api/feed", false),
            ("POST", "/api/feed/keep", true),
            ("POST", "/api/feed/close", false),
        ] {
            let found: Vec<_> = all.iter().filter(|r| r.method == method && r.path == path).collect();
            assert_eq!(found.len(), 1, "{method} {path} is declared once");
            assert!(found[0].agents, "{method} {path} is open to agents");
            assert_eq!(found[0].writes, writes, "{method} {path}");
        }
    }
}

#[cfg(test)]
#[path = "feed_protocol_tests.rs"]
mod protocol_tests;
