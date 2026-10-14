//! Linux: thread identity and the CLAP folders. The main loop is in `main_loop.rs`.

pub(crate) mod main_loop;

use std::path::{Path, PathBuf};

pub(crate) use crate::unix::{cpu_time, token};

/// The main thread's id is the process id: two system calls, no allocation, no lock.
#[inline]
pub(crate) fn is_process_main() -> bool {
    // SAFETY: plain system calls.
    unsafe { libc::gettid() == libc::getpid() }
}

pub(crate) fn clap(home: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = home.map(|h| h.join(".clap")).into_iter().collect();
    v.push(PathBuf::from("/usr/lib/clap"));
    v
}
