//! The block path: waking workers, one run, the workers' loop, and sleeping.
//!
//! Nothing here allocates, locks, uses a channel, a thread-local or I/O (tests/no_blocking.rs),
//! so it may run on the audio thread. The caller of `run` never parks: it spins while tasks keep
//! finishing, and sleeps in short steps only when none has finished for `stuck`. Only workers
//! park, with std's `park`, and only a waker that took their `parked` flag signals them.
//!
//! The orderings, edge by edge (design-8a §5.3 and §5.4):
//! - The caller's writes before a run come before its `job` store (SeqCst); a worker loads `job`
//!   (SeqCst) before it pops anything.
//! - A producer's writes come before its `waiting` decrement (AcqRel). All decrements of one
//!   count form a release sequence, so the last decrementer acquires every producer's writes.
//!   It pushes the consumer (Release), and whoever pops it loads the slot (Acquire). A `skip`
//!   store before a decrement travels the same chain.
//! - Every task ends with `left` −1 (SeqCst), and the caller sees `left == 0` (Acquire): every
//!   task's work comes before the end of the run.
//! - Dekker's pattern twice. A worker adds itself to `inside`, then loads `job`; the caller stores
//!   a null `job`, then waits for `inside == 0`: so either the caller waits for the worker, or the
//!   worker sees no job and touches nothing. A sleeper sets its `parked` flag, then looks for a new
//!   run; a waker publishes the run, then takes the flag: so either the sleeper sees the run, or
//!   the waker signals it.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread::Thread;
use std::time::{Duration, Instant};

use crate::sync::{self, AtomicBool, AtomicPtr, AtomicU64, Ordering::*};
use crate::tasks::{EMPTY, FAILED, Padded, RAN, State, Tasks};
use crate::Runner;

/// What the caller of `run` and the workers share.
pub(crate) struct Shared {
    /// The run being worked on, or null between runs.
    job: Padded<AtomicPtr<Job<'static>>>,
    /// The number of the last run published: a worker enters each run at most once.
    seq: Padded<AtomicU64>,
    /// Workers that may be touching the job now.
    pub(crate) inside: Padded<AtomicU64>,
    /// `Runner::rest`: idle workers sleep now, not after `patience`.
    resting: AtomicBool,
    /// The pool is dropped: workers leave.
    pub(crate) stop: AtomicBool,
    /// One per worker: worker k (thread k) is `workers[k − 1]`.
    pub(crate) workers: Box<[Worker]>,
    /// Tasks the caller of `run` ran.
    pub(crate) caller_tasks: Padded<AtomicU64>,
    pub(crate) patience: Duration,
    pub(crate) stuck: Duration,
}

/// A worker's flag and counters.
pub(crate) struct Worker {
    /// It is asleep, or about to be: a waker that swaps it from true signals it.
    pub(crate) parked: Padded<AtomicBool>,
    pub(crate) tasks: AtomicU64,
    pub(crate) parks: AtomicU64,
    pub(crate) signals: AtomicU64,
}

impl Shared {
    pub(crate) fn new(workers: Box<[Worker]>, patience: Duration, stuck: Duration) -> Shared {
        Shared {
            job: Padded(AtomicPtr::new(std::ptr::null_mut())),
            seq: Padded(AtomicU64::new(0)),
            inside: Padded(AtomicU64::new(0)),
            resting: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            workers,
            caller_tasks: Padded(AtomicU64::new(0)),
            patience,
            stuck,
        }
    }
}

impl Worker {
    pub(crate) fn new() -> Worker {
        Worker {
            parked: Padded(AtomicBool::new(false)),
            tasks: AtomicU64::new(0),
            parks: AtomicU64::new(0),
            signals: AtomicU64::new(0),
        }
    }
}

/// One run, as the workers see it. It lives on the caller's stack for the run.
#[derive(Clone, Copy)]
struct Job<'a> {
    id: u64,
    tasks: &'a Tasks,
    task: &'a (dyn Fn(u32) -> bool + Sync),
    spread: bool,
}

/// Turns a panic in the pool's own code into an abort: a worker must never unwind out of its
/// loop with the job in hand, nor the caller out of `run` while workers may still read the job.
struct AbortOnPanic;

impl Drop for AbortOnPanic {
    fn drop(&mut self) {
        std::process::abort();
    }
}

impl Runner {
    /// Wake the workers a run of this width can use, before the work that comes before the run
    /// (for a block: the pre-pass), so their wake-up overlaps it: at most `min(workers, width − 1)`,
    /// or every worker with `spread`. A worker is signalled only if it is asleep, so one that slept
    /// through many wakes wakes once. Never blocks.
    pub fn wake(&mut self, width: usize) {
        let Some(shared) = self.live() else { return };
        shared.resting.store(false, SeqCst);
        signal(shared, &self.threads, self.wanted(width));
    }

    /// From now on, workers with nothing to do sleep at once instead of spinning for `patience`:
    /// no run follows soon (after a callback's last block, or a render's last).
    pub fn rest(&mut self) {
        if let Some(shared) = &self.shared {
            shared.resting.store(true, SeqCst);
        }
    }

    /// Run every task of `tasks` once, each after every task it reads, on this thread and the
    /// pool's workers. `task(t)` runs task t and says whether it succeeded.
    ///
    /// - It returns once every task has run, failed or been skipped, and no worker can touch
    ///   `tasks` or `task` again.
    /// - A task returning `false` fails. Everything that reads it, directly or not, is skipped;
    ///   the rest runs. [`Tasks::outcome`] says which.
    /// - A panicking task counts as failed. After the run, `run` returns the payload of the
    ///   lowest-numbered task that panicked, for the caller to raise again.
    /// - A panic in the pool's own code aborts the process when the runner has workers; on
    ///   [`Runner::one`] it unwinds, as any panic would.
    /// - The caller never parks. It spins while tasks keep finishing. After `stuck` with none
    ///   finishing, it sleeps in steps of `stuck / 4` and checks again. It uses no per-thread
    ///   state, so any thread may call `run`, one std did not start included.
    /// - A worker leaves a run when nothing is queued and every task left is running, or after
    ///   `patience` with nothing finishing, and never enters the same run twice. It then spins
    ///   for `patience` waiting for the next run (or until [`Runner::rest`]), and parks.
    /// - It wakes the workers [`Runner::wake`] would, again after the run is published.
    /// - It allocates nothing.
    pub fn run(&mut self, tasks: &mut Tasks, task: &(dyn Fn(u32) -> bool + Sync)) -> Result<(), Box<dyn Any + Send>> {
        if tasks.is_empty() {
            return Ok(());
        }
        tasks.reset();
        self.runs += 1;
        let width = tasks.width();
        let spread = self.spread && self.live().is_some();
        let job = Job { id: self.runs, tasks: &*tasks, task, spread };
        let ran = match self.live() {
            Some(shared) if width > 1 || spread => together(shared, &self.threads, self.wanted(width), &job),
            _ => help(&job, 0, Role::Caller(self.stuck())),
        };
        match &self.shared {
            Some(shared) => {
                shared.caller_tasks.fetch_add(ran, Relaxed);
            }
            None => self.alone += ran,
        }
        if spread {
            let (on_workers, across) = tasks.spread();
            self.report.tasks_on_workers += on_workers;
            self.report.edges_across += across;
        }
        match tasks.take_panic() {
            Some(payload) => Err(payload),
            None => Ok(()),
        }
    }

    /// The pool's shared state, while its workers live.
    fn live(&self) -> Option<&Arc<Shared>> {
        self.shared.as_ref().filter(|s| !s.workers.is_empty() && !s.stop.load(Acquire))
    }

    /// How many workers a run of this width wakes.
    fn wanted(&self, width: usize) -> usize {
        let workers = self.threads.len();
        if self.spread { workers } else { workers.min(width.saturating_sub(1)) }
    }

    fn stuck(&self) -> Duration {
        self.shared.as_ref().map_or(Duration::from_millis(1), |s| s.stuck)
    }
}

/// A run on the caller and the workers: publish it, wake them, help until every task is done,
/// then close it and wait until no worker is inside. Returns how many tasks the caller ran.
fn together(shared: &Shared, threads: &[Thread], want: usize, job: &Job) -> u64 {
    let guard = AbortOnPanic;
    shared.job.store((job as *const Job<'_>).cast::<Job<'static>>().cast_mut(), SeqCst);
    shared.seq.store(job.id, SeqCst);
    shared.resting.store(false, SeqCst);
    signal(shared, threads, want);
    if job.spread {
        // Every root goes to a worker; the caller takes only what they push.
        let roots = job.tasks.roots().len() as u32;
        while job.tasks.head.load(Acquire) < roots && !shared.stop.load(Acquire) {
            std::hint::spin_loop();
        }
    }
    let ran = help(job, 0, Role::Caller(shared.stuck));
    shared.job.store(std::ptr::null_mut(), SeqCst);
    let mut wait = Wait::new(shared.stuck);
    loop {
        let inside = shared.inside.load(SeqCst);
        if inside == 0 {
            break;
        }
        if wait.idle(inside) {
            std::thread::sleep(shared.stuck / 4);
        }
        std::hint::spin_loop();
    }
    std::mem::forget(guard);
    ran
}

/// Signal the first `want` workers that are asleep.
pub(crate) fn signal(shared: &Shared, threads: &[Thread], want: usize) {
    for (w, thread) in shared.workers.iter().zip(threads).take(want) {
        if w.parked.swap(false, SeqCst) {
            w.signals.fetch_add(1, Relaxed);
            sync::unpark(thread);
        }
    }
}

#[derive(Clone, Copy)]
enum Role {
    /// The caller of `run`: stays until every task is done; sleeps in steps after this long with
    /// none finishing.
    Caller(Duration),
    /// A worker: leaves when nothing is left it could take, or after this long with none finishing.
    Worker(Duration),
}

/// One thread's share of a run: take ready tasks until the run is done (the caller), or until
/// nothing is left it could take (a worker). Returns how many tasks it ran.
fn help(job: &Job, me: u8, role: Role) -> u64 {
    let tasks = job.tasks;
    let mut ran = 0;
    let mut rng = Rng((job.id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (u64::from(me) << 56)) | 1);
    let mut wait = Wait::new(match role {
        Role::Caller(d) | Role::Worker(d) => d,
    });
    loop {
        tasks.touch();
        if job.spread {
            // Vary the interleaving: another thread may take what this one just pushed.
            for _ in 0..rng.next() % 2001 {
                std::hint::spin_loop();
            }
        }
        if let Some(t) = pop(tasks) {
            run_one(job, t, me);
            ran += 1;
            continue;
        }
        let left = tasks.left.load(Acquire);
        if left == 0 {
            return ran;
        }
        match role {
            Role::Worker(_) => {
                if left <= tasks.running.load(Acquire) || wait.idle(u64::from(left)) {
                    return ran;
                }
            }
            Role::Caller(stuck) => {
                if wait.idle(u64::from(left)) {
                    std::thread::sleep(stuck / 4);
                }
            }
        }
        std::hint::spin_loop();
    }
}

/// Take the next ready task, if one is queued.
fn pop(tasks: &Tasks) -> Option<u32> {
    tasks.touch();
    loop {
        let h = tasks.head.load(Acquire);
        if h >= tasks.tail.load(Acquire) {
            return None;
        }
        // Reserved but not yet written: nothing to take yet.
        let t = tasks.queue[h as usize].load(Acquire);
        if t == EMPTY {
            return None;
        }
        // Each slot is written once a run and `head` only grows, so a won slot holds `t`.
        if tasks.head.compare_exchange_weak(h, h + 1, AcqRel, Relaxed).is_ok() {
            tasks.running.fetch_add(1, SeqCst);
            return Some(t);
        }
    }
}

/// Run task t (unless something it reads failed), then complete it.
fn run_one(job: &Job, t: u32, me: u8) {
    let tasks = job.tasks;
    let skipped = tasks.state[t as usize].skip.load(Relaxed);
    let ok = !skipped
        && match catch_unwind(AssertUnwindSafe(|| (job.task)(t))) {
            Ok(ok) => ok,
            Err(payload) => {
                // SAFETY: this thread popped t, so it is the only one running it.
                unsafe { tasks.panics[t as usize].put(payload) };
                false
            }
        };
    tasks.finish(t, ok, skipped, me);
    complete(tasks, t, ok);
}

/// Pass task t's result on: its consumers are skipped if it didn't succeed, and each whose last
/// input this was is pushed, most critical first. Then t leaves the count: the last thing a task
/// does to the run state.
fn complete(tasks: &Tasks, t: u32, ok: bool) {
    tasks.touch();
    #[cfg(test)]
    if crate::tests::PANIC_IN_COMPLETE.load(Relaxed) {
        panic!("a panic in the pool's own code, for a test");
    }
    for &c in tasks.consumers(t) {
        let s = &tasks.state[c as usize];
        if !ok {
            s.skip.store(true, Relaxed);
        }
        if s.waiting.fetch_sub(1, AcqRel) == 1 {
            let slot = tasks.tail.fetch_add(1, AcqRel);
            tasks.queue[slot as usize].store(c, Release);
        }
    }
    tasks.running.fetch_sub(1, SeqCst);
    tasks.left.fetch_sub(1, SeqCst);
}

/// The run state's own steps, between and after runs.
impl Tasks {
    /// Get ready for a run: every count full, the roots in the queue. Nothing else touches the
    /// run state now (`&mut`).
    pub(crate) fn reset(&mut self) {
        for (s, &n) in self.state.iter_mut().zip(&self.inputs) {
            *s.waiting.get_mut() = n;
            *s.skip.get_mut() = false;
            *s.outcome.get_mut() = 0;
            *s.ran_on.get_mut() = 0;
        }
        for (slot, &r) in self.queue.iter_mut().zip(self.roots.iter().chain(std::iter::repeat(&EMPTY))) {
            *slot.get_mut() = r;
        }
        *self.head.get_mut() = 0;
        *self.tail.get_mut() = self.roots.len() as u32;
        *self.left.get_mut() = self.len() as u32;
        *self.running.get_mut() = 0;
    }

    /// Mark task `t`'s outcome. Only its own thread does, once a run.
    pub(crate) fn finish(&self, t: u32, ok: bool, skipped: bool, me: u8) {
        let s = &self.state[t as usize];
        if !skipped {
            s.outcome.store(if ok { RAN } else { FAILED }, Relaxed);
        }
        s.ran_on.store(me, Relaxed);
    }

    /// The payload of the lowest-numbered task that panicked in the last run, and no other kept.
    pub(crate) fn take_panic(&mut self) -> Option<Box<dyn Any + Send>> {
        let mut first = None;
        for slot in self.panics.iter_mut() {
            let p = slot.take();
            if first.is_none() {
                first = p;
            }
        }
        first
    }

    /// For the spread report, after a run: tasks that ran on workers, and edges between two tasks
    /// that both ran, on different threads.
    pub(crate) fn spread(&mut self) -> (u64, u64) {
        let ran = |s: &State| s.outcome.load(Relaxed) != 0;
        let mut on_workers = 0;
        let mut across = 0;
        for t in 0..self.len() {
            let s = &self.state[t];
            if !ran(s) {
                continue;
            }
            let at = s.ran_on.load(Relaxed);
            on_workers += u64::from(at != 0);
            for &c in self.consumers(t as u32) {
                let sc = &self.state[c as usize];
                across += u64::from(ran(sc) && sc.ran_on.load(Relaxed) != at);
            }
        }
        (on_workers, across)
    }

    /// The models' check that nothing touches a run's state after `run` returned.
    #[inline(always)]
    pub(crate) fn touch(&self) {
        #[cfg(feature = "models")]
        if self.sealed.load(std::sync::atomic::Ordering::SeqCst) {
            crate::models::BREACHES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// A worker's whole life: its `start` hook, then runs until the pool is dropped.
pub(crate) fn worker(shared: Arc<Shared>, me: u8, start: Option<Arc<dyn Fn(usize) + Send + Sync>>) {
    let guard = AbortOnPanic;
    if let Some(start) = start {
        start(usize::from(me));
    }
    let w = &shared.workers[usize::from(me) - 1];
    // The newest run this worker has entered, or seen end.
    let mut last = 0;
    loop {
        // Wait for a run it hasn't seen: spin for `patience` (or until told to rest), then sleep.
        let mut wait = Wait::new(shared.patience);
        let seen = loop {
            if shared.stop.load(Acquire) {
                std::mem::forget(guard);
                return;
            }
            let seq = shared.seq.load(Acquire);
            if seq != last {
                break seq;
            }
            if shared.resting.load(Relaxed) || wait.idle(0) {
                sleep(&shared, w, last);
                wait = Wait::new(shared.patience);
            }
            std::hint::spin_loop();
        };
        shared.inside.fetch_add(1, SeqCst);
        let job = shared.job.load(SeqCst);
        if job.is_null() {
            last = seen;
        } else {
            // SAFETY: `inside` counts this worker, so the caller of `run` waits for it before the
            // job leaves its stack (Dekker's pattern, above). The job is copied out at once.
            let job = unsafe { *job };
            if job.id > last {
                last = job.id;
                let ran = help(&job, me, Role::Worker(shared.patience));
                w.tasks.fetch_add(ran, Relaxed);
            }
        }
        shared.inside.fetch_sub(1, SeqCst);
    }
}

/// Park until a waker takes this worker's flag (or the pool stops). Set the flag first, then look
/// once more for a run published since `last`: either this sees it, or its waker sees the flag.
fn sleep(shared: &Shared, w: &Worker, last: u64) {
    w.parks.fetch_add(1, Relaxed);
    w.parked.store(true, SeqCst);
    if shared.seq.load(SeqCst) != last || shared.stop.load(SeqCst) {
        if !w.parked.swap(false, SeqCst) {
            // A waker took the flag and signals: let that signal land now, so at most one is
            // ever pending.
            sync::park();
        }
        return;
    }
    while w.parked.load(SeqCst) && !shared.stop.load(SeqCst) {
        sync::park();
    }
}

/// Whether a count has stayed the same for a while. Reads the clock once every 64 calls.
struct Wait {
    limit: Duration,
    seen: u64,
    calls: u32,
    since: Option<Instant>,
}

impl Wait {
    fn new(limit: Duration) -> Wait {
        Wait { limit, seen: u64::MAX, calls: 0, since: None }
    }

    /// Whether `now` has been the same for at least the limit.
    fn idle(&mut self, now: u64) -> bool {
        if now != self.seen {
            self.seen = now;
            self.calls = 0;
            self.since = None;
            return false;
        }
        self.calls = self.calls.wrapping_add(1);
        if !self.calls.is_multiple_of(64) {
            return false;
        }
        match self.since {
            None => {
                self.since = Some(Instant::now());
                false
            }
            Some(at) => at.elapsed() >= self.limit,
        }
    }
}

/// A small xorshift generator for the spread spins.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}
