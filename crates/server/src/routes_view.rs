//! The timeline's view routes, in a router of their own, merged into the API's: the page's view
//! settings, kept per song in `.music2/view.json` (Q16). Not a write: a read-only window keeps
//! them in memory only.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/api/graph/view", post(graph_view))
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
