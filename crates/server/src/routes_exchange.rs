//! Swapping files with other music apps (studio lane C), in a router of its own merged into the
//! API's: the song or a clip written as a MIDI file, and the song's stems (`crate::stems`). A route here that writes the open project is
//! in [`WRITES_EXCHANGE`], and a read-only window answers it 423 with WRITE-18's message, as
//! `routes.rs` answers its own. A MIDI file goes to the songs folder's Exports (a new name), or to
//! a file the user picked (replaced, as Export Audio replaces one), never into the project.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use m2_editor::midifile::ClipAt;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

/// The routes here that write the open project.
pub const WRITES_EXCHANGE: &[&str] = &[];

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/graph/midifile/where", post(midifile_where))
        .route("/api/graph/midifile/export", post(midifile_export))
        .route("/api/graph/stems", post(stems_start))
        .route("/api/graph/stems/check", post(stems_check))
        .route_layer(axum::middleware::from_fn_with_state(state, writes))
}

/// A write to a project open read-only answers 423 (WRITE-17), as in `routes.rs`.
async fn writes(State(s): S, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if WRITES_EXCHANGE.contains(&req.uri().path())
        && let Some(why) = s.graph.read_only()
    {
        return (StatusCode::LOCKED, Json(json!({"error": why}))).into_response();
    }
    next.run(req).await
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
}

/// `<dir>/<base>.mid`, `<base> 2.mid`, `<base> 3.mid`…
fn numbered(dir: &Path, base: &str) -> impl Iterator<Item = PathBuf> + use<> {
    let (dir, base) = (dir.to_path_buf(), crate::project::folder_name(base));
    (1..).map(move |n| dir.join(if n == 1 { format!("{base}.mid") } else { format!("{base} {n}.mid") }))
}

/// The songs folder's Exports, made if it isn't there.
fn exports() -> Result<PathBuf> {
    let dir = crate::export::exports_dir()?;
    m2_os::fs::make_dirs(&dir).with_context(|| format!("making {}", dir.display()))?;
    Ok(dir)
}

/// Where a MIDI file named `base` goes unless the user picks a file: the first free name in Exports.
fn free_path(base: &str) -> Result<PathBuf> {
    Ok(numbered(&exports()?, base).find(|p| p.symlink_metadata().is_err()).expect("some number is free"))
}

/// A file the user picked: a full path, `.mid` added when it has no MIDI ending, in a folder that
/// is there.
fn picked(path: PathBuf) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("save to a full path, like /Users/you/Music/song.mid (not {})", path.display());
    }
    let midi = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"));
    let path = if midi {
        path
    } else {
        let mut named = path.into_os_string();
        named.push(".mid");
        PathBuf::from(named)
    };
    let parent = path.parent().context("save to a file, not a whole disk")?;
    if !parent.is_dir() {
        bail!("there's no folder {} to save into", parent.display());
    }
    if path.is_dir() {
        bail!("{} is a folder", path.display());
    }
    Ok(path)
}

/// Write `bytes` to `path` whole: beside it first, then renamed over it, so `path` is never half
/// a file. With `fresh`, only as a new file: the next free name when `path` is taken meanwhile.
fn write_file(path: &Path, bytes: &[u8], fresh: Option<&str>) -> Result<PathBuf> {
    let name = path.file_name().context("a file name")?.to_string_lossy().into_owned();
    let part = path.with_file_name(format!(".{name}.{}.part", std::process::id()));
    let written = (|| -> Result<PathBuf> {
        let mut f = std::fs::File::create(&part)?;
        f.write_all(bytes)?;
        m2_os::fs::push(&f)?;
        drop(f);
        match fresh {
            Some(base) => {
                for p in numbered(path.parent().context("a folder")?, base).skip_while(|p| p != path) {
                    match m2_os::fs::rename(&part, &p, m2_os::fs::Rename::Excl) {
                        Ok(()) => return Ok(p),
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        // A volume that can't rename that way: look, then rename.
                        Err(e) if e.kind() == std::io::ErrorKind::Unsupported && p.symlink_metadata().is_err() => {
                            std::fs::rename(&part, &p)?;
                            return Ok(p);
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                unreachable!("some number is free")
            }
            None => {
                std::fs::rename(&part, path)?;
                Ok(path.to_path_buf())
            }
        }
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    written.with_context(|| format!("couldn't write {}", path.display()))
}

#[derive(Deserialize)]
struct WhereReq {
    session: u64,
    #[serde(default)]
    clip: Option<ClipAt>,
}

/// Where Export as MIDI's save panel starts (the app): `{path}`, the song's (or the clip's) new file
/// in the songs folder's Exports, which is made if it isn't there. Reads only.
async fn midifile_where(State(s): S, Json(r): Json<WhereReq>) -> ApiResult {
    let state = s.clone();
    let path = blocking(move || {
        let (_, base) = state.graph.midi_file(r.session, r.clip)?;
        free_path(&base)
    })
    .await?;
    Ok(Json(json!({"path": path})))
}

#[derive(Deserialize)]
struct ExportReq {
    session: u64,
    /// The file the user picked; else a new one in the songs folder's Exports.
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    clip: Option<ClipAt>,
}

/// Write the song (or `clip`) as a MIDI file (Q37, Q38): `{session, path?, clip?: {module, lane,
/// id}}`. Answers `{path}`, the file written. Writes no part of the project, so a read-only window
/// may.
async fn midifile_export(State(s): S, Json(r): Json<ExportReq>) -> ApiResult {
    let state = s.clone();
    let path = blocking(move || {
        let (bytes, base) = state.graph.midi_file(r.session, r.clip)?;
        match r.path {
            Some(p) => write_file(&picked(p)?, &bytes, None),
            None => write_file(&free_path(&base)?, &bytes, Some(&base)),
        }
    })
    .await?;
    Ok(Json(json!({"path": path})))
}

#[derive(Deserialize)]
struct StemsReq {
    session: u64,
}

/// Export the open song's stems (Q39): `{session}`. Answers `{path}`, the new folder they go in;
/// `export` events with `stems: true` follow (see `crate::stems`). Writes no part of the project.
async fn stems_start(State(s): S, Json(r): Json<StemsReq>) -> ApiResult {
    let state = s.clone();
    let path = blocking(move || crate::stems::start(&state, r.session)).await?;
    Ok(Json(json!({"path": path})))
}

/// Whether the open song's stems can be made now: `{why}`, words when they can't (Q40), else null.
async fn stems_check(State(s): S, Json(r): Json<StemsReq>) -> ApiResult {
    let state = s.clone();
    let why = blocking(move || state.graph.stems_refused(r.session)).await?;
    Ok(Json(json!({"why": why})))
}

#[cfg(test)]
#[path = "routes_exchange_tests.rs"]
mod tests;
