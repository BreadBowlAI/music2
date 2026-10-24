//! The routes of 8b.7's leases, in the one route table (AGENT-8): drags on clips (WRITE-22a) and
//! the tempo field's drag (RENDER-27a), heard while they last. Each is the musician's: agents don't
//! drag (AGENT-3).

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use m2_editor::clips::Gesture;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::graph::clips::Answer;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/lane/lease/begin" => lane_begin; writes, musician,
        "Begin a drag on clips, heard as it goes",
        r#"{"session": 7, "gesture": {"op": "move", "clips": [{"module": "keys", "lane": "chords", "id": "k3f9q2zt"}], "by": 1}, "client": "w1"}"#;
    POST "/api/graph/lane/lease/update" => lane_update; reads, musician,
        "Hear a clip drag's newest gesture",
        r#"{"lease": 3, "seq": 2, "gesture": {"op": "move", "clips": [{"module": "keys", "lane": "chords", "id": "k3f9q2zt"}], "by": 2}}"#;
    POST "/api/graph/lane/lease/keepalive" => lane_keepalive; reads, musician,
        "Keep a clip drag going while it is held still",
        r#"{"lease": 3}"#;
    POST "/api/graph/lane/lease/commit" => lane_commit; writes, musician,
        "Save a clip drag, as one undo step",
        r#"{"lease": 3, "gesture": {"op": "move", "clips": [{"module": "keys", "lane": "chords", "id": "k3f9q2zt"}], "by": 2}}"#;
    POST "/api/graph/lane/lease/cancel" => lane_cancel; reads, musician,
        "End a clip drag without saving",
        r#"{"lease": 3}"#;
    POST "/api/graph/tempo/lease/begin" => tempo_begin; writes, musician,
        "Begin a drag of the tempo field, heard as it goes",
        r#"{"session": 7, "at": 0, "client": "w1"}"#;
    POST "/api/graph/tempo/lease/update" => tempo_update; reads, musician,
        "Hear a tempo drag's newest value",
        r#"{"lease": 3, "seq": 2, "bpm": 126}"#;
    POST "/api/graph/tempo/lease/keepalive" => tempo_keepalive; reads, musician,
        "Keep a tempo drag going while it is held still",
        r#"{"lease": 3}"#;
    POST "/api/graph/tempo/lease/commit" => tempo_commit; writes, musician,
        "Save a tempo drag, as one undo step",
        r#"{"lease": 3, "bpm": 126}"#;
    POST "/api/graph/tempo/lease/cancel" => tempo_cancel; reads, musician,
        "End a tempo drag without saving",
        r#"{"lease": 3}"#;
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
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

#[derive(Deserialize)]
struct TempoBeginReq {
    session: u64,
    /// A beat: the tempo entry in effect there is dragged (the playhead's).
    at: f64,
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    ttl_ms: Option<u64>,
}

/// Begin a drag of the tempo field: `{session, at, client?, ttl_ms?}` answers `{lease, at, bpm,
/// heard}`: the entry dragged, and whether it is heard as it goes (not in a song with audio).
async fn tempo_begin(State(s): S, Json(r): Json<TempoBeginReq>) -> ApiResult {
    let ttl = r.ttl_ms.map(Duration::from_millis);
    let begun = blocking(move || s.graph.tempo_begin(r.session, r.at, r.client, ttl)).await?;
    Ok(Json(json!(begun)))
}

#[derive(Deserialize)]
struct TempoUpdateReq {
    lease: u64,
    #[serde(default)]
    seq: Option<u64>,
    bpm: f64,
}

/// A tempo drag's newest value, heard at the first block after its plan is prepared. Answers
/// `{seq}`: null when the update was older than one taken.
async fn tempo_update(State(s): S, Json(r): Json<TempoUpdateReq>) -> ApiResult {
    let seq = blocking(move || s.graph.tempo_update(r.lease, r.seq, r.bpm)).await?;
    Ok(Json(json!({"seq": seq})))
}

async fn tempo_keepalive(State(s): S, Json(r): Json<LeaseReq>) -> ApiResult {
    s.graph.tempo_keepalive(r.lease)?;
    Ok(Json(json!({})))
}

/// End a tempo drag without a write: the accepted tempo plays again.
async fn tempo_cancel(State(s): S, Json(r): Json<LeaseReq>) -> ApiResult {
    blocking(move || s.graph.tempo_cancel(r.lease)).await?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct TempoCommitReq {
    lease: u64,
    bpm: f64,
}

/// Write a tempo drag: one Tx, one undo step, answered as `POST /api/graph/tempo` answers; 409
/// `{error, changed: true}` when the tempo changed elsewhere since it began.
async fn tempo_commit(State(s): S, Json(r): Json<TempoCommitReq>) -> Result<Response, ApiError> {
    let state = s.clone();
    let done = blocking(move || state.graph.tempo_commit(r.lease, r.bpm)).await;
    s.graph_changed();
    Ok(match done? {
        crate::graph::TempoCommitted::Done(v) => Json(v).into_response(),
        crate::graph::TempoCommitted::Changed(why) => changed(why),
    })
}

#[cfg(test)]
#[path = "routes_leases_tests.rs"]
mod tests;
