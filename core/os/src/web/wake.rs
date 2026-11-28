//! The web: one thread (the web engine note's decision 5), so nothing can ring a wake while its
//! thread waits on it. A ring sets one flag; a wait takes it, or says at once that the time ran out:
//! sleeping out `at_most` would only spend it, since no ring could end the sleep early (and a
//! sleep traps on `wasm32-unknown-unknown`). Nothing here allocates, locks or does I/O
//! (tests/no_blocking.rs reads this file whole).

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::time::Duration;

pub(crate) struct Wake {
    rung: AtomicBool,
}

impl Wake {
    pub(crate) fn new() -> Result<Wake, i32> {
        Ok(Wake { rung: AtomicBool::new(false) })
    }

    pub(crate) fn ring(&self) {
        self.rung.store(true, SeqCst);
    }

    pub(crate) fn wait(&self, _at_most: Duration) -> bool {
        self.rung.swap(false, SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::Wake;

    // RENDER-28, PLAT-3: on the web's one thread a ring before the wait is taken once; a wait no
    // ring came before says at once that the time ran out, whatever it was given.
    #[test]
    fn a_wake_on_one_thread_takes_one_ring_or_returns_at_once() {
        let w = Wake::new().unwrap();
        let t = Instant::now();
        assert!(!w.wait(Duration::from_secs(3600)), "not rung");
        assert!(t.elapsed() < Duration::from_secs(60), "it didn't sleep out the hour: {:?}", t.elapsed());
        w.ring();
        w.ring();
        assert!(w.wait(Duration::from_secs(3600)), "rung");
        assert!(!w.wait(Duration::ZERO), "a ring is taken once, and the wake holds one");
    }
}
