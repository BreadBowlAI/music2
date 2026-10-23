//! Lane D's routes (next): turn an effect off and on, move it before or after another, and choose
//! what a Duck listens to. In a router of its own, merged into the
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
pub const WRITES_FX: &[&str] = &["/api/graph/effect/bypass", "/api/graph/effect/move", "/api/graph/effect/sidechain"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/effect/bypass", post(effect_bypass))
        .route("/api/graph/effect/move", post(effect_move))
        .route("/api/graph/effect/sidechain", post(effect_sidechain))
        .route_layer(axum::middleware::from_fn_with_state(state, writes))
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
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
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

#[derive(Deserialize)]
struct MoveReq {
    session: u64,
    revision: u64,
    /// The effect's card: its call, `file#ordinal`.
    node: String,
    /// Before the effect it is written around (Move left), or after the one around it.
    earlier: bool,
}

/// Move an effect before or after the one written next to it (Next D2): `{session, revision, node,
/// earlier}`, one Tx and one undo step. A write. Answers `{file, before, after, revision}`; effects
/// not written one inside the other are refused in words.
async fn effect_move(State(s): S, Json(r): Json<MoveReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.move_effect(r.session, r.revision, &r.node, r.earlier)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct SidechainReq {
    session: u64,
    revision: u64,
    /// The Duck's card: its call, `file#ordinal`.
    node: String,
    /// The track it is to listen to, by key.
    track: String,
}

/// Make a Duck listen to another track (Next D3): `{session, revision, node, track}`, one Tx and
/// one undo step. A write. Answers `{file, before, after, revision}`; refused in words when the
/// Duck can't hear that track from where it is written.
async fn effect_sidechain(State(s): S, Json(r): Json<SidechainReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.listen_to(r.session, r.revision, &r.node, &r.track)).await;
    s.graph_changed();
    Ok(Json(result?))
}
