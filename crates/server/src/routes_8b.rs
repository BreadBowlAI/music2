//! The routes of 8b's back ends, in the one route table (AGENT-8): mute and solo, clip gestures,
//! and audition, also read off a page's WebSocket ([`ws_text`]).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::graph::audition::Audition;
use crate::graph::clips::Answer;
use crate::graph::overlay::Change;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/overlay" => graph_overlay; reads, musician,
        "Mute or solo tracks while listening, which never changes the song",
        r#"{"session": 7, "solo": {"keys": true}}"#;
    POST "/api/graph/track/mute" => track_mute; writes, agents,
        "Mute tracks in the song, written in their code, or unmute them",
        r#"{"session": 7, "modules": ["keys"], "muted": true}"#;
    POST "/api/graph/clips" => graph_clips; writes, agents,
        "A clip gesture: create, move, resize, split, join, duplicate, paste, delete, quantize, notes, points, audio or set",
        r#"{"session": 7, "gesture": {"op": "move", "clips": [{"module": "keys", "lane": "chords", "id": "k3f9q2zt"}], "by": 4}}"#;
    POST "/api/graph/automate/remove" => automate_remove; writes, agents,
        "Take a knob's automation out: the lane's value at the beat (else its fallback) goes back into the code",
        r#"{"session": 7, "module": "bass", "lane": "cutoff", "beat": 16}"#;
    POST "/api/graph/audition" => graph_audition; reads, musician,
        "Play or let go of notes on a lane, as the piano roll and the keyboard do",
        r#"{"client": "w1", "module": "keys", "lane": "chords", "on": [{"h": 1, "pitch": 60}]}"#;
    POST "/api/graph/midi/inject" => midi_inject; reads, musician,
        "What a MIDI keyboard would send (only with M2_MIDI=inject, for the browser suites)",
        r#"{"device": "Keys", "plugged": true, "bytes": [144, 60, 100]}"#;
    POST "/api/graph/plugin/window" => plugin_window; reads, musician,
        "Open or close a plugin's own window",
        r#"{"session": 7, "node": "synth", "open": true}"#;
    POST "/api/graph/plugin/latch" => plugin_latch; reads, musician,
        "Let a lane or a cable drive a plugin's parameter again",
        r#"{"session": 7, "node": "synth"}"#;
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

#[derive(Deserialize)]
struct OverlayReq {
    session: u64,
    #[serde(flatten)]
    change: Change,
}

/// Mute or solo tracks while listening (MON-1 to MON-4): `{session, mute?: {k: bool}, solo?: {k:
/// bool}, exclusive?: k, clear?: true}`. Not a write: it works read-only, in memory. Answers with
/// the state's `overlays`; every window hears a `graph` event.
async fn graph_overlay(State(s): S, Json(r): Json<OverlayReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.overlay(r.session, r.change)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct WindowReq {
    session: u64,
    /// The plugin node, by its name in the graph (`plugin` of its view node, the key of the graph
    /// state's `plugins`).
    node: String,
    open: bool,
}

/// Open or close a plugin's own window (HOST-7): `{session, node, open}`. Not a write: it works
/// read-only too. Answers `{window: "open" | "closed"}`, or why it can't open, in plain words.
async fn plugin_window(State(s): S, Json(r): Json<WindowReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.plugin_window(r.session, &r.node, r.open)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct LatchReq {
    session: u64,
    node: String,
    /// The latched parameter's CLAP id; all of the node's without one.
    id: Option<u32>,
}

/// "Re-enable" (MON-6): `{session, node, id?}`. The lane or cable drives the latched parameter
/// again. Not a write: a latch lasts for the session only.
async fn plugin_latch(State(s): S, Json(r): Json<LatchReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.plugin_reenable(r.session, &r.node, r.id)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct MuteReq {
    session: u64,
    modules: Vec<String>,
    muted: bool,
}

/// Mute tracks in the song, or unmute them (MON-5): `{session, modules: [key], muted}`, one Tx and
/// one undo step. A write. Answers `{file, before, after, revision}`.
async fn track_mute(State(s): S, Json(r): Json<MuteReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.mute_in_song(r.session, r.modules, r.muted)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct ClipsReq {
    session: u64,
    gesture: m2_editor::clips::Gesture,
}

/// A clip gesture (8b.5): `{session, gesture: {op, …}}`, one Tx and one undo step. A write.
/// Answers `{file, before, after, revision, made: [{module, lane, id}]}`; 409 `{error, changed:
/// true}` when a clip it names by its `sha` changed since the window drew it (WRITE-12).
async fn graph_clips(State(s): S, Json(r): Json<ClipsReq>) -> Result<Response, ApiError> {
    let state = s.clone();
    let result = blocking(move || state.graph.clips(r.session, r.gesture)).await;
    s.graph_changed();
    Ok(match result? {
        Answer::Done(v) => Json(v).into_response(),
        Answer::Changed(why) => (StatusCode::CONFLICT, Json(json!({"error": why, "changed": true}))).into_response(),
    })
}

#[derive(Deserialize)]
struct UnautomateReq {
    session: u64,
    module: String,
    lane: String,
    /// The playhead: the lane's value there is written. None: its fallback.
    #[serde(default)]
    beat: Option<f64>,
}

/// Remove automation (HOME-2): `{session, module, lane, beat?}`. A write. Answers `{file, before,
/// after, revision}`.
async fn automate_remove(State(s): S, Json(r): Json<UnautomateReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.remove_automation(r.session, r.module, r.lane, r.beat)).await;
    s.graph_changed();
    Ok(Json(result?))
}

/// Notes clips as the timeline shows them, each with its `sha` (8b.5), read as clips: what the
/// piano roll's whole-lane write sends back until it sends `notes` gestures (Q9).
pub(crate) fn shown_clips<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<m2_song::NoteClip>, D::Error> {
    let mut clips = Vec::<Value>::deserialize(d)?;
    for c in &mut clips {
        if let Some(c) = c.as_object_mut() {
            c.remove("sha");
        }
    }
    clips.into_iter().map(|c| serde_json::from_value(c).map_err(serde::de::Error::custom)).collect()
}

#[derive(Deserialize)]
struct AuditionReq {
    /// The window, or the agent, playing: its notes are its own.
    client: String,
    #[serde(flatten)]
    audition: Audition,
}

/// Play or let go of notes on a lane (MON-7): `{client, module, lane, on: [{h, pitch, velocity}],
/// off: [h], keep: [h]}`, as a window sends on its WebSocket. Not a write: it works read-only, and
/// never waits for the session. Answers `{}`, or `{"t": "audition", "silent": …}` when nothing
/// plays the lane.
async fn graph_audition(State(s): S, Json(r): Json<AuditionReq>) -> ApiResult {
    let answer = blocking(move || s.graph.audition(&r.client, r.audition)).await?;
    Ok(Json(answer.unwrap_or_else(|| json!({}))))
}

#[derive(Deserialize)]
struct InjectReq {
    /// The keyboard's name.
    device: String,
    /// Plugged in (true) or out (false).
    #[serde(default)]
    plugged: Option<bool>,
    /// One MIDI message: a status byte and its data bytes.
    #[serde(default)]
    bytes: Option<Vec<u8>>,
}

/// What a MIDI keyboard would send, posted (8b.10): `{device, plugged?, bytes?}`, heard as the
/// listener hears it (plugged first, then the message). Only with `M2_MIDI=inject`, the suites' way
/// in where no keyboard can be opened; otherwise there is no such API (404).
async fn midi_inject(State(s): S, Json(r): Json<InjectReq>) -> Result<Response, ApiError> {
    if !crate::midi_injects() {
        return Ok((StatusCode::NOT_FOUND, Json(json!({"error": "no such API here: it answers only with M2_MIDI=inject"}))).into_response());
    }
    blocking(move || {
        use m2_os::midi::Event;
        match r.plugged {
            Some(true) => s.midi(Event::Plugged(&r.device)),
            Some(false) => s.midi(Event::Unplugged(&r.device)),
            None => {}
        }
        if let Some(b) = &r.bytes {
            s.midi(Event::Bytes(&r.device, b));
        }
        Ok(())
    })
    .await?;
    Ok(Json(json!({})).into_response())
}

/// A message a window sent on its WebSocket: `{"t": "audition", …}` from the window `client` plays
/// notes as [`graph_audition`] does. What it answers goes to that window only, as text; an error
/// is `{"t": "audition", "error": …}`. `{"t": "midi", "module", "lane"}` says what MIDI keyboards
/// play (`module: null`: nothing), and every window hears it. Other messages are ignored.
pub(crate) async fn ws_text(state: &Arc<AppState>, client: Option<&str>, text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    if v["t"] == "midi" {
        let target = match (v["module"].as_str(), v["lane"].as_str()) {
            (Some(m), Some(l)) => Some((m.to_string(), l.to_string())),
            _ => None,
        };
        let st = state.clone();
        if let Ok(Ok(())) = crate::caller::blocking(move || st.graph.midi_target(target)).await {
            state.midi_changed();
        }
        return None;
    }
    if v["t"] != "audition" {
        return None;
    }
    let said = |e: String| Some(json!({"t": "audition", "error": e}).to_string());
    let Some(client) = client.map(str::to_string) else {
        return said("This window has no client id, which audition needs.".into());
    };
    let a: Audition = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return said(format!("This audition message doesn't read: {e}")),
    };
    let state = state.clone();
    match crate::caller::blocking(move || state.graph.audition(&client, a)).await {
        Ok(Ok(answer)) => answer.map(|v| v.to_string()),
        Ok(Err(e)) => said(format!("{e:#}")),
        Err(e) => said(e.to_string()),
    }
}

#[cfg(test)]
#[path = "routes_8b_tests.rs"]
pub(crate) mod tests;
