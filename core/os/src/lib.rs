//! What music2 does differently on each operating system, in one place (PLAT-2).
//!
//! The portable modules here hold the public API, its documentation and the logic every OS
//! shares; they name no OS. One folder per OS (`mac/`, `linux/`) implements the same private
//! functions, and code the same on macOS and Linux is written once in `unix.rs`. "What is
//! different on Linux?" is answered by reading `linux/`. The rest of music2 names no OS.
//!
//! It does what the OS does, or says what it can't (PLAT-3): fallbacks and their rules stay with
//! the caller.

pub mod dirs;
pub mod fs;
pub mod host;
pub mod main_loop;
pub mod process;
pub mod thread;
pub mod watch;

#[cfg_attr(target_os = "macos", path = "mac/mod.rs")]
#[cfg_attr(target_os = "linux", path = "linux/mod.rs")]
mod os;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("music2 builds on macOS and Linux; Windows comes in its own step (PLAT-1)");

#[cfg(unix)]
mod unix;

/// `ErrorKind::Unsupported`, naming what this OS or this volume can't do (PLAT-3).
pub fn unsupported(what: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Unsupported, what)
}
