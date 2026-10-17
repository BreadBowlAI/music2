//! The pool's atomics and its one way to sleep, in one place.
//!
//! In a normal build these are std's own. With the `models` feature, each atomic is a thin
//! wrapper that first passes a chaos point: a random `yield_now`, a short spin of up to 64
//! iterations, or nothing. Many runs then try many interleavings (tests/models.rs). loom would
//! try them all, but it isn't in the offline registry here.
//!
//! The chaos point uses a per-thread random number, never a shared atomic: a locked instruction
//! there would be a full fence on x86 and hide the very reorderings a model looks for.

pub(crate) use std::sync::atomic::Ordering;

#[cfg(not(feature = "models"))]
pub(crate) use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU32, AtomicU64};

/// Sleep until `unpark` (or spuriously). Only a worker calls it: std started it.
pub(crate) fn park() {
    #[cfg(feature = "models")]
    chaos::point();
    std::thread::park();
}

/// Wake a worker parked in `park`, or make its next `park` return at once.
pub(crate) fn unpark(thread: &std::thread::Thread) {
    #[cfg(feature = "models")]
    chaos::point();
    thread.unpark();
}

#[cfg(feature = "models")]
pub(crate) use chaos::{AtomicBool, AtomicPtr, AtomicU8, AtomicU32, AtomicU64};

#[cfg(feature = "models")]
pub(crate) mod chaos {
    use std::cell::Cell;
    use std::sync::atomic::{self, Ordering};

    static SEED: atomic::AtomicU64 = atomic::AtomicU64::new(0x5EED);
    static THREADS: atomic::AtomicU64 = atomic::AtomicU64::new(0);

    thread_local! {
        static RNG: Cell<u64> = Cell::new(SEED.load(Ordering::Relaxed) ^ mix(THREADS.fetch_add(1, Ordering::Relaxed)));
    }

    /// The seed for threads that draw their first number after this.
    pub fn seed(seed: u64) {
        SEED.store(seed, Ordering::Relaxed);
    }

    fn mix(mut z: u64) -> u64 {
        z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A yield one time in eight, a spin of up to 64 iterations one time in four, else nothing.
    pub fn point() {
        let r = RNG.with(|c| {
            let r = mix(c.get());
            c.set(r);
            r
        });
        match r % 8 {
            0 => std::thread::yield_now(),
            1 | 2 => {
                for _ in 0..(r >> 8) % 64 {
                    std::hint::spin_loop();
                }
            }
            _ => {}
        }
    }

    macro_rules! wrap {
        ($name:ident, $std:ty, $t:ty) => {
            #[derive(Default)]
            pub(crate) struct $name($std);
            #[allow(dead_code)]
            impl $name {
                pub(crate) const fn new(v: $t) -> Self {
                    Self(<$std>::new(v))
                }
                pub(crate) fn load(&self, o: Ordering) -> $t {
                    point();
                    self.0.load(o)
                }
                pub(crate) fn store(&self, v: $t, o: Ordering) {
                    point();
                    self.0.store(v, o)
                }
                pub(crate) fn swap(&self, v: $t, o: Ordering) -> $t {
                    point();
                    self.0.swap(v, o)
                }
                pub(crate) fn compare_exchange_weak(&self, a: $t, b: $t, s: Ordering, f: Ordering) -> Result<$t, $t> {
                    point();
                    self.0.compare_exchange_weak(a, b, s, f)
                }
                pub(crate) fn get_mut(&mut self) -> &mut $t {
                    self.0.get_mut()
                }
            }
        };
    }
    macro_rules! count {
        ($name:ident, $t:ty) => {
            #[allow(dead_code)]
            impl $name {
                pub(crate) fn fetch_add(&self, v: $t, o: Ordering) -> $t {
                    point();
                    self.0.fetch_add(v, o)
                }
                pub(crate) fn fetch_sub(&self, v: $t, o: Ordering) -> $t {
                    point();
                    self.0.fetch_sub(v, o)
                }
            }
        };
    }
    wrap!(AtomicBool, atomic::AtomicBool, bool);
    wrap!(AtomicU8, atomic::AtomicU8, u8);
    wrap!(AtomicU32, atomic::AtomicU32, u32);
    wrap!(AtomicU64, atomic::AtomicU64, u64);
    count!(AtomicU32, u32);
    count!(AtomicU64, u64);

    pub(crate) struct AtomicPtr<T>(atomic::AtomicPtr<T>);
    impl<T> AtomicPtr<T> {
        pub(crate) const fn new(p: *mut T) -> Self {
            Self(atomic::AtomicPtr::new(p))
        }
        pub(crate) fn load(&self, o: Ordering) -> *mut T {
            point();
            self.0.load(o)
        }
        pub(crate) fn store(&self, p: *mut T, o: Ordering) {
            point();
            self.0.store(p, o)
        }
    }
}
