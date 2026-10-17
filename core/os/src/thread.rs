//! Who the calling thread is, the CPU time it used, and how it is scheduled.

use std::io;
use std::time::Duration;

use crate::os;

/// The calling thread's identity: `pthread_self`, a register read. Never a thread-local (a
/// thread's first one allocates on macOS) and never a lock: the audio thread asks (RENDER-28).
#[inline]
pub fn token() -> usize {
    os::token()
}

/// Whether the calling thread is the process's main thread (macOS: `pthread_main_np`; Linux: its
/// thread id is the process id). No allocation, no lock.
#[inline]
pub fn is_process_main() -> bool {
    os::is_process_main()
}

/// The CPU time the calling thread has used (G11's watcher check).
pub fn cpu_time() -> Duration {
    os::cpu_time()
}

/// Marks the calling thread latency-sensitive, as an audio thread is: on macOS the user-interactive
/// QoS class, which also keeps it on a performance core. Linux has no such class and changes
/// nothing: it says so (`Unsupported`). Never fatal: the caller reports what it got (step 7's G7f
/// drop test).
pub fn interactive() -> io::Result<()> {
    os::interactive()
}

/// Marks the calling thread as background work beside playback (8a: an export's workers while the
/// app plays): on macOS the background QoS class, which keeps it on the efficiency cores; on Linux
/// a nice of 10, or the nice it has when that is higher (raising nice needs no rights; lowering it
/// does). Never fatal: the caller says what it got.
pub fn background() -> io::Result<()> {
    os::background()
}

/// From now on the calling thread reads and writes numbers as C does, with a `.`, whatever locale
/// the process has. A GTK app takes the user's (`setlocale(LC_ALL, "")`), whose decimal comma
/// makes the C library's `strtod` read "0.5" as 0, and QuickJS's `JSON.parse` uses it. For a
/// thread of its own: nothing puts the locale it had back.
pub fn c_numbers() {
    os::c_numbers()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_keeps_its_token_and_another_thread_has_another() {
        let here = token();
        assert_eq!(token(), here);
        let there = std::thread::spawn(|| (token(), is_process_main())).join().unwrap();
        assert_ne!(there.0, here);
        assert!(!there.1, "a spawned thread is never the process's main thread");
    }

    #[test]
    fn cpu_time_grows_while_the_thread_works() {
        // Work until the clock says 10 ms, however long that takes on a busy machine: beside 32
        // busy threads, 30 ms of wall time held less than 10 ms of CPU in 1 run of 20. A thread's
        // CPU time never runs ahead of the wall clock.
        let before = cpu_time();
        let t = std::time::Instant::now();
        let mut x = 0u64;
        let mut used = Duration::ZERO;
        while used < Duration::from_millis(10) && t.elapsed() < Duration::from_secs(5) {
            for _ in 0..10_000 {
                x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
            }
            used = cpu_time() - before;
        }
        let wall = t.elapsed();
        assert!(used >= Duration::from_millis(10) && used <= wall + Duration::from_millis(1), "{used:?} of CPU in {wall:?}");
    }
}
