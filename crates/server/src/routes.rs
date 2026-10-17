//! HTTP + WebSocket API for the open song project. Thin: every edit goes through [`Graph`](crate::graph::Graph).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};

use crate::AppState;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

pub struct ApiError(String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(json!({"error": self.0}))).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(format!("{:#}", e.into()))
    }
}

fn err(msg: impl Into<String>) -> ApiError {
    ApiError(msg.into())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| err(e.to_string()))?.map_err(ApiError::from)
}

pub fn router(state: Arc<AppState>, web_dir: &Path) -> Router {
    let spa = ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")));
    Router::new()
        .route("/api/ws", get(ws))
        .route("/api/graph", get(graph_get))
        .route("/api/graph/open", post(graph_open))
        .route("/api/graph/close", post(graph_close))
        .route("/api/graph/status", get(graph_status))
        .route("/api/graph/preview", post(graph_preview))
        .route("/api/graph/preview/cancel", post(graph_cancel_preview))
        .route("/api/graph/set", post(graph_set))
        .route("/api/graph/leases", get(graph_leases))
        .route("/api/graph/lease/begin", post(lease_begin))
        .route("/api/graph/lease/update", post(lease_update))
        .route("/api/graph/lease/keepalive", post(lease_keepalive))
        .route("/api/graph/lease/commit", post(lease_commit))
        .route("/api/graph/lease/cancel", post(lease_cancel))
        .route("/api/graph/connect", post(graph_connect))
        .route("/api/graph/effect/add", post(graph_add_effect))
        .route("/api/graph/effect/remove", post(graph_remove_effect))
        .route("/api/graph/lane", post(graph_lane))
        .route("/api/graph/relink", post(graph_relink))
        .route("/api/graph/data/delete", post(graph_delete_data))
        .route("/api/graph/fix", post(graph_fix))
        .route("/api/graph/label", post(graph_label))
        .route("/api/graph/automate", post(graph_automate))
        .route("/api/graph/track/add", post(track_add))
        .route("/api/graph/track/duplicate", post(track_duplicate))
        .route("/api/graph/track/delete", post(track_delete))
        .route("/api/graph/mix/insert", post(mix_insert))
        .route("/api/graph/layout", post(graph_layout))
        .route("/api/graph/tempo", post(graph_tempo))
        .route("/api/graph/meter", post(graph_meter))
        .route("/api/graph/title", post(graph_title))
        .route("/api/graph/panic", post(graph_panic))
        .route("/api/graph/lock/retry", post(graph_lock_retry))
        .route("/api/tx/check", post(tx_check))
        .route("/api/tx/commit", post(tx_commit))
        .route("/api/graph/undo", post(graph_undo))
        .route("/api/graph/redo", post(graph_redo))
        .route("/api/graph/disconnect", post(graph_disconnect))
        .route("/api/graph/transport", post(graph_transport))
        .route("/api/graph/seek", post(graph_seek))
        .route("/api/graph/loop", post(graph_loop))
        .route("/api/graph/files", get(graph_files))
        .route("/api/graph/file", put(graph_put_file))
        .route("/api/graph/types", get(graph_types))
        .route("/api/graph/unsaved", post(graph_unsaved))
        .route("/api/graph/export", post(graph_export))
        .merge(crate::routes_8b::router(state.clone()))
        .merge(crate::routes_view::router())
        .route("/api/project/templates", get(project_templates))
        .route("/api/project/new", post(project_new))
        // An unknown API path is a clear error, not the UI's index.html.
        .route("/api/{*rest}", any(no_such_api))
        .fallback_service(spa)
        .layer(axum::middleware::from_fn_with_state(state.clone(), writes))
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth))
        .with_state(state)
}

/// Reject `/api` calls without the session token.
async fn auth(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let token = &s.token;
    if req.uri().path().starts_with("/api/") {
        let header = req.headers().get("x-m2-token").and_then(|v| v.to_str().ok());
        let query = req.uri().query().is_some_and(|q| q.split('&').any(|kv| kv.strip_prefix("token=") == Some(token)));
        if header != Some(token.as_str()) && !query {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
        }
    }
    next.run(req).await
}

/// The routes that write the open project, or begin a gesture whose commit would.
const WRITES: [&str; 26] = [
    "/api/graph/preview",
    "/api/graph/set",
    "/api/graph/lease/begin",
    "/api/graph/lease/commit",
    "/api/graph/connect",
    "/api/graph/disconnect",
    "/api/graph/effect/add",
    "/api/graph/effect/remove",
    "/api/graph/lane",
    "/api/graph/relink",
    "/api/graph/data/delete",
    "/api/graph/fix",
    "/api/graph/label",
    "/api/graph/automate",
    "/api/graph/track/add",
    "/api/graph/track/duplicate",
    "/api/graph/track/delete",
    "/api/graph/mix/insert",
    "/api/graph/layout",
    "/api/graph/tempo",
    "/api/graph/meter",
    "/api/graph/title",
    "/api/graph/undo",
    "/api/graph/redo",
    "/api/graph/file",
    "/api/tx/commit",
];

/// A write to a project open read-only answers 423 with WRITE-18's message, before anything is
/// planned (WRITE-17). Checks, reads, the transport and exports go on.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path();
    if WRITES.contains(&path)
        && let Some(why) = s.graph.read_only()
    {
        return (StatusCode::LOCKED, Json(json!({"error": why}))).into_response();
    }
    next.run(req).await
}

async fn no_such_api() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": "no such API"}))).into_response()
}

// ---------------------------------------------------------------- events

#[derive(Deserialize)]
struct WsQuery {
    /// The window's id (one per page load), which its knob drags carry: when this socket closes,
    /// the window's gestures end (C6).
    #[serde(default)]
    client: Option<String>,
}

async fn ws(State(s): S, Query(q): Query<WsQuery>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| async move {
        ws_loop(s.clone(), socket, q.client.clone()).await;
        if let Some(client) = q.client {
            // Off the async threads: putting values back may wait for room in the player's queue.
            let state = s.clone();
            let _ = tokio::task::spawn_blocking(move || state.graph.client_gone(&client)).await;
            s.graph_changed();
        }
    })
}

/// Forward every event (toasts, `graph`) and the latest meters to one client until it goes away,
/// and take what it sends: audition, whose answers go to this socket only (`routes_8b::ws_text`).
async fn ws_loop(state: Arc<AppState>, mut socket: WebSocket, client: Option<String>) {
    let (unheard, mut events) = state.subscribe();
    let mut meters = state.meters.subscribe();
    for text in unheard {
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            Ok(()) = meters.changed() => {
                let text = meters.borrow_and_update().clone();
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            ev = events.recv() => {
                if let Ok(text) = ev
                    && socket.send(Message::Text(text.into())).await.is_err()
                {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Text(text))) => {
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
    let v = blocking(move || crate::project::new_song(&state.graph, &r)).await?;
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
    let v = blocking(move || state.graph.open(&r.path)).await?;
    s.graph_changed();
    Ok(Json(v))
}

async fn graph_close(State(s): S) -> ApiResult {
    let state = s.clone();
    blocking(move || {
        state.graph.close();
        Ok(())
    })
    .await?;
    s.graph_changed();
    Ok(Json(json!({})))
}

async fn graph_status(State(s): S) -> Json<Value> {
    let mut v = json!(s.graph.status());
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
    value: f64,
    #[serde(default)]
    expect: Option<f64>,
}

/// Commit a value: the gesture's lease when it has one, else a plain value edit. While the draft
/// doesn't compile a gesture's commit is pending: `{"pending": true, "errors": …}` (C5).
async fn graph_set(State(s): S, Json(r): Json<SetReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.set(r.session, r.revision, &r.id, &r.gesture, r.value, r.expect)).await;
    s.graph_changed();
    Ok(Json(result?))
}

// ---------------------------------------------------------------- gesture leases (WRITE-22)

/// The knob drags in progress, as the news lists them: an agent can cancel one it finds here.
async fn graph_leases(State(s): S) -> Json<Value> {
    Json(json!(s.graph.leases()))
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
    /// The effect's ComponentId (`std:delay`), as the graph state's `effects` list it.
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
    /// The fix as a problem carried it, `{code, title, likely, tx}`: its Tx is what is committed
    /// (DIAG-1).
    fix: Value,
}

/// Apply one of the fixes a problem offers: commit its Tx, which its bases guard, so a fix
/// offered on files that changed since is a conflict (DIAG-1). Undo takes it back.
async fn graph_fix(State(s): S, Json(r): Json<FixReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.fix(r.session, &r.fix)).await;
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
    /// The instrument, by ComponentId (`std:synth-pluck`), as the graph state's `instruments` list it.
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
    let result = tokio::task::spawn_blocking(move || state.graph.delete_track(r.session, &r.module, r.cascade)).await;
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
}

/// Check a Tx (WRITE-4, WRITE-11): its delta, the delta's hash and the diagnostics; nothing is
/// written. 409 for a conflict, 422 for a rejection.
async fn tx_check(State(s): S, Json(r): Json<TxReq>) -> Response {
    let state = s.clone();
    let result = tokio::task::spawn_blocking(move || state.graph.tx_check(r.session, &r.tx)).await;
    tx_answer(result)
}

/// Commit a Tx (WRITE-5): `Seen(hash)` commits only the delta its check showed. The engine plays
/// what it made, and every window hears of it.
async fn tx_commit(State(s): S, Json(r): Json<TxReq>) -> Response {
    let state = s.clone();
    let result = tokio::task::spawn_blocking(move || state.graph.tx_commit(r.session, &r.tx)).await;
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

async fn graph_undo(State(s): S, Json(r): Json<SessionReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.undo(r.session)).await;
    s.graph_changed();
    Ok(Json(result?))
}

async fn graph_redo(State(s): S, Json(r): Json<SessionReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.redo(r.session)).await;
    s.graph_changed();
    Ok(Json(result?))
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
    /// With `stop`: the beat to go back to, where play started (taste.md §5.1). Sent with the stop,
    /// so nothing in between sees it stopped where it was.
    #[serde(default)]
    to: Option<f64>,
}

/// The answer says where a stop with `to` stopped (`at`), for ⇧Space to continue from.
async fn graph_transport(State(s): S, Json(r): Json<GraphTransportReq>) -> ApiResult {
    let state = s.clone();
    let at = blocking(move || match (r.action.as_str(), r.to) {
        ("stop", Some(to)) => state.graph.stop_to(to).map(Some),
        _ => state.graph.transport(&r.action).map(|()| None),
    })
    .await?;
    Ok(Json(json!({"at": at})))
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
    Ok(Json(blocking(move || state.graph.files()).await?))
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
}

#[derive(Deserialize)]
struct UnsavedReq {
    session: u64,
    files: Vec<String>,
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
    let result = tokio::task::spawn_blocking(move || state.graph.write_file(r.session, &r.path, &r.base, &r.text)).await;
    match result {
        Ok(Ok(v)) => {
            s.graph_changed();
            Json(v).into_response()
        }
        Ok(Err(e)) => match e.downcast_ref::<crate::files::Conflict>() {
            Some(c) => (StatusCode::CONFLICT, Json(json!({"error": c.to_string(), "current": c.current}))).into_response(),
            None => ApiError::from(e).into_response(),
        },
        Err(e) => err(e.to_string()).into_response(),
    }
}
