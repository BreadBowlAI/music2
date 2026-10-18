//! A plain window a plugin's own view goes into (HOST-7: only the plugin host process opens them).
//!
//! In short: [`Window::open`] makes a top-level window on the main loop's thread and gives its
//! [`Native`] handle, which CLAP's `gui.set_parent` takes. When the person closes it (the title
//! bar's close button), `closed` runs on that thread, later, never inside an OS call; the caller
//! then drops it. A program that never opens one never loads the OS's window library.
//!
//! A window is this process's alone. Everything asked of it, tests' questions included
//! ([`Window::shown`], [`Window::ask_to_close`]), is asked of the [`Window`] by the process that
//! opened it; another process asks that one (the plugin host's `WindowCall`s). A [`Native`]
//! handle is never used in another process: on macOS it is a pointer into this one.
//!
//! | OS | Window | The person's close | Events |
//! |---|---|---|---|
//! | Linux | X11, through `libX11.so.6` opened at run time (`dlopen`), so a run without a screen never needs it. Wayland only through XWayland | `WM_DELETE_WINDOW` | the X connection's fd, watched on the main loop |
//! | macOS | `NSWindow` through the Objective-C runtime; the host process is an accessory app (no Dock icon) | the window delegate's `windowShouldClose:` | AppKit's, pumped by the main loop |
//! | Windows (designed, not built) | an `HWND` of a class of our own (`CreateWindowExW`) | `WM_CLOSE` | the loop's message loop |

use std::io;

use crate::main_loop::Loop;
use crate::os;

/// What CLAP calls the window API this OS's windows are (`gui.is_api_supported`).
pub const API: &str = os::window::API;

/// A window's handle as a plugin takes it: an X11 window id, an `NSView` pointer (the window's
/// content view), or an `HWND`. Meaningful only in the process that opened the window; another
/// process may compare it, never use it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Native(pub u64);

/// What the person's close calls, on the main loop's thread.
pub type OnClose = Box<dyn FnMut() + Send>;

/// A top-level window. Closed and destroyed when dropped, on the thread that opened it.
pub struct Window(os::window::Window);

impl Window {
    /// Open a window titled `title`, `width` by `height`, on `lp`'s thread (call it there).
    /// `closed` runs there when the person closes it; the window stays until dropped.
    pub fn open(lp: &Loop, title: &str, width: u32, height: u32, closed: OnClose) -> io::Result<Window> {
        debug_assert!(lp.is_current(), "a window opens on its main loop's thread");
        os::window::Window::open(lp, title, width.max(1), height.max(1), closed).map(Window)
    }

    pub fn native(&self) -> Native {
        self.0.native()
    }

    /// Make it this size (a plugin asked, CLAP's `request_resize`).
    pub fn resize(&self, width: u32, height: u32) {
        self.0.resize(width.max(1), height.max(1))
    }

    /// Tests: whether it is shown on the screen now. On its loop's thread, in the process that
    /// opened it (as every call here).
    pub fn shown(&self) -> bool {
        self.0.shown()
    }

    /// Tests: ask it to close, as the person's close button does (the window manager's message,
    /// or AppKit's `performClose:`). Its `closed` runs after, on its loop's thread, as for the
    /// person's close. On its loop's thread, in the process that opened it.
    pub fn ask_to_close(&self) {
        self.0.ask_to_close()
    }
}

/// Whether this process could open a window now, and in plain words why not (no screen).
pub fn available() -> Result<(), String> {
    os::window::available()
}
