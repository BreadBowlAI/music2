//! The installed plugins, as the side browser and + Track list them (gaps #32). The host process
//! finds them (`PluginHost::scan`, HOST-3): the app never loads plugin code. A scan loads every
//! plugin library, so it runs once per app run, on the first ask, off the async threads, and its
//! list is kept until Look again (`POST /api/plugins/scan`). A song with no plugins starts no host
//! process until a list is opened.
//!
//! Adding one goes through the add routes std's sounds use: `POST /api/graph/effect/add` and
//! `/api/graph/track/add` take `clap:<id>` as their `component`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    GET "/api/plugins" => list; reads, agents,
        "The installed CLAP plugins: id, name, vendor, version, and whether each is an instrument or an effect",
        r#"{}"#;
    POST "/api/plugins/scan" => scan; reads, agents,
        "Look for installed plugins again, and list them",
        r#"{}"#;
}

/// `{plugins: [{id: "clap:…", name, vendor, version, instrument, effect}], errors, folder}`: the
/// list kept for this app run, made now on the first ask.
async fn list(State(s): S) -> ApiResult {
    Ok(Json(crate::caller::blocking(move || s.plugins(false)).await.map_err(anyhow::Error::from)?))
}

/// Look again: scan now, and keep that list.
async fn scan(State(s): S) -> ApiResult {
    Ok(Json(crate::caller::blocking(move || s.plugins(true)).await.map_err(anyhow::Error::from)?))
}

/// What `host` finds installed, as the app lists it: by name, each plugin once (the first folder's,
/// as `m2_plugins::find` takes it). Each is an instrument, an effect, or both, by its CLAP features.
/// `errors`: the libraries that didn't load. `folder`: where to install one, the user's CLAP
/// folder, written from `~` when it is in the home folder.
pub(crate) fn listed(host: &dyn m2_plugins::PluginHost) -> Value {
    let (found, errors) = host.scan();
    let mut seen = std::collections::BTreeSet::new();
    let mut plugins: Vec<&m2_plugins::Found> = found.iter().filter(|f| seen.insert(f.id.clone())).collect();
    plugins.sort_by_key(|f| (f.name.to_lowercase(), f.id.clone()));
    let plugins: Vec<Value> = plugins
        .into_iter()
        .map(|f| {
            json!({
                "id": format!("clap:{}", f.id),
                "name": f.name,
                "vendor": f.vendor,
                "version": f.version,
                "instrument": f.is_instrument(),
                "effect": f.is_effect(),
            })
        })
        .collect();
    let home = m2_os::dirs::home();
    let folder = m2_os::dirs::clap().into_iter().next().map(|d| match home.as_deref().and_then(|h| d.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => d.display().to_string(),
    });
    json!({"plugins": plugins, "errors": errors, "folder": folder})
}
