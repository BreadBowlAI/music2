//! What music2 does differently on each operating system, in one place (PLAT-2).
//!
//! The portable modules here hold the public API, its documentation and the logic every OS
//! shares; they name no OS. One folder per platform (`mac/`, `linux/`, `web/`) implements the
//! same private functions, and code the same on macOS and Linux is written once in `unix.rs`.
//! "What is different on Linux?" is answered by reading `linux/`. The rest of music2 names no OS.
//!
//! It does what the OS does, or says what it can't (PLAT-3): fallbacks and their rules stay with
//! the caller.
//!
//! The web (`wasm32-unknown-unknown`, the web engine's platform: PLAT-1) builds only the modules
//! the engine calls. The plugin host's IPC, the main loop, windows, file-system events and MIDI
//! are the desktop's alone, so a crate that needs them doesn't build for the web.

pub mod dirs;
pub mod fp;
pub mod fs;
pub mod host;
#[cfg(not(target_arch = "wasm32"))]
pub mod ipc;
#[cfg(not(target_arch = "wasm32"))]
pub mod main_loop;
#[cfg(all(feature = "midi", not(target_arch = "wasm32")))]
pub mod midi;
pub mod process;
pub mod rt;
pub mod thread;
#[cfg(not(target_arch = "wasm32"))]
pub mod watch;
#[cfg(not(target_arch = "wasm32"))]
pub mod window;

#[cfg_attr(target_os = "macos", path = "mac/mod.rs")]
#[cfg_attr(target_os = "linux", path = "linux/mod.rs")]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), path = "web/mod.rs")]
mod os;
#[cfg(not(any(target_os = "macos", target_os = "linux", all(target_arch = "wasm32", target_os = "unknown"))))]
compile_error!("music2 builds on macOS, on Linux, and for the web as wasm32-unknown-unknown; Windows comes in its own step (PLAT-1)");

#[cfg(unix)]
mod unix;

/// The web's folder, built here for this machine's tests, so its logic is tested where tests run
/// (a test can't run on wasm32-unknown-unknown). Nothing but its tests calls it.
#[cfg(test)]
#[allow(dead_code)]
#[path = "web/mod.rs"]
mod web_on_this_machine;

/// `ErrorKind::Unsupported`, naming what this OS or this volume can't do (PLAT-3).
pub fn unsupported(what: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Unsupported, what)
}
