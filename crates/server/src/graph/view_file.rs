//! `.music2/view.json`: how this machine last saw the song (design-8b §2.2). Local (HOME table):
//! never a Tx, never an undo step, never shared. It keeps the loop range, read when the project
//! opens, and the page's view settings ([`KEYS`]: the zoom, the track heights…), which the page
//! reads in the state's `local` and sets through `POST /api/graph/view`. Entries it doesn't know
//! (a newer version's) are kept as read.
//!
//! Written whole, through a temporary file in `.music2/` renamed over it, so a reader never sees
//! half of one; one write at a time in this process, so the loop's route and the view route never
//! lose each other's keys. A file that doesn't read is written over: it holds nothing the song
//! needs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

use super::Graph;

/// The page's view settings, the keys `POST /api/graph/view` takes (Q16): the timeline's zoom
/// (px a beat), whether it follows the playhead, track and lane heights by key, the automation
/// lanes hidden, each view's grid and snap, and the audition toggle. `loop` has its own route.
pub const KEYS: [&str; 6] = ["zoom", "follow", "heights", "hidden", "grid", "audition"];

/// Held over each write's read, change and rename.
static WRITING: Mutex<()> = parking_lot::const_mutex(());
/// Each write's own temporary name.
static TMP: AtomicU64 = AtomicU64::new(0);

fn file(dir: &Path) -> PathBuf {
    dir.join(m2_song::path::LOCAL).join("view.json")
}

/// What the file holds; empty when there is none, or it doesn't read.
pub(super) fn read(dir: &Path) -> Map<String, Value> {
    std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// Set `key` to `value`, keeping every other entry.
pub(super) fn write(dir: &Path, key: &str, value: Value) -> Result<()> {
    set(dir, &Map::from_iter([(key.to_string(), value)]))
}

/// Merge `changes` in, a null taking its key out, keeping every other entry.
pub(super) fn set(dir: &Path, changes: &Map<String, Value>) -> Result<()> {
    let _one = WRITING.lock();
    let mut view = read(dir);
    let before = view.clone();
    merge(&mut view, changes);
    if view == before {
        return Ok(());
    }
    let to = file(dir);
    let local = to.parent().expect("in .music2");
    std::fs::create_dir_all(local).with_context(|| format!("making {}", local.display()))?;
    let tmp = local.join(format!("view.json.{}.{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
    let text = serde_json::to_string_pretty(&Value::Object(view)).expect("JSON") + "\n";
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &to).with_context(|| format!("writing {}", to.display()))
}

/// `changes` merged into `into`: a null takes its key out.
fn merge(into: &mut Map<String, Value>, changes: &Map<String, Value>) {
    for (k, v) in changes {
        match v {
            Value::Null => into.remove(k),
            v => into.insert(k.clone(), v.clone()),
        };
    }
}

/// The page's view settings as the file has them: [`KEYS`] only.
pub(super) fn local(dir: &Path) -> Map<String, Value> {
    read(dir).into_iter().filter(|(k, _)| KEYS.contains(&k.as_str())).collect()
}

/// The loop it had: its range in beats, and whether it was on.
pub(super) fn looped(dir: &Path) -> Option<((f64, f64), bool)> {
    let l = read(dir).remove("loop")?;
    let (start, end) = (l.get("start")?.as_f64()?, l.get("end")?.as_f64()?);
    Some(((start, end), l.get("on").and_then(Value::as_bool).unwrap_or(false)))
}

impl Graph {
    /// Set the page's view settings (Q16): `changes` merged into what the open song's `local`
    /// holds, a null taking its key out, and kept in `.music2/view.json`. Never a Tx, never an
    /// undo step. A read-only window keeps them in memory only, as the loop (design-8b §2.2).
    /// Only [`KEYS`]. Answers `{local}`.
    pub fn set_view(&self, token: u64, changes: Map<String, Value>) -> Result<Value> {
        if let Some(k) = changes.keys().find(|k| !KEYS.contains(&k.as_str())) {
            bail!("{k} isn't a view setting: {}", KEYS.join(", "));
        }
        let (dir, keep, local) = {
            let mut cp = self.inner.control.lock();
            let p = cp.open.as_mut().filter(|p| p.engine.token == token).context("the open project changed; refresh first")?;
            merge(&mut p.local, &changes);
            (p.shown.dir.clone(), p.shown.read_only.is_none(), p.local.clone())
        };
        if keep {
            set(&dir, &changes)?;
        }
        Ok(json!({ "local": local }))
    }
}

#[cfg(test)]
#[path = "view_file_tests.rs"]
mod tests;
