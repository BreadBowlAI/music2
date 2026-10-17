//! Audio in the app (lane A), in a router of its own, merged into the API's: an audio file's
//! waveform peaks (Q9). A route here that writes the project is in [`WRITES_AUDIO`], and a
//! read-only window answers it 423 with WRITE-18's message, as `routes.rs` answers its own.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;
use crate::peaks::Store;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;

/// The routes here that write the open project.
pub const WRITES_AUDIO: &[&str] = &[];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new().route("/api/graph/audio/peaks", get(audio_peaks)).route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_AUDIO.contains(&req.uri().path())
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
struct PeaksReq {
    session: u64,
    asset: String,
}

/// The waveform peaks of asset `asset` of the open song (Q9), as `peaks.rs` lays them out:
/// `application/octet-stream`. Made on a blocking task, never on the session's actor, so a
/// compile never holds a waveform up. A read: a read-only window may ask.
async fn audio_peaks(State(s): S, Query(r): Query<PeaksReq>) -> Result<Response, ApiError> {
    let state = s.clone();
    let bytes = blocking(move || {
        let file = state.graph.asset_file(r.session, &r.asset)?;
        Ok(Store::global().of(&file)?.bytes())
    })
    .await?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream"), (header::CACHE_CONTROL, "no-store")], bytes).into_response())
}

#[cfg(test)]
#[path = "routes_audio_tests.rs"]
mod tests;
