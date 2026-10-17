//! macOS: the few CoreFoundation calls `m2-os` makes, declared once (the file watcher's paths, the
//! output device's id for engine threads).

use std::ffi::c_void;

pub(crate) type CFIndex = isize;
pub(crate) type CFRef = *const c_void;

#[repr(C)]
pub(crate) struct CFArrayCallBacks {
    version: CFIndex,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
    equal: *const c_void,
}

/// `kCFStringEncodingUTF8`.
pub(crate) const UTF8: u32 = 0x0800_0100;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(crate) static kCFTypeArrayCallBacks: CFArrayCallBacks;
    pub(crate) fn CFStringCreateWithBytes(alloc: CFRef, bytes: *const u8, len: CFIndex, encoding: u32, external: u8) -> CFRef;
    pub(crate) fn CFArrayCreate(alloc: CFRef, values: *const CFRef, n: CFIndex, callbacks: *const CFArrayCallBacks) -> CFRef;
    pub(crate) fn CFRelease(cf: CFRef);
}

/// `text` as a new `CFString` (null if CoreFoundation refused), for the caller to `CFRelease`.
pub(crate) fn string(text: &str) -> CFRef {
    // SAFETY: bytes and length from one `str`, valid for the call; CoreFoundation copies them.
    unsafe { CFStringCreateWithBytes(std::ptr::null(), text.as_ptr(), text.len() as CFIndex, UTF8, 0) }
}
