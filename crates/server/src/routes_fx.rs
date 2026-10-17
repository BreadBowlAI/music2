//! Lane D's routes (next): turn an effect off and on. In a router of its own, merged into the
//! API's, as `routes_mix.rs` is; a route here that writes the project is in [`WRITES_FX`], and a
//! read-only window answers it 423 with WRITE-18's message.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project.
pub const WRITES_FX: &[&str] = &["/api/graph/effect/bypass"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new().route("/api/graph/effect/bypass", post(effect_bypass)).route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_FX.contains(&req.uri().path())
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
struct BypassReq {
    session: u64,
    revision: u64,
    /// The effect's card: its call, `file#ordinal`.
    node: String,
    /// Turn it off (`bypass: true`), or on again.
    on: bool,
}

/// Turn an effect off or on again (Next D1): `{session, revision, node, on}`, one Tx and one undo
/// step. A write. Answers `{file, before, after, revision}`; a refusal says why in words.
async fn effect_bypass(State(s): S, Json(r): Json<BypassReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.bypass(r.session, r.revision, &r.node, r.on)).await;
    s.graph_changed();
    Ok(Json(result?))
}
