//! macOS: the main dispatch queue, which AppKit drains in the app and a `CFRunLoop` drains under
//! `run_for`. Plugins post their own work to the main run loop too, so the plugin main thread must
//! be the process's (HOST-2). A loop thread (tests) runs a `CFRunLoop` of its own, its jobs a run
//! loop source.
//!
//! Timers are `CFRunLoopTimer`s and fds `CFFileDescriptor`s on the loop's run loop, in its common
//! modes (so they fire while AppKit tracks a drag too). Each one's callback is in a cell that
//! CoreFoundation holds through the context's retain and release, so a timer cancelled from inside
//! its own callback, or from another thread, never frees what runs.

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::cf::{CFIndex, CFRef, CFRelease};
use crate::main_loop::{Job, OnFd, OnTimer, Ready, Timer, Watch, Where};
use crate::thread;

pub(crate) const APP_ON_PROCESS_MAIN: bool = true;

pub(crate) struct Loop {
    /// None: the process's main dispatch queue and run loop.
    own: Option<Own>,
    /// The run loop timers and fds go on.
    rl: Rl,
    regs: Mutex<HashMap<u64, Reg>>,
    next: AtomicU64,
}

/// A loop thread's run loop and its job source.
struct Own {
    jobs: Arc<Mutex<VecDeque<Job>>>,
    source: Rl,
    thread: usize,
    closed: Arc<AtomicBool>,
}

/// A CoreFoundation object other threads may signal, wake or add to.
#[derive(Clone, Copy)]
struct Rl(CFRef);

// SAFETY: run loops, sources, timers and file descriptors are thread-safe to add, signal, wake,
// stop and invalidate from any thread (CoreFoundation's thread-safety rules).
unsafe impl Send for Rl {}
unsafe impl Sync for Rl {}

enum Reg {
    Timer(Rl),
    Fd { fd: Rl, source: Rl, cell: Arc<FdCell> },
}

// SAFETY: as `Rl`.
unsafe impl Send for Reg {}

struct TimerCell {
    f: Mutex<Option<OnTimer>>,
}

struct FdCell {
    f: Mutex<Option<OnFd>>,
    /// What it is watched for: CoreFoundation's callback types.
    want: AtomicU8,
}

#[repr(C)]
struct DispatchQueue {
    _opaque: [u8; 0],
}

/// The context `CFRunLoopTimerCreate` and `CFFileDescriptorCreate` take.
#[repr(C)]
struct Context {
    version: CFIndex,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: *const c_void,
}

/// A version 0 run loop source's context.
#[repr(C)]
struct SourceContext {
    version: CFIndex,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: *const c_void,
    equal: *const c_void,
    hash: *const c_void,
    schedule: *const c_void,
    cancel: *const c_void,
    perform: Option<extern "C" fn(*const c_void)>,
}

unsafe extern "C" {
    /// What `dispatch_get_main_queue()` returns (a macro in the C headers).
    static _dispatch_main_q: DispatchQueue;
    fn dispatch_async_f(queue: *const DispatchQueue, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

const READ: u8 = 1;
const WRITE: u8 = 2;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: CFRef;
    static kCFRunLoopCommonModes: CFRef;
    fn CFRunLoopRunInMode(mode: CFRef, seconds: f64, return_after_source_handled: u8) -> i32;
    fn CFRunLoopGetMain() -> CFRef;
    fn CFRunLoopGetCurrent() -> CFRef;
    fn CFRunLoopStop(rl: CFRef);
    fn CFRunLoopWakeUp(rl: CFRef);
    fn CFRunLoopAddSource(rl: CFRef, source: CFRef, mode: CFRef);
    fn CFRunLoopAddTimer(rl: CFRef, timer: CFRef, mode: CFRef);
    fn CFRunLoopSourceCreate(alloc: CFRef, order: CFIndex, ctx: *mut SourceContext) -> CFRef;
    fn CFRunLoopSourceSignal(source: CFRef);
    fn CFRunLoopSourceInvalidate(source: CFRef);
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopTimerCreate(
        alloc: CFRef,
        fire: f64,
        interval: f64,
        flags: usize,
        order: CFIndex,
        callout: extern "C" fn(CFRef, *mut c_void),
        ctx: *mut Context,
    ) -> CFRef;
    fn CFRunLoopTimerInvalidate(timer: CFRef);
    fn CFFileDescriptorCreate(
        alloc: CFRef,
        fd: i32,
        close_on_invalidate: u8,
        callout: extern "C" fn(CFRef, usize, *mut c_void),
        ctx: *const Context,
    ) -> CFRef;
    fn CFFileDescriptorEnableCallBacks(f: CFRef, types: usize);
    fn CFFileDescriptorDisableCallBacks(f: CFRef, types: usize);
    fn CFFileDescriptorCreateRunLoopSource(alloc: CFRef, f: CFRef, order: CFIndex) -> CFRef;
    fn CFFileDescriptorInvalidate(f: CFRef);
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run `job` on the main dispatch queue, in a later turn of the main thread's run loop.
pub(crate) fn on_main_queue(job: Job) {
    let ctx = Box::into_raw(Box::new(job)) as *mut c_void;
    // SAFETY: the main queue lives for the whole process; the trampoline takes the box back.
    unsafe { dispatch_async_f(&_dispatch_main_q, ctx, trampoline) };
}

extern "C" fn trampoline(ctx: *mut c_void) {
    // SAFETY: `post` boxed the job and gave up ownership of it.
    let job = unsafe { Box::from_raw(ctx as *mut Job) };
    job();
}

// The cells' reference counts, which CoreFoundation keeps through the contexts.
extern "C" fn retain_timer(p: *const c_void) -> *const c_void {
    // SAFETY: `p` is an `Arc<TimerCell>`'s pointer, alive while CoreFoundation holds it.
    unsafe { Arc::increment_strong_count(p as *const TimerCell) };
    p
}
extern "C" fn release_timer(p: *const c_void) {
    // SAFETY: as in `retain_timer`; each release matches a retain.
    unsafe { Arc::decrement_strong_count(p as *const TimerCell) };
}
extern "C" fn retain_fd(p: *const c_void) -> *const c_void {
    // SAFETY: as `retain_timer`, for an `Arc<FdCell>`.
    unsafe { Arc::increment_strong_count(p as *const FdCell) };
    p
}
extern "C" fn release_fd(p: *const c_void) {
    // SAFETY: as `release_timer`.
    unsafe { Arc::decrement_strong_count(p as *const FdCell) };
}
extern "C" fn retain_jobs(p: *const c_void) -> *const c_void {
    // SAFETY: as `retain_timer`, for the jobs queue.
    unsafe { Arc::increment_strong_count(p as *const Mutex<VecDeque<Job>>) };
    p
}
extern "C" fn release_jobs(p: *const c_void) {
    // SAFETY: as `release_timer`.
    unsafe { Arc::decrement_strong_count(p as *const Mutex<VecDeque<Job>>) };
}

extern "C" fn timer_fired(_: CFRef, info: *mut c_void) {
    // SAFETY: CoreFoundation holds the cell while the timer can fire.
    let cell = unsafe { &*(info as *const TimerCell) };
    let Some(mut f) = locked(&cell.f).take() else { return };
    f();
    let mut slot = locked(&cell.f);
    if slot.is_none() {
        *slot = Some(f);
    }
}

extern "C" fn fd_ready(fd: CFRef, types: usize, info: *mut c_void) {
    // SAFETY: as in `timer_fired`.
    let cell = unsafe { &*(info as *const FdCell) };
    let Some(mut f) = locked(&cell.f).take() else { return };
    f(Ready { read: types & READ as usize != 0, write: types & WRITE as usize != 0, error: false });
    let mut slot = locked(&cell.f);
    if slot.is_none() {
        *slot = Some(f);
    }
    drop(slot);
    // A file descriptor's callbacks fire once each: enabled again for the next.
    // SAFETY: the object this callback was called for.
    unsafe { CFFileDescriptorEnableCallBacks(fd, cell.want.load(Ordering::Relaxed) as usize) };
}

extern "C" fn perform_jobs(info: *const c_void) {
    // SAFETY: the run loop source holds the queue while it can fire.
    let jobs = unsafe { &*(info as *const Mutex<VecDeque<Job>>) };
    let now = std::mem::take(&mut *locked(jobs));
    for job in now {
        job();
    }
}

fn want_bits(w: Ready) -> u8 {
    (if w.read { READ } else { 0 }) | (if w.write { WRITE } else { 0 })
}

impl Loop {
    pub(crate) fn start(at: Where) -> io::Result<Loop> {
        let regs = Mutex::default();
        match at {
            Where::ProcessMain | Where::AppMain if super::is_process_main() => {
                // SAFETY: the main run loop lives as long as the process.
                Ok(Loop { own: None, rl: Rl(unsafe { CFRunLoopGetMain() }), regs, next: AtomicU64::new(0) })
            }
            Where::ProcessMain | Where::AppMain => Err(io::Error::other("the main dispatch queue's loop starts on the main thread")),
            Where::Thread(name) => {
                let jobs: Arc<Mutex<VecDeque<Job>>> = Arc::default();
                let closed = Arc::new(AtomicBool::new(false));
                let (tx, rx) = std::sync::mpsc::channel::<(usize, usize, usize)>();
                let (queue, stop) = (jobs.clone(), closed.clone());
                std::thread::Builder::new().name(name.into()).spawn(move || {
                    let mut ctx = SourceContext {
                        version: 0,
                        info: Arc::as_ptr(&queue) as *mut c_void,
                        retain: Some(retain_jobs),
                        release: Some(release_jobs),
                        copy_description: std::ptr::null(),
                        equal: std::ptr::null(),
                        hash: std::ptr::null(),
                        schedule: std::ptr::null(),
                        cancel: std::ptr::null(),
                        perform: Some(perform_jobs),
                    };
                    // SAFETY: this thread's run loop; the source retains the queue through the context.
                    let (rl, source) = unsafe {
                        let rl = CFRunLoopGetCurrent();
                        let source = CFRunLoopSourceCreate(std::ptr::null(), 0, &mut ctx);
                        CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
                        (rl, source)
                    };
                    let _ = tx.send((rl as usize, source as usize, thread::token()));
                    while !stop.load(Ordering::Acquire) {
                        // SAFETY: this thread's run loop, in its default mode.
                        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1.0, 0) };
                    }
                    // SAFETY: the source this thread made, invalidated and released once.
                    unsafe {
                        CFRunLoopSourceInvalidate(source);
                        CFRelease(source);
                    }
                })?;
                let (rl, source, thread) = rx.recv().map_err(|_| io::Error::other("the loop thread ended as it started"))?;
                Ok(Loop {
                    own: Some(Own { jobs, source: Rl(source as CFRef), thread, closed }),
                    rl: Rl(rl as CFRef),
                    regs,
                    next: AtomicU64::new(0),
                })
            }
        }
    }

    pub(crate) fn post(&self, job: Job) {
        match &self.own {
            None => on_main_queue(job),
            Some(o) => {
                locked(&o.jobs).push_back(job);
                // SAFETY: the loop thread's source and run loop, alive while the loop is.
                unsafe {
                    CFRunLoopSourceSignal(o.source.0);
                    CFRunLoopWakeUp(self.rl.0);
                }
            }
        }
    }

    pub(crate) fn is_current(&self) -> bool {
        match &self.own {
            None => super::is_process_main(),
            Some(o) => thread::token() == o.thread,
        }
    }

    /// Run the main thread's run loop for at most `most`, returning after anything ran. The main
    /// dispatch queue is drained there.
    pub(crate) fn run_for(&self, most: Duration) {
        assert!(self.own.is_none() && super::is_process_main(), "Loop::run_for runs a ProcessMain loop, on its own thread");
        // Once a window was opened, AppKit's events too (the plugin host process).
        if super::window::pump(most) {
            return;
        }
        // SAFETY: the main thread's run loop, on the main thread.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, most.as_secs_f64(), 1) };
    }

    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub(crate) fn watch(&self, fd: i32, want: Ready, f: OnFd) -> io::Result<Watch> {
        let cell = Arc::new(FdCell { f: Mutex::new(Some(f)), want: AtomicU8::new(want_bits(want)) });
        let ctx = Context {
            version: 0,
            info: Arc::as_ptr(&cell) as *mut c_void,
            retain: Some(retain_fd),
            release: Some(release_fd),
            copy_description: std::ptr::null(),
        };
        // SAFETY: CoreFoundation retains the cell through the context; the fd stays the caller's.
        let (f, source) = unsafe {
            let f = CFFileDescriptorCreate(std::ptr::null(), fd, 0, fd_ready, &ctx);
            if f.is_null() {
                return Err(io::Error::other("CoreFoundation refused to watch the file descriptor"));
            }
            CFFileDescriptorEnableCallBacks(f, want_bits(want) as usize);
            let source = CFFileDescriptorCreateRunLoopSource(std::ptr::null(), f, 0);
            CFRunLoopAddSource(self.rl.0, source, kCFRunLoopCommonModes);
            CFRunLoopWakeUp(self.rl.0);
            (f, source)
        };
        let id = self.id();
        locked(&self.regs).insert(id, Reg::Fd { fd: Rl(f), source: Rl(source), cell });
        Ok(Watch(id))
    }

    pub(crate) fn rewatch(&self, w: Watch, want: Ready) -> bool {
        let regs = locked(&self.regs);
        let Some(Reg::Fd { fd, cell, .. }) = regs.get(&w.0) else { return false };
        let bits = want_bits(want);
        cell.want.store(bits, Ordering::Relaxed);
        // SAFETY: a live file descriptor object of this loop.
        unsafe {
            CFFileDescriptorDisableCallBacks(fd.0, (READ | WRITE) as usize);
            CFFileDescriptorEnableCallBacks(fd.0, bits as usize);
        }
        true
    }

    pub(crate) fn unwatch(&self, w: Watch) {
        let Some(Reg::Fd { fd, source, cell }) = locked(&self.regs).remove(&w.0) else { return };
        locked(&cell.f).take();
        // SAFETY: the objects `watch` made, invalidated and released once; the cell goes with them.
        unsafe {
            CFFileDescriptorInvalidate(fd.0);
            CFRunLoopSourceInvalidate(source.0);
            CFRelease(source.0);
            CFRelease(fd.0);
        }
    }

    pub(crate) fn timer(&self, period: Duration, f: OnTimer) -> io::Result<Timer> {
        let cell = Arc::new(TimerCell { f: Mutex::new(Some(f)) });
        let mut ctx = Context {
            version: 0,
            info: Arc::as_ptr(&cell) as *mut c_void,
            retain: Some(retain_timer),
            release: Some(release_timer),
            copy_description: std::ptr::null(),
        };
        let p = period.as_secs_f64();
        // SAFETY: CoreFoundation retains the cell through the context; the timer is added to this
        // loop's run loop and released by `cancel`.
        let t = unsafe {
            let t = CFRunLoopTimerCreate(std::ptr::null(), CFAbsoluteTimeGetCurrent() + p, p, 0, 0, timer_fired, &mut ctx);
            if t.is_null() {
                return Err(io::Error::other("CoreFoundation refused a timer"));
            }
            CFRunLoopAddTimer(self.rl.0, t, kCFRunLoopCommonModes);
            t
        };
        drop(cell);
        let id = self.id();
        locked(&self.regs).insert(id, Reg::Timer(Rl(t)));
        Ok(Timer(id))
    }

    pub(crate) fn cancel(&self, t: Timer) {
        let Some(Reg::Timer(t)) = locked(&self.regs).remove(&t.0) else { return };
        // SAFETY: the timer `timer` made, invalidated and released once.
        unsafe {
            CFRunLoopTimerInvalidate(t.0);
            CFRelease(t.0);
        }
    }
}

impl Drop for Loop {
    fn drop(&mut self) {
        for (_, r) in locked(&self.regs).drain() {
            // SAFETY: as in `cancel` and `unwatch`.
            unsafe {
                match r {
                    Reg::Timer(t) => {
                        CFRunLoopTimerInvalidate(t.0);
                        CFRelease(t.0);
                    }
                    Reg::Fd { fd, source, .. } => {
                        CFFileDescriptorInvalidate(fd.0);
                        CFRunLoopSourceInvalidate(source.0);
                        CFRelease(source.0);
                        CFRelease(fd.0);
                    }
                }
            }
        }
        if let Some(o) = &self.own {
            o.closed.store(true, Ordering::Release);
            // SAFETY: the loop thread's run loop: stopped, then woken in case it waits.
            unsafe {
                CFRunLoopStop(self.rl.0);
                CFRunLoopWakeUp(self.rl.0);
            }
        }
    }
}
