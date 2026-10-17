//! The routes of 8b.7's leases, in a router of their own, merged into the API's: drags on clips
//! heard while they last (WRITE-22a). A route here that writes the project, or begins a gesture
//! whose commit would, is in [`WRITES_LEASES`], and a read-only window answers it 423 with
//! WRITE-18's message, as `routes.rs` answers its own.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use m2_editor::clips::Gesture;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::graph::clips::Answer;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project, or begin a gesture whose commit would.
pub const WRITES_LEASES: &[&str] = &["/api/graph/lane/lease/begin", "/api/graph/lane/lease/commit"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/lane/lease/begin", post(lane_begin))
        .route("/api/graph/lane/lease/update", post(lane_update))
        .route("/api/graph/lane/lease/keepalive", post(lane_keepalive))
        .route("/api/graph/lane/lease/commit", post(lane_commit))
        .route("/api/graph/lane/lease/cancel", post(lane_cancel))
        .route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_LEASES.contains(&req.uri().path())
        && let Some(why) = s.graph.read_only()
    {
        return (StatusCode::LOCKED, Json(json!({"error": why}))).into_response();
    }
    next.run(req).await
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

/// A clip changed since the window drew it (WRITE-12): 409, as `POST /api/graph/clips` answers.
fn changed(why: String) -> Response {
    (StatusCode::CONFLICT, Json(json!({"error": why, "changed": true}))).into_response()
}

#[derive(Deserialize)]
struct BeginReq {
    session: u64,
    gesture: Gesture,
    /// The window (its WebSocket's `client`): when it closes, the drag ends.
    #[serde(default)]
    client: Option<String>,
    /// How long the lease lives without an update or a keepalive (10 s by default, 60 s at most).
    #[serde(default)]
    ttl_ms: Option<u64>,
}

/// Begin a drag on clips, heard as it goes: `{session, gesture, client?, ttl_ms?}` answers
/// `{lease, previewable, lanes: [{module, lane}], heard, revision}`. A gesture that isn't
/// previewed (an audio lane's) answers `previewable: false` and no lease: it is written on release.
async fn lane_begin(State(s): S, Json(r): Json<BeginReq>) -> Result<Response, ApiError> {
    let ttl = r.ttl_ms.map(Duration::from_millis);
    let begun = blocking(move || s.graph.lane_begin(r.session, r.gesture, r.client, ttl)).await?;
    Ok(match begun {
        Ok(b) => Json(json!(b)).into_response(),
        Err(why) => changed(why),
    })
}

#[derive(Deserialize)]
struct UpdateReq {
    lease: u64,
    /// Updates older than one taken are dropped; without, it follows the last.
    #[serde(default)]
    seq: Option<u64>,
    gesture: Gesture,
}

/// Hear a drag's newest gesture. Answers `{seq}`: the engine message's number, null when nothing
/// was sent or the update was older than one taken.
async fn lane_update(State(s): S, Json(r): Json<UpdateReq>) -> ApiResult {
    let seq = blocking(move || s.graph.lane_update(r.lease, r.seq, r.gesture)).await?;
    Ok(Json(json!({"seq": seq})))
}

#[derive(Deserialize)]
struct LeaseReq {
    lease: u64,
}

async fn lane_keepalive(State(s): S, Json(r): Json<LeaseReq>) -> ApiResult {
    s.graph.lane_keepalive(r.lease)?;
    Ok(Json(json!({})))
}

/// End a drag without a write: the accepted data plays again.
async fn lane_cancel(State(s): S, Json(r): Json<LeaseReq>) -> ApiResult {
    blocking(move || s.graph.lane_cancel(r.lease)).await?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct CommitReq {
    lease: u64,
    gesture: Gesture,
}

/// Write a drag: one Tx, one undo step. Answers as `POST /api/graph/clips` does: `{file, before,
/// after, revision, made}`, or 409 `{error, changed: true}` when a clip it names changed since.
async fn lane_commit(State(s): S, Json(r): Json<CommitReq>) -> Result<Response, ApiError> {
    let state = s.clone();
    let done = blocking(move || state.graph.lane_commit(r.lease, r.gesture)).await;
    s.graph_changed();
    Ok(match done? {
        Answer::Done(v) => Json(v).into_response(),
        Answer::Changed(why) => changed(why),
    })
}

#[cfg(test)]
#[path = "routes_leases_tests.rs"]
mod tests;
