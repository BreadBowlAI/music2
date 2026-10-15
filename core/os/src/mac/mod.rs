//! macOS: thread identity, the disk, this machine, and the folders. The main loop and file-system
//! events are in `main_loop.rs` and `watch.rs`.

pub(crate) mod main_loop;
pub(crate) mod watch;

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::fs::Rename;
#[cfg(test)]
pub(crate) use crate::unix::sync_refused;
pub(crate) use crate::unix::{
    c_numbers, change_and_inode, cpu_time, file_id, file_id_of, host_name, local_time, make_writable, push, sync_dir, token,
};
use crate::unix::{check, cpath};

#[inline]
pub(crate) fn is_process_main() -> bool {
    // SAFETY: always safe to call.
    unsafe { libc::pthread_main_np() != 0 }
}

pub(crate) fn clap(home: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = home.map(|h| h.join("Library/Audio/Plug-Ins/CLAP")).into_iter().collect();
    v.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    v
}

pub(crate) fn cache(home: &Path) -> PathBuf {
    home.join("Library/Caches/music2")
}

/// `F_FULLFSYNC`, the drive's cache flushed. `fsync` alone leaves what it pushed in that cache.
pub(crate) fn barrier(f: &File) -> io::Result<()> {
    // SAFETY: an fcntl on a descriptor the caller owns.
    if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_FULLFSYNC) } == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        // A volume without F_FULLFSYNC (some network and FAT volumes): its strongest, fsync. Never
        // File::sync_all, which is F_FULLFSYNC again.
        Some(code) if barrier_refused(code) => push(f),
        _ => Err(e),
    }
}

/// The errors with which a volume refuses `F_FULLFSYNC` (which one it gives is unverified). Any
/// other, `EIO` above all, means the flush failed, and is never covered by an `fsync`.
pub(crate) fn barrier_refused(code: i32) -> bool {
    [libc::ENOTSUP, libc::EOPNOTSUPP, libc::ENOTTY, libc::EINVAL].contains(&code)
}

pub(crate) fn rename(from: &Path, to: &Path, how: Rename) -> io::Result<()> {
    let flags = match how {
        Rename::Swap => libc::RENAME_SWAP,
        Rename::Excl => libc::RENAME_EXCL,
    };
    let (a, b) = (cpath(from)?, cpath(to)?);
    // SAFETY: two NUL-terminated paths that outlive the call.
    check(unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), flags) })
}

/// What `renamex_np` says on a volume that can't swap or rename exclusively.
pub(crate) fn rename_unsupported(code: i32) -> bool {
    [libc::ENOTSUP, libc::EINVAL].contains(&code)
}

/// What taking a lock says on a volume that can't lock. `ENOTSUP` (45) and `EOPNOTSUPP` (102)
/// differ here.
pub(crate) fn lock_unsupported(code: i32) -> bool {
    [libc::ENOTSUP, libc::EOPNOTSUPP, libc::ENOLCK].contains(&code)
}

pub(crate) fn clone_file(from: &Path, to: &Path) -> io::Result<()> {
    let (a, b) = (cpath(from)?, cpath(to)?);
    // SAFETY: two NUL-terminated paths that outlive the call.
    check(unsafe { libc::clonefile(a.as_ptr(), b.as_ptr(), 0) })?;
    push(&File::open(to)?)
}

/// `MNT_LOCAL`: not a network volume.
pub(crate) fn is_local(dir: &Path) -> bool {
    let Ok(c) = cpath(dir) else { return true };
    // SAFETY: statfs fills `st`, which outlives the call.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return true;
    }
    st.f_flags & libc::MNT_LOCAL as u32 != 0
}

/// `getattrlist`'s name: how APFS or HFS+ stores it, whatever capitals or Unicode form `p` asks
/// with.
pub(crate) fn stored_name(p: &Path) -> Option<OsString> {
    let c = cpath(p).ok()?;
    // SAFETY: an attrlist asking for the name alone, and a buffer that holds any name (255
    // UTF-16 units, at most 3 bytes each in UTF-8) after its length and reference.
    let mut list: libc::attrlist = unsafe { std::mem::zeroed() };
    list.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    list.commonattr = libc::ATTR_CMN_NAME;
    #[repr(C, align(4))]
    struct Buf([u8; 1024]);
    let mut buf = Buf([0; 1024]);
    let got =
        unsafe { libc::getattrlist(c.as_ptr(), (&raw mut list).cast(), buf.0.as_mut_ptr().cast(), buf.0.len(), libc::FSOPT_NOFOLLOW) };
    if got != 0 {
        return None;
    }
    // The length, then the name's reference: its offset from the reference, and its length with
    // the NUL.
    let at = |i: usize| u32::from_ne_bytes([buf.0[i], buf.0[i + 1], buf.0[i + 2], buf.0[i + 3]]);
    let (offset, len) = (at(4) as i32, at(8) as usize);
    let start = 4 + offset as usize;
    let stored = buf.0.get(start..start + len.saturating_sub(1))?;
    Some(OsString::from_vec(stored.to_vec()))
}

/// The hardware UUID (`gethostuuid`).
pub(crate) fn machine_id() -> Option<String> {
    let mut id = [0u8; 16];
    let wait = libc::timespec { tv_sec: 1, tv_nsec: 0 };
    // SAFETY: gethostuuid writes 16 bytes into `id`.
    if unsafe { libc::gethostuuid(id.as_mut_ptr(), &wait) } != 0 {
        return None;
    }
    let hex: String = id.iter().map(|b| format!("{b:02X}")).collect();
    Some(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

#[cfg(test)]
mod tests {
    // §2.2 of the platform design: the barrier falls back to fsync only when F_FULLFSYNC is
    // refused; a failed flush (EIO) is returned. The rename and lock lists are tested in fs.rs.
    #[test]
    fn the_barrier_falls_back_only_when_the_volume_refuses() {
        use libc::{EEXIST, EINVAL, EIO, ENOLCK, ENOTSUP, ENOTTY, EOPNOTSUPP};
        for code in [ENOTSUP, EOPNOTSUPP, ENOTTY, EINVAL] {
            assert!(super::barrier_refused(code), "{code}");
        }
        for code in [ENOLCK, EEXIST, EIO] {
            assert!(!super::barrier_refused(code), "{code}");
        }
    }
}
