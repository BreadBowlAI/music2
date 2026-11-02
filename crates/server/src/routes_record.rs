//! Lane B's routes, in the one route table (AGENT-8): Record (gaps #24, #25), on the armed tracks or
//! a notes lane or an audio lane, with Punch and Overdub (MON-9 to MON-12); arming a track and its
//! input's channels; and choosing a take (MON-11). Each but arming is a write, and the musician's
//! (AGENT-11). None waits for the session: a take starts and ends on the control plane, and the take
//! thread writes it (`graph/record.rs`). `POST /api/graph/input/inject` feeds the silent input, for
//! browser suites (D24).

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
        "Record a take on the armed tracks, or on a notes lane or an audio lane; punch on the loop, overdub notes",
        r#"{"session": 7, "module": "keys", "lane": "chords", "count_in": true, "punch": false, "overdub": false}"#;
    POST "/api/graph/record/stop" => record_stop; writes, musician,
        "End the take where it is heard; play goes on",
        r#"{"session": 7}"#;
    POST "/api/graph/record/cancel" => record_cancel; writes, musician,
        "Drop the take, or stop counting in",
        r#"{"session": 7}"#;
    POST "/api/graph/arm" => arm; reads, musician,
        "Arm a track to record, or disarm it, and choose the input channels it records",
        r#"{"session": 7, "module": "vox", "lane": "clips", "armed": true, "channels": "3-4"}"#;
    POST "/api/graph/take" => take; writes, agents,
        "Choose which take an audio clip over a loop plays (from 1)",
        r#"{"session": 7, "module": "vox", "lane": "clips", "id": "k3f9q2zt", "take": 2}"#;
    POST "/api/graph/input/inject" => input_inject; reads, musician,
        "What the sound input would hear next (only with M2_AUDIO_INPUT=inject, for the browser suites)",
        r#"{"tone": {"hz": 440, "seconds": 1}, "channel": 3}"#;
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
    /// Only between the loop's edges, replacing what was there (MON-10).
    #[serde(default)]
    punch: bool,
    /// A notes take into the clip under it (MON-12).
    #[serde(default)]
    overdub: bool,
}

fn yes() -> bool {
    true
}

/// `POST /api/graph/record/start {session, module, lane, count_in?, quantize?, punch?, overdub?}`: a
/// take on every armed track (MON-9), or with none on that notes lane (Q19 to Q21) or that audio
/// lane, from the sound input Settings chose, heard while it records when Settings says so (D13,
/// D21). With `punch`, only between the loop's edges (MON-10); with `overdub`, a notes take goes
/// into the clip under it (MON-12). Answers the take as the windows see it: `{phase: "count",
/// count_in: {beats, ms}, …}` while it counts in, `{phase: "take", lanes, …}` once it records.
/// Refused while an export runs (D26).
async fn record_start(State(s): S, Json(r): Json<StartReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(
        blocking(move || {
            if state.exports.running() {
                anyhow::bail!("Wait for the export to finish.");
            }
            let input = state.settings().map(|k| k.read().input).unwrap_or_default();
            let opts = Options {
                count_in: r.count_in,
                quantize: r.quantize,
                input: input.choice(),
                hear: input.monitor,
                punch: r.punch,
                overdub: r.overdub,
            };
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
struct ArmReq {
    session: u64,
    module: String,
    /// The lane it records on: its notes lane, else its audio lane (needed to arm).
    #[serde(default)]
    lane: Option<String>,
    #[serde(default)]
    armed: Option<bool>,
    /// The input's channels it records: `"1-2"`, `"3-4"`, or one, `"2"` (MON-13).
    #[serde(default)]
    channels: Option<String>,
}

/// `POST /api/graph/arm {session, module, lane?, armed?, channels?}`: arm track `module` on `lane`,
/// or disarm it, and choose the channels it records (MON-9, MON-13). No edit: no undo step, and it
/// works read-only. An armed audio track hears its input while Settings' Hear yourself is on.
/// Refused while a take records. Answers `{armed: {module: lane}, channels: {module: "3-4"}}`.
async fn arm(State(s): S, Json(r): Json<ArmReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || {
        let channels = match r.channels.as_deref() {
            Some(c) => Some(
                m2_play::Channels::parse(c)
                    .ok_or_else(|| anyhow::anyhow!("Channels are a pair, such as \"3-4\", or one, such as \"2\"."))?,
            ),
            None => None,
        };
        let input = state.settings().map(|k| k.read().input).unwrap_or_default();
        state.graph.arm(r.session, &r.module, r.lane.as_deref(), r.armed, channels, input.choice(), input.monitor)
    })
    .await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct TakeReq {
    session: u64,
    #[serde(flatten)]
    clip: m2_editor::clips::ClipRef,
    /// Which take, from 1.
    take: usize,
    /// The revision the pick goes from (AGENT-2): a clip changed since is refused.
    #[serde(default)]
    revision: Option<u64>,
}

/// `POST /api/graph/take {session, module, lane, id, take}`: the audio clip `id` plays take `take`
/// (from 1) of its file's takes over a loop (MON-11): its offset moves to the same place in that
/// pass. One Tx, one undo step ("Vox take 2"). Answers the edit.
async fn take(State(s): S, Json(r): Json<TakeReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || {
        let take = r.take.checked_sub(1).ok_or_else(|| anyhow::anyhow!("Takes count from 1."))?;
        state.graph.choose_take(r.session, r.clip, take, r.revision)
    })
    .await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct InjectReq {
    /// Mono samples at the song's rate.
    #[serde(default)]
    samples: Vec<f32>,
    /// Or a sine: its pitch, length and level (0.5 unless said).
    #[serde(default)]
    tone: Option<Tone>,
    /// Only on this channel (from 1); on every channel unless said.
    #[serde(default)]
    channel: Option<u16>,
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

/// `POST /api/graph/input/inject {samples?, tone?: {hz, seconds, gain?}, channel?, unplug?, refuse?}`:
/// what the silent input hears next, one sample for each frame the engine renders from the next
/// callback on (D24), on every channel or only on `channel`; with `unplug`, then its device goes
/// away, as a microphone unplugged; with `refuse`, the
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
        s.graph.inject_input(&r.samples, r.tone.map(|t| (t.hz, t.seconds, t.gain)), r.channel)?;
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
