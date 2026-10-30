//! The library (LIB-1 to LIB-7): the one search, previews, your sample folders, and adding a
//! sample.
//!
//! - `GET /api/library?q=<words>[&fits=<beat>]`: `{items: [...], folders: {indexing, not_wav}}`,
//!   the items the search finds, best first; with `fits`, only loops that fit the open song at
//!   that beat (LIB-1).
//! - `GET /api/library/preview?id=<id>`: the item's preview, a WAV; a sample's own file, only for
//!   one the library lists (LIB-6); 404 with `{error}` in words when it has none (a plugin).
//! - `GET /api/library/folders`, `POST /api/library/folders {add | remove}`: your sample folders
//!   (LIB-5), each with how many samples it has so far and whether it is being indexed.
//! - `POST /api/library/sample {session, id, to}` or `{session, id, at, call}`: add a sample to the
//!   open song, a clip at `to` or a Sampler's file (LIB-7).

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use m2_editor::import::Target;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;

crate::api::routes! {
    GET "/api/library" => search; reads, agents,
        "Search everything you can add: instruments, sounds (presets), effects, samples and plugins, best first; q's words may name a kind, loop, one-shot, a tempo or a key; fits (a beat) keeps the loops that fit the song there",
        r#"{"q": "warm pad"}"#;
    GET "/api/library/preview" => preview; reads, agents,
        "An item's preview: a short WAV of it playing, rendered offline and kept; a sample's own file",
        r#"{"id": "std:synth-pluck"}"#;
    GET "/api/library/folders" => folders_get; reads, agents,
        "Your sample folders, each with how many samples it has and whether it is being indexed",
        r#"{}"#;
    POST "/api/library/folders" => folders_set; reads, musician,
        "Add a folder of samples to the library, or remove one",
        r#"{"add": "/Users/you/Samples"}"#;
    POST "/api/library/sample" => add_sample; writes, agents,
        "Add a sample from the library to the song: copied into assets/ with a note of where it came from, then a clip at `to` (a loop follows the song's tempo), or a Sampler's file at `call`",
        r#"{"session": 7, "id": "sample:std:samples/kit/kick.wav", "to": {"kind": "below", "beat": 0}}"#;
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    /// Fits my song: the beat whose tempo a loop must fit.
    #[serde(default)]
    fits: Option<f64>,
}

async fn search(State(s): S, Query(q): Query<SearchQuery>) -> Result<Json<Value>, ApiError> {
    let found = crate::routes::blocking(move || {
        let items = s.library()?;
        let mut query = m2_editor::library::Query::parse(&q.q);
        if let Some(beat) = q.fits {
            query.fits = Some(s.graph.fits(beat)?.ok_or_else(|| anyhow::anyhow!("Fits my song needs a song open."))?);
        }
        let found: Vec<Value> =
            m2_editor::library::search(&items, &query).into_iter().map(serde_json::to_value).collect::<Result<_, _>>()?;
        let folders = s.sample_folders_shown();
        let indexing = folders.iter().filter(|f| f.indexing).count();
        let not_wav: usize = folders.iter().map(|f| f.not_wav).sum();
        Ok::<_, anyhow::Error>(json!({ "items": found, "folders": {"indexing": indexing, "not_wav": not_wav} }))
    })
    .await?;
    Ok(Json(found))
}

#[derive(Deserialize)]
struct PreviewQuery {
    id: String,
}

async fn preview(State(s): S, Query(q): Query<PreviewQuery>) -> Response {
    let made = crate::caller::blocking(move || {
        // A sample's preview is its file, only for one the library lists (LIB-6).
        if q.id.starts_with("sample:") {
            let item = s.listed_sample(&q.id)?;
            let file = &item.sample.as_ref().expect("a sample").file;
            return std::fs::read(file).map_err(|e| anyhow::anyhow!("{} can't be read: {e}", item.label));
        }
        let folders = s.library_folders();
        let cache = folders.previews.ok_or_else(|| anyhow::anyhow!("This app keeps no previews: it has no cache folder."))?;
        let p = crate::library::preview(&s.previews, &q.id, &s.preview_packages()?, folders.presets.as_deref(), &cache)?;
        Ok::<_, anyhow::Error>(p.wav)
    })
    .await;
    match made {
        Ok(Ok(bytes)) => ([(header::CONTENT_TYPE, "audio/wav"), (header::CACHE_CONTROL, "no-store")], bytes).into_response(),
        Ok(Err(e)) => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("{e:#}") }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn folders_get(State(s): S) -> Result<Json<Value>, ApiError> {
    let shown = crate::routes::blocking(move || Ok::<_, anyhow::Error>(s.sample_folders_shown())).await?;
    Ok(Json(json!({ "folders": shown })))
}

#[derive(Deserialize)]
struct FoldersSet {
    #[serde(default)]
    add: Option<PathBuf>,
    #[serde(default)]
    remove: Option<PathBuf>,
}

/// Add a folder of yours (LIB-5), which starts its index in the background, or remove one; this
/// computer's settings keep them. A folder added must be one, and not one already there.
async fn folders_set(State(s): S, Json(r): Json<FoldersSet>) -> Result<Json<Value>, ApiError> {
    let shown = crate::routes::blocking(move || {
        let settings = s.settings().ok_or_else(|| anyhow::anyhow!("This app keeps no settings, so it keeps no folders of samples."))?;
        if let Some(add) = &r.add {
            if !add.is_absolute() || !add.is_dir() {
                anyhow::bail!("{} isn't a folder on this computer.", add.display());
            }
            settings.change(|k| {
                if !k.samples.contains(add) {
                    k.samples.push(add.clone());
                }
            })?;
        }
        if let Some(remove) = &r.remove {
            settings.change(|k| k.samples.retain(|f| f != remove))?;
            s.sample_index.forget(remove);
        }
        Ok(s.sample_folders_shown())
    })
    .await?;
    Ok(Json(json!({ "folders": shown })))
}

#[derive(Deserialize)]
struct SampleReq {
    session: u64,
    id: String,
    /// Where its clip goes: `{kind: "lane", module, lane, beat}`, `{kind: "track", module, beat}`
    /// or `{kind: "below", beat}`.
    #[serde(default)]
    to: Option<Target>,
    /// Or the Sampler call it becomes the file of, in revision `at`.
    #[serde(default)]
    at: Option<u64>,
    #[serde(default)]
    call: Option<String>,
}

/// Add sample `id` from the library to the open song (LIB-7): a clip at `to`, one undo step,
/// answering the edit with `made`, `tracks`, `asset`, `reused`, `noted` and `follows`; or the file of
/// Sampler call `call` in revision `at`. A write.
async fn add_sample(State(s): S, Json(r): Json<SampleReq>) -> Result<Json<Value>, ApiError> {
    let state = s.clone();
    let result = crate::routes::blocking(move || {
        let item = state.listed_sample(&r.id)?;
        let sample = item.sample.expect("a sample");
        match (r.to, r.at, r.call) {
            (Some(to), None, None) => state.graph.add_sample(r.session, sample, to),
            (None, Some(at), Some(call)) => state.graph.sample_to_sampler(r.session, at, call, sample),
            _ => anyhow::bail!("A sample goes to a place (`to`) or to a Sampler (`at` and `call`)."),
        }
    })
    .await;
    s.graph_changed();
    Ok(Json(result?))
}

#[cfg(test)]
#[path = "routes_library_tests.rs"]
mod tests;
