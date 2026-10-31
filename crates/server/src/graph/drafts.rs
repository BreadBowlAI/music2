//! The code editor's text not yet saved (WRITE-27): kept in `.music2/drafts/` as it is typed, and
//! offered back when the song next opens, after a crash or any close that left it.
//!
//! One file a code file, `<16 hex digits of the path's SHA-256>.json`, holding `{file, base, text}`:
//! the path, the hash of the file's text the editor was in step with, and its text. Written whole,
//! through a temporary file pushed and renamed over it, so a crash leaves the last one or the one
//! before, never half. Local (HOME table): never the song's files (WRITE-1), never a Tx, never an
//! undo step.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use m2_song::tx::TxPath;
use m2_song::{Class, Sha};
use serde::Serialize;
use serde_json::{Value, json};

fn folder(dir: &Path) -> PathBuf {
    dir.join(m2_song::path::LOCAL).join("drafts")
}

fn file_of(dir: &Path, file: &str) -> PathBuf {
    folder(dir).join(format!("{}.json", &Sha::of(file.as_bytes()).hex()[..16]))
}

/// A draft found when the song opened: its file, its text, and whether the file changed since the
/// text was based on it (or is gone).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Left {
    pub file: String,
    pub text: String,
    pub changed: bool,
}

/// Keep `text` as the draft of code file `file`, based on `base`, the text the editor was in step
/// with; None takes it out.
pub(super) fn keep(dir: &Path, file: &str, base: &str, text: Option<&str>) -> Result<()> {
    if !TxPath::new(file).is_ok_and(|p| p.class() == Class::Code) {
        bail!("{file} isn't a code file of the song");
    }
    let to = file_of(dir, file);
    let Some(text) = text else {
        return match std::fs::remove_file(&to) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("removing {}", to.display())),
            _ => Ok(()),
        };
    };
    let folder = folder(dir);
    std::fs::create_dir_all(&folder).with_context(|| format!("making {}", folder.display()))?;
    static N: AtomicU64 = AtomicU64::new(0);
    let tmp = folder.join(format!(".draft.{}.{}.part", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let bytes = serde_json::to_vec(&json!({"file": file, "base": Sha::of(base.as_bytes()).prefixed(), "text": text})).expect("JSON");
    let done = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(&bytes)?;
        m2_os::fs::push(&f)?;
        std::fs::rename(&tmp, &to)
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done.with_context(|| format!("writing {}", to.display()))
}

/// The drafts the project at `dir` has, each with whether its file changed since. A draft whose
/// text is the file's is taken out without a word; one that doesn't read, and what a write killed
/// half way left, go too. Sorted by file.
pub(super) fn left(dir: &Path) -> Vec<Left> {
    let Ok(entries) = std::fs::read_dir(folder(dir)) else { return vec![] };
    let mut out = vec![];
    for e in entries.flatten() {
        let path = e.path();
        let read = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let draft = read.as_ref().and_then(|v| {
            let file = v.get("file")?.as_str()?;
            let base = Sha::parse_prefixed(v.get("base")?.as_str()?)?;
            let text = v.get("text")?.as_str()?;
            let ours = TxPath::new(file).is_ok_and(|p| p.class() == Class::Code) && file_of(dir, file) == path;
            ours.then(|| (file.to_string(), base, text.to_string()))
        });
        let Some((file, base, text)) = draft else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        let now = std::fs::read(dir.join(&file)).ok();
        if now.as_deref() == Some(text.as_bytes()) {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        out.push(Left { changed: now.map(|b| Sha::of(&b)) != Some(base), file, text });
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

impl super::Graph {
    /// Keep the code editor's unsaved text of `file` in project `token` (WRITE-27), based on
    /// `base`, the text it was in step with; None takes it out. A draft left from before is
    /// answered by it: restored (kept again) or discarded. A read-only window keeps none.
    pub fn set_draft(&self, token: u64, file: &str, base: &str, text: Option<&str>) -> Result<()> {
        let dir = {
            let mut cp = self.inner.control.lock();
            let p = cp.open.as_mut().filter(|p| p.engine.token == token).context("the open project changed; refresh first")?;
            if p.shown.read_only.is_some() {
                return Ok(());
            }
            p.drafts.retain(|d| d.file != file);
            p.shown.dir.clone()
        };
        keep(&dir, file, base, text)
    }
}

#[cfg(test)]
#[path = "drafts_tests.rs"]
mod tests;
