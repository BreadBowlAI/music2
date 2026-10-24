//! The timeline's view routes, in the one route table (AGENT-8): the page's view settings, kept per
//! song in `.music2/view.json` (Q16), which is not a write: a read-only window keeps them in memory
//! only; and the ruler's markers (Q13), which are.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use m2_editor::markers::Marker;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/view" => graph_view; reads, musician,
        "Set the timeline's view (zoom, follow, track heights, hidden lanes, the grid, audition, the metronome and the guide)",
        r#"{"session": 7, "set": {"zoom": 2}}"#;
    POST "/api/graph/markers" => graph_markers; writes, agents,
        "Write the ruler's markers, the whole list, into layout.json",
        r#"{"session": 7, "markers": [{"beat": 32, "name": "Chorus"}]}"#;
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

#[derive(Deserialize)]
struct ViewReq {
    session: u64,
    set: Map<String, Value>,
}

/// The page's view settings (Q16): `{session, set: {key: value | null}}`, merged into what the
/// state's `local` holds (null takes a key out). Only the 8 keys of `view_file::KEYS`: `zoom`,
/// `follow`, `heights`, `hidden`, `grid`, `audition`, `metronome` and `guide`. Answers `{local}`.
async fn graph_view(State(s): S, Json(r): Json<ViewReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.set_view(r.session, r.set)).await?))
}

#[derive(Deserialize)]
struct MarkersReq {
    session: u64,
    markers: Vec<Marker>,
}

/// The ruler's markers (Q13): `{session, markers: [{beat, name}]}`, the whole list, written into
/// `layout.json`. One undo step with no revision. A write. Answers `{file, before, after, revision}`.
async fn graph_markers(State(s): S, Json(r): Json<MarkersReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.markers(r.session, r.markers)).await;
    s.graph_changed();
    Ok(Json(result?))
}
