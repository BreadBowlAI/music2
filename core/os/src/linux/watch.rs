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
        for id in f.watchers.iter().copied().collect::<Vec<_>>() {
            let Some(w) = self.watchers.get(&id) else { continue };
            let (heard, root_wd, enter) = (w.heard.clone(), w.root_wd, w.enter);
            if gone {
                if mask & libc::IN_MOVE_SELF != 0 && wd != root_wd {
                    // A folder moved within the project or out of it: its watches, and those under
                    // it, still have the old paths.
                    self.rebuild(fd, id);
                } else if wd == root_wd || mask & libc::IN_UNMOUNT != 0 {
                    heard(Heard::Rescan);
                }
                // A folder under the root that went is named by its parent's event.
                continue;
            }
            if made && let Some(rel) = rel(&self.watchers[&id].root, &path).filter(|r| enter(r)) {
                // A folder made, or moved in: watch it and the folders under it. Naming it then
                // makes the watcher walk it, which finds what was written before the watch.
                if self.watch_tree(fd, id, &path, &rel).is_err() {
                    heard(Heard::Rescan);
                }
            }
            heard(Heard::Path(&path));
        }
    }

    /// Watch `dir` for watcher `id`: its descriptor, which a folder already watched keeps.
    fn add(&mut self, fd: RawFd, id: u64, dir: &Path) -> io::Result<i32> {
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

    /// Watch `dir`, at `rel` under the watcher's root, and the folders under it.
    fn watch_tree(&mut self, fd: RawFd, id: u64, dir: &Path, rel: &str) -> io::Result<()> {
        match self.add(fd, id, dir) {
            Ok(_) => self.watch_under(fd, id, dir, rel),
            Err(e) if skipped(&e) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Watch every folder under `dir` (watched already) that `enter` allows, each before it is
    /// listed. An error when a watch can't be made: watches or memory ran out.
    fn watch_under(&mut self, fd: RawFd, id: u64, dir: &Path, rel: &str) -> io::Result<()> {
        let Some(enter) = self.watchers.get(&id).map(|w| w.enter) else { return Ok(()) };
        let mut stack = vec![(dir.to_path_buf(), rel.to_string())];
        while let Some((dir, rel)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
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
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    /// Watch watcher `id`'s root and every folder under it again, let go of the descriptors that
    /// walk didn't reach, and tell it to walk.
    fn rebuild(&mut self, fd: RawFd, id: u64) {
        let Some(w) = self.watchers.get_mut(&id) else { return };
        let (root, heard, held) = (w.root.clone(), w.heard.clone(), std::mem::take(&mut w.wds));
        w.root_wd = -1;
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
/// walk could read in it either. Its parent's events name it.
fn skipped(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR | libc::EACCES))
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
        let instance = instance()?;
        let fd = instance.fd.as_raw_fd();
        let mut st = instance.lock();
        let id = st.next;
        st.next += 1;
        let heard = Arc::from(heard);
        st.watchers.insert(id, Watcher { root: root.to_path_buf(), root_wd: -1, enter, heard, wds: BTreeSet::new() });
        let started = st.add(fd, id, root).and_then(|wd| {
            st.watchers.get_mut(&id).expect("registered").root_wd = wd;
            st.watch_under(fd, id, root, "")
        });
        if let Err(e) = started {
            st.unregister(fd, id);
            return Err(e);
        }
        Ok(Events { id, instance })
    }

    /// Under the instance's lock, which every dispatch holds: one in progress ends first.
    pub(crate) fn flush(&self) {
        self.instance.drain(&mut self.instance.lock());
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
