//! Stems (studio lane C, Q39, Q40): each track of the open song as its own WAV file, for another
//! music app. A stem is the track's own output, before its mixer channel (no fader, pan or master),
//! rendered exactly as `m2 bounce <song> x --track <key>` renders it: 32-bit float, from 0 to 2 s
//! past the song's last clip, the same bits (RENDER-3). Into a new folder in the songs folder's
//! Exports, `<song> stems` (then `<song> stems 2`), named `1 Kick.wav`, `2 Hats.wav` in the tracks'
//! order. Tracks muted in the song are left out, as the audio export leaves them out. A song that
//! plays plugins is refused for now (Q40): its render borrows the live instances, keyed by the
//! song's own graph (RENDER-1).
//!
//! Stems take the export's slot ([`crate::export::Slot`]): one export at a time, of either kind.
//! They run on a thread of their own, one stem after another, each built on the session's actor
//! from the revision planned. Clients hear how it goes as `export` events with `stems: true`:
//! `progress` (0..1 for them all) with the `stem` being rendered, its number `n` and how many `of`
//! them; then `done` with the folder's `path`, the `count` and the `seconds` of each; or an
//! `error`, after which nothing it wrote is left.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde_json::json;

use crate::AppState;
use crate::export::Slot;

/// Progress goes out at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// `<exports>/<song> stems`, `<song> stems 2`…, made create-only: the first that wasn't there.
fn new_folder(exports: &Path, song: &str) -> Result<PathBuf> {
    m2_os::fs::make_dirs(exports).with_context(|| format!("making {}", exports.display()))?;
    let base = crate::project::folder_name(&format!("{song} stems"));
    for n in 1.. {
        let dir = exports.join(if n == 1 { base.clone() } else { format!("{base} {n}") });
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                m2_os::fs::sync_dir(exports)?;
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(anyhow!(e).context(format!("making {}", dir.display()))),
        }
    }
    unreachable!("some number is free")
}

/// A stem's file name: `3 Bass.wav`, the label made safe for a file.
fn file_name(n: usize, label: &str) -> String {
    format!("{n} {}.wav", crate::project::folder_name(label))
}

/// Start exporting the open song's stems (see the module's doc), as the window with session
/// `token` asked. Returns the folder they go in; the renders run on.
pub fn start(state: &Arc<AppState>, token: u64) -> Result<PathBuf> {
    start_in(state, token, &crate::export::exports_dir()?)
}

/// [`start`] into a new folder in `exports`.
fn start_in(state: &Arc<AppState>, token: u64, exports: &Path) -> Result<PathBuf> {
    let song = state.graph.name().context("no project is open")?;
    let slot =
        Slot::take(state, &song).ok_or_else(|| anyhow!("A song is already being exported. Wait for it to finish, then try again."))?;
    let plan = state.graph.stems(token)?;
    let dir = state.graph.dir().context("no project is open")?;
    let folder = new_folder(exports, &plan.song)?;
    let stop = slot.stop();
    let (thread_state, thread_folder) = (state.clone(), folder.clone());
    std::thread::Builder::new().name("music2-stems".into()).spawn(move || {
        let (state, folder, plan) = (thread_state, thread_folder, plan);
        let of = plan.stems.len();
        let about = json!({"t": "export", "song": plan.song, "dir": dir, "paused": false, "stems": true});
        let mut written: Vec<PathBuf> = vec![];
        let mut seconds = 0.0;
        let mut last: Option<Instant> = None;
        let mut run = || -> Result<()> {
            for (i, (label, _)) in plan.stems.iter().enumerate() {
                let render = state.graph.stem_render(&plan, i)?.stop_when(stop.clone());
                let path = folder.join(file_name(i + 1, label));
                let tell = |done: f64, last: &mut Option<Instant>| {
                    if last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
                        *last = Some(Instant::now());
                        let progress = (i as f64 + done) / of as f64;
                        let _ =
                            state.events.send(with(&about, json!({"progress": progress, "stem": label, "n": i + 1, "of": of})).to_string());
                    }
                };
                // Each stem says it started, whatever the clock says.
                last = None;
                tell(0.0, &mut last);
                let r = render.write_float(&path, |p| {
                    if p < 1.0 {
                        tell(p, &mut last)
                    }
                })?;
                written.push(path);
                seconds = r.seconds();
            }
            Ok(())
        };
        // A bug in a render mustn't leave every later export refused as "already running".
        let result = std::panic::catch_unwind(AssertUnwindSafe(&mut run))
            .unwrap_or_else(|_| Err(anyhow!("music2 hit a bug while exporting stems")))
            .map_err(crate::graph::plainly);
        let event = match result {
            Ok(()) => json!({"done": true, "path": folder, "count": written.len(), "seconds": seconds}),
            Err(e) => {
                // Nothing half done is left: the stems this run wrote go, and the folder.
                for p in &written {
                    let _ = std::fs::remove_file(p);
                }
                let _ = std::fs::remove_dir(&folder);
                json!({"error": format!("{e:#}"), "code": m2_editor::faults::Faulted::of(&e).map(|f| f.code), "path": folder})
            }
        };
        // Free before saying so: a client may export again as soon as it hears.
        drop(slot);
        state.announce(with(&about, event).to_string());
    })?;
    Ok(folder)
}

/// `about` with `more`'s fields.
fn with(about: &serde_json::Value, more: serde_json::Value) -> serde_json::Value {
    let mut v = about.clone();
    if let (Some(a), serde_json::Value::Object(m)) = (v.as_object_mut(), more) {
        a.extend(m);
    }
    v
}

#[cfg(test)]
#[path = "stems_tests.rs"]
mod tests;
