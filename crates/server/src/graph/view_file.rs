//! `.music2/view.json`: how this machine last saw the song (design-8b §2.2). Local (HOME table):
//! never a Tx, never an undo step, never shared. It keeps the loop range, read when the project
//! opens, and the page's view settings (polish Q16, [`KEYS`]): each view's grid and snap, the
//! audition toggle, the zoom, follow, track heights and hidden lanes. Entries it doesn't know are
//! kept as read.
//!
//! Written whole, through a temporary file in `.music2/` renamed over it, so a reader never sees
//! half of one. One write at a time (the loop's route and the settings' route run on different
//! threads), so neither loses the other's entry. A file that doesn't read is written over: it
//! holds nothing the song needs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

use super::{Graph, Inner};

/// The view settings the page keeps in `view.json`, by name: nothing else is taken from it.
pub const KEYS: [&str; 6] = ["zoom", "follow", "heights", "hidden", "grid", "audition"];

/// Held over each read, change and rename of the file.
static WRITING: Mutex<()> = Mutex::new(());
/// Each write's temporary file has a name of its own.
static TEMP: AtomicU64 = AtomicU64::new(0);

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

/// Merge `changes` into the file: each entry set, or removed where it is null. Every other entry
/// is kept. Nothing is written when nothing changes.
pub(super) fn set(dir: &Path, changes: &Map<String, Value>) -> Result<()> {
    let _one = WRITING.lock();
    let before = read(dir);
    let view = merged(before.clone(), changes);
    if view == before {
        return Ok(());
    }
    let to = file(dir);
    let local = to.parent().expect("in .music2");
    std::fs::create_dir_all(local).with_context(|| format!("making {}", local.display()))?;
    let tmp = local.join(format!("view.json.{}.{}.tmp", std::process::id(), TEMP.fetch_add(1, Ordering::Relaxed)));
    let text = serde_json::to_string_pretty(&Value::Object(view)).expect("JSON") + "\n";
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &to).with_context(|| format!("writing {}", to.display()))
}

/// `view` with `changes` in it: null removes an entry.
fn merged(mut view: Map<String, Value>, changes: &Map<String, Value>) -> Map<String, Value> {
    for (k, v) in changes {
        if v.is_null() {
            view.remove(k);
        } else {
            view.insert(k.clone(), v.clone());
        }
    }
    view
}

/// The loop it had: its range in beats, and whether it was on.
pub(super) fn looped(dir: &Path) -> Option<((f64, f64), bool)> {
    let l = read(dir).remove("loop")?;
    let (start, end) = (l.get("start")?.as_f64()?, l.get("end")?.as_f64()?);
    Some(((start, end), l.get("on").and_then(Value::as_bool).unwrap_or(false)))
}

/// The open song's view settings as the page reads them (`GraphState.local`): `view.json`'s
/// [`KEYS`] as they were when the song opened, with what windows set since. Kept here too, so a
/// window open read-only keeps them in memory, as it keeps its mutes (design-8b §2.2).
#[derive(Default)]
pub(super) struct Local {
    /// The project they are of (its session token); 0 before the first is read.
    token: u64,
    values: Map<String, Value>,
    /// Bumped when the server changes them itself, not at a window's own set: the page reads them
    /// again when it moves (a track's ID changing, say).
    stamp: u64,
}

impl Local {
    /// The settings of project `token` in `dir`, read from the file the first time it is asked.
    fn of(&mut self, token: u64, dir: &Path) -> &mut Local {
        if self.token != token {
            let mut values = read(dir);
            values.retain(|k, _| KEYS.contains(&k.as_str()));
            *self = Local { token, values, stamp: self.stamp + 1 };
        }
        self
    }
}

impl Inner {
    /// The open project's view settings and their stamp, as `state()` publishes them.
    pub(super) fn local(&self, token: u64, dir: &Path) -> (Value, u64) {
        let mut local = self.local.lock();
        let l = local.of(token, dir);
        (Value::Object(l.values.clone()), l.stamp)
    }
}

impl Graph {
    /// Keep view settings of the open song (polish Q16): `set` merges into them, null removes one.
    /// Only [`KEYS`]: the loop has its own route. Written to `.music2/view.json` at once; in a
    /// read-only window kept in memory only. Never a Tx, never an undo step. Answers with them all.
    pub fn set_view(&self, token: u64, changes: Map<String, Value>) -> Result<Value> {
        for k in changes.keys() {
            if k == "loop" {
                bail!("The loop is kept by its own route: /api/graph/loop.");
            }
            if !KEYS.contains(&k.as_str()) {
                bail!("\"{k}\" isn't a view setting. These are: {}.", KEYS.join(", "));
            }
        }
        let (dir, read_only) = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref().filter(|p| p.engine.token == token).context("the open song changed; refresh first")?;
            (p.shown.dir.clone(), p.shown.read_only.is_some())
        };
        let values = {
            let mut local = self.inner.local.lock();
            let l = local.of(token, &dir);
            l.values = merged(std::mem::take(&mut l.values), &changes);
            l.values.clone()
        };
        if !read_only {
            set(&dir, &changes)?;
        }
        Ok(json!(values))
    }
}

#[cfg(test)]
#[path = "view_file_tests.rs"]
mod tests;
