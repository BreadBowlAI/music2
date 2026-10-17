//! Lane C's routes, in a router of their own, merged into the API's: this machine's settings (gaps
//! #31), and undo's names (gaps #29). Settings aren't a project write: no session, no undo; they
//! work with no song open and in a read-only window. Naming an undo step writes nothing either.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::routes::ApiError;
use crate::settings::OutputKept;
use crate::{AppState, project};

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

pub fn router(_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new().route("/api/settings", get(settings_get).post(settings_set)).route("/api/graph/step", post(graph_step))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

/// The settings as the window shows them: what is chosen, the outputs there are (the default
/// first), what plays now, where new songs go and the default for that, the home folder (the
/// window writes paths in it as `~/…`), and whether an export runs (the output can't change then).
/// `stopped`: the change just made stopped playback.
fn shown(s: &AppState, stopped: bool) -> Value {
    let choice = s.graph.output_choice();
    let outputs: Vec<Value> = m2_play::outputs()
        .into_iter()
        .map(|o| json!({"name": o.name, "default": o.default, "rate": o.rate, "frames": o.frames.map(|(min, max)| [min, max])}))
        .collect();
    let chosen = s.settings().and_then(|k| k.read().songs).is_some();
    json!({
        "output": {"device": choice.device, "frames": choice.frames},
        "outputs": outputs,
        "now": s.graph.output_now(),
        "songs": project::songs_dir().ok(),
        "songs_chosen": chosen,
        "songs_default": project::default_songs_dir().ok(),
        "home": m2_os::dirs::home(),
        "exporting": s.exports.running(),
        "stopped": stopped,
    })
}

/// `GET /api/settings` (see [`shown`]).
async fn settings_get(State(s): S) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || Ok(shown(&state, false))).await?))
}

/// A field given as null is `Some(None)`; one left out is `None`.
fn given<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct SettingsReq {
    /// The output and its buffer size: null in either is the system's default.
    #[serde(default)]
    output: Option<OutputKept>,
    /// Where new songs go: a full path, or null for the default.
    #[serde(default, deserialize_with = "given")]
    songs: Option<Option<PathBuf>>,
}

/// `POST /api/settings {output?: {device, frames}, songs?}`: applied at once and kept in
/// `~/.music2/settings.json`. A new output makes a new player where the song was, stopped. A songs
/// folder must be a full path outside any song. Answers as `GET` does.
async fn settings_set(State(s): S, Json(r): Json<SettingsReq>) -> ApiResult {
    let state = s.clone();
    let answer = blocking(move || {
        let mut stopped = false;
        if let Some(output) = r.output {
            if state.exports.running() {
                anyhow::bail!("Wait for the export to finish.");
            }
            stopped = state.graph.set_output(output.choice())?;
            if let Some(k) = state.settings() {
                k.change(|kept| kept.output = output)?;
            }
        }
        if let Some(songs) = r.songs {
            let songs = songs.map(|d| project::songs_folder(&d)).transpose()?;
            project::set_songs_dir(songs.clone());
            if let Some(k) = state.settings() {
                k.change(|kept| kept.songs = songs)?;
            }
        }
        Ok(shown(&state, stopped))
    })
    .await;
    // A new player: the windows fetch the status again.
    s.graph_changed();
    Ok(Json(answer?))
}

#[derive(Deserialize)]
struct StepReq {
    session: u64,
    revision: u64,
    name: String,
    #[serde(default)]
    structural: bool,
}

/// `POST /api/graph/step {session, revision, name, structural}`: name the undo step of the edit this
/// window made as `revision` ("Hats pan"; gaps #29). Answers `{named, revision, undo_name, redo_name}`.
async fn graph_step(State(s): S, Json(r): Json<StepReq>) -> ApiResult {
    let state = s.clone();
    Ok(Json(blocking(move || state.graph.name_step(r.session, r.revision, r.name, r.structural)).await?))
}
