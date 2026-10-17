//! Linux: a wake is one word and a futex on it. 1 is a ring waiting; the waiter takes it with a
//! swap to 0, and sleeps while the word is 0. Nothing here allocates, locks or does I/O
//! (tests/no_blocking.rs reads this file whole).

use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::time::{Duration, Instant};

pub(crate) struct Wake {
    word: AtomicU32,
}

impl Wake {
    pub(crate) fn new() -> Result<Wake, i32> {
        Ok(Wake { word: AtomicU32::new(0) })
    }

    pub(crate) fn ring(&self) {
        // Only the ring that sets the word wakes: a waiter sleeps only while it is 0.
        if self.word.swap(1, SeqCst) == 0 {
            // SAFETY: a futex call on a word this wake owns; it wakes at most one sleeper.
            unsafe { libc::syscall(libc::SYS_futex, self.word.as_ptr(), libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG, 1) };
        }
    }

    pub(crate) fn wait(&self, at_most: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.word.swap(0, SeqCst) == 1 {
                return true;
            }
            let Some(left) = at_most.checked_sub(start.elapsed()).filter(|l| !l.is_zero()) else { return false };
            let ts = libc::timespec { tv_sec: left.as_secs().min(i64::MAX as u64) as libc::time_t, tv_nsec: left.subsec_nanos() as _ };
            // SAFETY: a futex wait on a word this wake owns, while it holds 0, with a relative
            // timeout that outlives the call. A ring, a signal, a spurious wake-up or the time
            // running out each end it; the loop then looks again.
            unsafe {
                libc::syscall(
                    libc::SYS_futex,
                    self.word.as_ptr(),
                    libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG,
                    0,
                    &ts as *const libc::timespec,
                )
            };
        }
    }
}
