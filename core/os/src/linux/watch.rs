//! Linux: inotify. One instance per process, shared by every watcher in it (the limit of 128
//! instances is per user and shared with editors), read by one thread. inotify watches one folder
//! per watch, so each folder the caller enters is watched before it is listed: a folder made in
//! between is never missed. An event is queued before the writer's call returns, so a flush is a
//! read that never waits.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::unix::cpath;
use crate::watch::{Hear, Heard};

/// `IN_MODIFY` stays, so a file written while open is named, not found by a walk alone.
const MASK: u32 = libc::IN_MODIFY
    | libc::IN_ATTRIB
    | libc::IN_CLOSE_WRITE
    | libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_ONLYDIR
    | libc::IN_DONT_FOLLOW
    | libc::IN_EXCL_UNLINK;

/// How long the reader rests after each read, with the instance free: FSEvents' latency. The first
/// event after a quiet spell goes out at once, and the kernel merges a writer's events meanwhile
/// (1 GB in 4 KB writes woke a reader that never rests about 170,000 times; with the rest, 76).
const PAUSE: Duration = Duration::from_millis(10);

/// The size of an event before its name.
const HEADER: usize = std::mem::size_of::<libc::inotify_event>();

/// The process's one instance, made by the first watcher, and its reader.
struct Instance {
    fd: OwnedFd,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    watchers: HashMap<u64, Watcher>,
    /// What each watch descriptor watches. A folder two watchers share has one.
    folders: HashMap<i32, Folder>,
    next: u64,
}

struct Watcher {
    root: PathBuf,
    /// The root's descriptor; -1 when the root is watched no more.
    root_wd: i32,
    /// What the root's path named when it was last watched, by its volume and inode: a folder put
    /// in its place (a restore from a backup) is a new one to watch.
    root_id: Option<(u64, u64)>,
    enter: fn(&str) -> bool,
    heard: Arc<dyn Fn(Heard<'_>) + Send + Sync>,
    /// The descriptors this watcher holds.
    wds: BTreeSet<i32>,
}

struct Folder {
    /// The path it was last reached by.
    path: PathBuf,
    /// The watchers that hold it: each hears its events, and it is removed when none does.
    watchers: BTreeSet<u64>,
}

static INSTANCE: Mutex<Option<&'static Instance>> = Mutex::new(None);

/// The instance, made with its reader thread the first time. An error (128 instances per user
/// here, shared with editors) is tried again by the next watcher.
fn instance() -> io::Result<&'static Instance> {
    let mut made = INSTANCE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = *made {
        return Ok(i);
    }
    // SAFETY: a plain system call; the descriptor is owned from here on.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` is a new descriptor nothing else owns.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // It lives as long as the process: watchers come and go, and the reader blocks while none do.
    let i: &'static Instance = Box::leak(Box::new(Instance { fd, state: Mutex::default() }));
    std::thread::Builder::new().name("m2-watch".into()).spawn(move || i.read())?;
    *made = Some(i);
    Ok(i)
}

impl Instance {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The reader: wait for events, take them in with the instance held, then rest with it free.
    fn read(&self) {
        let fd = self.fd.as_raw_fd();
        loop {
            let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            // SAFETY: one pollfd, which outlives the call.
            if unsafe { libc::poll(&mut p, 1, -1) } > 0 {
                self.drain(&mut self.lock());
            }
            std::thread::sleep(PAUSE);
        }
    }

    /// Read every event queued, and tell each watcher its own, until none is left.
    fn drain(&self, st: &mut State) {
        let fd = self.fd.as_raw_fd();
        let mut buf = [0u8; 16384];
        loop {
            // SAFETY: a read into `buf`, which outlives the call.
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            // EAGAIN: all read.
            if n <= 0 {
                return;
            }
            let bytes = &buf[..n as usize];
            let mut at = 0;
            while at + HEADER <= bytes.len() {
                // SAFETY: the kernel wrote a whole event header here; read unaligned.
                let ev: libc::inotify_event = unsafe { std::ptr::read_unaligned(bytes[at..].as_ptr().cast()) };
                let end = (at + HEADER + ev.len as usize).min(bytes.len());
                // The name is padded with NULs.
                let name = &bytes[at + HEADER..end];
                let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
                st.dispatch(fd, ev.wd, ev.mask, name);
                at = end;
            }
        }
    }
}

impl State {
    fn dispatch(&mut self, fd: RawFd, wd: i32, mask: u32, name: &[u8]) {
        if mask & libc::IN_Q_OVERFLOW != 0 {
            // The queue is the instance's, so every watcher lost events, folders made among them:
            // watch again what is there, and walk.
            for id in self.watchers.keys().copied().collect::<Vec<_>>() {
                self.rebuild(fd, id);
            }
            return;
        }
        if mask & libc::IN_IGNORED != 0 {
            // The kernel let the watch go: its folder went, or it was removed.
            self.forget(wd);
            return;
        }
        let Some(f) = self.folders.get(&wd) else { return };
        let path = if name.is_empty() { f.path.clone() } else { f.path.join(OsStr::from_bytes(name)) };
        let gone = name.is_empty() && mask & (libc::IN_MOVE_SELF | libc::IN_DELETE_SELF | libc::IN_UNMOUNT) != 0;
        let made = mask & libc::IN_ISDIR != 0 && mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0;
        // A folder's mode changed: one that couldn't be read when it was made may be readable now.
        let opened = mask & libc::IN_ISDIR != 0 && mask & libc::IN_ATTRIB != 0 && !name.is_empty();
        for id in f.watchers.iter().copied().collect::<Vec<_>>() {
            let Some(w) = self.watchers.get(&id) else { continue };
            let (heard, root_wd, enter) = (w.heard.clone(), w.root_wd, w.enter);
            if gone {
                if mask & libc::IN_MOVE_SELF != 0 || wd == root_wd {
                    // A folder moved within the project or out of it: its watches, and those under
                    // it, still have the old paths. The root moved or went: whatever is at its path
                    // now is watched, and the old tree let go.
                    self.rebuild(fd, id);
                } else if mask & libc::IN_UNMOUNT != 0 {
                    heard(Heard::Rescan);
                }
                // A folder under the root that went is named by its parent's event.
                continue;
            }
            if (made || opened)
                && let Some(rel) = rel(&self.watchers[&id].root, &path).filter(|r| enter(r))
            {
                // A folder made, moved in, or readable at last: watch it and the folders under it.
                // Naming it then makes the watcher walk it, which finds what was written before the
                // watch. A folder whose mode changed that this watcher holds already is left as it is.
                let held = self.watchers[&id].wds.len();
                let fresh = match self.add(fd, id, &path) {
                    Ok(_) if made || self.watchers[&id].wds.len() > held => self.watch_under(fd, id, &path, &rel),
                    Ok(_) => Ok(()),
                    Err(e) if skipped(&e) => Ok(()),
                    Err(e) => Err(e),
                };
                if fresh.is_err() {
                    heard(Heard::Rescan);
                }
            }
            heard(Heard::Path(&path));
        }
    }

    /// Watch `dir` for watcher `id`: its descriptor, which a folder already watched keeps.
    fn add(&mut self, fd: RawFd, id: u64, dir: &Path) -> io::Result<i32> {
        #[cfg(test)]
        if tests::NO_WATCH_LEFT.lock().unwrap().contains(dir) {
            return Err(io::Error::from_raw_os_error(libc::ENOSPC));
        }
        let c = cpath(dir)?;
        // SAFETY: a NUL-terminated path that outlives the call.
        let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), MASK) };
        if wd < 0 {
            return Err(io::Error::last_os_error());
        }
        let f = self.folders.entry(wd).or_insert_with(|| Folder { path: PathBuf::new(), watchers: BTreeSet::new() });
        // A folder moved within the project keeps its descriptor: its path is the new one.
        f.path = dir.to_path_buf();
        f.watchers.insert(id);
        if let Some(w) = self.watchers.get_mut(&id) {
            w.wds.insert(wd);
        }
        Ok(wd)
    }

    /// Watch every folder under `dir` (watched already) that `enter` allows, each before it is
    /// listed. An error when a watch can't be made (watches or memory ran out) or a folder can't be
    /// listed (no file descriptor left): what is under it is left to the walks, and every other
    /// folder is still watched, so a rebuild never lets go of a folder it could watch.
    fn watch_under(&mut self, fd: RawFd, id: u64, dir: &Path, rel: &str) -> io::Result<()> {
        let Some(enter) = self.watchers.get(&id).map(|w| w.enter) else { return Ok(()) };
        let mut failed = None;
        let mut stack = vec![(dir.to_path_buf(), rel.to_string())];
        while let Some((dir, rel)) = stack.pop() {
            let listed = std::fs::read_dir(&dir);
            #[cfg(test)]
            let listed =
                if tests::NOT_LISTED.lock().unwrap().contains(&dir) { Err(io::Error::from_raw_os_error(libc::EMFILE)) } else { listed };
            let entries = match listed {
                Ok(entries) => entries,
                Err(e) if skipped(&e) => continue,
                Err(e) => {
                    failed = failed.or(Some(e));
                    continue;
                }
            };
            for e in entries.flatten() {
                // A link is never followed, as the walks never follow one.
                if !e.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                // A name that isn't UTF-8 is no project path, nor is anything under it.
                let name = e.file_name();
                let Some(name) = name.to_str() else { continue };
                let rel = if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") };
                if !enter(&rel) {
                    continue;
                }
                let path = e.path();
                match self.add(fd, id, &path) {
                    Ok(_) => stack.push((path, rel)),
                    Err(e) if skipped(&e) => {}
                    Err(e) => failed = failed.or(Some(e)),
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }

    /// Watch watcher `id`'s root and every folder under it again, let go of the descriptors that
    /// walk didn't reach, and tell it to walk.
    fn rebuild(&mut self, fd: RawFd, id: u64) {
        let Some(w) = self.watchers.get_mut(&id) else { return };
        let (root, heard, held) = (w.root.clone(), w.heard.clone(), std::mem::take(&mut w.wds));
        w.root_wd = -1;
        w.root_id = identity(&root);
        if let Ok(wd) = self.add(fd, id, &root) {
            self.watchers.get_mut(&id).expect("registered").root_wd = wd;
            // Out of watches: the folders left unwatched are found by the walks.
            let _ = self.watch_under(fd, id, &root, "");
        }
        let reached = self.watchers[&id].wds.clone();
        for wd in held.difference(&reached) {
            self.release(fd, id, *wd);
        }
        heard(Heard::Rescan);
    }

    /// Watcher `id` holds `wd` no more; the watch goes when no watcher holds it.
    fn release(&mut self, fd: RawFd, id: u64, wd: i32) {
        if let Some(w) = self.watchers.get_mut(&id) {
            w.wds.remove(&wd);
        }
        let Some(f) = self.folders.get_mut(&wd) else { return };
        f.watchers.remove(&id);
        if f.watchers.is_empty() {
            self.folders.remove(&wd);
            // SAFETY: a plain system call on our own descriptor.
            unsafe { libc::inotify_rm_watch(fd, wd) };
        }
    }

    /// The kernel removed `wd`.
    fn forget(&mut self, wd: i32) {
        let Some(f) = self.folders.remove(&wd) else { return };
        for id in f.watchers {
            if let Some(w) = self.watchers.get_mut(&id) {
                w.wds.remove(&wd);
                if w.root_wd == wd {
                    w.root_wd = -1;
                }
            }
        }
    }

    fn unregister(&mut self, fd: RawFd, id: u64) {
        let Some(w) = self.watchers.remove(&id) else { return };
        for wd in w.wds {
            self.release(fd, id, wd);
        }
    }
}

/// A folder gone again, no folder any more, or one that can't be read: no watch, and nothing a
/// walk could read in it either. Its parent's events name it, and its mode's change (`IN_ATTRIB`)
/// when it becomes readable.
fn skipped(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR | libc::EACCES))
}

/// What `root` names, by its volume and inode; None when nothing is there.
fn identity(root: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(root).ok().map(|m| (m.dev(), m.ino()))
}

/// `e` in plain words, `what`, when it is `code`: one of inotify's limits ran out. They are the
/// user's, shared with every program that watches files, and raised with `sysctl`.
fn limit(e: io::Error, code: i32, what: &'static str) -> io::Error {
    if e.raw_os_error() == Some(code) { io::Error::new(e.kind(), what) } else { e }
}

/// `path` relative to `root`, `/`-separated; None outside it or not UTF-8.
fn rel(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root).ok()?.to_str().map(String::from)
}

/// One watcher's part of the instance.
pub(crate) struct Events {
    id: u64,
    instance: &'static Instance,
}

impl Events {
    /// A failed watch at start is an error: the watcher walks, and says why.
    pub(crate) fn start(root: &Path, enter: fn(&str) -> bool, heard: Hear) -> io::Result<Events> {
        let instance = instance()
            .map_err(|e| limit(e, libc::EMFILE, "Linux's limit on programs watching files is reached (fs.inotify.max_user_instances)"))?;
        let fd = instance.fd.as_raw_fd();
        let mut st = instance.lock();
        let id = st.next;
        st.next += 1;
        let heard = Arc::from(heard);
        let root_id = identity(root);
        st.watchers.insert(id, Watcher { root: root.to_path_buf(), root_wd: -1, root_id, enter, heard, wds: BTreeSet::new() });
        let started = st.add(fd, id, root).and_then(|wd| {
            st.watchers.get_mut(&id).expect("registered").root_wd = wd;
            st.watch_under(fd, id, root, "")
        });
        if let Err(e) = started {
            st.unregister(fd, id);
            return Err(limit(e, libc::ENOSPC, "Linux's limit on watched folders is reached (fs.inotify.max_user_watches)"));
        }
        Ok(Events { id, instance })
    }

    /// Under the instance's lock, which every dispatch holds: one in progress ends first. Then a
    /// folder put at the root's path since it was watched (removed and made again, or a copy moved
    /// in) is watched: nothing else tells it, and while a file in the project is held open (the
    /// session's lock), the root's removal makes no event of its own.
    pub(crate) fn flush(&self) {
        let st = &mut self.instance.lock();
        self.instance.drain(st);
        let Some(w) = st.watchers.get(&self.id) else { return };
        // Only a folder that is there and is another: a root that is gone gives no walk at each
        // flush, since each walk flushes.
        if let Some(now) = identity(&w.root)
            && w.root_id != Some(now)
        {
            st.rebuild(self.instance.fd.as_raw_fd(), self.id);
        }
    }
}

impl Drop for Events {
    /// Under the instance's lock: no `heard` runs once this returns.
    fn drop(&mut self) {
        self.instance.lock().unregister(self.instance.fd.as_raw_fd(), self.id);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!("m2-os-inotify-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p.canonicalize().unwrap())
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    type Log = Arc<std::sync::Mutex<Vec<Option<PathBuf>>>>;

    /// Folders whose watch fails as when the user's watches have run out (`ENOSPC`).
    pub(super) static NO_WATCH_LEFT: std::sync::Mutex<BTreeSet<PathBuf>> = std::sync::Mutex::new(BTreeSet::new());
    /// Folders that can't be listed, as when the process has no file descriptor left (`EMFILE`).
    pub(super) static NOT_LISTED: std::sync::Mutex<BTreeSet<PathBuf>> = std::sync::Mutex::new(BTreeSet::new());

    fn start(root: &Path) -> (crate::watch::Events, Log) {
        let log: Log = Arc::default();
        let l = log.clone();
        let heard: Hear = Box::new(move |h| {
            l.lock().unwrap().push(match h {
                Heard::Path(p) => Some(p.to_path_buf()),
                Heard::Rescan => None,
            })
        });
        let enter = |rel: &str| rel.split('/').all(|s| !s.starts_with('.'));
        (crate::watch::Events::start(root, enter, heard).unwrap(), log)
    }

    /// The descriptors that watch something under `dir`, and which watchers hold each.
    fn watched_under(dir: &Path) -> Vec<(PathBuf, usize)> {
        let st = instance().unwrap().lock();
        let mut v: Vec<_> = st.folders.values().filter(|f| f.path.starts_with(dir)).map(|f| (f.path.clone(), f.watchers.len())).collect();
        v.sort();
        v
    }

    fn heard(log: &Log) -> Vec<Option<PathBuf>> {
        log.lock().unwrap().clone()
    }

    // WRITE-19: a folder `enter` refuses (`.music2`, `.git`) is never watched, so the session's
    // own journal and git's objects make no event.
    #[test]
    fn a_folder_enter_refuses_is_not_watched() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        std::fs::create_dir_all(dir.0.join(".music2/tx")).unwrap();
        std::fs::create_dir_all(dir.0.join("data/.hidden")).unwrap();
        let (events, log) = start(&dir.0);
        assert_eq!(watched_under(&dir.0), vec![(dir.0.clone(), 1), (dir.0.join("data"), 1)]);
        std::fs::write(dir.0.join(".music2/tx/journal"), "x").unwrap();
        std::fs::write(dir.0.join("data/.hidden/x"), "x").unwrap();
        events.flush();
        assert_eq!(heard(&log), vec![], "nothing in a folder never entered");
    }

    // WRITE-19: a file written while open is named at each write (`IN_MODIFY`), not only when it
    // is closed, so its first half is never a change only a walk finds.
    #[test]
    fn a_file_written_while_open_is_named_at_each_write() {
        use std::io::Write;
        // The overflow test's rescan would come between the halves.
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        let (events, log) = start(&dir.0);
        let p = dir.0.join("hits.json");
        let mut f = std::fs::File::create(&p).unwrap();
        for half in ["{\"clips\": ", "[]}"] {
            events.flush();
            log.lock().unwrap().clear();
            f.write_all(half.as_bytes()).unwrap();
            events.flush();
            assert_eq!(heard(&log), vec![Some(p.clone())], "{half}");
        }
    }

    // WRITE-19 (one instance per process): two watchers on one folder share its descriptor and
    // both hear it; when one goes, the other's watch stays, and when both have gone, it goes.
    #[test]
    fn two_watchers_share_a_folder_until_both_go() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        std::fs::create_dir_all(dir.0.join("lib")).unwrap();
        let (a, log_a) = start(&dir.0);
        let (b, log_b) = start(&dir.0);
        assert_eq!(watched_under(&dir.0), vec![(dir.0.clone(), 2), (dir.0.join("lib"), 2)]);
        let p = dir.0.join("lib/x.ts");
        std::fs::write(&p, "x").unwrap();
        a.flush();
        assert!(heard(&log_a).contains(&Some(p.clone())) && heard(&log_b).contains(&Some(p.clone())));
        drop(a);
        assert_eq!(watched_under(&dir.0), vec![(dir.0.clone(), 1), (dir.0.join("lib"), 1)]);
        let n = heard(&log_a).len();
        std::fs::write(&p, "y").unwrap();
        b.flush();
        assert_eq!(heard(&log_a).len(), n, "nothing after drop");
        assert!(heard(&log_b).iter().filter(|h| **h == Some(p.clone())).count() >= 2);
        drop(b);
        assert_eq!(watched_under(&dir.0), vec![]);
    }

    // WRITE-19: a folder moved out of the project is watched no more (the rebuild lets its
    // descriptors go, its folder under it's too), and the watcher is told to walk.
    #[test]
    fn a_folder_moved_out_is_let_go_and_walked() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        let root = dir.0.join("project");
        std::fs::create_dir_all(root.join("lib/inner")).unwrap();
        let (events, log) = start(&root);
        assert_eq!(watched_under(&dir.0).len(), 3);
        std::fs::rename(root.join("lib"), dir.0.join("lib")).unwrap();
        events.flush();
        assert!(heard(&log).contains(&None), "a rescan: {:?}", heard(&log));
        assert!(heard(&log).contains(&Some(root.join("lib"))), "{:?}", heard(&log));
        assert_eq!(watched_under(&dir.0), vec![(root.clone(), 1)]);
        log.lock().unwrap().clear();
        std::fs::write(dir.0.join("lib/inner/x.ts"), "x").unwrap();
        events.flush();
        assert_eq!(heard(&log), vec![], "outside the project now");
    }

    // WRITE-19: a folder that can't be watched (the user's watches ran out) is left to the walks,
    // and the watcher is told to walk. A rebuild that meets it again still watches every other
    // folder: it never lets go of one it could watch.
    #[test]
    fn a_folder_that_cant_be_watched_leaves_the_others_watched() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        for n in ["a", "b", "c", "d"] {
            std::fs::create_dir_all(dir.0.join(n).join("in")).unwrap();
        }
        let (events, log) = start(&dir.0);
        let full = dir.0.join("full");
        NO_WATCH_LEFT.lock().unwrap().insert(full.clone());
        std::fs::create_dir_all(full.join("in")).unwrap();
        events.flush();
        assert!(heard(&log).contains(&None), "a rescan: {:?}", heard(&log));
        // A folder moved within the project: the rebuild meets `full` again.
        std::fs::rename(dir.0.join("d"), dir.0.join("e")).unwrap();
        events.flush();
        let watched: Vec<PathBuf> = watched_under(&dir.0).into_iter().map(|(p, _)| p).collect();
        let mut all = vec![dir.0.clone()];
        all.extend(["a", "a/in", "b", "b/in", "c", "c/in", "e", "e/in"].map(|r| dir.0.join(r)));
        assert_eq!(watched, all, "all but `full`");
        log.lock().unwrap().clear();
        let p = dir.0.join("a/in/x.ts");
        std::fs::write(&p, "x").unwrap();
        events.flush();
        assert!(heard(&log).contains(&Some(p)), "{:?}", heard(&log));
        NO_WATCH_LEFT.lock().unwrap().remove(&full);
    }

    // WRITE-19 (review evt-1 of the platform step): a folder put where the root was is watched by
    // the next flush, and the old tree let go: a copy moved into its place, and the root removed
    // while a file in it is held open (as the session holds its lock), which gives the root no
    // event of its own, then made again.
    #[test]
    fn a_root_put_back_is_watched_again() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        let root = dir.0.join("project");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let (events, log) = start(&root);
        // Moved away: the old tree is let go at once, so its writes aren't heard as the project's.
        std::fs::rename(&root, dir.0.join("old")).unwrap();
        events.flush();
        assert!(heard(&log).contains(&None), "a rescan: {:?}", heard(&log));
        assert_eq!(watched_under(&dir.0), vec![], "the old tree let go");
        log.lock().unwrap().clear();
        std::fs::write(dir.0.join("old/lib/a.ts"), "y").unwrap();
        events.flush();
        assert_eq!(heard(&log), vec![], "the old tree is heard no more");
        // A copy moved in.
        std::fs::create_dir_all(dir.0.join("copy/lib")).unwrap();
        std::fs::rename(dir.0.join("copy"), &root).unwrap();
        events.flush();
        assert!(heard(&log).contains(&None), "a rescan: {:?}", heard(&log));
        assert_eq!(watched_under(&dir.0), vec![(root.clone(), 1), (root.join("lib"), 1)]);
        log.lock().unwrap().clear();
        for p in [root.join("song.ts"), root.join("lib/a.ts")] {
            std::fs::write(&p, "x").unwrap();
            events.flush();
            assert!(heard(&log).contains(&Some(p.clone())), "{p:?}: {:?}", heard(&log));
        }
        // Removed while a file in it is open, then made again.
        std::fs::create_dir_all(root.join(".music2")).unwrap();
        let held = std::fs::File::create(root.join(".music2/lock")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        events.flush();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        events.flush();
        log.lock().unwrap().clear();
        let p = root.join("lib/b.ts");
        std::fs::write(&p, "x").unwrap();
        events.flush();
        assert!(heard(&log).contains(&Some(p)), "{:?}", heard(&log));
        drop(held);
    }

    // WRITE-19 (review evt-3 of the platform step): a folder that can't be read when it is made gets
    // no watch, and is watched once its mode lets it be read.
    #[test]
    fn a_folder_readable_only_later_is_watched_then() {
        use std::os::unix::fs::PermissionsExt;
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        let (events, log) = start(&dir.0);
        let locked = dir.0.join("locked");
        {
            // The reader sees the folder made only once it can't be read.
            let _held = instance().unwrap().lock();
            std::fs::create_dir(&locked).unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        }
        events.flush();
        assert_eq!(watched_under(&dir.0), vec![(dir.0.clone(), 1)], "no watch on a folder it can't read");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        events.flush();
        log.lock().unwrap().clear();
        let p = locked.join("x.ts");
        std::fs::write(&p, "x").unwrap();
        events.flush();
        assert!(heard(&log).contains(&Some(p)), "{:?}", heard(&log));
    }

    // WRITE-19 (decision D4 of the platform review): when the user's watches have run out at start,
    // the error says which of Linux's limits, in words; a folder the watch walk can't list (no file
    // descriptor left) leaves what is under it to the walks, and the watcher is told to walk.
    #[test]
    fn a_limit_run_out_is_named_and_an_unlisted_folder_walked() {
        let _shared = crate::watch::TESTS.read();
        let dir = Dir::new();
        NO_WATCH_LEFT.lock().unwrap().insert(dir.0.clone());
        let e = crate::watch::Events::start(&dir.0, |_| true, Box::new(|_| {})).err().expect("no watch left");
        NO_WATCH_LEFT.lock().unwrap().remove(&dir.0);
        assert_eq!(e.to_string(), "Linux's limit on watched folders is reached (fs.inotify.max_user_watches)");
        let (events, log) = start(&dir.0);
        let full = dir.0.join("full");
        NOT_LISTED.lock().unwrap().insert(full.clone());
        std::fs::create_dir_all(full.join("in")).unwrap();
        events.flush();
        NOT_LISTED.lock().unwrap().remove(&full);
        assert!(heard(&log).contains(&None), "a rescan: {:?}", heard(&log));
    }

    // WRITE-19: when the queue overflows, events are lost for every watcher, a new folder's among
    // them: the rebuild watches what is there, so a write in a folder made while events were being
    // lost is still named, and each watcher is told to walk.
    #[test]
    fn an_overflow_watches_what_is_there_again_and_walks() {
        // Every other watcher in the process hears the overflow too.
        let _alone = crate::watch::TESTS.write();
        let dir = Dir::new();
        let (events, log) = start(&dir.0);
        let queued: usize = std::fs::read_to_string("/proc/sys/fs/inotify/max_queued_events").unwrap().trim().parse().unwrap();
        {
            // The reader and every flush wait meanwhile, so the kernel queues all of it.
            let _held = instance().unwrap().lock();
            // A create, a write and a close each, in the root (a new folder isn't watched yet).
            for i in 0..queued / 3 + 100 {
                std::fs::write(dir.0.join(format!("{i}")), "x").unwrap();
            }
            // Made once the queue is full: its event is lost.
            std::fs::create_dir_all(dir.0.join("late")).unwrap();
        }
        let t = Instant::now();
        events.flush();
        assert!(heard(&log).contains(&None), "a rescan after {:?}", t.elapsed());
        assert!(watched_under(&dir.0).iter().any(|(p, _)| *p == dir.0.join("late")), "{:?}", watched_under(&dir.0));
        log.lock().unwrap().clear();
        let p = dir.0.join("late/x.ts");
        std::fs::write(&p, "x").unwrap();
        events.flush();
        assert!(heard(&log).contains(&Some(p)), "{:?}", heard(&log));
    }
}
