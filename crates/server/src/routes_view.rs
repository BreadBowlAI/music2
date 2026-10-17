//! The timeline's view routes, in a router of their own, merged into the API's: the page's view
//! settings, kept per song in `.music2/view.json` (Q16), which is not a write: a read-only window
//! keeps them in memory only; and the ruler's markers (Q13), which are. A route here that writes
//! the project is in [`WRITES_VIEW`], and a read-only window answers it 423 with WRITE-18's
//! message, as `routes.rs` answers its own.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use m2_editor::markers::Marker;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project.
pub const WRITES_VIEW: &[&str] = &["/api/graph/markers"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/view", post(graph_view))
        .route("/api/graph/markers", post(graph_markers))
        .route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_VIEW.contains(&req.uri().path())
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

/// The page's view settings (Q16): `{session, set: {key: value | null}}`, merged into what the
/// state's `local` holds (null takes a key out). Only `zoom`, `follow`, `heights`, `hidden`,
/// `grid` and `audition`. Answers `{local}`.
async fn graph_view(State(s): S, Json(r): Json<ViewReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.set_view(r.session, r.set)).await?))
}

#[derive(Deserialize)]
struct MarkersReq {
    session: u64,
    markers: Vec<Marker>,
}

/// The ruler's markers (Q13): `{session, markers: [{beat, name}]}`, the whole list, written into
/// `layout.json`. One undo step with no revision. A write. Answers `{file, before, after, revision}`.
async fn graph_markers(State(s): S, Json(r): Json<MarkersReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.markers(r.session, r.markers)).await;
    s.graph_changed();
    Ok(Json(result?))
}
