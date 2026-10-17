//! Linux: engine threads ask for `SCHED_FIFO` within `RLIMIT_RTPRIO`, as far as a user without
//! root may. No rtkit, no `RLIMIT_RTTIME` (Deferred). Linux has no audio workgroups.

use std::time::Duration;

use crate::rt::Joined;

/// The highest FIFO priority asked for: the device thread's.
const TOP: u64 = 10;

/// Linux has none.
pub(crate) struct Workgroup;

impl Workgroup {
    pub(crate) fn none() -> Workgroup {
        Workgroup
    }

    pub(crate) fn recommends(&self) -> Option<usize> {
        None
    }
}

pub(crate) fn output_workgroup(_uid: Option<&str>) -> Result<Workgroup, String> {
    Ok(Workgroup)
}

/// A worker: one step below the device thread when the limit leaves room (equal FIFO threads
/// never preempt each other, so a spinning worker would hold a core the device thread wants).
pub(crate) fn engine_thread(_period: Option<Duration>, _w: &Workgroup) -> Joined {
    Joined { real_time: fifo(false), ..Joined::default() }
}

pub(crate) fn end_engine_thread() {}

pub(crate) fn on_device_thread() -> Joined {
    Joined { real_time: fifo(true), ..Joined::default() }
}

/// `SCHED_FIFO` for the calling thread: the device thread at `min(RLIMIT_RTPRIO, 10)`, a worker
/// one lower when that is 2 or more. The soft limit is raised to the hard one first, which needs
/// no rights. With a limit of 0 it still asks: root and `CAP_SYS_NICE` may. Whether it was granted.
fn fifo(device: bool) -> bool {
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: getrlimit and setrlimit on a struct that outlives the calls.
    unsafe {
        libc::getrlimit(libc::RLIMIT_RTPRIO, &mut limit);
        if limit.rlim_cur < limit.rlim_max {
            limit.rlim_cur = limit.rlim_max;
            libc::setrlimit(libc::RLIMIT_RTPRIO, &limit);
        }
    }
    let cap = match limit.rlim_cur {
        0 => TOP,
        l => l.min(TOP),
    };
    let prio = if !device && cap >= 2 { cap - 1 } else { cap };
    let param = libc::sched_param { sched_priority: prio as libc::c_int };
    // SAFETY: 0 is the calling thread; `param` outlives the call.
    unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &param) == 0 }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::rt::{Group, end_engine_thread, engine_thread, on_device_thread};

    // 8a (D4): a thread that asks for real time without the rights to it is told so, and runs on.
    // On a desktop whose user may (an `rtprio` limit), it is FIFO. This server refuses: its
    // granted branch is a desktop's.
    #[test]
    fn an_engine_thread_gets_fifo_or_says_why() {
        let g = Group::of_output(None, Duration::from_micros(2667));
        assert_eq!(g.missing(), None, "Linux has no workgroup to miss");
        let (worker, device, policy) = std::thread::spawn(move || {
            let w = engine_thread(&g);
            let d = on_device_thread(&g);
            // SAFETY: plain system call on this thread.
            let policy = unsafe { libc::sched_getscheduler(0) };
            end_engine_thread();
            (w, d, policy)
        })
        .join()
        .unwrap();
        assert!(!worker.workgroup && !worker.cancelled && !device.workgroup);
        assert_eq!(worker.real_time, device.real_time, "one rule for both");
        if worker.real_time {
            assert_eq!(policy, libc::SCHED_FIFO, "granted: FIFO");
        } else {
            assert_ne!(policy, libc::SCHED_FIFO);
            let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            // SAFETY: getrlimit fills `limit`.
            unsafe { libc::getrlimit(libc::RLIMIT_RTPRIO, &mut limit) };
            // SAFETY: plain system call.
            let root = unsafe { libc::geteuid() } == 0;
            assert!(limit.rlim_max == 0 && !root, "refused, though the limit is {} (root: {root})", limit.rlim_max);
        }
    }
}
