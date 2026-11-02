//! What a durable commit needs from the disk (WRITE-15, PLANE-5), and what tells files apart.
//!
//! Each call does what the OS does, with the strongest call the volume accepts, or returns
//! `Unsupported` (PLAT-3). The fallbacks stay with the caller: a copy when a clone can't be made,
//! "look, then rename" where a volume can't rename exclusively, compare-then-rename for a swap.
//!
//! macOS and Linux number their errors differently (`ENOTSUP` is 45 and `EOPNOTSUPP` 102 on
//! macOS, both 95 on Linux), and std's `ErrorKind::Unsupported` covers only `ENOSYS` and
//! `EOPNOTSUPP`, so it misses macOS's 45. So each OS keeps its own lists of the errors that mean
//! "this volume can't".

use std::ffi::OsString;
use std::fs::{File, Metadata};
use std::io::{self, ErrorKind};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::os;

/// How [`rename`] treats its destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rename {
    /// Exchange the two names: both must exist.
    Swap,
    /// Only where nothing is: `AlreadyExists` onto a taken name.
    Excl,
}

/// What a path names, whatever it is called: its volume and its index there (`st_dev` and
/// `st_ino` on Unix).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId {
    pub(crate) volume: u64,
    pub(crate) index: u64,
}

/// What identifies a file's version without reading it (FMT-16, WRITE-19): one type for the
/// watcher and the hash cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Stamp {
    pub size: u64,
    /// Nanoseconds since the epoch.
    pub mtime: i128,
    pub ctime: i128,
    pub inode: u64,
}

impl Stamp {
    /// From metadata the caller already has. Unix: the change time in ns and the inode.
    pub fn of(m: &Metadata) -> Stamp {
        let (ctime, inode) = os::change_and_inode(m);
        Stamp { size: m.len(), mtime: ns(m.modified()), ctime, inode }
    }

    /// The stamp of what is at `p`, links not followed; None when nothing is.
    pub fn at(p: &Path) -> Option<Stamp> {
        std::fs::symlink_metadata(p).ok().map(|m| Stamp::of(&m))
    }
}

/// A time in nanoseconds since the epoch; 0 when the volume has none.
pub(crate) fn ns(t: io::Result<SystemTime>) -> i128 {
    t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as i128)
}

/// Push a file's bytes, or a folder's entries, towards the drive: `fsync`. Durable alone on Linux;
/// on macOS durable once a [`barrier`] follows, so a commit flushes the drive's cache once. Never
/// `File::sync_all`, which on macOS is `F_FULLFSYNC`, a flush each time.
pub fn push(f: &File) -> io::Result<()> {
    os::push(f)
}

/// Make everything pushed so far durable: a commit's one barrier. macOS: `F_FULLFSYNC`, or
/// `fsync` when the volume refuses it; any other error, such as `EIO`, is returned as it is, so a
/// failed flush is never reported as done (PLANE-5). Linux: `fsync`.
pub fn barrier(f: &File) -> io::Result<()> {
    os::barrier(f)
}

/// Make a folder's new entries durable once a barrier follows: `fsync` of the folder opened
/// read-only. Ok where the OS keeps new entries durable by itself, and where it can't be done: a
/// folder that can be written but not listed can't be opened to sync it, and some volumes have no
/// folder sync (`vboxsf`). Every writer would skip those, so the rule is here once (WRITE-1,
/// WRITE-15). Any other error, `EIO` above all, is returned.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    os::sync_dir(dir)
}

/// Make `folder`, with every folder above it that isn't there, each one made synced in the one
/// above it (WRITE-1): a file then made in `folder`, pushed and its folder synced, is durable with
/// every folder on its way by the next barrier.
pub fn make_dirs(folder: &Path) -> io::Result<()> {
    let made: Vec<&Path> = folder.ancestors().take_while(|f| !f.as_os_str().is_empty() && !f.exists()).collect();
    std::fs::create_dir_all(folder)?;
    for new in made.iter().rev() {
        sync_dir(folder_of(new))?;
    }
    Ok(())
}

/// Write `bytes` at `to` whole: to a temporary name beside it (`.<name>.<pid>.<n>.part`), pushed,
/// then renamed over it, so a reader, or a crash, finds the old file or the new one, never half.
/// What `.music2/` keeps this way: the undo history, unsaved code, the view settings.
pub fn replace(to: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let name = to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = to.with_file_name(format!(".{name}.{}.{}.part", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let done = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        push(&f)?;
        std::fs::rename(&tmp, to)
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// The folder `p` is in: `.` for a bare name.
pub fn folder_of(p: &Path) -> &Path {
    p.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."))
}

/// Read into `buf` from byte `offset` of `f`, without moving its position: `pread` on Unix (and
/// `seek_read` on Windows later). Returns the bytes read, fewer at the file's end. The one way
/// music2 reads audio as it plays (RENDER-30): a disk thread's, or an offline render's own
/// thread's, never the sound thread's.
pub fn read_at(f: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    if let Some(watch) = READS.get() {
        watch(f, buf.len())?;
    }
    os::read_at(f, buf, offset)
}

/// [`read_at`] until `buf` is full or the file ends, in reads of at most `piece` bytes. Returns
/// the bytes read.
pub fn read_full_at(f: &File, buf: &mut [u8], offset: u64, piece: usize) -> io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        let end = (done + piece.max(1)).min(buf.len());
        match read_at(f, &mut buf[done..end], offset + done as u64) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

/// A test's watcher of reads ([`watch_reads`]).
pub type Watch = fn(&File, usize) -> io::Result<()>;

static READS: std::sync::OnceLock<Watch> = std::sync::OnceLock::new();

/// Call `f` with each [`read_at`]'s file and size, on the reading thread, before it reads: tests
/// see who reads, and how much, and slow the disk down. An error from `f` is the read's, as from
/// a drive unplugged mid-read. Set once per process.
#[doc(hidden)]
pub fn watch_reads(f: Watch) {
    let _ = READS.set(f);
}

/// Swap two existing names, or rename only where nothing is (macOS `renamex_np`, Linux
/// `renameat2`). `Unsupported` when the volume can't; `AlreadyExists` for [`Rename::Excl`] onto a
/// taken name.
pub fn rename(from: &Path, to: &Path, how: Rename) -> io::Result<()> {
    os::rename(from, to, how).map_err(|e| unsupported_if(e, os::rename_unsupported))
}

/// A clone of `from` at `to`, pushed; otherwise the OS's error as it is (`Unsupported` where the OS
/// has no clone call). The caller copies on any error but `AlreadyExists`.
pub fn clone_file(from: &Path, to: &Path) -> io::Result<()> {
    os::clone_file(from, to)
}

/// What `p` names, whatever it is called (links followed).
pub fn file_id(p: &Path) -> io::Result<FileId> {
    os::file_id(p)
}

/// What an open file is.
pub fn file_id_of(f: &File) -> io::Result<FileId> {
    os::file_id_of(f)
}

/// Whether a rename from `a` to `b` stays on one volume (WRITE-15 step 3).
pub fn same_volume(a: &Path, b: &Path) -> io::Result<bool> {
    Ok(file_id(a)?.volume == file_id(b)?.volume)
}

/// Whether the volume holding `dir` is this machine's own, not a network or FUSE volume (WRITE-17,
/// WRITE-19): elsewhere the lock stops only this machine's programs, and events name only its
/// writes. True when the OS can't tell.
pub fn is_local(dir: &Path) -> bool {
    os::is_local(dir)
}

/// The last name of `p` as the disk stores it, when the OS can tell (macOS); None otherwise, or
/// when nothing is there.
pub fn stored_name(p: &Path) -> Option<OsString> {
    os::stored_name(p)
}

/// Take the exclusive lock on `f` without waiting: Ok(true) taken, Ok(false) held elsewhere (by
/// another open of the file, in this process or another), Err(Unsupported) when the volume can't
/// lock (WRITE-17's `lock.unsupported`). The lock lasts as long as the open file.
pub fn try_lock(f: &File) -> io::Result<bool> {
    match f.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(e)) => Err(unsupported_if(e, os::lock_unsupported)),
    }
}

/// Let the user change a copied file, even if what it was copied from was read-only.
pub fn make_writable(p: &Path) -> io::Result<()> {
    os::make_writable(p)
}

/// `e` as `Unsupported` when its error number is on the OS's `list`, else as it is.
fn unsupported_if(e: io::Error, list: fn(i32) -> bool) -> io::Error {
    match e.raw_os_error() {
        Some(code) if list(code) => io::Error::new(ErrorKind::Unsupported, e),
        _ => e,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new(tag: &str) -> Temp {
            let d = std::env::temp_dir().join(format!("m2-os-fs-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Temp(d)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // WRITE-25: `replace` writes a file whole, over what was there, and leaves no temporary file;
    // one that can't be made (no folder) leaves nothing either.
    #[test]
    fn replace_writes_whole_and_leaves_nothing_beside() {
        let t = Temp::new("replace");
        let p = t.0.join("index.json");
        replace(&p, b"one").unwrap();
        replace(&p, b"two, longer").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two, longer");
        assert!(replace(&t.0.join("gone/index.json"), b"x").is_err());
        let names: Vec<String> = std::fs::read_dir(&t.0).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["index.json"]);
    }

    // RENDER-30: a read at an offset reads those bytes, leaves the file's position alone, and
    // reads short at the end.
    #[test]
    fn read_at_reads_where_asked() {
        let t = Temp::new("read-at");
        let p = t.0.join("f");
        std::fs::write(&p, (0..=255u8).collect::<Vec<_>>()).unwrap();
        let f = File::open(&p).unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(read_at(&f, &mut buf, 100).unwrap(), 4);
        assert_eq!(buf, [100, 101, 102, 103]);
        assert_eq!(read_at(&f, &mut buf, 254).unwrap(), 2, "short at the end");
        assert_eq!(&buf[..2], &[254, 255]);
        let mut big = vec![0u8; 300];
        assert_eq!(read_full_at(&f, &mut big, 10, 7).unwrap(), 246, "in pieces, to the end");
        assert_eq!(big[..246], (10..=255u8).collect::<Vec<_>>()[..]);
        use std::io::Read;
        let mut first = [0u8; 1];
        (&f).read_exact(&mut first).unwrap();
        assert_eq!(first, [0], "the file's own position never moved");
    }

    // PLAT-3 (§2.2 of the platform design): each OS's lists, fed by name. On Linux ENOTSUP and EOPNOTSUPP
    // are both 95; the Mac run checks its 45 and 102, which std's ErrorKind would miss. macOS's
    // list for the barrier's fallback is tested in mac/mod.rs, the one OS that has it.
    #[test]
    fn each_os_says_which_errors_mean_the_volume_cant() {
        use libc::{EEXIST, EINVAL, EIO, ENOLCK, ENOTSUP, ENOTTY, EOPNOTSUPP};
        for code in [ENOTSUP, EINVAL] {
            assert!(os::rename_unsupported(code), "rename: {code}");
        }
        for code in [ENOTTY, ENOLCK, EEXIST, EIO] {
            assert!(!os::rename_unsupported(code), "rename: {code}");
        }
        for code in [ENOTSUP, EOPNOTSUPP, ENOLCK] {
            assert!(os::lock_unsupported(code), "lock: {code}");
        }
        for code in [EINVAL, ENOTTY, EEXIST, EIO] {
            assert!(!os::lock_unsupported(code), "lock: {code}");
        }
        for code in [EINVAL, ENOTSUP, EOPNOTSUPP] {
            assert!(os::sync_refused(code), "folder sync: {code}");
        }
        for code in [EIO, ENOLCK, ENOTTY, EEXIST] {
            assert!(!os::sync_refused(code), "folder sync: {code}");
        }
        let e = unsupported_if(io::Error::from_raw_os_error(ENOTSUP), os::lock_unsupported);
        assert_eq!(e.kind(), ErrorKind::Unsupported);
        assert_eq!(unsupported_if(io::Error::from_raw_os_error(EIO), os::lock_unsupported).raw_os_error(), Some(EIO), "as it is");
    }

    // WRITE-15, PLAT-3, OS-03, OS-07: a swap exchanges two files; a create-only rename never
    // replaces one.
    #[test]
    fn a_swap_exchanges_and_an_exclusive_rename_never_replaces() {
        let t = Temp::new("rename");
        let (a, b, c) = (t.0.join("a"), t.0.join("b"), t.0.join("c"));
        std::fs::write(&a, "new").unwrap();
        std::fs::write(&b, "old").unwrap();
        rename(&a, &b, Rename::Swap).unwrap();
        assert_eq!((std::fs::read_to_string(&a).unwrap(), std::fs::read_to_string(&b).unwrap()), ("old".into(), "new".into()));
        assert_eq!(rename(&a, &b, Rename::Excl).unwrap_err().kind(), ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "new", "untouched");
        rename(&a, &c, Rename::Excl).unwrap();
        assert!(!a.exists() && std::fs::read_to_string(&c).unwrap() == "old");
        assert_eq!(rename(&a, &b, Rename::Swap).unwrap_err().kind(), ErrorKind::NotFound, "a swap needs both");
    }

    // FMT-16, WRITE-19, OS-10, OS-11, OS-16, OS-17: a file's id follows it under any name; its
    // stamp, its version.
    #[test]
    fn a_file_is_told_by_its_id_and_its_version_by_its_stamp() {
        let t = Temp::new("id");
        let (a, b, other) = (t.0.join("a"), t.0.join("b"), t.0.join("other"));
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&other, "x").unwrap();
        std::fs::hard_link(&a, &b).unwrap();
        assert_eq!(file_id(&a).unwrap(), file_id(&b).unwrap(), "two names, one file");
        assert_ne!(file_id(&a).unwrap(), file_id(&other).unwrap());
        assert_eq!(file_id_of(&File::open(&a).unwrap()).unwrap(), file_id(&a).unwrap());
        assert_eq!(file_id(&t.0.join("none")).unwrap_err().kind(), ErrorKind::NotFound);
        assert!(same_volume(&t.0, &a).unwrap());
        assert!(is_local(&t.0), "the temporary folder is on this machine");
        let before = Stamp::at(&a).unwrap();
        assert_eq!(before.size, 1);
        assert_eq!(Stamp::at(&a), Some(before), "the same version");
        std::fs::write(&a, "yy").unwrap();
        assert_ne!(Stamp::at(&a), Some(before), "a rewrite");
        assert_eq!(Stamp::at(&t.0.join("none")), None);
    }

    // WRITE-15: what a commit calls works on a plain file and folder; a clone is made, or the OS
    // says it can't, and a taken name is never cloned over.
    #[test]
    fn what_a_commit_calls_works_here() {
        let t = Temp::new("sync");
        let f = t.0.join("f");
        std::fs::write(&f, "bytes").unwrap();
        let file = File::open(&f).unwrap();
        push(&file).unwrap();
        barrier(&file).unwrap();
        sync_dir(&t.0).unwrap();
        match clone_file(&f, &t.0.join("clone")) {
            Ok(()) => assert_eq!(std::fs::read_to_string(t.0.join("clone")).unwrap(), "bytes"),
            Err(e) => assert_eq!(e.kind(), ErrorKind::Unsupported, "{e}"),
        }
        std::fs::write(t.0.join("taken"), "mine").unwrap();
        assert!(clone_file(&f, &t.0.join("taken")).is_err());
        assert_eq!(std::fs::read_to_string(t.0.join("taken")).unwrap(), "mine");
    }

    // WRITE-1, WRITE-15 (decision D2 of the platform review): a folder sync is skipped where it
    // can't be done, by every writer alike: a folder that can be written but not listed, and a
    // volume with no folder sync (Linux's `/proc` has none, as `vboxsf` has none). Any other error
    // fails: a folder that isn't there.
    #[test]
    fn a_folder_sync_is_skipped_only_where_it_cant_be_done() {
        use std::os::unix::fs::PermissionsExt;
        let t = Temp::new("syncdir");
        let w = t.0.join("write-only");
        std::fs::create_dir(&w).unwrap();
        std::fs::set_permissions(&w, std::fs::Permissions::from_mode(0o300)).unwrap();
        let synced = sync_dir(&w);
        std::fs::set_permissions(&w, std::fs::Permissions::from_mode(0o700)).unwrap();
        synced.unwrap();
        #[cfg(target_os = "linux")]
        sync_dir(Path::new("/proc")).unwrap();
        assert_eq!(sync_dir(&t.0.join("none")).unwrap_err().kind(), ErrorKind::NotFound);
    }

    // WRITE-1: the folders made for a new file, each made one synced in the one above it.
    #[test]
    fn make_dirs_makes_every_folder_on_the_way() {
        let t = Temp::new("makedirs");
        make_dirs(&t.0.join("a/b/c")).unwrap();
        assert!(t.0.join("a/b/c").is_dir());
        make_dirs(&t.0.join("a/b")).unwrap();
        std::fs::write(t.0.join("a/file"), "x").unwrap();
        assert!(make_dirs(&t.0.join("a/file/d")).is_err(), "a file in the way");
        assert_eq!(folder_of(Path::new("song.ts")), Path::new("."));
        assert_eq!(folder_of(Path::new("a/song.ts")), Path::new("a"));
        assert_eq!(folder_of(Path::new("/song.ts")), Path::new("/"));
    }

    // WRITE-17, OS-08: one holder at a time, a second open of the file included; let go with the
    // file.
    #[test]
    fn a_lock_has_one_holder() {
        let t = Temp::new("lock");
        let open = || std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(t.0.join("lock")).unwrap();
        let first = open();
        assert!(try_lock(&first).unwrap());
        assert!(!try_lock(&open()).unwrap(), "held by the first open");
        drop(first);
        assert!(try_lock(&open()).unwrap(), "let go");
    }

    #[test]
    fn a_copied_read_only_file_can_be_made_writable() {
        let t = Temp::new("writable");
        let f = t.0.join("f");
        std::fs::write(&f, "").unwrap();
        let mut perms = std::fs::metadata(&f).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&f, perms).unwrap();
        make_writable(&f).unwrap();
        assert!(!std::fs::metadata(&f).unwrap().permissions().readonly());
    }

    // WRITE-19, OS-19: a disk that folds capitals says how it stores a name; one that can't say
    // gives None.
    #[test]
    fn a_stored_name_is_the_disks_spelling() {
        let t = Temp::new("stored");
        std::fs::write(t.0.join("Mixed.txt"), "").unwrap();
        for asked in ["Mixed.txt", "mixed.txt"] {
            let stored = stored_name(&t.0.join(asked));
            assert!(stored.as_ref().is_none_or(|s| s == "Mixed.txt"), "{asked}: {stored:?}");
        }
        assert_eq!(stored_name(&t.0.join("none")), None);
    }
}
