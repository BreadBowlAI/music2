# Survey: every OS-specific piece of music2's code, for the platform layer

The code surveyed is HEAD `8aa301e` (`git grep` and `git show HEAD:…`), with step 7's working tree read only where it adds
OS code (marked **WT**). Nothing was built, run or edited in the checkout. The Linux and Windows facts come from the crate
sources in `~/.cargo/registry` (cpal 0.18.2, libc 0.2.190, tauri-utils 2.10.1, muda 0.20.0, clap-sys 0.5.0), the kernel
sources and man pages, and vendor docs. The sources are listed at the end. A claim marked *(verify)* comes from memory or
from a secondary source, and the first Linux run should check it.

Each piece has an id (`OS-nn`), its file and lines at HEAD, the contract rule it serves, what macOS does today, and its
Linux and Windows equivalents with their caveats.

---

## 0. Read this first

1. **Linux compiles, but it doesn't pass yet** *(expected; not built here)*. Every macOS call is behind
   `cfg(target_os = "macos")`, so the Linux fallbacks build: no events, no exclusive renames, plain `fsync` as the barrier,
   `/etc/machine-id`, and a channel queue as the plugin main thread. These tests should fail on Linux at HEAD (§13.1):
   - the three event-dependent tests in `crates/server/src/graph/watch_tests.rs`;
   - `core/plugins/tests/main_thread.rs`, which asserts `process_main`, a value that is always false off macOS;
   - `a_commit_that_fails_after_its_preview_accepts_nothing`, which expects the second rename into the project to be
     `extra.json`, but without `RENAME_SWAP` the fallback renames twice;
   - very likely `fixture_renders_change_only_with_stds_version` for `examples/resample`. Audio lanes call the platform's
     `cosf` and `pow` in host code (§8);
   - when CI runs as root, as Docker does by default, every test that relies on `chmod` refusing a write or a read.
2. **The durability model is macOS-shaped.** On macOS `fsync` only pushes to the drive, and one `F_FULLFSYNC` flushes the
   drive. On Linux `fsync` already flushes the drive, so the "push many, flush once" economy (step 6 design R1) doesn't
   exist there. On Windows a directory can't be synced, and NTFS has no atomic exchange (§1).
3. **On Windows the lock file can't be read by others.** Rust's `File::try_lock` there is `LockFileEx` over the whole
   file, and those locks are mandatory, so a second process can't read the holder's JSON. Lock one byte past the end of
   the file instead. On NFS, `flock` becomes a POSIX whole-file lock that belongs to the process, which breaks the step 6
   design's C10 and the export sweeper's in-process checks (§2).
4. **inotify has no FlushSync, and it doesn't need one.** It needs one watch per directory, and it has per-user limits.
   An event is queued when its system call returns, so "flush" is a non-blocking drain *(verify)* (§4).
5. **The plugin main thread differs per OS.** The app needs GTK's main context on Linux and the Win32 message loop on
   Windows. Linux CLAP plugins expect the host's `timer-support` and `posix-fd-support`. Windows plugins need a pumped
   message queue, even in the CLI (§5).
6. **Determinism across OSes isn't free.**
   - Host-side DSP math goes through each platform's libm.
   - `std/kernels/std.wasm` embeds the builder's absolute path (`<repo>/core/kernel/src/lib.rs`)
     and the rustc commit. A rebuild on Linux therefore changes the kernels' hashes.
   - The toolchain isn't pinned (§8, §11).
7. **The browser suites have three Mac assumptions.**
   - The pinned chrome-headless-shell 133.0.6943.141 has no linux-arm64 build. Chrome for Testing has one only from
     milestone 153.
   - Suites that inject `native: false` get `isMac` from `navigator.platform`, so Mac-only strings (`⌘E`) fail on
     Linux.
   - The UI's fonts come from Google Fonts over the network (§12).
8. **Paths.** The bundle's resource layout (`X.app/Contents/Resources`), `~/Library/Caches`, `~/Music` and the CLAP paths
   are macOS-only. On Windows, `$HOME`, the variable every test uses to isolate itself, means nothing to the platform's own
   folder APIs (§7).
9. **Step 8b's shortcut plan collides off macOS.** "⌃M Mute, ⌃S Solo" become Ctrl+M and Ctrl+S there, and Ctrl+S is Save.
   8b needs a per-OS chord map (§9).
10. **Docker on this Mac.**
    - Run the container as a non-root user.
    - Keep the target directory and temp files on volumes inside the VM, not on bind mounts.
    - Set `M2_AUDIO=none`.
    - Use an arm64 chrome-headless-shell of milestone 153 or later, or run amd64 under emulation.
    - Watch `vm.max_map_count`: wasmtime makes about 2 mappings per kernel instance.
    - The owner needs to start Docker Desktop once (§15).

---

## 1. Durable writes and atomic renames (`core/session/src/tx/stage.rs`)

What the commit relies on (WRITE-15, PLANE-5, the step 6 design's 6a.2 and R1/R9/R10):
- **push:** bytes go to the device, cheaply;
- **barrier:** everything pushed is made durable, once per commit;
- **swap:** replacing a target, and reading back what it held (R9 catches an outside writer);
- **exclusive rename:** a create that never overwrites;
- **clone:** a before-image that shares no inode with its target.

| id | piece (HEAD lines) | rule | macOS today |
|---|---|---|---|
| OS-01 | `push()`: `libc::fsync` on unix, `sync_all` elsewhere (stage.rs 88-103) | WRITE-15 steps 2-4, 8; R1 | `fsync` pushes to the drive without flushing its cache |
| OS-02 | `Real::barrier`: `fcntl(F_FULLFSYNC)`, else `sync_all` (141-153) | PLANE-5 ("the sync is never skipped or weakened"), WRITE-15 step 5 | one drive-cache flush per commit, about 5 ms idle and 8-14 ms busy (contract PLANE-5) |
| OS-03 | `Real::rename` `Swap`/`Excl`: `renamex_np(RENAME_SWAP / RENAME_EXCL)`, `ENOTSUP`/`EINVAL` → `Unsupported` (155-174); fallback in `rename_all` (aside, then plain) | WRITE-15 step 7; R9; FMT-14 (create-only) | APFS and HFS+ support both; FAT, exFAT and SMB don't, and the fallback runs |
| OS-04 | `Real::clone_file`: `clonefile(2)`, else read and `create` (192-207) | WRITE-15 step 3 (before-images in `blobs/`) | APFS clones in O(1) |
| OS-05 | `sync_dir`: `File::open(dir)` + `push` (137-139) | WRITE-15 steps 4, 8 | works |
| OS-06 | `cpath()`: `OsStrExt` bytes to a `CString` (105-109) | — | unix only |
| OS-07 | `name_new` in `crates/editor/src/render.rs` 446-458: `hard_link` as create-only naming, falling back to a look and a rename on `ErrorKind::Unsupported` | FMT-14, WRITE-1 | `link(2)` on exFAT gives `ENOTSUP`, so the fallback runs |

### Linux

- **OS-01, push.** `fsync(2)` on Linux "includes writing through or flushing a disk cache if present" (fsync(2)). Each
  push is a full flush, durable on its own, so a commit costs N+3 flushes instead of one.
  - **Simplest:** keep `fsync` per push (or `fdatasync` for files, which skips timestamp-only metadata) and make the
    barrier a no-op beyond the last one. Then measure G11 on Linux.
  - **Cheaper and fragile:** `sync_file_range(WRITE|WAIT_AFTER)` per file, then one `fsync`. The man page calls it
    "extremely dangerous": it writes no metadata, and durability then depends on ext4/xfs journaling details. Not
    recommended.
  - `syncfs(fd)` would be one barrier for the whole filesystem: it "matches fsync() called on every file in that
    filesystem", so it also waits for every other writer. G11's busy-disk case would then wait for the 1 GB writer.
- **OS-02, barrier.** `fsync` of the journal file (or no-op, as above). It is durable on ext4, xfs and btrfs with the
  default barriers, and on NFS it is a COMMIT to the server. tmpfs (where `/tmp` often is) makes nothing durable, which
  is fine for tests and meaningless for gates. Old kernels and "lesser used filesystems" may not flush caches (fsync(2)).
- **OS-03, swap and exclusive rename.** Use `renameat2(AT_FDCWD, a, AT_FDCWD, b, RENAME_EXCHANGE | RENAME_NOREPLACE)`.
  The kernel call dates from Linux 3.15, the glibc wrapper from 2.28, and libc 0.2.190 has it for gnu and musl. An
  unsupported flag returns **`EINVAL`**, which the existing mapping already treats as `Unsupported`. Support by
  filesystem:

  | filesystem | NOREPLACE (Excl) | EXCHANGE (Swap) | source |
  |---|---|---|---|
  | ext4 | 3.15 | yes (3.15) | rename(2); ext4's flag mask *(verify)* |
  | btrfs, tmpfs | 3.17 | yes *(verify version)* | rename(2) |
  | xfs | 4.0 | yes *(verify)* | rename(2) |
  | f2fs | yes | yes *(verify)* | — |
  | overlayfs (Docker's container root) | yes | yes | `ovl_rename`: `flags & ~(RENAME_EXCHANGE \| RENAME_NOREPLACE)` (v6.12) |
  | FUSE (sshfs, rclone, virtiofs, ntfs-3g) | if the server implements rename2 (protocol 7.23+), else `EINVAL` | same | `fuse_rename2` (v6.12) |
  | NFS | **no** | **no** | `nfs_rename`: `if (flags) return -EINVAL` (v6.12) |
  | 9p (WSL2's `/mnt/c`, some VMs) | **no** | **no** | `v9fs_vfs_rename`: `if (flags)` → `-EINVAL` (v6.12) |
  | SMB/CIFS client | yes (3.17) | **no** | `cifs_rename2`: `flags & ~RENAME_NOREPLACE` (v6.12) |
  | vfat | 4.9 | 6.0 *(verify; the 2022 patch series)* | rename(2) |
  | exfat, ntfs3 | yes | **no** *(verify)* | — |

  Caveat: `EINVAL` also means other things (a directory into its own subdirectory). On NFS, without `NOREPLACE`, the
  fallback "look, then rename" has the race the macOS fallback has on SMB. The contract already accepts that race for
  outside writers.
- **OS-04, clone.** `ioctl(dst, FICLONE, src)` reflinks on btrfs, XFS (with `reflink=1`, the default since xfsprogs 5.1)
  and bcachefs. ext4 has no reflink, so the fallback copy runs and a 50 MB plugin blob is copied once per commit that
  replaces it. `std::fs::copy` uses `copy_file_range`, which may share extents on btrfs and XFS. That still meets "shares
  no inode".
- **OS-05, directory sync.** `fsync` on an `O_RDONLY` directory descriptor works on ext4, xfs and btrfs, and "is needed"
  for a new entry to be durable (fsync(2)). Some FUSE and network filesystems return `EINVAL` for a directory sync. Today
  any error there fails the commit (`disk.sync_dir(..).map_err(io)?`), so map `EINVAL` from a directory sync to "this
  volume can't" rather than failing every commit.
- **OS-07, hard links.** On Linux `link(2)` on vfat or exfat returns **`EPERM`** ("does not support the creation of hard
  links"), not `ENOTSUP`. So `name_new`'s fallback never runs, and an export to a FAT stick fails on Linux. OS-03's
  exclusive rename is exactly the "create-only name" `name_new` wants, and it removes the hard-link trick on every OS.

### Windows

- **push and barrier.** `FlushFileBuffers` flushes the file's data and metadata and the device cache, so each call costs
  what `F_FULLFSYNC` costs. Without a push-only call, R1's economy is lost here too. `FILE_FLAG_WRITE_THROUGH` and
  `FILE_FLAG_NO_BUFFERING` are alternatives for the journal.
- **OS-05, directories.** A directory can't be flushed. `File::open(dir)` without `FILE_FLAG_BACKUP_SEMANTICS` fails, and
  `FlushFileBuffers` needs write access *(verify)*. NTFS journals renames in `$LogFile`; whether `FlushFileBuffers` on the
  renamed file forces the log record of its rename *(verify)* decides whether WRITE-15 step 8 can be met. `sync_dir` must
  be a no-op there, or the commit fails on every Tx.
- **OS-03, exclusive rename.** `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`, or `SetFileInformationByHandle(FileRenameInfoEx)`
  without the replace flag, fails if the target exists, atomically on NTFS and ReFS.
- **OS-03, swap.** NTFS has **no atomic exchange**, and `ReplaceFileW` isn't one, so the existing fallback is the path.
  Windows-specific: a rename or replace fails with `ERROR_SHARING_VIOLATION` or `ERROR_ACCESS_DENIED` while another
  process holds the file without `FILE_SHARE_DELETE`. Defender, the search indexer, OneDrive, Dropbox and editors all do,
  briefly. Commits need a short retry on those errors, with no equivalent on macOS or Linux.
- **OS-04, clone.** `FSCTL_DUPLICATE_EXTENTS_TO_FILE` on ReFS and Dev Drive (`FILE_SUPPORTS_BLOCK_REFCOUNTING`).
  Windows 11 24H2's `CopyFile2` block-clones there by itself *(verify)*. Otherwise a copy.
- **OS-07.** `CreateHardLinkW` works on NTFS, not on FAT or exFAT. Use the exclusive rename instead, as on Linux.

**Implication.** The layer is `Disk::Real` per OS plus a small **volume capability probe**: try exchange, noreplace,
clone, hard link and case folding once per volume, when the lock is taken, and remember the answers. The code already
falls back on `Unsupported`, so the probe only makes the fallbacks deliberate and lets gates and messages say which path
ran.

---

## 2. The project lock and machine identity (`core/session/src/lock.rs`)

| id | piece (HEAD lines) | rule | macOS today |
|---|---|---|---|
| OS-08 | `File::try_lock` (283, 325); also `render.rs` 271 (part file), 440 (sweeper) | WRITE-17, WRITE-18, C10, C19 | `flock(LOCK_EX\|LOCK_NB)`, advisory, belonging to the open file description |
| OS-09 | `cannot_lock`: `ENOTSUP`/`EOPNOTSUPP`/`ENOLCK` (231-237) | WRITE-17 `lock.unsupported` | as listed |
| OS-10 | `is_at`: `(dev, ino)` of the held file against the path (242-256) | WRITE-17 (a lock whose `.music2/` was removed) | `MetadataExt` |
| OS-11 | `prepare`: `.music2/tx/` on the project's `st_dev` (199-207) | WRITE-15 step 3 (staging must rename into place) | `MetadataExt::dev` |
| OS-12 | `host_id`: `gethostuuid`, then `/etc/machine-id`, then the host name (122-139) | FMT-20, WRITE-16 (`tx.foreign`), C18 | the hardware UUID |
| OS-13 | `host_name`: `gethostname` (142-152) | WRITE-18 messages | |
| OS-14 | `clock`: `localtime_r` (107-118) | WRITE-18 message ("since 14:02") | |
| OS-15 | `local_volume`: `statfs` `f_flags & MNT_LOCAL` (213-228) | WRITE-17 `lock.local`; WRITE-19 source choice | true elsewhere |

### Linux

- **OS-08, the lock.** On a local filesystem `flock` is the same as on macOS: per open file description, and released at
  exit.
  - **NFS:** since 2.6.12, NFS emulates `flock` "as fcntl(2) byte-range locks on the entire file", and an exclusive lock
    needs the file open for writing (flock(2)). POSIX locks belong to the **process**, with three consequences.
    - A second session in one process gets the lock too, so the step 6 design's C10 ("a second session … finds the lock
      held") fails on NFS.
    - Closing *any* descriptor of that file in the holder process drops the lock. `read_holder` is called after a
      `WouldBlock`, and `probe()` runs readers in the same process.
    - The sweeper's read-only `File::open` plus `try_lock` (`render.rs:440`) fails with `EBADF` (no write access), so
      stale parts are never swept on NFS. That one is harmless.

    With `local_lock=flock`, NFS locks are local only.
  - **SMB:** since Linux 5.5, `flock` is emulated with SMB byte-range locks that "are not advisory anymore: any IO on a
    locked file will always fail with EACCES when done from a separate file descriptor". `read_holder` then fails, as on
    Windows. Before 5.5, `flock` isn't propagated over SMB.
  - **FUSE:** local locks unless the filesystem implements them.
  - **Recommendation:** on volumes `local_volume` calls network, say `lock.local` (already the plan) and don't trust the
    process-level property. Read the holder through the *held* descriptor, or keep it in a sibling `lock.json`.
- **OS-09.** `ENOTSUP == EOPNOTSUPP` (95) on Linux. `ENOLCK` (37) is what NFS without `lockd` returns.
- **OS-10, OS-11.** Same code. btrfs subvolumes report different `st_dev` values, so a `.music2/` inside another subvolume
  is refused, correctly. Bind mounts of one filesystem share `st_dev` but `rename` across them gives `EXDEV`. That is
  rare; a commit would then fail at its first rename, after staging, and roll back.
- **OS-12, machine id.** `/etc/machine-id` (systemd), else `/var/lib/dbus/machine-id`. Caveats:
  - Docker images often have no machine-id, so the fallback is the host name, the container's id, which changes every
    `docker run`. Journals left by an earlier container then read `tx.foreign`. That is harmless in CI.
  - Cloned VM images can share one machine-id.
  - `/sys/class/dmi/id/product_uuid` is the hardware UUID but readable by root only.
- **OS-13, OS-14.** Same calls. Containers run in UTC unless `TZ` is set.
- **OS-15, MNT_LOCAL.** Linux has no such flag. Two ways to tell:
  - by `statfs.f_type` magic: NFS `0x6969`, SMB `0x517B`, CIFS `0xFF534D42`, SMB2 `0xFE534D42`, CODA, AFS, Ceph
    `0x00C36400`, 9p `0x01021997`. libc lacks the CIFS, SMB2, 9p and Ceph constants, so define them locally;
  - better, by the fs type name in `/proc/self/mountinfo`: `nfs`, `nfs4`, `cifs`, `smb3`, `9p`, `ceph`, `fuse.sshfs`,
    `fuse.rclone`. `FUSE_SUPER_MAGIC` alone is ambiguous: virtiofs and ntfs-3g are local, sshfs isn't.

  This also decides whether inotify sees other machines' writes (it doesn't, §4).

### Windows

- **OS-08, the lock.** Rust's `try_lock` is `LockFileEx(LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY)` over the
  whole range. LockFileEx: "Locking a portion of a file for exclusive access denies all other processes both read and
  write access". So **`read_holder` can't read who holds the lock**, and WRITE-18's message falls back to "another
  program".
  - **Fix:** lock one byte far past the end ("Locking a region that goes beyond the current end-of-file position is not an
    error"), as SQLite does with its pending byte. That needs a direct `LockFileEx`, since `std::fs::File::lock` has no
    range.
  - The lock belongs to the handle: "If the locking process opens the file a second time, it cannot access the specified
    region through this second handle", so C10 holds.
  - The OS releases locks at exit, but "the time it takes … depends upon available system resources". A retry covers it.
  - For `render.rs:269-271`, whether a `try_clone` (DuplicateHandle) handle still writes the region the original locked
    *(verify)* decides whether the part lock trick works.
- **OS-09.** `ERROR_NOT_SUPPORTED` and `ERROR_INVALID_FUNCTION` on filesystems without byte-range locks.
- **OS-10.** `(volume serial, file index)` from `GetFileInformationByHandle`. `std`'s `MetadataExt::{volume_serial_number,
  file_index}` is unstable (`windows_by_handle`). ReFS needs the 128-bit id (`FileIdInfo`). The `same-file` crate does
  this. Rust opens files with `FILE_SHARE_DELETE`, so `git clean` can still delete a held `.music2/lock`, and the "lock
  whose folder was removed" case exists there too.
- **OS-12.** `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`. It is per install, and images not sysprep'd share it.
- **OS-13, OS-14.** `GetComputerNameExW(ComputerNameDnsHostname)` and `localtime_s`.
- **OS-15.** `GetDriveTypeW(root) == DRIVE_REMOTE`, plus UNC paths (`\\server\share`). Mapped drives report `REMOTE`.

---

## 3. File identity, stamps and spelling (stamps, hash cache, Tx paths, the watcher)

| id | piece (HEAD lines) | rule | macOS today |
|---|---|---|---|
| OS-16 | `Stamp::of`: `ctime` and `ino` from `MetadataExt` on unix, `created()` and inode 0 elsewhere (`core/session/src/watch.rs` 48-60; `core/song/src/hash.rs` 144-158) | FMT-16 (cache key `(path, size, mtime, ctime, inode)`), WRITE-19 (stamps, own writes via `own_stamp`) | real ctime and inode |
| OS-17 | Tx `spelled()`: on a case or Unicode mismatch, finds the real name by `(dev, ino)` (`core/session/src/tx/mod.rs` 504-545; the unix part 520-533) | WRITE-3a | APFS ignores case and normalization |
| OS-18 | `file_id`: `(dev, ino)` on unix, `canonicalize` elsewhere (`crates/editor/src/render.rs` 407-417) | RENDER-5 (no approximate render in `assets/`) | |
| OS-19 | watcher `spelled()`: `getattrlist(ATTR_CMN_NAME, FSOPT_NOFOLLOW)` reads the stored name (`watch.rs` 190-222); `true` off macOS | WRITE-19, WRITE-3a (`a_case_only_rename_settles_as_one_path`) | the stored name |

The rest of the spelling machinery is portable already: imports matched against folder listings
(`core/script/src/lib.rs` 290-345), `Snapshot` listing names, and the editor's "free in any capitals" checks.

### Linux

- **OS-16.** Same code: real `ctime` and `ino`, with nanosecond timestamps on ext4, xfs, btrfs and tmpfs.
- **OS-17, OS-18.** Same code.
- **OS-19, stored spelling.** Off macOS `spelled()` answers `true`. On a case-insensitive Linux volume, a case-only rename's
  old name still opens. It would be stat'd and taken in as a second key, which is the very bug the macOS code prevents.
  - Case-insensitive volumes on Linux: vfat, exfat, ntfs3/ntfs-3g, SMB/CIFS mounts, and ext4 or f2fs directories with the
    casefold flag (`chattr +F`, used by Steam/Proton and Android).
  - The portable answer is to list the parent directory and compare names. That is O(directory) per path, so cache the
    listing per directory mtime, or do it only on volumes the probe found case-insensitive (§1).
  - Linux ext4 casefold also normalizes (NFD-based fold). Plain ext4 is byte-exact: `café` in NFC and NFD are two files
    there, where APFS sees one.

### Windows

- **OS-16.** `created()` stands in for ctime, and the inode is 0.
  - File-system **tunneling**: NTFS and FAT give a file created within about 15 s of a delete or rename under the same
    name the old creation time *(verify)*. An editor's atomic save (write a temp file, rename it over) keeps the old
    "ctime", so the key leans on mtime and size alone.
  - Use `FILE_BASIC_INFO.ChangeTime` (`GetFileInformationByHandleEx`) and the file index (OS-10) instead.
  - NTFS timestamps have 100 ns resolution. FAT's mtime is 2 s, which FMT-16's racy rule covers.
- **OS-17, OS-19.** `GetFinalPathNameByHandleW` gives every component as stored. Today's `#[cfg(not(unix))]` says only
  "another way".
- NTFS ignores case by default; per-directory case sensitivity exists (`fsutil file setCaseSensitiveInfo`). It doesn't
  normalize Unicode, so NFC and NFD names are two files, as on ext4.

---

## 4. File-system events (`crates/server/src/graph/fsevents.rs`, `watcher.rs`)

What the watcher needs from an event source (WRITE-19, and G11's "idle under 1% of a core"):
- **names:** project-relative paths that changed, as hints;
- **rescan:** events were dropped, or the root moved or was remounted;
- **flush:** have every event that happened before now delivered (`FSEventStreamFlushSync`), so a walk can tell "on its
  way" from "missed";
- **events:** a counter, for the gates;
- **wake:** an event wakes the watcher's thread.

| id | piece (HEAD lines) | macOS today |
|---|---|---|
| OS-20 | `fsevents.rs` (all): `FSEventStreamCreate` with `FileEvents\|NoDefer\|WatchRoot`, latency 10 ms, on a private dispatch queue; `MAX_HINTS` 4096; drop, root and mount flags → rescan | CoreServices and libdispatch through `extern "C"` |
| OS-21 | `watcher.rs` 176-193, `source()`: walks with `M2_WATCH=walk`, or when `!local_volume`, or when the stream won't start; flushes around a full walk (`look`, 246-275); a `Source::Events` arm in `tick` | |
| OS-22 | `graph.rs:40`: `#[cfg(target_os = "macos")] mod fsevents;` | |

### Linux: inotify

- **Watches.** One watch per directory, since inotify "monitoring of directories is not recursive". Watch the project and
  every directory a walk enters. Never watch `.git`, `.music2` or `node_modules`: `walked()` already says which. That
  also keeps the watch count down.
- **New directories.** Add a watch on `IN_CREATE | IN_ISDIR` and on a directory's `IN_MOVED_TO`, then walk it: "the new
  subdirectory may already contain files". The existing "a folder moved in is walked" rule fits.
- **Event mask.** `IN_CLOSE_WRITE | IN_MODIFY | IN_ATTRIB | IN_CREATE | IN_DELETE | IN_MOVED_FROM | IN_MOVED_TO |
  IN_DELETE_SELF | IN_MOVE_SELF | IN_EXCL_UNLINK | IN_ONLYDIR` (the last on directories).
  - **Rescan** on `IN_Q_OVERFLOW` (wd -1, "always generated" after a drop), on `IN_UNMOUNT`, and on `IN_MOVE_SELF` or
    `IN_DELETE_SELF` of the root.
  - Rename cookies are racy: "IN_MOVED_FROM can be visible before IN_MOVED_TO", and a rename from outside has no
    `MOVED_FROM`. Treat both names as hints and don't pair them.
- **Flush.** inotify events are queued from within the system call that changed the file, so after the writer's call
  returns, the event is in the queue *(verify)*. The man page says only that the queue is ordered. "Flush" is then a
  non-blocking `read` that drains the fd, which is stronger than `FlushSync`, because nothing is "on its way".
- **Limits** (`/proc/sys/fs/inotify/`):
  - `max_user_watches` was 8192 until it became RAM-scaled to at most 1% of memory, clamped to [8192, 1,048,576]
    (a 2020 patch, Linux 5.11 *(verify version)*);
  - `max_user_instances` 128 per user;
  - `max_queued_events` 16384.

  Instances are per *user*: an open project, an editor and VS Code all share the 128. Exceeding the watch limit gives
  `ENOSPC` on `inotify_add_watch`. The answer is the existing walk fallback, with the reason printed, as "FSEvents didn't
  start" is today.
- **Remote filesystems.** inotify "does not catch remote events that occur on network filesystems", the same rule as
  macOS's (OS-15). Inside Docker Desktop, bind mounts are VirtioFS, and host-side changes are reported to be seen by
  container watchers. The tests write inside the container anyway (§15).
- **fanotify** can watch a whole filesystem recursively, but filesystem and mount marks need `CAP_SYS_ADMIN`. Not usable.

### Windows

- **API.** `ReadDirectoryChangesW` (or `…ExW` with `FILE_NOTIFY_EXTENDED_INFORMATION`), recursive (`bWatchSubtree`), with
  overlapped I/O.
- **Rescan.** Overflow returns a zero-length result or `ERROR_NOTIFY_ENUM_DIR`.
- **Flush.** There is none; a sentinel write that the watcher waits to hear is the usual stand-in. Note that
  `.music2/` is filtered out of hints today.
- **Root moves.** The open directory handle keeps the root from being renamed (Windows locks it), and root moves aren't
  reported.
- **SMB.** Change notifications from an SMB *server* report every client's changes, unlike macOS and Linux, but the
  buffer is limited to 64 KB on network shares *(verify)*.

---

## 5. The plugin main thread (HOST-2) and thread identity

| id | piece (HEAD lines) | rule | macOS today |
|---|---|---|---|
| OS-23 | `Kind::Dispatch`: `dispatch_async_f(&_dispatch_main_q, …)`, `CFRunLoopRunInMode` pump (`core/plugins/src/main_thread.rs` 36-47, 74-107, 133-147, 286-293) | HOST-2 | AppKit's main thread (app) or `CFRunLoop` (`run_main`) |
| OS-24 | `is_process_main()`: `pthread_main_np`; **false off macOS** (59-72) | HOST-2 ("thread checks answer truthfully") | |
| OS-25 | `token()`: `pthread_self` through a bare `extern "C"` (49-57) | HOST-2, RENDER-28 (no thread-local on the audio thread) | |
| OS-26 | `use_process_main_thread()` bails off macOS (200-216) | HOST-2 in the app | |
| OS-27 | `run_main` off macOS: the main thread drains a crossbeam channel (255-312) | HOST-2 in `m2`, the headless server, the gates | |
| OS-28 | probe records `process_main` (`core/plugins/src/probe.rs` 85-92, 352, 523); asserted by `core/plugins/tests/main_thread.rs` 78, 82 | HOST-2 tests | |
| OS-29 | `apps/music2-desktop/src/plugin_host.rs` 1-10 and 21-24 (the AppKit main queue), 65-71 (`leave_now` = `_exit(0)`) | HOST-2 quit | |
| OS-30 | `apps/music2-desktop/tests/appkit_main_thread.rs`: AppKit through `objc_msgSend`, `#[link(name = "AppKit")]`; prints "macOS only" elsewhere (319-322) | HOST-2 in the app | |
| OS-31 | host extensions offered: thread-check, log, state, params, latency (`core/plugins/src/inprocess.rs` 106-129) | HOST-1/2 | enough on macOS, where plugins post to the CFRunLoop themselves |

### Linux

- **OS-24, main-thread check.** `gettid() == getpid()`, through `libc::gettid` (glibc 2.30+) or `syscall(SYS_gettid)`.
  It never allocates. Without it, `core/plugins/tests/main_thread.rs` fails at line 78 today.
- **OS-25, thread token.** `pthread_self` is `unsigned long`, a register read, so it is fine. (A `thread_local!` in the
  main binary has static TLS, but its first use may register a destructor through `__cxa_thread_atexit_impl`, which can
  allocate. The rule against thread-locals stays useful.)
- **OS-26, the app.** Tauri on Linux runs GTK's main loop on the main thread. Posting is
  `g_main_context_invoke(NULL, …)` on the default context, or Tauri's own `AppHandle::run_on_main_thread`, which works
  on every OS. "Calls queued before the loop starts run once it does" holds for a GLib idle source.
- **OS-27, the CLI.** The channel loop works for plugins that never touch an event loop. Linux CLAP plugins get main-loop
  time through two Linux-only *host* extensions: `timer-support` (`clap_host_timer_support`) and `posix-fd-support`
  (`clap_host_posix_fd_support`; both in clap-sys 0.5's `src/ext/`).
  - JUCE-based plugins (Surge XT) use them on Linux for timers and X11 fds *(verify how Surge behaves when the host lacks
    them)*.
  - Then `run_main` and the app's executor must be a `poll(2)` loop over an eventfd (jobs), a timerfd (timers) and the
    plugins' fds.
  - In the app that loop is the GLib main context, which takes fds (`g_unix_fd_add`) and timers (`g_timeout_add`).
- **OS-29.** `_exit(2)` is the same on Linux.

### Windows

- **OS-24.** No `pthread_main_np` exists. Record `GetCurrentThreadId()` when the program installs the executor, which
  `run_main` and `use_process_main_thread` already require to be on the main thread.
- **OS-25.** `pthread_self` doesn't exist in the MSVC CRT, so the `extern "C"` declaration **fails to link**. Use
  `GetCurrentThreadId()`, which allocates nothing.
- **OS-26, OS-27.** The plugin main thread must **pump Win32 messages**. JUCE and most VST-era code create hidden windows
  and `SetTimer`s on the thread that initializes them, and `SendMessage` to them from another thread blocks until that
  thread pumps.
  - `run_main` then becomes `MsgWaitForMultipleObjectsEx` + `PeekMessage`/`DispatchMessage`, with an event for jobs.
  - The app posts through `run_on_main_thread`, or `PostMessage` to a hidden `HWND`.
- **OS-29.** `_exit` is in the UCRT. To skip `DLL_PROCESS_DETACH`, which would run plugin code (the reason `leave_now`
  exists), use `TerminateProcess(GetCurrentProcess(), 0)`.

**Implication.** The executor reduces to two functions the program installs: `post(job)` and `is_main()`. Each program
picks its OS loop: AppKit or CFRunLoop, GLib or a poll loop, or a Win32 message loop. HOST-2's text names AppKit and
`CFRunLoop` and needs the generic wording. HOST-3's host process later owns this loop, and plugin windows, on every OS.

---

## 6. CLAP discovery and loading (`core/plugins/src/lib.rs`)

| id | piece (HEAD lines) | rule | macOS today |
|---|---|---|---|
| OS-32 | `search_paths()`: `CLAP_PATH`, `$HOME/Library/Audio/Plug-Ins/CLAP`, `/Library/Audio/Plug-Ins/CLAP` (44-55) | FMT-18a, HOST-1 | |
| OS-33 | `binary_in()`: a file, or `Contents/MacOS/<first entry>` (128-135) | | bundles |
| OS-34 | `scan()`: one level per folder, `*.clap` (211-229) | | |
| OS-35 | `load()`: `libloading::Library::new`, `entry.init(bundle path)` (156-174) | | |

### Linux

- **Search paths.** `~/.clap` and `/usr/lib/clap` (CLAP `entry.h`). `CLAP_PATH` is ':'-separated, and
  `std::env::split_paths` already handles that.
- **Bundles.** A Linux `.clap` is the DSO itself, and the `is_file()` branch covers it. `init` takes "the path to the DSO
  (Linux, Windows), or the bundle (macOS)", and passing `bundle` is right.
- **Scan depth.** The spec says "each directory should be **recursively** searched". Today's scan is one level deep on
  every OS, macOS included, where vendors use subfolders.
- **dlopen.** libloading's default is `RTLD_LAZY | RTLD_LOCAL`. `RTLD_NOW` would fail at load rather than at first call.
- **System libraries.** A plugin's DSO needs its own system libraries, Surge XT's X11, xcb, freetype and fontconfig among
  them, even headless. Install them in CI images.
- **Windows (GUI, step 9).** `CLAP_WINDOW_API_X11` is what plugins implement. Wayland embedding isn't supported in
  practice, so plugin windows need X11 or XWayland. The host process (HOST-3) can be an X11 client whatever toolkit the
  app uses.

### Windows

- **Search paths.** `%COMMONPROGRAMFILES%\CLAP` and `%LOCALAPPDATA%\Programs\Common\CLAP`. `CLAP_PATH` is ';'-separated.
- **Bundles.** A `.clap` is a DLL.
- **Loading.** `LoadLibraryExW` with `LOAD_WITH_ALTERED_SEARCH_PATH`, so the plugin's own DLLs beside the `.clap` resolve
  *(verify libloading's flags)*.
- **GUI (step 9).** `CLAP_WINDOW_API_WIN32` (`HWND`).

---

## 7. Paths, folders and the bundle

| id | piece (HEAD lines) | rule | macOS today | Linux | Windows |
|---|---|---|---|---|---|
| OS-36 | `songs_dir()` = `$HOME/Music/music2`, `HOME` required (`crates/server/src/project.rs` 24-30); `Exports` there (`export.rs` 27, 63) | UX (README) | `~/Music` | `XDG_MUSIC_DIR` (`~/.config/user-dirs.dirs`, e.g. `~/Musique`), unset on servers, else `~/Music` | `FOLDERID_Music`, often **redirected into OneDrive**, a synced folder (FMT-20 `project.synced`) |
| OS-37 | `cache_dir()` = `$HOME/Library/Caches/music2` (`crates/server/src/resources.rs` 10-16); the hash cache (`core/song/src/hash.rs` 11-13, 195-197) | FMT-16, RENDER-30, FMT-20 | `~/Library/Caches/music2` | `$XDG_CACHE_HOME/music2` (`~/.cache/music2`) | `%LOCALAPPDATA%\music2\cache` |
| OS-38 | `publish_session`: `~/.music2/session.json`, mode 0600 (`apps/music2-desktop/src/main.rs` 120-136) | FMT-20 Today (per user) | | same; `$XDG_RUNTIME_DIR` is the idiom, but tools already read `~/.music2` | `%USERPROFILE%\.music2`; the profile's inherited ACL is private (no mode bits) |
| OS-39 | `bundled()` = `<exe>/../../Resources/<name>` (`resources.rs` 53-59); the desktop's `web_dir` uses Tauri's `resource_dir()` (`main.rs` 107-118) | — (std, templates and examples found by `m2`, the server and the app) | `music2.app/Contents/Resources` | Tauri puts resources at `/usr/lib/<productName>/` (deb/rpm), `${APPDIR}/usr/lib/<productName>` (AppImage), `${exe_dir}/../lib/<name>` (tauri-utils `resource_dir_from`) | the executable's folder |
| OS-40 | `source_tree()` = `CARGO_MANIFEST_DIR + "/../.."` (`resources.rs`; desktop `main.rs` 118; `headless.rs` 26) | | | fine | mixed separators work |
| OS-41 | `folder_part`: `/ \ :` → `-`, leading dots trimmed (`project.rs` 217-220); `reserved`: `Exports` in any case, "macOS doesn't tell" (239-244) | UX | | case-sensitive; still refused, which is conservative | also forbid `< > " \| ? *`, trailing dots and spaces, device names (`CON`, `nul.txt`) |
| OS-42 | `AssetName::new` allows `: * ? " < > \|`, trailing dot or space, and device names (`core/song/src/path.rs` 217-234) | FMT-14 | | legal | **not creatable**: a Linux-made `assets/a:b.wav` breaks a Windows checkout. A format rule, the owner's call |
| OS-43 | web `~` display: `path.startsWith(`${home}/`)` (`web/src/lib/graph.ts` 540-542) | UX | | fine | `\` paths never match |
| OS-44 | WRITE-12: dot files move with a folder Rename; any other unknown file refuses it | WRITE-12 | `.DS_Store` handled | editors' `name~` backups refuse it | Explorer's `Thumbs.db` and `desktop.ini` aren't dot files and refuse it |

- **Tests and `HOME`.** Tests, the e2e suites and `appkit_main_thread.rs` (281-286) isolate themselves by setting `$HOME`.
  On Linux the XDG lookups also read `$HOME`, so that keeps working. On Windows `dirs`/`SHGetKnownFolderPath` ignore
  `HOME` and `USERPROFILE` overrides. Keep one override variable (`HOME`, or an `M2_HOME`) that every folder function reads
  first, on every OS.
- **The case-insensitive `Exports` check** and the editor's "any capitals" checks stay correct but conservative on Linux.

---

## 8. Floating point and render determinism across OSes and CPUs

| id | piece | rule | today |
|---|---|---|---|
| OS-45 | host-side libm on the render path: `core/runtime/src/audio.rs` 191 (WSOLA window, `cosf`), 253 (`2f64.powf` for pitch), 283 (`10f32.powf` for gain), 294/297 (fades, `cosf`), 411-424 (crossfade `sin`/`cos`) | RENDER-2; `fixture_renders_change_only_with_stds_version` (`core/session/src/gate_tests.rs` 368-389, `tests/renders.json`) | the hashes in `renders.json` were recorded with Apple's libm |
| OS-46 | the kernels' math (`std/kernels-src`, `core/kernel` 235) | RENDER-2 | inside `std.wasm` the functions are compiler-builtins' `libm` (pure Rust), the same everywhere |
| OS-47 | `core/play/src/resample.rs` 46-49 (`sin`/`cos` f64) | RENDER-31 (device boundary) | live only; RENDER-33 is measured before it |
| OS-48 | wasmtime `Config`: `cranelift_nan_canonicalization(false)`, `wasm_relaxed_simd(false)` (`core/runtime/src/packages.rs` 87-97) | RENDER-2 | wasm floats are bit-exact on x86_64 and aarch64 except NaN payloads, which the renderer silences |
| OS-49 | denormals: wasm keeps subnormals and RENDER-2 forbids FTZ | gate 8 (dropouts) | Apple silicon handles subnormals at full speed |

- **OS-45, host libm.** Apple's libm, glibc, musl and the UCRT round some inputs differently in the last ULP. glibc on
  x86_64 also picks FMA variants of `cosf` by CPU (ifunc), so two x86 machines can differ. `examples/resample` uses
  pitch (12, −5, 7), fades and warp, so its recorded hash will probably not match on Linux. **The fix that makes RENDER-2
  hold everywhere:** call the `libm` crate (pure Rust; the same code compiler-builtins puts in `std.wasm`) for every
  transcendental on the render path, then record `renders.json` once. That changes macOS bits too, so per the test's own
  message it is a recorded, deliberate change, the owner's call. Arithmetic, `round`, `floor` and the like are exact
  everywhere, since Rust never fuses or reassociates.
- **OS-49, denormals.** x86 cores, Intel and AMD, pay a microcode assist per subnormal operation. Decaying tails (reverb,
  filters, envelopes) then cost many times more on an x86 Linux or Windows box than on an M1. Flushing the FTZ/DAZ bits
  would change bits, so it is ruled out. The kernels must keep their own state out of the subnormal range
  (deterministically: a tiny offset, or an explicit flush below a threshold). Relevant if the owner's Linux server is
  x86_64, and for gate 8 on any x86 machine.
- **Step 8a's RENDER-2a** (the `FPCR` at each callback) has an x86_64 twin: **MXCSR** (FTZ is bit 15, DAZ bit 6, rounding
  bits 13-14). Plugins set it, JUCE's `ScopedNoDenormals` among them. aarch64 Linux and Windows on Arm use FPCR as macOS
  does.

---

## 9. The desktop shell (Tauri)

| id | piece (HEAD lines) | macOS today | Linux | Windows |
|---|---|---|---|---|
| OS-50 | `tauri.conf.json`: `bundle.targets: ["app"]`, a `macOS` section (min 11.0, ad-hoc signing, hardened runtime off), icons `.icns` and `.ico` and PNGs, `category: "Music"` | `.app` | add `deb`/`rpm`/`appimage`; runtime deps webkit2gtk-4.1 and GTK 3; AppImage bundles them, deb declares them | `nsis`/`msi`; WebView2 runtime (bootstrapper); code signing for SmartScreen |
| OS-51 | `gen/schemas/macOS-schema.json` is committed; `tauri-build` writes `<platform>-schema.json` | | a build adds an untracked `linux-schema.json`; commit it or ignore it | `windows-schema.json` |
| OS-52 | window: `title_bar_style(Overlay)`, `hidden_title`, `traffic_light_position(20, 25)` behind `cfg(macos)` (`main.rs` 79-84); web pads `pl-[88px]` when `isMac` (`GraphCanvas.tsx` 432) | traffic lights inside the 64 px bar | native decorations above the bar: fine | same |
| OS-53 | `window.__M2__.platform = std::env::consts::OS` (`main.rs` 69-72); `isMac` and `kbd()` in `web/src/lib/native.ts` 14-19 | `"macos"` | `"linux"`: Ctrl labels | `"windows"` |
| OS-54 | `menu.rs` 22-37: an app menu with About, Services, Hide, Hide Others, Show All, Quit; File → "Show in Finder"; Edit → Redo `CmdOrCtrl+Shift+Z`; View → Full Screen; `CmdOrCtrl+…` accelerators | native menu bar | muda on GTK: `services`, `hide_others`, `show_all` and `fullscreen` are **unsupported** (muda's docs); the menu bar is inside the window, and the first menu would be called "music2". Convention: Quit in File, About in Help | the same items unsupported; Redo is Ctrl+Y by convention |
| OS-55 | `actions.rs` 84-94 `reveal_file`: `/usr/bin/open -R` on macOS, else `open::that(parent)`; "reveal": `open::that(dir)` | Finder selects the file | `org.freedesktop.FileManager1.ShowItems` over D-Bus (Nautilus, Dolphin, Nemo select the file), else `xdg-open` the folder; needs a desktop session | `explorer.exe /select,"<path>"` or `SHOpenFolderAndSelectItems` |
| OS-56 | dialogs: `tauri-plugin-dialog` → rfd (`Cargo.lock`: rfd on gtk-sys) | NSOpenPanel | GTK 3 file chooser (portal optional); needs a display | IFileDialog |
| OS-57 | `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` (`main.rs` 10) | — | — | already Windows-aware |
| OS-58 | JIT: the README says the hardened runtime is off because macOS kills the app when std's kernels compile; notarizing needs `allow-jit` | | SELinux `execmem` denials on confined domains (unconfined desktop users are fine) | Arbitrary Code Guard isn't on for normal apps; Defender may scan JIT pages |

- **Step 8b (from `design-8b.md` 354-378).** "Native accelerators are only ⌘ and ⌃ chords": Mute ⌃M, Solo ⌃S, panic ⌘.,
  ⌥K. Off macOS, ⌘ becomes Ctrl, so ⌃M meets Ctrl+M and **⌃S meets Ctrl+S (Save)**, and ⌥ is Alt, which menus own on
  Windows and Linux. 8b needs a per-OS chord table before it ships menus. This belongs to the platform layer's keys and
  labels (`kbd()` is its seed), not to every component.
- **Labels.** "Show in Finder" becomes "Show in Files" or "Show in Folder" (Linux) and "Show in Explorer" (Windows). The
  web already says "Show Folder" or "Show in Folder" off the Mac (`NewSongDialog.tsx` 28, `exporting.ts` 74). Pick one.
- **WebKitGTK.** It is a different engine from WKWebView. Monaco, canvas and CSS should be checked. On some Wayland and
  NVIDIA setups WebKitGTK needs `WEBKIT_DISABLE_DMABUF_RENDERER=1` *(verify)*.

---

## 10. Audio I/O (`core/play`)

| id | piece (HEAD lines) | rule | macOS today |
|---|---|---|---|
| OS-59 | `Player::start`: `cpal::default_host().default_output_device()`, `default_output_config()`, `BufferSize` default, f32 stream on a thread of its own (`core/play/src/lib.rs` 534-580) | RENDER-6a, RENDER-31, gate 8 | CoreAudio; its IO thread is created realtime by the HAL |
| OS-60 | `M2_AUDIO=none` → `Player::silent()`, which sleeps on `Instant` (582-600) | tests, headless | |
| OS-61 | the server starts the device on first use, and an error is the caller's (`crates/server/src/graph.rs` 431-437) | | |
| OS-62 | `M2_AUDIO_CAPTURE` capture player in `crates/server/examples/headless.rs` 45-74 | e2e live3b | portable |

### Linux

- **Host choice (OS-59).** cpal 0.18.2's Linux `default_host()` is PipeWire if built with the `pipewire` feature and
  available, then PulseAudio with `pulseaudio`, else **ALSA** (`cpal/src/platform/mod.rs` 833-849). music2 enables no
  feature, so it is ALSA. On a PipeWire desktop, ALSA's `default` device goes through `pipewire-alsa`, adding a buffer
  and losing control of the quantum. A `jack` feature exists too. Enabling `pipewire` is the cleanest choice.
- **Buffer size.** `BufferSize::Fixed(n)` maps to ALSA period sizing, and on PipeWire to `node.latency = n/rate`, a
  preference the graph's quantum is clamped by. Gate 8's 128 frames needs it set.
- **Realtime.** cpal's ALSA and PipeWire callback threads are cpal's own, at **normal priority**, unless the `realtime`
  feature is on: then `audio_thread_priority` promotes them, through RealtimeKit with `realtime-dbus`. CoreAudio gives
  macOS that for free. Under RealtimeKit a client must set `RLIMIT_RTTIME`, and the priority is capped by the daemon. A
  user with `RLIMIT_RTPRIO` (the `audio` group in `limits.conf`) can call `sched_setscheduler` directly. Containers have
  neither.
- **No device.** Headless or in Docker, `default_output_device()` gives none or a `default` PCM that fails to open, and
  `start()` errors. alsa-lib also prints `ALSA lib … cannot find card '0'` to stderr. Make "no device" fall back to the
  silent player with a diagnostic, and keep `M2_AUDIO=none` for tests.
- **Build.** `alsa-sys` links `libasound`. `m2-play` sits under `m2-server`, `m2-gates`, `m2` (through m2-server) and the
  app, so even the headless server and the CLI need `libasound2-dev` and `pkg-config` to build, and `libasound2` to run.
- **Overloads.** ALSA reports an xrun as `EPIPE` on write. PipeWire counts xruns in the driver's profiler and stream info.
  JACK has an xrun callback. cpal surfaces some as stream errors.

### Windows

- **Host.** cpal's WASAPI host is **shared mode**, event-driven (`AUDCLNT_STREAMFLAGS_EVENTCALLBACK`). The engine period
  defaults to about 10 ms; `IAudioClient3` low-latency shared periods aren't used by cpal *(verify)*.
- **ASIO.** The `asio` feature needs Steinberg's ASIO SDK at build time, under its licence. Producers on Windows expect
  ASIO.
- **Realtime.** MMCSS: `AvSetMmThreadCharacteristicsW(L"Pro Audio")`, which cpal's `realtime` feature applies on WASAPI.
- **OS-60, timers.** The default timer resolution of 15.6 ms makes `thread::sleep` periods jitter. Use
  `CreateWaitableTimerExW(CREATE_WAITABLE_TIMER_HIGH_RESOLUTION)` or `timeBeginPeriod(1)`.

---

## 11. Build, toolchain and packaging

| id | piece | Linux | Windows |
|---|---|---|---|
| OS-63 | `std/build.sh` (POSIX `sh`; `cargo build --target wasm32-unknown-unknown`; `ls -l`) | works | needs Git Bash or WSL, or a `.ps1` twin |
| OS-64 | **`std.wasm` embeds host paths:** `strings` finds `<repo>/core/kernel/src/lib.rs`, `/rustc/b940084d7…/library/…` (rustc 1.99.0, the local toolchain) and `src/fx.rs` | a rebuild elsewhere, or with another rustc, gives other bytes, hence other kernel hashes: RENDER-25's shape (kernels start from init) and RENDER-2's package-content provenance change | same |
| OS-65 | no `rust-toolchain.toml`; edition 2024; `File::try_lock` (stable since 1.89) and let-chains set the minimum; local `rustc 1.99.0` | pin 1.99.0, plus `wasm32-unknown-unknown`, for CI and reproducible `std.wasm` | same |
| OS-66 | system crates in `Cargo.lock`: `alsa-sys` (cpal); for the app only, `gtk-sys`, `webkit2gtk-sys`, `soup3-sys`, `javascriptcore-rs-sys`, `libappindicator-sys` (tray-icon), `libdbus-sys` (tao), `gdkx11-sys`, `gdkwayland-sys`; `rquickjs-sys` (cc) everywhere | headless: `cargo test --workspace --exclude music2-desktop` needs only `libasound2-dev`, `pkg-config` and a C compiler; the full workspace also needs `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libdbus-1-dev` | MSVC build tools; `webview2-com-sys`; whether QuickJS builds with cl.exe *(verify)* |
| OS-67 | npm lockfiles (`web/`, `e2e/`) | they list `linux-arm64-gnu`, `linux-x64-gnu` and musl bindings for rolldown, tailwind oxide, lightningcss, the Tauri CLI and TypeScript 7's native compiler, so `npm ci` works on Linux | `win32-x64-msvc` and `win32-arm64` too |

- **OS-64's fix.** Add `--remap-path-prefix=<repo>=.` (and `$CARGO_HOME`) to `std/kernels-src/.cargo/config.toml`, pin
  the toolchain, and treat the committed `std.wasm` as the artifact. CI may rebuild it only to check that it reproduces.

---

## 12. Tests and the browser suites

| id | piece (HEAD lines) | Linux | Windows |
|---|---|---|---|
| OS-68 | unix-only APIs outside `cfg(unix)`: `core/session/src/tx/tests.rs` 660-661 and 842 (symlink), `tx/hook_tests.rs` 5 and `tx/kill_tests.rs` 9 (`ExitStatusExt`, `libc::SIGABRT`), `crates/server/src/graph/preview_tests.rs` 5 (`PermissionsExt`), `crates/server/src/export.rs` 368 (symlink), **`crates/gates/src/server.rs` 1019 (non-test code)**, `crates/gates/src/server/watch.rs` 22-28 (`libc::clock_gettime`) | compile | **don't compile**; the gates binary doesn't either |
| OS-69 | permission-denial tests: `gate_tests.rs` 185-205 (0o000), `core/song/src/data/tests.rs` 392-398, `crates/editor/src/data_tests.rs` 443-452, `preview_tests.rs` 173-180 (`.music2/tx/` 0o500), `crates/gates/src/server.rs` 1025-1033, `e2e/g11-play.mjs` 78-128 | **fail as root**, since root ignores mode bits (`CAP_DAC_OVERRIDE`). Docker runs as root by default, so run CI as a non-root user | no mode bits; the read-only attribute is ignored on directories; would need ACLs |
| OS-70 | symlink tests (mostly `cfg(unix)`) | fine | `CreateSymbolicLinkW` needs Developer Mode or admin |
| OS-71 | `std::env::temp_dir()` everywhere | `/tmp`, case-sensitive (tmpfs or overlayfs); the `folds` branches (`tx/tests.rs` 852) take the other side, so the case-insensitive paths go untested on Linux CI (a casefold ext4 dir or a vfat loop mount needs root to make) | `%TEMP%`, NTFS, case-insensitive |
| OS-72 | kill tests `abort()` children hundreds of times (`kill_tests.rs` 102-160, `stage_tests.rs` 41, `hook_tests.rs` 377-394) | on hosts with `systemd-coredump` or `apport`, each abort writes a core dump: slow, and it fills `/var/crash`. Set `RLIMIT_CORE` to 0 (or `PR_SET_DUMPABLE` 0) in the child | WER dialogs or dumps; `abort` exits with code 3, no signal |
| OS-73 | `e2e/chrome.mjs` 5-7 and `e2e/run.mjs` 120-123: `~/.cache/puppeteer/chrome-headless-shell/mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64/…` | **133.0.6943.141 has linux64 only, no linux-arm64** (Chrome for Testing's `133.0.6943.141.json`). linux-arm64 appears from **milestone 153** (`latest-versions-per-milestone-with-downloads.json`; stable today is 155.0.8059.79). So the path per platform, and for an arm64 container a pin ≥ 153 | `win64` exists |
| OS-74 | suites inject `__M2__ = { native: false, platform: "macos" }`; `isMac` then comes from `navigator.platform` (`web/src/lib/native.ts` 16) | on Linux that is "Linux aarch64" or "Linux x86_64", so `isMac` is false and Mac strings fail, e.g. `export.mjs:65` expects "⌘E" in a tooltip. **Fix:** let `isMac` follow `__M2__.platform` whenever it is given, a one-line change | same |
| OS-75 | `Meta+…` chords in suites (`code.mjs`, `notes.mjs`, `export.mjs` …) | work, since the UI takes `metaKey \|\| ctrlKey` | same |
| OS-76 | fonts from Google Fonts (`web/index.html` 9; `--font-sans: "Inter", …, -apple-system`) | needs the network in CI. Without it the fallback fonts differ by OS, and so do text metrics and layout checks. Self-hosting the two woff2 fonts in `web/` makes rendering the same everywhere, and offline | same |
| OS-77 | `e2e/baseline.sh`: `shasum -a 256` | `shasum` comes with `perl`, which slim images lack; use `sha256sum` (coreutils), same output | Git Bash has both |
| OS-78 | `e2e/run.mjs` kills with `SIGTERM`/`SIGKILL`, scratch in `os.tmpdir()` | fine | no signals; `taskkill /T` |
| OS-79 | Chrome's runtime deps in a container | nss, atk, cups, drm, xkbcommon, gbm, pango, cairo, asound, plus fonts (`fonts-dejavu-core` or `fonts-liberation`); `--no-sandbox` is already passed | — |

### 12.1 Tests expected to fail on Linux at HEAD (not run; from the code)

| test | why | fixed by |
|---|---|---|
| `crates/server/src/graph/watch_tests.rs`: line 38 (`stats(&graph).events`, "a temp folder is on a local volume"), `own_commits_make_no_batch_with_events` (67), `events_that_miss_changes_give_way_to_walks` (229-240) | there is no event source off macOS | OS-20 (inotify) |
| `core/plugins/tests/main_thread.rs` 78, 82 | `process_main` is always false off macOS | OS-24 (`gettid() == getpid()`) |
| `core/session/src/tx/hook_tests.rs` `a_commit_that_fails_after_its_preview_accepts_nothing` (148) | without `Swap`, the second rename into the project is the fallback's, not `extra.json` | OS-03 (`renameat2`) |
| `core/session/src/gate_tests.rs` `fixture_renders_change_only_with_stds_version`, likely for `examples/resample` | host libm (OS-45) | `libm` crate, or recording per OS |
| everything in OS-69, when run as root | root ignores modes | a non-root CI user |
| the gates binary's G11 watcher check "watched by FSEvents (a local volume)" (`crates/gates/src/server/watch.rs` 164) | no events | OS-20, renamed "watched by events" |

---

## 13. Gates and measurements

| id | piece | Linux | Windows |
|---|---|---|---|
| OS-80 | `thread_cpu()`: `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` (`crates/gates/src/server/watch.rs` 22-28) | same | `QueryThreadCycleTime`; `GetThreadTimes` only moves per clock tick |
| OS-81 | `barriers()`: `sync_all` timed, called "F_FULLFSYNC" (`crates/gates/src/server.rs` 826-840; README G11) | `fsync`, and the label becomes "the durable sync" | `FlushFileBuffers` |
| OS-82 | **WT** `crates/gates/src/store.rs` 151-178 `memory()`: `task_info(TASK_VM_INFO)` → phys_footprint, virtual_size, the address-space span | `/proc/self/status` (`VmRSS`, `RssAnon`, `VmSwap`, `VmSize`), `/proc/self/smaps_rollup` (`Pss`). The span is the arch's user VA: 128 TiB on x86_64 (47-bit), 256 TiB on arm64 with 48-bit VA, **512 GiB with a 39-bit VA kernel**. Also `RLIMIT_AS` | `GetProcessMemoryInfo` (`PrivateUsage`), `GlobalMemoryStatusEx.ullTotalVirtual` (128 TiB) |
| OS-83 | **WT** `store.rs` 286-291: `pthread_set_qos_class_self_np(USER_INTERACTIVE)` | no QoS classes; `sched_setaffinity` to a big core (`/sys/devices/system/cpu/cpu*/cpu_capacity`) | `SetThreadPriority`; avoid EcoQoS |
| OS-84 | **WT** recorded baselines `crates/gates/store-S.txt` and `callback-S.txt` (M1 Pro) | machine-specific: a Linux run needs its own | same |
| OS-85 | the reference Mac (an M1) for every timing gate (contract, Acceptance gates) | Linux timings are informational unless the contract adds a Linux reference | same |

**wasmtime's address space on Linux (WT, RENDER-32).** Step 7 measured one renderer reserving 24,611 GiB for 6,058
kernel instances (4 GiB plus guards per memory), 37.9% of macOS's 131,068 GiB. Two Linux-only limits apply:
- the VA itself: 128 TiB on x86_64 is the same order, but a **39-bit arm64 kernel (512 GiB) can't hold even one p50
  renderer**;
- **`vm.max_map_count`**: each memory is at least two mappings (accessible plus guard), so two or four renderers side by
  side make 24,000 to 48,000 mappings, plus the rest of the process. The upstream default is 65,530. Fedora 39+,
  Ubuntu 24.04+ and Arch raised it to 1,048,576; Debian 12 and Docker Desktop's VM may not have *(check
  `/proc/sys/vm/max_map_count`)*.

`vm.overcommit_memory=2` or a `ulimit -v` breaks the 4 GiB reservations outright. That argues for the bounded
reservation option step 7 measured, or the pooling allocator, on Linux.

---

## 14. What the next steps add (from the designs; for the layer to own once)

Step 8a (`design-8a.md` §5.4, §6.3, §7, R4, R10), with each macOS piece and its counterparts:

| macOS piece | Linux | Windows |
|---|---|---|
| Mach semaphore, to sleep and wake workers | futex (std's park) or eventfd | `WaitOnAddress` or an event |
| `thread_policy_set(THREAD_TIME_CONSTRAINT_POLICY)` at the device period | `SCHED_FIFO`/`SCHED_RR` through RealtimeKit (`MakeThreadRealtime`; `RLIMIT_RTTIME` required) or `RLIMIT_RTPRIO`; PipeWire's `module-rt` defaults to priority 88, nice −11 | MMCSS "Pro Audio" (`AvSetMmThreadCharacteristicsW`, `AvSetMmThreadPriority`) |
| audio workgroup (`kAudioDevicePropertyIOThreadOSWorkgroup`, `os_workgroup_join`, `os_workgroup_max_parallel_threads`) | **nothing**: affinity to one L2 cluster from `/sys/devices/system/cpu/cpu*/cache/index2/shared_cpu_list`; the thread count is the design's to choose | Real-Time Work Queues (`RtwqLockSharedWorkQueue("Pro Audio")`) *(verify)* |
| `hw.perflevel0.physicalcpu`, `cpusperl2` | `cpu_capacity` (Arm big.LITTLE); `/sys/devices/cpu_core/cpus` and `cpu_atom/cpus` (Intel hybrid) | `GetSystemCpuSetInformation` (`EfficiencyClass`), `GetLogicalProcessorInformationEx` |
| `QOS_CLASS_UTILITY` for renders beside playback | `SCHED_BATCH` or nice 10; `SCHED_IDLE` | lower priority plus EcoQoS (`ThreadPowerThrottling`) |
| `FPCR` checks | FPCR on aarch64, **MXCSR on x86_64** | the same by architecture |
| CoreAudio overload notification | ALSA `EPIPE` xruns; PipeWire xrun counters; JACK's xrun callback | none: callback gaps only |
| `BufferFrameSize` from cpal's device id | ALSA period, or PipeWire `node.latency`/quantum | the WASAPI period |
| `mach_absolute_time` | `Instant` (vDSO `CLOCK_MONOTONIC`) | QPC |

8a's "rayon, `audio_thread_priority`: not needed on macOS" holds there only. Linux needs one of them, or its own
RealtimeKit call.

Step 8b: the chord map (§9), "Show in …" labels, and file dialogs.

Step 9:

| piece | macOS | Linux | Windows |
|---|---|---|---|
| host process (HOST-3): spawn | `posix_spawn` | `posix_spawn` | `CreateProcess` |
| shared-memory rings | `shm_open` | `memfd_create`, `shm_open` | `CreateFileMapping` |
| cross-process wake | — | a futex on shared memory, or eventfd | named events |
| crash detection | pipe EOF, `waitpid` | pipe EOF, `waitpid` | process handle wait |
| plugin windows | `NSView` | X11 `Window` (XWayland under Wayland) | `HWND` |
| disk streaming (RENDER-30) | `mmap`; `madvise`, `F_RDADVISE`, `F_NOCACHE` | `posix_fadvise`, `readahead` | `FILE_FLAG_SEQUENTIAL_SCAN` or `NO_BUFFERING` |
| the RENDER-30 cache folder | OS-37's folder | OS-37's folder | OS-37's folder |
| MIDI input | CoreMIDI | ALSA sequencer or rawmidi, or PipeWire MIDI | WinMM or Windows MIDI Services |
| audio input permission | microphone TCC prompt | PipeWire portal (in sandboxes) | privacy settings |

---

## 15. Linux CI in a container on this Mac

Facts here: Docker CLI 29.1.5 and `Docker.app` are installed, and the daemon isn't running; the machine is an M1 Pro with
32 GB on macOS 26.6.2; the local toolchain is rustc 1.99.0 with `wasm32-unknown-unknown`; Node is v24.19.0.

**What the owner does once:**
1. Start Docker Desktop (`open -a Docker`) and accept its terms.
2. In Settings → Resources, give it CPUs and memory for a workspace build: 6+ CPUs and 12-16 GB.
3. Set the disk image size knowingly: scratch cargo targets once filled 390 GB here (memory note).
4. Optional: Settings → General, "Use Rosetta for x86_64/amd64 emulation", for occasional `--platform linux/amd64` runs
   that stand in for an x86 server.

**How the container should run** (for the design to script):
- **arm64 Linux, native.** A Debian or Ubuntu base, rustup with a pinned 1.99.0 and `wasm32-unknown-unknown`, Node 24,
  `pkg-config`, `libasound2-dev`, a C compiler, and for the full workspace the GTK and WebKit packages of OS-66.
- **Non-root user**, so OS-69 passes, and `ulimit -c 0` (OS-72).
- **No bind-mounted build state.** The checkout goes in by `git archive HEAD | docker run -i … tar -x` (the working tree
  stays untouched) or a read-only bind mount copied inside. `CARGO_TARGET_DIR`, `CARGO_HOME` and `TMPDIR` live on named
  volumes on the VM's own ext4 disk, so `flock`, `renameat2`, `fsync` and inotify behave as on a Linux server. Whether
  VirtioFS bind mounts support `flock` and `RENAME_EXCHANGE` isn't documented, so don't rely on them.
- **`M2_AUDIO=none`.** There is no sound device. A server opened without it fails at the first play (OS-61).
- **Browser suites.** Build `web/dist` and the headless server inside. Install chrome-headless-shell **linux-arm64 at
  milestone 153 or later** (Chrome for Testing; `npx @puppeteer/browsers install chrome-headless-shell@<ver>`), with
  Chrome's deps and fonts, and set `E2E_CHROME`. Fix OS-74 (and ideally OS-76) first, or the platform-string checks fail.
  An amd64 container under Rosetta can use the pinned 133 `linux64` build, slowly.
- **Check `vm.max_map_count`** in the VM before step 7's gates (§13). It is a VM-wide sysctl, so raise it from a
  privileged container if needed.
- **Clean up** the named target volumes after each workflow (memory note).
- **What it can't check:** realtime scheduling, gate 8, plugin GUIs, case-insensitive paths (OS-71), and the desktop
  app's window (no display). `xvfb-run` could smoke-test the app's launch *(optional)*.

---

## 16. Already portable (no work, or a label only)

- `Instant` and `now_ns()` (`core/play/src/lib.rs` 52-55); `SystemTime` journal ids; `getrandom` tokens.
- The server binds `127.0.0.1:0`. The Tauri capability allows `http://127.0.0.1:*` and `http://localhost:*` on every
  webview.
- `create_new` (`O_EXCL` or `CREATE_NEW`) for assets and journals; `std::fs::rename` for the plain renames.
- Paths in the format are `/`-separated and parsed by `DataPath`. `\` is refused (`files.rs` tests). Windows device names
  in keys are already handled (ID-9, `core/song/src/key.rs` 14-17).
- The spelling checks by listing: script imports (`core/script/src/lib.rs`), `Snapshot`, and the editor's
  "free in any capitals" checks.
- The settle rule, the walk fallback, `M2_WATCH=walk`, and the racy 2 s window (FMT-16), which covers FAT's 2 s mtime.
- The no-alloc and no-lock trap tests: `GlobalAlloc`, plus grep tests over `src/audio.rs` and `core/play`.
- The wasmtime configuration (relaxed SIMD off, the non-finite silencing), and `std.wasm`'s own math.
- `windows_subsystem`, `icon.ico`, and `CLAP_PATH` parsing through `split_paths`.

---

## 17. What the survey implies for the layer (input to its design, not the design)

The OS-specific code at HEAD falls into **seven small seams**. Everything else is policy that is already portable and
should stay where it is:
1. **fs:** push, barrier, `rename(Swap | Excl)`, clone, directory sync, file id `(volume, index)` and change time, stored
   spelling, and a per-volume capability probe (network? case-insensitive? exchange, noreplace, clone, hard links?). It
   serves OS-01…07, 10, 11, 15-19 and 42. The `Disk` trait is already the seam for the commit; the rest are free
   functions.
2. **lock:** exclusive lock with a readable holder (one byte past EOF on Windows, and network caveats), `cannot_lock`
   codes, machine id, host name and local clock. It serves OS-08, 09, 12-14.
3. **watch:** an event source with `start(dir, wake) / take / put_back / flush / events`: FSEvents, inotify,
   `ReadDirectoryChangesW`. It serves OS-20…22.
4. **threads:** `is_process_main`, `thread_token`, the main-thread executor as `post` plus `is_main` installed by each
   program, Linux CLAP `timer-support` and `posix-fd-support` from that loop, and `leave_now`. It serves OS-23…31. 8a adds
   realtime promotion, sleep and wake, CPU topology, QoS and FP-mode checks here.
5. **dirs:** home with its test override, songs, cache, session file, bundled resources, CLAP search paths. It serves
   OS-32…40.
6. **audio:** the cpal host and feature choice per OS, a fixed buffer size, realtime promotion where the OS doesn't give it,
   and "no device" falling back to silent. It serves OS-59…61.
7. **shell:** the Tauri bundle targets and per-OS menu layout, the chord map and labels, reveal-in-file-manager, and the
   web's `isMac` source. It serves OS-50…58 and OS-74.

Plus three one-time fixes that aren't layers:
- the `libm` crate for render-path math (OS-45);
- reproducible `std.wasm` and a pinned toolchain (OS-64, OS-65);
- tests made OS-honest: `cfg(unix)` where they need it, a non-root CI user, `RLIMIT_CORE`, and one place for the browser
  path (OS-68…79).

Contract text that names macOS mechanisms and needs generic wording when the layer lands:
- PLANE-5 (`F_FULLFSYNC`);
- WRITE-15 Today (`RENAME_SWAP`, `RENAME_EXCL`);
- WRITE-17 Today (`MNT_LOCAL`);
- WRITE-19 Today (FSEvents, "this Mac's writes");
- FMT-16, FMT-20 and RENDER-30 (`~/Library/Caches`, "hardware UUID");
- HOST-2 and HOST-3 (AppKit, `CFRunLoop`, dispatch queue);
- the acceptance gates' "reference Mac", which stays: macOS remains the judged platform.

---

## Sources

Kernel and libc:
- rename(2), renameat2 flags and per-fs NOREPLACE support: https://man7.org/linux/man-pages/man2/rename.2.html
- Rename flag masks in Linux v6.12:
  - overlayfs: https://raw.githubusercontent.com/torvalds/linux/v6.12/fs/overlayfs/dir.c
  - NFS: https://raw.githubusercontent.com/torvalds/linux/v6.12/fs/nfs/dir.c
  - FUSE: https://raw.githubusercontent.com/torvalds/linux/v6.12/fs/fuse/dir.c
  - SMB client: https://raw.githubusercontent.com/torvalds/linux/v6.12/fs/smb/client/inode.c
  - 9p: https://raw.githubusercontent.com/torvalds/linux/v6.12/fs/9p/vfs_inode.c
- vfat RENAME_EXCHANGE patch series (2022): https://lkml.rescloud.iu.edu/2205.2/05418.html
- flock(2) on NFS and SMB, and open file descriptions: https://man7.org/linux/man-pages/man2/flock.2.html
- fsync(2), disk cache and directory sync: https://man7.org/linux/man-pages/man2/fsync.2.html
- syncfs(2): https://man7.org/linux/man-pages/man2/syncfs.2.html
- inotify(7), limits, overflow, races, remote filesystems: https://man7.org/linux/man-pages/man7/inotify.7.html
- inotify `max_user_watches` RAM-scaled default patch: https://lkml.iu.edu/hypermail/linux/kernel/2010.3/01777.html
- `vm.max_map_count` raised to 1,048,576:
  - Fedora: https://fedoraproject.org/wiki/Changes/IncreaseVmMaxMapCount
  - Arch: https://archlinux.org/news/increasing-the-default-vmmax_map_count-value/
- PipeWire module-rt (RealtimeKit and rlimits): https://manpages.debian.org/unstable/libpipewire-0.3-modules/libpipewire-module-rt.7.en.html

Rust, Windows and Chrome:
- Rust `File::lock` and `try_lock` platform mapping: https://doc.rust-lang.org/std/fs/struct.File.html
- Windows LockFileEx (mandatory locks, locking past EOF): https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex
- Chrome for Testing:
  - availability, linux-arm64 for current Stable: https://googlechromelabs.github.io/chrome-for-testing/
  - 133.0.6943.141, no linux-arm64: https://googlechromelabs.github.io/chrome-for-testing/133.0.6943.141.json
  - first milestone with linux-arm64: https://googlechromelabs.github.io/chrome-for-testing/latest-versions-per-milestone-with-downloads.json

CLAP, Docker and local crates:
- CLAP search paths, recursion, DSO or bundle: https://raw.githubusercontent.com/free-audio/clap/main/include/clap/entry.h
- Docker Desktop VirtioFS: https://www.docker.com/blog/speed-boost-achievement-unlocked-on-docker-desktop-4-6-for-mac/
- Local crate sources:
  - `cpal-0.18.2`: `src/platform/mod.rs` 833-849, `src/host/pipewire/device.rs` 178-181 and `rt_promote.rs`,
    `src/host/wasapi/device.rs` 55, 196
  - `tauri-utils-2.10.1`: `src/platform.rs` 257-340
  - `muda-0.20.0`: `src/items/predefined.rs`
  - `clap-sys-0.5.0`: `src/ext/{gui,posix_fd_support,timer_support}.rs`
  - `libc-0.2.190`: `renameat2`, `RENAME_*`, `gettid`, `FICLONE`, `syncfs`, `*_SUPER_MAGIC`
