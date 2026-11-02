//! The code editor's text not yet saved (WRITE-27): kept in `.music2/drafts/` as it is typed, and
//! offered back when the song next opens for writing, after a crash or any close that left it.
//!
//! Each window keeps its own draft of each file, `<16 hex digits of the path's SHA-256>-<16 of its
//! window's>.json`, holding `{file, base, text}`: the path, the hash of the file's text the editor
//! was in step with, and its text. Written whole (`m2_os::fs::replace`), so a crash leaves the last
//! one or the one before, never half. When the song opens for writing (or a read-only window takes
//! the lock), the drafts there are moved aside, into `left/`: those are the ones offered back, and
//! each stays until Restore or Discard answers it, whatever is typed meanwhile. Local (HOME table):
//! never the song's files (WRITE-1), never a Tx, never an undo step.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use m2_song::tx::TxPath;
use m2_song::{Class, Sha};
use serde::Serialize;
use serde_json::{Value, json};

fn folder(dir: &Path) -> PathBuf {
    dir.join(m2_song::path::LOCAL).join("drafts")
}

fn left_folder(dir: &Path) -> PathBuf {
    folder(dir).join("left")
}

/// The first 16 hex digits of `s`'s SHA-256.
fn short(s: &str) -> String {
    Sha::of(s.as_bytes()).hex()[..16].to_string()
}

/// A draft found when the song opened: its id (its name in `left/`), its file, its text, and
/// whether the file changed since the text was based on it (or is gone).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Left {
    pub id: String,
    pub file: String,
    pub text: String,
    pub changed: bool,
}

/// Keep `text` as window `window`'s draft of code file `file`, based on `base`, the text the
/// editor was in step with; None takes that window's draft out.
pub(super) fn keep(dir: &Path, window: &str, file: &str, base: &str, text: Option<&str>) -> Result<()> {
    if !TxPath::new(file).is_ok_and(|p| p.class() == Class::Code) {
        bail!("{file} isn't a code file of the song");
    }
    let to = folder(dir).join(format!("{}-{}.json", short(file), short(window)));
    let Some(text) = text else {
        return match std::fs::remove_file(&to) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("removing {}", to.display())),
            _ => Ok(()),
        };
    };
    std::fs::create_dir_all(folder(dir)).with_context(|| format!("making {}", folder(dir).display()))?;
    let bytes = serde_json::to_vec(&json!({"file": file, "base": Sha::of(base.as_bytes()).prefixed(), "text": text})).expect("JSON");
    m2_os::fs::replace(&to, &bytes).with_context(|| format!("writing {}", to.display()))
}

/// Move the drafts of the project at `dir` aside, into `left/`, and read every draft there: what
/// the song opens with. A draft whose text is the file's is taken out without a word; one that
/// doesn't read, and what a write killed half way left, go too. Sorted by file.
pub(super) fn set_aside(dir: &Path) -> Vec<Left> {
    let left = left_folder(dir);
    if let Ok(entries) = std::fs::read_dir(folder(dir)) {
        for e in entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())) {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".json").filter(|_| !name.starts_with('.')) else {
                let _ = std::fs::remove_file(e.path());
                continue;
            };
            if std::fs::create_dir_all(&left).is_err() {
                continue;
            }
            // One left from before and not answered yet keeps its name: this one gets another.
            let free = (0..).map(|n| if n == 0 { left.join(&name) } else { left.join(format!("{stem}-{n}.json")) }).find(|p| !p.exists());
            if let Some(to) = free {
                let _ = std::fs::rename(e.path(), to);
            }
        }
    }
    let Ok(entries) = std::fs::read_dir(&left) else { return vec![] };
    let mut out = vec![];
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let read = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let draft = read.as_ref().and_then(|v| {
            let file = v.get("file")?.as_str()?;
            let base = Sha::parse_prefixed(v.get("base")?.as_str()?)?;
            let text = v.get("text")?.as_str()?;
            let id = name.strip_suffix(".json").filter(|id| valid(id))?;
            TxPath::new(file).is_ok_and(|p| p.class() == Class::Code).then(|| (id.to_string(), file.to_string(), base, text.to_string()))
        });
        let Some((id, file, base, text)) = draft else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        let now = std::fs::read(dir.join(&file)).ok();
        if now.as_deref() == Some(text.as_bytes()) {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        out.push(Left { changed: now.map(|b| Sha::of(&b)) != Some(base), id, file, text });
    }
    out.sort_by(|a, b| (&a.file, &a.id).cmp(&(&b.file, &b.id)));
    out
}

/// Whether `id` is a left draft's name: lowercase hex digits and dashes.
fn valid(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
}

impl super::Graph {
    /// Keep window `window`'s unsaved text of `file` in project `token` (WRITE-27), based on
    /// `base`, the text it was in step with; None takes that window's draft out. A read-only window
    /// keeps none.
    pub fn set_draft(&self, token: u64, window: &str, file: &str, base: &str, text: Option<&str>) -> Result<()> {
        let dir = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref().filter(|p| p.engine.token == token).context("the open project changed; refresh first")?;
            if p.shown.read_only.is_some() {
                return Ok(());
            }
            p.shown.dir.clone()
        };
        keep(&dir, window, file, base, text)
    }

    /// The draft `id` left from before, restored or discarded (WRITE-27): it is offered no more.
    pub fn answer_draft(&self, token: u64, id: &str) -> Result<()> {
        if !valid(id) {
            bail!("{id} isn't a draft's id");
        }
        let dir = {
            let mut cp = self.inner.control.lock();
            let p = cp.open.as_mut().filter(|p| p.engine.token == token).context("the open project changed; refresh first")?;
            if p.shown.read_only.is_some() {
                return Ok(());
            }
            p.drafts.retain(|d| d.id != id);
            p.shown.dir.clone()
        };
        match std::fs::remove_file(left_folder(&dir).join(format!("{id}.json"))) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).context("removing the draft"),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "drafts_tests.rs"]
mod tests;
