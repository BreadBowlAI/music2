//! Lane A's routes (polish), in the one route table (AGENT-8): change a track's instrument, and its
//! ID; and the bus routes (RENDER-34): add a bus, change a send, route a channel.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/track/instrument" => track_instrument; writes, agents,
        "Change a track's instrument, by its component id as the state's instruments list it",
        r#"{"session": 7, "module": "keys", "component": "@m2/synths:synth-pad"}"#;
    POST "/api/graph/track/rekey" => track_rekey; writes, agents,
        "Change a track's ID (its key)",
        r#"{"session": 7, "module": "keys", "key": "piano"}"#;
    POST "/api/graph/track/rekey/check" => track_rekey_check; reads, agents,
        "Whether a track's ID can be changed to this one",
        r#"{"session": 7, "module": "keys", "key": "piano"}"#;
    POST "/api/graph/mix/bus" => mix_bus; writes, agents,
        "Add a bus: reverb, delay or group",
        r#"{"session": 7, "kind": "reverb", "label": "Room"}"#;
    POST "/api/graph/mix/send" => mix_send; writes, agents,
        "Add or remove a send from a channel to a bus, or put it before or after the fader (pre)",
        r#"{"session": 7, "channel": "keys", "bus": "reverb", "action": "add"}"#;
    POST "/api/graph/mix/route" => mix_route; writes, agents,
        "Send a channel to a group bus, or back to the master (to: null)",
        r#"{"session": 7, "channel": "keys", "to": "group"}"#;
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

#[derive(Deserialize)]
struct InstrumentReq {
    session: u64,
    /// The track, by its key.
    module: String,
    /// The instrument it plays now on, by ComponentId (`@m2/synths:synth-pad`), as the graph state's
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

#[derive(Deserialize)]
struct RekeyReq {
    session: u64,
    /// The track, by its key now.
    module: String,
    /// Its new key: what the UI calls its ID.
    key: String,
}

/// Change a track's ID (polish A5, design-8b §11.1): `{session, module, key}`, one Tx and one undo
/// step. A write. Answers `{file, before, after, revision}`; a refusal says why in ID words.
async fn track_rekey(State(s): S, Json(r): Json<RekeyReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.rekey(r.session, &r.module, &r.key)).await;
    s.graph_changed();
    Ok(Json(result?))
}

/// Whether `key` can be the track's new ID, as the dialog checks while you type: `{ok}`, or `{ok:
/// false, error}`. Reads only, so a read-only window may ask.
async fn track_rekey_check(State(s): S, Json(r): Json<RekeyReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.rekey_check(r.session, &r.module, &r.key)).await?))
}

#[derive(Deserialize)]
struct BusReq {
    session: u64,
    /// `reverb`, `delay` or `group`.
    kind: String,
    /// Its name; else the kind's (`Reverb`).
    #[serde(default)]
    label: Option<String>,
    /// The track it goes after in the layout; else last.
    #[serde(default)]
    after: Option<String>,
}

/// Add a bus (the Mix view's Add bus menu, RENDER-34): `{session, kind, label?, after?}`, one Tx
/// and one undo step. A write. Answers the edit, `{module, label}` and the new Mix view, `mix`.
async fn mix_bus(State(s): S, Json(r): Json<BusReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.mix_bus(r.session, &r.kind, r.label, r.after)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct SendReq {
    session: u64,
    /// The channel that sends, by its key.
    channel: String,
    /// The bus channel it sends to, by its key.
    bus: String,
    /// `add`, `remove`, or `pre` (set it before the fader or after it).
    action: String,
    /// Before the fader: for `add` and `pre`.
    #[serde(default)]
    pre: bool,
}

/// Change a send (RENDER-34): `{session, channel, bus, action, pre?}`, one Tx and one undo step;
/// its level is a knob, which `/api/graph/set` and the leases write. A write. Answers the edit and
/// the new Mix view, `mix`; a send that would make a loop is refused in words.
async fn mix_send(State(s): S, Json(r): Json<SendReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.mix_send(r.session, &r.channel, &r.bus, &r.action, r.pre)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct RouteReq {
    session: u64,
    /// The channel, by its key.
    channel: String,
    /// The bus channel it goes to instead of the master, by its key; null for the master.
    #[serde(default)]
    to: Option<String>,
}

/// Send a channel to a group bus, or back to the master (RENDER-34): `{session, channel, to}`, one
/// Tx and one undo step. A write. Answers the edit and the new Mix view, `mix`; an output that
/// would make a loop is refused in words.
async fn mix_route(State(s): S, Json(r): Json<RouteReq>) -> ApiResult {
    let state = s.clone();
    let result = blocking(move || state.graph.mix_route(r.session, &r.channel, r.to)).await;
    s.graph_changed();
    Ok(Json(result?))
}
