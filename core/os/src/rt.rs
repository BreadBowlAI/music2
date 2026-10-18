//! Engine threads: the threads that render live playback, set up as the OS sets up audio threads
//! (8a's live half, RENDER-28a).
//!
//! In short:
//! - [`Wake`]: one thread sleeps until another rings, or a time passes. The device's own thread
//!   sleeps on one when a task it waits for is stuck. No allocation, no lock: a futex on Linux, a
//!   Mach semaphore on macOS.
//! - [`Group`]: what engine threads need from the output device: its period, and on macOS the
//!   audio workgroup of its IO thread.
//! - [`engine_thread`], [`end_engine_thread`], [`on_device_thread`]: a thread's set-up, once, at
//!   its start or on the fill that hands it its engine threads. macOS: the time-constraint policy
//!   at the device's period, and the workgroup joined. Linux: `SCHED_FIFO` as far as this user may
//!   (`RLIMIT_RTPRIO`). Never fatal: what a thread got is in its [`Joined`].

use std::io;
use std::time::Duration;

use crate::os;

/// One thread sleeps until another rings, or a time passes.
///
/// - A ring before the wait makes the wait return at once.
/// - At most one ring may be waiting: its user keeps it so (the pool rings only after taking a
///   flag its sleeper set). macOS's semaphore would count more; Linux's word holds one.
/// - A wait ended early (a signal, a spurious wake-up) sleeps again for what is left.
/// - No allocation, no lock: any thread may ring or wait, one std did not start included.
pub struct Wake(os::wake::Wake);

impl Wake {
    /// A new wake, not rung. Off the audio thread: macOS makes a semaphore.
    pub fn new() -> io::Result<Wake> {
        os::wake::Wake::new().map(Wake).map_err(|code| io::Error::other(format!("no semaphore for a wake: Mach error {code}")))
    }

    /// Ring: the waiter wakes, or its next wait returns at once. Never blocks.
    pub fn ring(&self) {
        self.0.ring()
    }

    /// Sleep until rung, or until `at_most` has passed: true when rung (and the ring is taken),
    /// false when the time ran out.
    pub fn wait(&self, at_most: Duration) -> bool {
        self.0.wait(at_most)
    }
}

/// What engine threads need from the output device: its period, and on macOS the audio workgroup
/// of its IO thread, which every engine thread joins. Released when dropped.
pub struct Group {
    period: Option<Duration>,
    /// Whether engine threads ask for real time: a device plays.
    asks: bool,
    workgroup: os::rt::Workgroup,
    missing: Option<String>,
}

impl Group {
    /// The group of the output device named by `uid` (cpal's device id), or of the default output
    /// when None, whose callbacks come every `period`. Read it after the device's IO started: its
    /// workgroup belongs to its IO thread. macOS: the IO thread's audio workgroup. Linux has no
    /// workgroups. Never fails: what is missing is in [`Group::missing`].
    pub fn of_output(uid: Option<&str>, period: Duration) -> Group {
        let (workgroup, missing) = match os::rt::output_workgroup(uid) {
            Ok(w) => (w, None),
            Err(why) => (os::rt::Workgroup::none(), Some(why)),
        };
        Group { period: Some(period), asks: true, workgroup, missing }
    }

    /// No device (the silent and driven players): engine threads are plain threads.
    pub fn none(period: Option<Duration>) -> Group {
        let missing = Some("no audio device, so engine threads are plain threads: no real time, no workgroup".to_string());
        Group { period, asks: false, workgroup: os::rt::Workgroup::none(), missing }
    }

    /// The plugin host process's audio threads (HOST-3): real time at `period`, as engine threads
    /// ask, in no workgroup (joining the app's output workgroup from another process is a gap).
    pub fn real_time(period: Duration) -> Group {
        let missing = Some("another process's audio thread: real time, but in no workgroup".to_string());
        Group { period: Some(period), asks: true, workgroup: os::rt::Workgroup::none(), missing }
    }

    /// How often the device calls back.
    pub fn period(&self) -> Option<Duration> {
        self.period
    }

    /// How many threads the workgroup says can work at once (macOS:
    /// `os_workgroup_max_parallel_threads`); None without a workgroup.
    pub fn recommends(&self) -> Option<usize> {
        self.workgroup.recommends()
    }

    /// What this group lacks, in plain words: no device, or no workgroup and why. None when it
    /// has all this OS gives.
    pub fn missing(&self) -> Option<&str> {
        self.missing.as_deref()
    }
}

/// What a thread got when it became an engine thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Joined {
    /// It runs as real time: macOS's time-constraint policy at the device's period; Linux's
    /// `SCHED_FIFO`.
    pub real_time: bool,
    /// It joined the output device's audio workgroup (macOS). Linux has none: false.
    pub workgroup: bool,
    /// The workgroup refused it as cancelled: the output changed under it, and the player should
    /// be replaced.
    pub cancelled: bool,
}

/// Make the calling thread an engine thread of `g`'s device: real time at its period, in its
/// workgroup. Once, at the thread's start, before it renders; [`end_engine_thread`] before it
/// exits. A group with no device asks for nothing.
pub fn engine_thread(g: &Group) -> Joined {
    match g.asks {
        true => os::rt::engine_thread(g.period, &g.workgroup),
        false => Joined::default(),
    }
}

/// Before an engine thread exits: it leaves `g`'s workgroup, which it joined (macOS). The caller
/// holds `g` until then: the leave needs the workgroup, which `g` keeps.
pub fn end_engine_thread(g: &Group) {
    os::rt::end_engine_thread(&g.workgroup)
}

/// Once, on the device's own callback thread. macOS: nothing to do, CoreAudio made its IO thread
/// real time and put it in the group. Linux: `SCHED_FIFO`, one step above the workers.
pub fn on_device_thread(g: &Group) -> Joined {
    match g.asks {
        true => os::rt::on_device_thread(),
        false => Joined::default(),
    }
}

/// `ns` nanoseconds in Mach ticks, for a timebase of `numer / denom` ns a tick.
#[allow(dead_code)] // macOS's time-constraint policy; tested on every OS
pub(crate) fn ticks(ns: u64, numer: u32, denom: u32) -> u32 {
    (u128::from(ns) * u128::from(denom) / u128::from(numer.max(1))).min(u128::from(u32::MAX)) as u32
}

/// The time-constraint policy for a device period, in ticks: (period, computation, constraint).
/// An engine thread may compute for half a period, and must be done within one.
#[allow(dead_code)] // macOS's time-constraint policy; tested on every OS
pub(crate) fn policy(period: Duration, numer: u32, denom: u32) -> (u32, u32, u32) {
    let ns = period.as_nanos().min(u128::from(u64::MAX)) as u64;
    let p = ticks(ns, numer, denom);
    (p, p / 2, p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    /// Run `f` on its own thread; fail if it hasn't returned in 60 s (a hang).
    fn watched(f: impl FnOnce() + Send + 'static) {
        let h = std::thread::spawn(f);
        let t = Instant::now();
        while !h.is_finished() {
            assert!(t.elapsed() < Duration::from_secs(60), "hung for 60 s");
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Err(p) = h.join() {
            std::panic::resume_unwind(p);
        }
    }

    // 8a: a ring before the wait makes the wait return at once, whatever it was given.
    #[test]
    fn a_wake_rung_before_its_wait_returns_at_once() {
        watched(|| {
            let w = Wake::new().unwrap();
            w.ring();
            assert!(w.wait(Duration::from_secs(3600)));
        });
    }

    // 8a: a thread that waits an hour wakes when another rings.
    #[test]
    fn a_ring_from_another_thread_ends_the_wait() {
        watched(|| {
            let w = Arc::new(Wake::new().unwrap());
            let ringer = {
                let w = w.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(20));
                    w.ring();
                })
            };
            assert!(w.wait(Duration::from_secs(3600)), "rung");
            ringer.join().unwrap();
        });
    }

    // 8a: a wait no one rings ends when its time runs out, and says so.
    #[test]
    fn an_unrung_wait_times_out() {
        let w = Wake::new().unwrap();
        let t = Instant::now();
        assert!(!w.wait(Duration::from_millis(5)));
        assert!(t.elapsed() >= Duration::from_millis(5), "it waited the whole time: {:?}", t.elapsed());
        // One ring is taken once.
        w.ring();
        assert!(w.wait(Duration::ZERO));
        assert!(!w.wait(Duration::ZERO));
    }

    // 8a: both sides on threads std didn't start (CoreAudio's IO thread is one).
    #[test]
    fn a_wake_works_on_threads_std_did_not_start() {
        watched(|| {
            let w = Arc::new(Wake::new().unwrap());
            let waiter = {
                let w = w.clone();
                std::thread::spawn(move || crate::thread::foreign(&mut || assert!(w.wait(Duration::from_secs(3600)))).unwrap())
            };
            std::thread::sleep(Duration::from_millis(20));
            crate::thread::foreign(&mut || w.ring()).unwrap();
            waiter.join().unwrap();
        });
    }

    // 8a: without a device, engine threads are plain threads, and the group says why.
    #[test]
    fn a_group_without_a_device_has_no_workgroup_and_says_so() {
        let g = Group::none(Some(Duration::from_micros(2667)));
        assert_eq!(g.recommends(), None);
        assert!(g.missing().is_some_and(|m| m.contains("no audio device")), "{:?}", g.missing());
        let joined = std::thread::spawn(move || {
            let j = engine_thread(&g);
            end_engine_thread(&g);
            j
        });
        assert_eq!(joined.join().unwrap(), Joined::default());
    }

    // 8a (D22): the time-constraint policy in Mach ticks. Apple silicon's timebase is 125/3 ns a
    // tick (24 MHz), so a 128-frame period at 48 kHz is 64,000 ticks; Intel's is 1/1.
    #[test]
    fn the_time_constraint_policy_is_in_ticks() {
        assert_eq!(ticks(2_666_667, 125, 3), 64_000);
        assert_eq!(policy(Duration::from_nanos(2_666_667), 125, 3), (64_000, 32_000, 64_000));
        assert_eq!(policy(Duration::from_millis(10), 1, 1), (10_000_000, 5_000_000, 10_000_000));
        assert_eq!(ticks(u64::MAX, 1, 1), u32::MAX, "never wraps");
    }
}
