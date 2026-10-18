//! macOS: whether Privacy & Security lets music2 use the microphone (AVFoundation's
//! `+[AVCaptureDevice authorizationStatusForMediaType:]`, through the Objective-C runtime).

use std::ffi::{c_char, c_void};

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    // The same signature as `window.rs`'s: one symbol, one declaration.
    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn objc_msgSend();
}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: *const c_void;
}

/// `AVAuthorizationStatusRestricted` and `AVAuthorizationStatusDenied`.
const RESTRICTED: isize = 1;
const DENIED: isize = 2;

/// True when the user (or a policy) said no. Not yet asked is false: opening an input asks, with
/// the bundle's `NSMicrophoneUsageDescription`.
pub(crate) fn microphone_refused() -> bool {
    type Status = unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void) -> isize;
    // SAFETY: the class and selector are looked up by name and checked; objc_msgSend is called
    // through the exact type of the method (a class method taking an NSString, returning an
    // NSInteger), and AVMediaTypeAudio is AVFoundation's constant string.
    unsafe {
        let class = objc_getClass(c"AVCaptureDevice".as_ptr());
        let sel = sel_registerName(c"authorizationStatusForMediaType:".as_ptr());
        if class.is_null() || sel.is_null() {
            return false;
        }
        let send = std::mem::transmute::<unsafe extern "C" fn(), Status>(objc_msgSend);
        matches!(send(class, sel, AVMediaTypeAudio), RESTRICTED | DENIED)
    }
}
