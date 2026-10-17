//! The routes of 8b's back ends, in a router of their own, merged into the API's (G5): mute and
//! solo. A route here that writes the project is in [`WRITES_8B`], and a read-only window answers
//! it 423 with WRITE-18's message, as `routes.rs` answers its own.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::graph::overlay::Change;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project.
pub const WRITES_8B: &[&str] = &["/api/graph/track/mute"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/overlay", post(graph_overlay))
        .route("/api/graph/track/mute", post(track_mute))
        .route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_8B.contains(&req.uri().path())
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
struct OverlayReq {
    session: u64,
    #[serde(flatten)]
    change: Change,
}

/// Mute or solo tracks while listening (MON-1 to MON-4): `{session, mute?: {k: bool}, solo?: {k:
/// bool}, exclusive?: k, clear?: true}`. Not a write: it works read-only, in memory. Answers with
/// the state's `overlays`; every window hears a `graph` event.
async fn graph_overlay(State(s): S, Json(r): Json<OverlayReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.overlay(r.session, r.change)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct MuteReq {
    session: u64,
    modules: Vec<String>,
    muted: bool,
}

/// Mute tracks in the song, or unmute them (MON-5): `{session, modules: [key], muted}`, one Tx and
/// one undo step. A write. Answers `{file, before, after, revision}`.
async fn track_mute(State(s): S, Json(r): Json<MuteReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.mute_in_song(r.session, r.modules, r.muted)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[cfg(test)]
#[path = "routes_8b_tests.rs"]
mod tests;
