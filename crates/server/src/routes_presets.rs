//! Presets (FMT-22, gaps #26): a sound card's list, Save as preset… and loading one.
//!
//! - `GET /api/graph/presets?session&node[&revision]`: `{sound, label, std: [{name}], yours:
//!   [{name}], plugin: [{name}]}`, each by name: `plugin` is the plugin's own (HOST-8), with
//!   `plugin_error` in words when they couldn't be read.
//! - `POST /api/graph/preset/save {session, node, name, replace, revision?}`: your preset from the
//!   call's settings; no song file changes. `{name, left}`, or `{taken}` when a preset of that name
//!   (in any capitals) is there and `replace` isn't set.
//! - `POST /api/graph/preset/load {session, at, node, from: "std"|"yours"|"plugin", name}`: one
//!   Tx (a write route: the lock applies). `{file, before, after, revision, changed, kept: [{label,
//!   why}]}`.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

pub fn router(_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/presets", get(list))
        .route("/api/graph/preset/save", post(save))
        .route("/api/graph/preset/load", post(load))
}

/// Run `f` off the async threads: presets are files.
async fn blocking(f: impl FnOnce() -> anyhow::Result<Value> + Send + 'static) -> ApiResult {
    Ok(Json(crate::caller::blocking(f).await.map_err(anyhow::Error::from)??))
}

#[derive(Deserialize)]
struct ListQuery {
    session: u64,
    /// The call, as the view names it (`tracks/keys.ts#3`).
    node: String,
    /// The revision `node` was named at; the accepted one when left out.
    #[serde(default)]
    revision: Option<u64>,
}

async fn list(State(s): S, Query(q): Query<ListQuery>) -> ApiResult {
    blocking(move || s.graph.presets(q.session, q.revision, &q.node)).await
}

#[derive(Deserialize)]
struct SaveReq {
    session: u64,
    node: String,
    name: String,
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    revision: Option<u64>,
}

async fn save(State(s): S, Json(r): Json<SaveReq>) -> ApiResult {
    blocking(move || s.graph.save_preset(r.session, r.revision, &r.node, &r.name, r.replace)).await
}

#[derive(Deserialize)]
struct LoadReq {
    session: u64,
    /// The revision `node` was named at.
    at: u64,
    node: String,
    /// `std` (std's own), `yours`, or `plugin` (the plugin's own).
    from: String,
    name: String,
}

async fn load(State(s): S, Json(r): Json<LoadReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.load_preset(r.session, r.at, &r.node, &r.from, &r.name)).await;
    s.graph_changed();
    result
}
