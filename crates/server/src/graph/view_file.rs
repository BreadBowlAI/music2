//! `.music2/view.json`: how this machine last saw the song (design-8b §2.2). Local (HOME table):
//! never a Tx, never an undo step, never shared. Today it keeps the loop range, read when the
//! project opens; entries it doesn't know (a newer version's zoom) are kept as read.
//!
//! Written whole, through a temporary file in `.music2/` renamed over it, so a reader never sees
//! half of one. A file that doesn't read is written over: it holds nothing the song needs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

fn file(dir: &Path) -> PathBuf {
    dir.join(m2_song::path::LOCAL).join("view.json")
}

/// What the file holds; empty when there is none, or it doesn't read.
pub(super) fn read(dir: &Path) -> Map<String, Value> {
    std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// Set `key` to `value`, keeping every other entry.
pub(super) fn write(dir: &Path, key: &str, value: Value) -> Result<()> {
    let mut view = read(dir);
    if view.get(key) == Some(&value) {
        return Ok(());
    }
    view.insert(key.to_string(), value);
    let to = file(dir);
    let local = to.parent().expect("in .music2");
    std::fs::create_dir_all(local).with_context(|| format!("making {}", local.display()))?;
    let tmp = local.join(format!("view.json.{}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(&Value::Object(view)).expect("JSON") + "\n";
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &to).with_context(|| format!("writing {}", to.display()))
}

/// The loop it had: its range in beats, and whether it was on.
pub(super) fn looped(dir: &Path) -> Option<((f64, f64), bool)> {
    let l = read(dir).remove("loop")?;
    let (start, end) = (l.get("start")?.as_f64()?, l.get("end")?.as_f64()?);
    Some(((start, end), l.get("on").and_then(Value::as_bool).unwrap_or(false)))
}
