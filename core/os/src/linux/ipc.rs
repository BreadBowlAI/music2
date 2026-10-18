//! Linux: shared memory is `shm_open` (a file in `/dev/shm`) and `mmap`; a wait is a futex
//! without the private flag, which the kernel keys by the page, not by this process. The futex
//! calls neither allocate, lock nor do I/O.

use std::io;
use std::sync::atomic::AtomicU32;
use std::time::Duration;

pub(crate) use crate::unix::{shm_create as create, shm_open_mapped as open, shm_remove as remove, unmap};

/// `shm_open` as this OS declares it.
pub(crate) fn shm_open(name: &std::ffi::CStr, flags: libc::c_int) -> io::Result<libc::c_int> {
    // SAFETY: a NUL-terminated name that outlives the call.
    let fd = unsafe { libc::shm_open(name.as_ptr(), flags, 0o600) };
    if fd < 0 { Err(io::Error::last_os_error()) } else { Ok(fd) }
}

pub(crate) fn wait(word: &AtomicU32, expected: u32, at_most: Duration) {
    let ts = libc::timespec { tv_sec: at_most.as_secs().min(i64::MAX as u64) as libc::time_t, tv_nsec: at_most.subsec_nanos() as _ };
    // SAFETY: a futex wait on a word that lives while the caller holds it, with a relative timeout
    // that outlives the call. A wake, a changed word, a signal or the time running out ends it.
    unsafe { libc::syscall(libc::SYS_futex, word.as_ptr(), libc::FUTEX_WAIT, expected, &ts as *const libc::timespec) };
}

pub(crate) fn wake(word: &AtomicU32) {
    // SAFETY: a futex wake on a word that lives while the caller holds it.
    unsafe { libc::syscall(libc::SYS_futex, word.as_ptr(), libc::FUTEX_WAKE, i32::MAX) };
}
