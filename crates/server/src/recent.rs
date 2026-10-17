//! The songs opened lately, newest first (polish C2, Q17): `~/.music2/recent.json`, this machine's
//! and never a song's. The start screen lists them, File ▸ Open Recent shows ten, and the app opens
//! the newest at launch when it was still open when the app quit (Q18, X22).
//!
//! ```json
//! {"songs": [{"path": "/abs/Night Drive", "opened": "2026-10-17T09:00:00Z"}], "open": true, "reopening": null}
//! ```
//! - `open`: the newest song was open when the list was last written. Close Song makes it false,
//!   so a song closed before quitting doesn't open at the next launch.
//! - `reopening`: the song a launch was opening, until it opened. Still there at the next launch,
//!   the app closed while opening it, so that launch doesn't try again (the crash guard).
//!
//! Written whole, through a temporary file renamed over it, one change at a time. A file that
//! doesn't read is an empty list. The file is given, never found here: tests pass a temporary
//! one, so none of them writes the real `~/.music2`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// How many songs the list keeps.
pub const KEEP: usize = 20;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Kept {
    #[serde(default)]
    songs: Vec<Entry>,
    #[serde(default)]
    open: bool,
    #[serde(default)]
    reopening: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    /// When it was last opened, in UTC (RFC 3339, to the second).
    opened: String,
}

/// A song as the start screen lists it: its folder, its name and first tempo (read from its
/// `music2.toml` and `data/arrangement.json` now), when it was opened, and whether its folder is
/// gone ("Moved or deleted").
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Song {
    pub path: PathBuf,
    pub name: String,
    pub tempo: Option<f64>,
    pub opened: String,
    pub missing: bool,
}

/// What a launch with no folder given opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reopen {
    /// The newest song: it was open when the app quit.
    Song(PathBuf),
    /// Nothing: the app closed while opening this one at the last launch.
    Crashed(PathBuf),
    Nothing,
}

/// The list in its file.
pub struct Recent {
    file: PathBuf,
    lock: Mutex<()>,
}

/// Each write's temporary file has a name of its own.
static TEMP: AtomicU64 = AtomicU64::new(0);

impl Recent {
    pub fn new(file: PathBuf) -> Recent {
        Recent { file, lock: Mutex::new(()) }
    }

    /// The file it is kept in.
    pub fn file(&self) -> &Path {
        &self.file
    }

    fn read(&self) -> Kept {
        std::fs::read(&self.file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write(&self, kept: &Kept) -> Result<()> {
        let dir = self.file.parent().context("the recent list's file has no folder")?;
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        let tmp = dir.join(format!("recent.json.{}.{}.tmp", std::process::id(), TEMP.fetch_add(1, Ordering::Relaxed)));
        let text = serde_json::to_string_pretty(kept).expect("JSON") + "\n";
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.file).with_context(|| format!("writing {}", self.file.display()))
    }

    /// Change the list, one change at a time; written only when it changed.
    fn change<T>(&self, f: impl FnOnce(&mut Kept) -> T) -> Result<T> {
        let _one = self.lock.lock();
        let mut kept = self.read();
        let before = kept.clone();
        let out = f(&mut kept);
        if kept != before {
            self.write(&kept)?;
        }
        Ok(out)
    }

    /// The songs, newest first, as the start screen shows them.
    pub fn list(&self) -> Vec<Song> {
        self.read().songs.into_iter().map(|e| listed(e.path, e.opened)).collect()
    }

    /// The songs' folders, newest first.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.read().songs.into_iter().map(|e| e.path).collect()
    }

    /// `dir` was opened: it goes first, once, and the list keeps [`KEEP`]. It is open now.
    pub fn opened(&self, dir: &Path) -> Result<()> {
        let path = folder(dir);
        let opened = now();
        self.change(|k| {
            k.songs.retain(|e| e.path != path);
            k.songs.insert(0, Entry { path, opened });
            k.songs.truncate(KEEP);
            k.open = true;
        })
    }

    /// The open song was closed (Close Song): the next launch doesn't open it.
    pub fn closed(&self) -> Result<()> {
        self.change(|k| k.open = false)
    }

    /// Take `dir` off the list.
    pub fn forget(&self, dir: &Path) -> Result<()> {
        let path = folder(dir);
        self.change(|k| k.songs.retain(|e| e.path != path && e.path != dir))
    }

    /// Take every song off the list (Open Recent ▸ Clear Menu).
    pub fn clear(&self) -> Result<()> {
        self.change(|k| {
            k.songs.clear();
            k.open = false;
        })
    }

    /// The song in `old` is in `new` now (its folder renamed): it keeps its place.
    pub fn moved(&self, old: &Path, new: &Path) -> Result<()> {
        let (from, to) = (folder(old), folder(new));
        self.change(|k| {
            k.songs.retain(|e| e.path != to);
            for e in &mut k.songs {
                if e.path == from || e.path == old {
                    e.path = to.clone();
                }
            }
        })
    }

    /// What a launch with no folder given opens: the newest song if it was open when the app quit
    /// and its folder is there, unless the last launch closed while opening a song. That one isn't
    /// tried again, nor at the launch after (it isn't open any more).
    pub fn to_reopen(&self) -> Reopen {
        self.change(|k| {
            if let Some(dir) = k.reopening.take() {
                k.open = false;
                return Reopen::Crashed(dir);
            }
            match k.songs.first() {
                Some(e) if k.open && e.path.join("song.ts").is_file() => Reopen::Song(e.path.clone()),
                _ => Reopen::Nothing,
            }
        })
        .unwrap_or(Reopen::Nothing)
    }

    /// A launch is about to open `dir`: if the app closes before [`reopened`](Self::reopened),
    /// the next launch knows.
    pub fn reopening(&self, dir: &Path) -> Result<()> {
        let path = dir.to_path_buf();
        self.change(|k| k.reopening = Some(path))
    }

    /// The launch's song opened (or failed to, and said why): the guard is off.
    pub fn reopened(&self) -> Result<()> {
        self.change(|k| k.reopening = None)
    }
}

/// `dir` as the list keeps it: with links and `..` resolved while it is there.
fn folder(dir: &Path) -> PathBuf {
    dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
}

/// A song of the list, read from its folder now.
fn listed(path: PathBuf, opened: String) -> Song {
    let missing = !path.join("song.ts").is_file();
    let named = std::fs::read_to_string(path.join(m2_song::path::MANIFEST))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("project")?.get("name")?.as_str().map(str::to_string))
        .filter(|n| !n.trim().is_empty());
    let name = named.unwrap_or_else(|| path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned()));
    let tempo = (!missing).then(|| crate::project::arrangement(&path).tempo.first().map(|t| t.bpm)).flatten();
    Song { path, name, tempo, opened, missing }
}

/// Now, in UTC: `2026-10-17T09:00:00Z`.
fn now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    utc(secs)
}

/// `secs` after 1970 as an RFC 3339 time in UTC (Howard Hinnant's days-to-civil).
fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
#[path = "recent_tests.rs"]
mod tests;
