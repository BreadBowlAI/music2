//! Swapping files with other music apps (studio lane C), in the one route table (AGENT-8): the song
//! or a clip written as a MIDI file, a MIDI file brought in, and the song's stems
//! (`crate::stems`). A MIDI file goes to the songs folder's Exports (a new name), or to a file the
//! user picked (replaced, as Export Audio replaces one), never into the project: a file picked in
//! the open song's folder is refused, whoever asks, so an agent's export, a read, writes nothing of
//! the song.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, header};
use m2_editor::midifile::{ClipAt, TempoChoice};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::graph::exchange::{MidiSource, MidiTarget};
use crate::routes::ApiError;

type S = State<Arc<AppState>>;
type ApiResult = Result<Json<Value>, ApiError>;

crate::api::routes! {
    POST "/api/graph/midifile/where" => midifile_where; reads, agents,
        "Where a MIDI file of the song, or of a clip, would go: a new name in the songs folder's Exports",
        r#"{"session": 7}"#;
    POST "/api/graph/midifile/export" => midifile_export; reads, agents,
        "Write the song, or a clip, as a MIDI file: to the path given, else to a new file in Exports",
        r#"{"session": 7, "clip": {"module": "keys", "lane": "chords", "id": "k3f9q2zt"}}"#;
    POST "/api/graph/midifile/import" => midifile_import up to m2_editor::smf::MAX_BYTES + 1024; writes, agents,
        "Bring a MIDI file on this computer in: into a lane, on new tracks after one, or on new tracks below (tempo: keep or use the file's)",
        r#"{"session": 7, "path": "/Users/you/Downloads/riff.mid", "to": {"kind": "below", "beat": 0}, "tempo": "keep"}"#;
    POST "/api/graph/stems" => stems_start; reads, musician,
        "Export the song's stems, which may pause playback (agents render with m2 bounce)",
        r#"{"session": 7}"#;
    POST "/api/graph/stems/check" => stems_check; reads, agents,
        "Whether the song's stems can be exported now, and why not",
        r#"{"session": 7}"#;
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, ApiError> {
    crate::caller::blocking(f).await.map_err(anyhow::Error::from)?.map_err(ApiError::from)
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

/// Whether `path` is in the song's folder `song` (as `Graph::dir` names it), through a link too: a
/// MIDI file never goes there, being no file of the song.
fn in_song(path: &Path, song: &Path) -> bool {
    let song = song.canonicalize().unwrap_or_else(|_| song.to_path_buf());
    path.parent().and_then(|p| p.canonicalize().ok()).is_some_and(|p| p.starts_with(&song))
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
/// may: a file in the song's folder is refused, with nothing written.
async fn midifile_export(State(s): S, Json(r): Json<ExportReq>) -> ApiResult {
    let state = s.clone();
    let path = blocking(move || {
        let (bytes, base) = state.graph.midi_file(r.session, r.clip)?;
        let (to, fresh) = match r.path {
            Some(p) => (picked(p)?, None),
            None => (free_path(&base)?, Some(base.as_str())),
        };
        if let Some(song) = state.graph.dir().filter(|song| in_song(&to, song)) {
            bail!(
                "save it outside the song's folder ({}): a MIDI file is no part of the song, and that folder keeps the song's own files",
                song.display()
            );
        }
        write_file(&to, &bytes, fresh)
    })
    .await?;
    Ok(Json(json!({"path": path})))
}

#[derive(Deserialize)]
struct ImportJson {
    session: u64,
    path: PathBuf,
    to: MidiTarget,
    /// The tempo question's answer (Q35).
    #[serde(default)]
    tempo: Option<TempoChoice>,
}

#[derive(Deserialize)]
struct ImportQuery {
    session: Option<u64>,
    /// The file's name.
    name: Option<String>,
    /// Where it goes, as JSON.
    to: Option<String>,
    /// The tempo question's answer: `keep` or `use`.
    tempo: Option<TempoChoice>,
}

/// Bring a MIDI file in (C2, Q32 to Q36, D11), as one undo step: JSON `{session, path, to,
/// tempo?}` (the app: a file on this computer, read by the server), or the file's bytes as the
/// body with `?session=&name=&to=&tempo=` (a browser's upload, up to 16 MB). `to` is `{kind:
/// "lane", module, lane, beat}`, `{kind: "track", module, beat}` or `{kind: "below", beat}`;
/// `tempo` answers the tempo question, `keep` or `use`. A write. Answers `{asks: {file_bpm,
/// song_bpm}}` when the question is due (nothing written), else the edit with `made`, `tracks`,
/// `name` (the undo step's) and `undo_name`.
async fn midifile_import(State(s): S, Query(q): Query<ImportQuery>, headers: HeaderMap, body: Bytes) -> ApiResult {
    let json = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("application/json"));
    let (session, source, to, tempo) = match json {
        true => {
            let r: ImportJson = serde_json::from_slice(&body).map_err(anyhow::Error::from)?;
            (r.session, MidiSource::Path(r.path), r.to, r.tempo)
        }
        false => {
            let session = q.session.ok_or_else(|| anyhow::anyhow!("an upload names its session"))?;
            let name = q.name.filter(|n| !n.is_empty()).ok_or_else(|| anyhow::anyhow!("an upload names its file"))?;
            let to: MidiTarget = serde_json::from_str(q.to.as_deref().unwrap_or(""))
                .map_err(|e| anyhow::anyhow!("where the file goes doesn't read: {e}"))?;
            (session, MidiSource::Bytes { name, bytes: body.to_vec() }, to, q.tempo)
        }
    };
    let state = s.clone();
    let result = blocking(move || state.graph.import_midi(session, source, to, tempo)).await;
    s.graph_changed();
    Ok(Json(result?))
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
