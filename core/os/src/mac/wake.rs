//! macOS: a wake is a Mach semaphore. `semaphore_signal` never blocks; `semaphore_timedwait`
//! takes one signal, or returns when the time runs out. Nothing here allocates, locks or does I/O
//! (tests/no_blocking.rs reads this file whole).

use std::ffi::c_int;
use std::time::{Duration, Instant};

use libc::{KERN_OPERATION_TIMED_OUT, KERN_SUCCESS, kern_return_t, mach_port_t};

/// `mach_timespec_t`.
#[repr(C)]
struct MachTimespec {
    tv_sec: u32,
    tv_nsec: c_int,
}

/// `SYNC_POLICY_FIFO`.
const FIFO: c_int = 0;

unsafe extern "C" {
    fn semaphore_create(task: mach_port_t, semaphore: *mut mach_port_t, policy: c_int, value: c_int) -> kern_return_t;
    fn semaphore_signal(semaphore: mach_port_t) -> kern_return_t;
    fn semaphore_timedwait(semaphore: mach_port_t, wait_time: MachTimespec) -> kern_return_t;
    fn semaphore_destroy(task: mach_port_t, semaphore: mach_port_t) -> kern_return_t;
}

pub(crate) struct Wake {
    semaphore: mach_port_t,
}

// SAFETY: a Mach semaphore may be signalled and waited on from any thread.
unsafe impl Send for Wake {}
unsafe impl Sync for Wake {}

/// This task's port.
fn task() -> mach_port_t {
    // SAFETY: reads the process's task port, set at its start.
    #[allow(deprecated)]
    unsafe {
        libc::mach_task_self()
    }
}

impl Wake {
    pub(crate) fn new() -> Result<Wake, i32> {
        let mut semaphore = 0;
        // SAFETY: `semaphore` outlives the call, which fills it.
        match unsafe { semaphore_create(task(), &mut semaphore, FIFO, 0) } {
            KERN_SUCCESS => Ok(Wake { semaphore }),
            code => Err(code),
        }
    }

    pub(crate) fn ring(&self) {
        // SAFETY: a semaphore this wake owns until it drops.
        unsafe { semaphore_signal(self.semaphore) };
    }

    pub(crate) fn wait(&self, at_most: Duration) -> bool {
        let start = Instant::now();
        loop {
            // Once the time is up this still takes a signal already given (a wait of 0).
            let left = at_most.saturating_sub(start.elapsed());
            let ts = MachTimespec { tv_sec: left.as_secs().min(u64::from(u32::MAX)) as u32, tv_nsec: left.subsec_nanos() as c_int };
            // SAFETY: a semaphore this wake owns until it drops.
            match unsafe { semaphore_timedwait(self.semaphore, ts) } {
                KERN_SUCCESS => return true,
                KERN_OPERATION_TIMED_OUT if left.is_zero() || start.elapsed() >= at_most => return false,
                // Interrupted (`KERN_ABORTED`), or woken early: wait again for what is left.
                _ if left.is_zero() => return false,
                _ => {}
            }
        }
    }
}

impl Drop for Wake {
    fn drop(&mut self) {
        // SAFETY: the semaphore this wake made; nothing uses it after the drop.
        unsafe { semaphore_destroy(task(), self.semaphore) };
    }
}
