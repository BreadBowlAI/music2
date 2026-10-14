# The platform layer: the smallest design

Label `design-small`. Written 2026-10-14 on `jerry-ax102`, against `main` at `f126d03`. Angle: the least code that
makes Linux green and keeps macOS identical (except the one deliberate `libm` change), with one obvious place per OS.

Inputs: `docs/architecture.md` (normative), `survey-code.md` (OS-01…OS-85), `survey-later.md`, and the ground notes
`ground/{tests,e2e,facts,branches,ci}.md`, which outrank the surveys. From `ci.md` only the Mac check's contents and the
Linux plugin facts are used: there is no GitHub Actions workflow in this step.

Three new probes were run for this design (§A at the end): checking and linting the macOS code from Linux, the Linux
plugins headless, and `libm::exp2` against `libm::pow` at the fixture's pitches. Anything not checked on this machine or
in the source is marked *(unverified)*.

---

## 1. In short

**What changes**

| What | In plain words |
|---|---|
| Linux on this server becomes fully green | Every Rust test, every lint, the desktop app's build and tests, all 29 browser runs, and the same exported audio as the Mac. (One browser run also uses two plugins; it still has to be tried with them here.) |
| One new small part, `m2-os` | All code that differs between a Mac and Linux moves into it: one file for the Mac, one for Linux. Later steps add their per-OS code there. |
| The Mac's code moves, but works the same | Nothing it does changes. The Mac code is only moved into `m2-os`. |
| One deliberate sound change | Exported songs with audio clips (pitch, fades, stretching) change in the very last bit of a few samples. You can't hear it. After that, the Mac and Linux export exactly the same bits. |
| One check command, `ci/check.sh` | The same command checks everything, on the server and on your Mac. It ends with a short summary. |

**What you will notice**
- In the app on your Mac: nothing.
- An export of a song with audio clips gives a file with other bytes once. It sounds the same.
- On the server, lock messages show the server's name, `Ubuntu-2404-noble-amd64-base` (question Q4).

**What you do once, at the end of this step** (about 30–40 minutes of the Mac's time, mostly waiting)

| # | Do this on your Mac | |
|---|---|---|
| 1 | Open Terminal. Type `cd ~/dev/suite2/music2` and then `git pull`. | |
| 2 | Type `ci/check.sh` and press Return. | The first run may download Rust 1.99.0 and the test browser by itself. |
| 3 | Copy the last lines it prints (from `music2 check:` to `RESULT:`) and paste them to me. | |

**When I will ask you for a Mac run:** at the end of this step, at the end of step 7, and before merging `taste`. Never
for work that only changes Linux. Until you run it, the Mac is written down as "unverified since <commit>".

---

## 2. Where the layer lives, and its shape

### 2.1 One small crate, one file per OS

```
core/os/                      crate m2-os: everything music2 does differently on each OS (PLAT-2)
  Cargo.toml                  depends on libc only
  src/lib.rs                  picks the OS file; the shared type Heard; the rule in its doc comment
  src/macos.rs                every macOS call: libc, CoreFoundation, CoreServices, libdispatch
  src/linux.rs                every Linux call: libc (renameat2, statfs, inotify, gettid, prctl)
  src/tests.rs                one set of tests, run on both OSes
  tests/one_place.rs          fails if `target_os` appears outside core/os and the desktop shell
```

About 600 lines in all. About 300 of them are today's macOS code, moved unchanged (FSEvents, the main dispatch queue,
`renamex_np`, `clonefile`, `F_FULLFSYNC`, `statfs`, `gethostuuid`, `getattrlist`). The new Linux code is about 300
lines, two thirds of it the inotify source.

When one OS file passes about 600 lines (8a and step 9 will grow them), it becomes a folder `macos/` or `linux/` with
the same module names. Not before.

### 2.2 The public API

`lib.rs`:

```rust
//! m2-os: everything music2 does differently on each OS (PLAT-2). One file per OS defines every
//! item listed in this doc comment, with the same names and signatures. Code outside this crate
//! never names an OS. Depends on libc only.

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod os;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("music2 builds on macOS and Linux; Windows comes in its own step (PLAT-1)");

pub use os::*;

/// What a file-system event source heard (WRITE-19). Names are hints, never facts.
pub enum Heard<'a> {
    /// Something at this path, or under it, changed. Absolute, spelled as the source spells it.
    Path(&'a std::path::Path),
    /// Events were lost, or the watched folder itself moved or went: only a walk tells.
    Rescan,
}
```

What each OS file defines (the signatures are the contract between the two files):

```rust
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ── files and volumes (WRITE-15, WRITE-17, WRITE-19) ──────────────────────────────────────────
/// Exchange `a` and `b`; both exist. `Unsupported` when this volume can't.
pub fn rename_swap(a: &Path, b: &Path) -> io::Result<()>;
/// Rename `from` to `to` only where nothing is (`AlreadyExists` otherwise). `Unsupported` when
/// this volume can't.
pub fn rename_excl(from: &Path, to: &Path) -> io::Result<()>;
/// Make everything pushed (`fsync`ed) before this call durable: a commit's one barrier.
pub fn barrier(file: &File) -> io::Result<()>;
/// A clone of `from` at the new path `to` that shares no inode, synced. `Unsupported` when this
/// volume can't clone (the caller copies).
pub fn clone_file(from: &Path, to: &Path) -> io::Result<()>;
/// Whether the volume holding `dir` is local to this machine: not a network or FUSE volume.
pub fn local_volume(dir: &Path) -> bool;
/// The last name of `p` as the disk stores it, when the OS can tell; None otherwise.
pub fn stored_name(p: &Path) -> Option<OsString>;

// ── identity and folders ────────────────────────────────────────────────────────────────────
/// This machine's lasting id: the hardware UUID (macOS), `/etc/machine-id` (Linux).
pub fn machine_id() -> Option<String>;
/// Where music2 keeps what it can rebuild (FMT-16, RENDER-30). Read from `$HOME` only, so a
/// test's scratch HOME isolates it.
pub fn cache_dir() -> Option<PathBuf>;
/// The OS's CLAP folders, the user's first. `CLAP_PATH` comes before them (m2-plugins).
pub fn clap_dirs() -> Vec<PathBuf>;

// ── threads (HOST-2, RENDER-28) ─────────────────────────────────────────────────────────────
/// The calling thread's identity: a register read, never a thread-local. Allocates nothing.
pub fn thread_token() -> usize;
/// Whether the calling thread is the process's main thread. No allocation, no lock.
pub fn is_process_main() -> bool;
/// Queue `job` on the process's main loop. Never blocks.
pub fn post_to_main(job: Box<dyn FnOnce() + Send>);
/// Main thread only: run queued jobs for at most `at_most`; return after any ran.
pub fn run_main_loop(at_most: Duration);
/// Whether the desktop app's toolkit runs that main loop by itself (macOS: AppKit drains the
/// main dispatch queue). False on Linux until step 9 (§9).
pub const APP_RUNS_MAIN_LOOP: bool;

// ── file-system events (WRITE-19) ───────────────────────────────────────────────────────────
pub struct Events { /* per OS */ }
impl Events {
    /// Watch `root` (canonical) and every folder under it whose path relative to `root`
    /// `enter` allows. `heard` runs on the source's own thread, or inside `flush`. An error
    /// says why events won't start; the caller then walks.
    pub fn start(root: &Path, enter: fn(&str) -> bool, heard: Box<dyn Fn(crate::Heard<'_>) + Send + Sync>)
        -> io::Result<Events>;
    /// Return once every event that happened before this call has been given to `heard`.
    pub fn flush(&self);
}
// Drop: stops watching; no `heard` runs once drop has returned.

// ── tests ───────────────────────────────────────────────────────────────────────────────────
/// In a child process about to crash on purpose: no core dump, no crash report.
pub fn no_crash_report();
```

Nothing else. Step 7's merge, 8a and step 9 add their items to the same two files (§9).

### 2.3 What stays outside `m2-os`, and why

| Stays where it is | Why |
|---|---|
| Policy around each call: the commit's fallback renames (`stage.rs`), the lock's messages (`lock.rs`), the hint rules and the settle rule (`graph/events.rs`, `watcher.rs`, `watch.rs`), the plugin executor (`main_thread.rs`) | It is the same on every OS. Only the system calls under it differ. |
| `cfg(unix)` code that is the same on macOS and Linux: `push` (`fsync`), `clock` (`localtime_r`), `host_name` (`gethostname`), `cannot_lock` (errno), `Stamp::of` (ctime, inode), `is_at`, `prepare`'s `st_dev`, `file_id`, the Tx spelling lookup, `writable`, `session.json`'s mode 0600 | Moving it now adds code and diff for no gain on either OS. The Windows step moves each piece into `m2-os` when it writes the Windows twin (PLAT-2 allows this). |
| The desktop shell's OS code (`apps/music2-desktop`): the window builder's traffic lights, `reveal_file`, menus, the AppKit test | The shell is one place already, and `taste` rewrites the menus and the window builder. Editing them on `main` would make 12 conflicts (`branches.md` §3.2). |
| Tests that check one OS's mechanism (`appkit_main_thread.rs`) | They exist to test that OS. |
| What other crates already make the same: cpal (audio), std (`Instant`, `sleep`, thread parking), libloading, the `open` crate | No layer code needed (survey-later §5.1). |

### 2.4 Why this shape

**Free functions in one file per OS, not one trait with associated types.** Only one OS is compiled into a build, so a
trait has exactly one implementation. It buys no polymorphism. It costs `use m2_os::{Os, This}` and `This::call()` at
every call site, and `<This as Os>::Group` in signatures. The thing a trait would enforce, "every OS has every item",
is enforced here by compiling both files: Linux builds `linux.rs`, and the macOS lint in every Linux run
(`cargo clippy --target aarch64-apple-darwin`, §7.5, measured working) builds `macos.rs`. A caller that uses an item one
file lacks fails that build. Survey-later's associated types (`Group`, `Wake`, `Shared`, `Child`, `GuiLoop`) become a
`pub struct` of the same name in each file.

**One crate, not a `cfg` module inside each crate.** Four reasons:
1. One place to read per OS. "What is different on Linux?" is one file.
2. Later pieces are shared by several crates: the thread token (plugins, play), process memory (gates, server),
   engine threads (play, plugins in the host process). A leaf crate lets each call the same function.
3. `pub` items of a library are never dead code, so an item one OS doesn't use yet makes no clippy lint (today's 15
   Linux lints are all that kind).
4. One rule, one test (`tests/one_place.rs`): `target_os` appears only in `core/os` and the shell.

The cost is moving about 300 lines of macOS code between crates. The diff is mechanical, the Mac's behaviour is
unchanged, and none of the moved files is touched by `wip/step7`, `taste` or `sounds` (checked with `git diff
main...origin/<branch>`).

**In-tree inotify, not the `notify` crate.** The watcher needs a flush ("every event before now has been delivered").
FSEvents has `FSEventStreamFlushSync`, which is why `fsevents.rs` is in-tree today. On Linux the flush is a non-blocking
drain, because an event is queued before the writer's call returns (0 misses in 2.8 million checks, `facts.md` §3).
`notify` hides the fd, so it can't offer that. In-tree inotify is about 200 lines of `libc`.

### 2.5 The rule

PLAT-2 (§6.2): code whose behaviour differs between OSes lives in `m2-os`; outside it no code names an OS, except the
shell and its tests; `cfg(unix)` code that is the same on macOS and Linux may stay until the Windows step.
`core/os/tests/one_place.rs` checks it: it reads every `.rs` file under `core/`, `crates/` and `apps/m2/`, and fails on
`target_os` outside `core/os/`. It starts with an allow-list of today's files, and each slice removes its own lines.
The list is empty after slice S5.

---

## 3. Every OS piece (OS-01 to OS-85)

What "this step" means:

| Mark | Meaning |
|---|---|
| **Built** | New or changed code, on Linux and macOS, in this step. |
| **Moved** | Today's macOS code moved into `m2-os` unchanged; Linux gets its twin. |
| **No change** | Works on Linux as it is. |
| **Designed** | The shape is fixed here; built at the named time. |
| **Deferred to X** | Not now, with the reason. |

The Windows column is the plan in one line. Windows is designed for, built in its own step.

### 3.1 Durable writes and renames (`core/session/src/tx/stage.rs`)

| id | Piece | This step | Linux: how, durability | Windows |
|---|---|---|---|---|
| OS-01 | `push()`: `fsync` per file | No change (`cfg(unix)`, same call) | `fsync` also flushes the drive cache, so each push is durable alone. Measured: 0.03 ms for 4 KB, 0.11 ms for a new file. | `FlushFileBuffers`, a full flush each |
| OS-02 | `Real::barrier` | Moved: `m2_os::barrier` (macOS `F_FULLFSYNC`; Linux `fsync`, as today) | `fsync` of the journal: about 0.03 ms more. A whole commit: 0.56 ms p50 idle, 1.37–1.48 ms p95 with a 1 GB writer (`facts.md` §2). This NVMe has power-loss protection, so these are best case; a laptop disk is slower *(unverified)*. Never `syncfs`: it waited up to 890 ms for other programs. tmpfs makes nothing durable (tests only). | `FlushFileBuffers` on the journal |
| OS-03 | `Real::rename` `Swap`/`Excl` | Built: `m2_os::rename_swap` / `rename_excl` (macOS `renamex_np`, moved; Linux `renameat2(RENAME_EXCHANGE / RENAME_NOREPLACE)`, new) | Works on ext4 and tmpfs (measured), btrfs and xfs (survey). NFS, 9p and exFAT give `EINVAL` (survey, *unverified here*), mapped to `Unsupported` as on macOS: the fallback runs, crash-safe after S3. The commit's renames are file to file, so `EINVAL`'s other meaning (a folder into itself) can't arise there. | `MoveFileExW` without replace for Excl; NTFS has no exchange, so the fallback always; retry on sharing violations |
| OS-04 | `Real::clone_file` | Moved: `m2_os::clone_file` (macOS `clonefile`; Linux returns `Unsupported`, so the copy runs, as today) | ext4 can't clone (`FICLONE` gives `EOPNOTSUPP`, measured): a 50 MB plugin blob is copied once per commit that replaces it. `FICLONE` for btrfs/XFS: designed, deferred until such a volume can test it. | `FSCTL_DUPLICATE_EXTENTS_TO_FILE` on ReFS / Dev Drive, else copy |
| OS-05 | `sync_dir` | No change | `fsync` of a folder opened `O_RDONLY` works (measured). Some FUSE or network volumes may refuse it *(unverified)*: designed as "this volume can't", built when such a volume can be tested. | no-op: folders can't be flushed |
| OS-06 | `cpath()` | Moved into `m2-os` | — | wide strings |
| OS-07 | `name_new` (export's create-only naming) | Built: `rename_excl`, then the old look-then-rename fallback; the hard link goes | `link()` gives `EPERM` on vfat/exFAT, so today's fallback never runs there and an export to a FAT stick fails (survey). `RENAME_NOREPLACE` is the create-only name itself. On the Mac the result is the same file at the same moment. | `MoveFileExW` without replace |

### 3.2 The project lock and machine identity (`core/session/src/lock.rs`)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-08 | `File::try_lock` (`flock`) | No change | Same as macOS on local volumes (measured: per open file, freed at exit and at `SIGKILL`; a second open in one process gets `WouldBlock`). On NFS `flock` becomes a process lock, so network volumes get `lock.local` (OS-15). | `LockFileEx` one byte past the end, so the holder stays readable |
| OS-09 | `cannot_lock` codes | No change | `ENOTSUP` = `EOPNOTSUPP` (95), `ENOLCK` (37) | `ERROR_NOT_SUPPORTED`, `ERROR_INVALID_FUNCTION` |
| OS-10 | `is_at` (dev, ino) | No change (`cfg(unix)`) | same | volume serial and file index |
| OS-11 | `prepare`: `.music2/tx/` on the project's volume | No change | same; a bind mount gives `EXDEV` at the first rename, rolled back | volume serial |
| OS-12 | `host_id` | Moved: `m2_os::machine_id` (macOS `gethostuuid`); `lock.rs` keeps the host-name fallback | `/etc/machine-id` (present here). `product_uuid` is root-only. | `MachineGuid` in the registry |
| OS-13 | `host_name` | No change | `gethostname` gives `Ubuntu-2404-noble-amd64-base` here (Q4) | `GetComputerNameExW` |
| OS-14 | `clock` | No change | `localtime_r`; the server is on Europe/Berlin | `localtime_s` |
| OS-15 | `local_volume` | Built: `m2_os::local_volume` (macOS `MNT_LOCAL`, moved; Linux `statfs` `f_type`) | Network: NFS `0x6969`, SMB `0x517B`, CIFS `0xFF534D42`, SMB2 `0xFE534D42`, 9p `0x01021997`, Ceph `0x00C36400`, AFS `0x5346414F`, Coda `0x73757245`; and FUSE `0x65735546`, since sshfs and ntfs-3g can't be told apart, so FUSE gets walks and `lock.local` (safe side). ext4 `0xEF53` and tmpfs `0x01021994` are local (measured). | `GetDriveTypeW` = `DRIVE_REMOTE`, and UNC paths |

### 3.3 File identity and spelling

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-16 | `Stamp::of` (ctime, inode) in `watch.rs`, `hash.rs` | No change (`cfg(unix)`) | real ctime and inode, nanosecond times | `ChangeTime` and file index (`GetFileInformationByHandleEx`) |
| OS-17 | Tx `spelled()` real-name lookup | No change | same; ext4 names are byte-exact | `GetFinalPathNameByHandleW` |
| OS-18 | `file_id` in `render.rs` | No change | same | file index |
| OS-19 | watcher `spelled()` (`getattrlist`) | Moved: `m2_os::stored_name` (macOS `getattrlist`; Linux `None`: the name as asked) | Right on plain ext4 (case-sensitive). On vfat, exFAT or a casefold folder a case-only rename could be taken in twice. Designed: list the parent on a volume that folds case. Deferred: no such volume here, and making one needs root. | `GetFinalPathNameByHandleW` |

### 3.4 File-system events (WRITE-19)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-20 | the event source (`fsevents.rs`) | Built: `m2_os::Events`. macOS: today's FSEvents stream, moved; its flags become `Heard`. Linux: inotify, new. | One inotify instance per process, shared by every watcher in it, because the 128-instance limit is per user and shared with editors (a test run can hold one watcher per test thread, 32 here *(estimate)*, and several agents' runs share this server). A watch on each folder `enter` allows (never `.git`, `.music2`, `node_modules`). Mask: `IN_MODIFY \| IN_ATTRIB \| IN_CLOSE_WRITE \| IN_CREATE \| IN_DELETE \| IN_MOVED_FROM \| IN_MOVED_TO \| IN_DELETE_SELF \| IN_MOVE_SELF \| IN_ONLYDIR \| IN_DONT_FOLLOW \| IN_EXCL_UNLINK`. A new or moved-in folder: watch it and its folders, then hint the folder (a folder hint covers what is under it). `IN_Q_OVERFLOW` (wd −1), `IN_UNMOUNT`, and the root's `IN_MOVE_SELF`/`IN_DELETE_SELF`: `Rescan`. `IN_IGNORED`: forget the wd. Names are hints; cookies are never paired. One reader thread (normal priority) blocks in `poll` on the inotify fd and an eventfd for stop; it reads and dispatches under a mutex. `flush` takes that mutex and reads until `EAGAIN`, which is complete because an event is queued before the writer's call returns (measured). Limits here: 997,324 watches, 16,384 queued events. `mmap` writes give no event until close: the settle rule and the 5 s walk cover them. A failed `inotify_init1` or watch at start is an error (the watcher walks and says why); a failed watch later is a `Rescan`. | `ReadDirectoryChangesExW`, recursive, overlapped; flush by a sentinel write |
| OS-21 | `source()` in `watcher.rs` | Built: no OS `cfg` left; it asks `m2_os::local_volume` and `Events::start`, and walks with the reason on an error | — | same |
| OS-22 | `mod fsevents` (macOS only) | Built: `graph/fsevents.rs` becomes `graph/events.rs` on every OS: `Hints`, `MAX_HINTS`, the prefix and `walked()` filter, take, put back, the event count. Only the OS calls left it. | — | same |

### 3.5 The plugin main thread (HOST-2) and thread identity

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-23 | `Kind::Dispatch`: main dispatch queue and `CFRunLoop` pump | Moved: `m2_os::post_to_main` and `run_main_loop`. `main_thread.rs` gets one kind for both OSes, `Kind::Process` ("the process's main loop"); `Kind::Queue` stays for tests. | `post_to_main` puts the job on a process-wide channel; `run_main_loop` waits on it for at most the given time and runs what came. Under `run_main` the main thread runs it, as it drains a channel today. | a Win32 message loop (`MsgWaitForMultipleObjectsEx`) |
| OS-24 | `is_process_main()` | Built: Linux `gettid() == getpid()` | Two system calls, no allocation, no lock. `ground/tests.md` checked it: the test passes and the no-alloc tests pass. | thread id recorded at install |
| OS-25 | `token()` (`pthread_self`) | Moved: `m2_os::thread_token`. `main_thread::token` and `is_process_main` stay as re-exports, since step 7's `probe.rs` calls them 6 times. | a register read | `GetCurrentThreadId` |
| OS-26 | the app's executor (`use_process_main_thread`) | Deferred to step 9. `APP_RUNS_MAIN_LOOP` is false on Linux, so the Linux app says "plugins can't be hosted", as today. Reason: GTK's loop must also serve CLAP's `timer-support` and `posix-fd-support`, which step 9's main loop does (Q3). | — | `run_on_main_thread` or a hidden `HWND` |
| OS-27 | `run_main` off macOS | Built (the channel loop above). The CLAP timer and fd extensions: deferred to step 9. | Measured for this design: Linux Surge XT 1.3.4 and Airwindows Consolidated load, play notes and process audio headless without them (`m2-plugins` tests with `CLAP_PATH`, §A.2). | message pump |
| OS-28 | the probe's `process_main`; `core/plugins/tests/main_thread.rs` | Fixed by OS-24 | — | — |
| OS-29 | `leave_now` (`_exit`) | No change (desktop shell) | same | `TerminateProcess` |
| OS-30 | `appkit_main_thread.rs` | Built: its macOS-only items under `cfg(target_os = "macos")` (clippy lints 7–15). A Linux twin (GTK under `xvfb-run`) comes with OS-26. | prints "macOS only" | a Win32 twin |
| OS-31 | host extensions offered | No change now; `timer-support` and `posix-fd-support` come with step 9's main loop | not needed for audio here (measured, OS-27) | — |

### 3.6 CLAP discovery and loading

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-32 | `search_paths()` | Built: `CLAP_PATH` first, then `m2_os::clap_dirs()` (macOS as today; Linux `~/.clap`, `/usr/lib/clap`) | Both are missing here. Never install the plugins into `/usr/lib/clap`: `fx-fixes` needs them missing, and only `HOME` (`~/.clap`) is scratch in tests. | `%COMMONPROGRAMFILES%\CLAP`, `%LOCALAPPDATA%\Programs\Common\CLAP`; `;`-separated |
| OS-33 | `binary_in()` | No change | a Linux `.clap` is the library file (measured: loads) | a DLL |
| OS-34 | one-level scan | No change; the spec's recursive scan is the same work on every OS, deferred | — | — |
| OS-35 | `load()` | No change | `RTLD_LAZY \| RTLD_LOCAL`, works | `load_with_flags(LOAD_WITH_ALTERED_SEARCH_PATH)` |

### 3.7 Paths, folders and the bundle

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-36 | `songs_dir()` = `$HOME/Music/music2` | No change. `XDG_MUSIC_DIR`: designed, deferred (servers don't set it; the suites read `testhome/Music/music2`). | `~/Music/music2` | `FOLDERID_Music` (often OneDrive) |
| OS-37 | `cache_dir()` | Built: `m2_os::cache_dir()`. macOS unchanged. | `$HOME/.cache/music2`, from `HOME` only so tests stay isolated (`XDG_CACHE_HOME` is ignored on purpose). Today it would make `~/Library/Caches` on Linux. | `%LOCALAPPDATA%\music2\cache` |
| OS-38 | `~/.music2/session.json` (0600) | No change | same | `%USERPROFILE%\.music2` |
| OS-39 | `bundled()` resources | No change; Linux packaging deferred (Q3). A dev build finds the source tree. | a deb would put them in `/usr/lib/music2/` | the exe's folder |
| OS-40 | `source_tree()` | No change | — | — |
| OS-41 | `folder_part`, `reserved` | Built (Q1): a new song's folder name counts as taken when any entry matches it ignoring case, on every OS. The Mac is unchanged (APFS already says so). | ext4 tells cases apart, so today "exports 2" is free next to "Exports 2" | also refuse `< > " \| ? *`, trailing dots and device names |
| OS-42 | `AssetName` allows `: * ? " < > \|` | No change; a format rule for the Windows step | legal | not creatable |
| OS-43 | the web's `~` display | No change | — | `\` paths |
| OS-44 | WRITE-12 dot files | No change; an editor's `name~` backup refusing a folder Rename is deferred | — | `Thumbs.db`, `desktop.ini` |

### 3.8 Floating point and render determinism (§5)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-45 | host math in `core/runtime/src/audio.rs` (8 calls) | Built: the `libm` crate, with a test that bans host math there | §5.1 | same |
| OS-46 | the kernels' math | No change: inside `std.wasm` it is compiler-builtins' libm, equal to the `libm` crate bit for bit (measured) | same | same |
| OS-47 | `core/play/src/resample.rs` (live only) | Built: `libm::sin`/`cos` too, so one rule covers all host audio | its f32 tables came out the same under glibc and `libm` (measured) | same |
| OS-48 | wasmtime config (NaN canonicalization off, relaxed SIMD off) | No change | wasm floats are exact on x86_64 and arm64 except NaN payloads, which the renderer silences | same |
| OS-49 | denormals | Deferred to 8a: a gate-8 item on x86 (§5.5) | x86 pays a microcode assist per subnormal | same as x86 |

### 3.9 The desktop shell (Tauri)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-50 | `bundle.targets: ["app"]` | Deferred (Q3) | a deb needs webkit2gtk-4.1, GTK 3, libsoup 3, ALSA, glibc ≥ 2.39 as built here (`facts.md` §9) | NSIS or MSI; WebView2 |
| OS-51 | `gen/schemas/<os>-schema.json` | Built: commit `linux-schema.json`, so a Linux build leaves the tree clean | — | `windows-schema.json` |
| OS-52 | traffic lights, `pl-[88px]` | No change (`taste` rewrites the window builder) | native title bar above the top bar: fine | same |
| OS-53 | `__M2__.platform` | No change | `"linux"` | `"windows"` |
| OS-54 | menus (`menu.rs`) | Deferred to the key table on `taste` (8b): a menu per OS. Don't touch `menu.rs` on `main`. | GTK greys out Services, Hide, Hide Others, Show All, Full Screen (muda source) | the same items; Redo is Ctrl+Y |
| OS-55 | `reveal_file`; "Show in Finder" labels | No change in Rust (Linux opens the folder; `gio` is the fallback since `xdg-open` is missing here). One label per OS goes with the key table on `taste`. | `FileManager1.ShowItems` over D-Bus would select the file: designed | `explorer /select,` |
| OS-56 | dialogs (rfd) | No change | GTK file chooser; needs a display | `IFileDialog` |
| OS-57 | `windows_subsystem` | No change | — | already there |
| OS-58 | JIT and the hardened runtime | No change | SELinux `execmem` only in confined domains | no change |

### 3.10 Audio I/O (`core/play`)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-59 | `Player::start` (cpal) | No change; 8a owns the device (host, buffer size, realtime) | cpal's only host here is ALSA. This server has no sound card: the default device exists, but its config fails ("not available"), and alsa-lib prints 8 lines. No rtkit, `RLIMIT_RTPRIO` 0: realtime is impossible here as set up. | WASAPI shared; MMCSS |
| OS-60 | `M2_AUDIO=none` | No change; every check sets it | — | high-resolution timer |
| OS-61 | device started on first use | No change now. Designed for 8a: "no device → silent player with a note" must also catch a failing `default_output_config` (measured). | — | — |
| OS-62 | the capture player (live3b) | No change | — | — |

Not an OS piece, found by survey-later: one xrun marks the player stalled (bug 1). It is the same on every OS. It is
fixed first thing when step 7 is finished, because step 7's export `Hold` needs a player that one xrun doesn't retire.

### 3.11 Build, toolchain and packaging

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-63 | `std/build.sh` | Built: copy from `${CARGO_TARGET_DIR:-target}` (it breaks when a target dir is set, as on this server) | — | a `.ps1` twin or Git Bash |
| OS-64 | `std.wasm` holds the builder's path | Built: `m2-kernel` moves into the kernels' workspace; one rebuild (§5.3) | measured path-independent | same |
| OS-65 | no pinned toolchain | Built: `rust-toolchain.toml` | already 1.99.0 here | same |
| OS-66 | system libraries | No change; present here (`facts.md` §9). A fresh Ubuntu 24.04 needs `pkg-config libasound2-dev libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libdbus-1-dev build-essential`, plus `clang` for the macOS lint's stub | — | MSVC tools, WebView2; QuickJS under `cl.exe` *(unverified)* |
| OS-67 | npm lockfiles | No change | Linux bindings listed | Windows bindings listed |

### 3.12 Tests and the browser suites

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-68 | unix-only APIs outside `cfg(unix)` | No change (they compile on both) | — | fix in the Windows step |
| OS-69 | permission tests | No change; never run checks as root | runs as `jerry` | ACLs |
| OS-70 | symlink tests | No change | — | Developer Mode |
| OS-71 | `temp_dir()` is case-sensitive | No change. The case-folding paths are tested on the Mac only: a casefold or vfat volume needs root here. | — | NTFS folds case |
| OS-72 | core dumps from the kill tests | Built: `m2_os::no_crash_report()` in `kill_tests::child` and `hook_tests::hook_child` (Linux `prctl(PR_SET_DUMPABLE, 0)`; macOS does nothing, as today) | apport takes ~1.1 s per dump even with `ulimit -c 0`; the kill test drops from 473 s to 104 s (measured) | WER off for the child |
| OS-73 | Chrome path (`chrome.mjs`, `run.mjs`) | Built: per OS in `chrome.mjs`; `run.mjs` imports it | `linux64` 133.0.6943.141 here; no linux-arm64 build of 133 exists | `win64` |
| OS-74 | `isMac` off the app | Built: follows `__M2__.platform` when given (one line in `native.ts`) | — | — |
| OS-75 | `Meta` chords inside Monaco | Built: off the Mac, Chrome runs through `e2e/chrome-as-mac.sh`, which gives pages a Mac user agent. A Linux-keys suite comes with the key table on `taste` (Q2). | Monaco takes its key map from the user agent | same |
| OS-76 | fonts from Google Fonts | No change on `main` (`taste` removes them; the server has network) | — | — |
| OS-77 | `shasum` in `baseline.sh` | No change: both OSes have it | — | Git Bash |
| OS-78 | `SIGTERM`/`SIGKILL` in `run.mjs` | No change | — | `taskkill /T` |
| OS-79 | Chrome's libraries | No change: present here | — | — |

### 3.13 Gates and measurements

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-80 | `thread_cpu()` | No change | `CLOCK_THREAD_CPUTIME_ID` | `QueryThreadCycleTime` |
| OS-81 | G11's "F_FULLFSYNC" and "watched by FSEvents" labels | Built: "the durable sync (F_FULLFSYNC on macOS, fsync on Linux)", "watched by file-system events" | — | — |
| OS-82 | step 7's `store.rs` `memory()` (Mach `task_info`) | Designed: `m2_os::process_memory() -> Option<Memory>`, built when step 7 merges. G7f's memory rules must fail, never pass, when it is `None` (today they pass by default on Linux: 0 ≤ 0). | `/proc/self/status` `VmRSS`, `VmSize`; the address space from the CPU's virtual address bits (48 here) | `GetProcessMemoryInfo` |
| OS-83 | step 7's QoS for the drop test's thread | Designed: `m2_os::audio_like_thread()`, built when step 7 merges | nothing allowed here (`RLIMIT_RTPRIO` 0): reported, not failed | `SetThreadPriority` |
| OS-84 | M1 records (`store-S.txt`, `callback-S.txt`) | Designed: each record names its machine; built when step 7 merges | a Linux run is recorded, never judged against an M1 | same |
| OS-85 | the reference Mac for timings | No change | Linux timings are informational | same |

---

## 4. Every Linux failure today, and what fixes it

Slices S0–S8 are in §8.

### 4.1 Rust tests (11)

| # | Test | Root cause (`ground/tests.md`) | Fixed by |
|---|---|---|---|
| 1 | `graph::watch_tests::an_idle_project_is_watched_without_a_stat` | no event source off macOS | S5 (inotify) |
| 2 | `graph::watch_tests::own_commits_make_no_batch_with_events` | same | S5 |
| 3 | `graph::watch_tests::events_that_miss_changes_give_way_to_walks` | same | S5 |
| 4 | `graph::watch_tests::an_event_wakes_an_idle_watcher_and_a_batch_wakes_it_when_due` | nothing calls `Wake::wake` off macOS | S5: the reader thread's `heard` adds a hint, and the hint wakes |
| 5 | `graph::tests::a_lane_written_in_two_halves_never_plays_empty` (event case) | no events | S5; its event-mode case is re-run there, since inotify names every write where FSEvents named the close |
| 6 | `tx::hook_tests::a_commit_that_fails_after_its_preview_accepts_nothing` | no `Swap` on Linux | S2 (`renameat2`) |
| 7 | `tx::stage_tests::a_rollback_that_fails_is_reported_at_once` | same | S2 |
| 8 | `tx::kill_tests::killed_at_every_commit_step_the_project_recovers` | same, and the fallback's middle state has no fix | S2 (passes on ext4), S3 (the fallback itself made recoverable and tested with `plain: true`) |
| 9 | `graph::preview_tests::an_interrupted_commit_after_its_preview_points_to_needs_attention` | same | S2 |
| 10 | `gate_tests::fixture_renders_change_only_with_stds_version` | glibc's `cosf` and friends | S6 (`libm`, one deliberate re-record) |
| 11 | `core/plugins/tests/main_thread.rs:78` | `is_process_main()` always false off macOS | S4 (`gettid() == getpid()`) |

### 4.2 Clippy `-D warnings`, whole workspace (15 lints)

| # | Lint | Fixed by |
|---|---|---|
| 1 | `main_thread.rs:217` `install` never used | S4: `run_main` installs `Kind::Process` through `install` on both OSes |
| 2 | `lock.rs:213` unused `dir` | S2: `local_volume` is `m2_os::local_volume` on both |
| 3 | `stage.rs:106` `cpath` never used | S2: `cpath` moves into `m2-os`, used by both files |
| 4 | `watcher.rs:55` `wake` never used | S5 |
| 5 | `watcher.rs:134` `backstop` never read | S5 (the `Events` arm exists on every OS) |
| 6 | `watcher.rs:216` `deaf` never read | S5 |
| 7–15 | `appkit_main_thread.rs` 28, 115, 117, 125, 147, 150, 159, 241, 265 | S4: under `cfg(target_os = "macos")` |

### 4.3 The desktop app

| Check | Today on Linux | After the step |
|---|---|---|
| `cargo build -p music2-desktop` (after the web build) | ok | ok |
| `cargo test -p music2-desktop` | ok, but checks nothing: 0 unit tests, AppKit test prints "macOS only" | the same, honestly: HOST-2 in the Linux app comes with step 9 (Q3) |
| `cargo build --release --workspace` | ok (1 min 37 s) | ok |
| untracked `gen/schemas/linux-schema.json` | made by every Linux build | committed (S0) |

### 4.4 Browser suites (29 runs)

| Run | Today | Cause | Fixed by (all in S7 unless said) |
|---|---|---|---|
| code-proj | FAIL | Monaco's Linux key map ignores `Meta` | the Mac user agent (`chrome-as-mac.sh`) |
| fixes-graph-fix | FAIL | same | same |
| attention-attn | FAIL | same | same |
| s6-write-fix | FAIL | same | same |
| export-export | FAIL | `isMac` reads `navigator.platform`: "Ctrl+E" | `native.ts`: `isMac` follows `__M2__.platform` |
| fx-fixes-none | FAIL | ext4 tells "exports 2" from "Exports 2" | Q1: a folder name taken in any case, on every OS (`project.rs`). If Q1 is no: the suite asks the disk whether it folds case (`e2e.md`). |
| notes-proj | FAIL | the suite reads the file before the server publishes the revision (2.7 ms gap, hidden on the Mac by `F_FULLFSYNC`) | `notes.mjs:97`, one line: wait until the revision moves, wait 300 ms, then check it moved by exactly one |
| live3b-proj | PART | no plugins on `CLAP_PATH` | `CLAP_PATH=~/m2-runs/plugins/clap` (unpacked there at 21:06, `ldd` finds every library). Its 11 checks with the Mac-saved Surge state on Linux are *unverified* until S7 runs them. If they need CLAP's timer or fd extensions, live3b stays PART on Linux until step 9, and says why. |
| the other 21 | ok | — | — |

The pieces, all small:

```js
// web/src/lib/native.ts: isMac follows the page's given platform first (OS-74)
export const isMac = window.__M2__?.platform ? window.__M2__.platform === "macos" : /Mac/.test(navigator.platform);

// e2e/chrome.mjs: the pinned browser per OS (OS-73); off the Mac through a wrapper with a Mac user agent (OS-75)
const mac = process.platform === "darwin";
const cache = `${os.homedir()}/.cache/puppeteer/chrome-headless-shell`;
export const real = process.env.E2E_CHROME ?? (mac
  ? `${cache}/mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64/chrome-headless-shell`
  : `${cache}/linux-133.0.6943.141/chrome-headless-shell-linux64/chrome-headless-shell`);
if (!mac) process.env.E2E_CHROME_BIN = real;   // puppeteer passes the environment on to the wrapper
export const exe = mac ? real : fileURLToPath(new URL("./chrome-as-mac.sh", import.meta.url));

// e2e/notes.mjs:97 (and the same at s6-fixes.mjs:112 with revF): one write, after the revision is published
check((await until(async () => (await revision()) > rev)) && (await sleep(300), (await revision()) === rev + 1), "the whole drag is one write");
```

```sh
#!/bin/sh
# e2e/chrome-as-mac.sh: chrome-headless-shell telling pages it runs on a Mac. The code editor
# (Monaco) takes its keys from the user agent, and the suites test the Mac's keys (Q2).
exec "$E2E_CHROME_BIN" --user-agent="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/133.0.0.0 Safari/537.36" "$@"
```

`run.mjs` imports `real` for its default (it runs with the real `HOME`; each suite gets `E2E_CHROME` from it, since
suites run with a scratch `HOME`). In `project.rs`'s `make()` (Q1), a candidate is skipped when
`std::fs::read_dir(parent)` lists a name whose `to_lowercase()` equals the candidate's, the rule `reserved()` already
uses for `Exports`. `ground/branches.md` measured the user agent and the `native.ts` line together making code,
fixes-graph, attention, s6-write, undo, start and export pass on `taste`.

Passed by luck today, fixed anyway:

| What | Fixed by |
|---|---|
| a project file missing for up to 0.19 ms during each save (live3a crashed on it 1 in 11) | S2: `renameat2` swaps, so the file never goes missing on ext4 |
| `s6-fixes.mjs:112`, the same file-then-revision race as notes | S7, the same one-line wait |
| the default Chrome path is the Mac's | S7, `chrome.mjs` per OS |
| fonts load from Google Fonts | not on `main`: `taste` removes them (OS-76) |

Not fixed on purpose (Q2): the suites on Linux test the Mac's keys and labels, because Chrome says it is a Mac and the
suites inject `platform: "macos"`. Ctrl, the key Linux users press, gets its own small suite with the key table on
`taste`. This keeps every suite file untouched on `main`, so `taste` merges without suite conflicts.

### 4.5 Offline renders

| Render | Mac at 8aa301e | Linux today | After S6, both OSes |
|---|---|---|---|
| examples-first-song | `6e61b87f…` | same | same |
| examples-first-song-from4-for6 | `ef0fadca…` | same | same |
| examples-resample | `6ac6fcb1…` | `d902bacf…` | `39f94764…` (predicted) |
| examples-resample-from4-for6 | `a360d04c…` | `ea243ccc…` | `d752521b…` (predicted) |
| templates-empty | `835aff53…` | same | same |
| templates-empty-from4-for6 | `a0249c1e…` | same | same |
| templates-starter | `413d35cf…` | same | same |
| templates-starter-from4-for6 | `71bb7191…` | same | same |

### 4.6 What "Linux green" means

All of these on this server, from one `ci/check.sh` run:

| Check | Pass means |
|---|---|
| web build | `npm ci && npm run build` in `web/` exits 0 |
| build | `cargo build --locked --workspace --all-targets`, the desktop app included |
| Rust tests | `cargo test --locked --workspace --no-fail-fast` exits 0: every test, the desktop app's and both no-harness tests included |
| kernel tests | `cargo test --release` in `std/kernels-src` exits 0 |
| std.wasm | a rebuild in another folder equals the committed file |
| clippy | `cargo clippy --locked --workspace --all-targets -- -D warnings` exits 0 |
| mac lint | the same clippy for `--target aarch64-apple-darwin` exits 0 (§7.5) |
| browser suites | 29 of 29 ok; live3b PART only when no plugins are on `CLAP_PATH` |
| renders | the 8 offline renders match `e2e/renders.sha256`, the one file both OSes use |

---

## 5. Determinism

### 5.1 Host math through `libm` (OS-45, OS-47)

`libm` 0.2.16 is already in `Cargo.lock`. `m2-runtime` and `m2-play` add `libm = "0.2"`, which adds no new crate.

| File:line (at f126d03) | Today | After |
|---|---|---|
| `core/runtime/src/audio.rs:191` WSOLA window | `(TAU * i / FRAME).cos()` (f32) | `libm::cosf(TAU * i as f32 / FRAME as f32)` |
| `audio.rs:253` pitch | `2f64.powf(clip.pitch / 12.0)` | `libm::exp2(clip.pitch / 12.0)` |
| `audio.rs:283` gain | `10f32.powf(clip.gain_db as f32 / 20.0)` | `libm::powf(10.0, clip.gain_db as f32 / 20.0)` |
| `audio.rs:294` fade in | `(PI * i / fade_in).cos()` | `libm::cosf(…)`, same argument |
| `audio.rs:297` fade out | `(PI * (n - i) / fade_out).cos()` | `libm::cosf(…)` |
| `audio.rs:411` declick | `(FRAC_PI_2 * (k + 1) / len).sin()` | `libm::sinf(…)` |
| `audio.rs:415` declick | `g0 * (…).cos()` | `g0 * libm::cosf(…)` |
| `audio.rs:424` declick | `(FRAC_PI_2 * k / len).sin()` | `libm::sinf(…)` |
| `core/play/src/resample.rs:46` sinc (f64) | `.sin()` | `libm::sin(…)` |
| `resample.rs:49` Blackman (f64) | `.cos()` twice | `libm::cos(…)` twice |

- **Why `exp2` by name.** A release build turns `2f64.powf(x)` into `exp2`; a debug build calls `pow`. Inside `libm` the
  two differ on 9.5% of a pitch grid (`facts.md` §6b). Naming `libm::exp2` makes debug (the Rust tests) and release
  (`m2 render`, exports) agree. At this fixture's pitches (−5, 0, 7, 12) `libm::exp2` and `libm::pow` give the same
  bits (measured, §A.3), so the predicted hashes below hold.
- **Why it is the same everywhere.** `libm` is plain Rust IEEE arithmetic; Rust never fuses or reorders floating point;
  its `sqrt` and `fma` are exact. On this machine `libm` equals the math inside `std.wasm` bit for bit (every f32 for
  `cosf`, `sinf`, `powf`; 67 million samples each for the f64 functions; `facts.md` §6f).
- **The test.** `core/runtime/tests/host_math.rs` (the pattern of `core/play/tests/no_blocking.rs`): it reads
  `core/runtime/src/audio.rs` and `core/play/src/resample.rs` up to their `#[cfg(test)]` modules and fails on `.sin(`,
  `.cos(`, `.tan(`, `.powf(`, `.exp(`, `.exp2(`, `.ln(`, `.log`, `.tanh(`, `.atan`, `.cbrt(` and `.hypot(`. Step 7's
  merge keeps it (step 7's moved `audio.rs` has the same 8 calls; `branches.md` §2.2). `powi`, `sqrt`, `floor` and the
  like stay allowed: they are exact or Rust's own code.
- **Left alone:** `crates/editor/src/render.rs:307` (`log10` of a peak, shown to the user, not audio),
  `core/kernel/src/lib.rs:235` (runs inside `std.wasm` in use), the probe plugin (tests only).

### 5.2 One deliberate change of the recorded hashes

S6 is the only slice whose renders change. Its commit says so, and why.

| Record | Entry | Before | After S6 |
|---|---|---|---|
| `core/session/tests/renders.json` (debug renders, the fixture test) | `examples/resample` | `cb4d39ba…` (the Mac's) | `b3daee50ee7e7193fc6d240f02384fcda83ccbe33b5c20fbf5904005b4aeb58e` (measured on Linux by `ground/tests.md`) |
| `e2e/renders.sha256` (new in S0, from HANDOFF's Mac hashes) | `examples-resample.wav` | `6ac6fcb1…` | `39f9476492e997a86aec5f94b6d0edf56c098e275186260915a3830bd9cfe0da` (predicted, `facts.md` §6e) |
| same | `examples-resample-from4-for6.wav` | `a360d04c…` | `d752521b47b66bcb9076f43715baedaa5c018f428ec90546465e6cae031560eb` (predicted) |

- The other 6 offline renders and 3 fixture renders keep their hashes.
- No std version bump: std didn't change. RENDER-2 already names "runtime" among what a render's bits depend on.
- The Mac must then produce the same bits. That is the prediction the end-of-step Mac run confirms *(unverified)*.
- If the Mac differs, the step is not done: the cause is found (a call left out, or a CPU difference inside `libm`),
  never a second hash file per OS.

### 5.3 `std.wasm` reproducible (OS-63, OS-64, OS-65)

- **What `ground/branches.md` §4.3 measured.** `--remap-path-prefix` is not enough. `m2-kernel` is a path dependency
  outside the kernels' workspace, so cargo hashes its absolute path into `-C metadata`, which reorders symbols: one
  folder rebuilt `sounds`' file bit for bit, another differed in 1,003 bytes. With `m2-kernel` inside the kernels'
  workspace, two folders gave the same bytes with no machine path in them.
- **The change (S1).**
  1. `git mv core/kernel std/kernels-src/kernel`. It becomes a member of the kernels' workspace (`std/kernels-src` is
     already excluded from the main one). The main workspace's dev-dependency follows the new path; `m2-runtime`
     already dev-depends on `std/kernels-src` the same way today.
  2. `rust-toolchain.toml` at the root:
     ```toml
     [toolchain]
     channel = "1.99.0"
     components = ["clippy", "rustfmt"]
     targets = ["wasm32-unknown-unknown", "aarch64-apple-darwin"]
     profile = "minimal"
     ```
     The macOS target lets every Linux run lint the macOS code (§7.5); on the Mac it is the native target.
  3. `std/build.sh` copies from `${CARGO_TARGET_DIR:-target}`.
  4. Rebuild `std.wasm` once, on purpose. It also takes in the 10 bytes of panic line numbers that have been stale
     since `933f4de`.
- **What it means for kernel hashes.** Every kernel's module hash changes once (new layout, relative panic paths).
  A running app built before would restart its instances at the swap (RENDER-25); offline renders keep their bits.
  S1 proves that: `renders.json` and `e2e/renders.sha256` stay as they are. Nothing in the repo pins `std.wasm`'s
  hash (checked).
- **The check.** `ci/check.sh` copies `std/` to another folder, rebuilds with the pinned toolchain and compares
  with `cmp`. On the Mac this also proves Linux and macOS build the same bytes *(unverified until the Mac run)*.
- **`sounds` merge.** Its `std.wasm` conflicts as a binary. Never take one side: rebuild from the merged sources with
  `std/build.sh`, check the renders, commit once.

### 5.4 x86 and arm wasm floats

- wasm's float operations are IEEE-exact on x86_64 and aarch64. Only NaN payload bits may differ; wasmtime runs with
  NaN canonicalization and relaxed SIMD off, and the renderer silences non-finite samples (OS-48).
- Evidence: before any change, 6 of 8 renders were already the same bits on Linux x86_64 and the Mac's arm64. Only
  `examples/resample` differed, and it is the only fixture that reaches host math.

### 5.5 Denormals on x86 (OS-49), later

- x86 cores pay a microcode assist for each operation on a subnormal number. A decaying reverb or filter tail can then
  cost much more on an x86 machine than on an M1. Apple silicon handles them at full speed.
- Flushing them in hardware (FTZ/DAZ in MXCSR) would change bits, and RENDER-2 forbids it.
- This is an 8a item: measure gate 8's p50 on this Ryzen *(informational)*. If it matters, the kernels keep their own
  state out of the subnormal range deterministically. That changes std's bits, so it needs a std version bump.
- 8a's FPCR check gets its x86 twin, MXCSR, in `m2-runtime`'s `fpenv.rs` (per CPU, not per OS).

---

## 6. Contract edits (`docs/architecture.md`)

The contract says a change that breaks a rule edits the rule in the same commit. So each slice carries its own edits
(the "Slice" column).

### 6.1 Rules that name macOS mechanisms

| Rule | Old text | New text | Slice |
|---|---|---|---|
| PLANE-5 | "and the commit goes on to journal, stage, sync (`F_FULLFSYNC`) and rename." | "and the commit goes on to journal, stage, make it durable (the OS's durable sync: `F_FULLFSYNC` on macOS, `fsync` on Linux) and rename." | S2 |
| PLANE-5 *Why* | (end of the line) | append: "On the Linux development server the syncs of a whole commit took 0.6 ms idle and 1.5 ms at p95 busy (an NVMe with power-loss protection, so a best case); the budget is judged on the reference Mac." | S2 |
| WRITE-15 *Today* | "pushed with `fsync`; one barrier (`F_FULLFSYNC`); renames checked at each step (`RENAME_SWAP`, `RENAME_EXCL`), rolled back in process when one finds an outside change." | "pushed with `fsync`; one barrier, the OS's durable sync (`m2_os::barrier`: `F_FULLFSYNC` on macOS, `fsync` on Linux); renames checked at each step with an atomic exchange and a create-only rename (`m2_os::rename_swap` and `rename_excl`: `renamex_np` on macOS, `renameat2` on Linux), rolled back in process when one finds an outside change. On a volume without them (FAT, exFAT, SMB, NFS; NTFS always), a replace moves the old file aside, then the new one in; recovery puts a file left aside back before it reads the journal, so a commit stopped between the two moves can be finished or discarded (`killed_at_every_commit_step_the_project_recovers` runs on both kinds of volume)." | S2, S3 |
| WRITE-16 *Today* | "another host is told by its hardware UUID" | "another host is told by its machine id (`m2_os::machine_id`: the hardware UUID on macOS, `/etc/machine-id` on Linux)" | S2 |
| WRITE-17 *Today* | "On a volume without `MNT_LOCAL` it takes the lock and raises the Notice `lock.local`, as the step 6 design's C2 proposes." | "On a network volume (`m2_os::local_volume`: no `MNT_LOCAL` on macOS; an NFS, SMB, CIFS, 9p, Ceph, AFS, Coda or FUSE file system on Linux) it takes the lock and raises the Notice `lock.local`, as the step 6 design's C2 proposes." | S2 |
| WRITE-19 *Today* | "on a local volume FSEvents names what changed (`crates/server/src/graph/fsevents.rs`, in-tree over CoreServices, for `FSEventStreamFlushSync`)" | "on a local volume file-system events name what changed (`m2_os::Events`: FSEvents on macOS, in-tree over CoreServices for `FSEventStreamFlushSync`; inotify on Linux, one instance per process, where an event is queued before the writer's call returns, so a flush is a read that never waits)" | S5 |
| WRITE-19 *Today* | "On a network volume (no `MNT_LOCAL`: its events name only this Mac's writes), when FSEvents won't start," | "On a network volume (its events name only this computer's writes), when events won't start (inotify's per-user limits included)," | S5 |
| FMT-16 | "in `~/Library/Caches/music2/`," | "in the OS's cache folder (`m2_os::cache_dir`: `~/Library/Caches/music2/` on macOS, `~/.cache/music2/` on Linux)," | S2 |
| FMT-16 *Today* | "The CLI and the app keep the cache in `~/Library/Caches/music2/hashes.json`" | "The CLI and the app keep the cache in `hashes.json` there" | S2 |
| FMT-20 *Today* | "with the machine's hardware UUID as what tells another host's journal" | "with the machine id ([WRITE-16](#commit-journal-and-recovery)) as what tells another host's journal" | S2 |
| RENDER-30 | "Decoded and warped renders are cached in `~/Library/Caches/music2/`," | "Decoded and warped renders are cached in the OS's cache folder ([FMT-16](#assets-and-hashes))," | S2 |
| Layout block (line 452) | "`~/Library/Caches/music2/     outside the project: decoded and warped renders, the hash cache`" | "`<OS cache folder>/music2/    outside the project: decoded and warped renders, the hash cache (~/Library/Caches on macOS, ~/.cache on Linux)`" | S2 |
| RENDER-2 | (end of the rule) | append: "Host code that makes audio for a render computes every transcendental function with the `libm` crate, never the OS's math library ([PLAT-3](#platforms))." | S6 |
| RENDER-2 *Today* | (end) | append: "Since the platform step the clip path's math is `libm` (`core/runtime/src/audio.rs`; `core/runtime/tests/host_math.rs` bans host math there), so `examples/resample` renders the same bits on macOS and Linux." | S6 |
| HOST-2 | "In the app that is the AppKit main thread, reached through the main dispatch queue. In the CLI, the headless server and tests, the main thread runs a `CFRunLoop` and serves as the plugin main thread." | "That thread runs the OS's main loop (`m2_os::post_to_main`, `run_main_loop`). In the app on macOS it is AppKit's, reached through the main dispatch queue. In the CLI, the headless server and the gates, the main thread runs it under `run_main`: a `CFRunLoop` on macOS, a job queue on Linux. On Linux the app hosts no plugins until step 9 gives it a main loop that also serves CLAP's `timer-support` and `posix-fd-support`." | S4 |
| HOST-2 *Today* | "the main thread in its `CFRunLoop` serving the main queue;" | "the main thread running the OS's main loop (a `CFRunLoop` serving the main queue on macOS, a job queue on Linux);" | S4 |
| HOST-2 *Today* | "`is_main_thread` asks the executor (`pthread_main_np`, or the dedicated thread's `pthread_self`)" | "`is_main_thread` asks the executor (`m2_os::is_process_main`: `pthread_main_np` on macOS, `gettid() == getpid()` on Linux; or the dedicated thread's token)" | S4 |
| HOST-3 | "and its own AppKit main thread for windows." | "and its own main loop for windows (AppKit on macOS; on Linux an X11 loop that also serves CLAP's `timer-support` and `posix-fd-support`)." | S4 |
| ABI-1 *Today* | "`core/kernel/src/lib.rs`" | "`std/kernels-src/kernel/src/lib.rs`" | S1 |
| Acceptance gates, first paragraph | "A step is done when its gates pass in CI or, for timings, in a recorded run on the reference projects." | "A step is done when its gates pass in `ci/check.sh` on Linux and on the owner's Mac ([PLAT-5](#platforms)) or, for timings, in a recorded run on the reference projects." | S0 |
| same | "Timings are taken on the **reference Mac**, the oldest supported Apple-silicon Mac (an M1), at 48 kHz." | keep, and append: "Timings measured on Linux are recorded for information only." | S0 |
| same, after the table | "These gates keep running in CI once their step lands:" | "These gates keep running in `ci/check.sh` once their step lands:" | S0 |

The "reference Mac" stays: macOS remains the judged platform for timings.

### 6.2 A new section: Platforms (before "Deferred, with triggers")

```markdown
## Platforms

**PLAT-1.** music2 MUST build and pass every check on macOS (Apple silicon, the reference) and on Linux (x86_64,
with the glibc it was built against). Windows is designed for in every OS piece and built in its own step. Timings
are judged on the [reference Mac](#acceptance-gates) only.
- *Why:* the work moved to a Linux server; the Mac stays where music2 is used and timed.
- *Today:* since the platform step: Linux on the development server, macOS by the owner's run of `ci/check.sh`.

**PLAT-2.** Code whose behaviour differs between operating systems MUST live in the `m2-os` crate (`core/os`), in one
file per OS (`macos.rs`, `linux.rs`) that defines the same items with the same signatures. Outside `m2-os` no code
names an OS, except the desktop shell (`apps/music2-desktop`) and its tests. Code that is the same on every Unix
through `std::os::unix` MAY stay at its call site under `cfg(unix)` until the Windows step.
- *Why:* one place to read per OS, so later steps write each OS piece once.
- *Today:* since the platform step; `core/os/tests/one_place.rs` fails on `target_os` anywhere else.

**PLAT-3.** A pure offline render MUST give the same bits on every supported OS and CPU. Host code that makes audio
for a render computes every transcendental function with the `libm` crate, the same code as `std.wasm`'s math. One
record of render hashes serves every OS (`core/session/tests/renders.json`, `e2e/renders.sha256`).
- *Why:* an export must not depend on the computer that made it.
- *Today:* since the platform step ([RENDER-2](#offline)).

**PLAT-4.** `std/kernels/std.wasm` MUST rebuild to the same bytes in any folder on any supported OS with the
toolchain `rust-toolchain.toml` pins. A commit that changes it says that the kernel hashes changed.
- *Why:* a kernel's hash is part of its identity ([RENDER-25](#plans-data-and-arrangement-swaps)); a rebuild must
  never change it by accident.
- *Today:* since the platform step: `m2-kernel` is a member of the kernels' workspace, so no absolute path reaches
  the build, and `ci/check.sh` rebuilds it in another folder and compares.

**PLAT-5.** Every slice runs `ci/check.sh` on Linux, including a lint of the macOS code
(`--target aarch64-apple-darwin`). The same script runs on the owner's Mac at the end of every step and before a
branch that changes macOS-only code is merged. Until it does, the step's commit message and `HANDOFF.md` say
"macOS: unverified since <commit>".
- *Why:* there are no hosted Mac runners; the owner's Mac is the reference, checked by hand.
- *Today:* since the platform step.
```

### 6.3 The order of work

Insert after item 6 (no renumbering, so "step 7", "8a" and the rest keep their names):

```markdown
**The platform layer** (between steps 6 and 7; about 1 to 2 weeks). Development moved to a Linux server, so Linux is
made green there first, natively, and the Mac stays green. **Built** when its gates pass.
- `m2-os` (`core/os`): what differs per OS, one file each for macOS and Linux ([PLAT-2](#platforms)): renames and the
  durable sync, the volume and machine checks, file-system events (FSEvents, inotify), the process's main loop for
  plugins, the cache and CLAP folders.
- Deterministic host math with `libm` ([PLAT-3](#platforms)); a reproducible `std.wasm` and a pinned toolchain
  ([PLAT-4](#platforms)); recovery of a replace stopped halfway on volumes without an atomic exchange.
- One check script for both OSes, `ci/check.sh`, run by hand on the owner's Mac ([PLAT-5](#platforms)).
- Windows is designed for in each piece, and built in its own step.
```

### 6.4 The acceptance gates table

Add a row:

| Step | Gates |
|---|---|
| Platform | `ci/check.sh` is all ok on Linux: every Rust test (the desktop app's included), clippy `-D warnings` on the whole workspace and for `aarch64-apple-darwin`, the web build, all 29 browser-suite runs (live3b in full where Surge XT and Airwindows are on `CLAP_PATH`), `std.wasm` rebuilt to its bytes in another folder, and the 8 offline renders equal to `e2e/renders.sha256`. The same script is all ok on the owner's Mac at the step's last commit. The kill test passes on volumes with and without an atomic exchange. No `target_os` outside `m2-os` and the shell. |

### 6.5 Deferred, with triggers (new rows)

| Deferred | Trigger |
|---|---|
| Windows (every `m2-os` item in `windows.rs`; the `cfg(unix)` code moved into `m2-os`) | The owner asks for Windows users. |
| Linux app packaging (deb, AppImage) and plugins in the Linux app | Someone runs the app on Linux; plugins come with step 9's main loop. |
| Stored spelling on case-folding Linux volumes (vfat, exFAT, casefold folders) | A project on such a volume, or a volume to test on. |

---

## 7. Verification

### 7.1 One script for both OSes: `ci/check.sh`

The full draft. It runs with macOS's bash 3.2 (no bash 4 features). Each step logs to its own file and the run goes on
after a failure, so one run gives the whole picture.

```bash
#!/bin/bash
# ci/check.sh: the full check of this checkout. The same script on macOS and on Linux.
#
#   ci/check.sh
#
# In short: it builds everything, runs every Rust test, clippy, the web build, every browser
# suite and the 8 offline renders, then prints a short summary to paste back. 15 to 40 minutes.
# It never opens the app, never plays sound, and never touches your ~/Music or ~/.music2.
#
# Optional settings (environment):
#   CARGO_TARGET_DIR  where cargo builds (default: target/ in this checkout)
#   CLAP_PATH         a folder with "Surge XT.clap" and "Airwindows Consolidated.clap", for the
#                     plugin checks. Default: ~/Library/Audio/Plug-Ins/CLAP on a Mac, ~/.clap on
#                     Linux, when it has both. Without them the plugin parts are skipped (PART).
#   CHECK_OUT         where the logs go (default: <target dir>/check/<date>-<time>)
# Exit status: 0 when all is ok (a PART only for missing plugins), 1 otherwise.

set -u
REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO" || exit 1
case $(uname -s) in
  Darwin) OS=macos; MACHINE="macOS $(sw_vers -productVersion), $(uname -m), $(sysctl -n hw.model)" ;;
  Linux)  OS=linux; MACHINE="Linux $(uname -r), $(uname -m), $(. /etc/os-release && echo "$PRETTY_NAME")" ;;
  *) echo "ci/check.sh: music2 is checked on macOS and Linux only" >&2; exit 2 ;;
esac

# Whatever runs music2 gets a scratch HOME below; rustup and cargo must still find the toolchain.
export RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup} CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
export PATH="$CARGO_HOME/bin:$PATH" M2_AUDIO=none CARGO_TERM_COLOR=never
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target}
T=$CARGO_TARGET_DIR
OUT=${CHECK_OUT:-$T/check/$(date +%Y%m%d-%H%M%S)}
mkdir -p "$OUT/home" || exit 1
SCRATCH=$OUT/home
TC=$(rustup show active-toolchain | cut -d' ' -f1)   # the one rust-toolchain.toml pins

if [ -z "${CLAP_PATH:-}" ]; then
  if [ $OS = macos ]; then d=$HOME/Library/Audio/Plug-Ins/CLAP; else d=$HOME/.clap; fi
  if [ -e "$d/Surge XT.clap" ] && [ -e "$d/Airwindows Consolidated.clap" ]; then export CLAP_PATH=$d; fi
fi

START=$(date +%s); BAD=0
: > "$OUT/table"
row() { printf '%-15s %-5s %s\n' "$1" "$2" "$3" >> "$OUT/table"; if [ "$2" = FAIL ]; then BAD=1; fi; }
run() { local name=$1; shift; echo "== $name  $(date +%H:%M:%S)"; "$@" > "$OUT/$name.log" 2>&1; }

# 1. The web UI: the desktop app embeds it, and the browser suites load it.
if run web sh -c 'cd web && npm ci --no-audit --no-fund && npm run build'
then row "web build" ok ""; else row "web build" FAIL "web.log"; fi

# 2. Everything: the desktop app, every test binary, m2 and the headless server.
if run build cargo build --locked --workspace --all-targets
then row build ok ""; else row build FAIL "build.log"; fi

# 3. Every Rust test, the desktop app's included (on a Mac the AppKit test needs your login session).
run test env HOME="$SCRATCH" cargo test --locked --workspace --no-fail-fast; st=$?
set -- $(grep -E '^test result:' "$OUT/test.log" | awk '{p+=$4; f+=$6} END {print p+0, f+0}')
if [ $st -eq 0 ]; then row "rust tests" ok "$1 passed"; else row "rust tests" FAIL "$1 passed, $2 failed: test.log"; fi

# 4. The kernels' own tests: std/kernels-src is a workspace of its own.
if run kernels cargo test --locked --release --workspace --manifest-path std/kernels-src/Cargo.toml
then row "kernel tests" ok ""; else row "kernel tests" FAIL "kernels.log"; fi

# 5. std.wasm rebuilds to the committed bytes in another folder (PLAT-4).
rm -rf "$OUT/std"
tar -C "$REPO" --exclude target -cf - std | tar -C "$OUT" -xf -
if run std-wasm env RUSTUP_TOOLCHAIN="$TC" CARGO_TARGET_DIR="$OUT/std-target" "$OUT/std/build.sh" \
   && cmp -s "$OUT/std/kernels/std.wasm" std/kernels/std.wasm
then row std.wasm ok "rebuilds to the committed bytes"; else row std.wasm FAIL "differs: std-wasm.log"; fi

# 6. Clippy, every warning an error.
if run clippy cargo clippy --locked --workspace --all-targets -- -D warnings
then row clippy ok ""; else row clippy FAIL "clippy.log"; fi

# 7. On Linux: the macOS-only code, type-checked and linted for aarch64-apple-darwin. There is no
#    macOS SDK here, so ci/nocc.sh stands in for the C compiler; a check never links C (PLAT-5).
if [ $OS = linux ]; then
  if run mac-lint env CC_aarch64_apple_darwin="$REPO/ci/nocc.sh" CXX_aarch64_apple_darwin="$REPO/ci/nocc.sh" \
       AR_aarch64_apple_darwin=ar cargo clippy --locked --target aarch64-apple-darwin --workspace --all-targets -- -D warnings
  then row "mac lint" ok "macOS code type-checks and lints"; else row "mac lint" FAIL "mac-lint.log"; fi
fi

# 8. Every browser suite, each on a fresh headless server with a scratch HOME (run.mjs).
(cd e2e && npm ci --no-audit --no-fund) > "$OUT/e2e-npm.log" 2>&1
chrome=$(cd e2e && node --input-type=module -e 'const m = await import(new URL("chrome.mjs", `file://${process.cwd()}/`)); console.log(m.real)')
if [ ! -x "$chrome" ]; then    # the pinned chrome-headless-shell, fetched once
  (cd e2e && node_modules/.bin/browsers install chrome-headless-shell@133.0.6943.141 --path "$HOME/.cache/puppeteer") \
    >> "$OUT/e2e-npm.log" 2>&1
fi
run e2e env M2_HEADLESS="$T/debug/examples/headless" node e2e/run.mjs all --out "$OUT/e2e"; st=$?
line=$(grep -E '^[0-9]+/[0-9]+ suites ok' "$OUT/e2e.log" | sed 's/;.*//')
case $st in
  0) row "browser suites" ok "$line" ;;
  3) if [ -z "${CLAP_PATH:-}" ]; then row "browser suites" PART "$line (no plugins, so live3b ran in part)"
     else row "browser suites" FAIL "$line: e2e.log"; fi ;;
  *) row "browser suites" FAIL "$line: e2e.log" ;;
esac

# 9. The 8 offline renders, with the debug m2 built above (renders keep their bits at any
#    opt-level), against the one record every OS shares (PLAT-3).
run renders env M2="$T/debug/m2" e2e/baseline.sh --tree . "$OUT/renders"
(cd "$OUT/renders" && shasum -a 256 -c "$REPO/e2e/renders.sha256") > "$OUT/renders-check.log" 2>&1
n=$(grep -c ': OK$' "$OUT/renders-check.log"); n=${n:-0}
if [ "$n" -eq 8 ]; then row renders ok "8 of 8 match e2e/renders.sha256"; else row renders FAIL "$n of 8 match: renders-check.log"; fi

# The summary to paste back.
mins=$(( ($(date +%s) - START + 59) / 60 ))
if [ -n "$(git status --porcelain)" ]; then state="with local changes"; else state=clean; fi
{
  echo "music2 check: $(git rev-parse --short HEAD) on $(git rev-parse --abbrev-ref HEAD) ($state)"
  echo "machine: $MACHINE"
  echo "plugins: ${CLAP_PATH:-none}"
  cat "$OUT/table"
  if [ $BAD = 0 ]; then echo "RESULT: ALL OK ($mins min)"; else echo "RESULT: FAILED ($mins min). Logs: $OUT"; fi
} | tee "$OUT/summary.txt"
exit $BAD
```

`ci/nocc.sh` (the version measured in §A.1):

```sh
#!/bin/sh
# A C compiler that compiles nothing, for `cargo clippy --target aarch64-apple-darwin` on Linux:
# only Rust is checked there, and no C object is ever linked. cc-rs's probes are answered as a
# clang that is not MSVC (-E by the real clang; -? fails); every compile writes an empty object.
case " $* " in *" -? "*) exit 1;; *" -E "*) for f; do :; done; exec clang -E "$f";; esac
prev=
for a in "$@"; do
  [ "$prev" = "-o" ] && : > "$a"
  prev=$a
done
exit 0
```

What the script does not run: the timing gates (`m2-gates`), the two `#[ignore]` release tests (they passed on Linux
at f126d03; run them when a step's gates ask), and the desktop app itself (never launched).

### 7.2 On this server

```sh
cd ~/dev/suite2/music2
~/m2-runs/tw check-<label> 'CARGO_TARGET_DIR=$HOME/m2-runs/target-<label> CLAP_PATH=$HOME/m2-runs/plugins/clap ci/check.sh'
# tw waits 9.5 minutes; a full check takes longer, so then: ~/m2-runs/tw-wait <channel> <log> (tw prints it)
```

- The command has no double quotes or parentheses, so `tw`'s echo line stays intact (`ground/tests.md`'s warning).
- The summary is in `<target>/check/<time>/summary.txt`; every step's log is beside it.
- Delete the target dir after the workflow (it grows to 20–60 GB).
- Expected time here: about 15–25 minutes *(estimate: build ~2 min warm, tests ~5 min with the kill test at 104 s,
  mac lint ~2.5 min cold and seconds warm (measured), suites ~6 min, renders ~1 min)*.

### 7.3 The Mac check by hand

- The owner runs `ci/check.sh` from the checkout, at the commit the lead names (§1's three steps).
- The Mac is **green** at a commit when the summary says `RESULT: ALL OK`, with every row ok: web build, build,
  Rust tests (the AppKit test and `main_thread.rs` among them), kernel tests, std.wasm, clippy, browser suites 29/29,
  renders 8 of 8. "PART" on the browser suites is acceptable only when the Mac has no plugins (the "plugins:" line says
  `none`).
- The lead then writes into `HANDOFF.md`: "macOS: verified at <commit> on <date> (owner's MacBook, <model line>)".
- Until then the step's commit message and `HANDOFF.md` say: "macOS: unverified since <first commit of the step>
  (last verified at 8aa301e)". The Linux state is recorded beside it.
- If a row fails: the lead reads the log the row names (the owner sends that file, or its last 50 lines), fixes it on
  Linux, checks the macOS lint, and asks for one more run.

### 7.4 When to ask the owner for a Mac run

As few as possible:

| When | Why |
|---|---|
| At the end of this step (required) | The macOS code moved, renders changed on purpose, `std.wasm` was rebuilt. |
| At the end of step 7 (its merge moves step 7's Mach memory call into `m2-os`) | Every step ends with one. |
| Before merging `taste` | It rewrites the macOS menus and the window builder. |
| Later: before merging any branch that changes `core/os/src/macos.rs`, any other macOS-only code (the shell's `cfg(target_os = "macos")`, `tauri.conf.json`'s macOS part, the AppKit test), and at the end of every step | PLAT-5 |

Not needed for: Linux-only work, OS-neutral code (the macOS lint covers its compile, and PLAT-3 makes renders shared),
or `sounds` alone (no macOS code; its new render hashes ride along to the next scheduled run).

### 7.5 Checking the macOS code from Linux (measured for this design)

| What | Result |
|---|---|
| `cargo check` and `cargo clippy -D warnings --target aarch64-apple-darwin` for `m2-plugins`, `m2-song` | pass with only the target's std (no C in them) |
| the same for crates that build C (`m2-runtime` via wasmtime's `helpers.c`, `m2-script` via QuickJS, the desktop app via `objc2-exception-helper`) | fail without help: gcc rejects `-arch` |
| with `ci/nocc.sh` as `CC_aarch64_apple_darwin` | **the whole workspace passes `cargo clippy --target aarch64-apple-darwin --workspace --all-targets -- -D warnings` at f126d03, the desktop app included** (6 s warm; about 2.5 min cold) |
| does it really see macOS-only code? | yes: a type error planted in `fsevents.rs` (macOS-only) in a scratch copy failed with E0308; and the AppKit test's 9 Linux dead-code lints are absent for the macOS target, so its macOS code is linted |
| needs | the `aarch64-apple-darwin` std (30.7 MB; `rust-toolchain.toml` makes rustup add it, user-level, no sudo), `clang` (installed) |

What it catches: type errors, missing or mismatched items between `macos.rs` and `linux.rs`, wrong `cfg`s, every rustc
and clippy lint in macOS-only Rust (dead code included). What it cannot catch: linking against the frameworks, any
runtime behaviour (FSEvents, `F_FULLFSYNC`, `renamex_np`, the run loop, AppKit), errors in C or Objective-C, build
scripts that need Xcode (bundling), and every test. That is what the Mac run is for.

### 7.6 Timings

- Linux timings (G3, G11, the watcher's CPU) are informational. This server's disk has power-loss protection and no
  sound card, so its numbers are a best case and can't stand in for a laptop.
- Judged timings stay on the reference Mac (PLANE-5's 20 ms, gate 8). No timing gate is part of `ci/check.sh`.
- This step changes no timed path on the Mac (its barrier, renames and FSEvents code are moved unchanged), so it
  needs no Mac timing run.

---

## 8. Build slices

Linux starts red (11 test failures, clippy stops at its first crate, 7 suites FAIL). So "green at the end of a slice"
means: every check that slice touches passes, nothing new fails, and the failure list only shrinks. The last code
slice (S7) ends fully green. Renders stay byte-identical in every slice except S6.

| Slice | What | Fixes | Renders | Mac run |
|---|---|---|---|---|
| **S0** Check script and pins | `ci/check.sh`, `ci/nocc.sh`; `rust-toolchain.toml`; `e2e/renders.sha256` holding HANDOFF's 8 Mac hashes, under the names `baseline.sh` writes (`…-from4-for6.wav`, where HANDOFF says `…-from4.wav`); `baseline.sh` renders the 8th case (`templates/empty --from 4 --seconds 6`); commit `linux-schema.json`; `build.sh` honours `CARGO_TARGET_DIR`; contract: acceptance-gates text, PLAT-1, PLAT-5, the order-of-work entry | none: it measures today's state (the 11 failures, renders 6 of 8, and the std.wasm row fails until S1). The mac lint already passes (§7.5). | same | no |
| **S1** Reproducible `std.wasm` | `git mv core/kernel std/kernels-src/kernel`; paths in the root `Cargo.toml` and the kernels' `Cargo.toml`; one deliberate rebuild; contract PLAT-4, ABI-1's pointer | the std.wasm row | same bits (the proof that kernel hashes, not sounds, changed) | no |
| **S2** `m2-os`: files, volumes, identity | the crate (`lib.rs`, `macos.rs`, `linux.rs`, `tests.rs`, `one_place.rs` with its allow-list); `rename_swap`/`rename_excl` (`renameat2`), `barrier`, `clone_file`, `local_volume` (`statfs`), `stored_name`, `machine_id`, `cache_dir`, `no_crash_report`; callers in `stage.rs`, `lock.rs`, `watch.rs`, `render.rs` (`name_new`), `resources.rs`, the kill and hook tests' children; contract PLANE-5, WRITE-15/16/17, FMT-16/20, RENDER-30 | tests 6–9; lints 2, 3; the kill test 473 s → 104 s; the vanishing file during saves | same | no (mac lint) |
| **S3** A replace stopped halfway can be recovered | `journal.rs`: before a writer reads an interrupted journal, a target that is missing while `.music2/tx/<id>/old-<k>` holds its before hash is moved back with a create-only rename (then the journal reads as usual, so Finish and Discard are offered); a reader's scan reports it as at its before hash. The kill test runs every case twice: `Recorder::default()` and `Recorder { plain: true }`. Contract WRITE-15 *Today*. | `ground/tests.md` surprise 1 (Mac on exFAT/SMB, Linux on NFS, Windows always) | same | no |
| **S4** Plugin main thread and CLAP folders | `thread_token`, `is_process_main` (`gettid`), `post_to_main`, `run_main_loop`, `APP_RUNS_MAIN_LOOP`, `clap_dirs`; `main_thread.rs` with `Kind::Process` on both OSes and re-exports of `token` and `is_process_main`; `lib.rs`'s `search_paths`; the AppKit test's items under `cfg`; contract HOST-2, HOST-3 | test 11; lints 1, 7–15. With `CLAP_PATH`, `main_thread.rs` also runs Surge XT on the real main thread (*to confirm here*) | same | no (mac lint) |
| **S5** File-system events | `m2_os::Events` (FSEvents moved; inotify built), `graph/fsevents.rs` → `graph/events.rs`, no OS `cfg` in `graph.rs` or `watcher.rs`; G11's two labels; `one_place.rs`'s allow-list is now empty; contract WRITE-19 | tests 1–5; lints 4–6: clippy is clean | same | no (mac lint) |
| **S6** Deterministic math | `libm` for the 8 calls in `audio.rs` and 3 in `resample.rs`; `host_math.rs`; re-record `renders.json` (resample) and the two resample lines of `renders.sha256`; contract RENDER-2, PLAT-3 | test 10; renders 8 of 8 | **changes, on purpose**: the 3 entries in §5.2 | no |
| **S7** Browser suites | `native.ts` (one line); `chrome.mjs` per OS plus `chrome-as-mac.sh`; `run.mjs` takes Chrome's path from `chrome.mjs`; `notes.mjs:97` and `s6-fixes.mjs:112`, one line each; Q1's `make()` check in `project.rs` with a unit test; run live3b with `CLAP_PATH` | the 7 FAIL suites; live3b ok or an honest PART | same | no |
| **S8** Handoff and the Mac run | `HANDOFF.md`: Linux green at `<commit>`, "macOS: unverified since <S0's commit>", how to run `ci/check.sh`; commit and push; ask the owner for the Mac run; record the result | — | — | **yes, the one run** |

Sizes *(estimates)*: S0 ~200 lines (mostly the script), S1 ~10 plus a move, S2 ~350 (half moved), S3 ~60,
S4 ~120 (half moved), S5 ~300 (FSEvents moved, inotify new), S6 ~60, S7 ~40. About a week of work, then the Mac run.

Optional: if the owner happens to be at the Mac after S5, an early run would catch a mistake in the moved macOS code
sooner. Not required.

### 8.1 The merges after this step

`HANDOFF.md`'s order stays: finish step 7, then `sounds`, then `taste`. None of the files this step changes is touched
by any branch except the ones below (checked with `git diff main...origin/<branch>`).

| Branch | Expected conflict | How to resolve |
|---|---|---|
| `wip/step7` | `Cargo.lock` (`m2-os`, `libm` edges beside step 7's) | Take `main`'s lock and build: cargo adds step 7's two path edges. Never `cargo update`. |
| | `docs/architecture.md` PLANE-5 (one long line) | Keep both edits. |
| | `core/plugins/src/probe.rs` (6 calls to `main_thread::token()` and `is_process_main()`) | No conflict: they stay as re-exports. |
| | `core/runtime/src/audio.rs` | Clean: step 7 moved the code, and the 8 `libm` edits land in it (`branches.md` §2.2). `host_math.rs` guards it. |
| | `crates/gates/src/store.rs` (new, Mach `task_info`, QoS) | Clean as text. At the merge, move its two `cfg` blocks into `m2_os::process_memory()` and `audio_like_thread()` (OS-82, OS-83); make G7f's memory rules fail when memory is `None`; records name their machine. |
| | `apps/music2-desktop/src/main.rs` | No conflict: this step doesn't touch it (`plugin_host::install` keeps its name; on Linux it still reports that plugins can't be hosted). |
| | `graph.rs`, `render.rs`, `gates/src/server.rs` | Clean: step 7's hunks are far from this step's lines. |
| `sounds` | `std/kernels/std.wasm` (binary) | Never take a side. Rebuild with `std/build.sh` (now path-independent), check the renders, commit once. Then add `examples/sounds` to `PURE`, `baseline.sh` and both hash records, recorded on Linux; the next Mac run confirms them. |
| `taste` | possibly one line of `docs/architecture.md` near WRITE-17/18 | Keep both. |
| | `e2e/notes.mjs`, `e2e/s6-fixes.mjs` | Expected clean: this step changes one line each, and `taste`'s changes there are on other lines with an unchanged line between *(confirm at the merge)*. |
| | `crates/server/src/project.rs`, `e2e/run.mjs`, `graph.rs`'s module list | Clean: `taste`'s hunks are elsewhere (`make()` is untouched; `run.mjs` only `SUITES` rows; `mod view_file;` sits two lines below `mod events;`). |
| | After the merge | `taste`'s 3 new suites need no change (the Mac user agent covers them). Re-run s8-tracks' "layout.json within a second" on the inotify watcher (`branches.md` §3.1 #11). The key table, the menu per OS, the reveal label and the Linux-keys suite are `taste`/8b work (§9). Mac run before the merge. |

---

## 9. How 8a, 8b, step 7's merge and step 9 plug in

Each fills items in the same two files, `core/os/src/macos.rs` and `linux.rs`. Nothing here designs them.

| Step | Adds to `m2-os` (both files, in its own slices) | Stays outside |
|---|---|---|
| step 7's merge | `process_memory()`, `audio_like_thread()`, and the machine named in gate records | G7f's rules (gates) |
| 8a | design-8a's `core/play/src/rt.rs` becomes the engine-thread section: `Group`, `on_device_thread`, `engine_thread`, `end_engine_thread`, `background_thread`, `cores`, `Wake` (`signal`, `wait`), `machine()`. Linux: RLIMIT_RTTIME set by us and SIGXCPU ignored, `sched_setscheduler` or rtkit; refusals reported, never fatal (this server grants none). | cpal's error sorting (bug 1) in `core/play`; `fpenv` (FPCR, MXCSR) per CPU in `m2-runtime`; the "no device → silent" rule in `core/play` |
| 8b | nothing in Rust | the key table (`menu.rs`'s `ITEMS` plus one label helper in `native.ts`), ⌃ chords on macOS only, a menu per OS (GTK: Quit in File, About in Help, no Services/Hide), the reveal label, a Linux-keys browser suite, on `taste` |
| 9 | the host-process kit (`spawn_host`, `Shared`, `SharedWake`, `Child`); `WINDOW_API` and `GuiLoop` (Linux: a `poll` loop over the X fd, plugins' fds, a timerfd and an eventfd, which also replaces this step's Linux job queue under `run_main` and gives the Linux app its plugin main thread, closing OS-26, OS-27, OS-31); plugin formats and folders (VST3); `will_need`, `io_priority`, `keep_awake`, `input_access`, `midi_instant` (`cache_dir` exists already) | midir, memmap2, cpal input |
| Windows | `windows.rs` with every item; the `cfg(unix)` leftovers (§2.3) move in; the lock one byte past EOF; no folder sync; a Win32 message loop | the build (MSVC, QuickJS under `cl.exe`) |

---

## 10. Owner questions

Only real decisions. Each has a recommended answer.

| # | Question | Recommended | Why |
|---|---|---|---|
| Q1 | On Linux, should a new song's folder name count as taken when a folder with the same name in other capitals exists ("exports 2" next to "Exports 2")? | **Yes**, on every OS. | The Mac already works this way, so nothing changes there. Song folders stay safe to copy to a Mac or Windows. The `fx-fixes` suite then passes unchanged. If no: the suite asks the disk instead. |
| Q2 | On Linux, the browser suites test the Mac's keys (⌘) and labels, by telling Chrome it is a Mac. Ctrl gets its own small suite later, with the key table on `taste`. OK? | **Yes.** | No suite file changes on `main`, so `taste` merges without suite conflicts. The Mac is the reference. Ctrl is tested once the key table defines it. |
| Q3 | The Linux desktop app: no plugins inside it and no Linux install package (deb, AppImage) in this step. Plugins come with step 9; a package when someone needs the app on Linux. OK? | **Yes.** | The server has no screen, so nobody runs the app there now. Plugins in the Linux app need the main loop step 9 builds anyway. |
| Q4 | Lock messages show the server as "Ubuntu-2404-noble-amd64-base". Rename it to `jerry-ax102`? It is one command (`sudo hostnamectl set-hostname jerry-ax102`), a system change only you should make. | **Yes, optional.** | Messages name the right machine. Nothing else depends on it. |

---

## A. Probes made for this design

All on `jerry-ax102` at `f126d03`, with my own target dir and scratch folder (both deleted afterwards). Logs:
`~/m2-runs/logs/ds-*.log`.

### A.1 The macOS code checked from Linux

- Fetched `rust-std-1.99.0-aarch64-apple-darwin.tar.xz` (30.7 MB, sha256 `a7c6de8a…`, matching the published
  `.sha256`) into a scratch sysroot, used through `RUSTFLAGS=--sysroot=…` (with `--target`, host build scripts don't
  get it). `rustup target add` gives the same without the sysroot.
- `cargo check` and `cargo clippy -D warnings --target aarch64-apple-darwin -p m2-plugins --all-targets`: exit 0.
  `m2-song`: exit 0. `m2-runtime`, `m2-script` and everything above them: fail in wasmtime's and QuickJS's build
  scripts (gcc rejects `-arch`).
- With `ci/nocc.sh` (two earlier versions failed: cc-rs first took it for MSVC, so it must answer `-E` like clang and
  fail `-?`): `cargo clippy --locked --target aarch64-apple-darwin --workspace --all-targets -- -D warnings` exit 0,
  desktop app included, 6 s warm. The cold runs took about 51 s and 96 s.
- A scratch copy (`git archive`) with `let _deliberate: u32 = "not a number";` in `fsevents.rs`'s `flush`:
  `cargo check --target aarch64-apple-darwin -p m2-server` failed with E0308 at that line.

### A.2 The Linux plugins, headless

- `~/m2-runs/plugins/clap` holds `Surge XT.clap` (1.3.4, sha256 `c923a148…`) and `Airwindows Consolidated.clap`
  (nightly 2026-10-11-8a67b80, sha256 `9bd19011…`), with `SurgeXTData` one level up. `ldd` on both: nothing missing.
- `cargo test -p m2-plugins` with a scratch `HOME`, `M2_AUDIO=none`, no `DISPLAY`, `CLAP_PATH` set: 13 of 13 unit tests
  pass, `surge_plays_notes`, `airwindows_processes_audio`, `state_round_trips` and `parameter_values_have_display_text`
  among them. The host offers neither `timer-support` nor `posix-fd-support`.
- `tests/main_thread.rs` still fails at line 78 (`process_main: false`), as known, before reaching Surge.

### A.3 `libm::exp2` against `libm::pow` at the fixture's pitches

| pitch | `exp2(p/12)` | `pow(2, p/12)` | same |
|---|---|---|---|
| −5 | `3fe7f910d768cfb0` | `3fe7f910d768cfb0` | yes |
| 0 | `3ff0000000000000` | `3ff0000000000000` | yes |
| 7 | `3ff7f910d768cfb0` | `3ff7f910d768cfb0` | yes |
| 12 | `4000000000000000` | `4000000000000000` | yes |

The same in debug and release. So `ground/tests.md`'s measured fixture hash (`b3daee50…`, made with `libm::pow`) and
`facts.md`'s predicted offline hashes (`39f94764…`, `d752521b…`, made with `libm::exp2`) both hold for this design's
`libm::exp2`.
