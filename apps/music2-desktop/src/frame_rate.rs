//! The page at the screen's own rate on a Mac (taste.md VIEW-11). WebKit draws a page in an app at
//! most 60 times a second on macOS 13 to 15, even on a 120 Hz screen, by a setting on by default,
//! `PreferPageRenderingUpdatesNear60FPSEnabled` (WebKit bug 294338); Safari turns it off, and the
//! public API can't. WebKit's private list of its settings, `+[WKPreferences _features]`, has it, and
//! `-[WKPreferences _setEnabled:forFeature:]` turns it off. Each private call is asked about first
//! (`respondsToSelector:`), so on a macOS without them nothing is called, and the page keeps 60.
//! Through the Objective-C runtime, as `tests/app_main_thread.rs` does (no new dependency).

use std::ffi::{CStr, c_char, c_void};

type Id = *mut c_void;

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Id;
    fn objc_msgSend();
}

/// The setting that holds the page near 60 frames a second.
pub const CAP: &CStr = c"PreferPageRenderingUpdatesNear60FPSEnabled";

fn class(name: &CStr) -> Id {
    // SAFETY: a NUL-terminated class name.
    unsafe { objc_getClass(name.as_ptr()) }
}

fn sel(name: &CStr) -> Id {
    // SAFETY: a NUL-terminated selector name.
    unsafe { sel_registerName(name.as_ptr()) }
}

/// `objc_msgSend` as the function type of the method it calls (arm64 needs the exact one).
macro_rules! send {
    ($ty:ty) => {
        // SAFETY: the caller names the method's real signature.
        unsafe { std::mem::transmute::<*const (), $ty>(objc_msgSend as *const ()) }
    };
}

/// Whether `obj` answers `name`: a class for its class methods, an object for its own. BOOL is a
/// `signed char` on Intel and a `bool` on arm64: one byte, 0 or 1, either way.
fn answers(obj: Id, name: &CStr) -> bool {
    let responds = send!(extern "C" fn(Id, Id, Id) -> i8);
    !obj.is_null() && responds(obj, sel(c"respondsToSelector:"), sel(name)) != 0
}

/// An object's property that is itself an object, if it has it.
fn get(obj: Id, name: &CStr) -> Option<Id> {
    let get = send!(extern "C" fn(Id, Id) -> Id);
    if !answers(obj, name) {
        return None;
    }
    let got = get(obj, sel(name));
    (!got.is_null()).then_some(got)
}

/// WebKit's `_WKFeature` for the setting `key`, if its list has it.
pub fn feature(key: &CStr) -> Option<Id> {
    let count = send!(extern "C" fn(Id, Id) -> usize);
    let at = send!(extern "C" fn(Id, Id, usize) -> Id);
    let utf8 = send!(extern "C" fn(Id, Id) -> *const c_char);
    let features = get(class(c"WKPreferences"), c"_features")?;
    (0..count(features, sel(c"count"))).map(|i| at(features, sel(c"objectAtIndex:"), i)).find(|&f| {
        let name = get(f, c"key").map_or(std::ptr::null(), |k| utf8(k, sel(c"UTF8String")));
        // SAFETY: UTF8String's NUL-terminated text, alive while its string is.
        !name.is_null() && unsafe { CStr::from_ptr(name) } == key
    })
}

/// Turns the cap off in `prefs`, a `WKPreferences`: true if it did.
pub fn lift_in(prefs: Id) -> bool {
    let set = send!(extern "C" fn(Id, Id, i8, Id));
    let Some(cap) = feature(CAP) else { return false };
    if !answers(prefs, c"_setEnabled:forFeature:") {
        return false;
    }
    set(prefs, sel(c"_setEnabled:forFeature:"), 0, cap);
    true
}

/// Turns the cap off for the window's web view, `view`, a `WKWebView`: true if it did. On the main
/// thread. Its configuration is a copy, but one that shares the web view's own preferences.
pub fn lift(view: *mut c_void) -> bool {
    get(view, c"configuration").and_then(|c| get(c, c"preferences")).is_some_and(lift_in)
}
