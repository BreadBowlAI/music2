//! The plugin main thread (HOST-2): where every main-thread CLAP call runs.
//!
//! In-process, that is the process's real main thread, because many plugins post timers and work
//! to the main run loop, and two "main threads" would race:
//! - the app: AppKit's main thread, reached through the main dispatch queue
//!   ([`use_process_main_thread`], called first thing in `main`);
//! - the CLI, the headless server and the gates: [`run_main`] runs the program on a worker while
//!   the main thread runs its `CFRunLoop`, which drains the same queue;
//! - tests, where libtest owns the main thread: one dedicated thread ([`test_main_thread`]).
//!
//! There is no fallback: a program that installed none gets an error naming what to call, never
//! a second main thread of its own.
//!
//! The rules that keep this free of deadlocks: the main thread never waits on a thread that may
//! wait for it (the session's actor, a renderer build), and never loads a plugin library itself
//! (`create` loads it on the calling thread first).

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use crossbeam_channel::{Sender, unbounded};

type Job = Box<dyn FnOnce() + Send>;

/// How long [`run_main`] keeps the main thread serving plugins after the program returned, for the
/// instances it dropped to be destroyed there.
pub const DRAIN: Duration = Duration::from_secs(2);

pub struct MainThread {
    kind: Kind,
    closed: AtomicBool,
}

enum Kind {
    /// The process's main dispatch queue, which AppKit (the app) or a `CFRunLoop` ([`run_main`])
    /// services.
    #[cfg(target_os = "macos")]
    Dispatch,
    /// One thread running jobs from a channel: tests' dedicated thread, or, off macOS, the main
    /// thread under [`run_main`].
    Queue { tx: Sender<Job>, thread: usize },
}

static EXEC: OnceLock<MainThread> = OnceLock::new();

/// This thread's identity: `pthread_self()`, a register read. Never a thread-local: on macOS a
/// thread's first thread-local access allocates, and the audio thread asks (RENDER-28).
pub(crate) fn token() -> usize {
    unsafe extern "C" {
        fn pthread_self() -> usize;
    }
    // SAFETY: always safe to call.
    unsafe { pthread_self() }
}

/// Whether this is the process's main thread.
#[cfg(target_os = "macos")]
pub(crate) fn is_process_main() -> bool {
    unsafe extern "C" {
        fn pthread_main_np() -> std::ffi::c_int;
    }
    // SAFETY: always safe to call.
    unsafe { pthread_main_np() != 0 }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn is_process_main() -> bool {
    false
}

#[cfg(target_os = "macos")]
mod darwin {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct Queue {
        _opaque: [u8; 0],
    }

    unsafe extern "C" {
        /// What `dispatch_get_main_queue()` returns (a macro in the C headers).
        pub static _dispatch_main_q: Queue;
        pub fn dispatch_async_f(queue: *const Queue, context: *mut c_void, work: extern "C" fn(*mut c_void));
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub static kCFRunLoopDefaultMode: *const c_void;
        pub fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    }

    /// Run the main thread's run loop for at most `seconds`, returning after anything ran. The
    /// main dispatch queue is drained there.
    pub fn pump(seconds: f64) {
        // SAFETY: called on the main thread only (run_main).
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 1) };
    }

    pub extern "C" fn trampoline(ctx: *mut c_void) {
        // SAFETY: `post` boxed the job and gave up ownership of it.
        let job = unsafe { Box::from_raw(ctx as *mut super::Job) };
        super::run_job(*job);
    }
}

fn run_job(job: Job) {
    // A panic must never unwind into libdispatch or the run loop.
    if catch_unwind(AssertUnwindSafe(job)).is_err() {
        eprintln!("music2: a job on the plugin main thread panicked");
    }
}

impl MainThread {
    /// Whether the calling thread is the plugin main thread. Never allocates or locks: plugins ask
    /// it from the audio thread.
    pub fn is_current(&self) -> bool {
        match &self.kind {
            #[cfg(target_os = "macos")]
            Kind::Dispatch => is_process_main(),
            Kind::Queue { thread, .. } => token() == *thread,
        }
    }

    /// Whether it has shut down: [`run_main`] drained it, and nothing runs there any more.
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Run `f` there later and return at once. After shut-down, `f` is dropped unrun.
    pub fn post(&self, f: impl FnOnce() + Send + 'static) {
        if self.closed() {
            return;
        }
        let job: Job = Box::new(f);
        match &self.kind {
            #[cfg(target_os = "macos")]
            Kind::Dispatch => {
                let ctx = Box::into_raw(Box::new(job)) as *mut std::ffi::c_void;
                // SAFETY: the main queue lives for the whole process; the trampoline takes the box back.
                unsafe { darwin::dispatch_async_f(&darwin::_dispatch_main_q, ctx, darwin::trampoline) };
            }
            Kind::Queue { tx, .. } => {
                let _ = tx.send(job);
            }
        }
    }

    /// Run `f` there and wait for what it returns. On the main thread itself it runs inline. An
    /// error once the executor has shut down, or when `f` panicked: never a hang.
    pub fn run<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> Result<R> {
        if self.closed() {
            bail!("the plugin main thread has shut down");
        }
        if self.is_current() {
            return catch_unwind(AssertUnwindSafe(f)).map_err(|p| anyhow!("the plugin main thread panicked: {}", why(&p)));
        }
        type Slot<R> = (Mutex<Option<std::thread::Result<R>>>, Condvar);
        let slot: Arc<Slot<R>> = Arc::new((Mutex::new(None), Condvar::new()));
        let done = slot.clone();
        self.post(move || {
            let r = catch_unwind(AssertUnwindSafe(f));
            *done.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
            done.1.notify_all();
        });
        let mut got = slot.0.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(r) = got.take() {
                return r.map_err(|p| anyhow!("the plugin main thread panicked: {}", why(&p)));
            }
            if self.closed() {
                bail!("the plugin main thread shut down before it ran this");
            }
            got = slot.1.wait_timeout(got, Duration::from_millis(50)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

fn why(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "a panic".into())
}

/// The process's plugin main thread. An error when the program installed none (C1): call
/// [`run_main`] in `main`, [`use_process_main_thread`] in the app, or [`test_main_thread`] in tests.
pub fn main_thread() -> Result<&'static MainThread> {
    EXEC.get().ok_or_else(|| {
        anyhow!(
            "plugins need a main thread, and this program set none up (HOST-2): call m2_plugins::run_main in main(), \
             use_process_main_thread() in the app, or test_main_thread() in tests"
        )
    })
}

/// Whether the calling thread is the plugin main thread (false when none is installed).
pub fn on_main_thread() -> bool {
    EXEC.get().is_some_and(MainThread::is_current)
}

/// The app: plugins run their main-thread calls on AppKit's main thread, through the main dispatch
/// queue (HOST-2). Call it first thing in `main`, on the main thread, before anything touches a
/// plugin. Calls queued before the run loop starts run once it does; the main thread itself must
/// never wait on a thread that may be creating plugins.
pub fn use_process_main_thread() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if !is_process_main() {
            bail!("use_process_main_thread must be called on the process's main thread");
        }
        install(MainThread { kind: Kind::Dispatch, closed: AtomicBool::new(false) })
    }
    #[cfg(not(target_os = "macos"))]
    bail!("the app's main-thread executor is built for macOS only; use run_main")
}

fn install(m: MainThread) -> Result<()> {
    let mut fresh = Some(m);
    let installed = EXEC.get_or_init(|| fresh.take().expect("installed once"));
    match fresh {
        None => Ok(()),
        // Two calls to the same kind are harmless; mixing kinds isn't.
        Some(m) if std::mem::discriminant(&m.kind) == std::mem::discriminant(&installed.kind) => Ok(()),
        Some(_) => bail!("a different plugin main thread is installed already"),
    }
}

/// A thread that runs jobs from a channel, as long as the process lives.
fn queue_thread(name: &str) -> MainThread {
    let (tx, rx) = unbounded::<Job>();
    let (ready_tx, ready) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _ = ready_tx.send(token());
            for job in rx {
                run_job(job);
            }
        })
        .expect("a thread for the plugin main thread");
    let thread = ready.recv().expect("the plugin main thread started");
    MainThread { kind: Kind::Queue { tx, thread }, closed: AtomicBool::new(false) }
}

/// Tests: one dedicated thread is the plugin main thread (libtest owns the process's). Idempotent,
/// and a no-op when the process installed its real main thread (C1).
pub fn test_main_thread() {
    EXEC.get_or_init(|| queue_thread("m2-plugin-main"));
}

/// The CLI, the headless server and the gates (HOST-2): `work` runs on a thread of its own, with an
/// 8 MiB stack, while the main thread runs its `CFRunLoop` and serves as the plugin main thread.
/// When `work` returns, the loop keeps serving until every plugin instance is destroyed or
/// [`DRAIN`] passes; then the executor shuts down, and later calls to it are errors. A panic in
/// `work` resumes here after the drain. Call it on the main thread, from `main`.
pub fn run_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    #[cfg(target_os = "macos")]
    let exec = {
        assert!(is_process_main(), "run_main must be called on the process's main thread");
        if let Err(e) = install(MainThread { kind: Kind::Dispatch, closed: AtomicBool::new(false) }) {
            panic!("{e:#}");
        }
        main_thread().expect("installed")
    };
    #[cfg(not(target_os = "macos"))]
    let (exec, rx) = {
        let (tx, rx) = unbounded::<Job>();
        if EXEC.set(MainThread { kind: Kind::Queue { tx, thread: token() }, closed: AtomicBool::new(false) }).is_err() {
            panic!("a plugin main thread is installed already");
        }
        (main_thread().expect("installed"), rx)
    };
    let (done_tx, done) = crossbeam_channel::bounded(1);
    let worker = std::thread::Builder::new()
        .name("m2-main".into())
        .stack_size(8 << 20)
        .spawn(move || {
            let r = catch_unwind(AssertUnwindSafe(work));
            let _ = done_tx.send(r);
            // Wake the loop at once.
            if let Ok(m) = main_thread() {
                m.post(|| {});
            }
        })
        .expect("a thread for the program");
    let pump = |seconds: f64| {
        #[cfg(target_os = "macos")]
        darwin::pump(seconds);
        #[cfg(not(target_os = "macos"))]
        if let Ok(job) = rx.recv_timeout(Duration::from_secs_f64(seconds)) {
            run_job(job);
        }
    };
    let result = loop {
        if let Ok(r) = done.try_recv() {
            break r;
        }
        pump(0.25);
    };
    let _ = worker.join();
    // Instances the program dropped are destroyed here, on the main thread (HOST-2).
    let deadline = Instant::now() + DRAIN;
    while crate::inprocess::live() > 0 && Instant::now() < deadline {
        pump(0.01);
    }
    exec.closed.store(true, Ordering::Release);
    let left = crate::inprocess::live();
    if left > 0 {
        eprintln!("music2: {left} plugin instance(s) still alive at exit");
    }
    match result {
        Ok(t) => t,
        Err(p) => resume_unwind(p),
    }
}
