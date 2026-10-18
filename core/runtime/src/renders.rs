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
//! whole before its build returns, so its bits never depend on timing.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::offline::{self, Kernel};
use crate::sandbox::Held;
use crate::stream::{Part, Raw};

/// One render to make: its kernel and what it is given, its part file, and the render lanes read.
pub(crate) struct Job {
    pub(crate) kernel: Kernel,
    pub(crate) source: Held,
    pub(crate) values: Vec<f64>,
    pub(crate) map: Vec<[f64; 2]>,
    pub(crate) rate: f64,
    pub(crate) part: Part,
    pub(crate) raw: Arc<Raw>,
    /// The clip's file, for the log when it fails.
    pub(crate) asset: String,
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
                    job.raw.end();
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
            let (stopped, kept): (Vec<Job>, Vec<Job>) = q.jobs.drain(..).partition(|j| j.raw.stopped());
            q.jobs = kept.into();
            stopped
        };
        for j in dropped {
            j.raw.end();
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
            j.raw.end();
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
        make(shared, job);
        shared.queue().busy = false;
        shared.wake.notify_all();
    }
}

/// Make one render into its part file, saying how much is done as it goes; renamed once whole. One
/// that stops or fails is over: what it didn't make plays silence, and its part file goes.
fn make(shared: &Shared, job: Job) {
    let Job { kernel, source, values, map, rate, mut part, raw, asset } = job;
    let stopped = || raw.stopped() || shared.closing.load(Ordering::Acquire);
    let run = offline::Run { source, values: &values, map: &map, rate, channels: part.channels, frames: part.frames };
    let mut sink = |b: &[&[f32]]| -> Result<()> {
        if shared.held.load(Ordering::Acquire) {
            held(shared, &raw, &mut part, &stopped)?;
        }
        if stopped() {
            bail!("stopped");
        }
        part.put(b)?;
        let w = part.written();
        if w > 0 && !raw.has(w) {
            raw.made(w);
        }
        Ok(())
    };
    let made = if stopped() { Err(anyhow::anyhow!("stopped")) } else { offline::run(&kernel, &run, &mut sink) };
    let frames = part.frames;
    match made.and_then(|()| part.finish()) {
        Ok(()) => raw.made(frames),
        Err(e) => {
            if !stopped() {
                eprintln!("music2: a warped clip of {asset} plays silence: its stretch failed: {e:#}");
            }
            raw.end();
        }
    }
}

/// While a hold is on and this render made all it allows: its frames put are in the file and said
/// done, and it waits for the hold to change, or to be stopped.
fn held(shared: &Shared, raw: &Raw, part: &mut Part, stopped: &dyn Fn() -> bool) -> Result<()> {
    let mut q = shared.queue();
    while q.hold.is_some_and(|n| part.frames_put() >= n) && !stopped() {
        part.flush()?;
        raw.made(part.written());
        q.waiting = true;
        shared.wake.notify_all();
        q = shared.wait(q);
    }
    q.waiting = false;
    Ok(())
}
