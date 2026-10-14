//! The loop the plugin main thread runs (HOST-2): jobs posted from any thread, run in order on one
//! thread. Step 9 adds the plugins' fds and timers here.

use std::io;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::{os, thread};

/// A job for a loop. It must not panic: a panic ends a loop thread, and aborts the process on
/// macOS's main queue. The caller catches it (the plugin executor does).
pub type Job = Box<dyn FnOnce() + Send>;

/// Where a loop runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Where {
    /// The process's main thread, which the program pumps with [`Loop::run_for`] (m2, the headless
    /// server, the gates). Start it on that thread.
    ProcessMain,
    /// The app, whose toolkit owns the main thread. macOS: the main dispatch queue, which AppKit
    /// drains. Linux: a loop thread of its own. Start it on the main thread, before the toolkit.
    AppMain,
    /// A loop on a new thread with this name (tests, every OS).
    Thread(&'static str),
}

/// Whether [`Where::AppMain`] is the process's main thread: true on macOS, false on Linux.
pub const APP_ON_PROCESS_MAIN: bool = os::main_loop::APP_ON_PROCESS_MAIN;

/// A running loop. Any thread may post to it.
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
}

/// Jobs that one thread runs in order: a loop thread of its own, or the thread that made the
/// queue, under `run_for`. The same on every OS.
pub(crate) struct Queue {
    tx: Sender<Job>,
    /// The jobs, when the thread that made the queue runs them; a loop thread owns its own.
    rx: Option<Mutex<Receiver<Job>>>,
    thread: usize,
}

impl Queue {
    /// A queue that a new thread named `name` runs for as long as the queue lives; with no name,
    /// one the calling thread runs through `run_for`.
    pub(crate) fn start(name: Option<&str>) -> io::Result<Queue> {
        let (tx, rx) = channel::<Job>();
        let Some(name) = name else {
            return Ok(Queue { tx, rx: Some(Mutex::new(rx)), thread: thread::token() });
        };
        let (ready_tx, ready) = channel();
        std::thread::Builder::new().name(name.into()).spawn(move || {
            let _ = ready_tx.send(thread::token());
            for job in rx {
                job();
            }
        })?;
        let thread = ready.recv().map_err(|_| io::Error::other("the loop thread ended as it started"))?;
        Ok(Queue { tx, rx: None, thread })
    }

    pub(crate) fn post(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    pub(crate) fn is_current(&self) -> bool {
        thread::token() == self.thread
    }

    /// Run the next job, waiting at most `most` for one.
    pub(crate) fn run_for(&self, most: Duration) {
        let rx = match &self.rx {
            Some(rx) if self.is_current() => rx.lock().unwrap_or_else(|e| e.into_inner()),
            _ => panic!("Loop::run_for runs a ProcessMain loop, on its own thread"),
        };
        if let Ok(job) = rx.recv_timeout(most) {
            job();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use super::*;

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

    #[test]
    fn a_loop_is_send_and_sync() {
        fn both<T: Send + Sync>() {}
        both::<Loop>();
    }
}
