//! A live session's warped renders, made in the background (the sx plan's slice 3, D10): a live
//! [`Cache`](crate::audio::Cache) never waits for a stretch. It hands each new render to its one
//! stretch thread, `m2-stretch`, which makes them in the order asked, each from the clip's start:
//! `std:stretch` runs block by block into the render's part file, and the render says how much of
//! it is in the file as it goes ([`Raw::made`]). Lanes play what is done, as it is done; a chunk
//! not done yet stays out of the ring, so it plays silence and counts a miss (RENDER-30). Once
//! whole, the part file is renamed to its cache name. A render the cache's next prune drops stops,
//! and its part file goes.
//!
//! An offline cache (an export's, a range render's) has no stretch thread: it makes each render
//! whole before its build returns, with the same maker ([`make_now`]), so its bits never depend on
//! timing. A render the cache folder can't take is made in memory instead ([`Making`], review
//! sx-r1); one that still fails fails the build offline, naming its clip (`stretch.failed`), and
//! live is marked failed, so each lane that plays it reports so.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::offline::{self, Kernel};
use crate::sandbox::Held;
use crate::stream::{Making, Raw};

/// One render to make: its kernel and what it is given, and where it goes, which lanes read.
pub(crate) struct Job {
    pub(crate) kernel: Kernel,
    pub(crate) source: Held,
    pub(crate) values: Vec<f64>,
    pub(crate) map: Vec<[f64; 2]>,
    pub(crate) rate: f64,
    pub(crate) out: Making,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    /// A job is being made.
    busy: bool,
    /// The test seam: how many frames a render may make before it waits; none, all it needs.
    hold: Option<usize>,
    /// The job being made waits at the hold.
    waiting: bool,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    /// Wakes the thread (a job, a hold let go, a stop, the end) and whoever waits for it to settle.
    wake: Condvar,
    /// A hold is on: only then does the thread look at the queue between blocks.
    held: AtomicBool,
    /// The cache is gone: the thread ends.
    closing: AtomicBool,
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait<'a>(&self, q: MutexGuard<'a, Queue>) -> MutexGuard<'a, Queue> {
        self.wake.wait(q).unwrap_or_else(|e| e.into_inner())
    }
}

/// A live cache's stretch thread: started with its first render, ended with the cache.
#[derive(Default)]
pub(crate) struct Stretcher {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Stretcher {
    /// Make `job` after those asked before it.
    pub(crate) fn push(&mut self, job: Job) {
        if self.thread.is_none() {
            let shared = self.shared.clone();
            match std::thread::Builder::new().name("m2-stretch".into()).spawn(move || work(&shared)) {
                Ok(t) => self.thread = Some(t),
                Err(e) => {
                    eprintln!("music2: no stretch thread ({e}): warped clips play silence");
                    job.out.raw().end();
                    return;
                }
            }
        }
        self.shared.queue().jobs.push_back(job);
        self.nudge();
    }

    /// Drop the jobs asked to stop that haven't started, and their part files; the one being made
    /// stops at its next block.
    pub(crate) fn forget_stopped(&self) {
        let dropped: Vec<Job> = {
            let mut q = self.shared.queue();
            let (stopped, kept): (Vec<Job>, Vec<Job>) = q.jobs.drain(..).partition(|j| j.out.raw().stopped());
            q.jobs = kept.into();
            stopped
        };
        for j in dropped {
            j.out.raw().end();
        }
        self.nudge();
    }

    /// Wake the thread to look again: one waiting at a hold says so again if it still waits.
    fn nudge(&self) {
        self.shared.queue().waiting = false;
        self.shared.wake.notify_all();
    }

    /// The test seam: each render makes at most `frames` frames, then waits until this changes.
    pub(crate) fn hold(&self, frames: Option<usize>) {
        self.shared.queue().hold = frames;
        self.shared.held.store(frames.is_some(), Ordering::Release);
        self.nudge();
    }

    /// Wait until every render asked for is made, or the one being made waits at a hold, for `cap`
    /// at most. True when so.
    pub(crate) fn settle(&self, cap: Duration) -> bool {
        let t = Instant::now();
        let mut q = self.shared.queue();
        loop {
            if q.jobs.is_empty() && !q.busy || q.waiting {
                return true;
            }
            let Some(left) = cap.checked_sub(t.elapsed()) else { return false };
            q = self.shared.wake.wait_timeout(q, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

impl Drop for Stretcher {
    fn drop(&mut self) {
        self.shared.closing.store(true, Ordering::Release);
        let jobs: Vec<Job> = self.shared.queue().jobs.drain(..).collect();
        for j in jobs {
            j.out.raw().end();
        }
        self.shared.wake.notify_all();
        if let Some(t) = self.thread.take() {
            drop(t.join());
        }
    }
}

/// The thread: each job in turn, till the cache is gone.
fn work(shared: &Shared) {
    loop {
        let job = {
            let mut q = shared.queue();
            loop {
                if shared.closing.load(Ordering::Acquire) {
                    return;
                }
                if let Some(j) = q.jobs.pop_front() {
                    q.busy = true;
                    break j;
                }
                q = shared.wait(q);
            }
        };
        let raw = job.out.raw().clone();
        // One that failed is silent; each lane that plays it reports `stretch.failed` (RENDER-25).
        if make(shared, job).is_err() && !raw.stopped() && !shared.closing.load(Ordering::Acquire) {
            raw.fail();
        }
        shared.queue().busy = false;
        shared.wake.notify_all();
    }
}

/// Make `job` now, on this thread: an offline cache's render (review sx-r1: one maker for both).
pub(crate) fn make_now(job: Job) -> Result<()> {
    make(&Shared::default(), job)
}

/// Make one render, block by block, saying how much is done as it goes: into its part file, renamed
/// once whole, or into memory. One that stops or fails is over: what it didn't make plays silence,
/// and its part file goes.
fn make(shared: &Shared, job: Job) -> Result<()> {
    let Job { kernel, source, values, map, rate, mut out } = job;
    let raw = out.raw().clone();
    let (channels, frames) = raw.size();
    let stopped = || raw.stopped() || shared.closing.load(Ordering::Acquire);
    let run = offline::Run { source, values: &values, map: &map, rate, channels, frames };
    let mut sink = |b: &[&[f32]]| -> Result<()> {
        if shared.held.load(Ordering::Acquire) {
            held(shared, &raw, &mut out, &stopped)?;
        }
        if stopped() {
            bail!("stopped");
        }
        out.put(b)?;
        let w = out.written();
        if w > 0 && !raw.has(w) {
            raw.made(w);
        }
        Ok(())
    };
    let made = if stopped() { Err(anyhow::anyhow!("stopped")) } else { offline::run(&kernel, &run, &mut sink) };
    let made = made.and_then(|()| out.finish());
    if made.is_err() {
        raw.end();
    }
    made
}

/// While a hold is on and this render made all it allows: its frames put are in the file and said
/// done, and it waits for the hold to change, or to be stopped.
fn held(shared: &Shared, raw: &Raw, out: &mut Making, stopped: &dyn Fn() -> bool) -> Result<()> {
    let mut q = shared.queue();
    while q.hold.is_some_and(|n| out.frames_put() >= n) && !stopped() {
        out.flush()?;
        raw.made(out.written());
        q.waiting = true;
        shared.wake.notify_all();
        q = shared.wait(q);
    }
    q.waiting = false;
    Ok(())
}
