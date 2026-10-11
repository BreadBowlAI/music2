//! HTTP + WebSocket API. Thin: every edit is still `Session::execute`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use m2_audio_io::Msg;
use m2_engine::{COMMANDS, Step};
use m2_project::Song;
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};

use crate::{AppState, library_dir};

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
        .route("/api/state", get(get_state))
        .route("/api/ws", get(ws))
        .route("/api/exec", post(exec))
        .route("/api/steps", post(steps))
        .route("/api/group", post(group))
        .route("/api/undo", post(undo))
        .route("/api/redo", post(redo))
        .route("/api/transport", post(transport))
        .route("/api/loop", post(set_loop))
        .route("/api/note", post(note))
        .route("/api/song/new", post(new_song))
        .route("/api/song/open", post(open_song))
        .route("/api/song/save", post(save_song))
        .route("/api/song/export", post(export_song))
        .route("/api/library", get(library))
        .route("/api/reveal", post(reveal))
        .route("/api/import-path", post(import_path))
        .route("/api/commands", get(commands))
        .route("/api/describe", get(describe))
        .route("/api/settings", get(get_settings))
        .route("/api/peaks/{asset}", get(peaks))
        .route("/api/plugin", get(plugin_def))
        .route("/api/graph", get(graph_get))
        .route("/api/graph/open", post(graph_open))
        .route("/api/graph/close", post(graph_close))
        .route("/api/graph/status", get(graph_status))
        .route("/api/graph/preview", post(graph_preview))
        .route("/api/graph/preview/cancel", post(graph_cancel_preview))
        .route("/api/graph/set", post(graph_set))
        .route("/api/graph/transport", post(graph_transport))
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

// ---------------------------------------------------------------- state

fn plugins_json(state: &AppState) -> Value {
    let reg = state.session.lock().registry.clone();
    json!(
        reg.list()
            .map(|p| json!({
                "id": p.id, "name": p.name, "kind": p.kind, "description": p.description, "tags": p.tags,
                "macros": p.macros, "asset_slots": p.asset_slots, "uses_sidechain": p.uses_sidechain(),
                "pads": p.pads.iter().map(|x| json!({"pitch": x.pitch, "name": x.name})).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>()
    )
}

/// Everything except the song (small; diffed as JSON).
struct Snapshot {
    meta: Value,
    song: Arc<Song>,
}

fn snapshot(state: &AppState) -> Snapshot {
    let (song, revision, path, dirty, undo, redo) = {
        let s = state.session.lock();
        (
            s.song().clone(),
            s.revision(),
            s.path().map(|p| p.display().to_string()),
            s.is_dirty(),
            s.undo_label().map(str::to_string),
            s.redo_label().map(str::to_string),
        )
    };
    let meta = json!({
        "revision": revision,
        "ui_rev": state.ui_rev.load(Ordering::Relaxed),
        "end_tick": song.end_tick(),
        "path": path,
        "dirty": dirty,
        "undo": undo,
        "redo": redo,
        "looping": state.looping.load(Ordering::Relaxed),
        "plugins": plugins_json(state),
        "audio": {
            "ok": state.audio.is_some(),
            "device": state.audio.as_ref().map(|a| a.device_name.clone()),
            "sample_rate": state.audio.as_ref().map(|a| a.sample_rate),
            "error": state.audio_error,
        },
        "library": library_dir().display().to_string(),
    });
    Snapshot { meta, song }
}

fn state_json(state: &AppState) -> Value {
    let snap = snapshot(state);
    let mut v = snap.meta;
    v["song"] = serde_json::to_value(&*snap.song).unwrap_or(Value::Null);
    v
}

async fn get_state(State(s): S) -> Json<Value> {
    Json(state_json(&s))
}

async fn ws(State(s): S, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| ws_loop(s, socket))
}

async fn ws_loop(state: Arc<AppState>, mut socket: WebSocket) {
    let mut events = state.events.subscribe();
    let mut last = (u64::MAX, u64::MAX);
    // What this client currently has; edits are sent as patches against it.
    let mut sent: Option<Snapshot> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(33));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let key = (state.session.lock().revision(), state.ui_rev.load(Ordering::Relaxed));
                if key != last {
                    last = key;
                    let now = snapshot(&state);
                    let msg = match &sent {
                        Some(prev) => {
                            let mut ops = crate::patch::diff(&prev.meta, &now.meta);
                            // Songs are compared structurally: unchanged tracks are skipped by pointer.
                            crate::patch::song_diff(&prev.song, &now.song, &["song".into()], &mut ops);
                            (!ops.is_empty()).then(|| json!({"t": "patch", "ops": ops}).to_string())
                        }
                        None => Some(json!({"t": "state", "state": state_json(&state)}).to_string()),
                    };
                    sent = Some(now);
                    if let Some(msg) = msg
                        && socket.send(Message::Text(msg.into())).await.is_err()
                    {
                        break;
                    }
                }
                if let Some(a) = &state.audio {
                    let song = state.session.lock().song().clone();
                    let peaks: serde_json::Map<String, Value> =
                        song.tracks.iter().map(|t| (t.id.to_string(), json!(a.shared.track_peak(t.id)))).collect();
                    let msg = json!({"t": "tick", "playhead": a.playhead(), "playing": a.is_playing(),
                        "master": a.shared.master_peak(), "peaks": peaks}).to_string();
                    if socket.send(Message::Text(msg.into())).await.is_err() { break; }
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
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------- editing

#[derive(Deserialize)]
struct ExecReq {
    cmd: String,
    #[serde(default)]
    params: Value,
}

async fn exec(State(s): S, Json(r): Json<ExecReq>) -> ApiResult {
    let params = if r.params.is_null() { json!({}) } else { r.params };
    let out = s.session.lock().execute(&r.cmd, params)?;
    if r.cmd.starts_with("clip.") || r.cmd.starts_with("notes.") || r.cmd == "song.set" {
        s.sync_loop();
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
struct StepsReq {
    label: String,
    steps: Vec<Step>,
}

async fn steps(State(s): S, Json(r): Json<StepsReq>) -> ApiResult {
    s.session.lock().execute_steps(&r.label, &r.steps)?;
    s.sync_loop();
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct GroupReq {
    action: String,
    #[serde(default)]
    label: String,
}

async fn group(State(s): S, Json(r): Json<GroupReq>) -> ApiResult {
    let mut sess = s.session.lock();
    match r.action.as_str() {
        "begin" => sess.begin_group(&r.label),
        _ => sess.end_group(),
    }
    Ok(Json(json!({})))
}

async fn undo(State(s): S) -> ApiResult {
    let l = s.session.lock().undo();
    s.sync_loop();
    Ok(Json(json!({"label": l})))
}

async fn redo(State(s): S) -> ApiResult {
    let l = s.session.lock().redo();
    s.sync_loop();
    Ok(Json(json!({"label": l})))
}

#[derive(Deserialize)]
struct TransportReq {
    action: String,
    #[serde(default)]
    tick: i64,
}

async fn transport(State(s): S, Json(r): Json<TransportReq>) -> ApiResult {
    match r.action.as_str() {
        "play" => {
            s.sync_loop();
            s.send(Msg::Play);
        }
        "stop" => s.send(Msg::Stop),
        "toggle" => {
            if s.audio.as_ref().is_some_and(|a| a.is_playing()) {
                s.send(Msg::Stop);
            } else {
                s.sync_loop();
                s.send(Msg::Play);
            }
        }
        "seek" => s.send(Msg::Seek(r.tick.max(0))),
        "panic" => s.send(Msg::Panic),
        other => return Err(err(format!("unknown transport action {other}"))),
    }
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct LoopReq {
    on: bool,
}

async fn set_loop(State(s): S, Json(r): Json<LoopReq>) -> ApiResult {
    s.looping.store(r.on, Ordering::Relaxed);
    s.sync_loop();
    s.bump();
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct NoteReq {
    track: u64,
    pitch: u8,
    on: bool,
}

async fn note(State(s): S, Json(r): Json<NoteReq>) -> ApiResult {
    s.send(Msg::LiveNote { track: r.track, pitch: r.pitch, velocity: 100, on: r.on });
    Ok(Json(json!({})))
}

// ---------------------------------------------------------------- files

async fn new_song(State(s): S) -> ApiResult {
    s.new_song()?;
    Ok(Json(json!({})))
}

async fn library() -> ApiResult {
    let dir = library_dir();
    let mut songs: Vec<(std::time::SystemTime, Value)> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "m2song"))
                .map(|e| {
                    let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                    let secs = modified.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    let name = e.path().file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    (modified, json!({"name": name, "path": e.path().display().to_string(), "modified": secs}))
                })
                .collect()
        })
        .unwrap_or_default();
    songs.sort_by_key(|s| std::cmp::Reverse(s.0));
    Ok(Json(json!({"dir": dir.display().to_string(), "songs": songs.into_iter().map(|x| x.1).collect::<Vec<_>>()})))
}

#[derive(Deserialize)]
struct OpenReq {
    path: String,
}

async fn open_song(State(s): S, Json(r): Json<OpenReq>) -> ApiResult {
    s.open_song(Path::new(&r.path))?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct SaveReq {
    /// Save a copy under this title in the library (Save as).
    #[serde(default)]
    title: Option<String>,
    /// Save to this file (desktop Save As dialog).
    #[serde(default)]
    path: Option<PathBuf>,
}

async fn save_song(State(s): S, Json(r): Json<SaveReq>) -> ApiResult {
    let path = s.save(r.path.as_deref(), r.title.as_deref())?;
    Ok(Json(json!({"path": path.display().to_string()})))
}

#[derive(Deserialize, Default)]
struct ExportReq {
    #[serde(default)]
    path: Option<PathBuf>,
}

async fn export_song(State(s): S, Json(r): Json<ExportReq>) -> ApiResult {
    let path = blocking(move || s.export_wav(r.path)).await?;
    Ok(Json(json!({"path": path.display().to_string()})))
}

async fn reveal(Json(r): Json<OpenReq>) -> ApiResult {
    crate::reveal(Path::new(&r.path))?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct ImportPathReq {
    path: PathBuf,
    #[serde(default, rename = "as")]
    as_: Option<String>,
    #[serde(default)]
    bar: Option<f64>,
}

/// Import a WAV by path (drag-and-drop, scripts).
async fn import_path(State(s): S, Json(r): Json<ImportPathReq>) -> ApiResult {
    let bar = r.bar.unwrap_or_else(|| s.playhead_bar());
    Ok(Json(s.import_audio_file(&r.path, r.as_.as_deref() == Some("instrument"), bar)?))
}

// ---------------------------------------------------------------- introspection

/// Every editing command with its JSON Schema: what `/api/exec` and `/api/steps` accept.
async fn commands() -> Json<Value> {
    Json(json!(
        COMMANDS
            .iter()
            .map(|c| json!({"id": c.id, "description": c.description, "mutates": c.mutates, "schema": (c.schema)()}))
            .collect::<Vec<_>>()
    ))
}

#[derive(Deserialize)]
struct DescribeQuery {
    #[serde(default)]
    clip: Option<u64>,
}

/// A compact, readable summary of the song (ids, names, bars), optionally with one
/// clip's notes. Cheaper to read than `/api/state`.
async fn describe(State(s): S, Query(q): Query<DescribeQuery>) -> Json<Value> {
    let sess = s.session.lock();
    Json(m2_engine::commands::describe(sess.song(), &sess.registry, q.clip))
}

async fn get_settings(State(s): S) -> ApiResult {
    Ok(Json(json!({
        "plugins_dir": m2_engine::plugins_dir().display().to_string(),
        "library": library_dir().display().to_string(),
        "audio": {
            "device": s.audio.as_ref().map(|a| a.device_name.clone()),
            "sample_rate": s.audio.as_ref().map(|a| a.sample_rate),
            "error": s.audio_error,
        },
    })))
}

/// Normalized waveform overview (512 bins) for drawing audio clips.
async fn peaks(State(s): S, UrlPath(asset): UrlPath<u64>) -> ApiResult {
    let data = s.session.lock().bank().get(asset).cloned().ok_or_else(|| err("no such asset"))?;
    let bins = 512usize;
    let per = (data.frames.len() / bins).max(1);
    let raw: Vec<f32> = data.frames.chunks(per).map(|c| c.iter().fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))).collect();
    let max = raw.iter().cloned().fold(0.01f32, f32::max);
    Ok(Json(json!({"peaks": raw.iter().map(|p| ((p / max).sqrt() * 1000.0).round() / 1000.0).collect::<Vec<_>>()})))
}

#[derive(Deserialize)]
struct PluginQuery {
    id: String,
}

/// The full m2plug JSON for a plugin.
async fn plugin_def(State(s): S, Query(q): Query<PluginQuery>) -> ApiResult {
    let def = s.session.lock().registry.get(&q.id).ok_or_else(|| err("no such plugin"))?;
    Ok(Json(serde_json::to_value(def.as_ref())?))
}

// ---------------------------------------------------------------- graph projects (new core)

async fn graph_get(State(s): S) -> Result<Json<Value>, ApiError> {
    s.graph.state().map(Json).ok_or_else(|| err("no project is open"))
}

#[derive(Deserialize)]
struct GraphOpenReq {
    path: PathBuf,
}

async fn graph_open(State(s): S, Json(r): Json<GraphOpenReq>) -> ApiResult {
    let state = s.clone();
    let v = blocking(move || state.graph.open(&r.path)).await?;
    let _ = s.events.send(json!({"t": "graph"}).to_string());
    Ok(Json(v))
}

async fn graph_close(State(s): S) -> ApiResult {
    s.graph.close();
    let _ = s.events.send(json!({"t": "graph"}).to_string());
    Ok(Json(json!({})))
}

async fn graph_status(State(s): S) -> Json<Value> {
    let (playing, seconds) = s.graph.transport_state();
    Json(json!({"playing": playing, "seconds": seconds}))
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
    let _ = s.events.send(json!({"t": "graph"}).to_string());
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
