//! macOS: FSEvents, file by file, on a private dispatch queue. In-tree, over CoreServices' C API,
//! rather than a crate: the watcher needs `FSEventStreamFlushSync`, to tell an event still on its
//! way from one that never came.

use std::ffi::{CStr, OsStr, c_char, c_void};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;

use crate::watch::{Hear, Heard};

const MUST_SCAN_SUB_DIRS: u32 = 0x0000_0001;
const USER_DROPPED: u32 = 0x0000_0002;
const KERNEL_DROPPED: u32 = 0x0000_0004;
const ROOT_CHANGED: u32 = 0x0000_0020;
const MOUNT: u32 = 0x0000_0040;
const UNMOUNT: u32 = 0x0000_0080;

/// What the stream's callback reads.
struct Shared {
    /// The folder as events spell it.
    root: Vec<u8>,
    heard: Hear,
}

impl Shared {
    fn heard(&self, path: &[u8], flags: u32) {
        let heard = if flags & (USER_DROPPED | KERNEL_DROPPED | ROOT_CHANGED | MOUNT | UNMOUNT) != 0 {
            Heard::Rescan
        } else if path == self.root.as_slice() && flags & MUST_SCAN_SUB_DIRS != 0 {
            // The folder's own entry says nothing, unless what is under it must be looked at again.
            Heard::Rescan
        } else {
            Heard::Path(Path::new(OsStr::from_bytes(path)))
        };
        (self.heard)(heard)
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

extern "C" fn callback(_stream: CFRef, info: *mut c_void, n: usize, paths: *mut c_void, flags: *const u32, _ids: *const u64) {
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

/// A running stream on the folder, file by file. FSEvents watches the whole tree, so `enter`
/// isn't needed here.
pub(crate) struct Events {
    stream: *mut c_void,
    queue: Queue,
    _shared: Arc<Shared>,
}

// SAFETY: the stream and its queue are only flushed, stopped and released through `&self` and
// `Drop`, which FSEvents and libdispatch allow from any thread; the callback runs on the queue
// and shares only `Shared`, which is `Sync`.
unsafe impl Send for Events {}
unsafe impl Sync for Events {}

impl Events {
    pub(crate) fn start(root: &Path, _enter: fn(&str) -> bool, heard: Hear) -> io::Result<Events> {
        let bytes = root.as_os_str().as_bytes();
        let shared = Arc::new(Shared { root: bytes.to_vec(), heard });
        let refused = || io::Error::other("FSEvents didn't start");
        // SAFETY: plain CoreFoundation and FSEvents calls; every object made here is released
        // here or by `Drop`, and `info` (the `Shared`) outlives the stream.
        unsafe {
            let path = CFStringCreateWithBytes(std::ptr::null(), bytes.as_ptr(), bytes.len() as CFIndex, UTF8, 0);
            if path.is_null() {
                return Err(refused());
            }
            let paths = CFArrayCreate(std::ptr::null(), &path, 1, &kCFTypeArrayCallBacks);
            CFRelease(path);
            if paths.is_null() {
                return Err(refused());
            }
            let ctx = StreamContext {
                version: 0,
                info: Arc::as_ptr(&shared) as *mut c_void,
                retain: std::ptr::null(),
                release: std::ptr::null(),
                copy_description: std::ptr::null(),
            };
            let stream =
                FSEventStreamCreate(std::ptr::null(), callback, &ctx, paths, SINCE_NOW, LATENCY, FILE_EVENTS | NO_DEFER | WATCH_ROOT);
            CFRelease(paths);
            if stream.is_null() {
                return Err(refused());
            }
            let queue = dispatch_queue_create(c"music2.watch".as_ptr(), std::ptr::null());
            FSEventStreamSetDispatchQueue(stream, queue);
            let s = Events { stream, queue, _shared: shared };
            if FSEventStreamStart(stream) != 0 { Ok(s) } else { Err(refused()) }
        }
    }

    pub(crate) fn flush(&self) {
        // SAFETY: the stream is started and not yet released.
        unsafe { FSEventStreamFlushSync(self.stream) }
    }
}

impl Drop for Events {
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
