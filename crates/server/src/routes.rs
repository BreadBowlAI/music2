//! HTTP + WebSocket API for the open song project. Thin: every edit goes through [`Graph`](crate::graph::Graph).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};

use crate::AppState;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// A route's error: 400 with its words, or 423 with what the musician holds when an agent's write
/// is busy (AGENT-3).
pub struct ApiError(StatusCode, Value);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        let e = e.into();
        match e.downcast_ref::<crate::graph::busy::Busy>() {
            Some(b) => ApiError(StatusCode::LOCKED, b.json()),
            None => ApiError(StatusCode::BAD_REQUEST, json!({"error": format!("{e:#}")})),
        }
    }
}

fn err(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, json!({"error": msg.into()}))
}

/// Run `f` off the async threads, as the request's caller (AGENT-10).
pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(|e| err(e.to_string()))?.map_err(ApiError::from)
}

crate::api::routes! {
    GET "/api/ws" => ws; reads, agents,
        "Hear what happens in the app as it happens: a WebSocket of events, one JSON object each, as m2 watch prints them",
        r#"{}"#;
    GET "/api/graph" => graph_get; reads, agents,
        "The open song as the app shows it: its session, the graph view with its revision and every knob's id, the timeline, the Mix view, problems, undo's names and where playback is",
        r#"{}"#;
    POST "/api/graph/open" => graph_open; reads, musician,
        "Open a song",
        r#"{"path": "/Users/you/Music/music2/Night drive"}"#;
    POST "/api/graph/close" => graph_close; reads, musician,
        "Close the open song",
        r#"{}"#;
    GET "/api/graph/status" => graph_status; reads, agents,
        "Where playback is: playing or not, the beat, the tempo there and the loop, with the open song's session",
        r#"{}"#;
    POST "/api/graph/preview" => graph_preview; writes, musician,
        "Hear a knob's new value while it is dragged (the first one begins the drag)",
        r#"{"session": 7, "revision": 42, "id": "tracks/keys.ts#1/@brightness", "gesture": "drag-1", "value": 0.6, "client": "w1"}"#;
    POST "/api/graph/preview/cancel" => graph_cancel_preview; reads, musician,
        "End a knob drag without saving, so the code's value plays again",
        r#"{"session": 7, "id": "tracks/keys.ts#1/@brightness", "gesture": "drag-1"}"#;
    POST "/api/graph/set" => graph_set; writes, agents,
        "Write a knob's value (a number, a choice's name, true or false), or a setting's (a number, one of its options, true or false, a file in assets/, a track), into the code, as one undo step; more knobs in `with` are written with it as the same step (for an agent, never a drag's commit)",
        r#"{"session": 7, "revision": 42, "id": "tracks/bass.ts#3/@shape", "gesture": "set", "value": "square"}"#;
    GET "/api/graph/leases" => graph_leases; reads, agents,
        "The drags in progress: knobs, clips and the tempo",
        r#"{}"#;
    POST "/api/graph/lease/begin" => lease_begin; writes, musician,
        "Begin a knob drag",
        r#"{"session": 7, "revision": 42, "id": "tracks/keys.ts#1/@brightness", "client": "w1", "gesture": "drag-1"}"#;
    POST "/api/graph/lease/update" => lease_update; reads, musician,
        "Hear a knob drag's newest value",
        r#"{"lease": 3, "seq": 12, "value": 0.6}"#;
    POST "/api/graph/lease/keepalive" => lease_keepalive; reads, musician,
        "Keep a knob drag going while the knob is held still",
        r#"{"lease": 3}"#;
    POST "/api/graph/lease/commit" => lease_commit; writes, musician,
        "Save a knob drag's value, as one undo step",
        r#"{"lease": 3, "value": 0.6}"#;
    POST "/api/graph/lease/cancel" => lease_cancel; reads, musician,
        "End a knob drag without saving",
        r#"{"lease": 3}"#;
    POST "/api/graph/connect" => graph_connect; writes, agents,
        "Connect a named signal to a knob's input: a cable, written in code",
        r#"{"session": 7, "revision": 42, "from": "kick.env", "to": "song.ts#5/0"}"#;
    POST "/api/graph/disconnect" => graph_disconnect; writes, agents,
        "Take a cable out: the input's default is written in its place",
        r#"{"session": 7, "revision": 42, "id": "song.ts#5/0"}"#;
    POST "/api/graph/effect/add" => graph_add_effect; writes, agents,
        "Put an effect after a call's sound, by its component id as the state's effects list it",
        r#"{"session": 7, "revision": 42, "call": "tracks/keys.ts#1", "component": "@m2/effects:delay"}"#;
    POST "/api/graph/effect/remove" => graph_remove_effect; writes, agents,
        "Take an effect out, keeping what it was applied to",
        r#"{"session": 7, "revision": 42, "call": "tracks/keys.ts#1"}"#;
    POST "/api/graph/lane" => graph_lane; writes, agents,
        "Write every clip of a notes lane (pitches as numbers or names, notes from their clip's start)",
        r#"{"session": 7, "revision": 42, "module": "keys", "lane": "chords", "clips": [{"id": "k3f9q2zt", "start": 0, "length": 4, "notes": [{"pitch": "C4", "start": 0, "length": 1}]}]}"#;
    POST "/api/graph/relink" => graph_relink; writes, agents,
        "Move data no lane or plugin reads to where it is read now: a lane file, a track's folder or a plugin state",
        r#"{"session": 7, "revision": 42, "from": "data/kick/hits.json", "to": "data/kicks/hits.json"}"#;
    POST "/api/graph/data/delete" => graph_delete_data; writes, agents,
        "Delete data no lane or plugin reads",
        r#"{"session": 7, "revision": 42, "file": "data/kick/old.json"}"#;
    POST "/api/graph/fix" => graph_fix up to crate::TX_BODY; writes, agents,
        "Apply a fix a problem offers, as the state's problems list it, whole",
        r#"{"session": 7, "fix": {"code": "delete-data", "title": "Delete it", "likely": false, "tx": {"id": "fix-delete-old", "base": {"data/kick/old.json": "sha256:9b74c9897bac770ffc029102a200c5de7d2a5d3e5d4c0f2f8b1c4e3a7d6f5e4c"}, "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}}}"#;
    POST "/api/graph/label" => graph_label; writes, agents,
        "Rename a track's label (its key stays)",
        r#"{"session": 7, "module": "keys", "label": "Piano"}"#;
    POST "/api/graph/automate" => graph_automate; writes, agents,
        "Automate a knob: a new, empty lane plays its value",
        r#"{"session": 7, "revision": 42, "id": "tracks/keys.ts#1/@brightness"}"#;
    POST "/api/graph/track/add" => track_add; writes, agents,
        "Add a track playing an instrument, by its component id as the state's instruments list it",
        r#"{"session": 7, "component": "@m2/synths:synth-pluck", "label": "Pluck", "after": "keys"}"#;
    POST "/api/graph/track/duplicate" => track_duplicate; writes, agents,
        "Copy a track: its code, its clips with new ids, its plugin states and its channel",
        r#"{"session": 7, "module": "keys"}"#;
    POST "/api/graph/track/delete" => track_delete; writes, agents,
        "Delete a track (with cascade, out of the effects that read it too)",
        r#"{"session": 7, "module": "keys", "cascade": false}"#;
    POST "/api/graph/mix/insert" => mix_insert; writes, agents,
        "Write a mixer channel's first pan into its entry",
        r#"{"session": 7, "channel": "keys", "control": "pan", "value": -0.2}"#;
    POST "/api/graph/layout" => graph_layout; writes, agents,
        "Reorder the tracks, or recolour one, in layout.json",
        r#"{"session": 7, "colour": {"module": "keys", "colour": "teal"}}"#;
    POST "/api/graph/tempo" => graph_tempo; writes, agents,
        "Set the tempo at a beat, or add or remove a tempo change there",
        r#"{"session": 7, "at": 0, "bpm": 96}"#;
    POST "/api/graph/meter" => graph_meter; writes, agents,
        "Set the time signature at a beat, or add or remove a change there",
        r#"{"session": 7, "at": 0, "beats": 7, "unit": 8}"#;
    POST "/api/graph/title" => graph_title; writes, agents,
        "Name the song: its name in music2.toml",
        r#"{"session": 7, "name": "Night drive"}"#;
    POST "/api/graph/panic" => graph_panic; reads, musician,
        "Stop all sound at once",
        r#"{}"#;
    POST "/api/graph/lock/retry" => graph_lock_retry; reads, musician,
        "Try the song's lock again, in a window open read-only",
        r#"{"session": 7}"#;
    POST "/api/tx/check" => tx_check up to crate::TX_BODY; reads, agents,
        "Check a Tx: its delta, the delta's hash and the diagnostics; nothing is written",
        r#"{"session": 7, "tx": {"id": "tx-delete-old", "base": {"data/kick/old.json": "sha256:9b74c9897bac770ffc029102a200c5de7d2a5d3e5d4c0f2f8b1c4e3a7d6f5e4c"}, "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}}"#;
    POST "/api/tx/commit" => tx_commit up to crate::TX_BODY; writes, agents,
        "Commit a Tx, as one undo step: m2 sends its writes here",
        r#"{"session": 7, "tx": {"id": "tx-delete-old", "base": {"data/kick/old.json": "sha256:9b74c9897bac770ffc029102a200c5de7d2a5d3e5d4c0f2f8b1c4e3a7d6f5e4c"}, "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}}"#;
    POST "/api/graph/undo" => graph_undo; writes, agents,
        "Take back the newest change (an agent's only when it is its own)",
        r#"{"session": 7}"#;
    POST "/api/graph/undo/skip" => graph_undo_skip; writes, musician,
        "Skip the newest step, which undo can't take back",
        r#"{"session": 7, "step": 57}"#;
    POST "/api/graph/redo" => graph_redo; writes, musician,
        "Put back the change undone last",
        r#"{"session": 7, "step": 57}"#;
    POST "/api/graph/transport" => graph_transport; reads, musician,
        "Play, stop, toggle or restart",
        r#"{"action": "play"}"#;
    POST "/api/graph/seek" => graph_seek; reads, musician,
        "Move the playhead to a beat",
        r#"{"beat": 32}"#;
    POST "/api/graph/loop" => graph_loop; reads, musician,
        "Set the loop's beats, or turn it on or off",
        r#"{"start": 32, "end": 64, "on": true}"#;
    GET "/api/graph/files" => graph_files; reads, agents,
        "The song's code files, as the code editor lists them",
        r#"{}"#;
    PUT "/api/graph/file" => graph_put_file up to crate::TX_BODY; writes, agents,
        "Save a code file over the text it had, as the code editor does (an agent's lands only if the song then builds)",
        r#"{"session": 7, "path": "tracks/keys.ts", "base": "the text it had", "text": "the new text"}"#;
    GET "/api/graph/types" => graph_types; reads, agents,
        "The type declarations the code editor checks with",
        r#"{}"#;
    POST "/api/graph/unsaved" => graph_unsaved; reads, musician,
        "Say which files have changes not yet saved in the code editor",
        r#"{"session": 7, "files": ["tracks/keys.ts"]}"#;
    POST "/api/graph/draft" => graph_draft up to crate::TX_BODY; reads, musician,
        "Keep a window's text of a code file not yet saved, so a crash doesn't lose it (text null takes it out)",
        r#"{"session": 7, "client": "the window's id", "file": "tracks/keys.ts", "base": "the text it had", "text": "the text typed so far"}"#;
    POST "/api/graph/draft/left" => graph_draft_left; reads, musician,
        "Say that unsaved code the song opened with was restored or discarded, so it isn't offered again",
        r#"{"session": 7, "id": "5d41402abc4b2a76-9e107d9d372bb682"}"#;
    POST "/api/graph/export" => graph_export; reads, musician,
        "Export the song's audio, which may pause playback (agents render with m2 render)",
        r#"{"path": "/Users/you/Music/Night drive.wav"}"#;
    GET "/api/project/templates" => project_templates; reads, agents,
        "The templates and examples a new song can start from, and the songs folder",
        r#"{}"#;
    POST "/api/project/new" => project_new; reads, musician,
        "Make a new song from a template or an example, and open it",
        r#"{"name": "Night drive", "template": "starter", "dir": "/Users/you/Music/music2"}"#;
}

/// The app's whole router: the route table's routes behind its gate (AGENT-8) and `auth`, and the
/// page for every other path.
pub fn app(state: Arc<AppState>, web_dir: &Path) -> Router {
    served(state, crate::api::router(), web_dir)
}

/// `routes` as the app serves them, behind `auth` and the table's gate: every `/api` path goes to
/// them, and the gate lets through only what the table has (AGENT-8); every other path is the
/// page's files. So no route outside the table answers, whatever else `routes` holds.
pub(crate) fn served(state: Arc<AppState>, routes: Router<Arc<AppState>>, web_dir: &Path) -> Router {
    let spa = ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")));
    Router::new()
        .route("/api/{*rest}", axum::routing::any_service(routes.with_state(state.clone())))
        .fallback_service(spa)
        .layer(axum::middleware::from_fn_with_state(state.clone(), crate::api::gate))
        .layer(axum::middleware::from_fn_with_state(state, auth))
}

/// Reject `/api` calls without one of the two keys (AGENT-10), and serve each as its caller: the
/// page's key is the musician's; the tools key an agent's, named by its `x-m2-as` header (else
/// `agent`), saying why with `x-m2-why`, both percent-encoded UTF-8. The key decides, never what a
/// request says about itself; the table's gate then refuses the agent the musician's routes
/// (AGENT-11).
async fn auth(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if !req.uri().path().starts_with("/api/") {
        return next.run(req).await;
    }
    let header = req.headers().get("x-m2-token").and_then(|v| v.to_str().ok());
    let query: Vec<&str> = req.uri().query().map_or(vec![], |q| q.split('&').filter_map(|kv| kv.strip_prefix("token=")).collect());
    let carries = |key: &str| header.is_some_and(|h| same_key(h, key)) || query.iter().any(|q| same_key(q, key));
    let caller = if carries(&s.page_token) {
        crate::caller::Caller::Page
    } else if carries(&s.token) {
        let said = |name: &str| req.headers().get(name).and_then(|v| v.to_str().ok()).map(crate::caller::decoded);
        crate::caller::Caller::agent(said("x-m2-as").as_deref(), said("x-m2-why").as_deref())
    } else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
    };
    crate::caller::serving(caller, next.run(req)).await
}

/// Whether `given` is the key `key` (AGENT-10), compared in constant time: every byte is looked at
/// whatever the first difference, so how long a refusal takes says nothing of the key. An empty key
/// is no key.
pub(crate) fn same_key(given: &str, key: &str) -> bool {
    let (a, b) = (given.as_bytes(), key.as_bytes());
    if b.is_empty() || a.len() != b.len() {
        return false;
    }
    std::hint::black_box(a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y))) == 0
}

// ---------------------------------------------------------------- events

#[derive(Deserialize)]
struct WsQuery {
    /// The window's id (one per page load), which its knob drags carry: when this socket closes,
    /// the window's gestures end (C6). Only a page's socket names a window.
    #[serde(default)]
    client: Option<String>,
    /// An agent's socket listens for meters only when it asks (`meters=1`, `m2 watch --meters`);
    /// a window's always does (TAP-4).
    #[serde(default)]
    meters: Option<String>,
}

async fn ws(State(s): S, Query(q): Query<WsQuery>, upgrade: WebSocketUpgrade) -> Response {
    // The page's socket, or an agent's (`m2 watch`, AGENT-6). An agent's names no window, whatever
    // its query says: its closing ends nothing the musician holds (AGENT-3).
    let page = !crate::caller::now().is_agent();
    let client = q.client.filter(|_| page);
    let meters = page || q.meters.is_some_and(|m| m == "1" || m == "true");
    upgrade.on_upgrade(move |socket| async move {
        ws_loop(s.clone(), socket, client.clone(), page, meters).await;
        if !page {
            return;
        }
        if let Some(gone) = client.clone() {
            // Off the async threads: putting values back may wait for room in the player's queue.
            let state = s.clone();
            let _ = crate::caller::blocking(move || state.graph.client_gone(&gone)).await;
            s.graph_changed();
        }
        // A page went away: the Compare pill closes with the window that opened it, or with the
        // last page (D2).
        let (state, others) = (s.clone(), s.pages() > 0);
        let _ = crate::caller::blocking(move || state.graph.page_gone(client.as_deref(), others)).await;
    })
}

/// Forward every event (toasts, `graph`) and, when it listens for them (`listens`), the latest
/// meters to one client until it goes away, with its window's own scopes after them (TAP-4,
/// TAP-8); and take what a page's socket sends: audition, whose answers go to this socket only, and
/// the MIDI keyboard's target (`routes_8b::ws_text`). What an agent's socket sends is ignored: those
/// are the musician's (AGENT-11). A socket that falls behind is told how many events it missed
/// (AGENT-6): `{"t": "lagged", "missed": n}`, then hears on from the oldest still kept.
async fn ws_loop(state: Arc<AppState>, mut socket: WebSocket, client: Option<String>, page: bool, listens: bool) {
    let (unheard, mut events, _page) = state.subscribe(page);
    let mut meters = listens.then(|| state.meters.subscribe());
    for text in unheard {
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            Ok(()) = async { meters.as_mut().expect("listens").changed().await }, if meters.is_some() => {
                let Some(m) = meters.as_mut() else { continue };
                let sent = m.borrow_and_update().clone();
                if socket.send(Message::Text(sent.all.clone().into())).await.is_err() {
                    break;
                }
                let mine = client.as_ref().and_then(|c| sent.scopes.iter().find(|(w, _)| w == c));
                if let Some((_, text)) = mine
                    && socket.send(Message::Text(text.clone().into())).await.is_err()
                {
                    break;
                }
            }
            ev = events.recv() => {
                let text = match ev {
                    Ok(text) => text,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        json!({"t": "lagged", "missed": missed, "session": state.graph.session()}).to_string()
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Text(text))) if page => {
                        if let Some(reply) = crate::routes_8b::ws_text(&state, client.as_deref(), &text).await
                            && socket.send(Message::Text(reply.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------- new songs

/// The templates and examples a new song can start from, and the songs folder.
async fn project_templates() -> ApiResult {
    Ok(Json(blocking(|| Ok(crate::project::catalog())).await?))
}

/// Make a new song from a template or an example, and open it.
async fn project_new(State(s): S, Json(r): Json<crate::project::NewSong>) -> ApiResult {
    let state = s.clone();
    let v = blocking(move || {
        let v = crate::project::new_song(&state.graph, &r)?;
        state.opened();
        Ok(v)
    })
    .await?;
    s.graph_changed();
    Ok(Json(v))
}

// ---------------------------------------------------------------- the open project

/// Off the async threads: it waits for an edit or a seek in progress, which holds the project.
async fn graph_get(State(s): S) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.state().ok_or_else(|| anyhow::anyhow!("no project is open"))).await?))
}

#[derive(Deserialize)]
struct GraphOpenReq {
    path: PathBuf,
}

async fn graph_open(State(s): S, Json(r): Json<GraphOpenReq>) -> ApiResult {
    let state = s.clone();
    let v = blocking(move || state.open_song(&r.path)).await?;
    s.graph_changed();
    Ok(Json(v))
}

async fn graph_close(State(s): S) -> ApiResult {
    let state = s.clone();
    blocking(move || {
        state.graph.close();
        state.closed();
        Ok(())
    })
    .await?;
    s.graph_changed();
    Ok(Json(json!({})))
}

async fn graph_status(State(s): S) -> Json<Value> {
    let mut v = json!(s.graph.status());
    // Which song it is of (AGENT-6): `m2 watch` ends when another opens.
    v["session"] = json!(s.graph.session());
    s.graph.activity().put(&mut v);
    Json(v)
}

#[derive(Deserialize)]
struct PreviewReq {
    session: u64,
    revision: u64,
    id: String,
    gesture: String,
    value: f64,
    /// The window it comes from (its WebSocket's `client`): optional, so older pages still work.
    #[serde(default)]
    client: Option<String>,
}

/// A knob drag's update: the gesture's lease, begun at its first preview (WRITE-22). Never waits
/// for the session.
async fn graph_preview(State(s): S, Json(r): Json<PreviewReq>) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.preview_for(r.client.as_deref(), r.session, r.revision, &r.id, &r.gesture, r.value)).await?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct CancelPreviewReq {
    session: u64,
    id: String,
    gesture: String,
}

async fn graph_cancel_preview(State(s): S, Json(r): Json<CancelPreviewReq>) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.cancel_preview(r.session, &r.id, &r.gesture)).await?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct SetReq {
    session: u64,
    revision: u64,
    id: String,
    gesture: String,
    /// A knob's value (a number, a choice's name, a switch's true or false), or a setting's: a
    /// number, a string or a boolean (CARD-4, CARD-21).
    value: Value,
    #[serde(default)]
    expect: Option<Value>,
    /// More knobs' values the same gesture moved, written with it as one undo step (CARD-22).
    #[serde(default)]
    with: Vec<WithValue>,
}

#[derive(Deserialize)]
struct WithValue {
    id: String,
    value: Value,
}

/// Commit a value: the gesture's lease when it has one, else a plain value edit; a setting's value
/// is one Tx on its call (CARD-4). While the draft doesn't compile a gesture's commit is pending:
/// `{"pending": true, "errors": …}` (C5).
async fn graph_set(State(s): S, Json(r): Json<SetReq>) -> ApiResult {
    let state = s.clone();
    let result = match r.with.is_empty() {
        true => blocking(move || state.graph.set_any(r.session, r.revision, &r.id, &r.gesture, r.value, r.expect)).await,
        false if r.expect.is_some() => Err(err("a gesture of several values takes no `expect`")),
        false => {
            let values = std::iter::once((r.id, r.value)).chain(r.with.into_iter().map(|w| (w.id, w.value))).collect();
            blocking(move || state.graph.set_all(r.session, r.revision, &r.gesture, values)).await
        }
    };
    s.graph_changed();
    Ok(Json(result?))
}

// ---------------------------------------------------------------- gesture leases (WRITE-22)

/// The knob drags in progress, as the news lists them: an agent can cancel one it finds here. And
/// the drags on clips (WRITE-22a) and the tempo's (RENDER-27a), with `kind: "lane"` or `"tempo"`.
async fn graph_leases(State(s): S) -> Json<Value> {
    let mut all = json!(s.graph.leases());
    if let Value::Array(a) = &mut all {
        a.extend(s.graph.lane_leases().iter().map(|l| json!(l)));
        a.extend(s.graph.tempo_leases().iter().map(|l| json!(l)));
    }
    Json(all)
}

#[derive(Deserialize)]
struct LeaseBeginReq {
    session: u64,
    revision: u64,
    id: String,
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    gesture: Option<String>,
    /// How long the lease lives without an update or a keepalive (10 s by default, 60 s at most).
    #[serde(default)]
    ttl_ms: Option<u64>,
}

/// Begin a drag on a control, as revision `revision` named it.
async fn lease_begin(State(s): S, Json(r): Json<LeaseBeginReq>) -> ApiResult {
    let state = s.clone();
    let owner = crate::graph::Owner { client: r.client, gesture: r.gesture, ttl: r.ttl_ms.map(std::time::Duration::from_millis) };
    let begun = blocking(move || state.graph.begin(r.session, r.revision, &r.id, owner)).await?;
    s.graph_changed();
    Ok(Json(json!(begun)))
}

#[derive(Deserialize)]
struct LeaseUpdateReq {
    lease: u64,
    /// Updates older than one taken are dropped; without, it follows the last.
    #[serde(default)]
    seq: Option<u64>,
    value: f64,
}

/// Play a value for a lease, straight into the engine. Answers with the engine message's
/// sequence number (null when nothing plays it yet, or the update was older than one taken).
async fn lease_update(State(s): S, Json(r): Json<LeaseUpdateReq>) -> ApiResult {
    let state = s.clone();
    let seq = blocking(move || state.graph.update(r.lease, r.seq, r.value)).await?;
    Ok(Json(json!({"seq": seq})))
}

#[derive(Deserialize)]
struct LeaseReq {
    lease: u64,
}

async fn lease_keepalive(State(s): S, Json(r): Json<LeaseReq>) -> ApiResult {
    s.graph.keepalive(r.lease)?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct LeaseCommitReq {
    lease: u64,
    value: f64,
    #[serde(default)]
    expect: Option<f64>,
}

/// Commit a lease's value to code (one undo step). While the draft doesn't compile it is
/// pending: `{"pending": true, "errors": …}`, written once the code compiles (C5).
async fn lease_commit(State(s): S, Json(r): Json<LeaseCommitReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.commit(r.lease, r.value, r.expect)).await;
    s.graph_changed();
    Ok(Json(match result? {
        crate::graph::Committed::Written(v) => v,
        crate::graph::Committed::Pending { errors } => json!({"pending": true, "errors": errors}),
    }))
}

/// End a lease without a commit: the code's value plays again.
async fn lease_cancel(State(s): S, Json(r): Json<LeaseReq>) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.cancel(r.lease)).await?;
    s.graph_changed();
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct ConnectReq {
    session: u64,
    revision: u64,
    from: String,
    to: String,
}

async fn graph_connect(State(s): S, Json(r): Json<ConnectReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.connect(r.session, r.revision, &r.from, &r.to)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct EffectReq {
    session: u64,
    revision: u64,
    call: String,
    /// The effect's ComponentId (`@m2/effects:delay`), as the graph state's `effects` list it; or an
    /// installed plugin's id, `clap:<id>`, as `GET /api/plugins` lists it (gaps #32).
    #[serde(default)]
    component: String,
}

async fn graph_add_effect(State(s): S, Json(r): Json<EffectReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.add_effect(r.session, r.revision, &r.call, &r.component)).await;
    s.graph_changed();
    Ok(Json(result?))
}

async fn graph_remove_effect(State(s): S, Json(r): Json<EffectReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.remove_effect(r.session, r.revision, &r.call)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct LaneReq {
    session: u64,
    revision: u64,
    module: String,
    lane: String,
    /// Every clip the lane is to have: pitches as numbers or names, notes relative to their clip.
    #[serde(deserialize_with = "crate::routes_8b::shown_clips")]
    clips: Vec<m2_song::NoteClip>,
}

/// Rewrite a notes lane's clips (the piano roll; clips made, moved or deleted in the timeline).
async fn graph_lane(State(s): S, Json(r): Json<LaneReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.write_notes(r.session, r.revision, &r.module, &r.lane, &r.clips)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct RelinkReq {
    session: u64,
    revision: u64,
    /// The data no lane or plugin reads, relative to the project: a lane file
    /// (`data/kick/hits.json`), a track's folder (`data/kick`) or a plugin state.
    from: String,
    /// Where it goes, as a project path (`data/kicks/hits.json`, `data/kicks`)…
    #[serde(default)]
    to: Option<String>,
    /// …or, for a lane file, the lane by its track's key and its own.
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    lane: Option<String>,
}

/// Move data no lane or plugin reads to where it is read now: a problem's relink fix.
async fn graph_relink(State(s): S, Json(r): Json<RelinkReq>) -> ApiResult {
    let to = match (r.to, r.module, r.lane) {
        (Some(to), ..) => to,
        (None, Some(module), Some(lane)) => {
            let addr = m2_song::LaneAddr::new(&module, &lane).map_err(err)?;
            m2_song::DataPath::lane(&addr).to_string()
        }
        _ => return Err(err("say where it goes: `to`, or a lane's `module` and `lane`")),
    };
    let state = s.clone();
    let result = blocking(move || state.graph.relink(r.session, r.revision, &r.from, &to)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct FixReq {
    session: u64,
    /// An agent's name and words for its step (AGENT-10): only with the tools key.
    #[serde(default, rename = "as")]
    by: Option<String>,
    #[serde(default)]
    why: Option<String>,
    /// The fix as a problem carried it, `{code, title, likely, tx}`: its Tx is what is committed
    /// (DIAG-1).
    fix: Value,
}

/// Apply one of the fixes a problem offers: commit its Tx, which its bases guard, so a fix
/// offered on files that changed since is a conflict (DIAG-1). Undo takes it back.
async fn graph_fix(State(s): S, Json(r): Json<FixReq>) -> ApiResult {
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), r.why.as_deref());
    let result = blocking(move || crate::caller::as_caller(caller, || state.graph.fix(r.session, &r.fix))).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct LabelReq {
    session: u64,
    /// The track by its key; the label is display text only (ID-10).
    module: String,
    label: String,
}

/// Rename a track's label (double-click its name in the timeline or on the canvas).
async fn graph_label(State(s): S, Json(r): Json<LabelReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.label(r.session, &r.module, &r.label)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct AutomateReq {
    session: u64,
    revision: u64,
    /// The knob's control id, at `revision`.
    id: String,
}

/// Automate a knob (its menu on the canvas): a new, empty control lane plays its value.
async fn graph_automate(State(s): S, Json(r): Json<AutomateReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.automate(r.session, r.revision, &r.id)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct TrackAddReq {
    /// The open project. A `revision` sent with it is ignored: the builder plans on the accepted one.
    session: u64,
    /// The instrument, by ComponentId (`@m2/synths:synth-pluck`), as the graph state's `instruments` list it;
    /// or an installed instrument plugin, `clap:<id>`, as `GET /api/plugins` lists it (gaps #32).
    component: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    key: Option<String>,
    /// The track it goes below in the layout (the selected one); else last.
    #[serde(default)]
    after: Option<String>,
}

/// Add a track (+ Track, ⌘T): its file, its import and channel in the song, its lane, its layout.
async fn track_add(State(s): S, Json(r): Json<TrackAddReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.add_track(r.session, &r.component, r.label, r.key, r.after)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct TrackReq {
    session: u64,
    /// The track, by its key.
    module: String,
    /// Delete only: take out the effects' reads of it too.
    #[serde(default)]
    cascade: bool,
}

/// Duplicate a track (⌘D on its header): code, clips with new ids, plugin states and channel.
async fn track_duplicate(State(s): S, Json(r): Json<TrackReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.duplicate_track(r.session, &r.module)).await;
    s.graph_changed();
    Ok(Json(result?))
}

/// Delete a track (⌘⌫ on its header). 409 with the dependents when other code reads it.
async fn track_delete(State(s): S, Json(r): Json<TrackReq>) -> Response {
    let state = s.clone();
    let result = crate::caller::blocking(move || state.graph.delete_track(r.session, &r.module, r.cascade)).await;
    s.graph_changed();
    tx_answer(result)
}

#[derive(Deserialize)]
struct MixInsertReq {
    /// The open project. A `revision` sent with it is ignored: the channel is named by its key,
    /// and the builder plans on the accepted revision.
    session: u64,
    /// The mixer channel, by its key.
    channel: String,
    /// The control the channel leaves out: `pan`.
    control: String,
    value: f64,
}

/// A mixer channel's first pan (the Mix view, as the drag passes its threshold): `pan` written
/// into the channel's entry. The answer names the new knob (`control`), which the drag then leases.
async fn mix_insert(State(s): S, Json(r): Json<MixInsertReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.mix_insert(r.session, &r.channel, &r.control, r.value)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct ColourReq {
    module: String,
    colour: String,
}

#[derive(Deserialize)]
struct LayoutReq {
    session: u64,
    /// Every track, top to bottom.
    #[serde(default)]
    order: Option<Vec<String>>,
    /// One track's colour, by palette name.
    #[serde(default)]
    colour: Option<ColourReq>,
    /// Write the defaults over a `layout.json` that doesn't read.
    #[serde(default)]
    reset: bool,
}

/// Reorder or recolour tracks: a write of `layout.json` only, with no revision (PLANE-3).
async fn graph_layout(State(s): S, Json(r): Json<LayoutReq>) -> ApiResult {
    let state = s.clone();
    let colour = r.colour.map(|c| (c.module, c.colour));
    let result = blocking(move || state.graph.layout(r.session, r.order, colour, r.reset)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct TempoReq {
    session: u64,
    /// The beat: whose tempo is set (the playhead's), where a change goes, or which is removed.
    #[serde(default)]
    at: f64,
    #[serde(default)]
    bpm: Option<f64>,
    /// A new tempo change at `at`, rather than the one in effect there.
    #[serde(default)]
    add: bool,
    #[serde(default)]
    remove: bool,
    /// With `remove`, the time signature change at `at` goes too, in the same step (a ruler mark's
    /// Remove these changes).
    #[serde(default)]
    meter: bool,
}

/// The song bar's tempo field and the ruler's tempo marks (design-8b §4): one undo step each.
async fn graph_tempo(State(s): S, Json(r): Json<TempoReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.tempo(r.session, r.at, r.bpm, r.add, r.remove, r.meter)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct MeterReq {
    session: u64,
    #[serde(default)]
    at: f64,
    #[serde(default)]
    beats: Option<u32>,
    #[serde(default)]
    unit: Option<u32>,
    /// A new time signature change on the bar line at or before `at`.
    #[serde(default)]
    add: bool,
    #[serde(default)]
    remove: bool,
}

/// The song bar's time signature and the ruler's marks.
async fn graph_meter(State(s): S, Json(r): Json<MeterReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.meter(r.session, r.at, r.beats, r.unit, r.add, r.remove)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct TitleReq {
    session: u64,
    name: String,
}

/// The song's name, double-clicked in the top bar: its manifest's `[project] name`.
async fn graph_title(State(s): S, Json(r): Json<TitleReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.title(r.session, &r.name)).await;
    s.graph_changed();
    Ok(Json(result?))
}

/// Stop all sound (⌘.): not a write, so a read-only window has it too.
async fn graph_panic(State(s): S) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.panic()).await?;
    Ok(Json(json!({})))
}

/// Try the project's lock again (WRITE-17): the read-only banner's Retry.
async fn graph_lock_retry(State(s): S, Json(r): Json<SessionReq>) -> ApiResult {
    let state = s.clone();
    let v = blocking(move || state.graph.retry_lock(r.session)).await?;
    s.graph_changed();
    Ok(Json(v))
}

#[derive(Deserialize)]
struct TxReq {
    /// The open project, when the caller names it.
    #[serde(default)]
    session: Option<u64>,
    /// The Tx's JSON (WRITE-3), as `m2 tx` reads it.
    tx: Value,
    /// A forwarded Tx (AGENT-1): the hash of the delta its sender checked. It is committed only
    /// while its delta has that hash, under its own expectation.
    #[serde(default)]
    seen: Option<String>,
    /// The new files under `assets/` it names, taken in before it is committed (AGENT-1).
    #[serde(default)]
    assets: Vec<String>,
    /// An agent's name and why it made the change, for its step (AGENT-10): only with the tools
    /// key.
    #[serde(default, rename = "as")]
    by: Option<String>,
    #[serde(default)]
    why: Option<String>,
}

/// Check a Tx (WRITE-4, WRITE-11): its delta, the delta's hash and the diagnostics; nothing is
/// written. 409 for a conflict, 422 for a rejection.
async fn tx_check(State(s): S, Json(r): Json<TxReq>) -> Response {
    let state = s.clone();
    let result = crate::caller::blocking(move || state.graph.tx_check(r.session, &r.tx)).await;
    tx_answer(result)
}

/// Commit a Tx (WRITE-5): `Seen(hash)` commits only the delta its check showed; a forwarded one
/// (`seen` beside it, AGENT-1) only while its delta has that hash, its new assets taken in first.
/// The engine plays what it made, and every window hears of it. An agent's that the musician
/// holds is busy: 423 (AGENT-3).
async fn tx_commit(State(s): S, Json(r): Json<TxReq>) -> Response {
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), r.why.as_deref());
    let result = crate::caller::blocking(move || {
        crate::caller::as_caller(caller, || state.graph.tx_commit(r.session, &r.tx, r.seen.as_deref(), &r.assets))
    })
    .await;
    s.graph_changed();
    tx_answer(result)
}

fn tx_answer(result: Result<anyhow::Result<(u16, Value)>, tokio::task::JoinError>) -> Response {
    match result {
        Ok(Ok((status, v))) => (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(v)).into_response(),
        Ok(Err(e)) => ApiError::from(e).into_response(),
        Err(e) => err(e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct DeleteDataReq {
    session: u64,
    revision: u64,
    file: String,
}

/// Delete data no lane or plugin reads: a problem's other fix. Undo puts it back.
async fn graph_delete_data(State(s): S, Json(r): Json<DeleteDataReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.delete_data(r.session, r.revision, &r.file)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct SessionReq {
    session: u64,
}

/// Undo or redo `{session, step}`: the step its id names, only while it is the newest (for redo,
/// the newest undone), else 409 naming the newest, and nothing changes (AGENT-2). Without `step`,
/// the newest.
#[derive(Deserialize)]
struct StepReq {
    session: u64,
    step: Option<u64>,
    /// An agent's name (AGENT-4): its undo takes back only its own step. Only with the tools key.
    #[serde(default, rename = "as")]
    by: Option<String>,
}

async fn graph_undo(State(s): S, Json(r): Json<StepReq>) -> Response {
    stepped(s, r, false).await
}

async fn graph_redo(State(s): S, Json(r): Json<StepReq>) -> Response {
    stepped(s, r, true).await
}

async fn stepped(s: Arc<AppState>, r: StepReq, redo: bool) -> Response {
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), None);
    let result = crate::caller::blocking(move || {
        crate::caller::as_caller(caller, || match (r.step, redo) {
            (Some(step), false) => state.graph.undo_step(r.session, step),
            (Some(step), true) => state.graph.redo_step(r.session, step),
            (None, false) => state.graph.undo(r.session),
            (None, true) => state.graph.redo(r.session),
        })
    })
    .await;
    s.graph_changed();
    step_answer(result)
}

/// An undo's, a redo's or a skip's answer: 409 when its step isn't the newest (`newest`), or
/// when undo can't take it back and it can be skipped (`stuck`), with words either way (AGENT-2).
pub(crate) fn step_answer(result: Result<anyhow::Result<Value>, tokio::task::JoinError>) -> Response {
    use crate::graph::steps::{NotElsewhere, NotNewest, NotTheirs, Stuck};
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => {
            if let Some(n) = e.downcast_ref::<NotNewest>() {
                return (StatusCode::CONFLICT, Json(json!({"error": n.to_string(), "newest": n.newest}))).into_response();
            }
            if let Some(n) = e.downcast_ref::<NotTheirs>() {
                return (StatusCode::CONFLICT, Json(json!({"error": n.to_string(), "newest": n.newest}))).into_response();
            }
            if let Some(n) = e.downcast_ref::<NotElsewhere>() {
                return (StatusCode::CONFLICT, Json(json!({"error": n.to_string(), "newest": n.newest}))).into_response();
            }
            if let Some(n) = e.downcast_ref::<crate::graph::compare::Stale>() {
                return (StatusCode::CONFLICT, Json(json!({"error": n.to_string(), "comparing": n.comparing}))).into_response();
            }
            if let Some(s) = e.downcast_ref::<Stuck>() {
                return (StatusCode::CONFLICT, Json(json!({"error": s.to_string(), "stuck": s.stuck}))).into_response();
            }
            ApiError::from(e).into_response()
        }
        Err(e) => err(e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct SkipReq {
    session: u64,
    step: u64,
}

/// Skip the newest step, which undo can't take back (AGENT-2): nothing is written.
async fn graph_undo_skip(State(s): S, Json(r): Json<SkipReq>) -> Response {
    let state = s.clone();
    let result = crate::caller::blocking(move || state.graph.skip_step(r.session, r.step)).await;
    s.graph_changed();
    step_answer(result)
}

#[derive(Deserialize)]
struct DisconnectReq {
    session: u64,
    revision: u64,
    id: String,
}

async fn graph_disconnect(State(s): S, Json(r): Json<DisconnectReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.disconnect(r.session, r.revision, &r.id)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct GraphTransportReq {
    action: String,
}

/// A stop leaves the playhead where it stopped, and a play carries on from there (taste.md §5.1).
async fn graph_transport(State(s): S, Json(r): Json<GraphTransportReq>) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.transport(&r.action)).await?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct SeekReq {
    beat: f64,
}

/// Seeking doesn't change the view, so clients aren't told to refresh.
async fn graph_seek(State(s): S, Json(r): Json<SeekReq>) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.seek(r.beat)).await?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct LoopReq {
    /// The loop's range in beats: both or neither. Setting it turns looping on unless `on` says.
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
    /// Loop or not, over the range set (kept while looping is off).
    #[serde(default)]
    on: Option<bool>,
}

/// Set the loop; answers with the transport as it now is. Like a seek, it doesn't change the view.
async fn graph_loop(State(s): S, Json(r): Json<LoopReq>) -> ApiResult {
    let range = match (r.start, r.end) {
        (Some(start), Some(end)) => Some((start, end)),
        (None, None) => None,
        _ => return Err(err("a loop's range needs both `start` and `end`")),
    };
    let state = s.clone();
    let status = blocking(move || state.graph.set_loop(range, r.on)).await?;
    Ok(Json(json!(status)))
}

#[derive(Deserialize)]
struct ExportReq {
    /// The file the user picked; else a new one in the songs folder's Exports.
    #[serde(default)]
    path: Option<PathBuf>,
}

/// Start Export Audio. It answers at once with where the file goes; `export` events follow.
async fn graph_export(State(s): S, Json(r): Json<ExportReq>) -> ApiResult {
    let state = s.clone();
    let path = blocking(move || crate::export::start(&state, r.path)).await?;
    Ok(Json(json!({"path": path})))
}

// ---------------------------------------------------------------- the code editor

async fn graph_files(State(s): S) -> ApiResult {
    let state = s.clone();
    let mut files = blocking(move || state.graph.files()).await?;
    // The text left unsaved is the musician's: an agent reads the song's files, not it (WRITE-27).
    if crate::caller::now().is_agent()
        && let Some(o) = files.as_object_mut()
    {
        o.remove("left");
    }
    Ok(Json(files))
}

async fn graph_types(State(s): S) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.types()).await?))
}

#[derive(Deserialize)]
struct FileReq {
    /// The open project the editor read the file from, if it says.
    #[serde(default)]
    session: Option<u64>,
    path: String,
    /// What the file said when the editor read it.
    base: String,
    text: String,
    /// An agent's name and words for its step (AGENT-10): only with the tools key, whose save is
    /// written only when the song takes it in as written (AGENT-1).
    #[serde(default, rename = "as")]
    by: Option<String>,
    #[serde(default)]
    why: Option<String>,
}

#[derive(Deserialize)]
struct UnsavedReq {
    session: u64,
    files: Vec<String>,
}

#[derive(Deserialize)]
struct DraftReq {
    session: u64,
    /// The window whose draft it is: each window keeps its own.
    #[serde(default)]
    client: String,
    file: String,
    /// The file's text the editor was in step with.
    base: String,
    /// Null: there is nothing unsaved now.
    text: Option<String>,
}

/// The code editor keeps a file's unsaved text in `.music2/drafts/` (WRITE-27), never in the song.
async fn graph_draft(State(s): S, Json(r): Json<DraftReq>) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.set_draft(r.session, &r.client, &r.file, &r.base, r.text.as_deref())).await?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct DraftLeftReq {
    session: u64,
    id: String,
}

/// A draft the song opened with was restored or discarded (WRITE-27).
async fn graph_draft_left(State(s): S, Json(r): Json<DraftLeftReq>) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.answer_draft(r.session, &r.id)).await?;
    Ok(Json(json!({})))
}

/// The code editor says which files have edits not yet saved, so the app asks before
/// closing or replacing the project.
async fn graph_unsaved(State(s): S, Json(r): Json<UnsavedReq>) -> ApiResult {
    s.graph.set_unsaved(r.session, r.files)?;
    Ok(Json(json!({})))
}

/// Save a file from the code editor. One changed since the editor read it is refused with
/// 409 and what it says now, so the editor can offer to load that or keep its own.
async fn graph_put_file(State(s): S, Json(r): Json<FileReq>) -> Response {
    let state = s.clone();
    let caller = crate::caller::now().saying(r.by.as_deref(), r.why.as_deref());
    let result =
        crate::caller::blocking(move || crate::caller::as_caller(caller, || state.graph.write_file(r.session, &r.path, &r.base, &r.text)))
            .await;
    match result {
        Ok(Ok(v)) => {
            s.graph_changed();
            Json(v).into_response()
        }
        Ok(Err(e)) => {
            if let Some(c) = e.downcast_ref::<crate::files::Conflict>() {
                return (StatusCode::CONFLICT, Json(json!({"error": c.to_string(), "current": c.current}))).into_response();
            }
            // An agent's save the song wouldn't take in as written: 422 with the build's words, as a
            // Tx's rejection says them.
            if let Some(u) = e.downcast_ref::<crate::files::Unbuilt>() {
                let rejected = json!({"error": u.to_string(), "rejected": {"diagnostics": u.diagnostics}});
                return (StatusCode::UNPROCESSABLE_ENTITY, Json(rejected)).into_response();
            }
            ApiError::from(e).into_response()
        }
        Err(e) => err(e.to_string()).into_response(),
    }
}

#[cfg(test)]
#[path = "routes_views_tests.rs"]
mod views_tests;
