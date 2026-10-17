//! A player's engine threads (8a's live half, RENDER-28a).
//!
//! In short:
//! - The device's own thread renders each block on a pool of threads, as an export does. Its
//!   workers are engine threads: real time at the device's period, and on a Mac in the output's
//!   audio workgroup (`m2_os::rt`). After each callback they sleep ([`BETWEEN`]).
//! - How many, the device's thread included ([`live_count`]): `M2_THREADS` if set; else one off a
//!   device (the silent and driven players); else the fast cores that share one cache, no more
//!   than the workgroup advises. On a device, a pool whose workers aren't all real time is
//!   dropped: one thread.
//! - The pool is made off the audio thread, once the device calls back (its period, and on a Mac
//!   its IO thread's workgroup), and its runner goes to the callback as a message ([`Handover`]).
//!   Until it lands the callback renders on its own thread. A song with plugins always does
//!   (HOST-2).
//! - A task's panic crashes the player, as a panic in the engine always did. A worker that never
//!   comes costs only speed. A stuck one puts the device's thread to sleep on its wake.

use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Result;
use m2_os::process::Cores;
use m2_os::rt::{Group, Joined};
use m2_runtime::threads::Joins;
use m2_runtime::{Pool, Runner};

/// How a player's engine threads run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveThreads {
    /// How many, the device's thread included: None for [`live_count`]'s rule.
    pub count: Option<usize>,
    pub between: Between,
}

/// What workers do between callbacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Between {
    /// They sleep at once: no CPU between callbacks, a wake-up each callback.
    Park,
    /// They spin for 1.25 periods, so the next callback finds them awake, then sleep.
    Spin,
}

/// After a callback, workers sleep at once. The owner's Mac sweep may switch this one line: to
/// `Spin` when spinning's p99 is at least 10% lower with no more dropouts (G12 `--between`).
pub const BETWEEN: Between = Between::Park;

impl LiveThreads {
    /// `M2_THREADS` when set, and [`BETWEEN`].
    pub fn from_env() -> Result<LiveThreads> {
        Ok(LiveThreads { count: m2_runtime::threads::from_env()?, between: BETWEEN })
    }
}

impl Default for LiveThreads {
    fn default() -> LiveThreads {
        LiveThreads { count: None, between: BETWEEN }
    }
}

/// The live thread count, the device's thread included: `asked` (`M2_THREADS`, tests, G12), 1 to
/// 64; else 1 off a device (the silent and driven players: other work shares this machine's
/// cores); else the fast cores that share one cache (a Mac's P-cores of one L2, never its E-cores;
/// one die here), no more than the workgroup `recommends`.
pub fn live_count(asked: Option<usize>, cores: &Cores, recommends: Option<usize>, on_device: bool) -> usize {
    match asked {
        Some(n) => n.clamp(1, Pool::MAX_THREADS),
        None if !on_device => 1,
        None => {
            let per_cache = cores.per_cache.clamp(1, Pool::MAX_THREADS);
            recommends.map_or(per_cache, |r| r.clamp(1, per_cache))
        }
    }
}

/// What a player's engine threads are and did, for G12's set-up line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineThreads {
    /// The threads that render, the device's own included.
    pub count: usize,
    /// Of the `count − 1` workers: how many run as real time, and how many joined the output's
    /// workgroup (macOS).
    pub real_time: usize,
    pub joined: usize,
    /// The device's own thread, as it reported when it took its engine threads.
    pub device: Joined,
    /// What is missing from the designed set-up, in plain words.
    pub missing: Option<String>,
    pub between: Between,
    /// The device thread's floating-point register as its last callback found it (RENDER-2a:
    /// the callback renders in the one mode, and puts this back after).
    pub device_mode: u64,
}

/// What a player's callback is handed: the runner of its pool, the output's group and how its
/// workers wait. Public only as what [`crate::Gone`] carries back to be dropped elsewhere.
#[doc(hidden)]
pub struct Handover {
    pub(crate) runner: Runner,
    pub(crate) group: Arc<Group>,
    pub(crate) between: Between,
}

/// A player's engine threads, as made: the pool (None on one thread), and what goes to the
/// callback.
pub(crate) struct Engines {
    pub(crate) pool: Option<Pool>,
    pub(crate) handover: Box<Handover>,
}

/// What a player says of its engine threads: written as they are made, and by the callback.
#[derive(Default)]
pub(crate) struct Report {
    count: AtomicUsize,
    real_time: AtomicUsize,
    joined: AtomicUsize,
    /// The device thread's [`Joined`]: 1 real time, 2 in the workgroup, 4 cancelled.
    device: AtomicU8,
    spin: AtomicU8,
    device_mode: AtomicU64,
    missing: OnceLock<String>,
}

impl Report {
    /// Engine threads as made: `count` threads, their workers' reports.
    pub(crate) fn made(&self, count: usize, joins: Option<&Joins>, between: Between, missing: Option<String>) {
        self.count.store(count, Ordering::Relaxed);
        if let Some(j) = joins {
            self.real_time.store(j.real_time.load(Ordering::Relaxed), Ordering::Relaxed);
            self.joined.store(j.joined.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        self.spin.store(u8::from(between == Between::Spin), Ordering::Relaxed);
        if let Some(m) = missing {
            let _ = self.missing.set(m);
        }
    }

    /// The device's thread took its engine threads. On the audio thread: atomics only.
    pub(crate) fn device(&self, j: Joined) {
        self.device.store(u8::from(j.real_time) | u8::from(j.workgroup) << 1 | u8::from(j.cancelled) << 2, Ordering::Relaxed);
    }

    /// The device thread's floating-point register as a callback found it.
    pub(crate) fn found(&self, mode: u64) {
        self.device_mode.store(mode, Ordering::Relaxed);
    }

    pub(crate) fn read(&self) -> EngineThreads {
        let d = self.device.load(Ordering::Relaxed);
        EngineThreads {
            count: self.count.load(Ordering::Relaxed).max(1),
            real_time: self.real_time.load(Ordering::Relaxed),
            joined: self.joined.load(Ordering::Relaxed),
            device: Joined { real_time: d & 1 != 0, workgroup: d & 2 != 0, cancelled: d & 4 != 0 },
            missing: self.missing.get().cloned(),
            between: if self.spin.load(Ordering::Relaxed) == 1 { Between::Spin } else { Between::Park },
            device_mode: self.device_mode.load(Ordering::Relaxed),
        }
    }
}

/// Engine threads of no device: plain threads at the clock's period, `frames` frames at `rate`
/// (the silent player). One, unless `live.count` says more.
pub(crate) fn for_clock(frames: usize, rate: u32, live: LiveThreads, report: &Report) -> Engines {
    let period = Duration::from_secs_f64(frames.max(1) as f64 / rate.max(1) as f64);
    let group = Arc::new(Group::none(Some(period)));
    let count = live_count(live.count, &m2_os::process::cores(), None, false);
    made(count, group, live.between, None, report)
}

/// A pool of `count` threads for `group`'s engine threads; one thread when `count` is 1 or the
/// OS won't start them. Reported in `report`.
fn made(count: usize, group: Arc<Group>, between: Between, mark: Option<fn(bool)>, report: &Report) -> Engines {
    let one = |group: Arc<Group>, missing: Option<String>| {
        report.made(1, None, between, missing);
        Engines { pool: None, handover: Box::new(Handover { runner: Runner::one(), group, between }) }
    };
    if count <= 1 {
        return one(group, None);
    }
    let (setup, joins) = m2_runtime::threads::live(group.clone(), between == Between::Spin, mark);
    match Pool::new(count, setup) {
        Ok((pool, runner)) => {
            report.made(count, Some(&joins), between, None);
            Engines { pool: Some(pool), handover: Box::new(Handover { runner, group, between }) }
        }
        Err(e) => one(group, Some(format!("one thread: the OS wouldn't start {} more ({e})", count - 1))),
    }
}

/// After each callback: with [`Between::Park`], workers sleep at once.
pub(crate) fn after_callback(runner: &mut Runner, between: Between) {
    if between == Between::Park {
        runner.rest();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RENDER-28a: the live thread count. A count asked for wins (1 to 64); off a device, one; on
    // a device, the fast cores of one cache, no more than the workgroup advises.
    #[test]
    fn the_live_count_follows_the_rule() {
        let mac = Cores { fast: 8, slow: 2, per_cache: 4 };
        let server = Cores { fast: 16, slow: 0, per_cache: 8 };
        assert_eq!(live_count(Some(3), &mac, Some(2), true), 3, "asked for");
        assert_eq!((live_count(Some(0), &mac, None, true), live_count(Some(99), &mac, None, true)), (1, 64));
        assert_eq!((live_count(None, &mac, None, false), live_count(None, &server, None, false)), (1, 1), "no device: one");
        assert_eq!(live_count(None, &mac, Some(8), true), 4, "one L2's P-cores, though the workgroup allows 8");
        assert_eq!(live_count(None, &mac, Some(2), true), 2, "the workgroup advises fewer");
        assert_eq!(live_count(None, &mac, Some(0), true), 1);
        assert_eq!(live_count(None, &server, None, true), 8, "one die here");
        let tiny = Cores { fast: 1, slow: 0, per_cache: 1 };
        assert_eq!(live_count(None, &tiny, Some(4), true), 1);
    }
}
