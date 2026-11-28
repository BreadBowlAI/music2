//! Feeds (FEED-1 to FEED-8), in the one route table (AGENT-8): a track's notes an agent writes
//! ahead of the playhead, which music2 plays. Open to agents (AGENT-11, the live note's first
//! decision). Open, put and close are playback, not writes: they work in a read-only window and
//! never wait for the session (RENDER-29). Keep is a write: one undo step.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
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
async fn feed_open(State(s): S, Json(r): Json<OpenReq>) -> ApiResult {
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

/// A put (FEED-4): `{ok, taken, late, lock, written_to, playhead}`.
async fn feed_put(State(s): S, Json(r): Json<PutReq>) -> ApiResult {
    same(&s, r.session)?;
    let state = s.clone();
    // A first write by an agent names it on the Live mark.
    let named =
        s.graph.feeds()["feeds"].as_array().is_some_and(|f| f.iter().any(|f| f["feed"] == r.put.feed.as_str() && f["writer"].is_null()));
    let caller = crate::caller::now().saying(r.by.as_deref(), None);
    let v = blocking(move || crate::caller::as_caller(caller, || state.graph.feed_put(r.put))).await?;
    if named {
        s.graph_changed();
    }
    Ok(Json(v))
}

/// Every open feed's state (FEED-5).
async fn feed_get(State(s): S) -> Json<Value> {
    Json(s.graph.feeds())
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
async fn feed_keep(State(s): S, Json(r): Json<FeedReq>) -> ApiResult {
    same(&s, r.session)?;
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), None);
    let v = blocking(move || crate::caller::as_caller(caller, || state.graph.feed_keep(&r.feed, r.into))).await;
    s.graph_changed();
    Ok(Json(v?))
}

/// Close a feed (FEED-1): its state, `why: "closed"`.
async fn feed_close(State(s): S, Json(r): Json<FeedReq>) -> ApiResult {
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
