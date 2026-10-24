//! The routes of polish's lane C, in the one route table (AGENT-8): the recent songs list, Save As
//! and Rename Song. (The view settings' route is `routes_view.rs`.) Save As only reads the song: a
//! read-only window has it.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    GET "/api/project/recent" => recent_list; reads, agents,
        "The songs opened lately, newest first",
        r#"{}"#;
    POST "/api/project/recent/forget" => recent_forget; reads, musician,
        "Take a song off the recent list",
        r#"{"path": "/Users/you/Music/music2/Night drive"}"#;
    GET "/api/project/save-as" => save_as_start; reads, agents,
        "Where Save As starts: the folder the song is in, and the first free name",
        r#"{}"#;
    POST "/api/project/save-as" => save_as; reads, musician,
        "Save a copy of the song, and open the copy",
        r#"{"session": 7, "name": "Night drive 2"}"#;
    POST "/api/project/rename" => rename; writes, musician,
        "Rename the song, and its folder too with folder",
        r#"{"session": 7, "name": "Night drive", "folder": true}"#;
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

/// The recent songs, newest first (polish C2): `{songs: [{path, name, tempo, opened, missing}]}`.
/// Empty when this server keeps no list.
async fn recent_list(State(s): S) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || Ok(json!({"songs": state.recent().map(crate::recent::Recent::list).unwrap_or_default()}))).await?))
}

#[derive(Deserialize)]
struct ForgetReq {
    path: PathBuf,
}

/// Take a song off the recent list (its × on the start screen): `{path}`, as the list gave it.
/// Answers with the list.
async fn recent_forget(State(s): S, Json(r): Json<ForgetReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(
        blocking(move || {
            let Some(recent) = state.recent() else { return Ok(json!({"songs": []})) };
            recent.forget(&r.path)?;
            Ok(json!({"songs": recent.list()}))
        })
        .await?,
    ))
}

/// Where Save As starts: `{name, dir, home}`, the first free "<name> 2" in the song's folder's folder.
async fn save_as_start(State(s): S) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || crate::project::save_as_start(&state.graph)).await?))
}

/// Save a copy of the open song and open the copy (Q19): `{session, name, dir?}`. Answers with the
/// copy's state; every window hears a `graph` event.
async fn save_as(State(s): S, Json(r): Json<crate::project::SaveAs>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || crate::project::save_as(&state, &r)).await;
    s.graph_changed();
    Ok(Json(result?))
}

/// Rename the open song, and its folder too with `folder` (Q20): `{session, name, folder}`. A
/// write. Answers `{name, dir, warning}`.
async fn rename(State(s): S, Json(r): Json<crate::project::Rename>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || crate::project::rename(&state, &r)).await;
    s.graph_changed();
    Ok(Json(result?))
}
