//! macOS: the main dispatch queue, which AppKit drains in the app and a `CFRunLoop` drains under
//! `run_for`. Plugins post their own work to the main run loop too, so the plugin main thread must
//! be the process's (HOST-2).

use std::ffi::c_void;
use std::io;
use std::time::Duration;

use crate::main_loop::{Job, Queue, Where};

pub(crate) const APP_ON_PROCESS_MAIN: bool = true;

pub(crate) enum Loop {
    /// The process's main dispatch queue.
    Main,
    Queue(Queue),
}

#[repr(C)]
struct DispatchQueue {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    /// What `dispatch_get_main_queue()` returns (a macro in the C headers).
    static _dispatch_main_q: DispatchQueue;
    fn dispatch_async_f(queue: *const DispatchQueue, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source_handled: u8) -> i32;
}

extern "C" fn trampoline(ctx: *mut c_void) {
    // SAFETY: `post` boxed the job and gave up ownership of it.
    let job = unsafe { Box::from_raw(ctx as *mut Job) };
    job();
}

impl Loop {
    pub(crate) fn start(at: Where) -> io::Result<Loop> {
        match at {
            Where::ProcessMain | Where::AppMain if super::is_process_main() => Ok(Loop::Main),
            Where::ProcessMain | Where::AppMain => Err(io::Error::other("the main dispatch queue's loop starts on the main thread")),
            Where::Thread(name) => Queue::start(Some(name)).map(Loop::Queue),
        }
    }

    pub(crate) fn post(&self, job: Job) {
        match self {
            Loop::Main => {
                let ctx = Box::into_raw(Box::new(job)) as *mut c_void;
                // SAFETY: the main queue lives for the whole process; the trampoline takes the box back.
                unsafe { dispatch_async_f(&_dispatch_main_q, ctx, trampoline) };
            }
            Loop::Queue(q) => q.post(job),
        }
    }

    pub(crate) fn is_current(&self) -> bool {
        match self {
            Loop::Main => super::is_process_main(),
            Loop::Queue(q) => q.is_current(),
        }
    }

    /// Run the main thread's run loop for at most `most`, returning after anything ran. The main
    /// dispatch queue is drained there.
    pub(crate) fn run_for(&self, most: Duration) {
        match self {
            Loop::Main => {
                assert!(super::is_process_main(), "Loop::run_for runs a ProcessMain loop, on its own thread");
                // SAFETY: the main thread's run loop, on the main thread.
                unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, most.as_secs_f64(), 1) };
            }
            Loop::Queue(q) => q.run_for(most),
        }
    }
}
