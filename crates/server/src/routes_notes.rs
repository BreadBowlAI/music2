//! The routes of the polish track's lane D, in a router of their own, merged into the API's: the
//! song's key. A route here that writes the project is in [`WRITES_NOTES`], and a read-only window
//! answers it 423 with WRITE-18's message, as `routes.rs` answers its own.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use m2_editor::songkey::SongKey;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project.
pub const WRITES_NOTES: &[&str] = &["/api/graph/song-key"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new().route("/api/graph/song-key", post(song_key)).route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_NOTES.contains(&req.uri().path())
        && let Some(why) = s.graph.read_only()
    {
        return (StatusCode::LOCKED, Json(json!({"error": why}))).into_response();
    }
    next.run(req).await
}

#[derive(Deserialize)]
struct SongKeyReq {
    session: u64,
    /// `{root, scale}`, or null for no key.
    key: Option<SongKey>,
}

/// The song's key (D2, Q22): `{session, key: {root, scale} | null}`, written into `layout.json` as
/// one undo step with no revision. Named `song-key`, apart from a track's ID (taste.md:71). A
/// write. Answers `{file, before, after, revision}`.
async fn song_key(State(s): S, Json(r): Json<SongKeyReq>) -> ApiResult {
    let state = s.clone();
    let result = crate::caller::blocking(move || state.graph.song_key(r.session, r.key)).await.map_err(anyhow::Error::from)?;
    s.graph_changed();
    Ok(Json(result?))
}
