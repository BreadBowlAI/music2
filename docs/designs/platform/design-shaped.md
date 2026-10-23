# The platform layer: design (shaped for what comes)

Written 2026-10-14 on the Linux server, at `f126d03`. Label: design-shaped.

Inputs: `HANDOFF.md`, the contract `docs/architecture.md`, the surveys `survey-code.md` (OS-01 to OS-85) and
`survey-later.md`, and the ground notes in `ground/` (`tests.md`, `e2e.md`, `facts.md`, `branches.md`, `ci.md`). The
ground notes were measured on this server and outrank the surveys. This design adds four measurements of its own
(Appendix A). Anything not checked on this machine or in source is marked *(unverified)*.

Owner update applied: there is **no GitHub Actions workflow** in this step. The Mac is checked by the owner, by hand,
with one script. From `ground/ci.md` only two things are used: what the Mac check must run, and the Linux plugin facts.

---

## 1. In short

**What changes**

| | |
|---|---|
| One new crate | `m2-os` (folder `core/os`) holds every piece of code that differs between macOS, Linux and Windows. It has one folder per OS. The rest of music2 stops caring which OS it runs on. |
| Linux | gets its missing pieces there: safe file swaps, file-change events, the plugin main thread, its folders. |
| Linux becomes fully green on this server | every Rust test, clippy, the desktop app's build and tests, all 29 browser suites, and the 8 offline renders. |
| Same sound on every computer | the few math calls on the export path switch to one shared math library (`libm`). |
| One check script | `ci/check.sh`, the same on Linux and on your Mac. One command. It ends with a short summary. |
| Mac code checked from Linux | every run on the server also compile-checks the Mac code and the Windows code. No Mac is needed for that. |
| Later steps | 8a (all CPU cores), 9 (plugin windows, recording, MIDI) and a Windows build each add one file per OS in `core/os`. Nothing moves again. |

**What you will notice**

| What | Details |
|---|---|
| Nothing on the Mac looks or sounds different. | The app, the menus and the look are not touched by this step. |
| One example song changes its fingerprint once. | Some samples of `examples/resample`'s export change in their very last bit. You cannot hear it. After that, every computer exports the same bits. |
| New songs never differ from an old folder only in capitals. | On every computer, "exports 2" counts as taken when "Exports 2" exists (question Q1). |
| Messages on the server name the computer oddly. | Lock messages say "Ubuntu-2404-noble-amd64-base", the server's real host name (question Q3). |

**What you must do once (on your Mac, at the end of this step)**

1. Pull `main`. Open Terminal on the Mac itself (not over ssh: one test needs your login session).
2. If Surge XT or Airwindows Consolidated are in `/Library/Audio/Plug-Ins/CLAP`, move them to
   `~/Library/Audio/Plug-Ins/CLAP`. The script tells you if they are in the wrong place.
3. Run `ci/check.sh`. Wait about 30 minutes *(estimate)*. Paste back the summary it prints at the end.

Three decisions are yours (section 10). Each has a recommended answer.

---

## 2. Where the layer lives, and its shape

### 2.1 The crate

```
core/os/                     crate m2-os. Depends only on libc (Unix). No other dependency.
  src/lib.rs                 the module list, unsupported()
  src/fs.rs                  durable writes, swaps, clones, file ids, volumes, the lock     (OS-01…11, 15…19)
  src/host.rs                machine id, host name, local clock                            (OS-12…14)
  src/watch.rs               file-change events for the watcher                            (OS-20…22)
  src/thread.rs              thread id, "is this the main thread", CPU time, leave now      (OS-24, 25, 29, 80, 83)
  src/main_loop.rs           the loop the plugin main thread runs                           (OS-23, 26, 27)
  src/dirs.rs                home, songs, cache, ~/.music2, CLAP folders, bundled files     (OS-32, 36…39)
  src/process.rs             memory, machine description, test crash helpers                (OS-72, 82, 84)
  src/unix.rs                code shared by macOS and Linux (fsync, flock, stat ids, paths as C strings)
  src/mac/{mod,fs,host,watch,thread,main_loop,dirs,process}.rs
  src/linux/{mod,fs,host,watch,thread,main_loop,dirs,process}.rs
  src/windows/{mod,fs,host,watch,thread,main_loop,dirs,process}.rs    compiles; Unsupported until the Windows step
```

- The portable files (`fs.rs`, `watch.rs`, …) hold the public API, its documentation and any logic shared by every OS.
  They contain no `cfg`.
- Each OS folder implements the same private functions with the same signatures. `lib.rs` picks one folder:

  ```rust
  #[cfg_attr(target_os = "macos", path = "mac/mod.rs")]
  #[cfg_attr(target_os = "linux", path = "linux/mod.rs")]
  #[cfg_attr(windows, path = "windows/mod.rs")]
  mod os;
  ```

- **"What is different on Linux?"** is answered by reading `core/os/src/linux/`. A missing function in one OS folder
  fails that OS's compile check, which runs on the server every time (section 7.2).
- **Mechanism, not policy.** `m2-os` does what the OS does. When the OS or the volume can't, it returns
  `ErrorKind::Unsupported`, naming what. It never falls back by itself. The fallback and its rule stay with the caller:
  the commit's "move aside, then rename" (`stage.rs`), the watcher's walks (`watcher.rs`), the silent player (`m2-play`).
- **Who depends on it:** `m2-song` (stamps), `m2-session` (commit, lock, watch), `m2-plugins` (main loop, CLAP
  folders), `m2-editor` (create-only names), `m2-server` (events, folders), `m2-gates`, `music2-desktop`, `m2`.
  `m2-os` depends on none of them.

### 2.2 The public API

Signatures as built in this step. One line of documentation each; the per-OS behaviour is in section 3.

```rust
// lib.rs
/// ErrorKind::Unsupported, naming what this OS or this volume can't do (PLAT-2).
pub fn unsupported(what: &'static str) -> std::io::Error;
```

```rust
// fs.rs: what a durable commit needs from the disk (WRITE-15, PLANE-5), and file identity.
pub enum Rename { Swap, Excl }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId { /* volume, file index */ }

/// Push a file's bytes (or a folder's entries) towards the drive. macOS: fsync, no cache flush.
/// Linux: fsync, which is durable on its own. Windows: FlushFileBuffers.
pub fn push(f: &File) -> io::Result<()>;
/// Make everything pushed so far durable: F_FULLFSYNC on macOS (else fsync); fsync on Linux.
pub fn barrier(f: &File) -> io::Result<()>;
/// Push a folder's new entries. Unsupported where folders can't be synced.
pub fn sync_dir(dir: &Path) -> io::Result<()>;
/// Swap two existing names, or rename only where nothing is. Unsupported when the volume can't;
/// AlreadyExists for Excl onto a taken name.
pub fn rename(from: &Path, to: &Path, how: Rename) -> io::Result<()>;
/// A copy that shares no inode with `from`, by cloning, then pushed. Unsupported: the caller copies.
pub fn clone_file(from: &Path, to: &Path) -> io::Result<()>;
/// What a path names, whatever it is called (links followed).
pub fn file_id(p: &Path) -> io::Result<FileId>;
pub fn file_id_of(f: &File) -> io::Result<FileId>;
/// (change time in ns, inode): the parts of a stamp std doesn't give portably (FMT-16, WRITE-19).
pub fn change_stamp(m: &Metadata) -> (i128, u64);
/// Whether a rename from `a` to `b` stays on one volume.
pub fn same_volume(a: &Path, b: &Path) -> io::Result<bool>;
/// Whether the volume holding `dir` is this machine's own (not NFS, SMB, 9p…): WRITE-17, WRITE-19.
pub fn is_local(dir: &Path) -> bool;
/// The last name of `p` as the disk stores it; None when nothing is there (WRITE-3a, OS-19).
pub fn stored_name(p: &Path) -> io::Result<Option<OsString>>;
/// Take the exclusive lock without waiting: Ok(true) taken, Ok(false) held elsewhere,
/// Err(Unsupported) when the volume can't lock (WRITE-17).
pub fn try_lock(f: &File) -> io::Result<bool>;
/// Let the user change a copied file, even if its source was read-only.
pub fn make_writable(p: &Path) -> io::Result<()>;
```

```rust
// host.rs: who this machine is, for journals and messages (FMT-20, WRITE-16, WRITE-18).
pub fn id() -> String;                                  // stable machine id
pub fn name() -> String;                                // host name, for messages
pub fn local_time(unix_secs: u64) -> Option<(u8, u8)>;  // (hour, minute), local time
```

```rust
// watch.rs: names of changed paths, as hints for the watcher (WRITE-19). Never contents.
pub const MAX_HINTS: usize = 4096;   // past this many in one tick: rescan
#[derive(Default, Debug)]
pub struct Hints { pub paths: BTreeSet<String>, pub rescan: bool }
pub struct Events { /* per OS */ }   // Send + Sync
impl Events {
    /// Watch `dir` (canonical) and every folder under it that `walked` allows. `wake` runs on the
    /// source's own thread when a hint is added. Err(why) when there are no events here.
    pub fn start(dir: &Path, walked: fn(&str) -> bool, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Events, String>;
    pub fn take(&self) -> Hints;
    pub fn put_back(&self, hints: Hints);
    /// Once this returns, every event for a change made before the call is in take().
    pub fn flush(&self);
    /// Events heard since start, for the gates.
    pub fn events(&self) -> u64;
}
```

```rust
// thread.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThreadId(usize);
pub fn current() -> ThreadId;          // never allocates or locks: the audio thread asks (RENDER-28)
pub fn is_process_main() -> bool;      // never allocates or locks
pub fn cpu_time() -> Duration;         // this thread's CPU time (G11)
pub fn interactive();                  // mark this thread latency-sensitive (step 7's G7f drop test)
pub fn leave_now(code: i32) -> !;      // end the process without exit handlers (HOST-2 quit)
```

```rust
// main_loop.rs: the loop the plugin main thread runs (HOST-2). Step 9 adds fds and timers here.
pub type Job = Box<dyn FnOnce() + Send>;
pub enum Where {
    /// The process's main thread, which the caller pumps with run_for (m2, the headless server, the gates).
    ProcessMain,
    /// An app whose toolkit owns the main thread: macOS, the main dispatch queue (AppKit drains it);
    /// Linux and Windows, a loop thread of its own.
    AppMain,
    /// A loop on a new thread with this name (tests, every OS).
    Thread(&'static str),
}
pub struct Loop { /* per OS */ }   // Send + Sync
impl Loop {
    pub fn start(at: Where) -> io::Result<Loop>;
    pub fn post(&self, job: Job);                 // never blocks; any thread
    pub fn is_current(&self) -> bool;             // never allocates or locks
    pub fn run_for(&self, most: Duration);        // ProcessMain only, on that thread: run what is due
}
```

```rust
// dirs.rs: every folder music2 uses outside a project. All come from home() (PLAT-8).
pub fn home() -> Option<PathBuf>;                         // $HOME first on every OS
pub fn songs() -> Option<PathBuf>;                        // where New Song puts songs
pub fn cache() -> Option<PathBuf>;                        // rebuildable caches (FMT-16, RENDER-30)
pub fn user_state() -> Option<PathBuf>;                   // ~/.music2 (session.json)
pub fn clap() -> Vec<PathBuf>;                            // CLAP's default folders, user first
pub fn bundled(exe: &Path, name: &str) -> Option<PathBuf>; // std, templates, examples, web next to the app
```

```rust
// process.rs
pub struct Memory { pub footprint: u64, pub mapped: u64, pub address_space: u64 }
pub fn memory() -> Option<Memory>;      // for step 7's G7f
pub fn machine() -> String;             // "MacBookPro18,1, macOS 26.6.2, arm64": every gate record names it
pub fn crash_quietly();                 // tests: this child aborts on purpose; no core dump, no crash report
pub fn aborted(status: &ExitStatus) -> bool;  // whether a child ended by abort()
```

How the plugin executor reads after the move (`core/plugins/src/main_thread.rs`, no `cfg` left):

```rust
pub struct MainThread { lp: m2_os::main_loop::Loop, closed: AtomicBool }
pub fn use_app_main_thread() -> Result<()>;            // was use_process_main_thread; the app, Where::AppMain
pub fn run_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T;  // Where::ProcessMain
pub fn test_main_thread();                             // Where::Thread("m2-plugin-main")
pub(crate) fn token() -> usize;                        // kept: step 7's probe.rs calls it 4 times
pub(crate) fn is_process_main() -> bool;               // kept: probe.rs calls it 2 times
```

### 2.3 What stays behind `cfg` outside `m2-os`, and why

| Where | What | Why it stays |
|---|---|---|
| `apps/music2-desktop/src/` | the window's title bar, the menus, "reveal in Finder", the 0600 session file | This is the shell. Tauri already hides most OS differences, and these lines change with the look (taste owns them). Moving them now would conflict with taste in 4 files (`branches.md` §3.2). |
| tests that need a Unix fixture | symlinks, permission bits, signals (`#[cfg(unix)]`) | They test portable code with a Unix-only setup. The Windows step rewrites or skips them. |
| `crates/gates` | G11's "disk refuses a write" (permission bits) | A measuring tool's fault injection, not product code. |
| `core/runtime/src/fpenv.rs` (8a) | FPCR on arm64, MXCSR on x86_64 | It depends on the CPU, not the OS. |
| crates that are already the OS layer | cpal (audio), Tauri (windows), libloading, rquickjs | No `cfg` in our code; they already speak one language. |

A grep test (slice S9) checks the rule (PLAT-1): outside `core/os`, the desktop shell and the gates, non-test code has
no `libc::`, no `std::os::unix` or `std::os::windows`, no `extern "C"` blocks for OS functions, and no `cfg` on the OS.

### 2.4 Why this shape

**Free functions in modules, not one trait with associated types** (`survey-later.md` §5 proposes `trait Os` with
`type This = mac::Mac`).

| | One trait, one type per OS | Modules with one folder per OS (chosen) |
|---|---|---|
| What it is | a namespace with extra syntax: every method is static, and only one OS exists per build | Rust's own namespace |
| Call sites | `<This as Os>::rename(…)`, `<This as Os>::Wake` | `m2_os::fs::rename(…)`, `m2_os::rt::Wake` |
| Every OS has every function | checked by the compiler | checked by the macOS and Windows compile checks, which run on every server check (measured cheap, section 7.2) |
| When a step's functions appear | declared up front for 8a and 9 ("sections"), before anyone builds them | each step adds its own module when it is built: no dead declarations |
| Types that differ by OS (`Group`, `Wake`, `Child`) | associated types leak into every signature | plain structs whose fields differ per OS folder |
| Behaviour the *program* picks (app or CLI loop; a test disk) | must be faked with more types | values: `Where::AppMain`; the session's `Disk` trait stays |

The trait's one real benefit, "the compiler says when Windows lacks a function", is delivered by the cross checks. A
trait does earn its place where code picks an implementation at run time. That already exists: the session's `Disk`
(tests inject faults). It stays in `m2-session`; `m2-os` only gives `Real` its calls.

**One crate, not `cfg` modules in each crate.**

| | `cfg` in each crate (today) | One crate (chosen) |
|---|---|---|
| Where OS code lives | 19 files in 8 crates today; 8a and 9 would add more in `m2-play`, `m2-runtime`, `m2-plugins` | one folder per OS |
| A Windows port touches | every crate | one crate |
| `unsafe` FFI | spread out | in one crate |
| Checking macOS code from Linux | needs stub C compilers for the whole workspace | `m2-os` alone needs none; the whole workspace works with stubs too (measured) |
| Shared pieces (`ThreadId`, `Wake`, `machine()`) | duplicated or tangled dependencies (`Stamp::of` exists twice today) | one place, below every crate |
| Cost now | none | one crate; about 700 lines move, all from files or hunks that no waiting branch edits (section 8.3) |

### 2.5 What changed from `survey-later.md` §5

- Kept: everything it says each OS does, the rule "Windows compiles from day one", and `Unsupported` until the Windows
  step.
- Changed: modules instead of a trait; no declared-but-empty sections for 8a and 9; `Group` and `Wake` become plain
  structs in an `rt` module that 8a adds.
- New: **one main loop for HOST-2 now and for plugin windows later.** The survey had a separate `GuiLoop` for step 9.
  Here the same `main_loop::Loop` serves the plugin main thread today and gains fds and timers in step 9, so CLAP's
  Linux extensions and the X11 loop land in the same Linux file.
- New: **on Linux and Windows the app's plugin main thread is a loop thread of its own**, not the toolkit's main thread.
  HOST-2's reason (plugins post work to the process's main run loop) is a macOS fact. Linux plugins get main-thread time
  only through the host's fd and timer extensions. This keeps GTK out of plugin hosting, and a slow plugin load can't
  freeze the Linux app's window. Contract text in §6.

### 2.6 Checking the macOS and Windows code from Linux (measured for this design)

The question was whether `cargo check --target aarch64-apple-darwin` works here. Measured with an isolated rustup
(Appendix A1):

| What | Result |
|---|---|
| Plain `cargo check` for macOS, per crate | `m2-ir`, `m2-kernel`, `m2-song`, `m2-plugins`: ok. Everything else fails in a build script that compiles C: wasmtime's `helpers.c`, QuickJS (`rquickjs-sys`), `objc2-exception-helper`. There is no macOS SDK here. |
| The same for Windows (`x86_64-pc-windows-msvc`) | `m2-ir`, `m2-kernel`, `m2-song`, `m2-plugins`, `m2-runtime`, `m2-play`: ok. The rest fails on QuickJS's C. |
| With a stub C compiler (`ci/stub-cc`, `ci/stub-ar`: write empty objects, succeed) | **the whole workspace, all targets, the desktop app included, checks and passes clippy `-D warnings` for macOS**: 11 s for `check`, 4 to 5 s for `clippy`, with dependencies already checked once. `check` and `clippy` never link, so the stubbed C is never used. |
| Windows, with the stubs | everything compiles except: the desktop app's build script (Tauri wants `llvm-rc`), and 15 errors in 7 files that use Unix-only APIs (OS-68, listed in §4.2). |
| Does it catch real mistakes? | Yes. In a scratch copy, an unused variable inside a macOS-only arm and a misspelled `libc::RENAME_SWAPP` both pass Linux's own check, and both fail the macOS check. |
| Cost | the macOS standard library (146 MB) and the Windows one (118 MB), added once with `rustup target add`. No sudo. |

So: **`ci/check.sh` on Linux runs `cargo clippy -D warnings` for macOS (whole workspace) and for Windows (workspace
without the desktop app).** What it catches and what it can't is in §7.2.

---

## 3. Every OS piece (OS-01 to OS-85)

Each piece says what this step does:
- **Built** (in slice Sn): working on macOS and Linux at the step's end.
- **Designed**: the function exists now; its body for some OS comes later, for the reason given.
- **Deferred** to a later step, with the reason.
- **No change**: right as it is.

Totals: 51 built, 2 designed, 6 deferred, 26 no change. The Windows column is one line each; every Windows body is
`Unsupported` (or plain std) until the Windows step.

### 3.1 Durable writes and renames (`stage.rs`, `render.rs`)

| id | Piece | This step | Linux, with honest notes | Windows plan |
|---|---|---|---|---|
| OS-01 | push (`fsync`) | Built (S2): `fs::push` | `fsync`. On Linux it also flushes the drive's cache, so each push is durable alone. 0.03 to 0.12 ms per 4 KB file here; `fdatasync` costs the same, so `fsync` stays (facts §2). | `FlushFileBuffers` (a full flush each call) |
| OS-02 | barrier (`F_FULLFSYNC`) | Built (S2): `fs::barrier` | `fsync` of the journal; cheap, since every push was already durable. A whole commit-like sequence: 0.56 ms p50 idle, 1.4 ms p95 while another program writes 1 GB (facts §2). Never `syncfs`: it waited up to 890 ms for other programs. Best case only: these NVMe drives have no volatile cache; a laptop drive is slower *(unverified)*. tmpfs makes nothing durable (tests only). | `FlushFileBuffers` |
| OS-03 | swap and create-only rename | Built (S2): `fs::rename` | `renameat2` with `RENAME_EXCHANGE` / `RENAME_NOREPLACE`. Both work on ext4 and tmpfs (measured). `EINVAL`, `ENOSYS`, `EOPNOTSUPP` mean `Unsupported`: NFS, 9p, exFAT, and swap on SMB take the fallback, which S6 makes recoverable. Also closes the 0.2 ms "file missing" window live3a crashed on (e2e.md). | Excl: `MoveFileExW` without replace. Swap: Unsupported (NTFS has no exchange) → the fallback; retry briefly on sharing violations. |
| OS-04 | clone (`clonefile`) | Built (S2): `fs::clone_file` | `ioctl(FICLONE)` on btrfs and XFS. ext4 gives `EOPNOTSUPP` (measured), so `Unsupported`, and the caller copies: a 50 MB plugin blob is copied once per commit that replaces it. | Unsupported → copy (ReFS block clone later) |
| OS-05 | folder sync | Built (S2): `fs::sync_dir` | `fsync` of a folder opened read-only (works; never `O_PATH`, which gives `EBADF`). `EINVAL` from some FUSE volumes → `Unsupported`; `stage.rs` then goes on without it, and the renames are as durable as that filesystem makes them *(unverified per filesystem)*. | Unsupported (folders can't be synced); whether the rename's log record is durable: check in the Windows step |
| OS-06 | `cpath()` | Built (S2): moved to `core/os/src/unix.rs` | same code | not needed |
| OS-07 | export's create-only name (hard link) | Built (S2): `render.rs`'s `name_new` uses `fs::rename(Excl)` on every OS; "look, then rename" stays only for `Unsupported` | Also fixes FAT sticks, where `link` gives `EPERM`, which never reached the fallback (survey; *unverified here*) | `MoveFileExW` without replace |

### 3.2 The project lock and machine identity (`lock.rs`)

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-08 | the lock | Built (S2): `fs::try_lock` | std's `flock`: per open file, freed at exit and at SIGKILL (measured). On NFS it becomes a per-process lock, and on SMB a mandatory one; both are network volumes, so the session raises `lock.local` there. | `LockFileEx` on one byte past the end, so others can still read who holds it |
| OS-09 | "this volume can't lock" | Built (S2): inside `try_lock` | `ENOTSUP` (= `EOPNOTSUPP`, 95), `ENOLCK` (NFS without lockd) | `ERROR_NOT_SUPPORTED`, `ERROR_INVALID_FUNCTION` |
| OS-10 | is the held file still `.music2/lock`? | Built (S2): `fs::file_id`, `fs::file_id_of` | `(st_dev, st_ino)`, the macOS code | volume serial and 128-bit file id (`FileIdInfo`) |
| OS-11 | `.music2/tx/` on the project's volume | Built (S2): `fs::same_volume` | `st_dev`. btrfs subvolumes differ, and are refused, which is right. | volume serial |
| OS-12 | machine id | Built (S2): `host::id` | `/etc/machine-id` (present here), then `/var/lib/dbus/machine-id`, then the host name. Not DMI's UUID (root only). | `MachineGuid` in the registry |
| OS-13 | host name | Built (S2): `host::name` | `gethostname`: here "Ubuntu-2404-noble-amd64-base" (Q3) | `GetComputerNameExW` |
| OS-14 | "since 14:02" | Built (S2): `host::local_time` | `localtime_r` | `localtime_s` |
| OS-15 | local or network volume | Built (S2): `fs::is_local` | `statfs` type: NFS `0x6969`, SMB `0x517B`, CIFS `0xFF534D42`, SMB2 `0xFE534D42`, 9p `0x01021997`, Ceph `0x00C36400` are network. FUSE counts as local; on sshfs that is wrong, and the watcher's "events missed changes, walk instead" rule covers it. Measured: ext4 `0xEF53`, tmpfs `0x01021994`. | `GetDriveTypeW` = remote, or a `\\server\share` path |

### 3.3 File identity, stamps and spelling

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-16 | stamp: ctime and inode | Built (S2): `fs::change_stamp` replaces the two copies of `Stamp::of` (`watch.rs`, `hash.rs`) | real ctime in ns and inode | `ChangeTime` and the file index need a handle; today's `created()` and 0 until then |
| OS-17 | Tx spelling by `(dev, ino)` | Built (S2): calls `fs::file_id` | same | same call |
| OS-18 | editor `file_id` | Built (S2): calls `fs::file_id` | same | same call |
| OS-19 | stored spelling (`getattrlist`) | macOS moved (S3). Linux **designed**: list the parent folder, only on a volume that folds case. Not built: no case-folding volume here to test it (vfat or casefold ext4 needs root). On plain ext4, "spelled as asked" is already right. | — | `GetFinalPathNameByHandleW`. Trigger for both: the Windows step, or a Linux user on vfat or casefold. |

### 3.4 File-change events (`fsevents.rs`, `watcher.rs`)

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-20 | the event source | Built (S3): `watch::Events` | **inotify.** One inotify fd per watcher (the limit, 128, is per user and shared with editors). One watch per folder a walk enters (limit 997,324 here). A new folder gets a watch, then a hint, and the watcher walks it, which covers what was written before the watch. A thread `poll`s the fd, drains it under the hints lock, and calls `wake` (this fixes test 4). `flush` drains the fd at once: an event is queued before the writer's call returns (0 misses in 2.8 million checks, facts §3). Rescan on `IN_Q_OVERFLOW` (after exactly 16,384 queued events), on the root moved or deleted, and on a folder renamed (the watch map is rebuilt). Names are hints; rename cookies are never paired. Writes through `mmap` show only at close; the settle rule and the walks cover that. `ENOSPC` or `EMFILE` → walks, with the reason printed. About 250 lines *(estimate)*. | `ReadDirectoryChangesW`, recursive; flush by a sentinel write. Walks until then. |
| OS-21 | choosing events or walks | Built (S3): `watcher.rs` has no `cfg`: `Events::start` failing, or `fs::is_local` false, means walks | inotify sees only this machine's writes, so network volumes walk, as on macOS | same |
| OS-22 | `mod fsevents` behind `cfg` | Built (S3): FSEvents moves as it is to `core/os/src/mac/watch.rs`; `walked` and `MAX_HINTS` become parameters | — | — |

### 3.5 The plugin main thread (HOST-2)

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-23 | the main dispatch queue and `CFRunLoop` | Built (S2): moved into `mac/main_loop.rs` | — | — |
| OS-24 | `is_process_main()` | Built (S2): `thread::is_process_main` | `gettid() == getpid()`: two plain system calls, no allocation, no lock. Fixes `main_thread.rs:78` (tests.md). | the id recorded when `Loop::start(ProcessMain)` runs |
| OS-25 | thread token | Built (S2): `thread::current` | `pthread_self` through `libc` | `GetCurrentThreadId` (today's bare `pthread_self` would not link there) |
| OS-26 | the app's executor | Built (S2): `Where::AppMain` | a loop thread of its own. GTK keeps its main thread; plugin hosting never touches GTK. Tested by the AppKit test made portable (OS-30): on Linux it installs as the app does and plays the same song with the probe plugin, on the loop thread, with no window. | the same (a message pump comes with the Windows step) |
| OS-27 | the CLI's loop | Built (S2): `Where::ProcessMain`, the same channel loop as today, moved. CLAP's `timer-support` and `posix-fd-support`: **deferred to step 9**: Surge XT and Airwindows already pass live3b's plugin part on Linux without them (the lead's run, 51 of 51, `$SCRATCH/logs/live3b-plugins-1014-210631-833394.log`). In step 9 the Linux loop becomes `epoll` over an eventfd, a timerfd and the plugins' fds, in the same file. | Win32 message pump (Windows step) |
| OS-28 | the probe records `process_main` | No change: OS-24 fixes it | | |
| OS-29 | `leave_now` (`_exit`) | Built (S2): `thread::leave_now` | `_exit` | `TerminateProcess`, which skips DLL detach |
| OS-30 | the AppKit test | Built (S2): renamed `app_main_thread.rs`. Its work (the song with the probe plugin, the carried instance, quit's teardown, the plugins' part) runs on every OS; only AppKit's run loop stays under `cfg(target_os = "macos")`. No branch edits this file. Every item is then used, which ends clippy lints 7 to 15. | the same work on the app's loop thread; no display needed | the same file |
| OS-31 | host extensions offered | Deferred to step 9, with OS-27 | | |

### 3.6 CLAP discovery and loading (`core/plugins/src/lib.rs`)

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-32 | search paths | Built (S5): `dirs::clap()` after `CLAP_PATH` | `~/.clap`, `/usr/lib/clap`. The suites' plugins stay in `$SCRATCH/plugins/clap`, named only by `CLAP_PATH`: `fx-fixes` needs them missing everywhere else. | `%LOCALAPPDATA%\Programs\Common\CLAP`, `%COMMONPROGRAMFILES%\CLAP` |
| OS-33 | the binary in a bundle | No change: a Linux `.clap` is the file itself | | a DLL |
| OS-34 | scan depth | No change: one level on every OS, though the spec says recursive. Not OS-specific; decide in step 9. | | |
| OS-35 | `dlopen` | No change: `RTLD_LAZY \| RTLD_LOCAL` loads and plays Surge XT and Airwindows here | | `load_with_flags(LOAD_WITH_ALTERED_SEARCH_PATH)`; libloading's default lacks it (facts) |

### 3.7 Paths and folders

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-36 | songs folder | Built (S5): `dirs::songs()` | `XDG_MUSIC_DIR` from `~/.config/user-dirs.dirs`, else `~/Music`; then `music2`. With a scratch `HOME` the suites still find `<home>/Music/music2`. | `FOLDERID_Music` (often inside OneDrive: `project.synced`) |
| OS-37 | cache folder | Built (S5): `dirs::cache()` | `$XDG_CACHE_HOME/music2` when absolute, else `~/.cache/music2` | `%LOCALAPPDATA%\music2\cache` |
| OS-38 | `~/.music2/session.json` | Built (S5): `dirs::user_state()`, the same path on every OS; mode 0600 stays in the shell | same | the profile's private ACL |
| OS-39 | bundled std, templates, examples | **Designed** (S5): `dirs::bundled(exe, name)`; macOS moved; Linux and Windows bodies come with packaging (OS-50) | `<exe>/../lib/music2/<name>` (Tauri's deb and AppImage layout); dev builds use the source tree, as now | the program's folder |
| OS-40 | source tree | No change | | |
| OS-41 | song folder names | Built (S4): a new song's name counts as taken in any capitals, on every OS (Q1) | ext4 tells cases apart; the rule doesn't depend on it | also forbid `< > " \| ? *`, a trailing dot or space, device names (Windows step) |
| OS-42 | asset names Windows can't create | Deferred to the Windows step: a format rule, the owner's call then | legal | |
| OS-43 | `~` in shown paths | Deferred to the Windows step | fine | `\` paths |
| OS-44 | unknown files block a folder rename (WRITE-12) | No change | an editor's `name~` backup refuses the rename, and says why | treat `Thumbs.db`, `desktop.ini` like `.DS_Store` |

### 3.8 Floating point and render determinism (details in section 5)

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-45 | host math on the render path | Built (S1): the `libm` crate for the 8 calls; one deliberate change of the recorded hashes | measured new hashes (§5.3) | same code |
| OS-46 | the kernels' math | No change: inside `std.wasm` it is the same `libm`, bit for bit (facts §6f) | | |
| OS-47 | `resample.rs` (`sin`, `cos`, f64) | Built (S1): `libm::sin`, `libm::cos` | live only; its f32 tables come out identical (facts §6d) | |
| OS-48 | wasmtime settings | No change: 6 of 8 renders already equal the Mac's on x86_64 | | |
| OS-49 | denormals | Deferred to 8a | x86 pays a microcode assist per subnormal. Kernels must keep their state out of the subnormal range, deterministically (a std change); `fpenv` sets MXCSR on x86_64. | same (x86) |

### 3.9 The desktop shell (Tauri)

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-50 | bundle targets | Deferred to a packaging step (trigger: a Linux release). Known: webkit2gtk-4.1, gtk-3, soup-3, libasound; built here it needs glibc 2.39 or newer (facts §9). | | NSIS or MSI, WebView2, signing |
| OS-51 | `gen/schemas/<os>-schema.json` | Built (S2): commit `linux-schema.json`. It is identical to `macOS-schema.json` (checked). | | commit `windows-schema.json` then |
| OS-52 | the title bar | No change on main: taste owns the window builder | native decorations above the top bar | same |
| OS-53 | `__M2__.platform`, `isMac` | Built (S4): `isMac` follows `__M2__.platform` when a page is given one (`native.ts`, one line) | | |
| OS-54 | menus | Deferred to the taste merge (taste rewrote `menu.rs`): Quit in File, About in Help, no Services or Hide (GTK greys them out) | | Redo Ctrl+Y; File ▸ Exit |
| OS-55 | reveal in Finder | No change in code: on Linux it opens the folder (`open` tries `xdg-open`, then `gio`, which is here). The three labels become one with the key table, after taste. | | `explorer /select,` |
| OS-56 | file dialogs | No change (GTK chooser; needs a display) | | IFileDialog |
| OS-57 | `windows_subsystem` | No change | | already there |
| OS-58 | JIT and hardened runtime | No change (only confined SELinux domains refuse `execmem`) | | |

### 3.10 Audio (`core/play`)

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-59 | the device host | No change in the host (ALSA, cpal's default with no features). PipeWire's feature: 8a decides (it needs `libpipewire-0.3-dev` and clang). **Plus a portable fix (S7):** cpal's errors sorted by kind. An xrun is counted and play goes on; a device change makes a new player; only "invalidated" or "unavailable" stalls the player. The error callback only bumps counters, because on ALSA it runs on the audio thread (survey-later bug 1). | | WASAPI shared (ASIO decided in the Windows step) |
| OS-60 | the silent player | No change: `thread::sleep` is accurate (50 µs slack) | | high-resolution timers in std |
| OS-61 | no audio device | Built (S7): no device, **or a device that fails to open**, gives the silent player and one message. On this server cpal finds "Default Audio Device", then `default_output_config` fails with "The requested audio device is not available" (facts §5). | | same |
| OS-62 | capture player (live3b) | No change | | |

### 3.11 Build and toolchain

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-63 | `std/build.sh` | Built (S8): its own target folder, and `--check` | | Git Bash, or a `.ps1` twin |
| OS-64 | machine path in `std.wasm` | Built (S8): `m2-kernel` moves into the kernels' workspace; one deliberate rebuild (§5.4) | | same |
| OS-65 | toolchain pin | Built (S1): `rust-toolchain.toml` | | same file |
| OS-66 | system libraries | No change. All present here (facts §9). For a fresh Ubuntu 24.04: `pkg-config build-essential libasound2-dev libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libdbus-1-dev`. | | MSVC build tools; QuickJS with cl.exe *(unverified)* |
| OS-67 | npm lockfiles | No change: they list the Linux and Windows bindings | | |

### 3.12 Tests and browser suites

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-68 | Unix-only APIs outside `cfg(unix)` | Built (S2): `cfg(unix)` on those tests, `process::aborted` for the abort checks, the gates' fault injection under `cfg(unix)` with a Windows "skipped", `thread::cpu_time` | | compiles (15 errors in 7 files today, §4.2) |
| OS-69 | permission tests fail as root | No change: the server runs them as a normal user | | ACLs |
| OS-70 | symlink tests | No change (`cfg(unix)`) | | Developer Mode |
| OS-71 | temp folders are case-sensitive | No change: the case-insensitive paths are covered by the Mac run | | NTFS folds case |
| OS-72 | aborting children dump core | Built (S2): `process::crash_quietly()` (`prctl(PR_SET_DUMPABLE, 0)`) in the kill and hook test children. The kill test drops from 473 s to 104 s (measured, tests.md §1c). `RLIMIT_CORE=0` is not enough with apport. | | turn crash reports off for the child |
| OS-73 | the Chrome path | Built (S4): chosen per OS in `run.mjs` and `chrome.mjs` (`mac_arm`, `linux64`) | | `win64` |
| OS-74 | `isMac` in the suites | Built (S4): with OS-53; the suites keep injecting "macos" | | |
| OS-75 | ⌘ keys inside Monaco | Built (S4): Chrome gets a Mac user agent from `e2e/chrome-mac.sh`, one place (Q2). A PC-keys pass comes with the key table (8b). | | a `.cmd` twin of the wrapper |
| OS-76 | fonts from Google Fonts | No change on main: taste removes them (conflict otherwise). Until then the suites need the network, which this server has. | | |
| OS-77 | `shasum` | No change: it exists here; `ci/check.sh` uses `sha256sum` or `shasum` | | |
| OS-78 | suite signals | No change | | `taskkill /T` |
| OS-79 | Chrome's libraries | No change (present) | | |

### 3.13 Gates and measurements

| id | Piece | This step | Linux | Windows plan |
|---|---|---|---|---|
| OS-80 | thread CPU time | Built (S2): `thread::cpu_time` | `CLOCK_THREAD_CPUTIME_ID` | `QueryThreadCycleTime` |
| OS-81 | the timed barrier, labelled `F_FULLFSYNC` | Built (S2): the gates time `fs::barrier` and say "the durable sync"; G11's "watched by FSEvents" says "watched by file system events" | | |
| OS-82 | memory for G7f (step 7) | Built (S9): `process::memory()` | `/proc/self/status`: footprint = `RssAnon` + `VmSwap`, mapped = `VmSize`; address space from the CPU's virtual address bits (48 here: 128 TiB for the user) and `RLIMIT_AS` | `GetProcessMemoryInfo` |
| OS-83 | QoS for G7f's audio-like thread | Built (S9): `thread::interactive()` | nothing: no realtime rights here (`RLIMIT_RTPRIO` 0, no rtkit); it says so | `SetThreadPriority` |
| OS-84 | M1 records | Built (S9): every gate record names its machine (`process::machine()`); a Linux run is recorded beside the M1's, never judged against it (PLAT-7) | | same |
| OS-85 | the reference Mac | No change: written into PLAT-3 and PLAT-7 | | |

---

## 4. Every Linux failure, and the change that fixes it

From `ground/tests.md` and `ground/e2e.md`, at `f126d03`.

### 4.1 Rust tests (11)

| # | Test | Cause | Fixed by |
|---|---|---|---|
| 1 | `graph::watch_tests::an_idle_project_is_watched_without_a_stat` | no events off macOS | S3 (inotify) |
| 2 | `graph::watch_tests::own_commits_make_no_batch_with_events` | same | S3 |
| 3 | `graph::watch_tests::events_that_miss_changes_give_way_to_walks` | same | S3 |
| 4 | `graph::watch_tests::an_event_wakes_an_idle_watcher_and_a_batch_wakes_it_when_due` | nothing calls `wake` off macOS | S3: the inotify thread calls `wake` |
| 5 | `graph::tests::a_lane_written_in_two_halves_never_plays_empty` | asserts `events == !walk` | S3; its event-mode case is re-run there, since inotify names every write, not only the close (tests.md §1d) |
| 6 | `tx::hook_tests::a_commit_that_fails_after_its_preview_accepts_nothing` | no swap: every replace took the fallback | S2 (`renameat2`) |
| 7 | `tx::stage_tests::a_rollback_that_fails_is_reported_at_once` | same | S2 |
| 8 | `tx::kill_tests::killed_at_every_commit_step_the_project_recovers` | same, and a real recovery gap | S2 makes it pass on ext4; S6 fixes the gap and runs the test on a disk that can't swap too |
| 9 | `graph::preview_tests::an_interrupted_commit_after_its_preview_points_to_needs_attention` | same | S2 |
| 10 | `gate_tests::fixture_renders_change_only_with_stds_version` | glibc's `cosf` in `audio.rs` | S1 (`libm`, record once) |
| 11 | `core/plugins/tests/main_thread.rs:78` | `is_process_main()` false off macOS | S2 (`gettid() == getpid()`) |

Tests 6, 7, 8 (on ext4), 9 and 11 were seen passing with these exact fixes in a scratch copy (tests.md). Test 10 with
`libm` was measured for this design (§5.3).

### 4.2 Clippy (15 lints) and the Windows compile check

| # | Lint (`-D warnings`, whole workspace) | Fixed by |
|---|---|---|
| 1 | `core/plugins/src/main_thread.rs:217` `install` never used | S2: every `Where` installs through it |
| 2 | `core/session/src/lock.rs:213` `dir` unused | S2: `fs::is_local` has a Linux body |
| 3 | `core/session/src/tx/stage.rs:106` `cpath` never used | S2: moved; `renameat2` uses it |
| 4 | `crates/server/src/graph/watcher.rs:55` `wake` never used | S3 |
| 5 | `watcher.rs:134` `backstop` never read | S3 |
| 6 | `watcher.rs:216` `deaf` never read | S3 |
| 7 to 15 | `apps/music2-desktop/tests/appkit_main_thread.rs` 28, 115, 117, 125, 147, 150, 159, 241, 265 | S2: the test's work runs on every OS (OS-30) |

The macOS compile check passes clippy today (measured). The Windows one needs these 15 errors fixed (S2), all Unix-only
APIs (OS-68), and lints 4 to 6 gone (S3): `crates/gates/src/server.rs` 1019, 1025, 1028, 1033; `crates/gates/src/server/watch.rs` 26;
`core/session/src/tx/hook_tests.rs` 5, 394; `kill_tests.rs` 9, 159; `tx/tests.rs` 660, 661, 842;
`crates/server/src/export.rs` 368; `crates/server/src/graph/preview_tests.rs` 5, 173, 180.

### 4.3 The desktop app

| Check | Today on Linux | After |
|---|---|---|
| `cargo build -p music2-desktop` | ok | ok; `linux-schema.json` committed, so the build leaves the tree clean |
| `cargo test -p music2-desktop` | exit 0, but tests nothing on Linux | `app_main_thread.rs` (the AppKit test, made portable) tests the app's plugin main thread on Linux too (S2) |
| plugins in the Linux app | "plugins can't be hosted" | hosted on the app's loop thread (S2) |

### 4.4 Browser suites (7 FAIL, 1 PART)

| Suite | Cause | Fixed by (S4) |
|---|---|---|
| code, fixes-graph, attention, s6-write | Monaco takes its key map from the user agent, so ⌘ chords do nothing on Linux | `e2e/chrome-mac.sh`: Chrome reports a Mac user agent, on every OS. Measured with this fix: the 4 pass in full (e2e.md, branches.md). |
| export | off the app, `isMac` reads `navigator.platform` | `native.ts`: `isMac` follows `__M2__.platform` when given |
| fx-fixes | ext4 tells "exports 2" from "Exports 2" | the product: a new song's name is taken in any capitals, on every OS (Q1); the suite stays as it is |
| notes ("the whole drag is one write") | the suite reads the file, then the revision, and Linux saves so fast it lands between the two (2.7 ms) | `notes.mjs:97`: wait until the revision moves, wait 300 ms, then check it moved by exactly one |
| live3b (PART) | no plugins | `CLAP_PATH=$SCRATCH/plugins/clap`. The lead unpacked Surge XT 1.3.4 and Airwindows Consolidated there (no sudo; `ldd` finds every library). Measured: live3b 51 of 51 with it. `ci/check.sh` sets it when the folder has both. |

Passed only by luck today, and what this step does:

| Luck | This step |
|---|---|
| a saved file is missing for up to 0.19 ms during each save (live3a crashed once in 11) | gone with `renameat2` (S2) |
| layout checks need Inter from Google Fonts | taste removes web fonts; nothing on main |
| `s6-fixes.mjs:112` has the notes race but hasn't lost yet | fixed after the taste merge (taste rewrote that suite) |
| Linux users press Ctrl; no suite presses it | the PC-keys pass, with the key table (Q2) |

### 4.5 What "Linux green" means

All of these, on this server, in one `ci/check.sh` run:
- `cargo build --workspace --all-targets` and `cargo test --workspace --no-fail-fast`, the desktop app included: 0
  failures (today 686 pass, 11 fail);
- `cargo clippy --workspace --all-targets -- -D warnings`: clean;
- the macOS and Windows compile checks (§2.6): clean;
- `web` builds; the kernels' own tests and lints pass; `std.wasm` rebuilds byte for byte;
- `node e2e/run.mjs all`: 29 of 29 ok. live3b may be PART only when the plugins folder is missing;
- the 8 offline renders match `e2e/renders.sha256`, the one file both OSes check;
- the check leaves the repo unchanged.

---

## 5. Determinism

### 5.1 Host math goes through `libm`

The rule (PLAT-5): code on the render path never calls the OS's math library for a transcendental function. It calls
the `libm` crate (0.2.16, already in `Cargo.lock`), which is the same code compiler-builtins puts inside `std.wasm`
(facts §6f: equal bit for bit on every `f32` input and 67 million `f64` inputs).

| File:line at `f126d03` | Today | After (S1) |
|---|---|---|
| `core/runtime/src/audio.rs:191` (stretch window) | `(TAU * i / FRAME).cos()` | `libm::cosf(TAU * i / FRAME)` |
| `audio.rs:253` (pitch) | `2f64.powf(clip.pitch / 12.0)` | `libm::exp2(clip.pitch / 12.0)` |
| `audio.rs:283` (gain) | `10f32.powf(gain_db / 20.0)` | `libm::powf(10.0, gain_db / 20.0)` |
| `audio.rs:294`, `297` (fades) | `(PI * i / fade).cos()` | `libm::cosf(…)` |
| `audio.rs:411`, `424` (declick) | `(FRAC_PI_2 * k / len).sin()` | `libm::sinf(…)` |
| `audio.rs:415` (declick) | `(FRAC_PI_2 * k / len).cos()` | `libm::cosf(…)` |
| `core/play/src/resample.rs:46`, `49` (live only) | `.sin()`, `.cos()` on f64 | `libm::sin`, `libm::cos` |

- **`libm::exp2` by name, not `pow(2, x)`.** Release builds turn `2f64.powf(x)` into `exp2`, debug builds call `pow`
  (facts §6a). Naming one function makes debug and release agree on every OS.
- **A grep test** (`core/runtime/tests/no_host_math.rs`) fails when non-test code in `core/runtime/src` or
  `core/play/src` calls `.sin(`, `.cos(`, `.tan(`, `.powf(`, `.exp(`, `.exp2(`, `.ln(`, `.log`, `.tanh(`, `.atan`,
  `.cbrt(`, `.hypot(` or `.sin_cos(`. `sqrt`, `powi`, `floor` and arithmetic are exact everywhere and stay.
- `crates/editor/src/render.rs:307` (`log10` for a peak shown in dB) and taste's colour names (`layout.rs`) are display
  only, not the render path. They stay.
- step 7's moved `audio.rs` takes all 8 edits cleanly and adds no new host math (branches.md §2.2).

### 5.2 Song code: `Math` inside QuickJS (found for this design)

Song code runs in QuickJS. QuickJS's `Math.sin`, `Math.cos`, `Math.pow`, `Math.exp`, `Math.log` and the rest, and the
`**` operator, call the C math library of the OS (`rquickjs-sys 0.14.0`, `quickjs.c` 15025-15037 and 49019-49050, read
in source). So a song that writes `440 * 2 ** ((n - 69) / 12)` can build different constants on the Mac and on Linux.
glibc and `libm` already differ on 3 to 10% of `sin`, `cos` and `pow(2, x)` inputs (facts §6b); Apple's library is a
third implementation *(how often it differs from glibc: unverified)*. No fixture and no std file uses `Math` today
(checked with grep), so no recorded hash shows it yet.

Fix, in S1, in the place that already removes `Date` and `Math.random` (`core/script/src/lib.rs`, `PRELUDE`):
- replace `Math.sin, cos, tan, asin, acos, atan, atan2, sinh, cosh, tanh, asinh, acosh, atanh, exp, expm1, log, log1p,
  log2, log10, pow, cbrt, hypot` with functions backed by the `libm` crate (rquickjs native functions);
- have the TypeScript transform lower `a ** b` to `Math.pow(a, b)` (oxc's ES2016 exponentiation transform), so the
  operator takes the same path.

This is the decided "deterministic math" rule applied to the build. It changes no recorded hash. A test builds a song
that uses `Math.sin`, `Math.pow` and `**`, and compares its constants with `libm`'s bits.

### 5.3 The one deliberate change of recorded hashes

Measured for this design with exactly the §5.1 patch (`libm::exp2` for pitch) on Linux (Appendix A2):

| Recorded where | Fixture | Mac at `8aa301e` | Linux at `f126d03` | After S1, measured on Linux |
|---|---|---|---|---|
| `core/session/tests/renders.json` | `examples/resample` | `cb4d39ba…` | `e2e93ef0…` | `b3daee50ee7e7193fc6d240f02384fcda83ccbe33b5c20fbf5904005b4aeb58e` |
| `e2e/renders.sha256` | `examples-resample.wav` | `6ac6fcb1…` | `d902bacf…` | `39f9476492e997a86aec5f94b6d0edf56c098e275186260915a3830bd9cfe0da` |
| `e2e/renders.sha256` | `examples-resample-from4-for6.wav` | `a360d04c…` | `ea243ccc…` | `d752521b47b66bcb9076f43715baedaa5c018f428ec90546465e6cae031560eb` |
| both | the other 3 fixtures and 6 renders | | equal to the Mac's | unchanged |

- Both new WAV hashes equal the ones facts.md predicted with `libm` swapped in through `LD_PRELOAD`. The `renders.json`
  hash equals tests.md's (made with `libm::pow`); for these pitches `exp2` and `pow` agree.
- The Mac must produce the same three hashes. *(unverified until the owner's Mac run.)* It should: `libm` is plain
  Rust arithmetic, and Rust never fuses or reorders floating point.
- The commit says it plainly: "Deliberate change of recorded renders (decided): host math now uses the libm crate;
  examples/resample changes in renders.json and e2e/renders.sha256; no other render changes."
- `e2e/renders.sha256` (new) holds all 8 lines, named as `baseline.sh` names the files (`-from4-for6`), one file for
  every OS. `baseline.sh` gains the eighth case (`templates/empty --from 4 --seconds 6`).

### 5.4 `std.wasm` rebuilds the same everywhere

| Fact | Source |
|---|---|
| `std.wasm` holds the Mac's path `<repo>/core/kernel/src/lib.rs` | facts §7, ci.md |
| Remapping that path rebuilds main's file only from `650f842`'s sources; at HEAD 10 bytes (panic line numbers) differ: the committed file is stale | facts §7 |
| A remap is not enough: cargo hashes `m2-kernel`'s absolute path into `-C metadata`, which reorders the layout (1,003 bytes differed between two folders for `sounds`) | branches.md §4.3 |
| With `m2-kernel` inside the kernels' own workspace, two folders gave identical bytes with no machine path | branches.md §4.3 |

The fix (S8):
- move `core/kernel` to `std/kernels-src/kernel` (a member of the kernels' workspace). The main workspace keeps
  `m2-kernel = { path = "std/kernels-src/kernel" }` as a path dependency (the folder is already excluded), as
  `m2-runtime`'s dev-dependency on `m2-std-kernels` already works today;
- `rust-toolchain.toml` pins 1.99.0 (S1);
- `std/build.sh` builds into its own temporary target folder (today, with `CARGO_TARGET_DIR` set, it copies from the
  wrong folder), refuses a file that contains `/Users/` or `/home/`, and gains `--check`: rebuild and compare;
- rebuild once, on purpose. `std.wasm`'s bytes change, so every kernel's module hash changes. Renders don't: the code
  is the same, and kernels start from init in every offline render anyway (RENDER-1, RENDER-25). S8 proves it:
  `renders.json`, `bit_identity` and the 8 renders stay byte-identical.

### 5.5 x86 and arm inside wasm

- Measured: 6 of 8 renders are bit-identical between the Mac (arm64) and this Ryzen (x86_64). The only differences were
  host math (facts §6e). So the kernels' math is already the same on both CPUs.
- Why it holds: wasm's basic float operations are exact IEEE on both; Cranelift never fuses a multiply-add (wasm has
  none outside relaxed SIMD, which is off); `libm` inside the module is integer and IEEE code. Only NaN bit patterns may
  differ, and the renderer silences non-finite samples (OS-48).

### 5.6 Denormals on x86 (OS-49): an 8a and gate item

- x86 cores take a slow path for each subnormal operation; Apple silicon doesn't. Decaying tails (reverb, filters)
  can cost much more on this server *(how much: unverified)*.
- Flushing them in hardware (FTZ, DAZ) would change bits, which RENDER-2 forbids.
- 8a decides: kernels keep their state out of the subnormal range by a deterministic rule (a std change, with a std
  version bump per FMT-3), and `fpenv` writes `MXCSR = 0x1F80` at each callback on x86_64, the twin of the FPCR check.
- G12 stays judged on the reference Mac; an x86 run of p50's tails is recorded here.

---

## 6. Contract edits

The rules keep their ids. Each edit below replaces the quoted text exactly. Edits land in the slice that changes the
code they describe (noted in brackets).

### 6.1 Rules that name macOS mechanisms

**PLANE-5** (S2)
- Rule: "and the commit goes on to journal, stage, sync (`F_FULLFSYNC`) and rename." →
  "and the commit goes on to journal, stage, make durable (one barrier, `m2_os::fs::barrier`: `F_FULLFSYNC` on macOS,
  `fsync` on Linux) and rename."
- *Why*, add at the end: "On Linux the whole commit took 0.6 ms idle and 1.5 ms at p95 while another program wrote 1 GB,
  on the development server's power-loss-protected drives (2026-10-14); a laptop drive is slower."

**WRITE-15** *Today* (S2, S6)
- "pushed with `fsync`; one barrier (`F_FULLFSYNC`); renames checked at each step (`RENAME_SWAP`, `RENAME_EXCL`),
  rolled back in process when one finds an outside change." →
  "pushed with `fsync`; one barrier (`F_FULLFSYNC` on macOS; `fsync` on Linux, where each push is already durable);
  renames checked at each step (an exchange and a create-only rename: `renamex_np` on macOS, `renameat2` on Linux,
  `m2_os::fs::rename`), rolled back in process when one finds an outside change. A volume without them (FAT, exFAT, SMB,
  NFS) takes the fallback: the target moves aside into the staging folder as `old-<k>`, then the new file comes in;
  recovery can finish or undo that middle state ([WRITE-16](#commit-journal-and-recovery))."

**WRITE-16** (S6)
- Rule: "Each fix is allowed only while every target still matches the hash the journal recorded before or after the
  Tx." → "Each fix is allowed only while every target still matches the hash the journal recorded before or after the
  Tx; a target the commit itself moved aside, unchanged, into its staging folder counts as at its before hash (the
  fallback's middle state, [WRITE-15](#commit-journal-and-recovery))."
- *Today*: "another host is told by its hardware UUID" → "another host is told by `m2_os::host::id()` (the hardware
  UUID on macOS, `/etc/machine-id` on Linux)". Add: "The kill test runs on a disk that swaps and on one that can't."

**WRITE-17** *Today* (S2)
- "On a volume without `MNT_LOCAL` it takes the lock and raises the Notice `lock.local`" → "On a network volume
  (`m2_os::fs::is_local`: no `MNT_LOCAL` on macOS; an NFS, SMB, 9p or Ceph `statfs` type on Linux) it takes the lock and
  raises the Notice `lock.local`".

**WRITE-19** *Today* (S3)
- "on a local volume FSEvents names what changed (`crates/server/src/graph/fsevents.rs`, in-tree over CoreServices, for
  `FSEventStreamFlushSync`)" → "on a local volume the OS's file events name what changed (`m2_os::watch`: FSEvents on
  macOS, in-tree over CoreServices for `FSEventStreamFlushSync`; inotify on Linux, where an event is queued before the
  writer's call returns, so a flush is a drain)".
- "On a network volume (no `MNT_LOCAL`: its events name only this Mac's writes), when FSEvents won't start" → "On a
  network volume (its events name only this machine's writes), when events won't start (on Linux, at the inotify watch
  limit)".

**FMT-16** (S5)
- Rule: "in `~/Library/Caches/music2/`" → "in the OS's cache folder (`m2_os::dirs::cache()`: `~/Library/Caches/music2/`
  on macOS; `$XDG_CACHE_HOME/music2/` or `~/.cache/music2/` on Linux)".
- *Today*: "keep the cache in `~/Library/Caches/music2/hashes.json`" → "keep the cache in `hashes.json` there".

**FMT-20** *Today* (S2)
- "with the machine's hardware UUID as what tells another host's journal" → "with `m2_os::host::id()` as what tells
  another host's journal (the hardware UUID on macOS, `/etc/machine-id` on Linux)".

**RENDER-30** (S5)
- "cached in `~/Library/Caches/music2/`" → "cached in the OS's cache folder ([FMT-16](#assets-and-hashes))".
- The layout block's last line: "`~/Library/Caches/music2/     outside the project: decoded and warped renders, the hash
  cache`" → "`<cache>/music2/              outside the project, in the OS's cache folder (FMT-16): decoded and warped
  renders, the hash cache`".

**HOST-2** (S2)
- Rule: "In the app that is the AppKit main thread, reached through the main dispatch queue. In the CLI, the headless
  server and tests, the main thread runs a `CFRunLoop` and serves as the plugin main thread." → "On macOS, where plugins
  post work to the process's main run loop, that is the process's main thread: in the app AppKit's, reached through the
  main dispatch queue; in the CLI, the headless server and the gates, the main thread runs a `CFRunLoop`. On Linux and
  Windows, where plugins get main-thread time only from the host (CLAP's fd and timer extensions, a message pump), it is
  a thread that runs `m2_os::main_loop`: the process's main thread in the CLI, the headless server and the gates, and a
  thread of its own in the app, whose main thread the toolkit owns."
- *Today*: "the app calls `m2_plugins::use_process_main_thread()` first thing in `main`
  (`apps/music2-desktop/src/main.rs`): the AppKit main thread, through the main dispatch queue." → "the app calls
  `m2_plugins::use_app_main_thread()` first thing in `main` (`apps/music2-desktop/src/main.rs`): AppKit's main thread
  through the main dispatch queue on macOS, a loop thread of its own on Linux (`tests/app_main_thread.rs`)."
- *Today*: "`is_main_thread` asks the executor (`pthread_main_np`, or the dedicated thread's `pthread_self`)" →
  "`is_main_thread` asks the executor (`m2_os::main_loop::Loop::is_current`: `pthread_main_np` for macOS's main queue,
  else the loop thread's id)".

**HOST-3** (S2)
- "and its own AppKit main thread for windows" → "and its own main loop for windows (AppKit on macOS; on Linux an X11
  loop that serves CLAP's fd and timer extensions; a message pump on Windows)".

**Acceptance gates** (S9)
- "A step is done when its gates pass in CI or, for timings, in a recorded run on the reference projects." → "A step is
  done when its gates pass in `ci/check.sh` on Linux and on the reference Mac ([PLAT-4](#platforms)) or, for timings,
  in a recorded run on the reference projects."
- After "Timings are taken on the **reference Mac**, the oldest supported Apple-silicon Mac (an M1), at 48 kHz." add:
  "Other machines run the same gates and record their timings with the machine named; only the reference Mac's are
  judged ([PLAT-7](#platforms))."
- "These gates keep running in CI once their step lands:" → "These gates keep running in `ci/check.sh` once their step
  lands:".

### 6.2 A new section: Platforms (PLAT-1 to PLAT-8)

Placed before "Deferred, with triggers". S1 adds the whole section. Each rule then gets a *Today* line that names the
slice that builds it ("not yet: S3"), updated as the slices land.

```markdown
## Platforms

**PLAT-1.** Every call that differs by OS (a foreign function, `libc`, `std::os::unix` or `std::os::windows`, a `cfg`
on the OS) MUST live in `m2-os` (`core/os`), one folder per OS. Outside it, only the desktop shell's Tauri code, tests
that need a Unix fixture, and the gates' fault injection may name the OS.
- *Why:* one place per OS to read, port and check; steps 8a and 9 add each OS piece once.

**PLAT-2.** `m2-os` MUST do what the OS does or answer `Unsupported`, naming what. It never falls back by itself: the
fallback and its rule stay with the caller (the commit's aside-then-rename, the watcher's walks, the silent player).
- *Why:* a fallback hidden in the OS layer would change a rule's meaning on one OS without the rule saying so.

**PLAT-3.** macOS 11 or later on Apple silicon is the reference platform: judged timings, the look, native menus.
Linux x86_64 with glibc 2.39 or later is the development platform. Windows x64 is designed for: `m2-os` compiles for
it, checked from Linux, and it is built in its own step.

**PLAT-4.** `ci/check.sh` is the full check, the same script on Linux and macOS: the build, every Rust test, clippy with
warnings denied (on Linux also for macOS and Windows), the desktop app's tests, the web build, every browser suite,
the 8 offline renders against `e2e/renders.sha256`, and the std rebuild. `main` passes it on Linux at every commit, and
on the reference Mac at each step's end. Until a Mac run passes, the step's commit message and `HANDOFF.md` say
"macOS unverified since <commit>".

**PLAT-5.** A pure offline render MUST give the same bits on every supported OS and CPU. Host code on the render path,
and song code in QuickJS, MUST NOT use the OS's math library for a transcendental function: they use the `libm` crate,
the code inside `std.wasm`.
- *Why:* RENDER-2's bits must not depend on whose computer exported the song. Each OS's math library rounds some
  results differently in the last bit (glibc and `libm`: up to 10% of inputs, measured).

**PLAT-6.** `std/kernels/std.wasm` MUST rebuild byte for byte from its sources with the pinned toolchain
(`rust-toolchain.toml`) on every OS and in any folder, and hold no machine path. `std/build.sh --check` proves it.

**PLAT-7.** Judged timings are the reference Mac's. Other machines run the same gates and record their timings with the
machine named (`m2_os::process::machine()`); they are never judged against Mac records, nor Mac records against them.

**PLAT-8.** Every folder `m2-os` returns comes from `m2_os::dirs::home()`, which reads `HOME` first on every OS, so a
test or a suite isolates itself by setting `HOME` (on Linux, also clearing `XDG_*`).
```

### 6.3 Order of work

Under item 6, after its bullets, add (S1):

> **6b. The platform layer** (about 2 weeks; built on main before step 7 is finished, when the work moved to a Linux
> server). `m2-os` with one folder per OS; Linux green natively; macOS checked by `ci/check.sh` on the owner's Mac;
> Windows compile-checked from Linux. Deterministic host and song-code math (`libm`), a reproducible `std.wasm`, and a
> fallback rename that recovery can finish or undo ([PLAT-1](#platforms) to [PLAT-8](#platforms)). Steps 8a and 9 add
> their OS pieces to `m2-os`.

### 6.4 Deferred, with triggers (new rows, S9)

| Deferred | Trigger |
|---|---|
| Linux packages (`.deb`, AppImage) and the bundled-resource path on Linux | A Linux release, or a Linux user. |
| The Windows build: every `core/os/src/windows/` body, NTFS's swap fallback and retries, the lock past EOF, a message pump, packaging | The owner asks for Windows. |
| Stored spelling on case-folding Linux volumes (vfat, casefold ext4) | The Windows step (NTFS needs the same), or a report from such a volume. |
| CLAP's `timer-support` and `posix-fd-support` on Linux | Plugin windows (step 9), or a Linux plugin that needs them headless. |

---

## 7. Verification

### 7.1 On this server

One script, `ci/check.sh`, run where the owner can watch:

```sh
cd <repo>
$SCRATCH/tw check 'CARGO_TARGET_DIR=$SCRATCH/target-check M2_CHECK_OUT=$SCRATCH/check-now ci/check.sh'
# about 15 minutes (estimate from the ground notes' parts); then $SCRATCH/tw-wait … as tw prints
```

- No double quotes in the `tw` command (the helper breaks on them, tests.md).
- Each slice ends with a full run. The summary is in `$M2_CHECK_OUT/summary.txt`.
- `ci/check.sh` adds the macOS and Windows standard libraries to rustup the first time (264 MB, user level, no sudo).
- It replaces `$SCRATCH/check.sh`. Delete `$SCRATCH/target-check` after each workflow (disk note).

### 7.2 The compile checks of macOS and Windows code, from Linux

**What they catch:** every compile error in code that only macOS or Windows builds (a wrong name, a wrong type, a
missing function in one OS folder, a `libc` constant that doesn't exist there), and every clippy lint in it (dead code,
unused variables). Measured: both deliberate mistakes in §2.6 were caught.

**What they cannot catch:**

| Missed | Why | Who catches it |
|---|---|---|
| link errors: a misspelled framework symbol, a wrong `extern "C"` signature | `check` never links | the Mac run |
| behaviour: FSEvents, `F_FULLFSYNC`, APFS renames, the AppKit run loop, Mach calls | nothing runs | the Mac run's tests |
| C code (wasmtime's helpers, QuickJS, objc2's exception helper) | stubbed out | the Mac run's build |
| build scripts that behave differently on a real Mac (Tauri's bundle, Info.plist) | they run here, for Linux | the Mac run's build |
| the Windows desktop app | its build script needs `llvm-rc` | the Windows step |
| bits, timings, the browser on a Mac | | the Mac run (bits), the gates on the Mac (timings) |

The stubs (`ci/stub-cc`, `ci/stub-ar`) are only ever set for the two cross targets, never for the host.

One side effect, seen while probing: the desktop app's build script (tauri-build) writes
`apps/music2-desktop/gen/schemas/<os>-schema.json` for the OS it targets. A Windows check with the desktop app included
wrote `windows-schema.json` (identical to the others; removed). The Windows check leaves the desktop app out, the macOS
file is committed and identical, and the script's "repo unchanged" step catches any other write.

```sh
#!/bin/sh
# ci/stub-cc: a C compiler for compile checks only. It writes an empty object wherever it is
# asked to and succeeds. `cargo check` and `cargo clippy` never link, so C that a build script
# compiles for macOS or Windows is never used. Never use it to build.
out=
prev=
for a in "$@"; do
  [ "$prev" = -o ] && out=$a
  case $a in -Fo*) out=${a#-Fo} ;; /Fo*) out=${a#/Fo} ;; esac
  prev=$a
done
[ -n "$out" ] && : > "$out"
exit 0
```

```sh
#!/bin/sh
# ci/stub-ar: an archiver for compile checks only: an empty archive at the first .a or .lib it is given.
for a in "$@"; do
  case $a in
    -out:*) f=${a#-out:} ;;
    /OUT:*) f=${a#/OUT:} ;;
    *.a|*.lib) f=$a ;;
    *) continue ;;
  esac
  printf '!<arch>\n' > "$f"
  exit 0
done
exit 0
```

### 7.3 The Mac check: `ci/check.sh` (full draft)

The same file runs on Linux. On the Mac it is one command: `ci/check.sh`. It is written for bash 3.2, the bash macOS
ships.

```bash
#!/usr/bin/env bash
# ci/check.sh: the full check of music2, the same script on macOS and Linux (PLAT-4).
#
# In short: run it in the repo with no arguments. It changes nothing in the repo. It ends with a
# short summary to paste back. The summary and every log are in ~/m2-check/<commit>/.
#
#   ci/check.sh             everything
#   ci/check.sh --no-e2e    everything except the browser suites
#
# Needs: rustup (rust-toolchain.toml installs Rust 1.99.0 the first time) and Node 24. The first
# run downloads chrome-headless-shell 133.0.6943.141 into ~/.cache/puppeteer. On Linux it also adds
# the macOS and Windows standard libraries to rustup, for the compile checks (no sudo).
#
# Plugins: live3b and the plugin tests use Surge XT and Airwindows Consolidated from $CLAP_PATH,
# else from ~/Library/Audio/Plug-Ins/CLAP (macOS) or $SCRATCH/plugins/clap (Linux) when both are
# there. Without them live3b is PART, and the check still passes. Never put them in
# /Library/Audio/Plug-Ins/CLAP or /usr/lib/clap: music2 always looks there, and fx-fixes needs
# them missing.
#
# On a Mac, run it in Terminal on the Mac itself, not over ssh: the AppKit test needs your
# login session.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) && cd "$ROOT" || exit 2
E2E=1
[ "${1:-}" = "--no-e2e" ] && E2E=0
. "$HOME/.cargo/env" 2> /dev/null || true

SHA=$(git rev-parse --short=12 HEAD)
OUT=${M2_CHECK_OUT:-$HOME/m2-check/$SHA}
SCR=$(mktemp -d "${TMPDIR:-/tmp}/m2-check.XXXXXX")
trap 'rm -rf "$SCR"' EXIT
rm -rf "$OUT" && mkdir -p "$OUT/e2e" "$SCR/home"
RESULTS=$OUT/results.tsv
FAILED=$OUT/failed.txt
: > "$RESULTS"
: > "$FAILED"
START=$(date +%s)
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}

# Nothing music2 starts may see your own folders: tests and suites get a scratch HOME, and on
# Linux no XDG folder of your session (PLAT-8). Cargo and rustup keep using yours.
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
export M2_AUDIO=none CARGO_TERM_COLOR=never
unset XDG_CACHE_HOME XDG_CONFIG_HOME XDG_DATA_HOME

case $(uname -s) in
  Darwin)
    OS=macos
    CHROME_DIR=mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64
    PLUGINS=$HOME/Library/Audio/Plug-Ins/CLAP
    MACHINE="$(sysctl -n hw.model), macOS $(sw_vers -productVersion), $(uname -m)" ;;
  Linux)
    OS=linux
    CHROME_DIR=linux-133.0.6943.141/chrome-headless-shell-linux64
    PLUGINS=$SCRATCH/plugins/clap
    MACHINE="$(. /etc/os-release && echo "$PRETTY_NAME"), $(uname -m), $(nproc) threads" ;;
  *)
    echo "ci/check.sh runs on macOS and Linux"
    exit 2 ;;
esac

CLAP=${CLAP_PATH:-}
if [ -z "$CLAP" ] && [ -e "$PLUGINS/Surge XT.clap" ] && [ -e "$PLUGINS/Airwindows Consolidated.clap" ]; then
  CLAP=$PLUGINS
fi
if [ -n "$CLAP" ]; then export CLAP_PATH=$CLAP; else unset CLAP_PATH; fi

# record <check> <PASS|FAIL|PART|SKIP> <note>
record() {
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$RESULTS"
  printf '%-24s %-4s  %s\n' "$1" "$2" "$3"
}
# quiet <log> <command...>: run it, with its output in the log
quiet() {
  log=$OUT/$1
  shift
  "$@" > "$log" 2>&1
}
sha256() { if command -v sha256sum > /dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }

echo "music2 check of $SHA on $MACHINE; logs in $OUT"
git status --porcelain > "$SCR/before"

# Plugins in a folder music2 always searches make fx-fixes fail: say so first.
for d in /Library/Audio/Plug-Ins/CLAP /usr/lib/clap; do
  if ls "$d" 2> /dev/null | grep -qE '^(Surge XT|Airwindows Consolidated)\.clap$'; then
    record "plugin folders" FAIL "$d has Surge XT or Airwindows: move them to a folder of your own"
  fi
done

# 1. The toolchain rust-toolchain.toml names.
if quiet toolchain.log rustc -V && grep -q ' 1\.99\.0 ' "$OUT/toolchain.log"; then
  record toolchain PASS "$(cut -d' ' -f1-2 "$OUT/toolchain.log")"
else
  record toolchain FAIL "not Rust 1.99.0: see toolchain.log"
fi

# 2. The web UI.
if quiet web.log sh -c 'cd web && npm ci --no-audit --no-fund && npm run build'; then
  record "web build" PASS ""
else
  record "web build" FAIL "see web.log"
fi

# 3. Everything: the desktop app, m2, the headless server and every test binary.
if quiet build.log cargo build --locked --workspace --all-targets; then
  record build PASS ""
else
  record build FAIL "see build.log"
fi

# 4. Every Rust test, the desktop app's included, with a scratch HOME.
quiet test.log env HOME="$SCR/home" cargo test --locked --workspace --no-fail-fast
st=$?
counts=$(grep -E '^test result:' "$OUT/test.log" | awk '{p += $4; f += $6} END {printf "%d passed, %d failed", p, f}')
grep -E '^    [A-Za-z0-9_:]+$' "$OUT/test.log" | sed 's/^ */test /' >> "$FAILED"
grep -o 'to rerun pass `[^`]*`' "$OUT/test.log" | sed 's/^to rerun pass /test binary /' >> "$FAILED"
if [ $st -eq 0 ]; then record "rust tests" PASS "$counts"; else record "rust tests" FAIL "$counts"; fi

# 5. Lints, the whole workspace.
if quiet clippy.log cargo clippy --locked --workspace --all-targets -- -D warnings; then
  record clippy PASS ""
else
  record clippy FAIL "$(grep -cE '^(warning|error)' "$OUT/clippy.log") findings: see clippy.log"
fi

# 6. std: its kernels' own tests and lints, and std.wasm rebuilt byte for byte (PLAT-6).
if quiet kernels.log sh -c 'cd std/kernels-src && cargo test --locked --release && cargo clippy --locked --all-targets -- -D warnings'; then
  record "std kernels" PASS ""
else
  record "std kernels" FAIL "see kernels.log"
fi
if quiet std-wasm.log std/build.sh --check; then
  record "std.wasm rebuild" PASS "byte for byte"
else
  record "std.wasm rebuild" FAIL "see std-wasm.log"
fi

# 7. Linux only: compile-check the macOS and Windows code (no SDK needed; section 7.2 of the
# platform design). C that build scripts compile is stubbed: check and clippy never link.
cross() {
  t=$1
  shift
  u=$(printf %s "$t" | tr - _)
  env "CC_$u=$ROOT/ci/stub-cc" "CXX_$u=$ROOT/ci/stub-cc" "AR_$u=$ROOT/ci/stub-ar" \
    cargo clippy --locked --target "$t" "$@" -- -D warnings
}
if [ $OS = linux ]; then
  quiet targets.log rustup target add aarch64-apple-darwin x86_64-pc-windows-msvc
  if quiet cross-macos.log cross aarch64-apple-darwin --workspace --all-targets; then
    record "macOS compile check" PASS ""
  else
    record "macOS compile check" FAIL "see cross-macos.log"
  fi
  # Without the desktop app: its Windows build script needs llvm-rc (Tauri's resources).
  if quiet cross-windows.log cross x86_64-pc-windows-msvc --workspace --exclude music2-desktop --all-targets; then
    record "Windows compile check" PASS ""
  else
    record "Windows compile check" FAIL "see cross-windows.log"
  fi
fi

# 8. Every browser suite, with the debug headless server and m2 from step 3.
if [ $E2E = 1 ]; then
  CHROME=${E2E_CHROME:-$HOME/.cache/puppeteer/chrome-headless-shell/$CHROME_DIR/chrome-headless-shell}
  quiet e2e-npm.log sh -c 'cd e2e && npm ci --no-audit --no-fund'
  if [ ! -x "$CHROME" ]; then
    (cd e2e && node_modules/.bin/browsers install chrome-headless-shell@133.0.6943.141 --path "$HOME/.cache/puppeteer") \
      >> "$OUT/e2e-npm.log" 2>&1
  fi
  E2E_CHROME=$CHROME M2_HEADLESS=$TARGET/debug/examples/headless \
    node e2e/run.mjs all --out "$SCR/e2e" > "$OUT/e2e.log" 2>&1
  st=$?
  cp "$SCR"/e2e/*.log "$OUT/e2e/" 2> /dev/null
  line=$(grep -E 'suites ok' "$OUT/e2e.log" | tail -1 | cut -d';' -f1)
  grep -E '^FAIL ' "$OUT/e2e.log" | awk '{print "suite " $2}' >> "$FAILED"
  case $st in
    0) record "browser suites" PASS "$line" ;;
    3) if [ -z "$CLAP" ]; then
         record "browser suites" PART "$line (live3b: no plugins)"
       else
         record "browser suites" FAIL "$line"
       fi ;;
    *) record "browser suites" FAIL "$line" ;;
  esac
else
  record "browser suites" SKIP "--no-e2e"
fi

# 9. The 8 offline renders with the debug m2 (it renders the release m2's bits), against the
# fingerprints recorded for every OS (PLAT-5).
if quiet renders.log env M2="$TARGET/debug/m2" e2e/baseline.sh --tree . "$SCR/renders" &&
  (cd "$SCR/renders" && sha256 -c "$ROOT/e2e/renders.sha256") > "$OUT/renders-cmp.log" 2>&1; then
  record "offline renders" PASS "8 of 8 match e2e/renders.sha256"
else
  n=$(grep -c ': OK$' "$OUT/renders-cmp.log" 2> /dev/null)
  record "offline renders" FAIL "${n:-0} of 8 match: see renders-cmp.log"
  grep -v ': OK$' "$OUT/renders-cmp.log" 2> /dev/null | sed 's/^/render /' >> "$FAILED"
fi

# 10. The check changed nothing in the repo.
git status --porcelain > "$SCR/after"
if cmp -s "$SCR/before" "$SCR/after"; then
  record "repo unchanged" PASS ""
else
  record "repo unchanged" FAIL "$(diff "$SCR/before" "$SCR/after" | grep -c '^>') files changed"
fi

# The summary to paste back.
fails=$(awk -F'\t' '$2 == "FAIL"' "$RESULTS" | wc -l | tr -d ' ')
total=$(wc -l < "$RESULTS" | tr -d ' ')
mins=$(( ($(date +%s) - START + 59) / 60 ))
{
  echo
  if [ "$fails" -eq 0 ]; then echo "music2 check: PASS"; else echo "music2 check: FAIL ($fails of $total checks)"; fi
  echo "commit   $SHA ($(git rev-parse --abbrev-ref HEAD))$([ -s "$SCR/before" ] && echo ', with local changes')"
  echo "machine  $MACHINE"
  echo "tools    $(rustc -V | cut -d' ' -f1-2), node $(node -v), $mins min"
  echo
  awk -F'\t' '{printf "%-24s %-4s  %s\n", $1, $2, $3}' "$RESULTS"
  if [ -s "$FAILED" ]; then
    echo
    echo "what failed:"
    head -25 "$FAILED" | sed 's/^/  /'
  fi
  echo
  echo "logs     $OUT"
} | tee "$OUT/summary.txt"
[ "$fails" -eq 0 ]
```

What the owner pastes back looks like this (made-up numbers):

```
music2 check: PASS
commit   1a2b3c4d5e6f (main)
machine  MacBookPro18,1, macOS 26.6.2, arm64
tools    rustc 1.99.0, node v24.19.0, 31 min

toolchain                PASS  rustc 1.99.0
web build                PASS
build                    PASS
rust tests               PASS  742 passed, 0 failed
clippy                   PASS
std kernels              PASS
std.wasm rebuild         PASS  byte for byte
browser suites           PASS  29/29 suites ok
offline renders          PASS  8 of 8 match e2e/renders.sha256
repo unchanged           PASS

logs     /Users/…/m2-check/1a2b3c4d5e6f
```

Also needed in the repo for the script (S1 and S4): `rust-toolchain.toml`, `ci/stub-cc`, `ci/stub-ar`,
`e2e/renders.sha256`, the eighth case in `e2e/baseline.sh`, `std/build.sh --check` (S8), `e2e/chrome-mac.sh`, and
`run.mjs` choosing the Chrome path per OS (S4).

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.99.0"
components = ["clippy", "rustfmt"]
targets = ["wasm32-unknown-unknown"]
profile = "minimal"
```

The cross targets stay out of this file, so the Mac doesn't download Windows' library; the Linux script adds them.

```sh
#!/bin/sh
# e2e/chrome-mac.sh: chrome-headless-shell, telling pages it runs on a Mac (OS-74, OS-75). Monaco, the
# code editor, picks its key map from the user agent, so this is the one place that decides it.
# The suites test the UI's Mac keys and labels on every OS until the key table adds a PC pass.
exec "${E2E_CHROME_BIN:?run.mjs sets it}" \
  --user-agent="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/133.0.0.0 Safari/537.36" \
  "$@"
```

`run.mjs` keeps `E2E_CHROME` as the real browser, passes it to the suites as `E2E_CHROME_BIN`, and names
`chrome-mac.sh` as their browser. `chrome.mjs` does the same when a suite runs alone.

### 7.4 When to ask the owner for a Mac run

As few as possible: **one per step, at its end.** More only when Mac-only code changes.

| When | Why | Ask? |
|---|---|---|
| **The end of this step** (after S9) | the moved Mac code, the new render hashes, the Chrome wrapper on a Mac | **yes** |
| Before merging a branch that changes Mac-only code: **taste** (`menu.rs`, the title bar) and **step 7's end** (`store.rs`'s Mach calls and QoS) | only a Mac runs that code | yes, once each, at that merge |
| A change of recorded render hashes or of `std.wasm` (a std version bump, the sounds merge) | the same bits on both OSes is the point | fold into the next run the step needs anyway |
| A toolchain bump, or a new version of tauri, cpal, wasmtime or rquickjs | their Mac code changes under us | fold into the step's end run |
| A Linux-only change, or portable code | the Linux run and the macOS compile check cover it | no |

Timing gates (G3, G11, G12) stay separate `m2-gates` runs on the Mac, when a step's gates include timings (step 7's
end, 8a), as before.

### 7.5 What "the Mac is green" means, and how "not yet" is written

- **Green:** `ci/check.sh` printed `music2 check: PASS` on the owner's Mac at that commit. live3b may be PART only if
  the plugins aren't installed there. The same `e2e/renders.sha256` passed on both OSes.
- **Until then**, the step's last commit message carries this line:

  `macOS: unverified since <commit> (compile-checked from Linux; the owner's ci/check.sh run is pending).`

  and `HANDOFF.md`'s first table gains a row:

  `| Platforms | Linux green at <commit> (ci/check.sh). macOS unverified since <commit>. Windows: compile-checked only. |`

- After the owner's run: "macOS green at <commit> (ci/check.sh on MacBookPro18,1, macOS 26.6.2, <date>)". If it fails,
  the fix is made on Linux, compile-checked for macOS, and the owner runs it once more.

### 7.6 Timings

- Linux timings are informational (PLAT-7). The gates binary prints `process::machine()` on every run.
- This server is a best case for durability (no volatile drive cache) and has no sound card and no realtime rights. Its
  G11 numbers are recorded, never used as a Linux reference.

---

## 8. Build slices

### 8.1 What "green at a slice's end" means

The baseline has 11 failing tests, failing clippy and 7 failing suites. No first slice can fix them all. So at each
slice's end: `ci/check.sh` passes on Linux **except the baseline failures a later slice owns, listed by name**; nothing
that passed before fails; and the 8 renders match `e2e/renders.sha256` (S1 records it). The list only shrinks. Two checks
join `ci/check.sh` only in S8, when they can pass: the std rebuild (`--check` doesn't exist before, and today's
`std.wasm` is stale by 10 bytes) and the kernels' own tests and lints (never run with `-D warnings` so far).

| After | Linux failures left | Renders |
|---|---|---|
| baseline | 11 Rust tests, clippy (Linux and Windows), 7 suites (+ live3b PART) | resample differs from the Mac |
| S1 | 10 Rust tests, clippy (Linux and Windows), 7 suites | the one deliberate change; then fixed |
| S2 | 5 watch tests, clippy lints 4 to 6 (Linux and Windows), 7 suites | byte-identical |
| S3 | 7 suites | byte-identical |
| **S4** | **none: Linux fully green** | byte-identical |
| S5 to S9 | none | byte-identical |

### 8.2 The slices

| Slice | What | Size *(estimate)* | Fixes | Mac run |
|---|---|---|---|---|
| **S1. The check, and the same math everywhere** | `rust-toolchain.toml`; `ci/check.sh`, `ci/stub-cc`, `ci/stub-ar`; `baseline.sh`'s eighth case; `libm` for the 8 calls and `resample.rs`; QuickJS `Math` and `**` through `libm`; the grep test; `renders.json` and the new `e2e/renders.sha256` recorded (the one deliberate change, in its own commit); the Platforms section (§6.2); the order-of-work entry. | ~150 lines + the script | test 10 | no |
| **S2. `m2-os`: files, lock, ids, threads** | the crate; `fs`, `host`, `thread`, `main_loop`, `process::crash_quietly` and `aborted`; macOS code moved as it is; Linux bodies (`renameat2`, `gettid`, `statfs`, `FICLONE`, `prctl`); Windows bodies `Unsupported`; `stage.rs`, `lock.rs`, `watch.rs`, `hash.rs`, `tx/mod.rs`, `render.rs` (create-only name), `project.rs` (`make_writable`), `main_thread.rs`, `plugin_host.rs` (`leave_now`), the gates' CPU time and barrier label call it; the AppKit test made portable (`app_main_thread.rs`); OS-68's `cfg(unix)` in tests and the gates' fault injection; `linux-schema.json` committed; the PLANE-5, WRITE-15, 17, FMT-20, HOST-2, 3 edits | ~700 moved, ~300 new | tests 6, 7, 8 (ext4), 9, 11; lints 1 to 3, 7 to 15; the 15 Windows errors; kill test 473 s → 104 s | no (the macOS compile check covers the moves) |
| **S3. File events** | `watch::Events`: FSEvents moved, inotify new, Windows `Unsupported`; `watcher.rs` without `cfg`; G11's label; WRITE-19's edit | ~250 moved, ~250 new | tests 1 to 5; lints 4 to 6 | no |
| **S4. Browser suites** | Chrome path per OS; `e2e/chrome-mac.sh`; `native.ts`'s `isMac`; `notes.mjs:97`; song names free in any capitals (Q1); `run.mjs` clears `XDG_*`; `CLAP_PATH` in the check | ~60 | the 7 suites; live3b ok with plugins | no |
| **S5. Folders** | `dirs`: home, songs (XDG), cache (XDG), `~/.music2`, CLAP folders, `bundled`; `resources.rs`, `project.rs`, `core/plugins/src/lib.rs`, the desktop's session file use it; FMT-16 and RENDER-30 edits | ~150 | OS-32, 36 to 39 | no |
| **S6. Recovering the fallback rename** | `journal.rs`: a missing target whose `old-<k>` holds its before bytes counts as at its before hash, so Finish (rename the staged file in) and Discard (rename `old-<k>` back) are offered; the kill, hook and stage tests run on a disk that can't swap too (`Recorder { plain: true }`); WRITE-15 and 16 edits | ~120 | tests.md surprise 1 (10 kill points with no fix) | no |
| **S7. The audio device** | cpal errors sorted by kind (xrun counted, device change → new player, invalidated → stalled; the callback only bumps counters); no device or a device that won't open → silent player and one message | ~80 | survey-later bug 1; OS-61 | no |
| **S8. `std.wasm` rebuilds the same** | `core/kernel` → `std/kernels-src/kernel`; `std/build.sh` (own target folder, `--check`, no machine path); one rebuild; renders, `renders.json` and `bit_identity` byte-identical; the std rebuild and the kernels' tests and lints join `ci/check.sh` | ~40 + the rebuild | OS-63, 64; the stale 10 bytes | no |
| **S9. Gates and the rule** | `process::memory`, `machine`, `thread::interactive` (for step 7's G7f); records name their machine; the PLAT-1 grep test; the gates' and Deferred edits; `HANDOFF.md`'s platform row | ~200 | OS-82 to 84 | **yes: the step's one Mac run, after S9** |

Each slice is one or a few commits on `main`, each green as defined in 8.1, pushed once the slice passes.

### 8.3 Keeping the three merges easy

Rules this design follows (from `branches.md`'s trial merges):
- **Stay off taste's files:** `menu.rs`, the window builder in the desktop's `main.rs`, `GraphCanvas.tsx`, the fonts,
  the README's Mac lines, and the suites one by one (the Chrome wrapper and `native.ts` fix 5 of main's failing suites,
  and 7 of taste's, in one place).
- **Don't touch `apps/music2-desktop/src/main.rs` at all.** The app's install call stays `plugin_host::install()`; only
  `plugin_host.rs`'s body changes. So step 7's allocator lines merge cleanly (the trial merge's conflict there came from
  a marker on the install lines).
- **Keep `main_thread::token()` and `is_process_main()`** as one-line wrappers, so step 7's `probe.rs` (6 calls)
  compiles unchanged.
- **Put the 8 `libm` edits exactly on the 8 lines;** they land cleanly in step 7's moved `audio.rs` (checked).
- **Make `std.wasm` reproducible (S8) before sounds merges,** so its merge rebuilds once, with the new build.

Conflicts to expect, and how to resolve them:

| Merge | Conflict | Resolution |
|---|---|---|
| wip/step7 | `Cargo.lock` (neighbouring lines: `m2-os`, `libm`) | take main's lock, then `cargo build` adds step 7's two path edges; never `cargo update` |
| wip/step7 | `docs/architecture.md` PLANE-5 (one long line each side) | keep both edits |
| wip/step7 | `crates/gates/src/store.rs` (no text conflict) | the step 7 finisher swaps its Mach `task_info` and QoS blocks for `m2_os::process::memory()` and `thread::interactive()`; its records name their machine |
| wip/step7 | `core/play/src/lib.rs`: S7 edits the error callback (line ~553); step 7's hunks are at 457 and 681 | expected clean; if not, keep both |
| sounds | `std/kernels/std.wasm` (binary) | never take one side: rebuild with `std/build.sh` from the merged sources, `--check`, commit once; add `examples/sounds` to `PURE`, `baseline.sh` and `e2e/renders.sha256` (recorded on Linux; the next Mac run confirms) |
| taste | `docs/architecture.md` (WRITE-18's *Today*, one line) | keep both |
| taste | `e2e/notes.mjs` (taste changed the drag line just above line 97) | keep taste's drag line and main's wait |
| taste, after | 3 new suites | nothing for Linux: they pass here (taste 35 of 35, gestures 30 of 30, s8-tracks 75 of 75 alone; branches.md §3.3); then the s6-fixes race fix, and s8-tracks' `layout.json` timing re-checked on inotify |

---

## 9. How 8a, 8b, 9 and Windows plug in

No design here; only where each piece goes.

| Step | Piece | Where in `m2-os` | Per OS |
|---|---|---|---|
| 8a | realtime engine threads: the group, the device thread's first callback, workers, background renders | new `rt.rs` + `mac/rt.rs`, `linux/rt.rs`, `windows/rt.rs` (design-8a's `core/play/src/rt.rs` becomes `mac/rt.rs`) | macOS: time-constraint policy, workgroup. Linux: `SCHED_FIFO` directly or through rtkit from a promoter thread, `RLIMIT_RTTIME` set by us at 200 ms with SIGXCPU ignored, `sched_yield` in spins (survey-later §2.4). Windows: MMCSS. |
| 8a | sleeping and waking (`Sem`) | `rt::Wake` | Mach semaphore, futex, `WaitOnAddress` |
| 8a | thread count, core topology | `rt::cores` | Linux: sysfs, affinity and cgroup quota. Here each core has its own L2; "one cluster" maps to one CCD (shared L3: 96 MB or 32 MB, facts §5) |
| 8a | the machine in gate records | `process::machine()` (exists from S9) gains topology and VA bits | |
| 8a | xruns, device changes | `m2-play` (S7 already sorts cpal's errors) | portable |
| 8a | FP mode, denormals | `m2-runtime`'s `fpenv` (per CPU), std kernels | not OS |
| 8a | a real device on this server | needs `snd-aloop` or a PipeWire null sink: a system change, the owner's OK first | |
| 8b | the key table, menus per OS, "Show in …" labels | not in `m2-os`: taste's `ITEMS`, a web helper, the desktop shell (allowed `cfg`); the suites' PC-keys pass reads the same table | |
| 9 | the plugin host process: spawn, shared memory, a cross-process wake, crash watch, dies with us | new `child.rs` + per-OS files | `posix_spawn` + shm + Mach semaphore by port; `posix_spawn` + memfd + shared futex + pidfd + `PR_SET_PDEATHSIG`; `CreateProcess` + section + Event + Job |
| 9 | plugin windows; CLAP's fd and timer extensions | `main_loop.rs` gains `add_fd`, `add_timer` and `WINDOW_API` | CFRunLoop sources; `epoll` + timerfd + eventfd (+ the X11 fd); a message pump |
| 9 | VST3 and AU folders | `dirs::vst3()`; AU macOS only | |
| 9 | disk streaming | `dirs::cache()` (exists), `fs::will_need`, `thread::io_priority` | `F_RDADVISE` / `setiopolicy_np`; `posix_fadvise` / `ioprio_set`; `PrefetchVirtualMemory` |
| 9 | recording permission, staying awake | `process::input_access()`, `process::keep_awake()` | TCC + Info.plist; granted; privacy setting |
| 9 | MIDI timestamps | a small `time` module: `midi_instant` | host ticks; receipt time; QPC |
| Windows | everything above, and every `windows/*.rs` body | the same files | listed in §3's last column and in `survey-code.md` |

---

## 10. Owner questions

Only real decisions. Each has a recommended answer.

**Q1. Song names that differ only in capitals.**
On a Mac, "exports 2" and "Exports 2" are the same folder. On Linux they are two folders. Today a new song on Linux
can be named "exports 2" next to "Exports 2". If that project is ever copied to a Mac, the two folders collide.
- **Recommended: yes, treat names as taken in any capitals, on every computer.** New songs then get the same name
  everywhere ("exports 3"), and projects stay safe to copy between computers. No browser suite needs changing.

**Q2. The browser suites on Linux test the Mac's keys for now.**
The suites tell Chrome "this is a Mac", so they press ⌘ and expect ⌘ labels on Linux too. That fixes 4 suites in one
place and keeps taste's merge easy. The cost: until the key table (8b), nothing tests the Ctrl keys a Linux user
presses.
- **Recommended: yes for now, and add a "PC keys" pass of the keyboard-heavy suites when the key table lands** (right
  after taste merges).

**Q3. The server's name.**
Messages on the server say "Ubuntu-2404-noble-amd64-base" (the server's real host name), for example "this project
is open for writing in music2 on Ubuntu-2404-noble-amd64-base". It changes nothing else: music2 tells computers
apart by their machine id, not their name.
- **Recommended: rename it once, if you like:** `sudo hostnamectl set-hostname <name>`. It is a system setting,
  so only you can do it. Skipping it is fine.

---

## Appendix A. Measured for this design

All on the Linux server, at `f126d03`, with a scratch rustup and target folder (deleted afterwards). Logs in
`$SCRATCH/logs/dsh-*`.

**A1. Compile checks for macOS and Windows from Linux.** An isolated `RUSTUP_HOME` with rustc 1.99.0 and the
`aarch64-apple-darwin` and `x86_64-pc-windows-msvc` standard libraries.
- Per crate, no stubs (`dsh-chk-*.log`, `dsh-cross-percrate.log`): macOS ok for `m2-ir`, `m2-kernel`, `m2-song`,
  `m2-plugins`; failing on C build scripts (wasmtime `helpers.c`, `rquickjs-sys`, `objc2-exception-helper`) for the
  rest. Windows ok up to `m2-play`; `rquickjs-sys` stops the rest.
- With `ci/stub-cc` and `ci/stub-ar` (`dsh-stub2-*.log`): macOS `cargo check --workspace --all-targets` exit 0 in 11 s,
  then `cargo clippy` exit 0 in 4 s; on the real tree `cargo clippy --workspace --all-targets -- -D warnings` exit 0
  (`dsh-cross-macos-clippy.log`). Windows: the desktop's build script panics on `llvm-rc`; 15 errors in 7 files
  (`dsh-cross-windows-clippy.log`).
- Deliberate mistakes in a scratch copy (`dsh-err-*.log`): Linux's own check passed both; the macOS check failed on
  `unused variable: unused_on_mac` (`main_thread.rs:208`) and `cannot find value RENAME_SWAPP in crate libc`
  (`stage.rs:161`).
- Sizes: the macOS standard library 146 MB, Windows (MSVC) 118 MB.

**A2. The exact `libm` patch** (`dsh-libm-exp2.diff`, `dsh-libm-*.log`, `dsh-renders-libm-SHA256SUMS`). The 8 calls of
§5.1 with `libm::exp2` for pitch, in a scratch copy:
- `fixture_renders_change_only_with_stds_version` prints `examples/resample` = `b3daee50…` and the other three fixtures
  unchanged;
- the release `m2` renders the 8 cases: `examples-resample.wav` `39f94764…`, `examples-resample-from4-for6.wav`
  `d752521b…`, the other 6 equal to the Linux baseline, which equals the Mac's.

**A3. QuickJS's math** (read in source): `rquickjs-sys-0.14.0/quickjs/quickjs.c` lines 15025-15037 (`js_math_pow`
calls C `pow`; `**` uses it, lines 15475 and 15547) and 49019-49050 (`Math.sin` and the rest call C). `grep` finds no
`Math.` in `examples/`, `templates/`, `std/src` or the SDK.

**A4. Other facts used here.** `apps/music2-desktop/gen/schemas/linux-schema.json` is byte-identical to the committed
`macOS-schema.json`. The lead's run of live3b with `CLAP_PATH=$SCRATCH/plugins/clap`: 51 passed, 0 failed
(`$SCRATCH/logs/live3b-plugins-1014-210631-833394.log`); `ldd` reports no missing library for either plugin
(`get-plugins-1014-210617-832021.log`). The branches touch none of the layer's files except `core/play/src/lib.rs`,
`core/runtime/src/audio.rs`, `crates/editor/src/render.rs`, `crates/gates/src/server.rs` (step 7) and `e2e/run.mjs`'s
suite list (taste) (`git diff main...origin/<branch> --stat`).

## Appendix B. Not verified

| Claim | Why not | Settled by |
|---|---|---|
| The Mac renders `b3daee50…`, `39f94764…` and `d752521b…` after S1 | no Mac here | the owner's run at the step's end |
| `ci/check.sh` works under macOS's bash 3.2 and takes about 30 minutes | no Mac here | the same run |
| The AppKit test needs a login session (not ssh) | not tried | the same run |
| The owner's plugins are in `~/Library/Audio/Plug-Ins/CLAP` | not visible from here | the script's first check |
| inotify keeps test 5's event-mode case green | inotify is not built yet | S3 |
| Linux stored spelling on case-folding volumes | no such volume here (needs root) | the Windows step or a report |
| A laptop drive's durable-commit cost on Linux | this server's drives have no volatile cache | a Linux laptop, when one exists |
| How much slower subnormals are on Zen 4 | not measured | 8a |
| `std.wasm`'s bytes don't depend on the target folder's path | measured only for the source folder (branches.md) | S8's `--check` in two folders |
