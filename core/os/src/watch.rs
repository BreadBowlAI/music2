//! File-system events for the project watcher (WRITE-19): the names of paths that changed, as
//! hints. Never contents, never facts: what a path holds is always read from the disk, and the
//! watcher's walks find what an event never named. macOS: FSEvents. Linux: inotify.
//!
//! What to make of a hint (which paths count, how many, when to walk) is the caller's (PLAT-3).

use std::io;
use std::path::Path;

use crate::os;

/// What a source heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard<'a> {
    /// Something at this path, or under it, changed. Absolute, spelled as the source spells it
    /// (the root as it was given, then the names on the disk).
    Path(&'a Path),
    /// Events were lost, or the watched folder itself moved or went: only a walk tells.
    Rescan,
}

/// What a source tells: on the source's own thread, or inside [`Events::flush`].
pub type Hear = Box<dyn Fn(Heard<'_>) + Send + Sync>;

/// File-system events on one folder and the folders under it, for as long as this lives.
pub struct Events(os::watch::Events);

impl Events {
    /// Watch `root` (canonical) and every folder under it whose path relative to `root`
    /// (`/`-separated) `enter` allows. A source may still name paths in other folders (FSEvents
    /// watches the whole tree), so the caller filters too. `heard` runs on the source's own thread,
    /// or inside `flush`, never once drop has returned. An error when events won't start here: the
    /// caller walks, and says why.
    pub fn start(root: &Path, enter: fn(&str) -> bool, heard: Hear) -> io::Result<Events> {
        os::watch::Events::start(root, enter, heard).map(Events)
    }

    /// Return once every event for a change made before this call has been given to `heard`.
    /// Never call it while holding a lock that `heard` takes.
    pub fn flush(&self) {
        self.0.flush()
    }
}

/// The watch tests share the process's source: one that disturbs every watcher in it (an
/// overflow) takes this for writing, the others for reading.
#[cfg(test)]
pub(crate) static TESTS: std::sync::RwLock<()> = std::sync::RwLock::new(());

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::*;

    /// A fresh folder, canonical, removed when dropped.
    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!("m2-os-watch-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p.canonicalize().unwrap())
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// What a source heard: `None` for a rescan.
    type Log = Arc<Mutex<Vec<Option<PathBuf>>>>;

    fn start(root: &Path) -> (Events, Log) {
        let log: Log = Arc::default();
        let l = log.clone();
        let heard: Hear = Box::new(move |h| {
            l.lock().unwrap().push(match h {
                Heard::Path(p) => Some(p.to_path_buf()),
                Heard::Rescan => None,
            })
        });
        (Events::start(root, |rel| !rel.starts_with('.') && !rel.contains("/."), heard).unwrap(), log)
    }

    /// Whether `log` named `p` (or a folder it is in) within 5 s, flushing as it waits.
    fn named(events: &Events, log: &Log, p: &Path) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(5) {
            events.flush();
            if log.lock().unwrap().iter().flatten().any(|h| p.starts_with(h)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    // WRITE-19: a write is named once flush returns, in the folder watched at start and in a
    // folder made and filled after it, at any depth.
    #[test]
    fn a_write_is_named_by_the_time_flush_returns() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        std::fs::create_dir_all(dir.0.join("data/kick")).unwrap();
        let (events, log) = start(&dir.0);
        for (i, rel) in ["song.ts", "data/kick/hits.json", "lib/a/b/c.ts"].into_iter().enumerate() {
            let p = dir.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, format!("{i}")).unwrap();
            assert!(named(&events, &log, &p), "{rel}: {:?}", log.lock().unwrap());
            log.lock().unwrap().clear();
            // Written again, in a folder that may have been made after the watch started.
            std::fs::write(&p, format!("{i} again")).unwrap();
            events.flush();
            assert!(log.lock().unwrap().iter().flatten().any(|h| p.starts_with(h)), "{rel} again: {:?}", log.lock().unwrap());
            log.lock().unwrap().clear();
        }
    }

    // WRITE-19: a folder renamed under the root is watched under its new name.
    #[test]
    fn a_renamed_folder_is_watched_under_its_new_name() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        std::fs::create_dir_all(dir.0.join("lib/inner")).unwrap();
        let (events, log) = start(&dir.0);
        std::fs::rename(dir.0.join("lib"), dir.0.join("src")).unwrap();
        events.flush();
        log.lock().unwrap().clear();
        let p = dir.0.join("src/inner/x.ts");
        std::fs::write(&p, "x").unwrap();
        events.flush();
        let heard = log.lock().unwrap().clone();
        assert!(heard.iter().flatten().any(|h| *h == p), "{heard:?}");
        assert!(!heard.iter().flatten().any(|h| h.starts_with(dir.0.join("lib"))), "never the old name: {heard:?}");
    }

    // WRITE-19: the root itself going is a rescan; nothing is heard after drop.
    #[test]
    fn the_root_going_is_a_rescan_and_drop_ends_it() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        let root = dir.0.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let (events, log) = start(&root);
        std::fs::rename(&root, dir.0.join("moved")).unwrap();
        let t = Instant::now();
        while !log.lock().unwrap().contains(&None) && t.elapsed() < Duration::from_secs(5) {
            events.flush();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(log.lock().unwrap().contains(&None), "{:?}", log.lock().unwrap());
        drop(events);
        let n = log.lock().unwrap().len();
        std::fs::write(dir.0.join("moved/x.ts"), "x").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(log.lock().unwrap().len(), n);
    }

    #[test]
    fn events_are_send_and_sync() {
        fn both<T: Send + Sync>() {}
        both::<Events>();
    }
}
