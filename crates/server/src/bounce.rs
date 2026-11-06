//! Bounce to audio in the app (RENDER-40): a track's menu, and `m2 bounce --in-place` while the app
//! has the song open. The track is planned on the actor, rendered here (on the request's own
//! thread, off the actor), then put in place as one undo step ([`crate::graph::Graph::bounce_place`]).
//!
//! It takes the export's slot ([`crate::export::Slot`]): one render at a time, of any kind, and
//! quitting stops it. Clients hear how it goes as `bounce` events naming the `song`, its folder
//! (`dir`), the `track` and its `label`: `progress` (0..1, at most ten a second), then `done`, or
//! an `error`. Stopping it ([`stop`]) leaves no file. A change to the song while it renders stops
//! it at once, saying so; the bounce's own files, which the watcher may take in once they are
//! named, are no change. When the step is refused the files it wrote go: nothing names them.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::AppState;
use crate::export::Slot;

/// What a bounce stopped while it renders (Stop, or the app quitting) answers.
pub const STOPPED: &str = "The bounce was stopped, so nothing was added.";

/// Progress goes out at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Bounce track `track` of the open song in place, as the window with session `token` asked, into
/// `assets/<name>.wav` when a name is given (else a free one). Waits until it is in the song, or
/// refused: answers the edit (see `Graph::bounce_place`).
pub fn run(state: &Arc<AppState>, token: u64, track: String, name: Option<String>) -> Result<Value> {
    run_with(state, token, track, name, Hooks::default())
}

/// What a test does at a point of a bounce, so it never races the render: as it renders (each time
/// it says how far it is), and once its files are written, before it is placed.
#[derive(Default)]
pub(crate) struct Hooks {
    pub(crate) rendering: Option<Box<dyn FnMut(f64) + Send>>,
    pub(crate) written: Option<Box<dyn FnOnce() + Send>>,
}

/// [`run`], with `hooks`.
pub(crate) fn run_with(state: &Arc<AppState>, token: u64, track: String, name: Option<String>, mut hooks: Hooks) -> Result<Value> {
    let song = state.graph.name().ok_or_else(|| anyhow!("no project is open"))?;
    let slot = Slot::take(state, &song)
        .ok_or_else(|| anyhow!("A song is being exported or bounced. Wait for it to finish, then bounce this track."))?;
    let bouncing = Bouncing::now(state);
    let plan = state.graph.bounce_plan(token, track.clone(), name)?;
    let about = json!({"t": "bounce", "song": song, "dir": plan.dir, "track": track, "label": plan.label});
    let tell = |more: Value| state.send(with(&about, more));
    tell(json!({"progress": 0.0}));
    let mut last = Instant::now();
    let stop = slot.stop();
    let render = plan.render.stop_when(stop.clone());
    // A change to the song stops it at once (RENDER-40): asked of the actor only once the shown
    // revision moved, and never again once it did.
    let planned = plan.bounce.planned();
    let mut changed = false;
    let written = plan.bounce.write(render, &plan.dir, &plan.asset, |p| {
        if let Some(hook) = &mut hooks.rendering {
            hook(p);
        }
        if let Some(planned) = planned.as_ref().filter(|_| !changed)
            && state.graph.shown_revision() != Some(planned.number())
            && state.graph.bounce_changed(token, planned.clone(), plan.asset.clone())
        {
            changed = true;
            stop.store(true, Ordering::SeqCst);
        }
        if p < 1.0 && last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            tell(json!({"progress": p}));
        }
    });
    let written = match written {
        Err(_) if changed => Err(anyhow!("{}", m2_editor::bounce::CHANGED)),
        Err(e) if stop.load(Ordering::SeqCst) && m2_editor::faults::Faulted::of(&e).is_none() => Err(anyhow!("{STOPPED}")),
        w => w.map_err(crate::graph::plainly),
    };
    if let (Ok(_), Some(hook)) = (&written, hooks.written.take()) {
        hook();
    }
    let placed = written.and_then(|bounced| {
        let placed = state.graph.bounce_place(token, track, bounced.clone());
        placed.map_err(|e| {
            bounced.remove();
            // Its words, as the window says them: "edit rejected" is no word of the musician's.
            anyhow!("{}", format!("{e:#}").trim_start_matches("edit rejected: "))
        })
    });
    // Free before saying so: a client may export or bounce again as soon as it hears.
    drop(bouncing);
    drop(slot);
    match &placed {
        Ok(v) => tell(json!({"done": true, "module": v["module"], "name": v["name"]})),
        Err(e) => tell(json!({"error": format!("{e:#}"), "code": m2_editor::faults::Faulted::of(e).map(|f| f.code)})),
    }
    placed
}

/// Says the slot's job is a bounce while it lives.
struct Bouncing<'a>(&'a AppState);

impl Bouncing<'_> {
    fn now(state: &AppState) -> Bouncing<'_> {
        state.exports.bouncing.store(true, Ordering::SeqCst);
        Bouncing(state)
    }
}

impl Drop for Bouncing<'_> {
    fn drop(&mut self) {
        self.0.exports.bouncing.store(false, Ordering::SeqCst);
    }
}

/// Stop the bounce that is rendering, if one is (an export is left alone), and wait up to 10 s for
/// it to take its file back. True once no bounce renders.
pub fn stop(state: &AppState) -> bool {
    if !ask_stop(state) {
        return true;
    }
    state.exports.stop(Duration::from_secs(10))
}

/// Ask the bounce that is rendering to stop, without waiting: whether one is.
fn ask_stop(state: &AppState) -> bool {
    let bouncing = state.exports.bouncing.load(Ordering::SeqCst);
    if bouncing {
        state.exports.stop.store(true, Ordering::SeqCst);
    }
    bouncing
}

/// `about` with `more`'s fields.
fn with(about: &Value, more: Value) -> Value {
    let mut v = about.clone();
    if let (Some(a), Value::Object(m)) = (v.as_object_mut(), more) {
        a.extend(m);
    }
    v
}

#[cfg(test)]
#[path = "bounce_tests.rs"]
mod tests;
