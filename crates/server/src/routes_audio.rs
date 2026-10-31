//! Audio in the app (lane A), in the one route table (AGENT-8): an audio file's waveform peaks
//! (Q9), and bringing audio files in (A2).

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use m2_editor::import::{Source, Target};
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::peaks::Store;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;

crate::api::routes! {
    GET "/api/graph/audio/peaks" => audio_peaks; reads, agents,
        "An audio file's waveform peaks, as bytes",
        r#"{"session": 7, "asset": "vox.wav"}"#;
    POST "/api/graph/audio/import" => audio_import up to UPLOAD; writes, agents,
        "Bring audio files on this computer in, and place their clips: on a lane, on a new track after one, or on new tracks below",
        r#"{"session": 7, "paths": ["/Users/you/Samples/vox.wav"], "to": {"kind": "below", "beat": 0}}"#;
    POST "/api/graph/audio/track" => audio_track; writes, agents,
        "Add an audio track",
        r#"{"session": 7, "label": "Vox", "after": "keys"}"#;
    POST "/api/graph/sample" => sample up to UPLOAD; writes, agents,
        "Give a recording on a card (a Sampler's file, or `setting` of the call) a WAV file on this computer",
        r#"{"session": 7, "at": 42, "call": "tracks/drums.ts#2", "file": "/Users/you/Samples/kick.wav"}"#;
}

/// The most one upload may be, all its files together: 1 GiB. A 15-minute stereo file of 32 bits at
/// 96 kHz is 691 MB; a longer, wider or faster one, or several together, can be more, and the page
/// says so in words before it sends them (the review's R9). The app sends paths: no limit.
const UPLOAD: usize = 1 << 30;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

#[derive(Deserialize)]
struct PeaksReq {
    session: u64,
    asset: String,
}

/// The waveform peaks of asset `asset` of the open song (Q9), as `peaks.rs` lays them out:
/// `application/octet-stream`. Made on a blocking task, never on the session's actor, so a
/// compile never holds a waveform up. A read: a read-only window may ask.
async fn audio_peaks(State(s): S, Query(r): Query<PeaksReq>) -> Result<Response, ApiError> {
    let state = s.clone();
    let bytes = blocking(move || {
        let file = state.graph.asset_file(r.session, &r.asset)?;
        Ok(Store::global().of(&file)?.bytes())
    })
    .await?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream"), (header::CACHE_CONTROL, "no-store")], bytes).into_response())
}

#[derive(Deserialize)]
struct ImportJson {
    session: u64,
    paths: Vec<String>,
    to: Target,
}

#[derive(Deserialize)]
struct ImportQuery {
    session: Option<u64>,
    /// The files in the body, in order, as JSON: `[{name, size}]`.
    files: Option<String>,
    /// The target, as JSON.
    to: Option<String>,
}

#[derive(Deserialize)]
struct Uploaded {
    name: String,
    size: usize,
}

/// Bring audio files in (A2, Q6, Q7), as one undo step: JSON `{session, paths, to}` (the app:
/// files on this computer, copied by the server), or the files' bytes back to back as the body with
/// `?session=&files=&to=` (a browser's upload; `files` as JSON, `[{name, size}]` in the body's
/// order; `to` as JSON). `to` is `{kind: "lane", module, lane, beat}`, `{kind: "track", module,
/// beat}` or `{kind: "below", beat}`. A write. Answers the edit with `made`, `tracks`, `name` (the
/// undo step's), `notes`, `assets` and `reused`. An agent's copies nothing while the musician holds
/// the song (AGENT-3): its files come into `assets/` only inside the change that lands.
async fn audio_import(State(s): S, Query(q): Query<ImportQuery>, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
    let json = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("application/json"));
    let (session, sources, to) = match json {
        true => {
            let r: ImportJson = serde_json::from_slice(&body).map_err(anyhow::Error::from)?;
            (r.session, r.paths.into_iter().map(|p| Source::Path(p.into())).collect(), r.to)
        }
        false => {
            let session = q.session.ok_or_else(|| anyhow::anyhow!("an upload names its session"))?;
            let files: Vec<Uploaded> =
                serde_json::from_str(q.files.as_deref().unwrap_or("")).map_err(|e| anyhow::anyhow!("an upload names its files: {e}"))?;
            let to: Target =
                serde_json::from_str(q.to.as_deref().unwrap_or("")).map_err(|e| anyhow::anyhow!("where the files go doesn't read: {e}"))?;
            let total = files.iter().try_fold(0usize, |sum, f| sum.checked_add(f.size));
            if total != Some(body.len()) {
                return Err(anyhow::anyhow!("the upload was cut short: try again").into());
            }
            let sources = match <[Uploaded; 1]>::try_from(files) {
                // One file, the usual upload: its bytes as they came.
                Ok([one]) => vec![Source::Bytes { name: one.name, bytes: Vec::from(body) }],
                Err(files) => {
                    let mut at = 0;
                    files
                        .into_iter()
                        .map(|f| {
                            at += f.size;
                            Source::Bytes { name: f.name, bytes: body[at - f.size..at].to_vec() }
                        })
                        .collect()
                }
            };
            (session, sources, to)
        }
    };
    let state = s.clone();
    let result = blocking(move || state.graph.import_audio(session, sources, to)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct TrackReq {
    session: u64,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    after: Option<String>,
}

/// The + Track list's Audio track (Q8): `{session, label?, after?}`. A write. Answers `{module, label, file,
/// before, after, revision}`; the step is named "adding Audio".
async fn audio_track(State(s): S, Json(r): Json<TrackReq>) -> Result<Json<Value>, ApiError> {
    let state = s.clone();
    let result = blocking(move || state.graph.add_audio_track(r.session, r.label, r.after)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[derive(Deserialize)]
struct SampleJson {
    session: u64,
    at: u64,
    call: String,
    /// The recording it becomes, by its setting's name (CARD-5): a Sampler's `sample` when left out.
    #[serde(default)]
    setting: Option<String>,
    /// The WAV's path on this computer.
    file: String,
}

#[derive(Deserialize)]
struct SampleQuery {
    session: Option<u64>,
    at: Option<u64>,
    call: Option<String>,
    setting: Option<String>,
    /// The file's own name: its bytes are the body.
    name: Option<String>,
}

/// A WAV dropped on a recording (CARD-5; track ab's Sampler): JSON `{session, at, call, setting?,
/// file}` (the app: a file on this computer, copied by the server), or the file's bytes as the body
/// with `?session=&at=&call=&setting=&name=` (a browser's upload). The file comes into `assets/`
/// (create-only), then one Tx writes `<setting>: "<name>"` into call `call` (a Sampler's `sample`
/// when `setting` is left out). A write. Answers the edit with
/// `name` (the undo step's, "setting Sampler to kick.wav") and `asset`.
async fn sample(State(s): S, Query(q): Query<SampleQuery>, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
    let json = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("application/json"));
    let (session, at, call, setting, source) = match json {
        true => {
            let r: SampleJson = serde_json::from_slice(&body).map_err(anyhow::Error::from)?;
            (r.session, r.at, r.call, r.setting, Source::Path(r.file.into()))
        }
        false => {
            let missing = |what: &str| anyhow::anyhow!("an upload to a sampler names its {what}");
            let (session, at) = (q.session.ok_or_else(|| missing("session"))?, q.at.ok_or_else(|| missing("revision"))?);
            let (call, name) = (q.call.ok_or_else(|| missing("call"))?, q.name.ok_or_else(|| missing("file"))?);
            (session, at, call, q.setting, Source::Bytes { name, bytes: Vec::from(body) })
        }
    };
    let state = s.clone();
    let result = blocking(move || state.graph.set_sample(session, at, call, setting, source)).await;
    s.graph_changed();
    Ok(Json(result?))
}

#[cfg(test)]
#[path = "routes_audio_tests.rs"]
pub(crate) mod tests;
