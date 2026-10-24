//! What an agent sees beside the musician (AGENT-6; track 3 of the agents design), in the one route
//! table (AGENT-8): the focus record the page posts, a pointer an agent posts, and the Compare pill
//! (D2). None is a write: they work in a read-only window too, and change no file, revision or undo
//! step, but Esc on Compare's Before, which undoes the step it compares.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::routes::{ApiError, blocking};

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    GET "/api/focus" => focus_get; reads, agents,
        "What the musician looks at, as m2 focus prints it: the view, the selected track, clips and notes, the knob last touched, the bars shown, where playback is",
        r#"{}"#;
    POST "/api/focus" => focus_post; reads, musician,
        "Say what the musician looks at (the page posts it)",
        r#"{"session": 7, "revision": 42, "view": "notes", "track": "keys"}"#;
    POST "/api/point" => point; reads, agents,
        "Point at a track, a clip, a lane, a knob or bars, with words, as m2 point does; or clear the pointer",
        r#"{"session": 7, "target": "bars:5-8", "words": "listen here"}"#;
    POST "/api/graph/compare" => compare; reads, musician,
        "Hear the newest change made elsewhere Before or After it (V), or close the pill",
        r#"{"session": 7, "step": 57, "before": true, "client": "w1"}"#;
}

/// What the musician looks at, as the page last said, with where playback is, the loop, the
/// unsaved code, Compare and the MIDI target: null with no song open.
async fn focus_get(State(s): S) -> Json<Value> {
    Json(s.graph.focus())
}

/// The page's focus record (only the page key: the tools key gets 403, AGENT-11). Every window and
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
    /// The step V compares: with `before`, play it Before (true) or After (false).
    #[serde(default)]
    step: Option<u64>,
    #[serde(default)]
    before: Option<bool>,
    /// `false`: the pill closes, at After; with `keep` (Esc), keeping what plays.
    #[serde(default)]
    open: Option<bool>,
    #[serde(default)]
    keep: bool,
    /// The window's id, as its socket gives it (`?client=`): that socket closing ends what it opened.
    #[serde(default)]
    client: Option<String>,
}

/// V (D2): play the newest change made elsewhere Before or After, or close the pill.
async fn compare(State(s): S, Json(r): Json<CompareReq>) -> Response {
    let state = s.clone();
    let result = crate::caller::blocking(move || match (r.open, r.step, r.before) {
        (Some(false), ..) => state.graph.close_compare(r.session, r.keep),
        (_, Some(step), Some(before)) => state.graph.compare(r.session, step, before, r.client),
        _ => Err(anyhow::anyhow!("Compare names its step and the side to play: `step` and `before`, or `open: false`")),
    })
    .await;
    s.graph_changed();
    crate::routes::step_answer(result)
}

#[cfg(test)]
#[path = "routes_see_tests.rs"]
mod tests;
