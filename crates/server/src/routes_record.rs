//! Lane B's routes, in the one route table (AGENT-8): Record (gaps #24), on a notes lane or an audio
//! lane (the io plan's slice 3). Each is a write, and the musician's (AGENT-11). None waits for the
//! session: a take starts and ends on the control plane, and the take thread writes it
//! (`graph/record.rs`). `POST /api/graph/input/inject` feeds the silent input, for browser suites
//! (D24).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::graph::record::Options;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/record/start" => record_start; writes, musician,
        "Record a take on a notes lane or an audio lane",
        r#"{"session": 7, "module": "keys", "lane": "chords", "count_in": true}"#;
    POST "/api/graph/record/stop" => record_stop; writes, musician,
        "End the take where it is heard; play goes on",
        r#"{"session": 7}"#;
    POST "/api/graph/record/cancel" => record_cancel; writes, musician,
        "Drop the take, or stop counting in",
        r#"{"session": 7}"#;
    POST "/api/graph/input/inject" => input_inject; reads, musician,
        "What the sound input would hear next (only with M2_AUDIO_INPUT=inject, for the browser suites)",
        r#"{"tone": {"hz": 440, "seconds": 1}}"#;
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

#[derive(Deserialize)]
struct StartReq {
    session: u64,
    module: String,
    lane: String,
    /// Count in one bar first, when stopped (on unless it says not).
    #[serde(default = "yes")]
    count_in: bool,
    /// Notes' grid in beats, to snap the take's starts to; none: as played.
    #[serde(default)]
    quantize: Option<f64>,
}

fn yes() -> bool {
    true
}

/// `POST /api/graph/record/start {session, module, lane, count_in?, quantize?}`: a take on that
/// notes lane (Q19 to Q21), or on that audio lane from the sound input Settings chose, heard while
/// it records when Settings says so (D13, D21). Answers the take as the windows see it: `{phase:
/// "count", count_in: {beats, ms}, …}` while it counts in, `{phase: "take", …}` once it records.
/// Refused while an export runs (D26).
async fn record_start(State(s): S, Json(r): Json<StartReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(
        blocking(move || {
            if state.exports.running() {
                anyhow::bail!("Wait for the export to finish.");
            }
            let input = state.settings().map(|k| k.read().input).unwrap_or_default();
            let opts = Options { count_in: r.count_in, quantize: r.quantize, input: input.choice(), hear: input.monitor };
            state.graph.record_start(r.session, &r.module, &r.lane, opts)
        })
        .await?,
    ))
}

#[derive(Deserialize)]
struct SessionReq {
    session: u64,
}

/// `POST /api/graph/record/stop {session}`: the take ends where it is heard, and play goes on.
/// Answers at once; `{"t": "take", "written": …}` (or `empty`) follows.
async fn record_stop(State(s): S, Json(r): Json<SessionReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.record_stop(r.session)).await?))
}

/// `POST /api/graph/record/cancel {session}`: counting in, nothing plays; recording, the take is
/// dropped. Nothing is written.
async fn record_cancel(State(s): S, Json(r): Json<SessionReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.record_cancel(r.session)).await?))
}

#[derive(Deserialize)]
struct InjectReq {
    /// Mono samples at the song's rate.
    #[serde(default)]
    samples: Vec<f32>,
    /// Or a sine: its pitch, length and level (0.5 unless said).
    #[serde(default)]
    tone: Option<Tone>,
    /// Then the input's device goes away, as a microphone unplugged (the review's io-r2).
    #[serde(default)]
    unplug: bool,
    /// Then the OS refuses the microphone, as a Mac's Don't Allow does (the ex plan's D11).
    #[serde(default)]
    refuse: bool,
}

#[derive(Deserialize)]
struct Tone {
    hz: f32,
    seconds: f32,
    #[serde(default = "half")]
    gain: f32,
}

fn half() -> f32 {
    0.5
}

/// `POST /api/graph/input/inject {samples?, tone?: {hz, seconds, gain?}, unplug?, refuse?}`: what
/// the silent input hears next, one sample for each frame the engine renders from the next callback
/// on (D24); with `unplug`, then its device goes away, as a microphone unplugged; with `refuse`, the
/// OS refuses the microphone from then, as a Mac's Don't Allow. Only
/// with `M2_AUDIO_INPUT=inject` (and in this crate's tests), as `M2_MIDI=inject` opens MIDI's.
/// Not a write: it plays what a microphone would.
async fn input_inject(State(s): S, Json(r): Json<InjectReq>) -> Result<Response, ApiError> {
    if !crate::input_injects() {
        return Ok(
            (StatusCode::NOT_FOUND, Json(json!({"error": "no such API here: it answers only with M2_AUDIO_INPUT=inject"}))).into_response()
        );
    }
    blocking(move || {
        s.graph.inject_input(&r.samples, r.tone.map(|t| (t.hz, t.seconds, t.gain)))?;
        if r.unplug {
            s.graph.unplug_input()?;
        }
        if r.refuse { s.graph.refuse_input() } else { Ok(()) }
    })
    .await?;
    Ok(Json(json!({})).into_response())
}

#[cfg(test)]
#[path = "routes_record_tests.rs"]
mod tests;
