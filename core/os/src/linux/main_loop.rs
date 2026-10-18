//! Linux: a `poll` loop. There is no process-wide run loop for plugins to post to: they get main
//! thread time only from the host, through CLAP's fd and timer extensions, which this loop serves.
//! So the app's loop is a thread of its own, and GTK keeps the main thread.
//!
//! One turn: wait in `poll` on the loop's `eventfd` (rung by a post or a change to what is
//! watched) and the watched fds, for at most the time to the next timer; then run the jobs queued,
//! the timers due and the fds ready. A callback runs with no lock held, so it may post, watch or
//! cancel anything, itself included.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::main_loop::{Job, OnFd, OnTimer, Ready, Timer, Watch, Where};
use crate::thread;

pub(crate) const APP_ON_PROCESS_MAIN: bool = false;

pub(crate) struct Loop {
    inner: Arc<Inner>,
    /// A loop thread of its own: it ends when the loop is dropped.
    own_thread: bool,
}

struct Inner {
    jobs: Mutex<VecDeque<Job>>,
    regs: Mutex<Regs>,
    /// Rung to end a `poll` early.
    wake: i32,
    thread: AtomicU64,
    closed: AtomicBool,
}

#[derive(Default)]
struct Regs {
    next: u64,
    fds: Vec<FdReg>,
    timers: Vec<TimerReg>,
}

struct FdReg {
    id: u64,
    fd: i32,
    want: Ready,
    /// Taken while it runs.
    f: Option<OnFd>,
}

struct TimerReg {
    id: u64,
    period: Duration,
    due: Instant,
    f: Option<OnTimer>,
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Drop for Inner {
    fn drop(&mut self) {
        // SAFETY: the eventfd this loop made, closed once.
        unsafe { libc::close(self.wake) };
    }
}

impl Inner {
    fn new() -> io::Result<Arc<Inner>> {
        // SAFETY: a plain system call.
        let wake = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if wake < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Arc::new(Inner {
            jobs: Mutex::default(),
            regs: Mutex::default(),
            wake,
            thread: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }))
    }

    fn ring(&self) {
        let one = 1u64;
        // SAFETY: eight bytes into the loop's eventfd; a full counter only means it is rung.
        unsafe { libc::write(self.wake, &one as *const u64 as *const libc::c_void, 8) };
    }

    /// One turn, waiting at most `most`.
    fn turn(&self, most: Duration) {
        let now = Instant::now();
        let (mut polls, ids, next) = {
            let r = locked(&self.regs);
            let mut polls = vec![libc::pollfd { fd: self.wake, events: libc::POLLIN, revents: 0 }];
            let mut ids = vec![0];
            for f in &r.fds {
                let mut events = 0;
                if f.want.read {
                    events |= libc::POLLIN;
                }
                if f.want.write {
                    events |= libc::POLLOUT;
                }
                polls.push(libc::pollfd { fd: f.fd, events, revents: 0 });
                ids.push(f.id);
            }
            (polls, ids, r.timers.iter().map(|t| t.due).min())
        };
        let mut wait = next.map_or(most, |d| d.saturating_duration_since(now).min(most));
        if !locked(&self.jobs).is_empty() {
            wait = Duration::ZERO;
        }
        let ms = wait.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as i32;
        // SAFETY: `polls` is a live array of that many pollfds.
        let n = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, ms) };
        if n > 0 && polls[0].revents != 0 {
            let mut v = 0u64;
            // SAFETY: eight bytes out of the loop's eventfd, which is non-blocking.
            unsafe { libc::read(self.wake, &mut v as *mut u64 as *mut libc::c_void, 8) };
        }
        let jobs = std::mem::take(&mut *locked(&self.jobs));
        for job in jobs {
            job();
        }
        self.timers();
        if n > 0 {
            for (p, &id) in polls.iter().zip(&ids).skip(1) {
                if p.revents == 0 {
                    continue;
                }
                let ready = Ready {
                    read: p.revents & libc::POLLIN != 0,
                    write: p.revents & libc::POLLOUT != 0,
                    error: p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0,
                };
                self.fd_ready(id, ready);
                // Closed while watched: polling it again would spin.
                if p.revents & libc::POLLNVAL != 0 {
                    locked(&self.regs).fds.retain(|f| f.id != id);
                }
            }
        }
    }

    fn timers(&self) {
        let now = Instant::now();
        loop {
            let due = {
                let mut r = locked(&self.regs);
                let Some(t) = r.timers.iter_mut().filter(|t| t.due <= now && t.f.is_some()).min_by_key(|t| t.due) else { return };
                // Once per turn at most, and never once per period missed.
                t.due = (t.due + t.period).max(now + t.period);
                (t.id, t.f.take())
            };
            let (id, Some(mut f)) = due else { return };
            f();
            if let Some(t) = locked(&self.regs).timers.iter_mut().find(|t| t.id == id) {
                t.f = Some(f);
            }
        }
    }

    fn fd_ready(&self, id: u64, ready: Ready) {
        let Some(mut f) = locked(&self.regs).fds.iter_mut().find(|f| f.id == id).and_then(|f| f.f.take()) else { return };
        f(ready);
        if let Some(r) = locked(&self.regs).fds.iter_mut().find(|r| r.id == id) {
            r.f = Some(f);
        }
    }
}

impl Loop {
    pub(crate) fn start(at: Where) -> io::Result<Loop> {
        let name = match at {
            Where::ProcessMain if super::is_process_main() => None,
            Where::ProcessMain => return Err(io::Error::other("the process's main loop starts on the process's main thread")),
            Where::AppMain => Some("m2-app-main"),
            Where::Thread(name) => Some(name),
        };
        let inner = Inner::new()?;
        let Some(name) = name else {
            inner.thread.store(thread::token() as u64, Ordering::Release);
            return Ok(Loop { inner, own_thread: false });
        };
        let (ready_tx, ready) = std::sync::mpsc::channel();
        let there = inner.clone();
        std::thread::Builder::new().name(name.into()).spawn(move || {
            there.thread.store(thread::token() as u64, Ordering::Release);
            let _ = ready_tx.send(());
            while !there.closed.load(Ordering::Acquire) {
                there.turn(Duration::from_secs(3600));
            }
        })?;
        ready.recv().map_err(|_| io::Error::other("the loop thread ended as it started"))?;
        Ok(Loop { inner, own_thread: true })
    }

    pub(crate) fn post(&self, job: Job) {
        locked(&self.inner.jobs).push_back(job);
        self.inner.ring();
    }

    pub(crate) fn is_current(&self) -> bool {
        thread::token() as u64 == self.inner.thread.load(Ordering::Acquire)
    }

    pub(crate) fn run_for(&self, most: Duration) {
        assert!(!self.own_thread && self.is_current(), "Loop::run_for runs a ProcessMain loop, on its own thread");
        self.inner.turn(most)
    }

    pub(crate) fn watch(&self, fd: i32, want: Ready, f: OnFd) -> io::Result<Watch> {
        if fd < 0 {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        let mut r = locked(&self.inner.regs);
        r.next += 1;
        let id = r.next;
        r.fds.push(FdReg { id, fd, want, f: Some(f) });
        drop(r);
        self.inner.ring();
        Ok(Watch(id))
    }

    pub(crate) fn rewatch(&self, w: Watch, want: Ready) -> bool {
        let found = locked(&self.inner.regs).fds.iter_mut().find(|f| f.id == w.0).map(|f| f.want = want).is_some();
        self.inner.ring();
        found
    }

    pub(crate) fn unwatch(&self, w: Watch) {
        locked(&self.inner.regs).fds.retain(|f| f.id != w.0);
        self.inner.ring();
    }

    pub(crate) fn timer(&self, period: Duration, f: OnTimer) -> io::Result<Timer> {
        let mut r = locked(&self.inner.regs);
        r.next += 1;
        let id = r.next;
        r.timers.push(TimerReg { id, period, due: Instant::now() + period, f: Some(f) });
        drop(r);
        self.inner.ring();
        Ok(Timer(id))
    }

    pub(crate) fn cancel(&self, t: Timer) {
        locked(&self.inner.regs).timers.retain(|x| x.id != t.0);
        self.inner.ring();
    }
}

impl Drop for Loop {
    fn drop(&mut self) {
        if self.own_thread {
            self.inner.closed.store(true, Ordering::Release);
            self.inner.ring();
        }
    }
}
