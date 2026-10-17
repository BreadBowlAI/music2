//! HOST-2 in the app: hosted plugins make their main-thread calls on the app's main loop. On macOS
//! that is AppKit's main thread, through the main dispatch queue, which AppKit's run loop drains
//! (Tauri runs `[NSApp run]` there). On Linux, where GTK owns the main thread and plugins never post
//! to it, it is a loop thread of its own (`m2_os::main_loop`).
//!
//! The rule that keeps this free of deadlocks: the main thread never waits on a thread that may be
//! creating or destroying plugins (the server's session, which waits on the main thread to do
//! that). So the project opens off the main thread, and quit closes it from a thread of its own
//! while the main thread goes on serving.
//!
//! Kept in a file of its own so `tests/app_main_thread.rs` runs this same code on the process's
//! main thread (under a real `[NSApp run]` on macOS), without launching the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use m2_server::AppState;
use m2_server::graph::Teardown;

/// Start the plugins' main thread: this one, AppKit's, on macOS; a loop thread of its own on Linux.
/// Call it first thing in `main`, on the main thread, before anything touches a plugin: calls
/// queued before AppKit's run loop starts run once it does.
pub fn install() -> anyhow::Result<()> {
    m2_plugins::use_app_main_thread()
}

/// Open a song project folder, put it first in the recent songs list, and tell the UI to show it.
/// Never on the main thread: opening creates the song's plugins there.
pub fn open_project(server: &AppState, dir: &Path) -> anyhow::Result<()> {
    server.open_song(dir)?;
    server.graph_changed();
    Ok(())
}

/// [`open_project`] on a thread of its own, for the main thread (the launch project); `failed`
/// hears why it didn't open.
pub fn open_off_main(
    server: Arc<AppState>,
    dir: PathBuf,
    failed: impl FnOnce(&AppState, &Path, anyhow::Error) + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Err(e) = open_project(&server, &dir) {
            failed(&server, &dir, e);
        }
    })
}

/// Quit's teardown, off the main thread: close the project, waiting while the main thread destroys
/// its plugin instances (2 s at most), and then any other instance of the process (an export
/// stopped just before drops its own), for 1 s at most. Returns how many the project's close
/// destroyed, and how many instances of the process are still alive: with some (a hung device
/// thread holds its renderer), the app must leave with [`leave_now`], since exit handlers would
/// unload plugin code under them.
pub fn close_for_quit(server: &AppState) -> Teardown {
    let closed = server.graph.close();
    let host = m2_plugins::host();
    let deadline = Instant::now() + Duration::from_secs(1);
    while host.live() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    Teardown { destroyed: closed.destroyed, left: host.live() }
}

/// Leave at once, without running exit handlers.
pub fn leave_now() -> ! {
    unsafe extern "C" {
        fn _exit(code: std::ffi::c_int) -> !;
    }
    // SAFETY: ends the process at once; nothing of the app's needs exit handlers.
    unsafe { _exit(0) }
}
