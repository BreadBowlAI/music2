//! The loop the plugin main thread runs (HOST-2): jobs posted from any thread, run in order on one
//! thread, and the plugins' fds and timers (CLAP's `posix-fd-support` and `timer-support`, which
//! Linux plugins need for their windows, HOST-3).
//!
//! | OS | Jobs | Timers | Fds |
//! |---|---|---|---|
//! | Linux | a queue and an `eventfd` | a list, due times as `poll`'s timeout | `poll` |
//! | macOS | the main dispatch queue (or a run loop source on a loop thread) | `CFRunLoopTimer` | `CFFileDescriptor` |
//! | Windows (designed, not built) | `PostThreadMessageW` to the loop's thread, its message loop | `SetTimer` on the loop's thread | none: CLAP's fd support is POSIX only; a plugin there gets timers |
//!
//! Every callback runs on the loop's thread, never inside another; one may watch, unwatch, add or
//! cancel any registration, its own included.

use std::io;
use std::time::Duration;

use crate::os;

/// A job for a loop. It must not panic: a panic ends a loop thread, and aborts the process on
/// macOS's main queue. The caller catches it (the plugin executor does).
pub type Job = Box<dyn FnOnce() + Send>;

/// What a watched fd is ready for, or is watched for. An error is always said.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ready {
    pub read: bool,
    pub write: bool,
    pub error: bool,
}

/// Called on the loop's thread when its fd is ready.
pub type OnFd = Box<dyn FnMut(Ready) + Send>;
/// Called on the loop's thread at each period.
pub type OnTimer = Box<dyn FnMut() + Send>;

/// A watched fd ([`Loop::watch`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Watch(pub(crate) u64);

/// A timer ([`Loop::timer`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Timer(pub(crate) u64);

/// Where a loop runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Where {
    /// The process's main thread, which the program pumps with [`Loop::run_for`] (m2, the headless
    /// server, the gates, the plugin host process). Start it on that thread.
    ProcessMain,
    /// The app, whose toolkit owns the main thread. macOS: the main dispatch queue, which AppKit
    /// drains. Linux: a loop thread of its own. Start it on the main thread, before the toolkit.
    AppMain,
    /// A loop on a new thread with this name (tests, every OS).
    Thread(&'static str),
}

/// Whether [`Where::AppMain`] is the process's main thread: true on macOS, false on Linux.
pub const APP_ON_PROCESS_MAIN: bool = os::main_loop::APP_ON_PROCESS_MAIN;

/// A running loop. Any thread may post to it, and register fds and timers on it. A loop thread
/// ends when its `Loop` is dropped.
pub struct Loop(os::main_loop::Loop);

impl Loop {
    /// Start the loop at `at`. An error when `at` must be the process's main thread and the
    /// caller isn't on it, or when a loop thread can't start.
    pub fn start(at: Where) -> io::Result<Loop> {
        os::main_loop::Loop::start(at).map(Loop)
    }

    /// Run `job` on the loop later. From any thread; never blocks.
    pub fn post(&self, job: Job) {
        self.0.post(job)
    }

    /// Whether the calling thread is the one that runs the loop's jobs. Never allocates or locks:
    /// plugins ask from the audio thread.
    pub fn is_current(&self) -> bool {
        self.0.is_current()
    }

    /// [`Where::ProcessMain`] only, on that thread: run what is due, waiting at most `most` for it.
    pub fn run_for(&self, most: Duration) {
        self.0.run_for(most)
    }

    /// Call `f` on the loop's thread whenever `fd` is ready for what `want` names, until
    /// [`Loop::unwatch`]. An fd closed while watched is unwatched, after `f` hears an error.
    pub fn watch(&self, fd: i32, want: Ready, f: OnFd) -> io::Result<Watch> {
        self.0.watch(fd, want, f)
    }

    /// Watch for something else (CLAP's `modify_fd`). False when it isn't watched.
    pub fn rewatch(&self, w: Watch, want: Ready) -> bool {
        self.0.rewatch(w, want)
    }

    /// Stop watching. Called on the loop's thread (CLAP's rule), its callback isn't called again;
    /// from another thread, a call already running may end after this returns.
    pub fn unwatch(&self, w: Watch) {
        self.0.unwatch(w)
    }

    /// Call `f` on the loop's thread every `period` (at least 1 ms), from one period from now,
    /// until [`Loop::cancel`]. A late loop calls it once, not once per period missed.
    pub fn timer(&self, period: Duration, f: OnTimer) -> io::Result<Timer> {
        self.0.timer(period.max(Duration::from_millis(1)), f)
    }

    /// Stop a timer, as [`Loop::unwatch`] stops a watch.
    pub fn cancel(&self, t: Timer) {
        self.0.cancel(t)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc::channel;

    use super::*;
    use crate::thread;

    #[test]
    fn a_loop_thread_runs_its_jobs_in_order_on_itself() {
        let lp = Loop::start(Where::Thread("m2-os-test")).unwrap();
        assert!(!lp.is_current());
        let (tx, rx) = channel();
        for i in 0..100 {
            let tx = tx.clone();
            let on = std::thread::current().id();
            lp.post(Box::new(move || {
                let _ = tx.send((i, std::thread::current().id() != on, std::thread::current().name() == Some("m2-os-test")));
            }));
        }
        let got: Vec<_> = (0..100).map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
        assert!(got.iter().enumerate().all(|(i, &(n, elsewhere, named))| n == i && elsewhere && named), "{got:?}");
        let (tx, rx) = channel();
        let lp = std::sync::Arc::new(lp);
        let there = lp.clone();
        lp.post(Box::new(move || {
            let _ = tx.send(there.is_current());
        }));
        assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "is_current on the loop's thread");
    }

    #[test]
    fn the_process_main_loop_starts_only_on_the_main_thread() {
        // libtest runs tests on threads of its own.
        let started = std::thread::spawn(|| Loop::start(Where::ProcessMain).is_ok()).join().unwrap();
        assert!(!started);
    }

    #[test]
    fn the_apps_loop_is_the_process_main_thread_exactly_when_the_os_says_so() {
        // Off the main thread: on macOS it can't start there; on Linux it is a thread of its own.
        let lp = std::thread::spawn(|| Loop::start(Where::AppMain)).join().unwrap();
        assert_eq!(lp.is_err(), APP_ON_PROCESS_MAIN);
        match lp {
            Err(_) => {}
            Ok(lp) => {
                let (tx, rx) = channel();
                lp.post(Box::new(move || {
                    let _ = tx.send(thread::is_process_main());
                }));
                assert!(!rx.recv_timeout(Duration::from_secs(5)).unwrap(), "Linux's app loop runs off the process's main thread");
            }
        }
    }

    // HOST-3: CLAP's timer-support and posix-fd-support are served from the loop. A timer fires at
    // its period and a readable fd runs its callback, both on the loop's own thread; each stops
    // when cancelled, and a callback may cancel itself.
    #[test]
    fn timers_and_fds_run_on_the_loop() {
        let lp = Arc::new(Loop::start(Where::Thread("m2-os-fds")).unwrap());
        let (fires, elsewhere) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicBool::new(false)));
        let (f, e, there) = (fires.clone(), elsewhere.clone(), lp.clone());
        let t = lp
            .timer(
                Duration::from_millis(20),
                Box::new(move || {
                    f.fetch_add(1, Ordering::SeqCst);
                    if !there.is_current() {
                        e.store(true, Ordering::SeqCst);
                    }
                }),
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let n = fires.load(Ordering::SeqCst);
        assert!((5..=16).contains(&n), "a 20 ms timer fired {n} times in 300 ms");
        lp.cancel(t);
        std::thread::sleep(Duration::from_millis(60));
        let n = fires.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(fires.load(Ordering::SeqCst), n, "a cancelled timer stops");

        let mut fds = [0i32; 2];
        // SAFETY: a pipe into two ints.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let (tx, rx) = channel();
        let there = lp.clone();
        let read_end = fds[0];
        let w = lp
            .watch(
                read_end,
                Ready { read: true, ..Ready::default() },
                Box::new(move |r| {
                    let mut b = [0u8; 16];
                    // SAFETY: reads into a local buffer from the pipe it watches.
                    let n = unsafe { libc::read(read_end, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
                    let _ = tx.send((r.read, n, there.is_current()));
                }),
            )
            .unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "nothing to read yet");
        // SAFETY: one byte into the pipe's write end.
        assert_eq!(unsafe { libc::write(fds[1], b"x".as_ptr() as *const libc::c_void, 1) }, 1);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), (true, 1, true), "read, one byte, on the loop's thread");
        lp.unwatch(w);
        // SAFETY: as above.
        unsafe { libc::write(fds[1], b"y".as_ptr() as *const libc::c_void, 1) };
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "an unwatched fd is quiet");

        // A timer that cancels itself, as a plugin's `on_timer` may.
        let once = Arc::new(AtomicU32::new(0));
        let (o, there) = (once.clone(), lp.clone());
        let me = Arc::new(std::sync::Mutex::new(None::<Timer>));
        let mine = me.clone();
        let t = lp
            .timer(
                Duration::from_millis(20),
                Box::new(move || {
                    o.fetch_add(1, Ordering::SeqCst);
                    if let Some(t) = *mine.lock().unwrap() {
                        there.cancel(t);
                    }
                }),
            )
            .unwrap();
        *me.lock().unwrap() = Some(t);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(once.load(Ordering::SeqCst), 1, "it ran once and stopped itself");
        // SAFETY: the pipe's two ends, closed once.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        assert!(!elsewhere.load(Ordering::SeqCst), "every timer ran on the loop's thread");
    }

    #[test]
    fn a_loop_is_send_and_sync() {
        fn both<T: Send + Sync>() {}
        both::<Loop>();
    }
}
