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

/// Never `O_PATH` on Linux: `fsync` gives `EBADF` on such a descriptor.
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    push(&File::open(dir)?)
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
