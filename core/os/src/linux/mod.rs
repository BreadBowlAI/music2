//! Linux: thread identity, the disk, this machine, and the folders. The main loop is in
//! `main_loop.rs`.

pub(crate) mod main_loop;

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs::Rename;
pub(crate) use crate::unix::{
    change_and_inode, cpu_time, file_id, file_id_of, host_name, local_time, make_writable, push, sync_dir, token,
};
use crate::unix::{check, cpath};

/// The main thread's id is the process id: two system calls, no allocation, no lock.
#[inline]
pub(crate) fn is_process_main() -> bool {
    // SAFETY: plain system calls.
    unsafe { libc::gettid() == libc::getpid() }
}

pub(crate) fn clap(home: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = home.map(|h| h.join(".clap")).into_iter().collect();
    v.push(PathBuf::from("/usr/lib/clap"));
    v
}

/// From `HOME` alone, never `XDG_CACHE_HOME`, so a test's scratch `HOME` holds it (PLAT-7).
pub(crate) fn cache(home: &Path) -> PathBuf {
    home.join(".cache/music2")
}

/// `fsync` also flushes the drive's cache here, so each push is durable alone.
pub(crate) fn barrier(f: &File) -> io::Result<()> {
    push(f)
}

/// `renameat2` with `RENAME_EXCHANGE` or `RENAME_NOREPLACE` (ext4, tmpfs, btrfs, XFS; not NFS,
/// 9p, exFAT or SMB).
pub(crate) fn rename(from: &Path, to: &Path, how: Rename) -> io::Result<()> {
    let flags = match how {
        Rename::Swap => libc::RENAME_EXCHANGE,
        Rename::Excl => libc::RENAME_NOREPLACE,
    };
    let (a, b) = (cpath(from)?, cpath(to)?);
    // SAFETY: two NUL-terminated paths that outlive the call.
    check(unsafe { libc::renameat2(libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), flags) })
}

/// What `renameat2` says on a volume or kernel that can't swap or rename exclusively.
pub(crate) fn rename_unsupported(code: i32) -> bool {
    [libc::EINVAL, libc::ENOSYS, libc::EOPNOTSUPP].contains(&code)
}

/// What taking a lock says on a volume that can't lock: `EOPNOTSUPP` (the same number as
/// `ENOTSUP` here), or `ENOLCK` (NFS without lockd).
pub(crate) fn lock_unsupported(code: i32) -> bool {
    [libc::EOPNOTSUPP, libc::ENOLCK].contains(&code)
}

/// No clone yet: ext4 has none, and `FICLONE` for btrfs and XFS waits for a volume to test it on.
pub(crate) fn clone_file(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(crate::unsupported("no file clones on Linux yet"))
}

/// File systems on another machine, by `statfs`'s type: NFS, SMB, CIFS, SMB2, 9p, Ceph, AFS and
/// Coda; and FUSE, since sshfs and ntfs-3g look the same there (the safe side: walks and
/// `lock.local`). A type not on the list counts as local.
const NOT_LOCAL: [u32; 9] = [0x6969, 0x517B, 0xFF53_4D42, 0xFE53_4D42, 0x0102_1997, 0x00C3_6400, 0x5346_414F, 0x7375_7245, 0x6573_5546];

pub(crate) fn is_local(dir: &Path) -> bool {
    let Ok(c) = cpath(dir) else { return true };
    // SAFETY: statfs fills `st`, which outlives the call.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return true;
    }
    // The kernel's type is a word whose top half is 0 on 64-bit: the low 32 bits are the magic.
    !NOT_LOCAL.contains(&(st.f_type as u32))
}

/// ext4 stores names byte for byte, as asked. On a volume that folds capitals (vfat, exFAT, a
/// casefold folder) listing the parent would tell: deferred until such a volume can be tested.
pub(crate) fn stored_name(_p: &Path) -> Option<OsString> {
    None
}

/// `/etc/machine-id`, else D-Bus's copy.
pub(crate) fn machine_id() -> Option<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .map(|id| id.trim().to_string())
        .find(|id| !id.is_empty())
}
