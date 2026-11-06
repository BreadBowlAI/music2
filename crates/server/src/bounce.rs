//! Bounce to audio in the app (RENDER-40): a track's menu, and `m2 bounce --in-place` while the app
//! has the song open. The track is planned on the actor, rendered here (on the request's own
//! thread, off the actor), then put in place as one undo step ([`crate::graph::Graph::bounce_place`]).
//!
//! It takes the export's slot ([`crate::export::Slot`]): one render at a time, of any kind, and
//! quitting stops it. Clients hear how it goes as `bounce` events naming the `song`, its folder
//! (`dir`), the `track` and its `label`: `progress` (0..1, at most ten a second), then `done`, or
//! an `error`. Stopping it ([`stop`]) leaves no file. When the step is refused (the song changed
//! while it rendered) the files it wrote go: nothing names them.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::AppState;
use crate::export::Slot;

/// Progress goes out at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Bounce track `track` of the open song in place, as the window with session `token` asked, into
/// `assets/<name>.wav` when a name is given (else a free one). Waits until it is in the song, or
/// refused: answers the edit (see `Graph::bounce_place`).
pub fn run(state: &Arc<AppState>, token: u64, track: String, name: Option<String>) -> Result<Value> {
    let song = state.graph.name().ok_or_else(|| anyhow!("no project is open"))?;
    let slot = Slot::take(state, &song)
        .ok_or_else(|| anyhow!("A song is being exported or bounced. Wait for it to finish, then bounce this track."))?;
    let bouncing = Bouncing::now(state);
    let plan = state.graph.bounce_plan(token, track.clone(), name)?;
    let about = json!({"t": "bounce", "song": song, "dir": plan.dir, "track": track, "label": plan.label});
    let tell = |more: Value| state.send(with(&about, more));
    tell(json!({"progress": 0.0}));
    let mut last = Instant::now();
    let render = plan.render.stop_when(slot.stop());
    let written = plan.bounce.write(render, &plan.dir, &plan.asset, |p| {
        if p < 1.0 && last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            tell(json!({"progress": p}));
        }
    });
    let placed = written
        .map_err(crate::graph::plainly)
        .and_then(|bounced| state.graph.bounce_place(token, track, bounced.clone()).inspect_err(|_| bounced.remove()));
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
    if !state.exports.bouncing.load(Ordering::SeqCst) {
        return true;
    }
    state.exports.stop(Duration::from_secs(10))
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
