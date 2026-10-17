//! A small pool of threads that runs a graph of numbered tasks (8a; design-8a §5).
//!
//! In short:
//! - [`Tasks`] is a graph of numbered tasks: an edge `(a, b)` means `b` reads what `a` wrote. It
//!   is built once, off the audio thread, and holds the state a run needs, so a run allocates
//!   nothing.
//! - [`Pool::new`] starts the workers and hands out the one [`Runner`]: only the runner's owner
//!   can start a run. The pool owns the workers and joins them when dropped. A runner that
//!   outlives its pool runs every task itself.
//! - [`Runner::run`] runs every task once, each after every task it reads, on the calling thread
//!   and the workers. It returns when all are done and no worker can touch the run any more.
//! - A failed task (it returned `false`, or panicked) skips everything that reads it; the rest
//!   still runs. A task's panic is raised again by the caller, after the run.
//! - The caller spins while tasks keep finishing. When none has finished for `stuck`, it dozes on
//!   a [`m2_os::rt::Wake`] until the task it waits for ends and rings it. Workers spin for a moment
//!   (`patience`) when they have nothing to do, then park (std's `park`) until [`Runner::wake`] or
//!   the next run signals them.
//! - [`Runner::one`] has no workers and no heap: the audio thread can make one per call.
//!
//! The pool knows nothing of audio: no wasmtime, no floating-point mode, and no OS calls of its
//! own: the caller's one sleep goes through `m2-os`'s `Wake`. Its user (m2-runtime) decides what
//! each task does, and sets each worker up through [`Setup::start`] and [`Setup::stop`].
//! The protocol is checked by models that yield and spin at random before every atomic step
//! (`--features models`, tests/models.rs).

mod run;
mod sync;
mod tasks;
#[cfg(test)]
mod tests;

use std::io;
use std::sync::Arc;
use std::thread::{JoinHandle, Thread};
use std::time::Duration;

use run::Shared;
use sync::Ordering::{Relaxed, SeqCst};
use sync::{AtomicBool, AtomicU8};
pub use tasks::{Cycle, Outcome, Tasks};

/// How a pool's workers start and wait.
#[derive(Clone)]
pub struct Setup {
    /// Called on each worker first, with its number (1 to threads − 1), before it takes any task:
    /// a user's per-thread set-up. A panic here aborts the process.
    pub start: Option<Arc<dyn Fn(usize) + Send + Sync>>,
    /// Called on each worker last, with its number, once the pool is dropped and before the
    /// thread exits (macOS: it leaves the audio workgroup). A panic here aborts the process.
    pub stop: Option<Arc<dyn Fn(usize) + Send + Sync>>,
    /// How long a worker with nothing to take spins before it parks, and how long it stays in a
    /// run with no task finishing before it leaves.
    pub patience: Duration,
    /// How long the caller of a run spins with nothing finishing before it dozes.
    pub stuck: Duration,
    /// The longest one doze lasts: the ring of the task that ends ends it sooner.
    pub doze: Duration,
}

impl Setup {
    /// No hooks; workers spin 50 µs, the caller 1 ms before it dozes for at most 1 ms: for
    /// offline renders and tests.
    pub fn plain() -> Setup {
        Setup {
            start: None,
            stop: None,
            patience: Duration::from_micros(50),
            stuck: Duration::from_millis(1),
            doze: Duration::from_millis(1),
        }
    }
}

/// The workers. Dropping the pool stops and joins them, so never drop it on the audio thread.
pub struct Pool {
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
}

impl Pool {
    /// The most threads a pool has, the caller included.
    pub const MAX_THREADS: usize = 64;

    /// A pool for `threads` threads in all, the caller of each run included: `threads − 1`
    /// workers, named `m2-pool-<k>`, with 2 MiB stacks. `threads` is 1 to [`Pool::MAX_THREADS`].
    /// Fails if the OS won't start a thread or make the caller's wake; then none is left running.
    pub fn new(threads: usize, setup: Setup) -> io::Result<(Pool, Runner)> {
        if !(1..=Pool::MAX_THREADS).contains(&threads) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("a pool has 1 to 64 threads, not {threads}")));
        }
        let workers = (1..threads).map(|_| run::Worker::new()).collect();
        let doze = run::Doze::new(sync::Bell::new()?, setup.doze);
        let shared = Arc::new(Shared::new(workers, setup.patience, setup.stuck, doze));
        let mut pool = Pool { shared: shared.clone(), handles: Vec::with_capacity(threads - 1) };
        for k in 1..threads {
            let (shared, start, stop) = (shared.clone(), setup.start.clone(), setup.stop.clone());
            let me = k as u8;
            let handle = std::thread::Builder::new()
                .name(format!("m2-pool-{k}"))
                .stack_size(2 << 20)
                .spawn(move || run::worker(shared, me, start, stop))?;
            pool.handles.push(handle);
        }
        let threads = pool.handles.iter().map(|h| h.thread().clone()).collect();
        let runner = Runner { shared: Some(shared), threads, ..Runner::one() };
        Ok((pool, runner))
    }

    /// Per thread: the caller of runs first, then each worker. Safe to read during a run.
    pub fn counters(&self) -> Vec<Counters> {
        counters(&self.shared)
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.stop.store(true, SeqCst);
        for h in &self.handles {
            sync::unpark(h.thread());
        }
        for h in self.handles.drain(..) {
            // A worker can't panic out of its loop: it aborts first.
            let _ = h.join();
        }
    }
}

/// The one handle that runs jobs on a pool: `Send`, not `Clone`, so one thread at a time
/// publishes a run.
pub struct Runner {
    shared: Option<Arc<Shared>>,
    /// The workers, to signal: worker k is `threads[k − 1]`.
    threads: Box<[Thread]>,
    /// Runs so far; the current run's number.
    runs: u64,
    spread: bool,
    report: Spread,
    /// Tasks run by a runner with no pool.
    alone: u64,
    /// Tests: the run a task trips in, and how.
    tripwire: Tripwire,
}

impl Runner {
    /// A runner with no workers: every task runs on the caller, in queue order. It holds no heap
    /// memory, so the audio thread may make one per call.
    pub fn one() -> Runner {
        Runner {
            shared: None,
            threads: Box::new([]),
            runs: 0,
            spread: false,
            report: Spread::default(),
            alone: 0,
            tripwire: Tripwire { at: None, taken: AtomicBool::new(false), on: AtomicU8::new(0) },
        }
    }

    /// The threads a run uses now, the caller included: 1 once the pool is dropped.
    pub fn threads(&self) -> usize {
        match &self.shared {
            Some(s) if !s.stop.load(SeqCst) => 1 + self.threads.len(),
            _ => 1,
        }
    }

    /// Per thread: the caller of runs first, then each worker.
    pub fn counters(&self) -> Vec<Counters> {
        match &self.shared {
            Some(shared) => counters(shared),
            None => vec![Counters { tasks: self.alone, ..Counters::default() }],
        }
    }

    /// How many runs this runner has started.
    #[doc(hidden)]
    pub fn runs(&self) -> u64 {
        self.runs
    }

    /// For tests: in run number `run` (counted as [`Runner::runs`] will count it), the first task a
    /// worker takes trips: it panics, as a task's own panic would, or holds until its flag is set.
    /// With no worker in that run, the caller's first task trips.
    #[doc(hidden)]
    pub fn trip(&mut self, run: u64, what: Trip) {
        self.tripwire = Tripwire { at: Some((run, what)), taken: AtomicBool::new(false), on: AtomicU8::new(0) };
    }

    /// The thread whose task tripped (0 is the caller), once one has.
    #[doc(hidden)]
    pub fn tripped_on(&self) -> Option<usize> {
        self.tripwire.on.load(SeqCst).checked_sub(1).map(usize::from)
    }

    /// For tests and gates: every run wakes every worker, whatever its width; the caller takes no
    /// root until workers have taken them all; each thread spins a random while before each
    /// task it takes, to vary the interleaving. Does nothing without live workers.
    #[doc(hidden)]
    pub fn spread(&mut self, on: bool) {
        self.spread = on;
    }

    /// What spread runs did, summed over every run so far.
    #[doc(hidden)]
    pub fn spread_report(&self) -> Spread {
        self.report
    }
}

/// One thread's counts, over the pool's life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// Tasks it ran (skipped ones included).
    pub tasks: u64,
    /// Times it set out to sleep (workers only).
    pub parks: u64,
    /// Signals sent to wake it (workers only).
    pub signals: u64,
    /// Times it slept on its wake, a task it waited for stuck (the caller only).
    pub dozes: u64,
    /// Dozes a ring ended: the task ended, or a worker left the run.
    pub rung: u64,
    /// Dozes that lasted their whole time. `rung + timeouts == dozes`.
    pub timeouts: u64,
}

/// What a tripped task does (tests, [`Runner::trip`]).
#[doc(hidden)]
#[derive(Clone)]
pub enum Trip {
    /// It panics, before its work.
    Panic,
    /// It sleeps in 1 ms steps until the flag is set, then does its work.
    Hold(Arc<std::sync::atomic::AtomicBool>),
}

/// A runner's trip: the run and what, whether a task took it, and on which thread (+1).
pub(crate) struct Tripwire {
    pub(crate) at: Option<(u64, Trip)>,
    pub(crate) taken: AtomicBool,
    pub(crate) on: AtomicU8,
}

/// What spread runs did: whether work really crossed threads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spread {
    /// Tasks run (not skipped) on a worker.
    pub tasks_on_workers: u64,
    /// Edges between two tasks that both ran, on different threads.
    pub edges_across: u64,
}

fn counters(shared: &Shared) -> Vec<Counters> {
    let d = &shared.doze;
    let caller = Counters {
        tasks: shared.caller_tasks.load(Relaxed),
        dozes: d.dozes.load(Relaxed),
        rung: d.rung.load(Relaxed),
        timeouts: d.timeouts.load(Relaxed),
        ..Counters::default()
    };
    let workers = shared.workers.iter().map(|w| Counters {
        tasks: w.tasks.load(Relaxed),
        parks: w.parks.load(Relaxed),
        signals: w.signals.load(Relaxed),
        ..Counters::default()
    });
    std::iter::once(caller).chain(workers).collect()
}

/// A task graph and the pool's shared state cross threads behind raw pointers: make sure they may.
const _: fn() = || {
    fn shared_between_threads<T: Send + Sync>() {}
    shared_between_threads::<Tasks>();
    shared_between_threads::<Shared>();
    fn sent<T: Send>() {}
    sent::<Runner>();
};

/// What the models read (`--features models`).
#[cfg(feature = "models")]
#[doc(hidden)]
pub mod models {
    use std::sync::atomic::{AtomicU64, Ordering};

    pub use crate::sync::chaos::seed;

    pub(crate) static BREACHES: AtomicU64 = AtomicU64::new(0);

    /// How many times the pool touched a sealed run state ([`Tasks::seal`](crate::Tasks::seal)).
    pub fn breaches() -> u64 {
        BREACHES.load(Ordering::SeqCst)
    }
}
