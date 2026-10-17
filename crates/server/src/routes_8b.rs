//! The routes of 8b's back ends, in a router of their own, merged into the API's (G5): mute and
//! solo, clip gestures, and audition, also read off the WebSocket ([`ws_text`]). A route here that writes the
//! project is in [`WRITES_8B`], and a read-only window answers it 423 with WRITE-18's message, as
//! `routes.rs` answers its own.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::graph::audition::Audition;
use crate::graph::clips::Answer;
use crate::graph::overlay::Change;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project.
pub const WRITES_8B: &[&str] = &["/api/graph/track/mute", "/api/graph/clips", "/api/graph/automate/remove"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/overlay", post(graph_overlay))
        .route("/api/graph/track/mute", post(track_mute))
        .route("/api/graph/clips", post(graph_clips))
        .route("/api/graph/automate/remove", post(automate_remove))
        .route("/api/graph/audition", post(graph_audition))
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

#[derive(Deserialize)]
struct ClipsReq {
    session: u64,
    gesture: m2_editor::clips::Gesture,
}

/// A clip gesture (8b.5): `{session, gesture: {op, …}}`, one Tx and one undo step. A write.
/// Answers `{file, before, after, revision, made: [{module, lane, id}]}`; 409 `{error, changed:
/// true}` when a clip it names by its `sha` changed since the window drew it (WRITE-12).
async fn graph_clips(State(s): S, Json(r): Json<ClipsReq>) -> Result<Response, ApiError> {
    let state = s.clone();
    let result = blocking(move || state.graph.clips(r.session, r.gesture)).await;
    s.graph_changed();
    Ok(match result? {
        Answer::Done(v) => Json(v).into_response(),
        Answer::Changed(why) => (StatusCode::CONFLICT, Json(json!({"error": why, "changed": true}))).into_response(),
    })
}

#[derive(Deserialize)]
struct UnautomateReq {
    session: u64,
    module: String,
    lane: String,
    /// The playhead: the lane's value there is written. None: its fallback.
    #[serde(default)]
    beat: Option<f64>,
}

/// Remove automation (HOME-2): `{session, module, lane, beat?}`. A write. Answers `{file, before,
/// after, revision}`.
async fn automate_remove(State(s): S, Json(r): Json<UnautomateReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.remove_automation(r.session, r.module, r.lane, r.beat)).await;
    s.graph_changed();
    Ok(Json(result?))
}

/// Notes clips as the timeline shows them, each with its `sha` (8b.5), read as clips: what the
/// piano roll's whole-lane write sends back until it sends `notes` gestures (Q9).
pub(crate) fn shown_clips<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<m2_song::NoteClip>, D::Error> {
    let mut clips = Vec::<Value>::deserialize(d)?;
    for c in &mut clips {
        if let Some(c) = c.as_object_mut() {
            c.remove("sha");
        }
    }
    clips.into_iter().map(|c| serde_json::from_value(c).map_err(serde::de::Error::custom)).collect()
}

#[derive(Deserialize)]
struct AuditionReq {
    /// The window, or the agent, playing: its notes are its own.
    client: String,
    #[serde(flatten)]
    audition: Audition,
}

/// Play or let go of notes on a lane (MON-7): `{client, module, lane, on: [{h, pitch, velocity}],
/// off: [h], keep: [h]}`, as a window sends on its WebSocket. Not a write: it works read-only, and
/// never waits for the session. Answers `{}`, or `{"t": "audition", "silent": …}` when nothing
/// plays the lane.
async fn graph_audition(State(s): S, Json(r): Json<AuditionReq>) -> ApiResult {
    let answer = blocking(move || s.graph.audition(&r.client, r.audition)).await?;
    Ok(Json(answer.unwrap_or_else(|| json!({}))))
}

/// A message a window sent on its WebSocket: `{"t": "audition", …}` from the window `client` plays
/// notes as [`graph_audition`] does. What it answers goes to that window only, as text; an error
/// is `{"t": "audition", "error": …}`. Other messages are ignored.
pub(crate) async fn ws_text(state: &Arc<AppState>, client: Option<&str>, text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    if v["t"] != "audition" {
        return None;
    }
    let said = |e: String| Some(json!({"t": "audition", "error": e}).to_string());
    let Some(client) = client.map(str::to_string) else {
        return said("This window has no client id, which audition needs.".into());
    };
    let a: Audition = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return said(format!("This audition message doesn't read: {e}")),
    };
    let state = state.clone();
    match tokio::task::spawn_blocking(move || state.graph.audition(&client, a)).await {
        Ok(Ok(answer)) => answer.map(|v| v.to_string()),
        Ok(Err(e)) => said(format!("{e:#}")),
        Err(e) => said(e.to_string()),
    }
}

#[cfg(test)]
#[path = "routes_8b_tests.rs"]
mod tests;
