//! What is the same on macOS and Linux. Both OS folders re-export it.

use std::ffi::CString;
use std::fs::{File, Metadata};
use std::io::{self, ErrorKind};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

use crate::fs::FileId;

#[inline]
pub(crate) fn token() -> usize {
    // SAFETY: always safe to call; a register read.
    unsafe { libc::pthread_self() as usize }
}

pub(crate) fn cpu_time() -> Duration {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime fills `t`, which outlives the call.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

pub(crate) fn c_numbers() {
    // Made once and kept for the process: a thread's locale must outlive its use.
    static C: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    // SAFETY: newlocale with "C", which every system has; nothing frees it.
    let c = *C.get_or_init(|| unsafe { libc::newlocale(libc::LC_ALL_MASK, c"C".as_ptr(), std::ptr::null_mut()) } as usize);
    if c != 0 {
        // SAFETY: a locale newlocale made, which lives as long as the process.
        unsafe { libc::uselocale(c as libc::locale_t) };
    }
}

pub(crate) fn foreign(f: &mut (dyn FnMut() + Send)) -> io::Result<()> {
    struct Call<'a> {
        f: &'a mut (dyn FnMut() + Send),
        panic: Option<Box<dyn std::any::Any + Send>>,
    }
    extern "C" fn start(arg: *mut libc::c_void) -> *mut libc::c_void {
        // SAFETY: `arg` is the `Call` below, which outlives this thread: it is joined first.
        let call = unsafe { &mut *arg.cast::<Call>() };
        // Never unwind across the C start routine: keep the panic for the joining thread.
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (call.f)())) {
            call.panic = Some(p);
        }
        std::ptr::null_mut()
    }
    let mut call = Call { f, panic: None };
    // SAFETY: a thread with an 8 MiB stack (macOS gives 512 KiB by default), running `start` on
    // `call`, joined before `call` goes.
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        let inited = libc::pthread_attr_init(&mut attr);
        if inited != 0 {
            return Err(io::Error::from_raw_os_error(inited));
        }
        libc::pthread_attr_setstacksize(&mut attr, 8 << 20);
        let mut t: libc::pthread_t = std::mem::zeroed();
        let made = libc::pthread_create(&mut t, &attr, start, (&mut call as *mut Call).cast());
        libc::pthread_attr_destroy(&mut attr);
        if made != 0 {
            return Err(io::Error::from_raw_os_error(made));
        }
        libc::pthread_join(t, std::ptr::null_mut());
    }
    if let Some(p) = call.panic {
        std::panic::resume_unwind(p);
    }
    Ok(())
}

pub(crate) fn load_average() -> Option<[f64; 3]> {
    let mut l = [0.0f64; 3];
    // SAFETY: getloadavg writes at most 3 numbers into `l`.
    (unsafe { libc::getloadavg(l.as_mut_ptr(), 3) } == 3).then_some(l)
}

/// A path for a C call.
pub(crate) fn cpath(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes()).map_err(|_| io::Error::new(ErrorKind::InvalidInput, "a NUL in a path"))
}

/// The result of a C call that returns 0 on success and sets `errno` otherwise.
pub(crate) fn check(ret: libc::c_int) -> io::Result<()> {
    if ret == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

pub(crate) fn push(f: &File) -> io::Result<()> {
    // SAFETY: an fsync on a descriptor the caller owns.
    check(unsafe { libc::fsync(f.as_raw_fd()) })
}

/// Never `O_PATH` on Linux: `fsync` gives `EBADF` on such a descriptor. A folder that can't be
/// opened to sync it, and a volume that refuses a folder `fsync`, are skipped: nothing more can be
/// done there.
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    let f = match File::open(dir) {
        Err(e) if e.kind() == ErrorKind::PermissionDenied => return Ok(()),
        f => f?,
    };
    match push(&f) {
        Err(e) if e.raw_os_error().is_some_and(sync_refused) => Ok(()),
        done => done,
    }
}

/// What `fsync` of a folder says on a volume that has no folder sync (Linux's `vboxsf` and
/// `/proc`: `EINVAL`). Any other error, `EIO` above all, means the sync failed.
pub(crate) fn sync_refused(code: i32) -> bool {
    [libc::EINVAL, libc::ENOTSUP, libc::EOPNOTSUPP].contains(&code)
}

/// The change time in ns and the inode, for a stamp.
pub(crate) fn change_and_inode(m: &Metadata) -> (i128, u64) {
    (m.ctime() as i128 * 1_000_000_000 + m.ctime_nsec() as i128, m.ino())
}

pub(crate) fn file_id(p: &Path) -> io::Result<FileId> {
    Ok(id(&std::fs::metadata(p)?))
}

pub(crate) fn file_id_of(f: &File) -> io::Result<FileId> {
    Ok(id(&f.metadata()?))
}

fn id(m: &Metadata) -> FileId {
    FileId { volume: m.dev(), index: m.ino() }
}

pub(crate) fn make_writable(p: &Path) -> io::Result<()> {
    let mut perms = std::fs::metadata(p)?.permissions();
    if perms.mode() & 0o200 == 0 {
        perms.set_mode(perms.mode() | 0o200);
        std::fs::set_permissions(p, perms)?;
    }
    Ok(())
}

pub(crate) fn host_name() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most `buf.len()` bytes into `buf`.
    if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned()).filter(|n| !n.is_empty())
}

pub(crate) fn local_time(unix_secs: u64) -> Option<(u8, u8)> {
    let t = libc::time_t::try_from(unix_secs).ok()?;
    // SAFETY: localtime_r writes only into `tm`, which outlives the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return None;
    }
    Some((tm.tm_hour as u8, tm.tm_min as u8))
}

// ---------------------------------------------------------------- shared memory (`ipc.rs`)

fn shm_name(name: &str) -> io::Result<CString> {
    CString::new(name).map_err(|_| io::Error::new(ErrorKind::InvalidInput, "a NUL in a shared memory name"))
}

/// Map `len` bytes of the shared memory object `fd`, then close it: the mapping keeps it.
fn map_fd(fd: libc::c_int, len: usize) -> io::Result<*mut u8> {
    // SAFETY: maps an object this process opened read-write; `fd` is closed once, here.
    let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
    let err = io::Error::last_os_error();
    // SAFETY: closes the descriptor this function was given.
    unsafe { libc::close(fd) };
    if p == libc::MAP_FAILED { Err(err) } else { Ok(p as *mut u8) }
}

pub(crate) fn shm_create(name: &str, len: usize) -> io::Result<*mut u8> {
    let c = shm_name(name)?;
    let fd = crate::os::ipc::shm_open(&c, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL)?;
    // SAFETY: sizes the object just made (zero-filled); on an error it is closed and removed.
    if unsafe { libc::ftruncate(fd, len as libc::off_t) } != 0 {
        let err = io::Error::last_os_error();
        // SAFETY: the descriptor and the name made above.
        unsafe {
            libc::close(fd);
            libc::shm_unlink(c.as_ptr());
        }
        return Err(err);
    }
    map_fd(fd, len).inspect_err(|_| {
        // SAFETY: the name made above.
        unsafe { libc::shm_unlink(c.as_ptr()) };
    })
}

pub(crate) fn shm_open_mapped(name: &str, len: usize) -> io::Result<*mut u8> {
    let fd = crate::os::ipc::shm_open(&shm_name(name)?, libc::O_RDWR)?;
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `st` outlives the call, which fills it.
    if unsafe { libc::fstat(fd, &mut st) } != 0 || (st.st_size as u64) < len as u64 {
        // SAFETY: the descriptor opened above.
        unsafe { libc::close(fd) };
        return Err(io::Error::new(ErrorKind::InvalidData, "the shared memory is smaller than asked"));
    }
    map_fd(fd, len)
}

pub(crate) fn shm_remove(name: &str) -> io::Result<()> {
    let c = shm_name(name)?;
    // SAFETY: a NUL-terminated name that outlives the call.
    check(unsafe { libc::shm_unlink(c.as_ptr()) })
}

pub(crate) fn unmap(p: *mut u8, len: usize) {
    // SAFETY: a mapping `map_fd` made, of this length, unmapped once (its `Shm` drops).
    unsafe { libc::munmap(p as *mut libc::c_void, len) };
}

// ---------------------------------------------------------------- the process (`process.rs`)

pub(crate) fn take_stdout() -> io::Result<File> {
    use std::os::fd::FromRawFd;
    // SAFETY: duplicates standard output, then points it at standard error; the new descriptor
    // is owned by the `File` alone.
    unsafe {
        let fd = libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let out = File::from_raw_fd(fd);
        if libc::dup2(2, 1) < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(out)
    }
}

pub(crate) fn exit_now(code: i32) -> ! {
    // SAFETY: ends the process at once, running no exit handler.
    unsafe { libc::_exit(code) }
}

#[cfg(test)]
mod tests {
    use crate::process::memory;

    const MB: u64 = 1 << 20;

    // OS-82, what G7f measures: touched pages count in the footprint and in what is mapped; a
    // reservation no page backs, as wasmtime makes for each kernel's memory, counts only in what is
    // mapped. The other tests in this binary map and unmap memory beside it: in about 1 run of 10
    // here, what was mapped fell, by up to 149 MB, while this touched 64 MB. So each try reads just
    // before and after its own change, and one try of 10 must see it; a memory() that misses the
    // pages fails every try. Each try maps its own pages and unmaps them: a freed allocation might
    // stay with the allocator (macOS's malloc keeps large blocks), and a later try that reused it
    // would see nothing grow.
    #[test]
    fn the_footprint_counts_touched_pages_and_a_reservation_only_maps() {
        let first = memory().expect("the OS says what the process holds");
        assert!(first.footprint > 0 && first.mapped > 0, "{first:?}");
        assert!(first.address_space >= 1 << 39 && first.address_space > first.mapped, "{first:?}");
        let mut tries = vec![];
        while tries.len() < 10 {
            match one_try() {
                Ok(()) => return,
                Err(seen) => tries.push(seen),
            }
            // Apart, so the tries don't all fall while the other tests start and end.
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("no try of 10 saw it:\n{}", tries.join("\n"));
    }

    /// 64 MB mapped and touched, then 16 GiB reserved, each read just before and after.
    fn one_try() -> Result<(), String> {
        let len = (64 * MB) as usize;
        let before = memory().unwrap();
        let flags = libc::MAP_PRIVATE | libc::MAP_ANON;
        // SAFETY: a new private mapping, written only below, within it, and unmapped whole.
        let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, flags, -1, 0) };
        assert_ne!(p, libc::MAP_FAILED, "{}", std::io::Error::last_os_error());
        for i in (0..len).step_by(4096) {
            // SAFETY: inside the mapping made above.
            unsafe { p.cast::<u8>().add(i).write_volatile(1) };
        }
        let touched = memory().unwrap();
        // SAFETY: the mapping made above, whole.
        unsafe { libc::munmap(p, len) };

        let size = 16u64 << 30;
        let flags = libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_NORESERVE;
        let unreserved = memory().unwrap();
        // SAFETY: a new private mapping no page backs, unmapped below and never touched.
        let p = unsafe { libc::mmap(std::ptr::null_mut(), size as usize, libc::PROT_NONE, flags, -1, 0) };
        assert_ne!(p, libc::MAP_FAILED, "{}", std::io::Error::last_os_error());
        let reserved = memory().unwrap();
        // SAFETY: the mapping made above, whole.
        unsafe { libc::munmap(p, size as usize) };

        let seen = touched.footprint >= before.footprint + 32 * MB
            && touched.mapped >= before.mapped + 32 * MB
            && touched.address_space == before.address_space
            && reserved.mapped >= unreserved.mapped + size - 64 * MB
            && reserved.footprint < unreserved.footprint + 1024 * MB;
        if seen { Ok(()) } else { Err(format!("{before:?}, touched {touched:?}; {unreserved:?}, reserved {reserved:?}")) }
    }
}
