//! Lane C's routes, in the one route table (AGENT-8): this machine's settings (gaps #31) and the
//! sound inputs there are (the io plan's D13), and undo's names (gaps #29). Settings aren't a
//! project write: no session, no undo; they work with no song open and in a read-only window.
//! Naming an undo step writes nothing either.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::routes::ApiError;
use crate::settings::{InputKept, MidiKept, OutputKept, RecordKept};
use crate::{AppState, project};

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    GET "/api/settings" => settings_get; reads, agents,
        "This computer's settings: the sound output, where new songs go, Record's options, the MIDI keyboards and the sound input",
        r#"{}"#;
    POST "/api/settings" => settings_set; reads, musician,
        "Change this computer's settings",
        r#"{"record": {"count_in": true, "quantize": false}}"#;
    GET "/api/audio/inputs" => audio_inputs; reads, agents,
        "The sound inputs there are, the default first",
        r#"{}"#;
    POST "/api/graph/step" => graph_step; reads, musician,
        "Name the undo step the window's own edit made",
        r#"{"session": 7, "revision": 42, "name": "Keys brightness"}"#;
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

/// The settings as the window shows them: what is chosen (an output by its id, and its name: as
/// listed, or as kept while it isn't connected), the outputs there are (the default first), what
/// plays now, where new songs go and the default for that, the home folder (the window writes paths
/// in it as `~/…`), whether an export runs (the output can't change then), and the MIDI keyboards
/// plugged in and turned off (with no song open too: the review's F3), and the sound input a take
/// records with whether you hear yourself (`input`, the io plan's D13 and D21; the inputs there are
/// come from `GET /api/audio/inputs`). `stopped`: the change just made stopped playback.
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
    let (record, midi, input) = kept.map(|k| (k.record, k.midi, k.input)).unwrap_or_default();
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
        "input": {"device": input.device, "name": input.name, "monitor": input.monitor},
    })
}

/// `GET /api/audio/inputs`: `{inputs: [{id, name, default, rate, channels}]}`, the default first
/// (D13). Opening an input at a take is what makes a Mac ask for the microphone, not this list (to
/// be checked on the Mac).
async fn audio_inputs() -> ApiResult {
    let inputs = blocking(|| Ok(m2_play::inputs())).await?;
    let inputs: Vec<Value> =
        inputs.iter().map(|i| json!({"id": i.id, "name": i.name, "default": i.default, "rate": i.rate, "channels": i.channels})).collect();
    Ok(Json(json!({"inputs": inputs})))
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
    /// The sound input (by its id, null: the output's own device) and Hear yourself (io).
    #[serde(default)]
    input: Option<InputKept>,
}

/// `POST /api/settings {output?: {device, frames}, songs?, record?: {count_in, quantize}, midi?: {off},
/// input?: {device, monitor}}` (`device`: an output's or an input's id): applied at once and kept in
/// `~/.music2/settings.json`. The next take opens the input chosen (D14). A new output makes a new player where the song was, stopped. A songs
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
        // io: the input a take opens next, by its name as listed now (one not connected keeps its
        // name), and Hear yourself.
        if let (Some(mut input), Some(k)) = (r.input, state.settings()) {
            let listed = input.device.as_ref().and_then(|id| m2_play::inputs().into_iter().find(|i| i.id == *id));
            input.name = match (&input.device, listed) {
                (None, _) => None,
                (Some(_), Some(i)) => Some(i.name),
                (Some(_), None) => input.name,
            };
            k.change(|kept| kept.input = input)?;
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
