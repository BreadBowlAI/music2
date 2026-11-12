//! VIEW-11 in the app, without a window: the shell's own switch ([`frame_rate`], the same file
//! `main` uses) turns WebKit's 60 frames a second setting off in a `WKPreferences`, as it does in
//! the window's. A macOS whose WebKit lists no such setting has nothing to turn off, and says so.
//! Elsewhere there is no WebKit: the page draws at the screen's rate already. WebKit's objects
//! want the main thread, which libtest owns, so this test has no harness.

// What the test doesn't call (`lift`, the window's) is the app's.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/frame_rate.rs"]
mod frame_rate;

#[cfg(target_os = "macos")]
fn main() {
    use std::ffi::{CStr, c_char, c_void};

    type Id = *mut c_void;
    #[link(name = "WebKit", kind = "framework")]
    unsafe extern "C" {}
    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Id;
        fn objc_msgSend();
    }
    // SAFETY: a NUL-terminated selector name.
    let sel = |name: &CStr| unsafe { sel_registerName(name.as_ptr()) };
    // SAFETY: `objc_msgSend` as `+[WKPreferences new]`'s signature, and `_isEnabledForFeature:`'s
    // (a BOOL, one byte, from a `_WKFeature`).
    let new: extern "C" fn(Id, Id) -> Id = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    // SAFETY: as above.
    let enabled: extern "C" fn(Id, Id, Id) -> i8 = unsafe { std::mem::transmute(objc_msgSend as *const ()) };

    let Some(cap) = frame_rate::feature(frame_rate::CAP) else {
        println!("ok: this macOS's WebKit lists no {}: nothing to turn off", frame_rate::CAP.to_string_lossy());
        return;
    };
    // SAFETY: a NUL-terminated class name.
    let prefs = new(unsafe { objc_getClass(c"WKPreferences".as_ptr()) }, sel(c"new"));
    assert!(!prefs.is_null(), "a WKPreferences");
    let before = enabled(prefs, sel(c"_isEnabledForFeature:"), cap);
    assert!(frame_rate::lift_in(prefs), "the switch says it turned the setting off");
    let after = enabled(prefs, sel(c"_isEnabledForFeature:"), cap);
    assert_eq!(after, 0, "the setting is off after the switch (it was {before})");
    println!("ok: {} was {before}, is {after}", frame_rate::CAP.to_string_lossy());
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("ok: no WebKit cap off the Mac");
}
