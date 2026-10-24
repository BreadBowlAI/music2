//! What an agent sees beside the musician (AGENT-6; track 3 of the agents design): the focus
//! record the page posts, a pointer an agent posts, and closing the Compare pill (D2). None is a
//! write: they work in a read-only window too, and change no file, revision or undo step.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::{ApiError, blocking};

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/focus", get(focus_get).post(focus_post))
        .route("/api/point", post(point))
        .route("/api/graph/compare", post(compare))
        .with_state(state)
}

/// What the musician looks at, as the page last said, with where playback is, the loop, the
/// unsaved code, Compare and the MIDI target: null with no song open.
async fn focus_get(State(s): S) -> Json<Value> {
    Json(s.graph.focus())
}

/// The page's focus record (only the page key: `auth` refuses the tools key's). Every window and
/// agent hears it.
async fn focus_post(State(s): S, Json(f): Json<crate::graph::focus::Focus>) -> ApiResult {
    Ok(Json(s.graph.post_focus(f)?))
}

#[derive(Deserialize)]
struct PointReq {
    session: u64,
    /// `track:bass`, `clip:bass/notes:c3`, `lane:bass/cutoff`, `knob:<id>` or `bars:9-16`.
    #[serde(default)]
    target: Option<String>,
    /// What the agent says there: plain text, at most 80 characters (AGENT-10).
    #[serde(default)]
    words: String,
    /// The pointer goes: the page's OK, or `m2 point --clear`.
    #[serde(default)]
    clear: bool,
    /// An agent's name, as the body gives it (else the `x-m2-as` header's).
    #[serde(default, rename = "as")]
    by: Option<String>,
}

/// Show a pointer at a target, or clear it: one at a time.
async fn point(State(s): S, Json(r): Json<PointReq>) -> ApiResult {
    let caller = crate::caller::now().saying(r.by.as_deref(), None);
    let at = match (r.clear, r.target) {
        (true, _) => None,
        (false, Some(target)) => Some(target),
        (false, None) => return Err(ApiError::from(anyhow::anyhow!("say where to point: `target`, or `clear`"))),
    };
    let state = s.clone();
    let v =
        blocking(move || crate::caller::as_caller(caller, || state.graph.point(r.session, at.as_deref().map(|t| (t, r.words.as_str())))))
            .await?;
    Ok(Json(v))
}

#[derive(Deserialize)]
struct CompareReq {
    session: u64,
    /// Only `false`: the pill closes. It opens with V's undo (`compare: true`).
    open: bool,
}

/// Close the Compare pill (Esc, or any other action), keeping what plays.
async fn compare(State(s): S, Json(r): Json<CompareReq>) -> ApiResult {
    if r.open {
        return Err(ApiError::from(anyhow::anyhow!("Compare opens with V: an undo with `compare: true`")));
    }
    let state = s.clone();
    let v = blocking(move || state.graph.close_compare(r.session)).await?;
    Ok(Json(json!({"comparing": v})))
}

#[cfg(test)]
#[path = "routes_see_tests.rs"]
mod tests;
