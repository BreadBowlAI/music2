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
use std::time::{Duration, Instant};

use anyhow::Result;
use m2_os::process::Cores;
use m2_os::rt::{Group, Joined};
use m2_runtime::threads::Joins;
use m2_runtime::{Counters, Meter, Pool, Runner};

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
    /// The callback took them: blocks render on them from now on.
    pub taken: bool,
    /// How many threads the output's workgroup advises (macOS), the device's own included.
    pub advice: Option<usize>,
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
    /// The output's workgroup refused a worker as cancelled: the output changed under it, so a
    /// new player should take over.
    pub(crate) cancelled: bool,
}

impl Engines {
    /// One thread: the device's own.
    fn alone(group: Arc<Group>, between: Between) -> Engines {
        Engines { pool: None, handover: Box::new(Handover { runner: Runner::one(), group, between }), cancelled: false }
    }
}

/// Debug builds run every engine thread under the allocation trap, as the device's own callback
/// is (RENDER-28): each worker from its start to its stop. A release build marks nothing.
const TRAP: Option<fn(bool)> = if cfg!(debug_assertions) { Some(crate::alloc::set_in_callback) } else { None };

/// How long the player's thread waits for the device's first callback, and for its workers to
/// start: then it plays on one thread, and says so.
const WAIT: Duration = Duration::from_secs(1);

/// What a player says of its engine threads: written as they are made, and by the callback.
#[derive(Default)]
pub(crate) struct Report {
    count: AtomicUsize,
    real_time: AtomicUsize,
    joined: AtomicUsize,
    /// The device thread's [`Joined`]: 1 real time, 2 in the workgroup, 4 cancelled; 8 once the
    /// callback took its engine threads.
    device: AtomicU8,
    spin: AtomicU8,
    device_mode: AtomicU64,
    missing: OnceLock<String>,
    /// Reads the pool's counters (G12).
    meter: OnceLock<Meter>,
    /// The workgroup's advice: 0 for none.
    advice: AtomicUsize,
}

impl Report {
    /// Engine threads as made: `count` threads, their workers' reports.
    pub(crate) fn made(&self, count: usize, joins: Option<&Joins>, meter: Option<Meter>, between: Between, missing: Option<String>) {
        self.count.store(count, Ordering::Relaxed);
        if let Some(m) = meter {
            let _ = self.meter.set(m);
        }
        if let Some(j) = joins {
            self.real_time.store(j.real_time.load(Ordering::Relaxed), Ordering::Relaxed);
            self.joined.store(j.joined.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        self.spin.store(u8::from(between == Between::Spin), Ordering::Relaxed);
        if let Some(m) = missing {
            let _ = self.missing.set(m);
        }
    }

    /// What the output's workgroup advises, as its group was read.
    pub(crate) fn advise(&self, threads: Option<usize>) {
        self.advice.store(threads.unwrap_or(0), Ordering::Relaxed);
    }

    /// The device's thread took its engine threads. On the audio thread: atomics only.
    pub(crate) fn device(&self, j: Joined) {
        self.device.store(u8::from(j.real_time) | u8::from(j.workgroup) << 1 | u8::from(j.cancelled) << 2 | 8, Ordering::Relaxed);
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
            taken: d & 8 != 0,
            advice: Some(self.advice.load(Ordering::Relaxed)).filter(|&n| n > 0),
        }
    }

    /// The pool's counters, the device's thread first: empty on one thread.
    pub(crate) fn counters(&self) -> Vec<Counters> {
        self.meter.get().map(Meter::counters).unwrap_or_default()
    }
}

/// Engine threads of no device: plain threads at the clock's period, `frames` frames at `rate`
/// (the silent player). One, unless `live.count` says more.
pub(crate) fn for_clock(frames: usize, rate: u32, live: LiveThreads, report: &Report) -> Engines {
    let period = Duration::from_secs_f64(frames.max(1) as f64 / rate.max(1) as f64);
    let group = Arc::new(Group::none(Some(period)));
    let count = live_count(live.count, &m2_os::process::cores(), None, false);
    made(count, group, live.between, report)
}

/// A pool of `count` threads for `group`'s engine threads; one thread when `count` is 1 or the
/// OS won't start them. Reported in `report`.
fn made(count: usize, group: Arc<Group>, between: Between, report: &Report) -> Engines {
    let one = |group: Arc<Group>, missing: Option<String>| {
        report.made(1, None, None, between, missing);
        Engines::alone(group, between)
    };
    if count <= 1 {
        return one(group, None);
    }
    let (setup, joins) = m2_runtime::threads::live(group.clone(), between == Between::Spin, TRAP);
    match Pool::new(count, setup) {
        Ok((pool, runner)) => {
            report.made(count, Some(&joins), Some(pool.meter()), between, None);
            Engines { pool: Some(pool), handover: Box::new(Handover { runner, group, between }), cancelled: false }
        }
        Err(e) => one(group, Some(format!("one thread: the OS wouldn't start {} more ({e})", count - 1))),
    }
}

/// Engine threads of the output whose stream just started, `uid` its device id (None: the default
/// output), at the device's `rate`. Off the audio thread, on the player's own. The period is the
/// first callback's frames at `rate` (`first` says them: 0 before it); then the output's group
/// is read (macOS: its IO thread's workgroup, which exists once IO runs), and the pool made at the
/// rule's count. The pool is kept only when every worker started and runs as real time
/// ([`judged`]); else one thread, and [`EngineThreads::missing`] says why.
pub(crate) fn for_device(uid: Option<&str>, rate: u32, live: LiveThreads, first: &dyn Fn() -> u32, report: &Report) -> Engines {
    let one = |missing: String| {
        report.made(1, None, None, live.between, Some(missing));
        Engines::alone(Arc::new(Group::none(None)), live.between)
    };
    let start = Instant::now();
    let frames = loop {
        match first() {
            0 if start.elapsed() < WAIT => std::thread::sleep(Duration::from_millis(1)),
            0 => return one("one thread: the device didn't call back within a second".into()),
            n => break n,
        }
    };
    let group = Arc::new(Group::of_output(uid, Duration::from_secs_f64(f64::from(frames) / f64::from(rate.max(1)))));
    report.advise(group.recommends());
    let count = live_count(live.count, &m2_os::process::cores(), group.recommends(), true);
    if count <= 1 {
        report.made(1, None, None, live.between, group.missing().map(str::to_string));
        return Engines::alone(group, live.between);
    }
    let (setup, joins) = m2_runtime::threads::live(group.clone(), live.between == Between::Spin, TRAP);
    let (pool, runner) = match Pool::new(count, setup) {
        Ok(made) => made,
        Err(e) => return one(format!("one thread: the OS wouldn't start {} more ({e})", count - 1)),
    };
    let workers = count - 1;
    let start = Instant::now();
    while joins.started.load(Ordering::SeqCst) < workers && start.elapsed() < WAIT {
        std::thread::sleep(Duration::from_millis(1));
    }
    let n = |c: &AtomicUsize| c.load(Ordering::SeqCst);
    let said = Said {
        workers,
        started: n(&joins.started),
        real_time: n(&joins.real_time),
        joined: n(&joins.joined),
        cancelled: n(&joins.cancelled),
    };
    let (keep, missing) = judged(&said, group.recommends().is_some());
    let missing = missing.or_else(|| group.missing().map(str::to_string));
    if !keep {
        // Its workers leave and are joined here, off the audio thread.
        drop(pool);
        let mut e = one(missing.unwrap_or_default());
        e.cancelled = said.cancelled > 0;
        return e;
    }
    report.made(count, Some(&joins), Some(pool.meter()), live.between, missing);
    Engines { pool: Some(pool), handover: Box::new(Handover { runner, group, between: live.between }), cancelled: false }
}

/// What a device's workers said as they started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Said {
    pub(crate) workers: usize,
    pub(crate) started: usize,
    pub(crate) real_time: usize,
    pub(crate) joined: usize,
    pub(crate) cancelled: usize,
}

/// Whether a device's pool is kept (D4): only when every worker started and runs as real time. A
/// worker that isn't real time can be preempted inside a block while the device's thread waits for
/// it, which is worse than one thread. A worker that didn't join a workgroup the output has plays
/// on, and is said. What is missing, in plain words.
pub(crate) fn judged(s: &Said, has_workgroup: bool) -> (bool, Option<String>) {
    let of = |n: usize| format!("{n} of {}", s.workers);
    if s.started < s.workers {
        return (false, Some(format!("one thread: {} engine threads didn't start within a second", of(s.workers - s.started))));
    }
    if s.cancelled > 0 {
        return (false, Some("one thread: the output's workgroup was cancelled, as the output changed".into()));
    }
    if s.real_time < s.workers {
        return (
            false,
            Some(format!(
                "one thread: {} engine threads were refused real time (this user may not run real-time threads)",
                of(s.workers - s.real_time)
            )),
        );
    }
    if has_workgroup && s.joined < s.workers {
        return (true, Some(format!("{} engine threads didn't join the output's workgroup", of(s.workers - s.joined))));
    }
    (true, None)
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
        let mac = Cores { fast: 8, slow: 2, per_cache: 4, per_last_cache: 8 };
        let server = Cores { fast: 16, slow: 0, per_cache: 8, per_last_cache: 8 };
        assert_eq!(live_count(Some(3), &mac, Some(2), true), 3, "asked for");
        assert_eq!((live_count(Some(0), &mac, None, true), live_count(Some(99), &mac, None, true)), (1, 64));
        assert_eq!((live_count(None, &mac, None, false), live_count(None, &server, None, false)), (1, 1), "no device: one");
        assert_eq!(live_count(None, &mac, Some(8), true), 4, "one L2's P-cores, though the workgroup allows 8");
        assert_eq!(live_count(None, &mac, Some(2), true), 2, "the workgroup advises fewer");
        assert_eq!(live_count(None, &mac, Some(0), true), 1);
        assert_eq!(live_count(None, &server, None, true), 8, "one die here");
        let tiny = Cores { fast: 1, slow: 0, per_cache: 1, per_last_cache: 1 };
        assert_eq!(live_count(None, &tiny, Some(4), true), 1);
    }

    // RENDER-28a (8a's live half, D4): a device's pool is kept only when every worker started and
    // runs as real time; else one thread, said in plain words. A worker outside a workgroup the
    // output has plays on, and is said.
    #[test]
    fn a_device_pool_is_kept_only_when_every_worker_is_real_time() {
        let all = Said { workers: 3, started: 3, real_time: 3, joined: 3, cancelled: 0 };
        assert_eq!(judged(&all, true), (true, None));
        assert_eq!(judged(&Said { joined: 0, ..all }, false), (true, None), "no workgroup here to join");
        let cases = [
            (Said { started: 2, ..all }, true, "1 of 3 engine threads didn't start"),
            (Said { real_time: 2, ..all }, false, "1 of 3 engine threads were refused real time"),
            (Said { real_time: 0, joined: 0, ..all }, false, "3 of 3 engine threads were refused real time"),
            (Said { cancelled: 1, ..all }, false, "the output's workgroup was cancelled"),
        ];
        for (said, has, words) in cases {
            let (keep, missing) = judged(&said, has);
            assert!(
                !keep && missing.as_deref().is_some_and(|m| m.starts_with("one thread: ") && m.contains(words)),
                "{said:?}: {missing:?}"
            );
        }
        let (keep, missing) = judged(&Said { joined: 1, ..all }, true);
        assert!(keep && missing.as_deref() == Some("2 of 3 engine threads didn't join the output's workgroup"), "{missing:?}");
    }

    // RENDER-28a (8a's live half): on this machine, a device's engine threads are made as a
    // device's are (its first callback's period, its group, real time asked for), and kept only
    // when real time was granted: here, a user with no rtprio limit gets one thread, and says why.
    #[test]
    fn a_device_without_real_time_plays_on_one_thread() {
        let report = Report::default();
        let live = LiveThreads { count: Some(4), between: Between::Park };
        let e = for_device(None, 48_000, live, &|| 128, &report);
        let probe = std::thread::spawn(|| {
            let j = m2_os::rt::engine_thread(&Group::of_output(None, Duration::from_millis(3)));
            m2_os::rt::end_engine_thread();
            j.real_time
        });
        let granted = probe.join().unwrap();
        let said = report.read();
        match granted {
            true => assert!(e.pool.is_some() && said.count == 4 && said.real_time == 3, "{said:?}"),
            false => assert!(
                e.pool.is_none() && said.count == 1 && said.missing.as_deref().is_some_and(|m| m.contains("refused real time")),
                "{said:?}"
            ),
        }
        let no_callback = for_device(None, 48_000, live, &|| 0, &Report::default());
        assert!(no_callback.pool.is_none());
        // The workgroup's advice, as read (none here), reaches G12's set-up line.
        assert_eq!(said.advice, Group::of_output(None, Duration::from_millis(3)).recommends());
        report.advise(Some(3));
        assert_eq!(report.read().advice, Some(3));
    }
}
