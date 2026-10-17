//! The routes of polish's lane C, in a router of their own, merged into the API's: the view
//! settings kept per song (`.music2/view.json`), the recent songs list, Save As and Rename Song.
//! A route here that writes the open project is in [`WRITES_PROJECT`], and a read-only window
//! answers it 423 with WRITE-18's message, as `routes.rs` answers its own.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project. Save As only reads it: a read-only window has it.
pub const WRITES_PROJECT: &[&str] = &["/api/project/rename"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/view", post(graph_view))
        .route("/api/project/recent", get(recent_list))
        .route("/api/project/recent/forget", post(recent_forget))
        .route("/api/project/save-as", get(save_as_start).post(save_as))
        .route("/api/project/rename", post(rename))
        .route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_PROJECT.contains(&req.uri().path())
        && let Some(why) = s.graph.read_only()
    {
        return (StatusCode::LOCKED, Json(json!({"error": why}))).into_response();
    }
    next.run(req).await
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

#[derive(Deserialize)]
struct ViewReq {
    session: u64,
    set: Map<String, Value>,
}

/// Keep view settings of the open song (polish Q16): `{session, set: {key: value | null}}`, merged
/// into `.music2/view.json` (null removes one). Only the page's settings: `zoom`, `follow`,
/// `heights`, `hidden`, `grid`, `audition`; the loop has its own route. Not a write: a read-only
/// window keeps them in memory. Answers with them all, as the state's `local`.
async fn graph_view(State(s): S, Json(r): Json<ViewReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.set_view(r.session, r.set)).await?))
}

/// The recent songs, newest first (polish C2): `{songs: [{path, name, tempo, opened, missing}]}`.
/// Empty when this server keeps no list.
async fn recent_list(State(s): S) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || Ok(json!({"songs": state.recent().map(crate::recent::Recent::list).unwrap_or_default()}))).await?))
}

#[derive(Deserialize)]
struct ForgetReq {
    path: PathBuf,
}

/// Take a song off the recent list (its × on the start screen): `{path}`, as the list gave it.
/// Answers with the list.
async fn recent_forget(State(s): S, Json(r): Json<ForgetReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(
        blocking(move || {
            let Some(recent) = state.recent() else { return Ok(json!({"songs": []})) };
            recent.forget(&r.path)?;
            Ok(json!({"songs": recent.list()}))
        })
        .await?,
    ))
}

/// Where Save As starts: `{name, dir, home}`, the first free "<name> 2" in the song's folder's folder.
async fn save_as_start(State(s): S) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || crate::project::save_as_start(&state.graph)).await?))
}

/// Save a copy of the open song and open the copy (Q19): `{session, name, dir?}`. Answers with the
/// copy's state; every window hears a `graph` event.
async fn save_as(State(s): S, Json(r): Json<crate::project::SaveAs>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || crate::project::save_as(&state, &r)).await;
    s.graph_changed();
    Ok(Json(result?))
}

/// Rename the open song, and its folder too with `folder` (Q20): `{session, name, folder}`. A
/// write. Answers `{name, dir, warning}`.
async fn rename(State(s): S, Json(r): Json<crate::project::Rename>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || crate::project::rename(&state, &r)).await;
    s.graph_changed();
    Ok(Json(result?))
}
