//! File-system events: which paths under the project folder changed, as hints for the watcher
//! (WRITE-19). Only names: what a path holds is always read from the disk, and a poll that sees
//! the whole project is what makes a batch ready, so a late or lost event delays a change at most.
//!
//! The OS's events come from [`m2_os::watch`] (FSEvents on macOS, inotify on Linux); what they
//! mean to the watcher is here, the same on every OS.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_os::watch::{Events, Heard};
use parking_lot::Mutex;

use super::watcher::Wake;

/// Past this many paths in one tick the watcher walks everything instead (a checkout's burst).
pub(crate) const MAX_HINTS: usize = 4096;

/// What the source said since the watcher last took it.
#[derive(Debug, Default)]
pub(crate) struct Hints {
    /// Project paths, `/`-separated.
    pub paths: BTreeSet<String>,
    /// Events were lost or too many came, or the project folder itself moved: only a walk of
    /// everything tells what changed.
    pub rescan: bool,
    /// Events delivered since the source started, for the gates.
    pub events: u64,
}

/// What the source's thread writes to, under its own lock, never the watch lock.
struct Shared {
    /// The project folder as events spell it.
    root: PathBuf,
    hints: Mutex<Hints>,
    /// Told when a hint is added, so an idle watcher ticks at once.
    wake: Arc<Wake>,
}

impl Shared {
    fn heard(&self, heard: Heard<'_>) {
        let said = {
            let mut h = self.hints.lock();
            h.events += 1;
            self.hint(&mut h, heard)
        };
        if said {
            self.wake.wake();
        }
    }

    /// Add what an event says to `h`: whether it says something.
    fn hint(&self, h: &mut Hints, heard: Heard<'_>) -> bool {
        let path = match heard {
            Heard::Path(p) => p,
            Heard::Rescan => {
                h.rescan = true;
                return true;
            }
        };
        // Elsewhere: another spelling of the folder than the one it was opened by, say. Only a
        // walk can tell.
        let Ok(rel) = path.strip_prefix(&self.root) else {
            h.rescan = true;
            return true;
        };
        // A name that isn't UTF-8 is no project path, nor is anything under it.
        if rel.to_str().is_none() {
            return false;
        }
        let rel = m2_song::path::slashed(rel);
        // The folder itself: a change to its own entry says nothing. What a walk never enters
        // (`.music2`, `.git`, `node_modules`) can hold nothing tracked: the session's own journal
        // and git's objects never make a hint.
        if rel.is_empty() || !m2_session::watch::walked(&rel) {
            return false;
        }
        h.paths.insert(rel);
        if h.paths.len() > MAX_HINTS {
            h.paths.clear();
            h.rescan = true;
        }
        true
    }
}

/// The OS's events on the project folder, as hints.
pub(crate) struct Stream {
    /// Dropped first: nothing is heard once it is.
    events: Events,
    shared: Arc<Shared>,
}

impl Stream {
    /// Start one on `dir` (its spelling as the disk has it: canonical), watching every folder a
    /// walk enters. An error when the OS's events won't start here.
    pub fn start(dir: &Path, wake: Arc<Wake>) -> io::Result<Stream> {
        let shared = Arc::new(Shared { root: dir.to_path_buf(), hints: Mutex::default(), wake });
        let s = shared.clone();
        let events = Events::start(dir, m2_session::watch::walked, Box::new(move |h| s.heard(h)))?;
        Ok(Stream { events, shared })
    }

    /// Everything said since the last take.
    pub fn take(&self) -> Hints {
        let mut h = self.shared.hints.lock();
        let events = h.events;
        let taken = std::mem::take(&mut *h);
        h.events = events;
        taken
    }

    /// Put back what a tick took and didn't use.
    pub fn put_back(&self, hints: Hints) {
        let mut h = self.shared.hints.lock();
        h.rescan |= hints.rescan;
        h.paths.extend(hints.paths);
        if h.paths.len() > MAX_HINTS {
            h.paths.clear();
            h.rescan = true;
        }
    }

    /// Have every event that happened before now delivered: an event still on its way is then in
    /// the hints. Never called with the hints' lock held, which the delivery takes.
    pub fn flush(&self) {
        self.events.flush()
    }

    /// Events delivered so far.
    pub fn events(&self) -> u64 {
        self.shared.hints.lock().events
    }
}
