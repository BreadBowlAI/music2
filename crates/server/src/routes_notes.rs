//! The routes of the polish track's lane D, in the one route table (AGENT-8): the song's key; and
//! track notes' table of what music2 knows of scales, chords and grooves (NOTE-1).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use m2_editor::grooves::SWINGS;
use m2_editor::scales::{Chord, TABLE};
use m2_editor::songkey::Written;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/song-key" => song_key; writes, agents,
        "Set the song's key, or take it away (null)",
        r#"{"session": 7, "key": {"root": "A", "scale": "minor"}}"#;
    GET "/api/theory" => theory; reads, agents,
        "The scales a key can have, the chords a stamp places and the grooves, each with its name and what it does",
        r#"{}"#;
}

/// The one table of scales (NOTE-1), the chord stamps' kinds (NOTE-4) and the grooves (NOTE-7):
/// `{scales: [{name, label, words, steps}], chords: [{name, label, words, inversions}], grooves:
/// [{name, label, words, example}], swings: [0.5, …]}`, where `example` is what a request writes. The key field and the roll's menus read it, and so
/// may agents. It needs no song.
async fn theory() -> ApiResult {
    Ok(Json(table()))
}

fn table() -> Value {
    let scales: Vec<Value> = TABLE.iter().map(|r| json!({"name": r.name, "label": r.label, "words": r.words, "steps": r.steps})).collect();
    let chords: Vec<Value> = Chord::ALL
        .iter()
        .map(|c| json!({"name": c.name(), "label": c.label(), "words": c.words(), "inversions": c.inversions()}))
        .collect();
    let grooves: Vec<Value> = m2_editor::grooves::named()
        .into_iter()
        .map(|(name, label, words, example)| json!({"name": name, "label": label, "words": words, "example": example}))
        .collect();
    json!({"scales": scales, "chords": chords, "grooves": grooves, "swings": SWINGS})
}

#[derive(Deserialize)]
struct SongKeyReq {
    session: u64,
    /// `{root, scale}`, or null for no key: read by the edit, so one that doesn't read is
    /// refused in words.
    key: Option<Written>,
}

/// The song's key (D2, Q22): `{session, key: {root, scale} | null}`, written into `layout.json` as
/// one undo step with no revision. Named `song-key`, apart from a track's ID (taste.md:71). A
/// write. Answers `{file, before, after, revision}`.
async fn song_key(State(s): S, Json(r): Json<SongKeyReq>) -> ApiResult {
    let state = s.clone();
    let result = crate::caller::blocking(move || state.graph.song_key(r.session, r.key)).await.map_err(anyhow::Error::from)?;
    s.graph_changed();
    Ok(Json(result?))
}

#[cfg(test)]
#[path = "routes_notes_tests.rs"]
mod tests;
