//! macOS: engine threads are time-constraint threads at the device's period, in the audio
//! workgroup of the output device's IO thread (Apple's "Workgroup Management": a real-time thread
//! triggered by work on the device's IO thread joins that thread's workgroup). The workgroup is
//! read from CoreAudio by the device's UID, after its IO started.

use std::cell::{Cell, UnsafeCell};
use std::ffi::c_void;
use std::time::Duration;

use super::cf::{self, CFRef, CFRelease};
use crate::rt::{Joined, policy};

type ObjectId = u32;

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

const fn code(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}

/// `kAudioObjectSystemObject`.
const SYSTEM: ObjectId = 1;
const TRANSLATE_UID: u32 = code(b"uidd");
const DEFAULT_OUTPUT: u32 = code(b"dOut");
/// `kAudioDevicePropertyIOThreadOSWorkgroup`.
const IO_WORKGROUP: u32 = code(b"oswg");
const GLOBAL: u32 = code(b"glob");
/// `kAudioObjectPropertyElementMain`.
const MAIN: u32 = 0;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(
        object: ObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

/// `os_workgroup_join_token_s`: a signature and 36 opaque bytes.
#[repr(C)]
struct Token {
    sig: u32,
    opaque: [u8; 36],
}

/// `mach_timebase_info_data_t`: ns a tick is `numer / denom`.
#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

// libSystem (os/workgroup_object.h, os/object.h, mach/mach_time.h).
unsafe extern "C" {
    fn mach_timebase_info(info: *mut Timebase) -> libc::kern_return_t;
    fn os_workgroup_join(wg: *mut c_void, token: *mut Token) -> i32;
    fn os_workgroup_leave(wg: *mut c_void, token: *mut Token);
    fn os_workgroup_max_parallel_threads(wg: *mut c_void, attr: *const c_void) -> i32;
    fn os_release(object: *mut c_void);
}

/// A retained `os_workgroup_t`, or null; released when dropped.
pub(crate) struct Workgroup(*mut c_void);

// SAFETY: a workgroup is an OS object meant for many threads; this holds one reference to it.
unsafe impl Send for Workgroup {}
unsafe impl Sync for Workgroup {}

impl Workgroup {
    pub(crate) fn none() -> Workgroup {
        Workgroup(std::ptr::null_mut())
    }

    pub(crate) fn recommends(&self) -> Option<usize> {
        if self.0.is_null() {
            return None;
        }
        // SAFETY: a workgroup this holds a reference to.
        let n = unsafe { os_workgroup_max_parallel_threads(self.0, std::ptr::null()) };
        (n > 0).then_some(n as usize)
    }
}

impl Drop for Workgroup {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: CoreAudio gave this reference to the caller to release.
            unsafe { os_release(self.0) };
        }
    }
}

/// A property of `object` whose value is a `T`, with an optional qualifier.
fn property<T: Copy>(object: ObjectId, selector: u32, qualifier: Option<&CFRef>, empty: T) -> Result<T, i32> {
    let address = Address { selector, scope: GLOBAL, element: MAIN };
    let mut value = empty;
    let mut size = size_of::<T>() as u32;
    let (q_size, q) = match qualifier {
        Some(q) => (size_of::<CFRef>() as u32, (q as *const CFRef).cast::<c_void>()),
        None => (0, std::ptr::null()),
    };
    // SAFETY: `address`, the qualifier and `value` outlive the call, and `size` is `value`'s.
    match unsafe { AudioObjectGetPropertyData(object, &address, q_size, q, &mut size, (&mut value as *mut T).cast()) } {
        0 => Ok(value),
        status => Err(status),
    }
}

pub(crate) fn output_workgroup(uid: Option<&str>) -> Result<Workgroup, String> {
    let device = match uid {
        Some(uid) => {
            let name = cf::string(uid);
            if name.is_null() {
                return Err(format!("no workgroup: CoreFoundation refused the device id {uid:?}"));
            }
            let device = property::<ObjectId>(SYSTEM, TRANSLATE_UID, Some(&name), 0);
            // SAFETY: the string made above, used no more.
            unsafe { CFRelease(name) };
            device.map_err(|s| format!("no workgroup: CoreAudio doesn't know the device {uid:?} (error {s})"))?
        }
        None => {
            property::<ObjectId>(SYSTEM, DEFAULT_OUTPUT, None, 0).map_err(|s| format!("no workgroup: no default output (error {s})"))?
        }
    };
    if device == 0 {
        return Err(format!("no workgroup: CoreAudio has no device {}", uid.unwrap_or("for the default output")));
    }
    match property::<*mut c_void>(device, IO_WORKGROUP, None, std::ptr::null_mut()) {
        Ok(wg) if !wg.is_null() => Ok(Workgroup(wg)),
        Ok(_) => Err("no workgroup: the output device has none".into()),
        Err(s) => Err(format!("no workgroup: the output device didn't give its IO thread's (error {s})")),
    }
}

/// The workgroup this thread joined and its token, until it leaves. Touched first at the thread's
/// start: a constant initializer, so no allocation and no destructor.
struct Joining {
    workgroup: Cell<*mut c_void>,
    token: UnsafeCell<Token>,
}

thread_local! {
    static JOINING: Joining = const { Joining { workgroup: Cell::new(std::ptr::null_mut()), token: UnsafeCell::new(Token { sig: 0, opaque: [0; 36] }) } };
}

pub(crate) fn engine_thread(period: Option<Duration>, w: &Workgroup) -> Joined {
    let real_time = period.is_some_and(time_constraint);
    let (workgroup, cancelled) = match w.0.is_null() {
        true => (false, false),
        false => JOINING.with(|j| {
            if !j.workgroup.get().is_null() {
                return (true, false);
            }
            // SAFETY: a workgroup this holds a reference to; the token lives in this thread's
            // own storage until `end_engine_thread` leaves with it.
            match unsafe { os_workgroup_join(w.0, j.token.get()) } {
                0 => {
                    j.workgroup.set(w.0);
                    (true, false)
                }
                // Cancelled: the device's IO stopped or moved.
                libc::EINVAL => (false, true),
                _ => (false, false),
            }
        }),
    };
    Joined { real_time, workgroup, cancelled }
}

pub(crate) fn end_engine_thread() {
    JOINING.with(|j| {
        let wg = j.workgroup.replace(std::ptr::null_mut());
        if !wg.is_null() {
            // SAFETY: the workgroup this thread joined, with the token the join wrote. The group
            // outlives every engine thread (the pool's hooks hold it).
            unsafe { os_workgroup_leave(wg, j.token.get()) };
        }
    })
}

/// CoreAudio made its IO thread real time and put it in the device's workgroup.
pub(crate) fn on_device_thread() -> Joined {
    Joined { real_time: true, workgroup: true, cancelled: false }
}

/// The Mach clock's ns a tick, as (numer, denom).
fn timebase() -> (u32, u32) {
    let mut tb = Timebase { numer: 0, denom: 0 };
    // SAFETY: fills `tb`, which outlives the call.
    unsafe { mach_timebase_info(&mut tb) };
    (tb.numer, tb.denom)
}

/// The calling thread's time-constraint policy at `period`: computes up to half a period, done
/// within one, preemptible. Whether the kernel took it.
fn time_constraint(period: Duration) -> bool {
    let (numer, denom) = timebase();
    let (period, computation, constraint) = policy(period, numer, denom);
    let mut p = libc::thread_time_constraint_policy { period, computation, constraint, preemptible: 1 };
    // SAFETY: the calling thread's own port (no new right is made: `pthread_mach_thread_np`), and a
    // policy struct of the count given, which outlives the call.
    unsafe {
        let thread = libc::pthread_mach_thread_np(libc::pthread_self());
        libc::thread_policy_set(
            thread,
            libc::THREAD_TIME_CONSTRAINT_POLICY as u32,
            (&mut p as *mut libc::thread_time_constraint_policy).cast(),
            libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        ) == libc::KERN_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::rt::{Group, end_engine_thread, engine_thread};

    // 8a: an engine thread runs under the time-constraint policy at the device's period.
    #[test]
    fn an_engine_thread_has_the_time_constraint_policy() {
        let period = Duration::from_nanos(2_666_667);
        let (joined, read) = std::thread::spawn(move || {
            let g = Group::of_output(None, period);
            let j = engine_thread(&g);
            let mut p = libc::thread_time_constraint_policy { period: 0, computation: 0, constraint: 0, preemptible: 0 };
            let mut count = libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT;
            let mut default = 0;
            // SAFETY: this thread's own port; `p` and `count` outlive the call.
            let kr = unsafe {
                libc::thread_policy_get(
                    libc::pthread_mach_thread_np(libc::pthread_self()),
                    libc::THREAD_TIME_CONSTRAINT_POLICY as u32,
                    (&mut p as *mut libc::thread_time_constraint_policy).cast(),
                    &mut count,
                    &mut default,
                )
            };
            end_engine_thread();
            (j, (kr, p.period, p.computation, p.constraint, default))
        })
        .join()
        .unwrap();
        assert!(joined.real_time);
        let (numer, denom) = super::timebase();
        let (period, computation, constraint) = crate::rt::policy(period, numer, denom);
        assert_eq!(read, (libc::KERN_SUCCESS, period, computation, constraint, 0));
    }

    // 8a: a worker joins the default output's workgroup, and leaves it before it exits.
    #[test]
    fn a_worker_joins_and_leaves_the_default_outputs_workgroup() {
        let g = Group::of_output(None, Duration::from_nanos(2_666_667));
        assert_eq!(g.missing(), None);
        assert!(g.recommends().is_some_and(|n| n >= 1), "{:?}", g.recommends());
        let g = std::sync::Arc::new(g);
        for _ in 0..2 {
            let g = g.clone();
            let joined = std::thread::spawn(move || {
                let j = engine_thread(&g);
                // A second call keeps the one join.
                assert!(engine_thread(&g).workgroup);
                end_engine_thread();
                j
            })
            .join()
            .unwrap();
            assert!(joined.real_time && joined.workgroup && !joined.cancelled, "{joined:?}");
        }
    }
}
