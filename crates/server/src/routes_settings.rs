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
use crate::settings::{MidiKept, OutputKept, RecordKept};
use crate::{AppState, project};

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

pub fn router(_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new().route("/api/settings", get(settings_get).post(settings_set)).route("/api/graph/step", post(graph_step))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

/// The settings as the window shows them: what is chosen (an output by its id, and its name: as
/// listed, or as kept while it isn't connected), the outputs there are (the default first), what
/// plays now, where new songs go and the default for that, the home folder (the window writes paths
/// in it as `~/…`), whether an export runs (the output can't change then), and the MIDI keyboards
/// plugged in and turned off (with no song open too: the review's F3). `stopped`: the change just
/// made stopped playback.
fn shown(s: &AppState, stopped: bool) -> Value {
    let choice = s.graph.output_choice();
    let listed = m2_play::outputs();
    let outputs: Vec<Value> = listed
        .iter()
        .map(|o| json!({"id": o.id, "name": o.name, "default": o.default, "rate": o.rate, "frames": o.frames.map(|(min, max)| [min, max])}))
        .collect();
    let kept = s.settings().map(|k| k.read());
    let name = choice.device.as_ref().and_then(|id| {
        let now = listed.iter().find(|o| o.id == *id).map(|o| o.name.clone());
        now.or_else(|| kept.as_ref().filter(|k| k.output.device.as_ref() == Some(id)).and_then(|k| k.output.name.clone()))
    });
    let chosen = kept.as_ref().is_some_and(|k| k.songs.is_some());
    // Lane B: Record's options, and the MIDI keyboards turned off (the defaults where none are kept).
    let (record, midi) = kept.map(|k| (k.record, k.midi)).unwrap_or_default();
    json!({
        "output": {"device": choice.device, "name": name, "frames": choice.frames},
        "outputs": outputs,
        "now": s.graph.output_now(),
        "songs": project::songs_dir().ok(),
        "songs_chosen": chosen,
        "songs_default": project::default_songs_dir().ok(),
        "home": project::home(),
        "exporting": s.exports.running(),
        "stopped": stopped,
        "record": {"count_in": record.count_in, "quantize": record.quantize},
        "midi": {"off": midi.off, "devices": s.graph.midi_state()["devices"]},
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
    /// The output (by its id) and its buffer size: null in either is the system's default.
    #[serde(default)]
    output: Option<OutputKept>,
    /// Where new songs go: a full path, or null for the default.
    #[serde(default, deserialize_with = "given")]
    songs: Option<Option<PathBuf>>,
    /// Record's options (lane B): both, as the window shows them.
    #[serde(default)]
    record: Option<RecordKept>,
    /// The MIDI keyboards turned off, by name (lane B).
    #[serde(default)]
    midi: Option<MidiKept>,
}

/// `POST /api/settings {output?: {device, frames}, songs?, record?: {count_in, quantize}, midi?: {off}}` (`device`: an output's id): applied at once and kept in
/// `~/.music2/settings.json`. A new output makes a new player where the song was, stopped. A songs
/// folder must be a full path outside any song. Answers as `GET` does.
async fn settings_set(State(s): S, Json(r): Json<SettingsReq>) -> ApiResult {
    let r_midi = r.midi.is_some();
    let state = s.clone();
    let answer = blocking(move || {
        let mut stopped = false;
        if let Some(mut output) = r.output {
            if state.exports.running() {
                anyhow::bail!("Wait for the export to finish.");
            }
            // Its name as listed now; one not connected keeps the name it was given.
            let listed = output.device.as_ref().and_then(|id| m2_play::outputs().into_iter().find(|o| o.id == *id));
            output.name = match (&output.device, listed) {
                (None, _) => None,
                (Some(_), Some(o)) => Some(o.name),
                (Some(_), None) => output.name,
            };
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
        // Lane B: kept for the page, which sends them with Record; and which keyboards play.
        if let (Some(record), Some(k)) = (r.record, state.settings()) {
            k.change(|kept| kept.record = record)?;
        }
        if let Some(midi) = r.midi.clone() {
            state.graph.set_midi_off(&midi.off);
            if let Some(k) = state.settings() {
                k.change(|kept| kept.midi = midi)?;
            }
        }
        Ok(shown(&state, stopped))
    })
    .await;
    if r_midi {
        s.midi_changed();
    }
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
