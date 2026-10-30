//! The library (LIB-1 to LIB-7): the one search, previews, your sample folders, and adding a
//! sample.
//!
//! - `GET /api/library?q=<words>[&fits=<beat>]`: `{items: [...], folders: {indexing, not_wav}}`,
//!   the items the search finds, best first; with `fits`, only loops that fit the open song at
//!   that beat (LIB-1). `indexing` counts the indexes of samples running (LIB-5): ask again when
//!   it is 0 to see what they found.
//! - `GET /api/library/preview?id=<id>`: the item's preview, a WAV; a sample's own file, streamed,
//!   only for one the library lists, as it was indexed, and up to 64 MB (LIB-6), a FLAC's as the
//!   WAV of it (FMT-24); 404 with
//!   `{error}` in words when it has none (a plugin).
//! - `GET /api/library/folders`, `POST /api/library/folders {add | remove}`: your sample folders
//!   (LIB-5), each with how many samples it has so far and whether it is being indexed.
//! - `POST /api/library/sample {session, id, to}` or `{session, id, at, call}`: add a sample to the
//!   open song, a clip at `to` or a Sampler's file (LIB-7).
//! - The search also answers From the library (REG-13): `library`, the registry's items of packs
//!   this computer hasn't got, and `registry`: `{fetching, made, error}`, or null with none set.
//!   A library item's preview is its blob (REG-14).
//! - `POST /api/library/get {id}` (a library item; a sample with `session` and `to`) or `{pack,
//!   version}` (a song's missing locked version): Get it (REG-15 to REG-17), one at a time,
//!   answering once it is done; `{"t": "get", id, label, done, total}` events say how far it is,
//!   and `{"t": "get", id, end: true}` that it ended. `POST /api/library/get/cancel` stops it.
//! - `GET /api/library/missing`: the open song's locked versions that aren't installed, each with
//!   whether the library has it (`can`) or why not (REG-17). `POST /api/library/store-changed`:
//!   packs were installed outside the app (`m2 pack get`); the open song is told.
//! - From the library answers at most `LIBRARY_ROWS` rows, and `library_more`, how many more.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use m2_editor::import::Target;
use m2_editor::library::samples;
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
        "An item's preview: a short WAV of it playing, rendered offline and kept; a sample's own file (a FLAC's as the WAV of it)",
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
    POST "/api/library/get" => get_item; writes, agents,
        "Get a pack or a sample from the library (the registry): a pack's whole version installed, checked, or one sample added to the song with its note; or a version the open song locks and is missing. Answers once it is done; `get` events say how far",
        r#"{"id": "library:@alice/felt-piano:felt-piano"}"#;
    POST "/api/library/get/cancel" => cancel_get; reads, agents,
        "Stop the Get running: nothing of it is kept",
        r#"{}"#;
    POST "/api/library/store-changed" => store_changed; reads, agents,
        "Tell the app packs were installed or removed outside it (m2 pack get does): the open song's lists are made again, and a song that waited for a pack builds",
        r#"{}"#;
    GET "/api/library/missing" => missing; reads, agents,
        "The packs the open song's music2.lock names that aren't installed, each with whether the library has that very version to get",
        r#"{}"#;
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
        let found: Vec<Value> = items.search(&query).into_iter().map(serde_json::to_value).collect::<Result<_, _>>()?;
        // From the library: what is typed searches the catalog kept here, and is never sent (REG-13).
        let offers = s.offers();
        let found_there = m2_editor::get::find(&offers, &query);
        let more = found_there.len().saturating_sub(crate::registry::LIBRARY_ROWS);
        let library: Vec<Value> = found_there.into_iter().take(crate::registry::LIBRARY_ROWS).map(|o| o.json()).collect();
        let indexing = s.sample_index.indexing();
        let not_wav: usize = s.sample_folders_shown().iter().map(|f| f.not_wav).sum();
        Ok::<_, anyhow::Error>(json!({
            "items": found, "library": library, "library_more": more, "registry": s.registry_said(),
            "folders": {"indexing": indexing, "not_wav": not_wav},
        }))
    })
    .await?;
    Ok(Json(found))
}

#[derive(Deserialize)]
struct PreviewQuery {
    id: String,
}

/// What a preview answers: a rendered WAV, or a sample's own file, as it was indexed.
enum Previewed {
    Wav(Vec<u8>),
    File(PathBuf, m2_os::fs::Stamp),
}

async fn preview(State(s): S, Query(q): Query<PreviewQuery>, headers: axum::http::HeaderMap) -> Response {
    let made = crate::caller::blocking(move || {
        // A library item's: its blob, fetched by its hash and kept (REG-14); a FLAC's as its WAV.
        if q.id.starts_with(m2_editor::get::PREFIX) {
            let file = s.library_preview(&q.id)?;
            let sample = m2_editor::library::Sample { file: file.clone(), ..Default::default() };
            let stamp = samples::still_plain(&sample).map_err(|e| anyhow::anyhow!("The preview can't be played: {e}."))?;
            if let Some(wav) = samples::flac_preview(&sample, stamp).map_err(|e| anyhow::anyhow!("The preview can't be played: {e}."))? {
                return Ok(Previewed::Wav(wav));
            }
            return Ok(Previewed::File(file, stamp));
        }
        // A sample's preview is its file, only for one the library lists (LIB-6).
        if q.id.starts_with("sample:") {
            let item = s.listed_sample(&q.id)?;
            let sample = item.sample.expect("a sample");
            let stamp = samples::still_plain(&sample).map_err(|e| anyhow::anyhow!("{} can't be played: {e}.", item.label))?;
            // A FLAC's is the WAV of it, made now (FMT-24).
            if let Some(wav) = samples::flac_preview(&sample, stamp).map_err(|e| anyhow::anyhow!("{} can't be played: {e}.", item.label))? {
                return Ok(Previewed::Wav(wav));
            }
            if stamp.size > samples::PREVIEW_BYTES {
                anyhow::bail!(
                    "{} is {} MB, too long to preview here (up to {} MB): add it to the song to hear it.",
                    item.label,
                    stamp.size / 1_000_000,
                    samples::PREVIEW_BYTES / 1_000_000
                );
            }
            return Ok(Previewed::File(sample.file, stamp));
        }
        let folders = s.library_folders();
        let cache = folders.previews.ok_or_else(|| anyhow::anyhow!("This app keeps no previews: it has no cache folder."))?;
        let p = crate::library::preview(&s.previews, &q.id, &s.preview_packages()?, folders.presets.as_deref(), &cache)?;
        Ok::<_, anyhow::Error>(Previewed::Wav(p.wav))
    })
    .await;
    let refused = |status, e: String| (status, Json(json!({ "error": e }))).into_response();
    match made {
        Ok(Ok(Previewed::Wav(bytes))) => {
            ([(header::CONTENT_TYPE, "audio/wav"), (header::CACHE_CONTROL, "no-store")], bytes).into_response()
        }
        Ok(Ok(Previewed::File(file, stamp))) => match stream(&file, stamp, &headers).await {
            Ok(r) => r,
            Err(e) => refused(StatusCode::NOT_FOUND, format!("{e:#}")),
        },
        Ok(Err(e)) => refused(StatusCode::NOT_FOUND, format!("{e:#}")),
        Err(e) => refused(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Stream `file` (LIB-6), read in pieces as it is sent, never whole, with its length (and a range
/// asked for). The file it opened must be the plain file stamped `stamp` when it was checked: one
/// a link was put in place of since is refused.
async fn stream(file: &std::path::Path, stamp: m2_os::fs::Stamp, headers: &axum::http::HeaderMap) -> anyhow::Result<Response> {
    let mut ask = axum::http::Request::builder();
    if let Some(range) = headers.get(header::RANGE) {
        ask = ask.header(header::RANGE, range);
    }
    let served = tower_http::services::ServeFile::new(file).try_call(ask.body(axum::body::Body::empty())?).await?;
    // It opened the file by its path: what is there now must still be the file checked.
    if m2_os::fs::Stamp::at(file) != Some(stamp) {
        anyhow::bail!("{} changed as it was asked for: a sample is only ever read where it was found.", file.display());
    }
    let (mut parts, body) = served.into_parts();
    parts.headers.insert(header::CONTENT_TYPE, axum::http::HeaderValue::from_static("audio/wav"));
    parts.headers.insert(header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    Ok(Response::from_parts(parts, axum::body::Body::new(body)))
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
/// computer's settings keep them. A folder added is kept by its real path ([`folder_to_add`]).
/// One removed stops its index and loses its cache file.
async fn folders_set(State(s): S, Json(r): Json<FoldersSet>) -> Result<Json<Value>, ApiError> {
    let shown = crate::routes::blocking(move || {
        let settings = s.settings().ok_or_else(|| anyhow::anyhow!("This app keeps no settings, so it keeps no folders of samples."))?;
        if let Some(add) = &r.add {
            let home = m2_os::dirs::home();
            let add = folder_to_add(add, &s.sample_folders(), home.as_deref())?;
            settings.change(|k| {
                if !k.samples.contains(&add) {
                    k.samples.push(add.clone());
                }
            })?;
        }
        if let Some(remove) = &r.remove {
            let real = std::fs::canonicalize(remove).ok();
            let gone: Vec<PathBuf> = s.sample_folders().into_iter().filter(|f| f == remove || Some(f) == real.as_ref()).collect();
            settings.change(|k| k.samples.retain(|f| !gone.contains(f)))?;
            let cache = s.library_folders().samples;
            for f in &gone {
                s.sample_index.forget(f);
                if let Some(cache) = &cache {
                    samples::forget(cache, f);
                }
            }
        }
        Ok(s.sample_folders_shown())
    })
    .await?;
    Ok(Json(json!({ "folders": shown })))
}

/// The folder `asked` as one of yours is kept (LIB-5): by its real path (links and `..` resolved),
/// a folder on this computer; not `/` nor your home folder `home` itself, whose every file would be
/// looked through; not inside or around one of `yours` already, whose files it would list again.
/// Why not, in words.
pub(crate) fn folder_to_add(asked: &std::path::Path, yours: &[PathBuf], home: Option<&std::path::Path>) -> anyhow::Result<PathBuf> {
    let shown = asked.display();
    let real = match std::fs::canonicalize(asked) {
        Ok(r) if asked.is_absolute() && r.is_dir() => r,
        _ => anyhow::bail!("{shown} isn't a folder on this computer."),
    };
    if real.parent().is_none() {
        anyhow::bail!("{shown} is the whole disk: choose the folder your samples are in.");
    }
    if home.and_then(|h| std::fs::canonicalize(h).ok()).is_some_and(|h| h == real) {
        anyhow::bail!(
            "{shown} is your home folder: choose the folder your samples are in, so music2 doesn't look through every file you have."
        );
    }
    for f in yours.iter().filter(|f| **f != real) {
        if real.starts_with(f) {
            anyhow::bail!("{shown} is inside {}, which is one of your folders already.", f.display());
        }
        if f.starts_with(&real) {
            anyhow::bail!("{shown} holds {}, one of your folders: remove that one first.", f.display());
        }
    }
    Ok(real)
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
        // Copied only as it was found: a plain file, never a link put there since (LIB-6).
        samples::still_plain(&sample).map_err(|e| anyhow::anyhow!("{} can't be added: {e}.", item.label))?;
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

/// Get a library item, or a version the open song locks and is missing (REG-15 to REG-17),
/// answering once it is done. A write: a pack installed, or a sample added to the song.
async fn get_item(State(s): S, Json(r): Json<crate::registry::GetReq>) -> Result<Json<Value>, ApiError> {
    let state = s.clone();
    let result = crate::routes::blocking(move || state.get_from_library(r)).await;
    s.graph_changed();
    Ok(Json(result?))
}

/// Stop the Get running (REG-15): `{cancelled}`, whether it stops; once it is being installed it
/// can't be, said in `why`.
async fn cancel_get(State(s): S) -> Json<Value> {
    Json(match s.cancel_get() {
        crate::registry::Cancelled::Taken => json!({ "cancelled": true }),
        crate::registry::Cancelled::Refused => json!({ "cancelled": false, "why": "It is being installed now: it can't be stopped." }),
        crate::registry::Cancelled::Nothing => json!({ "cancelled": false }),
    })
}

/// The store changed outside the app (REG-17): the open song is told, as after a Get.
async fn store_changed(State(s): S) -> Result<Json<Value>, ApiError> {
    let state = s.clone();
    crate::routes::blocking(move || state.store_changed()).await?;
    s.graph_changed();
    Ok(Json(json!({})))
}

/// The open song's locked versions that aren't installed (REG-17): each with whether the library
/// has it (`can`, and the bytes a Get fetches) or why not.
async fn missing(State(s): S) -> Result<Json<Value>, ApiError> {
    let missing = crate::routes::blocking(move || s.missing_packs()).await?;
    let shown: Vec<Value> = missing
        .iter()
        .map(|m| match &m.got {
            Ok(v) => {
                json!({"pack": m.name.as_str(), "version": m.version.to_string(), "hash": m.hash.prefixed(), "can": true, "size": v.size})
            }
            Err(why) => {
                json!({"pack": m.name.as_str(), "version": m.version.to_string(), "hash": m.hash.prefixed(), "can": false, "why": why})
            }
        })
        .collect();
    Ok(Json(json!({ "missing": shown })))
}

#[cfg(test)]
#[path = "routes_library_tests.rs"]
mod tests;
