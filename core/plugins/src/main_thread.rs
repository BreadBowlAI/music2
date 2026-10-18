//! The plugin main thread (HOST-2): where every main-thread CLAP call runs.
//!
//! It runs the OS's main loop (`m2_os::main_loop`). On macOS that is the process's real main
//! thread, because many plugins post timers and work to the main run loop, and two "main threads"
//! would race. Linux has no process-wide run loop for plugins to post to, so there the app's
//! plugin main thread is a loop thread of its own, and GTK keeps the main thread:
//! - the app: [`use_app_main_thread`], called first thing in `main` (AppKit's main thread through
//!   the main dispatch queue on macOS; a loop thread of its own on Linux);
//! - the CLI, the headless server and the gates: [`run_main`] runs the program on a worker while
//!   the main thread runs the loop (a `CFRunLoop` draining the main queue on macOS, a job queue on
//!   Linux);
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
use m2_os::main_loop::{Loop, Where};

/// How long [`run_main`] keeps the main thread serving plugins after the program returned, for the
/// instances it dropped to be destroyed there.
pub const DRAIN: Duration = Duration::from_secs(2);

pub struct MainThread {
    at: Where,
    main_loop: Loop,
    closed: AtomicBool,
}

static EXEC: OnceLock<MainThread> = OnceLock::new();

/// This thread's identity (`m2_os::thread::token`): never allocates or locks, since the audio
/// thread asks (RENDER-28).
#[inline]
pub(crate) fn token() -> usize {
    m2_os::thread::token()
}

/// Whether this is the process's main thread.
#[inline]
pub(crate) fn is_process_main() -> bool {
    m2_os::thread::is_process_main()
}

impl MainThread {
    /// Whether the calling thread is the plugin main thread. Never allocates or locks: plugins ask
    /// it from the audio thread.
    pub fn is_current(&self) -> bool {
        self.main_loop.is_current()
    }

    /// The OS's loop it runs: where plugins' timers, fds and windows go (HOST-3).
    pub(crate) fn main_loop(&self) -> &Loop {
        &self.main_loop
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
        // A panic must never unwind into the OS's loop.
        self.main_loop.post(Box::new(move || {
            if catch_unwind(AssertUnwindSafe(f)).is_err() {
                eprintln!("music2: a job on the plugin main thread panicked");
            }
        }));
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
/// [`run_main`] in `main`, [`use_app_main_thread`] in the app, or [`test_main_thread`] in tests.
pub fn main_thread() -> Result<&'static MainThread> {
    EXEC.get().ok_or_else(|| {
        anyhow!(
            "plugins need a main thread, and this program set none up (HOST-2): call m2_plugins::run_main in main(), \
             use_app_main_thread() in the app, or test_main_thread() in tests"
        )
    })
}

/// Whether the calling thread is the plugin main thread (false when none is installed).
pub fn on_main_thread() -> bool {
    EXEC.get().is_some_and(MainThread::is_current)
}

/// The app (HOST-2): plugins make their main-thread calls on the app's main loop: AppKit's main
/// thread through the main dispatch queue on macOS, a loop thread of its own on Linux. Call it
/// first thing in `main`, on the main thread, before anything touches a plugin. Calls queued
/// before AppKit's run loop starts run once it does; the main thread itself must never wait on a
/// thread that may be creating plugins.
///
/// Plugins themselves run in the host process (HOST-3), which is this program run again with
/// [`crate::HOST_FLAG`]: when it was, this serves as that process and never returns.
pub fn use_app_main_thread() -> Result<()> {
    serve_if_asked();
    install(Where::AppMain)?;
    crate::use_remote(crate::Remote::this_program()?);
    Ok(())
}

/// When this program was run as the host process: serve, for good.
fn serve_if_asked() {
    if std::env::args().nth(1).is_some_and(|a| a == crate::HOST_FLAG) {
        crate::serve_host();
    }
}

/// The plugin main thread, started at `at` the first time. Two calls for the same place are
/// harmless; mixing places isn't.
fn install(at: Where) -> Result<&'static MainThread> {
    let m = match EXEC.get() {
        Some(m) => m,
        None => {
            let main_loop = Loop::start(at).map_err(|e| anyhow!("the plugin main thread can't start: {e}"))?;
            let mut fresh = Some(MainThread { at, main_loop, closed: AtomicBool::new(false) });
            EXEC.get_or_init(|| fresh.take().expect("installed once"))
        }
    };
    if m.at != at {
        bail!("a different plugin main thread is installed already");
    }
    Ok(m)
}

/// Tests: one dedicated thread is the plugin main thread (libtest owns the process's). Idempotent,
/// and a no-op when the process installed its real main thread (C1).
pub fn test_main_thread() {
    let _ = install(Where::Thread("m2-plugin-main"));
}

/// The CLI, the headless server and the gates (HOST-2): `work` runs on a thread of its own, with an
/// 8 MiB stack, while the main thread runs the OS's main loop and serves as the plugin main thread.
/// When `work` returns, the loop keeps serving until every plugin instance is destroyed or
/// [`DRAIN`] passes; then the executor shuts down, and later calls to it are errors. A panic in
/// `work` resumes here after the drain. Call it on the main thread, from `main`.
///
/// Plugins themselves run in the host process (HOST-3), which is this program run again with
/// [`crate::HOST_FLAG`]: when it was, this serves as that process and never returns.
pub fn run_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    assert!(is_process_main(), "run_main must be called on the process's main thread");
    serve_if_asked();
    if let Ok(r) = crate::Remote::this_program() {
        crate::use_remote(r);
    }
    serve_main(work)
}

/// [`run_main`]'s loop, with the host already chosen: the host process runs its requests under it.
pub(crate) fn serve_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let exec = install(Where::ProcessMain).unwrap_or_else(|e| panic!("{e:#}"));
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
    let result = loop {
        if let Ok(r) = done.try_recv() {
            break r;
        }
        exec.main_loop.run_for(Duration::from_millis(250));
    };
    let _ = worker.join();
    // Instances the program dropped are destroyed here, on the main thread (HOST-2), or in the
    // host process.
    let live = || crate::inprocess::live() + crate::remote_live();
    let deadline = Instant::now() + DRAIN;
    while live() > 0 && Instant::now() < deadline {
        exec.main_loop.run_for(Duration::from_millis(10));
    }
    exec.closed.store(true, Ordering::Release);
    let left = live();
    if left > 0 {
        eprintln!("music2: {left} plugin instance(s) still alive at exit");
    }
    match result {
        Ok(t) => t,
        Err(p) => resume_unwind(p),
    }
}
