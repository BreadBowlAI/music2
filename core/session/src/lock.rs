//! The project lock (WRITE-17, WRITE-18): one process at a time writes a project through
//! music2. It is an exclusive OS advisory lock on `.music2/lock`, held for as long as the process
//! may write: the app while the project is open, a CLI write command for its run. The OS lets go
//! of it when the process exits, so a crash leaves no stale lock. The file says who holds it,
//! only to make messages clear. A process that can't take it opens the project read-only: it
//! plays and reads, and refuses every write with a message naming the holder. Forwarding writes
//! to the holder is deferred (owner decision 10).
//!
//! `flock` locks belong to the open file, so a second session of one project in one process
//! finds the lock held too, and is read-only (the step 6 design's C10).

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m2_song::path::LOCAL;
use serde::{Deserialize, Serialize};

/// How long a writer waits for a lock that is held, retrying, before it opens read-only: long
/// enough for a reader's journal scan, which holds it for a few milliseconds (C19).
pub const RETRY: Duration = Duration::from_millis(250);

/// Who holds a project's lock, as its file says: for messages only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Holder {
    pub pid: u32,
    pub program: String,
    /// The host name, for messages: it can change with the network.
    pub host: String,
    /// The machine, by its hardware UUID (`gethostuuid`): what tells this machine's journals
    /// from another's (FMT-20, the step 6 design's C18).
    pub host_id: String,
    pub token: String,
    /// When it was taken, in seconds since the epoch.
    pub since: u64,
}

/// The lock, held: dropped, the OS lets go of it.
#[derive(Debug)]
pub struct ProjectLock {
    _file: File,
    pub holder: Holder,
    /// Notices about the lock itself: `lock.local` on a network volume.
    pub notices: Vec<m2_song::Diagnostic>,
}

/// Why a session is read-only.
#[derive(Clone, Debug, PartialEq)]
pub enum ReadOnly {
    /// Another process holds the lock (or another session of this one); who, if its file says.
    Held(Option<Holder>),
    /// The volume can't lock, or `.music2/` can't be made, or it is on another volume than the
    /// project: the system's error, in words.
    Unsupported(String),
    /// Opened read-only on purpose: a reading command, or a session that gave its lock up.
    Asked,
}

/// What a session may do to its project's files.
#[derive(Debug)]
pub enum Access {
    Writer(ProjectLock),
    ReadOnly(ReadOnly),
}

impl Access {
    pub fn writer(&self) -> bool {
        matches!(self, Access::Writer(_))
    }

    /// Why a write is refused here, if it is: `lock.held` or `lock.unsupported`, said as WRITE-18
    /// says it. None for a writer.
    pub fn refusal(&self) -> Option<m2_song::Diagnostic> {
        match self {
            Access::Writer(_) => None,
            Access::ReadOnly(ReadOnly::Held(holder)) => Some(m2_song::Diagnostic::error("lock.held", held(holder.as_ref()))),
            Access::ReadOnly(ReadOnly::Unsupported(why)) => Some(m2_song::Diagnostic::error(
                "lock.unsupported",
                format!("nothing was changed: this project can't be locked ({why}), so music2 opens it read-only and never writes it"),
            )),
            Access::ReadOnly(ReadOnly::Asked) => {
                Some(m2_song::Diagnostic::error("lock.held", "nothing was changed: this project was opened read-only"))
            }
        }
    }
}

/// WRITE-18's message for a lock someone else holds.
pub fn held(holder: Option<&Holder>) -> String {
    match holder {
        Some(h) => format!(
            "nothing was changed: this project is open for writing in {} (pid {}, since {}).\nMake the edit there, or close the project in \
             {} and run this again.",
            h.program,
            h.pid,
            clock(h.since),
            h.program
        ),
        None => "nothing was changed: this project is open for writing in another program.\nMake the edit there, or close it and run this \
                 again."
            .into(),
    }
}

/// `14:02`, local time.
fn clock(unix: u64) -> String {
    #[cfg(unix)]
    {
        let t = unix as libc::time_t;
        // SAFETY: localtime_r writes only into `tm`, which outlives the call.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
        }
    }
    format!("{unix} s")
}

/// This machine's identity: its hardware UUID on macOS, `/etc/machine-id` elsewhere, else its
/// host name.
pub fn host_id() -> String {
    #[cfg(target_os = "macos")]
    {
        let mut id = [0u8; 16];
        let wait = libc::timespec { tv_sec: 1, tv_nsec: 0 };
        // SAFETY: gethostuuid writes 16 bytes into `id`.
        if unsafe { libc::gethostuuid(id.as_mut_ptr(), &wait) } == 0 {
            let hex: String = id.iter().map(|b| format!("{b:02X}")).collect();
            return format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]);
        }
    }
    if let Ok(id) = std::fs::read_to_string("/etc/machine-id")
        && !id.trim().is_empty()
    {
        return id.trim().to_string();
    }
    host_name()
}

/// The host name, for messages.
pub fn host_name() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: gethostname writes at most `buf.len()` bytes into `buf`.
        if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0 {
            let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            return String::from_utf8_lossy(&buf[..end]).into_owned();
        }
    }
    "this computer".into()
}

/// The lock file of the project at `dir`.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCAL).join("lock")
}

/// Faults a test can inject into the locks taken on its thread.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Fault {
    /// The volume can't lock (`ENOTSUP`).
    NoLock,
    /// `.music2/tx/` is on another volume than the project.
    OtherVolume,
    /// A network volume (no `MNT_LOCAL`).
    NotLocal,
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAULT: std::cell::Cell<Option<Fault>> = const { std::cell::Cell::new(None) };
}

fn fault(f: Fault) -> bool {
    #[cfg(test)]
    return FAULT.with(|x| x.get() == Some(f));
    #[cfg(not(test))]
    {
        let _ = f;
        false
    }
}

/// Make `.music2/` (with a `.gitignore` of its own, so git never lists it, FMT-20) and
/// `.music2/tx/`, which must be on the project's volume for a commit to rename into place.
fn prepare(dir: &Path) -> Result<(), String> {
    let local = dir.join(LOCAL);
    std::fs::create_dir_all(local.join("tx")).map_err(|e| format!("{} can't be made: {e}", local.display()))?;
    match OpenOptions::new().write(true).create_new(true).open(local.join(".gitignore")) {
        Ok(mut f) => {
            let _ = f.write_all(b"*\n");
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(_) => {}
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let dev = |p: &Path| std::fs::metadata(p).map(|m| m.dev());
        let other = fault(Fault::OtherVolume) || matches!((dev(dir), dev(&local.join("tx"))), (Ok(a), Ok(b)) if a != b);
        if other {
            return Err(format!("{} is on another volume than the project, so a commit couldn't rename into place", local.display()));
        }
    }
    Ok(())
}

/// Whether the volume holding `dir` is local to this machine (`MNT_LOCAL`): on a network volume
/// the lock stops only this machine's programs, and file system events report only its writes.
pub fn local_volume(dir: &Path) -> bool {
    if fault(Fault::NotLocal) {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return true };
        // SAFETY: statfs fills `st`, which outlives the call.
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(c.as_ptr(), &mut st) } == 0 {
            return st.f_flags & libc::MNT_LOCAL as u32 != 0;
        }
    }
    true
}

/// Whether an error from taking a lock means the volume can't lock at all.
fn cannot_lock(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    if let Some(code) = e.raw_os_error() {
        return code == libc::ENOTSUP || code == libc::EOPNOTSUPP || code == libc::ENOLCK;
    }
    e.kind() == ErrorKind::Unsupported
}

/// Whether `file` is still the file at `path`: the same inode on the same volume. A lock held on
/// a file that was removed (`git clean -fdx`, a `.music2/` deleted by hand) excludes nobody, since
/// the next opener makes a new one.
fn is_at(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (file.metadata(), std::fs::metadata(path)) {
            (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (file, path);
        true
    }
}

impl ProjectLock {
    /// Whether the lock still excludes other writers of the project at `dir`: its file is still
    /// the one at `.music2/lock` (WRITE-17).
    pub fn holds(&self, dir: &Path) -> bool {
        is_at(&self._file, &lock_path(dir))
    }
}

/// Take the lock of the project at `dir` for `program`, retrying a held lock for `retry`. On
/// success the file says who holds it; otherwise why the project is read-only.
pub fn acquire(dir: &Path, program: &str, retry: Duration) -> Result<ProjectLock, ReadOnly> {
    let path = lock_path(dir);
    let t = Instant::now();
    let mut file = loop {
        prepare(dir).map_err(ReadOnly::Unsupported)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| ReadOnly::Unsupported(format!("{} can't be opened: {e}", path.display())))?;
        if fault(Fault::NoLock) {
            return Err(ReadOnly::Unsupported(format!("{} can't be locked: Operation not supported", path.display())));
        }
        match file.try_lock() {
            // Locked, and still the file at the path: removed between the open and the lock, it
            // would exclude nobody, so it is opened again.
            Ok(()) if is_at(&file, &path) => break file,
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) if t.elapsed() < retry => std::thread::sleep(Duration::from_millis(25)),
            Err(std::fs::TryLockError::WouldBlock) => return Err(ReadOnly::Held(read_holder(&path))),
            Err(std::fs::TryLockError::Error(e)) if cannot_lock(&e) => {
                return Err(ReadOnly::Unsupported(format!("{} can't be locked: {e}", path.display())));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(ReadOnly::Unsupported(format!("{} can't be locked: {e}", path.display()))),
        }
        if t.elapsed() > retry + Duration::from_secs(1) {
            return Err(ReadOnly::Unsupported(format!("{} keeps being removed while it is locked", path.display())));
        }
    };
    let since = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let token = format!("{:016x}", {
        use std::hash::{BuildHasher, Hasher};
        std::collections::hash_map::RandomState::new().build_hasher().finish()
    });
    let holder = Holder { pid: std::process::id(), program: program.to_string(), host: host_name(), host_id: host_id(), token, since };
    // Who holds it, for messages: written only once it is ours.
    let _ = file.set_len(0).and_then(|_| file.write_all(serde_json::to_string(&holder).expect("serialises").as_bytes()));
    let mut notices = vec![];
    if !local_volume(dir) {
        notices.push(m2_song::Diagnostic::notice(
            "lock.local",
            "This folder is on a network volume: the project lock stops other programs on this Mac only, so another Mac editing it \
             is an outside writer",
        ));
    }
    Ok(ProjectLock { _file: file, holder, notices })
}

/// The lock of the project at `dir`, taken without waiting, for a reader's journal scan (6a.4 of
/// the step 6 design): only when `.music2/tx/` exists, so a reader never makes `.music2/`.
pub fn probe(dir: &Path) -> Option<File> {
    if !dir.join(LOCAL).join("tx").is_dir() {
        return None;
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_path(dir)).ok()?;
    file.try_lock().ok()?;
    is_at(&file, &lock_path(dir)).then_some(file)
}

/// Who holds the lock at `path`, if its file says.
pub fn read_holder(path: &Path) -> Option<Holder> {
    let mut text = String::new();
    File::open(path).ok()?.read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("m2-lock-{name}-{}-{n}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // WRITE-17: one writer at a time; the file says who; a second open of it, even in this
    // process, finds it held (C10); dropped, it is free again.
    #[test]
    fn one_writer_at_a_time() {
        let d = dir("one");
        let lock = acquire(&d, "music2", RETRY).unwrap();
        assert_eq!(lock.holder.pid, std::process::id());
        assert_eq!(read_holder(&lock_path(&d)).unwrap(), lock.holder);
        assert_eq!(std::fs::read_to_string(d.join(".music2/.gitignore")).unwrap(), "*\n", "git never lists .music2/");
        let t = Instant::now();
        match acquire(&d, "m2", RETRY) {
            Err(ReadOnly::Held(Some(h))) => assert_eq!((h.program.as_str(), h.pid), ("music2", std::process::id())),
            other => panic!("{other:?}"),
        }
        assert!(t.elapsed() >= RETRY, "a writer retries a held lock first");
        assert!(probe(&d).is_none(), "a reader's scan doesn't take a held lock");
        let message = Access::ReadOnly(ReadOnly::Held(Some(lock.holder.clone()))).refusal().unwrap();
        assert_eq!(message.code, "lock.held");
        assert!(message.message.starts_with("nothing was changed: this project is open for writing in music2 (pid "), "{message}");
        drop(lock);
        assert!(probe(&d).is_some(), "let go when dropped");
        assert!(acquire(&d, "m2", Duration::ZERO).is_ok());
        std::fs::remove_dir_all(d).ok();
    }

    // WRITE-17: a volume that can't lock, or a `.music2/tx/` on another volume, is read-only with
    // `lock.unsupported`; a network volume is written with the Notice `lock.local`.
    #[test]
    fn volumes_that_cant_lock() {
        let d = dir("faults");
        for (f, says) in [(Fault::NoLock, "Operation not supported"), (Fault::OtherVolume, "on another volume")] {
            FAULT.with(|x| x.set(Some(f)));
            match acquire(&d, "music2", RETRY) {
                Err(ReadOnly::Unsupported(why)) => {
                    assert!(why.contains(says), "{why}");
                    let r = Access::ReadOnly(ReadOnly::Unsupported(why)).refusal().unwrap();
                    assert_eq!(r.code, "lock.unsupported");
                }
                other => panic!("{f:?}: {other:?}"),
            }
        }
        FAULT.with(|x| x.set(Some(Fault::NotLocal)));
        let lock = acquire(&d, "music2", RETRY).unwrap();
        assert_eq!(lock.notices.iter().map(|n| n.code).collect::<Vec<_>>(), ["lock.local"]);
        FAULT.with(|x| x.set(None));
        drop(lock);
        assert!(acquire(&d, "music2", RETRY).unwrap().notices.is_empty());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn this_machine_has_an_identity() {
        let id = host_id();
        assert!(!id.is_empty());
        assert_eq!(id, host_id(), "stable");
        assert!(!host_name().is_empty());
    }
}
