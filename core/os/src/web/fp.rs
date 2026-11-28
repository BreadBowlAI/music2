//! The web: WebAssembly has one floating-point mode, the one music2 renders in (RENDER-2a), and no
//! code can read or set it. But a thread of the browser's may flush subnormals to zero anyway:
//! Chrome's AudioWorklet thread does (the web engine note, section 3). So the mode is measured, by
//! one division whose answer is a subnormal, in `f32` and in `f64`: a thread that keeps both is in
//! the one mode, and a thread that loses either flushes. Setting it changes nothing.
//!
//! The block path: nothing here allocates, locks or waits.

use std::hint::black_box;

/// Subnormals kept.
pub(crate) const CANON: u32 = 0;
/// Subnormals flushed to zero, by the thread's host.
pub(crate) const FLUSH: u32 = 1;

pub(crate) type Mode = u32;

/// What the calling thread does with a subnormal now.
#[inline]
pub(crate) fn get() -> Mode {
    let single = black_box(f32::MIN_POSITIVE) / black_box(2.0f32);
    let double = black_box(f64::MIN_POSITIVE) / black_box(2.0f64);
    if single != 0.0 && double != 0.0 { CANON } else { FLUSH }
}

/// No code can set the mode here: the engine renders only on a thread that keeps subnormals.
#[inline]
pub(crate) fn set(_mode: Mode) {}

#[inline]
pub(crate) fn is_canon(mode: Mode) -> bool {
    mode == CANON
}

#[cfg(test)]
mod tests {
    use super::*;

    // RENDER-2a: the web's mode is measured, not assumed. On a thread that keeps subnormals it is
    // the one mode; on one whose host flushes them (here this CPU's own flush-to-zero, as Chrome's
    // audio thread has) it says so; and setting it changes nothing.
    #[test]
    fn the_web_measures_the_mode_it_cant_set() {
        std::thread::spawn(|| {
            assert_eq!(get(), CANON, "a new thread keeps subnormals");
            assert!(is_canon(get()));
            crate::fp::flush_for_tests(true);
            assert_eq!(get(), FLUSH, "a thread that flushes is seen to");
            set(CANON);
            assert_eq!(get(), FLUSH, "setting it changes nothing");
            crate::fp::flush_for_tests(false);
            assert_eq!(get(), CANON);
        })
        .join()
        .unwrap();
    }
}
