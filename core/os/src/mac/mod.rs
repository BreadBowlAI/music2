//! macOS: thread identity, the disk, this machine and this process, and the folders. The main loop
//! and file-system events are in `main_loop.rs` and `watch.rs`.

mod cf;
pub(crate) mod main_loop;
pub(crate) mod rt;
pub(crate) mod ipc;
pub(crate) mod wake;
pub(crate) mod watch;
pub(crate) mod window;

use std::ffi::{CStr, OsString};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::fs::Rename;
use crate::process::Memory;
#[cfg(test)]
pub(crate) use crate::unix::sync_refused;
pub(crate) use crate::unix::{
    c_numbers, change_and_inode, cpu_time, file_id, file_id_of, foreign, host_name, load_average, local_time, make_writable, push,
    sync_dir, exit_now, take_stdout, token,
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

/// CoreMIDI is always there to ask; a Mac with no keyboard lists none.
#[cfg(feature = "midi")]
pub(crate) fn midi_allowed() -> bool {
    true
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

/// The user-interactive QoS class, which also keeps the thread on a performance core.
pub(crate) fn interactive() -> io::Result<()> {
    // SAFETY: sets the calling thread's own QoS class.
    match unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) } {
        0 => Ok(()),
        e => Err(io::Error::from_raw_os_error(e)),
    }
}

/// The background QoS class: the thread runs on the efficiency cores, beside playback.
pub(crate) fn background() -> io::Result<()> {
    // SAFETY: sets the calling thread's own QoS class.
    match unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0) } {
        0 => Ok(()),
        e => Err(io::Error::from_raw_os_error(e)),
    }
}

/// P-cores and E-cores from `hw.perflevel0` and `hw.perflevel1`, and the P-cores sharing one L2;
/// a Mac with one kind of core has no perflevels: `hw.physicalcpu`, all in one cache.
pub(crate) fn cores() -> crate::process::Cores {
    let fast = sysctl_int(c"hw.perflevel0.physicalcpu").or_else(|| sysctl_int(c"hw.physicalcpu")).unwrap_or(1).max(1);
    let slow = sysctl_int(c"hw.perflevel1.physicalcpu").unwrap_or(0);
    let per_cache = sysctl_int(c"hw.perflevel0.cpusperl2").unwrap_or(fast).clamp(1, fast);
    // One system-level cache serves every cluster: a render alone spreads over them all.
    crate::process::Cores { fast, slow, per_cache, per_last_cache: fast }
}

/// macOS doesn't say how long threads waited for a CPU.
pub(crate) fn waited() -> Option<std::time::Duration> {
    None
}

/// `task_info(TASK_VM_INFO)`: the physical footprint, the address space in use, and the span the
/// task may map.
pub(crate) fn memory() -> Option<Memory> {
    // `task_vm_info` through rev2 (<mach/task_info.h>).
    #[repr(C)]
    #[derive(Default)]
    struct VmInfo {
        virtual_size: u64,
        region_count: i32,
        page_size: i32,
        sizes: [u64; 16],
        phys_footprint: u64,
        min_address: u64,
        max_address: u64,
    }
    const TASK_VM_INFO: u32 = 22;
    let mut info = VmInfo::default();
    let mut count = (std::mem::size_of::<VmInfo>() / 4) as u32;
    // SAFETY: `info` is a `task_vm_info` through rev2 and `count` its size in `natural_t`s.
    #[allow(deprecated)]
    let kr = unsafe { libc::task_info(libc::mach_task_self(), TASK_VM_INFO, &mut info as *mut VmInfo as libc::task_info_t, &mut count) };
    (kr == 0).then(|| Memory {
        footprint: info.phys_footprint,
        mapped: info.virtual_size,
        address_space: info.max_address.saturating_sub(info.min_address),
    })
}

/// `hw.model`, the macOS version, and the CPU's brand string.
pub(crate) fn machine() -> (String, String, String) {
    let model = sysctl(c"hw.model").unwrap_or_else(|| "unknown Mac".into());
    let system = sysctl(c"kern.osproductversion").map_or_else(|| "macOS".into(), |v| format!("macOS {v}"));
    let cpu = sysctl(c"machdep.cpu.brand_string").unwrap_or_else(|| "unknown CPU".into());
    (model, system, cpu)
}

/// A number `sysctlbyname` gives, as a count.
fn sysctl_int(name: &CStr) -> Option<usize> {
    let mut v: i32 = 0;
    let mut len = std::mem::size_of::<i32>();
    // SAFETY: sysctlbyname writes at most `len` bytes into `v`.
    let got = unsafe { libc::sysctlbyname(name.as_ptr(), (&raw mut v).cast(), &mut len, std::ptr::null_mut(), 0) };
    (got == 0 && len == std::mem::size_of::<i32>() && v >= 0).then_some(v as usize)
}

/// A string `sysctlbyname` gives.
fn sysctl(name: &CStr) -> Option<String> {
    let mut buf = [0u8; 256];
    let mut len = buf.len();
    // SAFETY: sysctlbyname writes at most `len` bytes into `buf` and sets `len` to what it wrote.
    if unsafe { libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) } != 0 {
        return None;
    }
    let got = &buf[..len.min(buf.len())];
    let end = got.iter().position(|b| *b == 0).unwrap_or(got.len());
    Some(String::from_utf8_lossy(&got[..end]).trim().to_string()).filter(|s| !s.is_empty())
}

pub(crate) fn no_core_dump() {
    let none = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `none` outlives the call.
    unsafe { libc::setrlimit(libc::RLIMIT_CORE, &none) };
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

    // OS-83: a thread gets the user-interactive QoS class.
    #[test]
    fn interactive_gives_the_thread_the_user_interactive_class() {
        std::thread::spawn(|| {
            crate::thread::interactive().unwrap();
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // SAFETY: reads the calling thread's own class into locals that outlive the call.
            assert_eq!(unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) }, 0);
            assert_eq!(class as u32, libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE as u32);
        })
        .join()
        .unwrap();
    }

    // 8a (§5.2): a background thread gets the background QoS class.
    #[test]
    fn a_background_thread_has_background_qos() {
        std::thread::spawn(|| {
            crate::thread::background().unwrap();
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // SAFETY: reads the calling thread's own class into locals that outlive the call.
            assert_eq!(unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) }, 0);
            assert_eq!(class as u32, libc::qos_class_t::QOS_CLASS_BACKGROUND as u32);
        })
        .join()
        .unwrap();
    }

    // 8a (§5.2): this Mac has P-cores, a cache holds at most all of them, and there are no more
    // cores than threads.
    #[test]
    fn cores_on_this_mac_are_sane() {
        let c = crate::process::cores();
        let threads = std::thread::available_parallelism().unwrap().get();
        assert!(c.fast >= 1 && (1..=c.fast).contains(&c.per_cache) && c.fast + c.slow <= threads, "{c:?}, {threads} threads");
        assert_eq!(c.per_last_cache, c.fast, "one system-level cache serves every cluster");
    }

    // OS-84: the model is `hw.model` ("MacBookPro18,1"), the OS has its version, the CPU its name.
    #[test]
    fn the_mac_is_named() {
        let (model, system, cpu) = super::machine();
        assert!(!model.is_empty() && !model.contains(' ') && model != "unknown Mac", "{model}");
        assert!(system.strip_prefix("macOS ").is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit())), "{system}");
        assert!(!cpu.is_empty() && cpu != "unknown CPU", "{cpu}");
    }
}
