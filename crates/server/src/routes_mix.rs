//! Lane A's routes (polish): change a track's instrument. In a router of its own, merged into the
//! API's, as `routes_8b.rs` is; a route here that writes the project is in [`WRITES_MIX`], and a
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
pub const WRITES_MIX: &[&str] = &["/api/graph/track/instrument"];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/track/instrument", post(track_instrument))
        .route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_MIX.contains(&req.uri().path())
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
struct InstrumentReq {
    session: u64,
    /// The track, by its key.
    module: String,
    /// The instrument it plays now on, by ComponentId (`std:synth-pad`), as the graph state's
    /// `instruments` list it.
    component: String,
}

/// Change a track's instrument (polish A3): `{session, module, component}`, one Tx and one undo
/// step. A write. Answers `{file, before, after, revision}`; a refusal says why in words.
async fn track_instrument(State(s): S, Json(r): Json<InstrumentReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.swap_instrument(r.session, &r.module, &r.component)).await;
    s.graph_changed();
    Ok(Json(result?))
}
