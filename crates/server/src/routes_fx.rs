//! Lane D's routes (next), in the one route table (AGENT-8): turn an effect off and on, move it
//! before or after another, and choose what a Duck listens to.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/effect/bypass" => effect_bypass; writes, agents,
        "Turn an effect off (on: true bypasses it), or on again",
        r#"{"session": 7, "revision": 42, "node": "tracks/keys.ts#1", "on": true}"#;
    POST "/api/graph/effect/move" => effect_move; writes, agents,
        "Move an effect before the one written around it (earlier), or after the one around it",
        r#"{"session": 7, "revision": 42, "node": "tracks/keys.ts#1", "earlier": true}"#;
    POST "/api/graph/effect/sidechain" => effect_sidechain; writes, agents,
        "Make a Duck listen to another track",
        r#"{"session": 7, "revision": 42, "node": "song.ts#4", "track": "kick"}"#;
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
