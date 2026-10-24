//! The routes of the polish track's lane D, in the one route table (AGENT-8): the song's key.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use m2_editor::songkey::SongKey;
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/song-key" => song_key; writes, agents,
        "Set the song's key, or take it away (null)",
        r#"{"session": 7, "key": {"root": "A", "scale": "minor"}}"#;
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
