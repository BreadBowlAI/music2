//! macOS: thread identity and the CLAP folders. The main loop is in `main_loop.rs`.

pub(crate) mod main_loop;

use std::path::{Path, PathBuf};

pub(crate) use crate::unix::{cpu_time, token};

#[inline]
pub(crate) fn is_process_main() -> bool {
    // SAFETY: always safe to call.
    unsafe { libc::pthread_main_np() != 0 }
}

pub(crate) fn clap(home: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = home.map(|h| h.join("Library/Audio/Plug-Ins/CLAP")).into_iter().collect();
    v.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    v
}
