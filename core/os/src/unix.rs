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

#[cfg(test)]
mod tests {
    use crate::process::memory;

    const MB: u64 = 1 << 20;

    // OS-82, what G7f measures: touched pages count in the footprint and in what is mapped; a
    // reservation no page backs, as wasmtime makes for each kernel's memory, counts only in what is
    // mapped. The other tests in this binary map and unmap memory beside it: in about 1 run of 10
    // here, what was mapped fell, by up to 149 MB, while this touched 64 MB. So each try reads just
    // before and after its own change, and one try of 10 must see it; a memory() that misses the
    // pages fails every try.
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

    /// 64 MB touched, then 16 GiB reserved, each read just before and after.
    fn one_try() -> Result<(), String> {
        let before = memory().unwrap();
        let mut v = vec![0u8; (64 * MB) as usize];
        for i in (0..v.len()).step_by(4096) {
            v[i] = 1;
        }
        std::hint::black_box(&v);
        let touched = memory().unwrap();
        drop(v);

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
