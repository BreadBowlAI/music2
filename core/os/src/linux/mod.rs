//! Linux: thread identity, the disk, this machine and this process, and the folders. The main loop
//! and file-system events are in `main_loop.rs` and `watch.rs`.

mod cores;
pub(crate) mod ipc;
pub(crate) mod main_loop;
pub(crate) mod rt;
pub(crate) mod wake;
pub(crate) mod watch;
pub(crate) mod window;

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs::Rename;
use crate::process::Memory;
#[cfg(test)]
pub(crate) use crate::unix::sync_refused;
pub(crate) use crate::unix::{
    c_numbers, change_and_inode, cpu_time, exit_now, file_id, file_id_of, foreign, host_name, load_average, local_time, make_writable,
    push, read_at, sync_dir, take_stdout, token,
};
use crate::unix::{check, cpath};
pub(crate) use cores::cores;

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

/// Whether ALSA's sequencer, where MIDI keyboards are, may be opened. Opening it where it can't be
/// (no `snd-seq`, or no right to `/dev/snd/seq`) makes alsa-lib write to stderr, each try.
#[cfg(feature = "midi")]
pub(crate) fn midi_allowed() -> bool {
    // SAFETY: a NUL-terminated path that outlives the call.
    unsafe { libc::access(c"/dev/snd/seq".as_ptr(), libc::R_OK | libc::W_OK) == 0 }
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

/// File systems on another machine, by `statfs`'s type: NFS, SMB, CIFS, SMB2, 9p, Ceph, AFS (two
/// numbers: OpenAFS's `AFS_SUPER_MAGIC`, and the kernel's own kAFS, which reports `AFS_FS_MAGIC`)
/// and Coda; and FUSE, since sshfs and ntfs-3g look the same there (the safe side: walks and
/// `lock.local`). A type not on the list counts as local.
const NOT_LOCAL: [u32; 10] =
    [0x6969, 0x517B, 0xFF53_4D42, 0xFE53_4D42, 0x0102_1997, 0x00C3_6400, 0x5346_414F, 0x6B41_4653, 0x7375_7245, 0x6573_5546];

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

/// Linux has no per-program microphone switch: an input that opens can be read.
pub(crate) fn microphone_refused() -> bool {
    false
}

/// Linux has no class like macOS's QoS. A higher priority needs rights (`RLIMIT_NICE`,
/// `RLIMIT_RTPRIO` or rtkit) that this server doesn't grant and 8a's realtime threads will ask for.
pub(crate) fn interactive() -> io::Result<()> {
    Err(crate::unsupported("Linux has no QoS class: the thread keeps its priority"))
}

/// A nice of 10 for the calling thread (on Linux nice is per thread), or the one it has when that
/// is higher: raising nice is always allowed, lowering it needs rights.
pub(crate) fn background() -> io::Result<()> {
    // SAFETY: plain system calls on the calling thread's own id. getpriority may return -1 as a
    // nice, so errno is cleared first and read after.
    unsafe {
        let tid = libc::gettid() as libc::id_t;
        *libc::__errno_location() = 0;
        let nice = libc::getpriority(libc::PRIO_PROCESS, tid);
        if nice == -1 && *libc::__errno_location() != 0 {
            return Err(io::Error::last_os_error());
        }
        if nice >= 10 {
            return Ok(());
        }
        check(libc::setpriority(libc::PRIO_PROCESS, tid, 10))
    }
}

/// The second field of each thread's `/proc/self/task/<tid>/schedstat`: nanoseconds it waited on
/// a run queue.
pub(crate) fn waited() -> Option<std::time::Duration> {
    let mut ns = 0u64;
    for t in std::fs::read_dir("/proc/self/task").ok()?.flatten() {
        let Ok(text) = std::fs::read_to_string(t.path().join("schedstat")) else { continue };
        ns += text.split_whitespace().nth(1).and_then(|w| w.parse::<u64>().ok()).unwrap_or(0);
    }
    Some(std::time::Duration::from_nanos(ns))
}

/// `/proc/self/status` for the footprint and what is mapped; the CPU's virtual-address bits, or
/// `RLIMIT_AS` when lower, for the most the process can map.
pub(crate) fn memory() -> Option<Memory> {
    let (footprint, mapped) = status_memory(&std::fs::read_to_string("/proc/self/status").ok()?)?;
    let mut address_space = user_space(&std::fs::read_to_string("/proc/cpuinfo").ok()?)?;
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: getrlimit fills `limit`, which outlives the call.
    if unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut limit) } == 0 && limit.rlim_cur != libc::RLIM_INFINITY {
        address_space = address_space.min(limit.rlim_cur);
    }
    Some(Memory { footprint, mapped, address_space })
}

/// `RssAnon` + `VmSwap` (what the process alone is charged for, as macOS's footprint counts it)
/// and `VmSize` (every mapping, reservations no page backs included), in bytes.
fn status_memory(status: &str) -> Option<(u64, u64)> {
    let kb = |key: &str| {
        let value = status.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?;
        Some(value.trim().strip_suffix("kB")?.trim().parse::<u64>().ok()? * 1024)
    };
    Some((kb("RssAnon")? + kb("VmSwap")?, kb("VmSize")?))
}

/// The user half of the CPU's virtual addresses, from `/proc/cpuinfo`'s "address sizes" (x86):
/// 48 bits give 128 TiB. A CPU with 57 (five-level paging) still maps below 128 TiB unless asked
/// for an address above it, which wasmtime never does. Other CPUs don't say: None.
fn user_space(cpuinfo: &str) -> Option<u64> {
    let bits: u32 = cpuinfo.lines().find_map(|l| {
        let (key, value) = l.split_once(':')?;
        if key.trim() != "address sizes" {
            return None;
        }
        value.split(',').find_map(|p| p.trim().strip_suffix("bits virtual")?.trim().parse().ok())
    })?;
    (32..=64).contains(&bits).then(|| 1 << (bits.min(48) - 1))
}

/// The model from DMI (the product, else the board) or the device tree; the OS from
/// `/etc/os-release`; the CPU's name from `/proc/cpuinfo`.
pub(crate) fn machine() -> (String, String, String) {
    let read = |f: &str| {
        let text = std::fs::read_to_string(f).ok()?;
        Some(text.trim_matches(|c: char| c.is_whitespace() || c == '\0').to_string()).filter(|t| !t.is_empty())
    };
    let dmi = |vendor: &str, name: &str| {
        let name = read(&format!("/sys/class/dmi/id/{name}"))?;
        Some(match read(&format!("/sys/class/dmi/id/{vendor}")) {
            Some(v) if !name.starts_with(&v) => format!("{v} {name}"),
            _ => name,
        })
    };
    let model = dmi("sys_vendor", "product_name")
        .or_else(|| dmi("board_vendor", "board_name"))
        .or_else(|| read("/proc/device-tree/model"))
        .unwrap_or_else(|| "unknown model".into());
    let system = read("/etc/os-release").and_then(|t| os_name(&t)).unwrap_or_else(|| "Linux".into());
    let cpu = read("/proc/cpuinfo").and_then(|t| cpu_name(&t)).unwrap_or_else(|| "unknown CPU".into());
    (model, system, cpu)
}

/// `PRETTY_NAME` in `/etc/os-release`, its quotes taken off.
fn os_name(release: &str) -> Option<String> {
    let v = release.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?.trim().trim_matches(['"', '\'']).trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// The first "model name" in `/proc/cpuinfo`.
fn cpu_name(cpuinfo: &str) -> Option<String> {
    cpuinfo.lines().find_map(|l| {
        let (key, value) = l.split_once(':')?;
        (key.trim() == "model name" && !value.trim().is_empty()).then(|| value.trim().to_string())
    })
}

pub(crate) fn no_core_dump() {
    // SAFETY: a plain prctl on this process.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
}

#[cfg(test)]
mod tests {
    // OS-15, WRITE-17: the network file systems are not local, by the numbers libc names, and the
    // kernel's own AFS too, whose number libc doesn't name (`AFS_FS_MAGIC` in linux/magic.h; this
    // kernel's kafs.ko holds it, never OpenAFS's `AFS_SUPER_MAGIC`). Local ones stay local.
    #[test]
    fn network_file_systems_are_not_local() {
        use libc::{AFS_SUPER_MAGIC, CODA_SUPER_MAGIC, FUSE_SUPER_MAGIC, NFS_SUPER_MAGIC, SMB_SUPER_MAGIC};
        use libc::{BTRFS_SUPER_MAGIC, EXT4_SUPER_MAGIC, OVERLAYFS_SUPER_MAGIC, TMPFS_MAGIC, XFS_SUPER_MAGIC};
        for t in [NFS_SUPER_MAGIC, SMB_SUPER_MAGIC, AFS_SUPER_MAGIC, CODA_SUPER_MAGIC, FUSE_SUPER_MAGIC] {
            assert!(super::NOT_LOCAL.contains(&(t as u32)), "{t:#x}");
        }
        assert!(super::NOT_LOCAL.contains(&0x6B41_4653), "kAFS");
        for t in [EXT4_SUPER_MAGIC, TMPFS_MAGIC, BTRFS_SUPER_MAGIC, XFS_SUPER_MAGIC, OVERLAYFS_SUPER_MAGIC] {
            assert!(!super::NOT_LOCAL.contains(&(t as u32)), "{t:#x}");
        }
    }

    // OS-82: the footprint is the anonymous memory and the swap, what is mapped is `VmSize`, in
    // bytes; without them, nothing.
    #[test]
    fn status_gives_the_footprint_and_what_is_mapped() {
        let status =
            "Name:\tm2\nVmSize:\t 1313576 kB\nVmRSS:\t    7284 kB\nRssAnon:\t    1356 kB\nRssFile:\t    5928 kB\nVmSwap:\t      12 kB\n";
        assert_eq!(super::status_memory(status), Some(((1356 + 12) * 1024, 1_313_576 * 1024)));
        assert_eq!(super::status_memory("VmSize:\t 100 kB\nVmSwap:\t 0 kB\n"), None, "no RssAnon");
    }

    // OS-82: the process may map the user half of the CPU's virtual addresses, and no more than
    // 128 TiB without asking for an address above it.
    #[test]
    fn the_address_space_is_the_user_half_of_the_cpus() {
        let cpu = |bits: &str| format!("processor\t: 0\naddress sizes\t: 48 bits physical, {bits} bits virtual\npower management:\n");
        assert_eq!(super::user_space(&cpu("48")), Some(1 << 47));
        assert_eq!(super::user_space(&cpu("57")), Some(1 << 47));
        assert_eq!(super::user_space(&cpu("39")), Some(1 << 38));
        assert_eq!(super::user_space("processor\t: 0\nBogoMIPS\t: 50.00\n"), None, "an arm64 cpuinfo");
        let m = crate::process::memory().expect("Linux says");
        assert!(m.address_space <= 1 << 47, "{m:?}");
    }

    // OS-84: the OS and the CPU are named as the system names them.
    #[test]
    fn the_os_and_the_cpu_are_named() {
        assert_eq!(super::os_name("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.3 LTS\"\n").as_deref(), Some("Ubuntu 24.04.3 LTS"));
        assert_eq!(super::os_name("NAME=Arch\n"), None);
        let cpuinfo = "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD Ryzen 9 7950X3D 16-Core Processor\n";
        assert_eq!(super::cpu_name(cpuinfo).as_deref(), Some("AMD Ryzen 9 7950X3D 16-Core Processor"));
        let (model, system, cpu) = super::machine();
        assert!(![&model, &system, &cpu].iter().any(|p| p.is_empty()), "{model}, {system}, {cpu}");
    }

    // 8a (§5.2): a background thread runs at nice 10 or more, and its nice never goes down (a
    // test run under `nice -n 15` keeps 15).
    #[test]
    fn a_background_thread_is_nice_10() {
        let (before, after) = std::thread::spawn(|| {
            // SAFETY: reads the calling thread's own nice.
            let nice = || unsafe { libc::getpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t) };
            let before = nice();
            crate::thread::background().unwrap();
            (before, nice())
        })
        .join()
        .unwrap();
        assert!(after >= 10 && after >= before, "nice {before} → {after}");
    }

    // 8a: the threads' waits for a CPU are read, and only grow.
    #[test]
    fn waits_for_a_cpu_are_read() {
        let a = crate::process::waited().expect("Linux says");
        let b = crate::process::waited().expect("Linux says");
        assert!(b >= a || a - b < std::time::Duration::from_millis(10), "{a:?} then {b:?}: a thread may have ended between");
    }

    // OS-83: Linux gives a thread no QoS class, and says so; nothing fails.
    #[test]
    fn interactive_says_linux_has_no_qos_class() {
        let e = std::thread::spawn(crate::thread::interactive).join().unwrap().unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::Unsupported, "{e}");
    }
}
