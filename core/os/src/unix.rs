//! What is the same on macOS and Linux. Both OS folders re-export it.

use std::time::Duration;

#[inline]
pub(crate) fn token() -> usize {
    // SAFETY: always safe to call; a register read.
    unsafe { libc::pthread_self() as usize }
}

pub(crate) fn cpu_time() -> Duration {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime fills `t`, which outlives the call.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}
