//! HTTP + WebSocket API for the open song project. Thin: every edit goes through [`Graph`](crate::graph::Graph).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
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
        .route("/api/graph/connect", post(graph_connect))
        .route("/api/graph/effect/add", post(graph_add_effect))
        .route("/api/graph/effect/remove", post(graph_remove_effect))
        .route("/api/graph/lane", post(graph_lane))
        .route("/api/graph/undo", post(graph_undo))
        .route("/api/graph/redo", post(graph_redo))
        .route("/api/graph/disconnect", post(graph_disconnect))
        .route("/api/graph/transport", post(graph_transport))
        .route("/api/graph/seek", post(graph_seek))
        .route("/api/graph/files", get(graph_files))
        .route("/api/graph/file", put(graph_put_file))
        .route("/api/graph/types", get(graph_types))
        .route("/api/graph/unsaved", post(graph_unsaved))
        .route("/api/graph/export", post(graph_export))
        .route("/api/project/templates", get(project_templates))
        .route("/api/project/new", post(project_new))
        // An unknown API path is a clear error, not the UI's index.html.
        .route("/api/{*rest}", any(no_such_api))
        .fallback_service(spa)
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

async fn no_such_api() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": "no such API"}))).into_response()
}

// ---------------------------------------------------------------- events

async fn ws(State(s): S, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| ws_loop(s, socket))
}

/// Forward every event (toasts, `graph`) to one client until it goes away.
async fn ws_loop(state: Arc<AppState>, mut socket: WebSocket) {
    let (unheard, mut events) = state.subscribe();
    for text in unheard {
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
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
    s.graph.close();
    s.graph_changed();
    Ok(Json(json!({})))
}

async fn graph_status(State(s): S) -> Json<Value> {
    Json(json!(s.graph.status()))
}

#[derive(Deserialize)]
struct PreviewReq {
    session: u64,
    revision: u64,
    id: String,
    gesture: String,
    value: f64,
}

async fn graph_preview(State(s): S, Json(r): Json<PreviewReq>) -> ApiResult {
    let state = s.clone();
    blocking(move || state.graph.preview(r.session, r.revision, &r.id, &r.gesture, r.value)).await?;
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

async fn graph_set(State(s): S, Json(r): Json<SetReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.set(r.session, r.revision, &r.id, &r.gesture, r.value, r.expect)).await;
    s.graph_changed();
    Ok(Json(result?))
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
    #[serde(default)]
    module: String,
    #[serde(default)]
    name: String,
}

async fn graph_add_effect(State(s): S, Json(r): Json<EffectReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.add_effect(r.session, r.revision, &r.call, &r.module, &r.name)).await;
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
    clips: Vec<m2_session::NoteClip>,
}

/// Rewrite a notes lane's clips (the piano roll; clips made, moved or deleted in the timeline).
async fn graph_lane(State(s): S, Json(r): Json<LaneReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.write_notes(r.session, r.revision, &r.module, &r.lane, &r.clips)).await;
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
}

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
