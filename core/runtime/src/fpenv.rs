//! One floating-point mode on every thread that renders (RENDER-2a): round to nearest, subnormals
//! kept, no default NaN, every exception masked. That is MXCSR `0x1F80` on x86-64 and FPCR `0` on
//! arm64, each CPU's own default.
//!
//! WebAssembly requires subnormals, and a song's bits must not depend on which thread rendered a
//! partition. A plugin may leave flush-to-zero set behind it, and a thread music2 didn't start
//! (an audio device's, a host's) keeps whatever mode its owner gave it. So the engine sets the
//! mode at the start of each block it renders, before each partition and after each plugin call,
//! and puts the calling thread's own mode back at the block's end ([`Canon`]).
//!
//! The block path: nothing here allocates, locks or waits.

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("RENDER-2a: music2 sets the floating-point mode of x86-64 and arm64 CPUs only (fpenv.rs)");

#[cfg(target_arch = "x86_64")]
mod cpu {
    use std::arch::asm;

    /// MXCSR: every exception masked, round to nearest, flush-to-zero and denormals-are-zero off.
    pub(super) const CANON: u32 = 0x1F80;
    /// The control bits: the low six are exception flags, which arithmetic sets as it goes.
    const CONTROL: u32 = 0xFFC0;
    /// Flush-to-zero (bit 15) and denormals-are-zero (bit 6).
    #[cfg(test)]
    pub(super) const FLUSH: u32 = 0x8040;

    pub(super) type Mode = u32;

    #[inline]
    pub(super) fn get() -> Mode {
        let mut csr: u32 = 0;
        // SAFETY: stores MXCSR into a local.
        unsafe { asm!("stmxcsr [{}]", in(reg) &mut csr, options(nostack, preserves_flags)) };
        csr
    }

    #[inline]
    pub(super) fn set(csr: Mode) {
        // SAFETY: loads MXCSR from a local; any value with no reserved bit set is valid, and every
        // value here came from `get` or is one of the constants above.
        unsafe { asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, readonly, preserves_flags)) };
    }

    #[inline]
    pub(super) fn is_canon(csr: Mode) -> bool {
        csr & CONTROL == CANON
    }
}

#[cfg(target_arch = "aarch64")]
mod cpu {
    use std::arch::asm;

    /// FPCR: round to nearest, no flush-to-zero, no default NaN, no exception traps.
    pub(super) const CANON: u64 = 0;
    /// Flush-to-zero (bit 24).
    #[cfg(test)]
    pub(super) const FLUSH: u64 = 1 << 24;

    pub(super) type Mode = u64;

    #[inline]
    pub(super) fn get() -> Mode {
        let fpcr: u64;
        // SAFETY: reads FPCR into a register.
        unsafe { asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack, preserves_flags)) };
        fpcr
    }

    #[inline]
    pub(super) fn set(fpcr: Mode) {
        // SAFETY: writes FPCR; every value here came from `get` or is one of the constants above.
        unsafe { asm!("msr fpcr, {}", in(reg) fpcr, options(nomem, nostack, preserves_flags)) };
    }

    #[inline]
    pub(super) fn is_canon(fpcr: Mode) -> bool {
        fpcr == CANON
    }
}

/// Put the calling thread in the one mode, if it isn't: before each partition, after each plugin
/// call, and on each worker as it starts.
#[inline]
pub(crate) fn canon() {
    if !cpu::is_canon(cpu::get()) {
        cpu::set(cpu::CANON);
    }
}

/// Whether the calling thread computes in the one mode.
#[inline]
pub(crate) fn is_canon() -> bool {
    cpu::is_canon(cpu::get())
}

/// The one mode while it lives, and the calling thread's own mode back when it drops, unwinding
/// too: a block rendered on a thread music2 didn't start leaves that thread as it found it.
pub(crate) struct Canon {
    was: cpu::Mode,
}

impl Canon {
    #[inline]
    pub(crate) fn enter() -> Canon {
        let was = cpu::get();
        if !cpu::is_canon(was) {
            cpu::set(cpu::CANON);
        }
        Canon { was }
    }
}

impl Drop for Canon {
    #[inline]
    fn drop(&mut self) {
        if !cpu::is_canon(self.was) {
            cpu::set(self.was);
        }
    }
}

/// For tests: the calling thread flushes subnormals to zero, as a plugin or a host might leave it.
#[cfg(test)]
pub(crate) fn flush_to_zero() {
    cpu::set(cpu::get() | cpu::FLUSH);
}

/// For tests: whether the calling thread flushes subnormals to zero.
#[cfg(test)]
pub(crate) fn flushes() -> bool {
    cpu::get() & cpu::FLUSH == cpu::FLUSH
}

#[cfg(test)]
mod tests {
    use super::*;

    // RENDER-2a: a thread starts in the one mode; flush-to-zero leaves it; `Canon` gives the one
    // mode while it lives and the thread's own back after.
    #[test]
    fn canon_enters_the_one_mode_and_leaves_the_threads_own() {
        std::thread::spawn(|| {
            assert!(is_canon(), "a new thread computes in the CPU's default mode");
            flush_to_zero();
            assert!(!is_canon() && flushes());
            {
                let _c = Canon::enter();
                assert!(is_canon() && !flushes());
                let tiny = std::hint::black_box(1e-30f32) * std::hint::black_box(1e-10f32);
                assert!(tiny != 0.0 && tiny.is_subnormal(), "subnormals are kept: {tiny:e}");
            }
            assert!(flushes(), "the thread's own mode is back");
            canon();
            assert!(is_canon());
        })
        .join()
        .unwrap();
    }
}
