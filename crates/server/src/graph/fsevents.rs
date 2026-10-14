//! FSEvents (macOS): which paths under the project folder changed, as hints for the watcher
//! (WRITE-19). Only names: what a path holds is always read from the disk, and a poll that sees
//! the whole project is what makes a batch ready, so a late or lost event delays a change at most.
//!
//! In-tree, over CoreServices' C API, rather than a crate: the watcher needs
//! `FSEventStreamFlushSync`, to tell an event still on its way from one that never came.

use std::collections::BTreeMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;

use super::watcher::Wake;

/// Past this many paths in one tick the watcher walks everything instead (a checkout's burst).
pub(crate) const MAX_HINTS: usize = 4096;

/// What the stream said since the watcher last took it.
#[derive(Debug, Default)]
pub(crate) struct Hints {
    /// Project paths, `/`-separated, with their events' flags or'ed.
    pub paths: BTreeMap<String, u32>,
    /// Events were dropped or too many came, or the project folder itself moved: only a walk of
    /// everything tells what changed.
    pub rescan: bool,
    /// Events delivered since the stream started, for the gates.
    pub events: u64,
}

const MUST_SCAN_SUB_DIRS: u32 = 0x0000_0001;
const USER_DROPPED: u32 = 0x0000_0002;
const KERNEL_DROPPED: u32 = 0x0000_0004;
const ROOT_CHANGED: u32 = 0x0000_0020;
const MOUNT: u32 = 0x0000_0040;
const UNMOUNT: u32 = 0x0000_0080;

/// What the stream's callback writes to, under its own lock, never the watch lock.
struct Shared {
    /// The project folder as events spell it, with a trailing `/`.
    prefix: Vec<u8>,
    hints: Mutex<Hints>,
    /// Told when a hint is added, so an idle watcher ticks at once.
    wake: Arc<Wake>,
}

impl Shared {
    fn heard(&self, path: &[u8], flags: u32) {
        let said = {
            let mut h = self.hints.lock();
            h.events += 1;
            self.hint(&mut h, path, flags)
        };
        if said {
            self.wake.wake();
        }
    }

    /// Add what an event says to `h`: whether it says something.
    fn hint(&self, h: &mut Hints, path: &[u8], flags: u32) -> bool {
        if flags & (USER_DROPPED | KERNEL_DROPPED | ROOT_CHANGED | MOUNT | UNMOUNT) != 0 {
            h.rescan = true;
            return true;
        }
        let root = &self.prefix[..self.prefix.len() - 1];
        let rel = match path.strip_prefix(self.prefix.as_slice()) {
            Some(rel) => rel,
            // The folder itself: a change to its own entry says nothing, unless what is under it
            // must be looked at again.
            None if path == root => {
                h.rescan |= flags & MUST_SCAN_SUB_DIRS != 0;
                return h.rescan;
            }
            // Elsewhere: another spelling of the folder than the one it was opened by, say. Only a
            // walk can tell.
            None => {
                h.rescan = true;
                return true;
            }
        };
        // A name that isn't UTF-8 is no project path, nor is anything under it.
        let Ok(rel) = std::str::from_utf8(rel) else { return false };
        let rel = rel.trim_end_matches('/');
        // What a walk never enters (`.music2`, `.git`, `node_modules`) can hold nothing tracked:
        // the session's own journal and git's objects never make a hint.
        if rel.is_empty() || !m2_session::watch::walked(rel) {
            return false;
        }
        *h.paths.entry(rel.to_string()).or_default() |= flags;
        if h.paths.len() > MAX_HINTS {
            h.paths.clear();
            h.rescan = true;
        }
        true
    }
}

type CFIndex = isize;
type CFRef = *const c_void;
type Queue = *mut c_void;

#[repr(C)]
struct CFArrayCallBacks {
    version: CFIndex,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
    equal: *const c_void,
}

#[repr(C)]
struct StreamContext {
    version: CFIndex,
    info: *mut c_void,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
}

type Callback = extern "C" fn(CFRef, *mut c_void, usize, *mut c_void, *const u32, *const u64);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: CFArrayCallBacks;
    fn CFStringCreateWithBytes(alloc: CFRef, bytes: *const u8, len: CFIndex, encoding: u32, external: u8) -> CFRef;
    fn CFArrayCreate(alloc: CFRef, values: *const CFRef, n: CFIndex, callbacks: *const CFArrayCallBacks) -> CFRef;
    fn CFRelease(cf: CFRef);
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        alloc: CFRef,
        cb: Callback,
        ctx: *const StreamContext,
        paths: CFRef,
        since: u64,
        latency: f64,
        flags: u32,
    ) -> *mut c_void;
    fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: Queue);
    fn FSEventStreamStart(stream: *mut c_void) -> u8;
    fn FSEventStreamFlushSync(stream: *mut c_void);
    fn FSEventStreamStop(stream: *mut c_void);
    fn FSEventStreamInvalidate(stream: *mut c_void);
    fn FSEventStreamRelease(stream: *mut c_void);
}

// libdispatch, in libSystem.
unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> Queue;
    fn dispatch_sync_f(queue: Queue, ctx: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_release(object: Queue);
}

const UTF8: u32 = 0x0800_0100;
const SINCE_NOW: u64 = u64::MAX;
const NO_DEFER: u32 = 0x02;
const WATCH_ROOT: u32 = 0x04;
const FILE_EVENTS: u32 = 0x10;
/// How long FSEvents may hold events to coalesce them, after the first of a quiet spell (which
/// `NO_DEFER` delivers at once). The watcher ticks every 50 ms anyway.
const LATENCY: f64 = 0.01;

extern "C" fn heard(_stream: CFRef, info: *mut c_void, n: usize, paths: *mut c_void, flags: *const u32, _ids: *const u64) {
    if n == 0 {
        return;
    }
    // SAFETY: `info` is the `Shared` the stream was made with, which outlives it (`Drop` waits
    // for the queue); without `UseCFTypes`, `paths` is `n` C strings, and `flags` has `n` flags.
    let (shared, paths, flags) = unsafe {
        (&*(info as *const Shared), std::slice::from_raw_parts(paths as *const *const c_char, n), std::slice::from_raw_parts(flags, n))
    };
    for (p, f) in paths.iter().zip(flags) {
        // SAFETY: each is a NUL-terminated path FSEvents owns for the call.
        shared.heard(unsafe { CStr::from_ptr(*p) }.to_bytes(), *f);
    }
}

extern "C" fn nothing(_: *mut c_void) {}

/// A running stream on the project folder, file by file.
pub(crate) struct Stream {
    stream: *mut c_void,
    queue: Queue,
    shared: Arc<Shared>,
}

// SAFETY: the stream and its queue are only flushed, stopped and released through `&self` and
// `Drop`, which FSEvents and libdispatch allow from any thread; the callback runs on the queue
// and shares only `Shared`, which is `Sync`.
unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

impl Stream {
    /// Start one on `dir` (its spelling as the disk has it: canonical). None when FSEvents won't.
    pub fn start(dir: &Path, wake: Arc<Wake>) -> Option<Stream> {
        use std::os::unix::ffi::OsStrExt;
        let mut prefix = dir.as_os_str().as_bytes().to_vec();
        prefix.push(b'/');
        let shared = Arc::new(Shared { prefix, hints: Mutex::default(), wake });
        let bytes = dir.as_os_str().as_bytes();
        // SAFETY: plain CoreFoundation and FSEvents calls; every object made here is released
        // here or by `Drop`, and `info` (the `Shared`) outlives the stream.
        unsafe {
            let path = CFStringCreateWithBytes(std::ptr::null(), bytes.as_ptr(), bytes.len() as CFIndex, UTF8, 0);
            if path.is_null() {
                return None;
            }
            let paths = CFArrayCreate(std::ptr::null(), &path, 1, &kCFTypeArrayCallBacks);
            CFRelease(path);
            if paths.is_null() {
                return None;
            }
            let ctx = StreamContext {
                version: 0,
                info: Arc::as_ptr(&shared) as *mut c_void,
                retain: std::ptr::null(),
                release: std::ptr::null(),
                copy_description: std::ptr::null(),
            };
            let stream = FSEventStreamCreate(std::ptr::null(), heard, &ctx, paths, SINCE_NOW, LATENCY, FILE_EVENTS | NO_DEFER | WATCH_ROOT);
            CFRelease(paths);
            if stream.is_null() {
                return None;
            }
            let queue = dispatch_queue_create(c"music2.watch".as_ptr(), std::ptr::null());
            FSEventStreamSetDispatchQueue(stream, queue);
            let s = Stream { stream, queue, shared };
            (FSEventStreamStart(stream) != 0).then_some(s)
        }
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
        for (p, f) in hints.paths {
            *h.paths.entry(p).or_default() |= f;
        }
        if h.paths.len() > MAX_HINTS {
            h.paths.clear();
            h.rescan = true;
        }
    }

    /// Have every event that happened before now delivered: an event still on its way is then in
    /// the hints. Never called with the hints' lock held, which the delivery takes.
    pub fn flush(&self) {
        // SAFETY: the stream is started and not yet released.
        unsafe { FSEventStreamFlushSync(self.stream) }
    }

    /// Events delivered so far.
    pub fn events(&self) -> u64 {
        self.shared.hints.lock().events
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: stopped and invalidated before release; the empty work run on the queue after
        // returns only once a callback in flight has, so none runs once `shared` is dropped.
        unsafe {
            FSEventStreamStop(self.stream);
            FSEventStreamInvalidate(self.stream);
            dispatch_sync_f(self.queue, std::ptr::null_mut(), nothing);
            FSEventStreamRelease(self.stream);
            dispatch_release(self.queue);
        }
    }
}
