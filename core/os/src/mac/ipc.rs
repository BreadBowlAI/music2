//! macOS: shared memory is `shm_open` and `mmap` (names of at most 31 bytes); a wait is
//! `__ulock_wait` of the shared kind, which the kernel keys by the page, so a wake from another
//! process reaches it. libc++ and Rust's std wait on it too; it is there from macOS 11, the app's
//! oldest. The wait and wake neither allocate, lock nor do I/O.

use std::ffi::{c_int, c_void};
use std::io;
use std::sync::atomic::AtomicU32;
use std::time::Duration;

pub(crate) use crate::unix::{shm_create as create, shm_open_mapped as open, shm_remove as remove, unmap};

/// `UL_COMPARE_AND_WAIT_SHARED`: a 32-bit word, in memory other processes may map.
const SHARED: u32 = 3;
/// `ULF_WAKE_ALL`.
const WAKE_ALL: u32 = 0x100;
/// `ULF_NO_ERRNO`: the result is the error, errno is left alone.
const NO_ERRNO: u32 = 0x0100_0000;

unsafe extern "C" {
    fn __ulock_wait(operation: u32, addr: *mut c_void, value: u64, timeout_us: u32) -> c_int;
    fn __ulock_wake(operation: u32, addr: *mut c_void, wake_value: u64) -> c_int;
}

/// `shm_open` as this OS declares it (its mode goes through C's `...`).
pub(crate) fn shm_open(name: &std::ffi::CStr, flags: libc::c_int) -> io::Result<libc::c_int> {
    // SAFETY: a NUL-terminated name that outlives the call; the mode is promoted as C promotes it.
    let fd = unsafe { libc::shm_open(name.as_ptr(), flags, 0o600 as libc::c_uint) };
    if fd < 0 { Err(io::Error::last_os_error()) } else { Ok(fd) }
}

pub(crate) fn wait(word: &AtomicU32, expected: u32, at_most: Duration) {
    // 0 means forever here: a wait of less than a microsecond is one.
    let us = at_most.as_micros().clamp(1, u128::from(u32::MAX)) as u32;
    // SAFETY: a wait on a word that lives while the caller holds it. A wake, a changed word, a
    // signal or the time running out ends it.
    unsafe { __ulock_wait(SHARED | NO_ERRNO, word.as_ptr() as *mut c_void, u64::from(expected), us) };
}

pub(crate) fn wake(word: &AtomicU32) {
    // SAFETY: a wake on a word that lives while the caller holds it; none waiting is no error.
    unsafe { __ulock_wake(SHARED | WAKE_ALL | NO_ERRNO, word.as_ptr() as *mut c_void, 0) };
}
