//! What two processes share: the plugin host process (HOST-3) and the app.
//!
//! In short:
//! - [`Shm`]: a region of memory both processes map, found by a name. One side makes it, the other
//!   opens it, and then the name is taken away ([`Shm::remove`]): the region lives while either
//!   side maps it, and nothing is left behind once both are gone.
//! - [`wait`] and [`wake`]: one thread sleeps on a 32-bit word in a shared region until a thread in
//!   either process changes the word and wakes it. No allocation, no lock, no I/O: the audio thread
//!   waits on its call this way.
//!
//! | OS | Shared memory | Wait and wake |
//! |---|---|---|
//! | Linux | `shm_open` + `mmap` | a futex, without the private flag |
//! | macOS | `shm_open` + `mmap` | `__ulock_wait` / `__ulock_wake`, the shared kind (macOS 11 on, as libc++ and Rust's std use it) |
//! | Windows (designed, not built) | `CreateFileMappingW` + `MapViewOfFile` on a named mapping | a named event per word (`CreateEventW`); `WaitOnAddress` doesn't cross processes |

use std::io;
use std::sync::atomic::AtomicU32;
use std::time::Duration;

use crate::os;

/// A region of memory two processes map read-write, zeroed when made. Unmapped when dropped.
pub struct Shm {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: the region is plain memory; what is in it, and who may touch it when, is its user's rule.
unsafe impl Send for Shm {}
unsafe impl Sync for Shm {}

impl Shm {
    /// Make a new region of `len` bytes named `name` (`/` then up to 29 letters, digits or `-`,
    /// which every OS takes). An error when the name is taken.
    pub fn create(name: &str, len: usize) -> io::Result<Shm> {
        check_name(name)?;
        let ptr = os::ipc::create(name, len)?;
        Ok(Shm { ptr, len })
    }

    /// Map the region another process made as `name`, `len` bytes long.
    pub fn open(name: &str, len: usize) -> io::Result<Shm> {
        check_name(name)?;
        let ptr = os::ipc::open(name, len)?;
        Ok(Shm { ptr, len })
    }

    /// Take the name away. The region lives on while a process maps it.
    pub fn remove(name: &str) -> io::Result<()> {
        check_name(name)?;
        os::ipc::remove(name)
    }

    /// Its first byte: page-aligned.
    pub fn ptr(&self) -> *mut u8 {
        self.ptr
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        os::ipc::unmap(self.ptr, self.len);
    }
}

/// macOS keeps 31 bytes of a name; `/` and the rest is what every OS takes.
fn check_name(name: &str) -> io::Result<()> {
    let ok =
        name.len() <= 30 && name.starts_with('/') && name.len() > 1 && name[1..].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    match ok {
        true => Ok(()),
        false => Err(io::Error::new(io::ErrorKind::InvalidInput, "a shared memory name is `/` and up to 29 letters, digits or `-`")),
    }
}

/// Sleep while `word` holds `expected`, at most `at_most`: until a thread in any process that
/// maps it calls [`wake`] on it. Returns at once when it holds another value. It may also return
/// early (a signal, a spurious wake-up): the caller looks at the word again. `word` must be in a
/// [`Shm`] (or this process's own memory). No allocation, no lock, no I/O.
pub fn wait(word: &AtomicU32, expected: u32, at_most: Duration) {
    os::ipc::wait(word, expected, at_most)
}

/// Wake every thread sleeping on `word`, in any process. No allocation, no lock, no I/O.
pub fn wake(word: &AtomicU32) {
    os::ipc::wake(word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    fn name(what: &str) -> String {
        format!("/m2t-{what}-{}", std::process::id())
    }

    // HOST-3: what one mapping writes, another mapping of the same name reads; the name can go
    // while both are mapped, and a name can't be made twice.
    #[test]
    fn two_mappings_share_their_bytes() {
        let n = name("bytes");
        let a = Shm::create(&n, 8192).unwrap();
        assert!(Shm::create(&n, 8192).is_err(), "a name is made once");
        let b = Shm::open(&n, 8192).unwrap();
        Shm::remove(&n).unwrap();
        assert!(Shm::open(&n, 8192).is_err(), "the name is gone");
        assert_ne!(a.ptr(), b.ptr(), "two mappings");
        // SAFETY: both regions are 8192 bytes long and mapped.
        unsafe {
            assert_eq!(*b.ptr().add(8191), 0, "zeroed when made");
            *a.ptr().add(8191) = 7;
            assert_eq!(*b.ptr().add(8191), 7);
        }
        assert!(Shm::create("no-slash", 16).is_err() && Shm::create("/a/b", 16).is_err());
    }

    // HOST-3: a wake reaches a thread sleeping on the same word through another mapping, as it
    // does across processes (a private futex would never reach it: red by FUTEX_PRIVATE_FLAG).
    #[test]
    fn a_wake_crosses_two_mappings() {
        let n = name("wake");
        let a = Shm::create(&n, 4096).unwrap();
        let b = Shm::open(&n, 4096).unwrap();
        Shm::remove(&n).unwrap();
        // SAFETY: the first 4 bytes of each mapping, page-aligned, live while `a` and `b` do.
        let (wa, wb) = unsafe { (&*(a.ptr() as *const AtomicU32), &*(b.ptr() as *const AtomicU32)) };
        let t = Instant::now();
        let slept = std::thread::scope(|s| {
            let sleeper = s.spawn(|| {
                let t = Instant::now();
                while wb.load(Ordering::Acquire) == 0 {
                    wait(wb, 0, Duration::from_secs(5));
                }
                t.elapsed()
            });
            std::thread::sleep(Duration::from_millis(50));
            wa.store(1, Ordering::Release);
            wake(wa);
            sleeper.join().unwrap()
        });
        assert!(slept < Duration::from_secs(2), "woken through the other mapping, not by the time running out: {slept:?}");
        // A wait on a word that holds another value returns at once; one that holds it, after the time.
        wait(wa, 0, Duration::from_secs(5));
        let t2 = Instant::now();
        wait(wa, 1, Duration::from_millis(20));
        assert!(t2.elapsed() >= Duration::from_millis(15), "slept for the time given");
        assert!(t.elapsed() < Duration::from_secs(4));
    }
}
