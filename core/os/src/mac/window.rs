//! macOS: `NSWindow`s, through the Objective-C runtime (no crate). The first window makes the
//! process an accessory app (no Dock icon, no menu bar of its own) and has the main loop pump
//! AppKit's events ([`pump`]). The person's close reaches `windowShouldClose:` of a delegate class
//! made at run time, which keeps the window and has the window's `closed` run next on the main
//! queue, outside AppKit's call: its owner drops the window there, and AppKit never finds it gone
//! under its own feet. Every pointer here is this process's, used on its main thread only.

use std::ffi::{CString, c_char, c_void};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::main_loop::Loop;
use crate::window::{Native, OnClose};

pub(crate) const API: &str = "cocoa";

type Id = *mut c_void;
type Sel = *const c_void;
type Class = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct Size {
    w: f64,
    h: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    size: Size,
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Class;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    fn objc_allocateClassPair(superclass: Class, name: *const c_char, extra: usize) -> Class;
    fn objc_registerClassPair(cls: Class);
    fn class_addMethod(cls: Class, name: Sel, imp: *const c_void, types: *const c_char) -> bool;
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSDefaultRunLoopMode: Id;
}

fn sel(name: &str) -> Sel {
    let c = CString::new(name).expect("a selector");
    // SAFETY: a NUL-terminated name; selectors live for the process.
    unsafe { sel_registerName(c.as_ptr()) }
}

fn class(name: &str) -> Class {
    let c = CString::new(name).expect("a class name");
    // SAFETY: as `sel`.
    unsafe { objc_getClass(c.as_ptr()) }
}

// `objc_msgSend` cast to each shape it is called with (arm64: no `_stret` variants).
unsafe fn send0(obj: Id, s: &str) -> Id {
    let f: unsafe extern "C" fn(Id, Sel) -> Id = unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { f(obj, sel(s)) }
}
unsafe fn send1(obj: Id, s: &str, a: Id) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, Id) -> Id = unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { f(obj, sel(s), a) }
}
unsafe fn send_u(obj: Id, s: &str, a: usize) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, usize) -> Id = unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { f(obj, sel(s), a) }
}
unsafe fn send_bool(obj: Id, s: &str) -> bool {
    let f: unsafe extern "C" fn(Id, Sel) -> bool = unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { f(obj, sel(s)) }
}

/// Whether AppKit was set up here: the main loop then pumps its events.
static APP: AtomicBool = AtomicBool::new(false);

/// Each window's `closed`, by its `NSWindow`; taken while it runs.
struct Closers(Vec<(usize, Option<OnClose>)>);
static CLOSERS: Mutex<Closers> = Mutex::new(Closers(Vec::new()));

impl Closers {
    fn get_mut(&mut self, w: usize) -> Option<&mut Option<OnClose>> {
        self.0.iter_mut().find(|x| x.0 == w).map(|x| &mut x.1)
    }
}

fn closers() -> MutexGuard<'static, Closers> {
    CLOSERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// `windowShouldClose:`: the person's close. The window stays; its `closed` runs next on the main
/// queue (its owner drops the window there).
extern "C" fn should_close(_: Id, _: Sel, window: Id) -> bool {
    let w = window as usize;
    super::main_loop::on_main_queue(Box::new(move || closed(w)));
    false
}

/// Call the `closed` of the window `w`, if it is still open, with no lock held.
fn closed(w: usize) {
    let f = closers().get_mut(w).and_then(Option::take);
    if let Some(mut f) = f {
        f();
        if let Some(slot) = closers().get_mut(w) {
            *slot = Some(f);
        }
    }
}

/// The delegate class, made once.
fn delegate_class() -> Class {
    struct C(Class);
    // SAFETY: a class object is immutable once registered.
    unsafe impl Send for C {}
    unsafe impl Sync for C {}
    static CLS: OnceLock<C> = OnceLock::new();
    CLS.get_or_init(|| {
        // SAFETY: a new subclass of NSObject with one method, registered before use.
        unsafe {
            let cls = objc_allocateClassPair(class("NSObject"), c"M2PluginWindowDelegate".as_ptr(), 0);
            class_addMethod(cls, sel("windowShouldClose:"), should_close as *const c_void, c"c@:@".as_ptr());
            objc_registerClassPair(cls);
            C(cls)
        }
    })
    .0
}

/// Make this process an accessory app, once: no Dock icon, its windows still shown.
fn app() -> Id {
    // SAFETY: AppKit's shared application, on the main thread.
    unsafe {
        let app = send0(class("NSApplication") as Id, "sharedApplication");
        if !APP.swap(true, Ordering::AcqRel) {
            // NSApplicationActivationPolicyAccessory.
            send_u(app, "setActivationPolicy:", 1);
            send0(app, "finishLaunching");
        }
        app
    }
}

/// One turn of AppKit's event loop, waiting at most `most`: what the main loop runs once a
/// window was opened. The main dispatch queue and run loop timers are served while it waits.
pub(crate) fn pump(most: std::time::Duration) -> bool {
    if !APP.load(Ordering::Acquire) {
        return false;
    }
    // SAFETY: AppKit on the main thread: the next event, if any, sent where it goes.
    unsafe {
        let app = app();
        let date = {
            let f: unsafe extern "C" fn(Id, Sel, f64) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            f(class("NSDate") as Id, sel("dateWithTimeIntervalSinceNow:"), most.as_secs_f64())
        };
        let next: unsafe extern "C" fn(Id, Sel, u64, Id, Id, bool) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let event = next(app, sel("nextEventMatchingMask:untilDate:inMode:dequeue:"), u64::MAX, date, NSDefaultRunLoopMode, true);
        if !event.is_null() {
            send1(app, "sendEvent:", event);
        }
        send0(app, "updateWindows");
    }
    true
}

pub(crate) struct Window {
    window: Id,
    delegate: Id,
}

// SAFETY: made, used and released on the main thread only (its owner's rule).
unsafe impl Send for Window {}

impl Window {
    pub(crate) fn open(_: &Loop, title: &str, width: u32, height: u32, closed: OnClose) -> io::Result<Window> {
        let app = app();
        // SAFETY: AppKit on the main thread: a titled, closable window, its delegate ours.
        unsafe {
            let alloc = send0(class("NSWindow") as Id, "alloc");
            let init: unsafe extern "C" fn(Id, Sel, Rect, usize, usize, bool) -> Id =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            // Titled, closable, miniaturizable; buffered.
            let rect = Rect { x: 200.0, y: 200.0, size: Size { w: width as f64, h: height as f64 } };
            let window = init(alloc, sel("initWithContentRect:styleMask:backing:defer:"), rect, 1 | 2 | 4, 2, false);
            if window.is_null() {
                return Err(io::Error::other("AppKit refused a new window"));
            }
            let set_bool: unsafe extern "C" fn(Id, Sel, bool) = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            set_bool(window, sel("setReleasedWhenClosed:"), false);
            let t = super::cf::string(title);
            send1(window, "setTitle:", t as Id);
            super::cf::CFRelease(t);
            let delegate = send0(send0(delegate_class() as Id, "alloc"), "init");
            send1(window, "setDelegate:", delegate);
            closers().0.push((window as usize, Some(closed)));
            send1(window, "makeKeyAndOrderFront:", std::ptr::null_mut());
            set_bool(app, sel("activateIgnoringOtherApps:"), true);
            Ok(Window { window, delegate })
        }
    }

    pub(crate) fn native(&self) -> Native {
        // SAFETY: the window's content view, which lives as long as the window.
        Native(unsafe { send0(self.window, "contentView") } as u64)
    }

    pub(crate) fn resize(&self, width: u32, height: u32) {
        // SAFETY: AppKit on the main thread.
        unsafe {
            let f: unsafe extern "C" fn(Id, Sel, Size) = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            f(self.window, sel("setContentSize:"), Size { w: width as f64, h: height as f64 });
        }
    }

    pub(crate) fn shown(&self) -> bool {
        // SAFETY: this process's window, alive while `self` is, asked on the main thread.
        unsafe { send_bool(self.window, "isVisible") }
    }

    pub(crate) fn ask_to_close(&self) {
        // SAFETY: as `shown`: the title bar's close button's action, which asks the delegate.
        unsafe { send1(self.window, "performClose:", std::ptr::null_mut()) };
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        closers().0.retain(|x| x.0 != self.window as usize);
        // SAFETY: the window and delegate `open` made, closed and released once, on the main thread.
        unsafe {
            send1(self.window, "setDelegate:", std::ptr::null_mut());
            send0(self.window, "close");
            send0(self.window, "release");
            send0(self.delegate, "release");
        }
    }
}

pub(crate) fn available() -> Result<(), String> {
    Ok(())
}
