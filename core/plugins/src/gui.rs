//! Plugin windows (HOST-7), and the main-loop services plugins need for them (HOST-3): CLAP's
//! `gui` embedded in a window of `m2_os::window`, and the host's `timer-support` and
//! `posix-fd-support`, served by the plugin main thread's loop. Everything here runs on the plugin
//! main thread; only the host process opens windows.

use std::ffi::{CString, c_void};
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use clap_sys::ext::gui::*;
use clap_sys::ext::posix_fd_support::*;
use clap_sys::ext::timer_support::*;
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::clap_plugin;
use m2_os::main_loop::Ready;
use m2_os::window::Window;

use crate::host::{Handle, HostEvent};
use crate::inprocess::{Core, HostData, core, ext, host_data};
use crate::main_thread::main_thread;

/// Open `c`'s window, titled `title`; one open already stays as it is.
pub(crate) fn open(c: &Core, h: Handle, title: &str) -> Result<()> {
    let name = c.name();
    if c.host.window.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return Ok(());
    }
    let g = c.ext::<clap_plugin_gui>(CLAP_EXT_GUI).ok_or_else(|| anyhow!("{name} has no window of its own"))?;
    let api = CString::new(m2_os::window::API)?;
    let p = c.plugin;
    // SAFETY: the plugin's gui calls, on the plugin main thread, in CLAP's order.
    unsafe {
        if !g.is_api_supported.is_some_and(|f| f(p, api.as_ptr(), false)) {
            bail!("{name} has no window that can show inside music2's on this computer");
        }
        m2_os::window::available().map_err(|why| anyhow!("{name}'s window can't open: {why}"))?;
        if !g.create.is_some_and(|f| f(p, api.as_ptr(), false)) {
            bail!("{name} couldn't make its window");
        }
        let (mut w, mut ht) = (640u32, 480u32);
        if !g.get_size.is_some_and(|f| f(p, &mut w, &mut ht)) {
            (w, ht) = (640, 480);
        }
        let destroy = || {
            if let Some(f) = g.destroy {
                f(p)
            }
        };
        let lp = main_thread()?.main_loop();
        let window = match Window::open(lp, title, w, ht, Box::new(move || closed_by_person(h))) {
            Ok(w) => w,
            Err(e) => {
                destroy();
                bail!("{name}'s window can't open: {e}");
            }
        };
        let parent = clap_window { api: api.as_ptr(), specific: clap_window_handle { ptr: window.native().0 as usize as *mut c_void } };
        if !g.set_parent.is_some_and(|f| f(p, &parent)) {
            destroy();
            bail!("{name} couldn't put its view in the window");
        }
        if let Some(f) = g.show {
            f(p);
        }
        *c.host.window.lock().unwrap_or_else(|e| e.into_inner()) = Some(window);
    }
    Ok(())
}

/// Close `c`'s window, if open: the plugin's view first, then the window.
pub(crate) fn close(c: &Core) {
    let Some(window) = c.host.window.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    if let Some(g) = c.ext::<clap_plugin_gui>(CLAP_EXT_GUI) {
        // SAFETY: the plugin's gui calls, on the plugin main thread, after `create`.
        unsafe {
            if let Some(f) = g.hide {
                f(c.plugin);
            }
            if let Some(f) = g.destroy {
                f(c.plugin);
            }
        }
    }
    drop(window);
}

/// The person closed `h`'s window: it closes, and its owner hears it.
fn closed_by_person(h: Handle) {
    let Some(c) = core(h) else { return };
    close(c);
    c.owner.send(HostEvent::WindowClosed(h));
}

/// What the plugin asked to be resized to, applied on the main thread (the sweep).
pub(crate) fn resize_asked(hd: &HostData) {
    let wh = hd.resize.swap(0, Ordering::AcqRel);
    if wh == 0 {
        return;
    }
    if let Some(w) = hd.window.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        w.resize((wh >> 32) as u32, wh as u32);
    }
}

/// Every timer and fd the plugin left registered, gone: once it is destroyed.
pub(crate) fn forget(hd: &HostData) {
    let Ok(m) = main_thread() else { return };
    for (_, t) in hd.timers.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
        m.main_loop().cancel(t);
    }
    for (_, w) in hd.fds.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
        m.main_loop().unwatch(w);
    }
}

/// A host data's address, which its callbacks carry: it lives until its instance is destroyed,
/// and `forget` ends them first.
#[derive(Clone, Copy)]
struct At(usize);

impl At {
    fn plugin(self) -> *const clap_plugin {
        // SAFETY: see the type.
        unsafe { &*(self.0 as *const HostData) }.plugin.load(Ordering::Acquire)
    }
}

pub(crate) unsafe extern "C" fn register_timer(h: *const clap_host, period_ms: u32, out: *mut clap_id) -> bool {
    let hd = unsafe { host_data(h) };
    let Ok(m) = main_thread() else { return false };
    if !m.is_current() || out.is_null() {
        return false;
    }
    let id = hd.next_timer.fetch_add(1, Ordering::Relaxed);
    let at = At(hd as *const HostData as usize);
    let t = m.main_loop().timer(
        Duration::from_millis(period_ms.max(1) as u64),
        Box::new(move || {
            let p = at.plugin();
            if let Some(f) = ext::<clap_plugin_timer_support>(p, CLAP_EXT_TIMER_SUPPORT).and_then(|t| t.on_timer) {
                // SAFETY: on the plugin main thread, while the plugin lives.
                unsafe { f(p, id) };
            }
        }),
    );
    let Ok(t) = t else { return false };
    hd.timers.lock().unwrap_or_else(|e| e.into_inner()).push((id, t));
    unsafe { *out = id };
    true
}

pub(crate) unsafe extern "C" fn unregister_timer(h: *const clap_host, id: clap_id) -> bool {
    let hd = unsafe { host_data(h) };
    let (Ok(m), mut timers) = (main_thread(), hd.timers.lock().unwrap_or_else(|e| e.into_inner())) else { return false };
    let Some(i) = timers.iter().position(|t| t.0 == id) else { return false };
    m.main_loop().cancel(timers.remove(i).1);
    true
}

fn want(flags: u32) -> Ready {
    Ready { read: flags & CLAP_POSIX_FD_READ != 0, write: flags & CLAP_POSIX_FD_WRITE != 0, error: flags & CLAP_POSIX_FD_ERROR != 0 }
}

pub(crate) unsafe extern "C" fn register_fd(h: *const clap_host, fd: i32, flags: u32) -> bool {
    let hd = unsafe { host_data(h) };
    let Ok(m) = main_thread() else { return false };
    if !m.is_current() || hd.fds.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|x| x.0 == fd) {
        return false;
    }
    let at = At(hd as *const HostData as usize);
    let w = m.main_loop().watch(
        fd,
        want(flags),
        Box::new(move |r| {
            let p = at.plugin();
            let mut said = 0;
            if r.read {
                said |= CLAP_POSIX_FD_READ;
            }
            if r.write {
                said |= CLAP_POSIX_FD_WRITE;
            }
            if r.error {
                said |= CLAP_POSIX_FD_ERROR;
            }
            if let Some(f) = ext::<clap_plugin_posix_fd_support>(p, CLAP_EXT_POSIX_FD_SUPPORT).and_then(|x| x.on_fd) {
                // SAFETY: on the plugin main thread, while the plugin lives.
                unsafe { f(p, fd, said) };
            }
        }),
    );
    let Ok(w) = w else { return false };
    hd.fds.lock().unwrap_or_else(|e| e.into_inner()).push((fd, w));
    true
}

pub(crate) unsafe extern "C" fn modify_fd(h: *const clap_host, fd: i32, flags: u32) -> bool {
    let hd = unsafe { host_data(h) };
    let Ok(m) = main_thread() else { return false };
    let fds = hd.fds.lock().unwrap_or_else(|e| e.into_inner());
    fds.iter().find(|x| x.0 == fd).is_some_and(|x| m.main_loop().rewatch(x.1, want(flags)))
}

pub(crate) unsafe extern "C" fn unregister_fd(h: *const clap_host, fd: i32) -> bool {
    let hd = unsafe { host_data(h) };
    let (Ok(m), mut fds) = (main_thread(), hd.fds.lock().unwrap_or_else(|e| e.into_inner())) else { return false };
    let Some(i) = fds.iter().position(|x| x.0 == fd) else { return false };
    m.main_loop().unwatch(fds.remove(i).1);
    true
}

pub(crate) unsafe extern "C" fn resize_hints_changed(_: *const clap_host) {}

/// From any thread: applied on the main thread at the next sweep.
pub(crate) unsafe extern "C" fn request_resize(h: *const clap_host, width: u32, height: u32) -> bool {
    let hd = unsafe { host_data(h) };
    hd.resize.store((width.max(1) as u64) << 32 | height.max(1) as u64, Ordering::Release);
    crate::inprocess::PENDING.store(true, Ordering::Release);
    true
}

pub(crate) unsafe extern "C" fn request_show(_: *const clap_host) -> bool {
    false
}

pub(crate) unsafe extern "C" fn request_hide(_: *const clap_host) -> bool {
    false
}

/// Floating windows only, which music2 doesn't ask for.
pub(crate) unsafe extern "C" fn gui_closed(_: *const clap_host, _: bool) {}
