//! The library (LIB-1 to LIB-3): the one search, and previews.
//!
//! - `GET /api/library?q=<words>`: `{items: [...]}`, the items the search finds, best first.
//! - `GET /api/library/preview?id=<id>`: the item's preview, a WAV; 404 with `{error}` in words
//!   when it has none (a plugin).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;

crate::api::routes! {
    GET "/api/library" => search; reads, agents,
        "Search everything you can add: instruments, sounds (presets), effects, samples and plugins, best first; q's words may name a kind, loop, one-shot, a tempo or a key",
        r#"{"q": "warm pad"}"#;
    GET "/api/library/preview" => preview; reads, agents,
        "An item's preview: a short WAV of it playing, rendered offline and kept",
        r#"{"id": "std:synth-pluck"}"#;
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

async fn search(State(s): S, Query(q): Query<SearchQuery>) -> Result<Json<Value>, ApiError> {
    let items = crate::routes::blocking(move || s.library()).await?;
    let found = m2_editor::library::search(&items, &m2_editor::library::Query::parse(&q.q));
    Ok(Json(json!({ "items": found })))
}

#[derive(Deserialize)]
struct PreviewQuery {
    id: String,
}

async fn preview(State(s): S, Query(q): Query<PreviewQuery>) -> Response {
    let made = crate::caller::blocking(move || {
        let cache = m2_os::dirs::cache().ok_or_else(|| anyhow::anyhow!("can't find your home folder ($HOME isn't set)"))?;
        let p = crate::library::preview(&s.previews, &q.id, m2_os::dirs::presets().as_deref(), &cache.join("previews"))?;
        Ok::<_, anyhow::Error>(std::fs::read(&p.file)?)
    })
    .await;
    match made {
        Ok(Ok(bytes)) => ([(header::CONTENT_TYPE, "audio/wav"), (header::CACHE_CONTROL, "no-store")], bytes).into_response(),
        Ok(Err(e)) => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("{e:#}") }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[cfg(test)]
#[path = "routes_library_tests.rs"]
mod tests;
