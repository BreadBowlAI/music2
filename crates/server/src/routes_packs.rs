//! The packs installed on this computer (PACK-11), for the browser's Packs list and agents: each
//! pack's name, versions, licence and what is in it. Reading the store writes nothing; it works
//! with no song open.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    GET "/api/packs" => packs_get; reads, agents,
        "The packs installed on this computer: each one's name, versions (newest last), licence, description and items",
        r#"{}"#;
}

async fn packs_get(State(_): S) -> ApiResult {
    Ok(Json(crate::caller::blocking(installed).await.map_err(anyhow::Error::from)?))
}

/// `{packs: [{name, versions, license, description, items: [{component, label, kind, category}]}]}`:
/// each item is an instrument or an effect of the newest version.
fn installed() -> Value {
    let Some(store) = m2_song::pack::Store::here() else { return json!({"packs": []}) };
    let std = crate::resources::std_dir();
    let packs: Vec<Value> = m2_editor::packs::installed(&store)
        .into_iter()
        .map(|p| {
            let catalog = m2_script::describe(&[("std".to_string(), std.clone()), (p.name.clone(), p.dir.clone())]);
            let prefix = format!("{}:", p.name);
            let items: Vec<Value> = catalog
                .components
                .values()
                .filter(|d| d.id.starts_with(&prefix) && d.editor.category.is_some())
                .map(|d| {
                    let kind = if d.is_instrument() { "instrument" } else { "effect" };
                    json!({"component": d.id, "label": d.label, "kind": kind, "category": d.editor.category})
                })
                .collect();
            json!({"name": p.name, "versions": p.versions, "license": p.license, "description": p.description, "items": items})
        })
        .collect();
    json!({ "packs": packs })
}
