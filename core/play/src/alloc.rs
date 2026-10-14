//! The audio thread's allocation counter (RENDER-28). Installed as the global allocator, as debug
//! builds of the app do and `tests/no_alloc.rs` does, it counts every allocation, free and
//! reallocation made while a device callback runs: the callback marks its thread around its work.
//! It never panics or prints in the callback; [`watch`] logs from a thread of its own.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// The counting allocator: the system's, counted while a callback runs.
pub struct Counting;

thread_local! {
    static IN_CALLBACK: Cell<bool> = const { Cell::new(false) };
    static HERE: Cell<u64> = const { Cell::new(0) };
}

/// Every thread's count, for [`watch`].
static ALL: AtomicU64 = AtomicU64::new(0);
static TRACE: AtomicBool = AtomicBool::new(false);

/// Mark the calling thread as running a device callback, or not. Touches only a thread-local
/// whose first use allocates nothing.
pub fn set_in_callback(on: bool) {
    let _ = IN_CALLBACK.try_with(|c| c.set(on));
}

/// What this thread's callbacks allocated, freed or reallocated so far.
pub fn here() -> u64 {
    HERE.try_with(Cell::get).unwrap_or(0)
}

/// Print each one counted from now on, with a backtrace (tests, `M2_TRAP_TRACE`).
pub fn trace(on: bool) {
    TRACE.store(on, Ordering::Relaxed);
}

fn count(what: &str, size: usize) {
    let _ = IN_CALLBACK.try_with(|on| {
        if on.get() {
            let _ = HERE.try_with(|n| n.set(n.get() + 1));
            ALL.fetch_add(1, Ordering::Relaxed);
            if TRACE.load(Ordering::Relaxed) {
                on.set(false);
                eprintln!("{what} of {size} bytes on the audio thread:\n{}", std::backtrace::Backtrace::force_capture());
                on.set(true);
            }
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count("alloc", l.size());
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        count("dealloc", l.size());
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count("realloc", n);
        unsafe { System.realloc(p, l, n) }
    }
}

/// Log, once a second while there are any, how many times the callbacks allocated or freed since
/// the last log (debug builds of the app).
pub fn watch() {
    let _ = std::thread::Builder::new().name("m2-alloc-watch".into()).spawn(|| {
        let mut seen = 0;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let n = ALL.load(Ordering::Relaxed);
            if n > seen {
                eprintln!("music2: the audio thread allocated or freed {} times in the last second (RENDER-28)", n - seen);
                seen = n;
            }
        }
    });
}
