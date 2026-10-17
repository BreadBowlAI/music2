//! The routes of polish's lane C, in a router of their own, merged into the API's: the view
//! settings kept per song (`.music2/view.json`). A route here that writes the open project is in
//! [`WRITES_PROJECT`], and a read-only window answers it 423 with WRITE-18's message, as
//! `routes.rs` answers its own.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project.
pub const WRITES_PROJECT: &[&str] = &[];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new().route("/api/graph/view", post(graph_view)).route_layer(axum::middleware::from_fn_with_state(state, writes))
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
