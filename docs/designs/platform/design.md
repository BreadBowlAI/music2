# The platform layer: design

Revision 2. Written 2026-10-14 on the Linux server, against `main` at `f126d03` (revision 1: label
`synthesize`; revision 2: label `revise`; a completeness critic's small fixes: label `critic`, listed in §15).

Revision 1 merged two drafts, `design-small.md` (the least code) and `design-shaped.md` (shaped for later steps), section by
section: the smallest shape that does not make step 8a, step 9 or the Windows step move code again (§10 lists every
choice). Five attacks then read revision 1 (`attack-{durability,watcher,macos,simplicity,verification}.md`). Revision 2
takes in what held up and says why the rest was left out (§9, the attack response).

Inputs: `docs/architecture.md` (the contract, normative), `survey-code.md` (OS-01 to OS-85), `survey-later.md`, and the
ground notes `ground/{tests,e2e,facts,branches,ci}.md`, which were measured here and outrank the surveys. From `ci.md`
only two things are used: what the Mac check must run, and the Linux plugin facts. There is no GitHub Actions workflow
in this step: the owner checks the Mac by hand.

Every fact was checked on this machine or in source, unless it is marked *(unverified)*. §11 lists what was measured;
§12 lists what is still unverified.

---

## 1. In short

**What changes**

| What | In plain words |
|---|---|
| Linux becomes fully green on this server | Every Rust test, every lint, the desktop app's build and tests, all 29 browser runs (the plugin ones included), and the same exported audio as the Mac (your Mac run confirms that part). |
| One new part, `m2-os` | Almost everything music2 does differently on a Mac and on Linux moves into it. It has one folder per OS. The rest of music2 stops naming an OS. The app's own window and menu code stays in the app (§2.4). |
| Later steps write each OS piece once | Step 8a (all CPU cores), step 9 (plugin windows, recording) and a Windows build add their own files in `m2-os`. Nothing moves again. |
| The same sound on every computer | A few math calls on the export path switch to one shared math library (`libm`). Song code gets the same math. |
| Saves can't lose a file in a crash, on any disk | Today a crash in the middle of a save on a USB stick or a network drive can lose a file. After this step each file is always either the old or the new version. Bounced and exported audio is safely on disk before a song uses it. |
| One check command, `ci/check.sh` | The same command checks everything, on the server and on your Mac. On your Mac it also runs the speed tests (the timing gates). It ends with a short summary to paste back. |
| The Mac code is checked from Linux too | Every check on the server also type-checks and lints the Mac-only code. A Mac is still needed to build it fully and to run it. |

**What you will notice**

| What | Details |
|---|---|
| In the app on your Mac: nothing. | The look, the menus and the sound stay as they are. |
| One example song exports other bytes, once. | `examples/resample` changes in the very last bit of some samples. You can't hear it. After that, every computer exports the same bits. |
| New song names ignore capitals on every computer. | "exports 2" counts as taken when "Exports 2" exists, as on the Mac today (question Q1). |
| Lock messages on the server name it oddly. | They say "Ubuntu-2404-noble-amd64-base", the server's real host name (question Q3). |

**What you do once, at the end of this step** (about 45 minutes of the Mac's time, mostly waiting *(estimate)*)

| # | Do this on your Mac | Note |
|---|---|---|
| 1 | Plug the Mac in, keep the lid open, and quit other apps. | The speed tests need a quiet Mac. The check keeps the Mac awake by itself. |
| 2 | Open Terminal on the Mac itself, not over ssh. Type `cd <repo>`, then `git switch main`, then `git pull`. | One test needs your login session. |
| 3 | Type `ci/check.sh` and press Return. | The first run may download Rust 1.99.0 and the test browser by itself. Each check prints a line when it starts. |
| 4 | If it stops at once and says your plugins are in the wrong folder, move Surge XT and Airwindows from `/Library/Audio/Plug-Ins/CLAP` to `~/Library/Audio/Plug-Ins/CLAP`, and run it again. | music2's tests need them out of the shared folder. Other music apps still find them there: the CLAP standard names both folders. |
| 5 | If one line says `running` for more than 30 minutes, press Ctrl-C once. | It stops that check and still prints the summary. |
| 6 | Copy the last lines it prints, from `music2 check:` to the `logs` line, and paste them to me. | |

**When I will ask you for a Mac run** (as few as possible)

| When | Why |
|---|---|
| At the end of this step | The Mac code moved, one render changed on purpose, `std.wasm` was rebuilt, and two speed tests changed. |
| At the end of step 7, before it lands on `main` | Step 7's speed tests are judged on your Mac, and its merge moves Mac-only memory code into `m2-os`. |
| Before `taste` lands on `main` | It rewrites the Mac menus and the window's title bar. |
| At the end of each later step (8a, 8b, 9) | Its speed tests are judged on your Mac. |

Never for work that changes only Linux code or code that is the same on every OS. Until you run it, the Mac is written
down as "macOS: unverified since <commit>".

**Your decisions** (§14): three questions, each with a recommended answer.

---

## 2. Where the layer lives, and its shape

### 2.1 One crate, one module per concern, one folder per OS

```
core/os/                       crate m2-os: what music2 does differently on each OS (PLAT-2). Depends on libc only.
  src/lib.rs                   the module list; picks the OS folder; unsupported()
  src/fs.rs                    durable writes, renames, clones, stamps, file ids, volumes, the lock  OS-01…19
  src/host.rs                  machine id, host name, local clock                                  OS-12…14
  src/watch.rs                 file-system events for the watcher                                  OS-20…22
  src/thread.rs                thread token, "is this the main thread", thread CPU time             OS-24, 25, 80
  src/main_loop.rs             the loop the plugin main thread runs                                 OS-23, 26, 27
  src/dirs.rs                  home, songs, cache, CLAP folders                                     OS-32, 36, 37
  src/unix.rs                  code the same on macOS and Linux (fsync, stat ids, C paths, pthread_self)
  src/mac/mod.rs               macOS: fs, host, thread, dirs, and its error lists
  src/mac/watch.rs             FSEvents
  src/mac/main_loop.rs         the main dispatch queue and the CFRunLoop pump
  src/linux/mod.rs             Linux: fs, host, thread, dirs, and its error lists
  src/linux/watch.rs           inotify
  src/linux/main_loop.rs       the job queue, and the app's loop thread
  tests/one_place.rs           fails when OS code appears outside core/os (PLAT-2)
```

- **Portable files hold the API.** `fs.rs`, `watch.rs` and the others hold the public items, their documentation, and
  any logic every OS shares. They contain no `cfg`.
- **Each OS folder implements the same private functions** with the same signatures. The small concerns share one
  `mod.rs` per OS; the two large ones (`watch`, `main_loop`) have their own file. A function that is the same on macOS
  and Linux is written once in `unix.rs` and re-exported by both folders. `lib.rs` picks one folder:

  ```rust
  #[cfg_attr(target_os = "macos", path = "mac/mod.rs")]
  #[cfg_attr(target_os = "linux", path = "linux/mod.rs")]
  mod os;
  #[cfg(not(any(target_os = "macos", target_os = "linux")))]
  compile_error!("music2 builds on macOS and Linux; Windows comes in its own step (PLAT-1)");
  ```

- **Later files go beside them.** 8a adds `mac/rt.rs` and `linux/rt.rs`, step 9 `child.rs`. A nested `mod rt;` inside
  `linux/mod.rs` is found at `linux/rt.rs` (measured by `attack-simplicity` with a scratch crate), so nothing moves.
- **"What is different on Linux?"** is answered by reading `core/os/src/linux/`. A function missing from one folder
  fails that OS's build. Linux builds `linux/`, and the macOS lint that every Linux check runs builds `mac/` (§2.7).
- **Mechanism, not policy (PLAT-3).** `m2-os` does what the OS does, with the strongest call the volume accepts. When
  the OS or the volume can't, it returns `ErrorKind::Unsupported`, naming what. It never emulates: the commit's
  compare-then-rename for a missing swap (`stage.rs`), the watcher's walks and hint limits (`graph/events.rs`,
  `watcher.rs`), and the copy when a clone can't be made stay with the caller.
- **Who depends on it:** `m2-song` (stamps), `m2-session` (commit, lock, recovery), `m2-editor` (export names, file
  ids), `m2-plugins` (main loop, CLAP folders), `m2-server` (events, folders), `m2-gates` (CPU time, the barrier), and
  the desktop app's test. `m2-os` depends on none of them.

### 2.2 The public API built in this step

One line of documentation each. The per-OS behaviour is in §3.

```rust
// lib.rs
/// ErrorKind::Unsupported, naming what this OS or this volume can't do (PLAT-3).
pub fn unsupported(what: &'static str) -> std::io::Error;
```

```rust
// fs.rs: what a durable commit needs from the disk (WRITE-15, PLANE-5), and file identity.
pub enum Rename { Swap, Excl }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId { /* (volume, file index): (st_dev, st_ino) on Unix */ }
/// What identifies a file's version without reading it (FMT-16, WRITE-19). One type for the
/// watcher and the hash cache, which have a copy each today.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Stamp { pub size: u64, pub mtime: i128, pub ctime: i128, pub inode: u64 }
impl Stamp {
    /// From metadata the caller already has. Unix: the change time in ns and the inode.
    /// Windows: the creation time and 0, all that std's stable Metadata gives (today's non-Unix arm).
    pub fn of(m: &Metadata) -> Stamp;
    /// The stamp of what is at `p`, links not followed.
    pub fn at(p: &Path) -> Option<Stamp>;
}

/// Push a file's bytes, or a folder's entries, towards the drive: fsync. Durable alone on Linux;
/// on macOS durable once a barrier follows.
pub fn push(f: &File) -> io::Result<()>;
/// Make everything pushed so far durable: the commit's one barrier. macOS: F_FULLFSYNC, or fsync
/// when the volume refuses it (never File::sync_all, which is F_FULLFSYNC there); any other error,
/// such as EIO, is returned as it is. Linux: fsync.
pub fn barrier(f: &File) -> io::Result<()>;
/// Make a folder's new entries durable once a barrier follows: fsync of the folder opened
/// read-only. Ok where the OS keeps new entries durable by itself (Windows' write-through renames).
/// (The review's D2: also Ok where it can't be done, a volume with no folder fsync (EINVAL) or a
/// folder that can't be opened (permission), so every writer skips those alike; EIO is returned.)
pub fn sync_dir(dir: &Path) -> io::Result<()>;
/// Swap two existing names, or rename only where nothing is. Unsupported when the volume can't
/// (each OS's own error list, below); AlreadyExists for Excl onto a taken name.
pub fn rename(from: &Path, to: &Path, how: Rename) -> io::Result<()>;
/// A clone of `from` at `to`, pushed; otherwise the OS's error as it is (Unsupported where the OS
/// has no clone call). The caller copies on any error but AlreadyExists, as stage.rs does today.
pub fn clone_file(from: &Path, to: &Path) -> io::Result<()>;
/// What a path names, whatever it is called (links followed); and the same for an open file.
pub fn file_id(p: &Path) -> io::Result<FileId>;
pub fn file_id_of(f: &File) -> io::Result<FileId>;
/// Whether a rename from `a` to `b` stays on one volume (WRITE-15 step 3).
pub fn same_volume(a: &Path, b: &Path) -> io::Result<bool>;
/// Whether the volume holding `dir` is this machine's own, not a network or FUSE volume
/// (WRITE-17, WRITE-19).
pub fn is_local(dir: &Path) -> bool;
/// The last name of `p` as the disk stores it, when the OS can tell; None otherwise.
pub fn stored_name(p: &Path) -> Option<OsString>;
/// Take the exclusive lock without waiting: Ok(true) taken, Ok(false) held elsewhere,
/// Err(Unsupported) when the volume can't lock (WRITE-17's lock.unsupported; error list below).
pub fn try_lock(f: &File) -> io::Result<bool>;
/// Let the user change a copied file, even if what it was copied from was read-only.
pub fn make_writable(p: &Path) -> io::Result<()>;
```

**The error lists.** macOS and Linux number their errors differently: `ENOTSUP` is 45 and `EOPNOTSUPP` is 102 on macOS,
both are 95 on Linux (`libc` 0.2.190). Rust's `ErrorKind::Unsupported` covers only `ENOSYS` and `EOPNOTSUPP` (std 1.99.0,
`sys/io/error/unix.rs`), so it misses macOS's 45. Each list is therefore written per OS, as today's code has it:

| What | macOS (today's code, kept) | Linux | Never |
|---|---|---|---|
| `rename` gives `Unsupported` | `ENOTSUP`, `EINVAL` | `EINVAL`, `ENOSYS`, `EOPNOTSUPP` | testing `e.kind() == ErrorKind::Unsupported` on the OS's error |
| `try_lock` gives `Unsupported` | `ENOTSUP`, `EOPNOTSUPP`, `ENOLCK` | `EOPNOTSUPP`, `ENOLCK` | same |
| `barrier` falls back to `fsync` | `F_FULLFSYNC` refused: `ENOTSUP`, `EOPNOTSUPP`, `ENOTTY`, `EINVAL` (which one a refusing volume gives: *unverified*); any other error is returned | — | `File::sync_all` (on Apple it *is* `F_FULLFSYNC`); `fsync` after `EIO`, which could report a failed flush as done and break PLANE-5 |
| `clone_file` | `clonefile`'s error as it is | `Unsupported` (no `FICLONE` yet) | — (the caller copies) |

Each list is one small function per OS that takes an error number (`rename_unsupported(code)`,
`lock_unsupported(code)`, and on macOS `barrier_refused(code)`). One portable unit test in `core/os` feeds them
`libc::ENOTSUP`, `EOPNOTSUPP`, `EINVAL`, `ENOTTY`, `ENOLCK`, `EEXIST` and `EIO` by name. On Linux the first two are both
95; the Mac run checks 45 and 102.

```rust
// host.rs: who this machine is, for journals and messages (FMT-20, WRITE-16, WRITE-18).
/// This machine's lasting id: the hardware UUID (macOS), /etc/machine-id (Linux). None when the OS
/// has none; the lock then uses the host name (lock.rs keeps that rule).
pub fn machine_id() -> Option<String>;
pub fn name() -> String;                                   // the host name, for messages
pub fn local_time(unix_secs: u64) -> Option<(u8, u8)>;     // (hour, minute), local time
```

```rust
// watch.rs: names of changed paths, as hints (WRITE-19). Never contents, never facts.
pub enum Heard<'a> {
    /// Something at this path, or under it, changed. Absolute, spelled as the source spells it.
    Path(&'a Path),
    /// Events were lost, or the watched folder itself moved or went: only a walk tells.
    Rescan,
}
pub struct Events { /* per OS */ }          // Send + Sync
impl Events {
    /// Watch `root` (canonical) and every folder under it whose path relative to `root` `enter`
    /// allows. `heard` runs on the source's own thread, or inside `flush`, never after drop returns.
    /// Err(why) when events won't start here; the caller walks and says why.
    pub fn start(root: &Path, enter: fn(&str) -> bool, heard: Box<dyn Fn(Heard<'_>) + Send + Sync>)
        -> io::Result<Events>;
    /// Return once every event for a change made before this call has been given to `heard`.
    /// Never call it while holding a lock that `heard` takes.
    pub fn flush(&self);
}
```

```rust
// thread.rs
/// The calling thread's identity: pthread_self, a register read. Never a thread-local; allocates
/// nothing (the audio thread asks, RENDER-28).
pub fn token() -> usize;
/// Whether the calling thread is the process's main thread. No allocation, no lock.
pub fn is_process_main() -> bool;
/// This thread's CPU time (G11's watcher check).
pub fn cpu_time() -> Duration;
```

```rust
// main_loop.rs: the loop the plugin main thread runs (HOST-2). Step 9 adds fds and timers here.
pub type Job = Box<dyn FnOnce() + Send>;
pub enum Where {
    /// The process's main thread, which the program pumps with run_for (m2, the headless server,
    /// the gates). Start it on that thread.
    ProcessMain,
    /// The app, whose toolkit owns the main thread. macOS: the main dispatch queue, which AppKit
    /// drains. Linux: a loop thread of its own. Start it on the main thread, before the toolkit.
    AppMain,
    /// A loop on a new thread with this name (tests, every OS).
    Thread(&'static str),
}
/// Whether Where::AppMain is the process's main thread: true on macOS, false on Linux.
pub const APP_ON_PROCESS_MAIN: bool;
pub struct Loop { /* per OS */ }            // Send + Sync
impl Loop {
    pub fn start(at: Where) -> io::Result<Loop>;
    pub fn post(&self, job: Job);              // any thread; never blocks
    pub fn is_current(&self) -> bool;          // never allocates or locks
    pub fn run_for(&self, most: Duration);     // ProcessMain only, on that thread: run what is due
}
```

```rust
// dirs.rs: the folders music2 writes outside a project, all under $HOME when it is set (PLAT-7),
// and the system's CLAP folders, which it only reads.
pub fn home() -> Option<PathBuf>;                           // $HOME, when set and not empty
pub fn songs() -> Option<PathBuf>;                          // where New Song puts songs
pub fn cache() -> Option<PathBuf>;                          // rebuildable caches (FMT-16, RENDER-30)
pub fn clap() -> Vec<PathBuf>;                              // the OS's CLAP folders, the user's first
```

Nothing else in this step. Step 7's merge adds a `process` module (`memory`, `machine`) and `thread::interactive`
(signatures fixed in §8); 8a adds an `rt` module; step 9 grows `main_loop` and adds a `child` module.

### 2.3 How the callers read afterwards

| Caller | After |
|---|---|
| `core/session/src/tx/stage.rs` `Real` | each `Disk` method is one call into `m2_os::fs`, and `clone_file` keeps its copy on any error but `AlreadyExists`. `push` and `cpath` leave. The `Disk` trait stays: tests inject faults through it, and that is a real run-time choice. |
| `core/session/src/lock.rs` | `try_lock`, `file_id`, `same_volume`, `is_local`, `host::*` from `m2-os`. `host_id()` stays: `machine_id()`, else the host name. |
| `core/session/src/watch.rs`, `core/song/src/hash.rs` | `m2_os::fs::Stamp` replaces both copies (`watch::Stamp` re-exports it). `hash.rs` keeps its saved form, the same four numbers, built from it. `spelled()` asks `fs::stored_name`. |
| `core/session/src/tx/mod.rs`, `crates/editor/src/render.rs` | `fs::file_id`; export's create-only name is `fs::rename(Excl)`. |
| `crates/server/src/project.rs`, `resources.rs`, `core/plugins/src/lib.rs` | `dirs::songs`, `dirs::cache`, `dirs::clap`, `dirs::home`; `fs::make_writable`. `resources.rs`'s `bundled()` stays as it is (§2.4). |
| `core/plugins/src/main_thread.rs` | `MainThread` wraps an `m2_os::main_loop::Loop`, and keeps its policy (shut-down, `run` and its waits, a panic never unwinding into the loop). No `cfg` left. |
| `crates/server/src/graph/fsevents.rs` | becomes `graph/events.rs`, the same on every OS: the hints, `MAX_HINTS`, the prefix and `walked()` filter, the event count and the wake. Only the CoreServices calls moved out. |
| `crates/server/src/graph/watcher.rs` | `source()` asks `fs::is_local` and `Events::start`; no `cfg` left. |
| `kill_tests::child`, `hook_tests::hook_child` | each calls `prctl(PR_SET_DUMPABLE, 0)` itself under `cfg(target_os = "linux")` before it aborts on purpose (a test fixture, which PLAT-2 allows). |

The plugin executor, as callers see it:

```rust
pub fn use_app_main_thread() -> Result<()>;                // was use_process_main_thread; Where::AppMain
pub fn run_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T;   // Where::ProcessMain
pub fn test_main_thread();                                 // Where::Thread("m2-plugin-main")
pub(crate) fn token() -> usize;                            // kept: step 7's probe.rs calls it 4 times
pub(crate) fn is_process_main() -> bool;                   // kept: probe.rs calls it 2 times
```

The app still calls `plugin_host::install()` in `main`; only that function's body changes. So this step never edits
`apps/music2-desktop/src/main.rs`, which both `wip/step7` and `taste` change.

### 2.4 What stays outside `m2-os`

| Stays where it is | Why |
|---|---|
| The desktop shell's OS code (`apps/music2-desktop/src`): the title bar, the menus, "reveal in Finder", `leave_now`, the session file's mode 0600 | The shell is one place already, Tauri hides most differences, and `taste` rewrites the menus and the window builder. Editing them on `main` would make conflicts (`branches.md` §3.2). |
| `bundled()` in `crates/server/src/resources.rs` | Plain path code with no `cfg` (`X.app/Contents/MacOS/prog` → `X.app/Contents/Resources/<name>`). Its three tests build a fake `.app` and run on Linux today, the only automated check of how the Mac app finds its files. Linux packaging adds a second candidate in the same function. |
| Tests that need a Unix fixture (symlinks, permission bits, signals, the kill tests' `prctl`) | They test portable code with an OS-only setup. The Windows step rewrites or skips them (OS-68). |
| The gates' fault injection (G11's "the disk refuses a write", permission bits) | A measuring tool's fault, not product code. |
| `core/runtime/src/fpenv.rs` (8a) | FPCR and MXCSR depend on the CPU, not the OS. |
| cpal, Tauri, libloading, rquickjs, std's `Instant`, `sleep` and thread parking | They already give one interface (survey-later §5.1). |
| (moved in) `midir` for 8b.10's MIDI keyboard | Lives in `m2-os` since `next` (its plan's Q1): one file, `core/os/src/midi.rs`, no OS folder, since `midir` hides CoreMIDI and ALSA; the server calls `m2_os::midi::listen`, behind its `midi` feature. A MIDI input that won't open (no sequencer, no permission, as on this server) means "no MIDI devices", never an error. |

### 2.5 Why this shape

**Free functions in modules, not one trait with associated types** (survey-later §5 sketches `trait Os` with
`type This = mac::Mac`). Only one OS is compiled into a build, so a trait has one implementation. It buys no
polymorphism. It costs `<This as Os>::rename(…)` at every call site and associated types in every signature. Its one
real gain, "the compiler says when an OS lacks a function", comes from building both folders on every Linux check
(§2.7). Where code really picks an implementation at run time, a trait already exists: the session's `Disk`.

**One crate, not `cfg` modules in each crate.**

| | `cfg` in each crate (today) | One crate (chosen) |
|---|---|---|
| Where OS code lives | 28 files in 7 crates today, tests included (grep at `f126d03`); 8a and 9 would add more in `m2-play`, `m2-runtime`, `m2-plugins` | one folder per OS |
| A Windows port touches | every crate | one crate, plus the shell |
| `unsafe` calls to the OS | spread out | in one crate |
| Shared pieces (thread token, stamps, later `Wake`, `machine()`) | duplicated (`Stamp::of` exists twice today) | one place, below every crate |
| Unused items | a Linux build lints a macOS-only item as dead code (today's 15 lints) | a library's `pub` items are never dead code |

**Few files per OS.** One `mod.rs` per OS holds the small concerns (each under about 25 lines *(estimate)*), and the
two large ones get their own file. Later files sit beside them, so nothing moves. The layout is a design choice, not a
contract rule (PLAT-2 names none).

**No Windows folder yet.** A Windows folder now would hold `Unsupported` stubs that the Windows step throws away, so
each Windows piece would be written twice. The rule in §2.8 keeps every OS call inside `m2-os`, so the Windows step
only adds `src/windows/` and one `cfg_attr` line. Every signature in §2.2 can be given a Windows body without changing its callers:
`Stamp::of` uses the `Metadata` fields that are stable on Windows (creation time, no file index; the others are
unstable in the pinned toolchain's std), and `sync_dir` may answer `Ok` where the OS needs no folder sync. What a Windows compile check finds today is recorded for that step (§8).

**Code that is the same on macOS and Linux moves too** (into `unix.rs`). Left at its call sites under `cfg(unix)`,
the Windows step would have to find and move it. Moved now, that step only adds files. It is small: two `Stamp::of`
copies, three file-id helpers, `push`, the lock's checks, the clock and the host name.

**In-tree inotify, not the `notify` crate.** The watcher needs a flush: "every event before now has been delivered".
FSEvents has `FSEventStreamFlushSync`, which is why `fsevents.rs` is in-tree today. On Linux an event is queued before
the writer's call returns (0 misses in 2.8 million checks, `facts.md` §3), so a flush is a read that never waits.
`notify` hides the file descriptor, so it can't offer that.

**One inotify instance per process, shared by every watcher in it.** The limit of 128 instances is per user and shared
with editors. A test run can hold one watcher per test thread, and several agents' runs share this server. One
instance per watcher could hit the limit and turn tests into flaky walks.

### 2.6 The plugin main thread on Linux

On macOS the plugin main thread must be the process's main thread: many plugins post timers and work to the main run
loop (HOST-2's *Why*). Linux has no process-wide run loop for plugins to post to. Linux plugins get main-thread time
only from the host, through CLAP's `timer-support` and `posix-fd-support`.

So on Linux:
- in `m2`, the headless server and the gates, the plugin main thread is the process's main thread, running today's
  job queue under `run_main`;
- in the app, GTK owns the main thread, so the plugin main thread is **a loop thread of its own**. Plugin hosting never
  touches GTK, and a slow plugin load can't freeze the app's window;
- step 9 gives this one loop CLAP's fd and timer extensions (§8). Windows will be the same as Linux, with a message
  pump.

This closes OS-26 now: the Linux app can host plugins. Measured here: Surge XT 1.3.4 and Airwindows Consolidated pass
live3b's plugin part (51 of 51 checks) in the headless server with no timer or fd extension
(`$SCRATCH/logs/live3b-plugins-1014-210631-833394.log`), and with the `gettid` fix the with-plugins Rust tests pass
(`main_thread.rs` with Surge XT, the gates' harness 6 of 6, `m2-play`'s live tests; `av-plugtests*.log`). The app's
test (`app_main_thread.rs`, §3.5 OS-30) runs the app's own hook on Linux without a window.

### 2.7 Checking the macOS code from Linux (measured by both drafts)

| What | Result |
|---|---|
| `cargo check --target aarch64-apple-darwin`, plain | Works only for crates with no C (`m2-ir`, `m2-kernel`, `m2-song`, `m2-plugins`). Others stop in C build scripts (wasmtime's `helpers.c`, QuickJS, `objc2-exception-helper`): there is no macOS SDK here. |
| With two stub scripts, `ci/stub-cc` and `ci/stub-ar` (§7.2), for that target only | **The whole workspace, desktop app included, passes `cargo clippy --target aarch64-apple-darwin --workspace --all-targets -- -D warnings` at `f126d03`.** `check` and `clippy` never link, so the stubbed C is never used. |
| Does it see macOS-only code? | Yes. Three mistakes planted in scratch copies (a type error in `fsevents.rs`, an unused variable in a macOS-only arm, a misspelled `libc::RENAME_SWAPP`) all passed Linux's own check and all failed the macOS one. It also caught a lint in shared test code that Linux clippy never reached, because Linux clippy stops at `m2-plugins` today (`attack-verification`). |
| Cost | The macOS standard library: about 31 MB to download, 146 MB on disk, added once with `rustup target add` (no sudo). About 1 to 2.5 minutes the first time, about 5 seconds after. |

**What it catches:** type errors and missing items in `mac/`, wrong `cfg`s, every rustc and clippy lint in macOS-only
Rust (dead code included).

**What it cannot catch:**

| Missed | Why | Caught by |
|---|---|---|
| link errors (a misspelled framework symbol, a wrong `extern "C"` signature) | `check` never links | the Mac run's build |
| behaviour: FSEvents, `F_FULLFSYNC`, `renamex_np`, the run loop, AppKit, Mach calls, macOS's error numbers | nothing runs | the Mac run's tests (the error-list test among them) |
| C and Objective-C (wasmtime's helpers, QuickJS, objc2's helper) | stubbed out | the Mac run's build |
| build scripts that behave differently on a Mac (Tauri's bundle) | they run here for Linux | the Mac run's build |
| render bits, the browser on a Mac, timings | nothing runs | the Mac run, its gates included |

### 2.8 The rule, and its test

PLAT-2 (§6.2): outside `m2-os`, code other than tests names no OS. `core/os/tests/one_place.rs` checks it:
- It reads every `.rs` file under `core/`, `crates/` and `apps/m2/`, skipping `tests/` folders, files named `tests.rs`
  or `*_tests.rs`, and items marked `#[cfg(test)]` (matched by braces).
- It fails on `target_os`, `cfg(unix)`, `cfg(not(unix))`, `cfg(windows)`, `libc::`, `std::os::`, an `extern "C" {`
  block, and `var_os("HOME")` or `var("HOME")`.
- It skips `core/os/` and `apps/music2-desktop/src/` (the shell), and allows one named function in
  `crates/gates/src/server.rs` (G11's fault injection).
- It starts in slice S5 with an allow-list of today's files. Each slice removes its own lines; the list is empty after
  S8.

---

## 3. Every OS piece (OS-01 to OS-85)

| Mark | Meaning |
|---|---|
| **Built** | New or changed behaviour, on macOS and Linux, in the slice named. |
| **Moved** | Today's code moved into `m2-os` as it is; Linux gets its body there. |
| **No change** | Right as it is. |
| **Designed** | The shape is fixed here; built at the time named. |
| **Deferred** | Not now, with the reason. |

Totals: 24 built, 20 moved, 32 no change, 4 designed, 5 deferred. The Windows column is the plan in one line. Windows is
built in its own step.

### 3.1 Durable writes and renames (`core/session/src/tx/stage.rs`, `crates/editor/src/render.rs`)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-01 | `push()`: `fsync` per file | Moved (S6): `fs::push` (`unix.rs`) | `fsync` also flushes the drive's cache, so each push is durable alone. Measured: 0.03 ms for 4 KB in place, 0.11 ms for a new file; `fdatasync` costs the same, so `fsync` stays. | `FlushFileBuffers`, a full flush each |
| OS-02 | `Real::barrier` (`F_FULLFSYNC`) | Built (S6): `fs::barrier`. macOS `F_FULLFSYNC`, and `fsync` when the volume refuses it: today's fallback calls `sync_all`, which on Apple is `F_FULLFSYNC` again, so every save fails on such a volume. Linux `fsync`, as today. | A whole commit-like sequence: 0.56 ms p50 idle, 1.37–1.48 ms p95 while another program writes 1 GB (`facts.md` §2). This disk is power-loss protected, so these are best case; a laptop drive is slower *(unverified)*. Never `syncfs`: it waited up to 890 ms for other programs. tmpfs makes nothing durable (tests only). | `FlushFileBuffers` on the journal |
| OS-03 | `Real::rename` `Swap` / `Excl` | Built (S6): `fs::rename` (macOS `renamex_np`, moved; Linux `renameat2` with `RENAME_EXCHANGE` / `RENAME_NOREPLACE`, new). Built (S7): the commit's fallback for a volume without them renames over the target in one step (below). | Both work on ext4 and tmpfs (measured). Linux's `Unsupported` list is `EINVAL`, `ENOSYS`, `EOPNOTSUPP`; macOS keeps its own (§2.2). NFS, 9p, exFAT, and Swap on SMB take the fallback *(survey; unverified here)*. Closes the gap in which a saved file is missing for up to 0.19 ms (live3a crashed on it once in 11 runs, `e2e.md`), on ext4 by the swap and elsewhere by S7. | Excl: `MoveFileExW` without replace. Swap: `Unsupported` (NTFS has no exchange), so the fallback, `MoveFileExW` with `REPLACE_EXISTING`; retry briefly on sharing violations. |
| OS-04 | `Real::clone_file` | Moved (S6): `fs::clone_file` (macOS `clonefile`; Linux `Unsupported`). The copy on any error but `AlreadyExists` stays in `stage.rs`, as today. | ext4 can't clone (`FICLONE` gives `EOPNOTSUPP`, measured): a 50 MB plugin blob is copied once per commit that replaces it. `FICLONE` for btrfs and XFS: designed, deferred until such a volume can test it (a body change in `linux/mod.rs` only). | block clone on ReFS and Dev Drive, else `Unsupported` |
| OS-05 | `sync_dir` | Moved (S6): `fs::sync_dir` | `fsync` of a folder opened read-only works (measured; never `O_PATH`, which gives `EBADF`). Some FUSE volumes may refuse it *(unverified)*. | `Ok`: NTFS keeps a rename made with `MOVEFILE_WRITE_THROUGH` durable *(unverified)*, so the commit needs no change there |
| OS-06 | `cpath()` | Moved (S6) into `unix.rs` | — | wide strings |
| OS-07 | `name_new`, export's create-only name (a hard link today) | Built (S6): `fs::rename(Excl)` on every OS; "look, then rename" only on `Unsupported`; the hard link goes. Built (S7): the part file is pushed before it gets its name, and its folder after (below). | `link()` gives `EPERM` on vfat and exFAT, so today's fallback never runs there and an export to a FAT stick fails *(survey; unverified here)*. On the Mac the result is the same file at the same moment. | `MoveFileExW` without replace |

**Saving without an atomic swap (S7).** Today a volume without an exchange (an exFAT stick, an SMB share, NFS; NTFS
always) replaces a file in three steps: move the target aside to `.music2/tx/<id>/old-<k>`, compare it, rename the new
file in. Killed between the first and the last step, the file is missing, and `ground/tests.md` found that recovery
then offers no fix (10 kill points in 4 of the 5 kill-test cases). Revision 1 planned to count the missing file as "at
its before hash"; `attack-durability` showed that can't work: Finish is refused as a conflict every time, Discard
deletes the file for good, and a missing `music2.toml` makes the folder "not a project" before recovery can run
(`session.rs:233-250` checks the manifest before it takes the lock). So the middle state goes instead:

| | Today | After S7 |
|---|---|---|
| Forward | rename target → `old-<k>`; compare `old-<k>` with the base; rename staged → target | read the target and compare it with the base; rename staged → target, one plain rename over it |
| In-process rollback | rename target → staged; rename `old-<k>` → target (the file is missing in between) | check the target holds the after bytes; create `old-<k>` from the before bytes the commit holds in memory (`Before.bytes`, pushed); rename it over the target |
| Killed anywhere | the target can be missing; no fix is offered | every target holds its before or its after bytes; today's Finish and Discard work unchanged |

- A plain rename over an existing file replaces it in one step on every POSIX volume, and `MoveFileExW` with
  `REPLACE_EXISTING` does the same on NTFS.
- The race with an outside writer stays the same size: today one that writes between the move aside and the rename in
  is overwritten; after S7 one that writes between the compare and the rename is. WRITE-19 already says
  compare-and-swap narrows this race but can't close it.
- Recovery needs no new state, and WRITE-16's rule needs no new words. The kill test runs every case twice, on a disk
  that swaps (`Recorder::default()`) and on one that can't (`Recorder { plain: true }`), plus a new case whose Tx
  writes `music2.toml` and `song.ts`.

**Files made outside a Tx (S7).** WRITE-1 lets music2 create a new asset outside a Tx. Today nothing syncs it:
`write_new_wav` (`core/runtime/src/audio.rs:73-89`), the bounce's provenance file (`apps/m2/src/main.rs`), an export's
part file (`render.rs` `write_as`), and a new song's copied files (`project.rs` `create_at`). Then `m2 bounce --place`
commits a Tx that pins the WAV's hash, and that Tx is synced. Measured here: a 40 MB file written and linked the way
`name_new` does stayed without disk blocks for 35.2 s after a Tx-like sync (`$SCRATCH/logs/ad-delalloc-*.log`). A
power cut in that window leaves a durable lane pointing at an asset with no bytes. So each of the four places pushes
the file (`fs::push`) before it gets its name, and syncs its folder (`fs::sync_dir`) before anything names it. Cost:
one `fsync` of the file, tens of ms for 40 MB here, next to a render that takes seconds. On macOS the placing Tx's
barrier makes the pushed bytes durable.

**The tidy at open (S7).** When a writer takes the lock, the scan removes a completed journal after a barrier on that
journal file (`journal.rs:291-302`). If the commit was killed after its renames but before it pushed its folders, the
renames exist only in memory. On Linux that barrier is an `fsync` of an unchanged file, which forced 0 journal commits
in 300 rounds (`$SCRATCH/logs/ad-jbd2.log`); `fsync` of the folder forced 300. ext4 is safe anyway, since it commits
its one journal in order. A file system that logs per file, such as btrfs, might make the removal durable before the
renames *(unverified: no btrfs here)*. So the scan first pushes the completed journal's target folders, as the commit's
step 8 does, then runs its barrier, then removes the journal. A folder `fsync` with nothing new costs about 0 ms here,
and the scan runs once per lock.

### 3.2 The project lock and machine identity (`core/session/src/lock.rs`)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-08 | the lock (`File::try_lock`, `flock`); also the export part file and its sweeper | Moved (S6): `fs::try_lock` | Same as macOS on local volumes (measured: per open file, freed at exit and at `SIGKILL`; a second open in one process gets `WouldBlock`). On NFS and SMB the lock's meaning differs; both are network volumes, so the session raises `lock.local` there. | `LockFileEx` one byte past the end, so others can still read who holds it |
| OS-09 | "this volume can't lock" codes | Moved (S6) into `fs::try_lock`, with each OS's own list (§2.2) | `EOPNOTSUPP` (95, the same number as `ENOTSUP` here), `ENOLCK` (37, NFS without lockd). On macOS `ENOTSUP` (45) and `EOPNOTSUPP` (102) differ, and both stay in its list. | `ERROR_NOT_SUPPORTED`, `ERROR_INVALID_FUNCTION` |
| OS-10 | `is_at`: is the held file still `.music2/lock`? | Moved (S6): `fs::file_id_of`, `fs::file_id` | `(st_dev, st_ino)` | volume serial and 128-bit file id |
| OS-11 | `prepare`: `.music2/tx/` on the project's volume | Moved (S6): `fs::same_volume` | `st_dev`. btrfs subvolumes differ and are refused, which is right; a bind mount gives `EXDEV` at the first rename, rolled back. | volume serial |
| OS-12 | `host_id` | Moved (S6): `host::machine_id` (macOS `gethostuuid`; Linux `/etc/machine-id`, then `/var/lib/dbus/machine-id`); `lock.rs` keeps the host-name fallback | `/etc/machine-id` is present here; `product_uuid` is root-only. | `MachineGuid` in the registry |
| OS-13 | `host_name` | Moved (S6): `host::name` | `gethostname` gives `Ubuntu-2404-noble-amd64-base` here (Q3) | `GetComputerNameExW` |
| OS-14 | `clock` ("since 14:02") | Moved (S6): `host::local_time` | `localtime_r`; the server runs on Europe/Berlin | `localtime_s` |
| OS-15 | `local_volume` | Built (S6): `fs::is_local` (macOS `MNT_LOCAL`, moved; Linux `statfs` `f_type`) | Network: NFS `0x6969`, SMB `0x517B`, CIFS `0xFF534D42`, SMB2 `0xFE534D42`, 9p `0x01021997`, Ceph `0x00C36400`, AFS `0x5346414F`, Coda `0x73757245`; and FUSE `0x65735546`, since sshfs and ntfs-3g look the same there, so FUSE gets walks and `lock.local` (the safe side). Measured local: ext4 `0xEF53`, tmpfs `0x01021994`. A type the list doesn't know counts as local (VirtualBox's `vboxsf`, for one); its 5 s walks still catch outside edits. | `GetDriveTypeW` = remote, or a `\\server\share` path |

### 3.3 File identity, stamps and spelling

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-16 | `Stamp::of` (ctime, inode) in `watch.rs` and `hash.rs` | Moved (S6): `fs::Stamp`, one type instead of two | real ctime in ns, and the inode | the creation time and 0, from std's stable `Metadata` (today's non-Unix arm). A write that keeps size and mtime goes unseen there; the racy-mtime rule covers recent ones. A handle-based change time is that step's call. |
| OS-17 | Tx `spelled()`: the real name by `(dev, ino)` | Moved (S6): calls `fs::file_id` | same; ext4 names are byte-exact | `GetFinalPathNameByHandleW` |
| OS-18 | the editor's `file_id` | Moved (S6): `fs::file_id` | same | the file index |
| OS-19 | the watcher's `spelled()` (`getattrlist`) | Moved (S6): `fs::stored_name` (macOS `getattrlist`; Linux `None`: the name as asked) | Right on plain ext4 (case-sensitive). On vfat, exFAT or a casefold folder a case-only rename could be taken in twice. Designed: list the parent on a volume that folds case. Deferred: no such volume here, and making one needs root. | `GetFinalPathNameByHandleW` |

### 3.4 File-system events (WRITE-19)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-20 | the event source (`fsevents.rs`) | Built (S8): `watch::Events`. macOS: today's FSEvents stream, moved; its flags become `Heard` (dropped events, root changed, mount, unmount → `Rescan`; the root's own entry → `Rescan` only with "must scan subfolders"). Linux: inotify, new (~300 lines *(estimate)*). | Below. | `ReadDirectoryChangesW`, recursive, overlapped; flush by a sentinel write |
| OS-21 | `source()` in `watcher.rs` | Built (S8): no OS `cfg`; it asks `fs::is_local` and `Events::start`, and walks with the reason when either says no | inotify sees only this machine's writes, so a network volume walks, as on macOS | same |
| OS-22 | `mod fsevents` (macOS only) | Built (S8): `graph/fsevents.rs` becomes `graph/events.rs` on every OS (hints, `MAX_HINTS` 4,096, the prefix and `walked()` filter, take, put back, the event count, the wake). A walk that a `Rescan` asked for counts no missed events (below). | — | same |

**The inotify source in detail** (`core/os/src/linux/watch.rs`):
- **One instance per process**, shared by every watcher in it, with one reader thread at normal priority. The thread
  blocks in `poll` on the inotify fd, then reads and dispatches under the instance's mutex, then **sleeps 10 ms with
  the mutex free** before it polls again. That is FSEvents' latency (`fsevents.rs`: 10 ms with `NO_DEFER`): the first
  event after a quiet spell goes out at once, and the kernel merges a writer's events meanwhile. Measured with a probe
  of this design (`$SCRATCH/logs/aw-probes.txt`): 1 GB written in 4 KB writes woke a reader that reads at once
  161,874 to 182,735 times (0.50 s of its CPU); with the pause, 76 times (0.001 s). 8,000 new folders (105,000 events)
  still went through with no overflow.
- **Watches.** At start, the source walks `root` and adds a watch on each folder `enter` allows (never `.git`,
  `.music2`, `node_modules`), **watching each folder before listing it**, so a folder made in between is never missed.
  Mask: `IN_MODIFY | IN_ATTRIB | IN_CLOSE_WRITE | IN_CREATE | IN_DELETE | IN_MOVED_FROM | IN_MOVED_TO | IN_DELETE_SELF |
  IN_MOVE_SELF | IN_ONLYDIR | IN_DONT_FOLLOW | IN_EXCL_UNLINK`. `IN_MODIFY` stays, so a file written while open is
  named and not counted as missed.
- **A folder two watchers share has one watch descriptor.** Each descriptor keeps a count and a list of watchers; every
  watcher on it hears its events, and the watch is removed only when no watcher holds it.
- **New folders.** On a folder's `IN_CREATE` or `IN_MOVED_TO`: watch it and the folders under it (watch, then list),
  then give `Path` for it. A folder hint makes the watcher walk it, which covers what was written before the watch.
- **The rebuild.** One routine walks the root's folders again, watching each before listing it (adding a watch to a
  folder that has one returns the same descriptor), removes the descriptors the walk didn't reach, and gives `Rescan`.
  It runs when a watched folder moves within or out of the project (folder moves are rare, WRITE-12) and on
  `IN_Q_OVERFLOW` (wd −1, after exactly 16,384 queued events, measured). The queue is the shared instance's, so an
  overflow rebuilds every watcher's root and gives each of them `Rescan`; a descriptor goes only when no watcher's walk
  reached it. Without the rebuild an overflow loses the
  new folders' `IN_CREATE`, and those folders stay unwatched: 668 to 679 of 2,000 in a probe whose reader was held up
  300 ms.
- **Rescan** also on `IN_UNMOUNT`, and on the root's `IN_MOVE_SELF` or `IN_DELETE_SELF`. `IN_IGNORED`: forget the
  descriptor. *(Corrected in the review: the root's `IN_DELETE_SELF` never comes while a file in the project is held
  open, and the session holds `.music2/lock` open. So each flush also checks what the root's path names, by volume and
  inode, and a folder put there since (a restore, the folder made again, a copy moved in) is watched, and the old tree
  let go. The root's `IN_MOVE_SELF` or `IN_DELETE_SELF` rebuilds at once.)*
- **A watch that fails after start** (`ENOSPC` when the user's watches run out, `ENOMEM`) leaves that folder
  unwatched and gives `Rescan`. Changes there are then found only by the 5 s walks, and the watcher's existing rule
  ("events that missed changes twice in 10 minutes give way to walks") moves the project to walks.
- **Names are hints.** Rename cookies are never paired; a rename from outside gives only `IN_MOVED_TO`, and a rename
  over a file gives no `IN_DELETE` (measured). Writes through `mmap` give no event until close (measured): the settle
  rule and the walks every 5 s cover them.
- **Flush** takes the instance's mutex and reads until `EAGAIN`. That is complete, because an event is queued before
  the writer's call returns (0 misses in 2.8 million checks, same and other process, ext4 and tmpfs). Since dispatch
  also runs under that mutex, a dispatch in progress ends before the flush returns. The reader's pause never delays a
  flush.
- **Drop** takes the mutex, unregisters the watcher and removes the watches only it held; no `heard` runs after.
- **Limits** here: 997,324 watches, 128 instances per user, 16,384 queued events. A failed `inotify_init1` or watch at
  start is an error (the watcher walks and says why).

**A Rescan is not a miss (S8, every OS).** Today a walk that a Rescan asked for records every change it finds as
"unnamed" (`watcher.rs` `apply`), since the lost events, or the hints dropped past `MAX_HINTS`, never named them. After
60 s each burst counts as one miss, and two bursts in 10 minutes switch the project to walks for the rest of the
session. That happens on the Mac today: two `git checkout`s of more than 4,096 files in 10 minutes do it. After S8 the
plan remembers that a Rescan asked for the walk, and that walk records no unnamed changes and clears the waiting ones.
Misses then mean what the rule says: events that never named a change.

### 3.5 The plugin main thread (HOST-2) and thread identity

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-23 | `Kind::Dispatch`: the main dispatch queue and the `CFRunLoop` pump | Moved (S5): `mac/main_loop.rs` (`ProcessMain` and `AppMain`) | — | — |
| OS-24 | `is_process_main()` | Built (S5): `thread::is_process_main` | `gettid() == getpid()`: two system calls, no allocation, no lock. `ground/tests.md` checked it: test 11 and the no-alloc tests pass with it. | the thread id recorded when the `ProcessMain` loop starts |
| OS-25 | `token()` (`pthread_self`) | Moved (S5): `thread::token` (`unix.rs`); `main_thread::token` and `is_process_main` stay as wrappers, since step 7's `probe.rs` calls them 6 times | a register read | `GetCurrentThreadId` (a bare `pthread_self` wouldn't link there) |
| OS-26 | the app's executor (`use_process_main_thread`) | Built (S5): `Loop::start(Where::AppMain)` through `use_app_main_thread`; macOS unchanged; Linux a loop thread of its own (§2.6) | plugins in the Linux app, on that thread; GTK keeps its own | the same as Linux, with a message pump |
| OS-27 | `run_main` off macOS | Moved (S5): `Where::ProcessMain` on Linux is today's job queue. CLAP's `timer-support` and `posix-fd-support`: deferred to step 9. | Surge XT and Airwindows pass live3b's plugin part without them (51 of 51, measured) | a message pump |
| OS-28 | the probe's `process_main`; `core/plugins/tests/main_thread.rs` | No change: OS-24 fixes it | — | — |
| OS-29 | `leave_now` (`_exit`) | No change (the shell) | same | `TerminateProcess` |
| OS-30 | `appkit_main_thread.rs` | Built (S5): renamed `app_main_thread.rs`. Its work (the song with the probe plugin, the carried instance, quit's teardown, the plugins' part) runs on every OS and asserts each main-thread call ran on the plugin main thread, which is the process's main thread exactly when `APP_ON_PROCESS_MAIN`. Only AppKit's run loop stays under `cfg(target_os = "macos")`. Every item is then used on Linux, which ends clippy lints 7–15. | the app's hook on its loop thread, with no window | the same file |
| OS-31 | host extensions offered | Deferred to step 9, with the loop's fds and timers | not needed for audio here (OS-27) | — |

### 3.6 CLAP discovery and loading (`core/plugins/src/lib.rs`)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-32 | `search_paths()` | Built (S5): `CLAP_PATH` first, then `dirs::clap()` | `~/.clap`, `/usr/lib/clap`; both missing here. Never install the plugins in either: `fx-fixes` needs them missing. The checks' copies live in `~/.local/share/m2-test-plugins/clap` (`ci/plugins.sh`, §7.3), named only by `CLAP_PATH`. | `%LOCALAPPDATA%\Programs\Common\CLAP`, `%COMMONPROGRAMFILES%\CLAP`; `;`-separated |
| OS-33 | `binary_in()` | No change | a Linux `.clap` is the library itself (loads, measured) | a DLL |
| OS-34 | one-level scan (the spec says recursive) | Deferred: the same on every OS; step 9 decides | — | — |
| OS-35 | `load()` | No change | `RTLD_LAZY \| RTLD_LOCAL` loads and plays both plugins here | `load_with_flags(LOAD_WITH_ALTERED_SEARCH_PATH)` |

### 3.7 Paths and folders

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-36 | `songs_dir()` = `$HOME/Music/music2` | Moved (S6): `dirs::songs()`, the same path on both | `~/Music/music2`. `XDG_MUSIC_DIR`: designed, deferred to Linux packaging (servers don't set it; the suites read `testhome/Music/music2`). | `FOLDERID_Music`, often in OneDrive (`project.synced`) |
| OS-37 | `cache_dir()` | Built (S6): `dirs::cache()`; macOS unchanged | `~/.cache/music2`, from `HOME` only so a test's scratch `HOME` isolates it (PLAT-7). Today it would make `~/Library/Caches` on Linux. | `%LOCALAPPDATA%\music2\cache` |
| OS-38 | `~/.music2/session.json` (0600) | No change (the shell) | same | `%USERPROFILE%\.music2` |
| OS-39 | `bundled()` resources | No change: plain path code, kept in `resources.rs` (§2.4) | Harmless: no Linux program sits in `X/Contents/MacOS/`. Designed for packaging: `<exe>/../lib/music2/<name>` (Tauri's deb and AppImage layout) as a second candidate, with its own test. | the program's folder |
| OS-40 | `source_tree()` | No change | — | — |
| OS-41 | `folder_part`, `reserved`, `make()` | Built (S2, Q1): a new song's folder name counts as taken when any entry in the folder matches it ignoring case, on every OS. The Mac is unchanged (APFS already says so). | ext4 tells cases apart, so today "exports 2" is free next to "Exports 2" | also refuse `< > " \| ? *`, a trailing dot or space, device names |
| OS-42 | `AssetName` allows `: * ? " < > \|` | No change: a format rule for the Windows step (the owner's call then) | legal | not creatable |
| OS-43 | the web's `~` display | No change in the web (its `home` now comes from `dirs::home`) | — | `\` paths |
| OS-44 | WRITE-12 dot files | No change | an editor's `name~` backup refuses a folder Rename, and says why | `Thumbs.db`, `desktop.ini` |

### 3.8 Floating point and render determinism (details in §5)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-45 | host math in `core/runtime/src/audio.rs` (8 calls) | Built (S4): the `libm` crate; a test bans host math | §5.1 | same code |
| OS-46 | the kernels' math | No change: inside `std.wasm` it is compiler-builtins' libm, equal to the `libm` crate bit for bit (measured) | same | same |
| OS-47 | `core/play/src/resample.rs` (live only) | Built (S4): `libm::sin`, `libm::cos` | its f32 tables came out the same under glibc and `libm` (measured) | same |
| OS-48 | wasmtime settings (NaN canonicalization off, relaxed SIMD off) | No change | 6 of 8 renders were already equal to the Mac's on x86_64 | same |
| OS-49 | denormals | Deferred to 8a (§5.6) | x86 pays a microcode assist per subnormal | same as x86 |

Not an OS piece, found by `design-shaped`: QuickJS's `Math` and `**` call the OS's C math library. Built in S4 (§5.2).

### 3.9 The desktop shell (Tauri)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-50 | `bundle.targets: ["app"]` | Deferred to a packaging step (trigger: a Linux release) | a deb needs webkit2gtk-4.1, GTK 3, libsoup 3, ALSA; built here it needs glibc 2.39 or newer (`facts.md` §9) | NSIS or MSI; WebView2; signing |
| OS-51 | `gen/schemas/<os>-schema.json` | Built (S1): commit `linux-schema.json`, so a Linux build leaves the tree clean. It is byte-identical to `macOS-schema.json` (checked). | — | `windows-schema.json` then |
| OS-52 | traffic lights, `pl-[88px]` | No change: `taste` owns the window builder | native title bar above the top bar: fine | same |
| OS-53 | `__M2__.platform` | No change | `"linux"` | `"windows"` |
| OS-54 | menus (`menu.rs`) | Built (taste's next part, A3): one menu bar per OS (`layout(mac)`, tested for both on any OS). The Mac's as before. Elsewhere no app menu: Quit (Ctrl+Q) ends File, About (GTK's dialog, with the version and icon) ends Help; no Services, Hide, Full Screen or Window menu, and no Cut, Copy, Paste or Select All rows (muda's GTK rows need libxdo; the web view has the keys). Labels per OS through `native.ts`'s `chord`, `opt`, `shifted`, `keyName` and `mod`; Ctrl+Backspace in a field deletes the word before. | GTK greys out Services, Hide, Hide Others, Show All, Full Screen (muda source) | the same items; Redo is Ctrl+Y |
| OS-55 | `reveal_file`; "Show in Finder" labels | Built (taste's next part, A3): "Show in Finder" on a Mac, "Show in Folder" elsewhere, in the File menu and both toasts (`revealLabel`). Linux opens the folder (`xdg-open` is missing here, `gio` is the fallback). | `FileManager1.ShowItems` over D-Bus would select the file: designed | `explorer /select,` |
| OS-56 | file dialogs (rfd) | No change | GTK chooser; needs a display | `IFileDialog` |
| OS-57 | `windows_subsystem` | No change | — | already there |
| OS-58 | JIT and the hardened runtime | No change | SELinux `execmem` only in confined domains | — |

### 3.10 Audio I/O (`core/play`)

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-59 | `Player::start` (cpal) | No change: 8a owns the device (host, buffer size, realtime). Bug 1 (one xrun retires the player; on ALSA its error callback allocates on the audio thread) is the same on every OS: fixed first when step 7 is finished (§8). | cpal's only host here is ALSA. No sound card, no rtkit, `RLIMIT_RTPRIO` 0: realtime is impossible here as set up. | WASAPI shared; MMCSS |
| OS-60 | `M2_AUDIO=none` | No change; every check sets it | — | high-resolution timer |
| OS-61 | no audio device | Designed for 8a: "no device → silent player with a note" must also catch a failing `default_output_config` | measured: the default device exists, its config fails ("not available"), and alsa-lib prints 8 lines | same |
| OS-62 | the capture player (live3b) | No change | — | — |

### 3.11 Build, toolchain and packaging

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-63 | `std/build.sh` | Built (S1): honours `CARGO_TARGET_DIR` (today it copies from the wrong folder when one is set, as on this server); `--check` rebuilds a copy in another folder and compares. In S1 so that the check never rebuilds `std.wasm` in place. | — | Git Bash, or a `.ps1` twin |
| OS-64 | `std.wasm` holds the builder's path | Built (S3): `core/kernel` moves into the kernels' workspace; one deliberate rebuild (§5.4) | measured path-independent | same |
| OS-65 | no pinned toolchain | Built (S1): `rust-toolchain.toml` | already 1.99.0 here | same file |
| OS-66 | system libraries | No change: all present here (`facts.md` §9). A fresh Ubuntu 24.04 needs `pkg-config build-essential libasound2-dev libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libdbus-1-dev`. | — | MSVC build tools; QuickJS under `cl.exe` *(unverified)*; `llvm-rc` for the app's build script |
| OS-67 | npm lockfiles | No change: they list the Linux and Windows bindings | — | — |

### 3.12 Tests and the browser suites

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-68 | Unix-only APIs in tests outside `cfg(unix)` | No change: they compile on both OSes | — | the Windows step's list (§8) |
| OS-69 | permission tests | No change; never run checks as root | runs as the user | ACLs |
| OS-70 | symlink tests | No change | — | Developer Mode |
| OS-71 | `temp_dir()` is case-sensitive | No change: the case-folding paths are tested on the Mac run (a casefold or vfat volume needs root here) | — | NTFS folds case |
| OS-72 | core dumps from the kill tests | Built (S6): the test children that abort on purpose (`kill_tests::child`, `hook_tests::hook_child`) call `prctl(PR_SET_DUMPABLE, 0)` under `cfg(target_os = "linux")`, a test fixture. macOS: nothing, as today. | apport takes about 1.1 s per dump even with `ulimit -c 0`; the kill test drops from 473 s to 104 s (measured). `RLIMIT_CORE` 0 alone is not enough. | crash reports off for the child |
| OS-73 | Chrome path (`chrome.mjs`, `run.mjs`) | Built (S2): per OS in `chrome.mjs`; `run.mjs` takes it from there | `linux64` 133.0.6943.141 here; no linux-arm64 build of 133 exists | `win64` |
| OS-74 | `isMac` off the app | Built (S2): `isMac` follows `__M2__.platform` when a page is given one (`native.ts`, one line) | — | — |
| OS-75 | `Meta` chords inside Monaco | Built (S2): off the Mac, Chrome runs through `e2e/chrome-as-mac.sh`, which gives pages a Mac user agent. The PC-keys pass is built (taste's next part, A3): `e2e/pc-keys.mjs` gives its page a PC user agent and `platform: "linux"` on every OS. In the app, a menu chord the editor binds runs its own action per OS (Ctrl+Backspace: `deleteWordLeft`). | Monaco takes its key map from the user agent | same, through a `.cmd` twin |
| OS-76 | fonts from Google Fonts | Built (taste's next part, A2): `taste` removed Google Fonts. The Mac keeps its system font; elsewhere the UI is in Inter, from `web/public/fonts` (OFL). `e2e/taste.mjs` checks the font each page draws in. | Inter | Inter |
| OS-77 | `shasum` in `baseline.sh` | No change: both OSes have it | — | Git Bash |
| OS-78 | `SIGTERM`/`SIGKILL` in `run.mjs` | No change | — | `taskkill /T` |
| OS-79 | Chrome's libraries | No change: present here | — | — |

### 3.13 Gates and measurements

| id | Piece | This step | Linux | Windows |
|---|---|---|---|---|
| OS-80 | `thread_cpu()` | Moved (S5): `thread::cpu_time` | `CLOCK_THREAD_CPUTIME_ID` | `QueryThreadCycleTime` |
| OS-81 | G11's "F_FULLFSYNC" and "watched by FSEvents" labels | Built (S6, S8): the gates time `fs::barrier` and say "the durable sync (F_FULLFSYNC on macOS, fsync on Linux)"; "watched by file-system events". The Mac run times them (§7.7). | — | — |
| OS-82 | step 7's `store.rs` `memory()` (Mach `task_info`) | Designed (§8): `process::memory()`, built at step 7's merge. G7f's memory rules must fail, never pass, when it is `None` (today they pass by default on Linux: 0 ≤ 0). | `/proc/self/status` (`RssAnon` + `VmSwap` as the footprint, `VmSize` as mapped); the address space from the CPU's virtual-address bits (48 here: 128 TiB) and `RLIMIT_AS` | `GetProcessMemoryInfo` |
| OS-83 | step 7's QoS for the drop test's thread | Designed (§8): `thread::interactive()`, at step 7's merge | nothing allowed here (`RLIMIT_RTPRIO` 0): reported, never fatal | `SetThreadPriority` |
| OS-84 | M1 records (`store-S.txt`, `callback-S.txt`) | Designed (§8): each record names its machine (`process::machine()`), at step 7's merge | a Linux run is recorded beside the M1's, never judged against it (PLAT-1) | same |
| OS-85 | the reference Mac for timings | No change: written into PLAT-1 | Linux timings are for information | same |

---

## 4. Every Linux failure today, and what fixes it

From `ground/tests.md` and `ground/e2e.md`, at `f126d03`. Slices S1–S9 are in §13.

### 4.1 Rust tests (11)

| # | Test | Root cause | Fixed by |
|---|---|---|---|
| 1 | `graph::watch_tests::an_idle_project_is_watched_without_a_stat` | no event source off macOS | S8 (inotify) |
| 2 | `graph::watch_tests::own_commits_make_no_batch_with_events` | same | S8 |
| 3 | `graph::watch_tests::events_that_miss_changes_give_way_to_walks` | same: with no events, nothing can be missed | S8 |
| 4 | `graph::watch_tests::an_event_wakes_an_idle_watcher_and_a_batch_wakes_it_when_due` | nothing calls `Wake::wake` off macOS | S8: `heard` adds a hint, and the hint wakes |
| 5 | `graph::tests::a_lane_written_in_two_halves_never_plays_empty` (its events case) | asserts `events == !walk`; there are no events | S8. inotify names every write (`IN_MODIFY`), grouped over 10 ms, where FSEvents named the close; the test accepts one or two revisions in its events case (`tests.rs:1027`) *(unverified until S8)*. |
| 6 | `tx::hook_tests::a_commit_that_fails_after_its_preview_accepts_nothing` | no `Swap` on Linux: every replace takes the fallback | S6 (`renameat2`) |
| 7 | `tx::stage_tests::a_rollback_that_fails_is_reported_at_once` | same | S6 |
| 8 | `tx::kill_tests::killed_at_every_commit_step_the_project_recovers` | same, and the fallback's middle state offers no fix (a session bug) | S6 makes it pass on ext4; S7 removes the middle state (§3.1) and runs the test on a disk that can't swap too |
| 9 | `graph::preview_tests::an_interrupted_commit_after_its_preview_points_to_needs_attention` | same as 6 | S6 |
| 10 | `gate_tests::fixture_renders_change_only_with_stds_version` | glibc's `cosf` and friends in `audio.rs` | S4 (`libm`; record once) |
| 11 | `core/plugins/tests/main_thread.rs:78` (no harness) | `is_process_main()` always false off macOS | S5 (`gettid() == getpid()`) |

Tests 6, 7, 8 (on ext4), 9 and 11 passed with exactly these fixes in a scratch copy (`ground/tests.md`). Test 10 with
the exact `libm` patch was measured for `design-shaped` (§5.3).

**Not a Linux failure, fixed in S1 anyway.** `files::tests::diagnostics_say_where_in_the_project_each_problem_is`,
listed in `HANDOFF.md` as a flake, fails every time in a checkout whose path is over 48 characters. QuickJS writes a
module name into a 64-byte buffer for its messages (`ATOM_GET_STR_BUF_SIZE`, `quickjs.c:3821`), and a package module is
named by its absolute path (`core/script/src/lib.rs` `resolve`). Measured by `attack-verification` in a 52-character
scratch path: "Could not find export 'nope' in module 'std/src/ds'". The workflow's review agents check in
`$SCRATCH/scratch-<label>/` copies, so the check would say FAIL at every slice there. S1 names package modules as
`display()` already shows them (`std/src/dsp.ts`), and the loader maps the name back to its file. A user whose app sits
in `~/Applications` would see the same cut-off name *(unverified in the app)*.

### 4.2 Clippy `-D warnings`, whole workspace (15 lints, all dead code from macOS-only code)

| # | Lint | Fixed by |
|---|---|---|
| 1 | `core/plugins/src/main_thread.rs:217` `install` never used | S5: every `Where` installs through it |
| 2 | `core/session/src/lock.rs:213` unused `dir` | S6: `fs::is_local` has a Linux body |
| 3 | `core/session/src/tx/stage.rs:106` `cpath` never used | S6: moved into `unix.rs`, used by both folders |
| 4 | `crates/server/src/graph/watcher.rs:55` `wake` never used | S8 |
| 5 | `watcher.rs:134` `backstop` never read | S8 (the events arm exists on every OS) |
| 6 | `watcher.rs:216` `deaf` never read | S8 |
| 7–15 | `apps/music2-desktop/tests/appkit_main_thread.rs` 28, 115, 117, 125, 147, 150, 159, 241, 265 | S5: the test runs on every OS (OS-30) |

With `-D warnings`, clippy stops at the first crate that fails, and every later crate depends on `m2-plugins` (lint 1),
then on `m2-session` (lints 2, 3). So the slices clear them in that order: S5, then S6, then S8. Until then the lead
lists every lint with a run without `-D warnings`, as `ground/tests.md` did.

### 4.3 The desktop app

| Check | Today on Linux | After |
|---|---|---|
| `cargo build -p music2-desktop` (after the web build) | ok | ok; `linux-schema.json` committed, so the build leaves the tree clean (S1) |
| `cargo test -p music2-desktop` | exit 0, but tests nothing: 0 unit tests, and the AppKit test prints "macOS only" | `app_main_thread.rs` runs the app's plugin hook on Linux too (S5) |
| plugins in the Linux app | "plugins can't be hosted" | hosted on the app's loop thread (S5); the app itself is never launched here |
| `cargo build --release --workspace` | ok (1 min 37 s) | ok |

### 4.4 Browser suites (29 runs: 7 FAIL, 1 PART, 21 ok)

| Run | Cause on Linux | Fixed by (all in S2) |
|---|---|---|
| code-proj, fixes-graph-fix, attention-attn, s6-write-fix | Monaco takes its key map from the user agent, so ⌘ chords do nothing inside the editor | `e2e/chrome-as-mac.sh`. Measured: with only a Mac user agent these 4 pass in full (`e2e.md`; on `taste` too, `branches.md`). |
| export-export | off the app, `isMac` reads `navigator.platform`, so the tooltip says "Ctrl+E" | `native.ts`: `isMac` follows `__M2__.platform` when given |
| fx-fixes-none | ext4 tells "exports 2" from "Exports 2" | Q1: a name taken in any capitals, on every OS (`project.rs`), which gives the Mac's answer, "exports 3". Measured by `attack-verification`: `fx-fixes` passes with it and no suite change. If Q1 is no: the suite asks the disk whether it folds case (`e2e.md`, measured). |
| notes-proj ("the whole drag is one write") | the suite reads the file, then the revision; Linux saves so fast that it reads between the rename and the revision (2.7 ms, measured). The Mac's `F_FULLFSYNC` hid this. | `notes.mjs:97`: wait until the revision moves, wait 300 ms, then check it moved by exactly one |
| live3b-proj (PART) | no plugins on `CLAP_PATH` | `ci/plugins.sh` (S1) unpacks them without sudo; `ci/check.sh` finds them. Measured: 51 of 51. |

With all of these, `attack-verification` measured **29 of 29 runs ok, 1,034 of 1,034 checks**, twice (once inside the
design's `ci/check.sh`), in a scratch copy built as this section writes it (`$SCRATCH/logs/av-e2e1-*.log`).

The pieces, all small:

```js
// web/src/lib/native.ts:16: isMac follows the platform a page is given first (OS-74)
export const isMac = window.__M2__?.platform ? window.__M2__.platform === "macos" : /Mac/.test(navigator.platform);
```

```js
// e2e/chrome.mjs: the pinned browser for this OS (OS-73); off the Mac through a wrapper that gives pages a Mac
// user agent (OS-75, Q2). run.mjs takes `real` from here.
import os from "node:os";
import { fileURLToPath } from "node:url";

const mac = process.platform === "darwin";
const cache = `${os.homedir()}/.cache/puppeteer/chrome-headless-shell`;
export const real = process.env.E2E_CHROME ?? (mac
  ? `${cache}/mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64/chrome-headless-shell`
  : `${cache}/linux-133.0.6943.141/chrome-headless-shell-linux64/chrome-headless-shell`);
if (!mac) process.env.E2E_CHROME_BIN = real;   // puppeteer passes the environment on to the wrapper
export const exe = mac ? real : fileURLToPath(new URL("./chrome-as-mac.sh", import.meta.url));
```

```js
// e2e/notes.mjs:97 (and the same at s6-fixes.mjs:112 with revF): one write, once the revision is published
check((await until(async () => (await revision()) > rev)) && (await sleep(300), (await revision()) === rev + 1), "the whole drag is one write");
```

`e2e/chrome-as-mac.sh` is in §7.3. In `project.rs`'s `make()` (Q1), a candidate is skipped when the folder lists a name
whose `to_lowercase()` equals the candidate's, the rule `reserved()` already uses for `Exports`; a unit test covers it.

**Passed by luck today, fixed anyway:**

| What | Fixed by |
|---|---|
| a project file missing for up to 0.19 ms during each save (live3a crashed on it once in 11; a probe saw 231 of 116,400 reads miss) | S6: `renameat2` swaps on ext4; S7: on a volume without a swap, one rename over the file |
| `s6-fixes.mjs:112`: the same file-then-revision race as notes | S2, the same one-line wait |
| the default Chrome path is the Mac's | S2, `chrome.mjs` per OS |
| fonts load from Google Fonts | not on `main`: `taste` removes them (OS-76) |

**Not fixed, on purpose (Q2):** on Linux the suites test the Mac's keys and labels, because Chrome says it is a Mac and
the suites inject `platform: "macos"`. Ctrl, the key Linux users press, gets its own suite, `pc-keys` (built in taste's next part,
A3). This keeps every suite file but two lines untouched on `main`. A trial merge showed `notes.mjs`,
`s6-fixes.mjs` and `native.ts` with these changes merge cleanly into `taste`, `wip/step7` and `sounds` (§11).

### 4.5 Offline renders

| Render | Mac at `8aa301e` | Linux at `f126d03` | After S4, every OS |
|---|---|---|---|
| examples-first-song | `6e61b87f…` | same | same |
| examples-first-song-from4-for6 | `ef0fadca…` | same | same |
| examples-resample | `6ac6fcb1…` | `d902bacf…` | `39f94764…` (measured on Linux; predicted for the Mac) |
| examples-resample-from4-for6 | `a360d04c…` | `ea243ccc…` | `d752521b…` (same) |
| templates-empty | `835aff53…` | same | same |
| templates-empty-from4-for6 | `a0249c1e…` | same | same |
| templates-starter | `413d35cf…` | same | same |
| templates-starter-from4-for6 | `71bb7191…` | same | same |

### 4.6 What "Linux green" means

All of these, on this server, in one `ci/check.sh` run:

| Check | Pass means |
|---|---|
| toolchain | `rustc 1.99.0`, as `rust-toolchain.toml` pins |
| web build | `npm ci && npm run build` in `web/` exits 0 |
| build | `cargo build --locked --workspace --all-targets`, the desktop app included |
| Rust tests | `cargo test --locked --workspace --no-fail-fast` exits 0: every test, the desktop app's, both no-harness tests, the with-plugins parts, the kill test on both disks and the new watcher tests included (today 686 pass, 11 fail) |
| clippy | `cargo clippy --locked --workspace --all-targets -- -D warnings` exits 0 |
| std kernels | the kernels' workspace: `cargo test --release` and clippy `-D warnings` exit 0 |
| std.wasm | a rebuild in another folder equals the committed file |
| mac lint | the same clippy for `--target aarch64-apple-darwin` exits 0 (§2.7) |
| browser suites | 29 of 29 ok, live3b in full with the plugins `ci/plugins.sh` installs |
| renders | the 8 offline renders match `e2e/renders.sha256`, the one record both OSes use |
| repo unchanged | the check wrote nothing in the working tree |

The headline must read `music2 check: PASS`: not `PASS without plugins`, and nothing passed only on a retry without the
lead saying why.

---

## 5. Determinism

### 5.1 Host math through `libm` (OS-45, OS-47)

`libm` 0.2.16 is already in `Cargo.lock`. `m2-runtime`, `m2-play` and `m2-script` add `libm = "0.2"`; no new crate.

| File:line at `f126d03` | Today | After (S4) |
|---|---|---|
| `core/runtime/src/audio.rs:191` WSOLA window | `(TAU * i / FRAME).cos()` (f32) | `libm::cosf(…)`, same argument |
| `audio.rs:253` pitch | `2f64.powf(clip.pitch / 12.0)` | `libm::exp2(clip.pitch / 12.0)` |
| `audio.rs:283` gain | `10f32.powf(clip.gain_db as f32 / 20.0)` | `libm::powf(10.0, clip.gain_db as f32 / 20.0)` |
| `audio.rs:294`, `297` fades | `(PI * i / fade).cos()` | `libm::cosf(…)` |
| `audio.rs:411`, `424` declick | `(FRAC_PI_2 * k / len).sin()` | `libm::sinf(…)` |
| `audio.rs:415` declick | `g0 * (…).cos()` | `g0 * libm::cosf(…)` |
| `core/play/src/resample.rs:46` sinc (f64, live only) | `.sin()` | `libm::sin(…)` |
| `resample.rs:49` Blackman window (f64) | `.cos()` twice | `libm::cos(…)` twice |

- **Why `exp2` by name.** A release build turns `2f64.powf(x)` into `exp2`; a debug build calls `pow` (`facts.md`
  §6a). Inside `libm` the two differ on 9.5% of a pitch grid. Naming `libm::exp2` makes debug builds (the Rust tests)
  and release builds (`m2 render`, exports) agree on every OS. At this fixture's pitches (−5, 0, 7, 12) `libm::exp2`
  and `libm::pow` give the same bits (measured), so both recorded hashes below hold.
- **Why it is the same everywhere.** `libm` is plain Rust IEEE arithmetic; Rust never fuses or reorders floating
  point, and `libm`'s `sqrt` and `fma` are exact. Its only per-CPU code is in exact operations (`sqrt`, `fma`, `rint`,
  `floor`), so the functions above run the same Rust on x86_64 and aarch64 (`attack-macos` read its source). On this
  machine `libm` equals the math inside `std.wasm` bit for bit (every f32 for `cosf`, `sinf`, `powf`; 67 million
  samples each for the f64 functions; `facts.md` §6f).
- **The test,** `core/runtime/tests/host_math.rs` (the pattern of `core/play/tests/no_blocking.rs`). It reads every
  `.rs` file under `core/runtime/src` and `core/play/src`, skips `#[cfg(test)]` items, and fails on `.sin(`, `.cos(`,
  `.tan(`, `.sin_cos(`, `.powf(`, `.exp(`, `.exp2(`, `.exp_m1(`, `.ln(`, `.ln_1p(`, `.log(`, `.log2(`, `.log10(`,
  `.tanh(`, `.sinh(`, `.cosh(`, `.asin(`, `.acos(`, `.atan(`, `.atan2(`, `.cbrt(`, `.hypot(` and `.powi(`, each with
  its bracket, so `.expect(` never matches. At `f126d03` it finds exactly the 11 calls above
  (checked with grep; no `.powi(` there). `sqrt`, `floor` and arithmetic are exact everywhere and stay. `powi` is not
  correctly rounded, and a release build may expand it into other multiplications than a debug build's call, so it is
  banned too. Step 7's moved `audio.rs`
  has the same 8 calls and no new ones (`branches.md` §2.2); the test keeps it that way.
- **Left alone:** `crates/editor/src/render.rs:307` (`log10` of a peak, a number shown to the user),
  `core/kernel/src/lib.rs:235` (host math only in host tests; in use it runs inside `std.wasm`), `taste`'s colour names
  in `crates/editor/src/layout.rs` (display only), the test plugin's synth in `core/plugins/src/probe.rs:551-552` (a
  hosted plugin, so its songs are not reproducible by RENDER-2), and `crates/gates/src/signal.rs` (test signals and
  level readings).

### 5.2 Song code: `Math` and `**` inside QuickJS

Song code runs in QuickJS. Its `Math.sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`,
`asinh`, `acosh`, `atanh`, `exp`, `expm1`, `log`, `log1p`, `log2`, `log10`, `pow`, `cbrt` and `hypot`, and the `**`
operator (`OP_pow`), call the C math library of the OS (`rquickjs-sys 0.14.0`, `quickjs.c` 15025–15037, 15475, 15547,
48669–48686, 49012–49050; read in source). So a song that writes `440 * 2 ** ((n - 69) / 12)` can build different
constants on the Mac and on Linux. No fixture, std file or SDK file uses `Math.` or `**` today (checked with grep;
`sounds` adds none, `attack-simplicity`), so no recorded hash shows it yet.

The fix (S4), where the prelude already removes `Date` and `Math.random` (`core/script/src/lib.rs`, `PRELUDE`):
- those 22 functions become native functions over the `libm` crate (f64). `pow` keeps JS's special case
  (`pow(±1, ±∞)` is NaN), and `hypot` folds its arguments as QuickJS does;
- the TypeScript transform lowers `a ** b` and `a **= b` to `Math.pow` (`oxc_transformer` 0.153.0, the version in
  `Cargo.lock`: `TransformOptions.env.es2016.exponentiation_operator = true`, read in source; `core/script/src/lib.rs`
  uses `TransformOptions::default()` today), so the operator takes the same path.

It should change no recorded hash, since no recorded song uses them *(checked in S4: the transform's option may change
other output)*. A test builds a song that uses `Math.sin`, `Math.pow` and `**` and compares its constants with `libm`'s
bits.

### 5.3 One deliberate change of the recorded hashes

S4 is the only slice whose renders change. S1 records the Mac's hashes from `HANDOFF.md`; S4's commit changes exactly
three lines, so the change is one readable diff.

| Record | Entry | Before (the Mac at `8aa301e`) | After S4 |
|---|---|---|---|
| `core/session/tests/renders.json` (debug renders, the fixture test) | `examples/resample` | `cb4d39ba83db404c19e6c7cd843fc7744838f3c1df9fa58fb315d2e1b7b79f7f` | `b3daee50ee7e7193fc6d240f02384fcda83ccbe33b5c20fbf5904005b4aeb58e` |
| `e2e/renders.sha256` (new in S1) | `examples-resample.wav` | `6ac6fcb117fe5b1dd1545bf3cf1fd38e03a0fc09864d0b339a499a32c925eea7` | `39f9476492e997a86aec5f94b6d0edf56c098e275186260915a3830bd9cfe0da` |
| same | `examples-resample-from4-for6.wav` | `a360d04c9fba10e4eee079360fa93e52578a2c2b3312dc209f192b609f9c1dbf` | `d752521b47b66bcb9076f43715baedaa5c018f428ec90546465e6cae031560eb` |

- **Measured on Linux** with exactly the §5.1 patch (`design-shaped` A2, `$SCRATCH/logs/dsh-libm-*`): `renders.json`
  gives `b3daee50…`; the release `m2` renders `39f94764…` and `d752521b…`, and the other renders are unchanged. Both WAV
  hashes equal what `facts.md` §6e predicted with `libm` swapped in through `LD_PRELOAD`: two methods agree. The debug
  `m2` gives the same 8 (`attack-verification`).
- **The Mac must give the same three** *(unverified until the owner's run)*. If it doesn't, the step is not done: the
  cause is found (a call left out, or a CPU difference inside `libm`), never a second record per OS.
- No std version bump: std didn't change. RENDER-2 already names the runtime among what a render's bits depend on.
- The commit message says: "Deliberate change of recorded renders (decided): host math uses the libm crate;
  examples/resample changes in renders.json and e2e/renders.sha256; no other render changes."
- `e2e/renders.sha256` uses the names `baseline.sh` writes (`…-from4-for6.wav`; `HANDOFF.md` writes `…-from4.wav`).
  `baseline.sh` gains the eighth case (`templates/empty --from 4 --seconds 6`), which `$SCRATCH/check.sh` adds by hand
  today.

### 5.4 `std.wasm` rebuilds the same everywhere (OS-63, OS-64, OS-65)

| Fact | Source |
|---|---|
| `std.wasm` holds the Mac's path `<repo>/core/kernel/src/lib.rs` | `facts.md` §7 |
| A path remap rebuilt the committed file only from `650f842`'s sources. At HEAD 10 bytes (panic line numbers) differ: `933f4de` added 10 lines to `core/kernel/src/lib.rs` after the last rebuild. | `facts.md` §7 |
| A remap is not enough: `m2-kernel` is a path dependency outside the kernels' workspace, so cargo hashes its absolute path into `-C metadata`, which reorders the layout (1,003 bytes differed between two folders for `sounds`). | `branches.md` §4.3 |
| With `m2-kernel` inside the kernels' workspace, two folders gave identical bytes, with no machine path. Another target folder too. | `branches.md` §4.3; `attack-verification` |
| A `rustc` that says `host: aarch64-apple-darwin` gave the same metadata and the same bytes: cargo leaves the host out for wasm32 units. | `attack-macos` (`am-stdhost-*.log`) |

The change:
1. **S1:** `std/build.sh` honours `CARGO_TARGET_DIR` and gains `--check` (§7.3): it copies `std/` and
   `rust-toolchain.toml` to a temporary folder, builds there with its own target folder, refuses a file that holds
   `/Users/` or `/home/`, and compares with the committed file. Its row says FAIL until S3, as §13.1 expects; the check
   never rebuilds `std.wasm` in place.
2. **S3:** `git mv core/kernel std/kernels-src/kernel`. Inside the kernels' folder it becomes a member of that
   workspace by itself. Its manifest's `version.workspace`, `edition.workspace` and `license.workspace` become plain
   values (that workspace has no `[workspace.package]`). The main workspace's `m2-kernel = { path =
   "std/kernels-src/kernel" }` follows; `std/kernels-src` is already excluded, and `m2-runtime` already dev-depends on
   `std/kernels-src` the same way. `m2-kernel` has no tests of its own (checked), so the main workspace loses none; its
   lints move to the kernels' clippy (which passes, measured by `attack-verification`).
3. **S3:** rebuild once, on purpose. That also takes in the 10 stale bytes.

What it means: `std.wasm`'s bytes change, so every kernel's module hash changes once. Offline renders don't: the code
is the same, and every offline render starts its kernels from init (RENDER-1, RENDER-25). S3 proves it:
`renders.json`, `bit_identity` and the renders stay byte-identical (measured by `attack-verification` in a scratch
copy). Nothing in the repo pins `std.wasm`'s hash (checked: `83cc5961…` appears nowhere outside `docs/`). A running app
built before restarts its instances at the swap (RENDER-25).

On the Mac, `--check` also proves that macOS and Linux build the same bytes *(unverified until the Mac run)*.

`sounds`' merge: its `std.wasm` conflicts as a binary. Never take one side: rebuild with `std/build.sh` from the merged
sources, run `--check`, check the renders, commit once.

### 5.5 x86 and arm inside wasm

- Measured: 6 of 8 renders are bit-identical between the Mac (arm64) and this Ryzen (x86_64). Only host math differed
  (`facts.md` §6e), so the kernels' math is already the same on both CPUs.
- Why it holds: wasm's float operations are exact IEEE on both; Cranelift never fuses a multiply-add (wasm has none
  outside relaxed SIMD, which is off); `libm` inside the module is integer and IEEE code. Only NaN bit patterns may
  differ, and the renderer silences non-finite samples (OS-48).

### 5.6 Denormals on x86 (OS-49), in 8a

- x86 cores take a slow path for each subnormal operation; Apple silicon doesn't. A decaying reverb or filter tail can
  cost much more here *(how much: unverified)*.
- Flushing them in hardware (FTZ, DAZ) would change bits, which RENDER-2 forbids.
- 8a decides: kernels keep their state out of the subnormal range by a deterministic rule (a std change, with a std
  version bump per FMT-3), and `fpenv` checks MXCSR on x86_64, the twin of the FPCR check. Gate 8 stays judged on the
  reference Mac; an x86 run is recorded here.

---

## 6. Contract edits (`docs/architecture.md`)

A change that breaks a rule edits the rule in the same commit, so each edit lands in the slice named.

### 6.1 Rules that name macOS mechanisms, and the save rules

| Rule | Old text | New text | Slice |
|---|---|---|---|
| PLANE-5 | "and the commit goes on to journal, stage, sync (`F_FULLFSYNC`) and rename." | "and the commit goes on to journal, stage, make durable (one barrier, `m2_os::fs::barrier`: `F_FULLFSYNC` on macOS, or `fsync` on a volume that refuses it; `fsync` on Linux) and rename." | S6 |
| PLANE-5 *Why* | (end) | append: "On the Linux development server a whole commit's syncs took 0.6 ms idle and 1.5 ms at p95 while another program wrote 1 GB (drives with power-loss protection, so a best case); the budget is judged on the reference Mac." | S6 |
| WRITE-1 rule | "except creating a new asset with create-only semantics ([WRITE-18](../../architecture.md#the-project-lock))." | "except creating a new asset with create-only semantics ([WRITE-18](../../architecture.md#the-project-lock)). A file music2 creates outside a Tx (a new asset, a bounce's provenance, a new song's files) is made durable, its bytes pushed and its folder synced, before anything names it." | S7 |
| WRITE-15 *Today* | "pushed with `fsync`; one barrier (`F_FULLFSYNC`); renames checked at each step (`RENAME_SWAP`, `RENAME_EXCL`), rolled back in process when one finds an outside change." | "pushed with `fsync`; one barrier (`m2_os::fs::barrier`: `F_FULLFSYNC` on macOS, `fsync` where a volume refuses it; `fsync` on Linux, where each push is already durable); renames checked at each step with an exchange and a create-only rename (`m2_os::fs::rename`: `renamex_np` on macOS, `renameat2` on Linux), rolled back in process when one finds an outside change." | S6 |
| WRITE-15 *Today* | (after the sentence above) | insert: "A volume without them (FAT, exFAT, SMB, NFS; NTFS always) reads and compares the target, then renames the staged file over it in one rename, so every target always holds its before or its after bytes; a rollback renames the before bytes back over it the same way." | S7 |
| WRITE-15 *Today* | "a commit's journal and staging are removed after the next commit's barrier, or at close, so the removal is never durable before the renames are." | "a commit's journal and staging are removed after the next commit's barrier, or at close, and a completed journal found when the lock is taken only after its commit's folders are pushed and a barrier, so the removal is never durable before the renames are." | S7 |
| WRITE-16 *Today* | "another host is told by its hardware UUID" | "another host is told by its machine id ([FMT-20](../../architecture.md#presentation-and-local-files))" | S6 |
| WRITE-16 *Today* | (end) | append: "The kill test runs every case twice: on a disk that swaps, and on one that can't (`Recorder { plain: true }`); one case writes `music2.toml`." | S7 |
| WRITE-17 *Today* | "On a volume without `MNT_LOCAL` it takes the lock and raises the Notice `lock.local`" | "On a network volume (`m2_os::fs::is_local`: no `MNT_LOCAL` on macOS; on Linux an NFS, SMB, CIFS, 9p, Ceph, AFS, Coda or FUSE file system) it takes the lock and raises the Notice `lock.local`" | S6 |
| WRITE-19 *Today* | "on a local volume FSEvents names what changed (`crates/server/src/graph/fsevents.rs`, in-tree over CoreServices, for `FSEventStreamFlushSync`)" | "on a local volume the OS's file-system events name what changed (`m2_os::watch`: FSEvents on macOS, in-tree over CoreServices for `FSEventStreamFlushSync`; inotify on Linux, one instance per process, read at most every 10 ms as FSEvents' latency groups events, and an event is queued before the writer's call returns, so a flush is a read that never waits)" | S8 |
| WRITE-19 *Today* | "On a network volume (no `MNT_LOCAL`: its events name only this Mac's writes), when FSEvents won't start," | "On a network volume (its events name only this computer's writes), when events won't start (inotify's per-user limits included)," | S8 |
| WRITE-19 *Today* | (end) | append: "A walk that lost events asked for (a Rescan) counts no missed events." | S8 |
| FMT-16 | "keyed by `(path, size, mtime, ctime, inode)`, in `~/Library/Caches/music2/`," | "keyed by `(path, size, mtime, ctime, inode)`, in the OS's cache folder (`m2_os::dirs::cache`: `~/Library/Caches/music2/` on macOS, `~/.cache/music2/` on Linux)," | S6 |
| FMT-16 *Today* | "The CLI and the app keep the cache in `~/Library/Caches/music2/hashes.json`" | "The CLI and the app keep the cache in `hashes.json` there" | S6 |
| FMT-20 *Today* | "with the machine's hardware UUID as what tells another host's journal" | "with the machine id (`m2_os::host::machine_id`: the hardware UUID on macOS, `/etc/machine-id` on Linux) as what tells another host's journal" | S6 |
| RENDER-2 | (end of the rule) | append: "Host code that makes audio for a render, and song code in QuickJS, compute every transcendental function with the `libm` crate, never the OS's math library ([PLAT-4](../../architecture.md#platforms))." | S4 |
| RENDER-2 *Today* | (end) | append: "Since the platform step host math on the render path is `libm` (`core/runtime/tests/host_math.rs` bans host math in `core/runtime/src` and `core/play/src`), and so are QuickJS's `Math` and `**`, so `examples/resample` renders the same bits on macOS and Linux." | S4 |
| RENDER-30 | "Decoded and warped renders are cached in `~/Library/Caches/music2/`," | "Decoded and warped renders are cached in the OS's cache folder ([FMT-16](../../architecture.md#assets-and-hashes))," | S6 |
| Layout block (line 452) | "`~/Library/Caches/music2/     outside the project: decoded and warped renders, the hash cache`" | "`<cache folder>/music2/      outside the project, in the OS's cache folder (FMT-16): decoded and warped renders, the hash cache`" | S6 |
| HOST-2 rule | "In the app that is the AppKit main thread, reached through the main dispatch queue. In the CLI, the headless server and tests, the main thread runs a `CFRunLoop` and serves as the plugin main thread." | "That plugin main thread runs the OS's main loop (`m2_os::main_loop`). On macOS, where plugins post work to the process's main run loop, it is the process's main thread: in the app AppKit's, reached through the main dispatch queue; in the CLI, the headless server and the gates, the main thread runs a `CFRunLoop`. On Linux, where plugins get main-thread time only from the host (CLAP's fd and timer extensions), it is the process's main thread in the CLI, the headless server and the gates, and in the app a loop thread of its own, since GTK owns the app's main thread." | S5 |
| HOST-2 rule | "MUST make every main-thread call on the process's real main thread" | "MUST make every main-thread call on the plugin main thread below" | S5 |
| HOST-2 *Why* | "The cost: until the host process ships, a slow plugin load blocks the app's main thread." | "The cost on macOS: until the host process ships, a slow plugin load blocks the app's main thread. Linux has no process-wide run loop for plugins to post to, so its app keeps plugins off GTK's thread." | S5 |
| HOST-2 *Today* | "the app calls `m2_plugins::use_process_main_thread()` first thing in `main` (`apps/music2-desktop/src/main.rs`): the AppKit main thread, through the main dispatch queue." | "the app calls `m2_plugins::use_app_main_thread()` first thing in `main` (`plugin_host::install`, `apps/music2-desktop/src/main.rs`): AppKit's main thread through the main dispatch queue on macOS, a loop thread of its own on Linux (`tests/app_main_thread.rs` runs this hook on both)." | S5 |
| HOST-2 *Today* | "the main thread in its `CFRunLoop` serving the main queue;" | "the main thread running the OS's main loop (a `CFRunLoop` serving the main queue on macOS, a job queue on Linux);" | S5 |
| HOST-2 *Today* | "`is_main_thread` asks the executor (`pthread_main_np`, or the dedicated thread's `pthread_self`)" | "`is_main_thread` asks the executor (`m2_os::main_loop::Loop::is_current`: `pthread_main_np` for macOS's main thread, else the loop thread's token)" | S5 |
| HOST-3 | "and its own AppKit main thread for windows." | "and its own main loop for windows (`m2_os::main_loop`: AppKit's on macOS; on Linux a loop over the X connection, the plugins' fds and timers, serving CLAP's `posix-fd-support` and `timer-support`)." | S5 |
| ABI-1 *Today* | "ABI v0 states it (`core/kernel/src/lib.rs`)" | "ABI v0 states it (`std/kernels-src/kernel/src/lib.rs`)" | S3 |
| Acceptance gates, first paragraph | "A step is done when its gates pass in CI or, for timings, in a recorded run on the reference projects." | "A step is done when its gates pass in `ci/check.sh` on Linux and on the owner's Mac ([PLAT-6](../../architecture.md#platforms)). Timed gates are judged in the Mac's run, which runs the gates `ci/gates.txt` names, in release, on the reference projects." | S1 |
| same | after "Timings are taken on the **reference Mac**, the oldest supported Apple-silicon Mac (an M1), at 48 kHz." | add: "Other machines run the same gates and record their timings with the machine named; only the reference Mac's are judged ([PLAT-1](../../architecture.md#platforms))." | S1 |
| same, after the table | "These gates keep running in CI once their step lands:" | "These gates keep running in `ci/check.sh` once their step lands:" | S1 |

The "reference Mac" stays: macOS remains the judged platform for timings. WRITE-16's rule needs no edit: after S7 every
target of an interrupted commit holds its before or its after bytes, which is what the rule already assumes.

### 6.2 A new section: Platforms (before "Deferred, with triggers")

S1 adds the section. Each rule's *Today* line names the slice that builds it ("not yet: S5") and is updated as the
slices land.

```markdown
## Platforms

**PLAT-1.** music2 MUST build and pass every check on macOS on Apple silicon (the reference platform: judged timings,
the look, native menus) and on Linux x86_64 with the glibc it was built against (the development platform). Windows is
designed for in every OS piece and built in its own step. Timings are judged on the [reference Mac](#acceptance-gates)
only; other machines run the same gates and record their timings with the machine named.
- *Why:* the work moved to a Linux server; the Mac stays where music2 is used and timed.
- *Today:* since the platform step: Linux on the development server, macOS by the owner's run of `ci/check.sh`.

**PLAT-2.** Code that differs between operating systems MUST live in the `m2-os` crate (`core/os`). Outside it, code
other than tests MUST NOT name an OS: no `cfg` on the OS, no `libc`, no `std::os`, no `extern "C"` block, no `$HOME`.
The desktop shell (`apps/music2-desktop/src`) and the gates' fault injection are the exceptions. Tests may use OS-only
fixtures under `cfg`.
- *Why:* one place per OS to read, port and check; later steps write each OS piece once.
- *Today:* since the platform step; `core/os/tests/one_place.rs` checks it.

**PLAT-3.** `m2-os` MUST do what the OS does, with the strongest call the volume accepts (`F_FULLFSYNC`, else `fsync`),
or return `Unsupported` naming what. It never emulates: no copy for a clone, no compare-then-rename for a swap, no walk
for events. Those fallbacks and their rules stay with the caller.
- *Why:* an emulation hidden in the OS layer would change a rule's meaning on one OS without the rule saying so.

**PLAT-4.** A pure offline render MUST give the same bits on every supported OS and CPU. Host code that makes audio for
a render, and song code run by QuickJS, MUST compute every transcendental function with the `libm` crate, the same code
as `std.wasm`'s math. One record of render hashes serves every OS (`core/session/tests/renders.json`,
`e2e/renders.sha256`).
- *Why:* an export must not depend on whose computer made it. Each OS's math library rounds some results differently
  in the last bit (glibc against `libm`: 0.04% to 9.7% of inputs, measured).
- *Today:* since the platform step ([RENDER-2](#offline)).

**PLAT-5.** `std/kernels/std.wasm` MUST rebuild to the same bytes in any folder on any supported OS with the toolchain
`rust-toolchain.toml` pins, and hold no machine path. A commit that changes it says that the kernel hashes changed.
- *Why:* a kernel's hash is part of its identity ([RENDER-25](#plans-data-and-arrangement-swaps)); a rebuild must never
  change it by accident.
- *Today:* since the platform step: `m2-kernel` is a member of the kernels' workspace, and `std/build.sh --check`
  rebuilds it in another folder and compares.

**PLAT-6.** `ci/check.sh` is the full check, the same script on Linux and macOS: the build, every Rust test, clippy
with warnings denied (on Linux also for the macOS target), the desktop app's tests, the web build, every browser suite,
std's tests and rebuild, and the offline renders against `e2e/renders.sha256`; on macOS also the timing gates
`ci/gates.txt` names. A check passes only with the test plugins present ("PASS without plugins" is not a pass). Every
slice passes it on Linux. The owner runs it on a Mac at the end of each step, and before a branch that changes
macOS-only code is merged. Until it passes there, the step's commit message and `HANDOFF.md` say "macOS: unverified
since <commit>".
- *Why:* there are no hosted Mac runners; the owner's Mac is checked by hand, as rarely as is safe.
- *Today:* since the platform step.

**PLAT-7.** Every folder music2 writes outside a project (songs, caches, the shell's `~/.music2`) MUST be under `$HOME`
when `HOME` is set, so a test or a suite isolates itself by setting `HOME`. Outside the desktop shell those folders
come from `m2_os::dirs`. Folders music2 only reads (the system's CLAP folders, files bundled with the app) are exempt.
- *Why:* a test must never write to the user's real folders.
- *Today:* since the platform step; `XDG_*` folders are not read yet ([Deferred](#deferred-with-triggers)).
```

### 6.3 The order of work

Insert after item 6's bullets, before "7. **The engine**" (no renumbering, so "step 7", "8a" and the rest keep their
names):

```markdown
6b. **The platform layer** (about 2 weeks). Development moved to a Linux server, so Linux is made green there first,
   natively, and the Mac stays green. **Built** when its gates pass.
   - `m2-os` (`core/os`), one folder per OS ([PLAT-2](#platforms)): renames and the durable sync, the lock and machine
     checks, file-system events (FSEvents, inotify), the plugin main loop, the folders.
   - The same render bits on every OS through `libm` ([PLAT-4](#platforms)); a reproducible `std.wasm` and a pinned
     toolchain ([PLAT-5](#platforms)); saves that a crash can't break on volumes without an atomic exchange, and new
     audio made durable before a song names it.
   - One check script for both OSes, `ci/check.sh`, run by hand on the owner's Mac, timing gates included
     ([PLAT-6](#platforms)).
   - Steps 8a and 9 add their OS pieces to `m2-os`; Windows is designed for in each piece and built in its own step.
```

### 6.4 The acceptance gates table (S9)

| Step | Gates |
|---|---|
| Platform | `ci/check.sh` says `music2 check: PASS` on Linux, with the test plugins: every Rust test (the desktop app's, the kill test on disks with and without an atomic exchange, and the watcher's folder tests included), clippy `-D warnings` on the whole workspace and for `aarch64-apple-darwin`, the web build, the kernels' tests and clippy, `std.wasm` rebuilt to its bytes in another folder, all 29 browser-suite runs in full, the 8 offline renders equal to `e2e/renders.sha256`, and the repo unchanged by the check. The same script says `PASS` on the owner's Mac at the step's last commit, G11 `tx-data` and `watcher` included. `core/os/tests/one_place.rs` has an empty allow-list. |

### 6.5 Deferred, with triggers (new rows, S9)

| Deferred | Trigger |
|---|---|
| Windows: `core/os/src/windows/`, the Windows twins of every piece in §3, the tests' `cfg(unix)` fixes | The owner asks for Windows users. |
| Linux packaging (deb, AppImage), a Linux candidate in `bundled()`, and the `XDG_*` folders (music, cache) | A Linux release, or someone runs the app on a Linux desktop. |
| Stored spelling on case-folding Linux volumes (vfat, exFAT, casefold folders) | A project on such a volume, or a volume to test on. |
| CLAP's `timer-support` and `posix-fd-support` on Linux | Plugin windows (step 9), or a Linux plugin that needs them headless. |
| `FICLONE` clones on btrfs and XFS | A Linux user on such a volume with large plugin states. |
| An event source that tells the watcher "I can't cover this project" at once (inotify out of watches, the root unmounted), instead of the two-miss rule | A user report of a machine out of inotify watches. |
| An exFAT disk image on the Mac, to run the fallbacks there | A Mac user with a project on an exFAT or network volume reports a problem. |

---

## 7. Verification

### 7.1 One script for both OSes: `ci/check.sh`

The full draft. It is written for bash 3.2, the bash macOS ships (no bash 4 features). Each check logs to its own file,
prints a line when it starts and one when it ends, and the run goes on after a failure, so one run gives the whole
picture. Its control flow was smoke-tested here with stand-in tools (§11): the retries, the summary after Ctrl-C, the
guard on the output folder, the plugin line and the log pack.

```bash
#!/bin/bash
# ci/check.sh: the full check of music2, the same script on macOS and Linux (PLAT-6).
#
# In short: run it in the repo with no arguments. It builds everything, runs every Rust test,
# clippy, the web build, every browser suite, std's checks and the 8 offline renders; on a Mac it
# also times the gates ci/gates.txt names. Then it prints a short summary to paste back. It
# changes nothing in the repo, never opens the app, never plays sound, and never touches your
# ~/Music or ~/.music2. If one line says "running" for over 30 minutes, press Ctrl-C once: it
# stops and prints the summary anyway.
#
#   ci/check.sh             everything (on a Mac, the timing gates too)
#   ci/check.sh --gates     on Linux, the timing gates too (recorded for information, PLAT-1)
#   ci/check.sh --no-gates  on a Mac, without them
#
# Optional settings:
#   CLAP_PATH         a folder holding "Surge XT.clap" and "Airwindows Consolidated.clap". The
#                     default, when both are there: ~/Library/Audio/Plug-Ins/CLAP on a Mac,
#                     ~/.local/share/m2-test-plugins/clap on Linux (ci/plugins.sh fills it).
#                     Without them the summary says "PASS without plugins".
#   CARGO_TARGET_DIR  where cargo builds (default: target/ in the repo)
#   M2_CHECK_OUT      where the logs go (default: ~/m2-check/<commit>). Only a folder an earlier
#                     check made is ever replaced.
# Exit status: 0 when every check passed, 1 when one failed, 2 when it couldn't start.
#
# Needs rustup and Node 24. The first run installs Rust 1.99.0 (rust-toolchain.toml) and
# chrome-headless-shell 133.0.6943.141. On a Mac, run it in Terminal on the Mac itself, not over
# ssh: the app's test needs your login session. Written for bash 3.2, which macOS ships.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) && cd "$ROOT" || exit 2
GATES=
case $(uname -s) in
  Darwin)
    OS=macos
    GATES=1
    MACHINE="$(sysctl -n hw.model), macOS $(sw_vers -productVersion), $(uname -m)"
    CHROME_DIR=mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64
    PLUGINS=$HOME/Library/Audio/Plug-Ins/CLAP ;;
  Linux)
    OS=linux
    MACHINE="$(. /etc/os-release && echo "$PRETTY_NAME"), $(uname -m), $(nproc) threads"
    CHROME_DIR=linux-133.0.6943.141/chrome-headless-shell-linux64
    PLUGINS=$HOME/.local/share/m2-test-plugins/clap ;;
  *)
    echo "ci/check.sh runs on macOS and Linux"
    exit 2 ;;
esac
for a in "$@"; do
  case $a in
    --gates) GATES=1 ;;
    --no-gates) GATES= ;;
    *) echo "ci/check.sh: no option $a (see the top of the script)"; exit 2 ;;
  esac
done
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
SHA=$(git rev-parse --short=12 HEAD) || exit 2
OUT=${M2_CHECK_OUT:-$HOME/m2-check/$SHA}
if [ -e "$OUT" ] && [ ! -e "$OUT/.m2-check" ]; then
  echo "ci/check.sh: $OUT wasn't made by a check, so it is left alone: name another folder in M2_CHECK_OUT"
  exit 2
fi
rm -rf "$OUT" && mkdir -p "$OUT/home" && : > "$OUT/.m2-check" || exit 2
SCR=$(mktemp -d "${TMPDIR:-/tmp}/m2-check.XXXXXX") || exit 2
START=$(date +%s)
T0=$START
RUNNING=
TAB=$(printf '\t')
: > "$OUT/results"
: > "$OUT/failed"
: > "$OUT/flaky"

# Whatever music2 starts gets a scratch HOME (tests, suites, renders, gates); cargo and rustup
# keep yours.
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
export M2_AUDIO=none CARGO_TERM_COLOR=never
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
CHROME=${E2E_CHROME:-$HOME/.cache/puppeteer/chrome-headless-shell/$CHROME_DIR/chrome-headless-shell}
if [ -z "${CLAP_PATH:-}" ] && [ -e "$PLUGINS/Surge XT.clap" ] && [ -e "$PLUGINS/Airwindows Consolidated.clap" ]; then
  CLAP_PATH=$PLUGINS
fi
if [ -n "${CLAP_PATH:-}" ]; then export CLAP_PATH; else unset CLAP_PATH; fi
# A Mac stays awake while this runs: idle sleep breaks timed tests.
if [ $OS = macos ]; then caffeinate -i -w $$ & fi

# begin <check> <usual time>: say a check started, so "slow" and "stuck" can be told apart.
begin() {
  RUNNING=$1
  T0=$(date +%s)
  printf '%-16s ...   running, usually %s (started %s)\n' "$1" "$2" "$(date +%H:%M)"
}
# record <check> <PASS|PART|FAIL> <note>: one row of the summary.
record() {
  RUNNING=
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$OUT/results"
  printf '%-16s %-4s  %s\n' "$1" "$2" "$3"
}
# quiet <log> <command...>: run it, with its output in $OUT/<log>.
quiet() {
  log=$OUT/$1
  shift
  "$@" > "$log" 2>&1 < /dev/null
}
# first <file> <pattern>: the first line that matches, cut to 160 characters.
first() { grep -m1 -E "$2" "$1" 2> /dev/null | cut -c1-160; }
# panicked <test>: where the test panicked, and the first line of why.
panicked() {
  grep -A1 "^thread '$1' panicked at " "$OUT/test.log" | head -2 | tr '\n' ' ' | sed "s/^thread '[^']*' panicked at //; s/ *$//" | cut -c1-160
}
# stuck: the tests libtest called slow that never finished, when a run was stopped.
stuck() {
  grep -E '^test .* has been running for over 60 seconds' "$OUT/test.log" 2> /dev/null | awk '{print $2}' | sort -u |
    while read -r t; do
      grep -q "^test $t \.\.\. " "$OUT/test.log" || echo "test $t (still running when stopped)"
    done
}
# suites <run.mjs arguments>: the browser suites, each on a fresh headless server.
suites() { env E2E_CHROME="$CHROME" M2_HEADLESS="$TARGET/debug/examples/headless" node e2e/run.mjs "$@"; }
# version <plugin>: what a Mac plugin bundle says its version is.
version() { /usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$CLAP_PATH/$1.clap/Contents/Info.plist" 2> /dev/null || echo '?'; }
# plugins: the folder the plugin checks used, and which builds are in it.
plugins() {
  if [ -z "${CLAP_PATH:-}" ]; then
    echo none
  elif [ $OS = macos ]; then
    echo "$CLAP_PATH (Surge XT $(version 'Surge XT'), Airwindows Consolidated $(version 'Airwindows Consolidated'))"
  elif (cd "$CLAP_PATH" && grep '\.clap$' "$ROOT/ci/plugins.sha256" | sha256sum -c --quiet) > /dev/null 2>&1; then
    echo "$CLAP_PATH (the builds ci/plugins.sha256 records)"
  else
    echo "$CLAP_PATH (NOT the builds ci/plugins.sha256 records)"
  fi
}

# The summary to paste back, and every log packed into one small file beside the logs.
summary() {
  fails=$(awk -F"$TAB" '$2 == "FAIL"' "$OUT/results" | wc -l | tr -d ' ')
  total=$(wc -l < "$OUT/results" | tr -d ' ')
  mins=$(( ($(date +%s) - START + 59) / 60 ))
  if [ -s "$SCR/before" ]; then state="with local changes"; else state=clean; fi
  if [ "$fails" -gt 0 ]; then
    verdict="FAIL ($fails of $total checks)"
  elif [ -z "${CLAP_PATH:-}" ]; then
    verdict="PASS without plugins"
  else
    verdict=PASS
  fi
  if [ -s "$OUT/flaky" ]; then verdict="$verdict; $(grep -c '' "$OUT/flaky") passed only on a retry"; fi
  # Logs only: no renders, no kept projects (a failed fx-fixes keeps a file of over 300 MB).
  (cd "$OUT" && tar -czf "$OUT.tgz" $(find . -maxdepth 2 -type f \( -name '*.log' -o -name '*.sum' -o -name results -o -name failed -o -name flaky \))) 2> /dev/null
  {
    echo
    echo "music2 check: $verdict"
    echo "commit   $SHA on $(git rev-parse --abbrev-ref HEAD) ($state)"
    echo "machine  $MACHINE"
    echo "tools    $(rustc -V 2> /dev/null | cut -d' ' -f1-2), node $(node -v 2> /dev/null), $mins min"
    echo "plugins  $(plugins)"
    echo
    awk -F"$TAB" '{printf "%-16s %-4s  %s\n", $1, $2, $3}' "$OUT/results"
    if [ -s "$OUT/gates.sum" ]; then echo; echo "gates:"; sed 's/^/  /' "$OUT/gates.sum"; fi
    if [ -s "$OUT/flaky" ]; then echo; echo "passed only on a retry:"; sed 's/^/  /' "$OUT/flaky"; fi
    if [ -s "$OUT/failed" ]; then echo; echo "what failed:"; head -40 "$OUT/failed" | sed 's/^/  /'; fi
    echo
    echo "logs     $OUT.tgz"
  } | tee "$OUT/summary.txt"
}

# Ctrl-C: the check that was running is stuck; say so, and still print the summary.
stopped() {
  trap '' INT TERM
  if [ -n "$RUNNING" ]; then
    record "$RUNNING" FAIL "stopped after $(( ($(date +%s) - T0) / 60 )) min (stuck?)"
    stuck >> "$OUT/failed"
  fi
  summary
  exit 1
}
trap 'rm -rf "$SCR"' EXIT
trap stopped INT TERM

echo "music2 check of $SHA on $MACHINE (logs in $OUT)"
git status --porcelain > "$SCR/before"

# 0. Plugins in a folder music2 always searches make fx-fixes fail, so the run could never pass:
#    stop at once, before 45 minutes are spent, and say what to do.
for d in /Library/Audio/Plug-Ins/CLAP /usr/lib/clap; do
  if [ -e "$d/Surge XT.clap" ] || [ -e "$d/Airwindows Consolidated.clap" ]; then
    echo "ci/check.sh: $d holds Surge XT or Airwindows, and one browser suite needs them out of it."
    echo "Move them to your own folder (~/Library/Audio/Plug-Ins/CLAP on a Mac), then run ci/check.sh again."
    exit 2
  fi
done

# 1. The pinned toolchain, installed if missing (rustup 1.28 or later reads rust-toolchain.toml).
begin toolchain "1 min"
quiet toolchain.log sh -c 'rustup toolchain install || rustup show active-toolchain; rustc -V'
if grep -q '^rustc 1\.99\.0 ' "$OUT/toolchain.log"; then
  record toolchain PASS "rustc 1.99.0"
else
  record toolchain FAIL "not Rust 1.99.0: see toolchain.log"
fi

# 2. The web UI: the desktop app embeds it, and the browser suites load it.
begin "web build" "1 min"
if quiet web.log sh -c 'cd web && npm ci --no-audit --no-fund && npm run build'; then
  record "web build" PASS ""
else
  record "web build" FAIL "see web.log"
fi

# 3. Everything: the desktop app, m2, the headless server and every test binary.
begin build "5 min; 15 the first time"
if quiet build.log cargo build --locked --workspace --all-targets; then
  record build PASS ""
else
  record build FAIL "see build.log"
  first "$OUT/build.log" '^error' | sed 's/^/build: /' >> "$OUT/failed"
fi

# 4. Every Rust test, the desktop app's included, with a scratch HOME. A test binary that failed
#    runs once more alone; if every one passes then, the run passes and names them as retried.
begin "rust tests" "10 min"
quiet test.log env HOME="$OUT/home" cargo test --locked --workspace --no-fail-fast
st=$?
counts=$(grep -E '^test result:' "$OUT/test.log" | awk '{p += $4; f += $6} END {printf "%d passed, %d failed", p, f}')
grep -E '^    [A-Za-z0-9_:]+$' "$OUT/test.log" | sed 's/^ *//' | sort -u > "$SCR/tests"
grep -o 'to rerun pass `[^`]*`' "$OUT/test.log" | sed 's/^to rerun pass `//; s/`$//' > "$SCR/binaries"
again=1
if [ $st -ne 0 ] && [ -s "$SCR/binaries" ]; then
  again=0
  while read -r args; do
    # cargo's own "-p <crate> --lib" or "--test <name>": split into words on purpose.
    env HOME="$OUT/home" cargo test --locked --no-fail-fast $args >> "$OUT/test-retry.log" 2>&1 < /dev/null || again=1
  done < "$SCR/binaries"
fi
if [ $st -eq 0 ]; then
  record "rust tests" PASS "$counts"
elif [ $again -eq 0 ]; then
  record "rust tests" PASS "$counts; passed on a retry"
  if [ -s "$SCR/tests" ]; then sed 's/^/test /' "$SCR/tests"; else sed 's/^/test binary /' "$SCR/binaries"; fi >> "$OUT/flaky"
else
  record "rust tests" FAIL "$counts: see test.log"
  while read -r t; do
    echo "test $t"
    r=$(panicked "$t")
    if [ -n "$r" ]; then echo "  $r"; fi
  done < "$SCR/tests" >> "$OUT/failed"
  sed 's/^/test binary /' "$SCR/binaries" >> "$OUT/failed"
  if [ ! -s "$SCR/binaries" ]; then first "$OUT/test.log" '^error' | sed 's/^/tests: /' >> "$OUT/failed"; fi
fi

# 5. Clippy, every warning an error.
begin clippy "2 min"
if quiet clippy.log cargo clippy --locked --workspace --all-targets -- -D warnings; then
  record clippy PASS ""
else
  record clippy FAIL "$(grep -cE '^(warning|error)' "$OUT/clippy.log") findings: see clippy.log"
  first "$OUT/clippy.log" '^ *--> ' | sed 's/^ *--> /clippy: /' >> "$OUT/failed"
fi

# 6. std: the kernels' own workspace (tests and lints), and std.wasm rebuilt to the committed
#    bytes in another folder (PLAT-5).
begin "std kernels" "2 min"
if quiet kernels.log sh -c 'cd std/kernels-src && cargo test --locked --release --workspace && cargo clippy --locked --workspace --all-targets -- -D warnings'; then
  record "std kernels" PASS "tests and clippy"
else
  record "std kernels" FAIL "see kernels.log"
fi
begin std.wasm "2 min"
if quiet std-wasm.log std/build.sh --check; then
  record std.wasm PASS "rebuilt to the same bytes in another folder"
else
  record std.wasm FAIL "$(tail -1 "$OUT/std-wasm.log" | cut -c1-100)"
fi

# 7. Linux only: the macOS-only code, type-checked and linted for aarch64-apple-darwin. There is
#    no macOS SDK here, so ci/stub-cc and ci/stub-ar stand in for C; clippy never links.
if [ $OS = linux ]; then
  begin "mac lint" "1 min; 3 the first time"
  quiet mac-target.log rustup target add aarch64-apple-darwin
  if quiet mac-lint.log env CC_aarch64_apple_darwin="$ROOT/ci/stub-cc" CXX_aarch64_apple_darwin="$ROOT/ci/stub-cc" \
      AR_aarch64_apple_darwin="$ROOT/ci/stub-ar" \
      cargo clippy --locked --target aarch64-apple-darwin --workspace --all-targets -- -D warnings; then
    record "mac lint" PASS "the macOS code type-checks and lints"
  else
    record "mac lint" FAIL "see mac-lint.log"
    first "$OUT/mac-lint.log" '^ *--> ' | sed 's/^ *--> /mac lint: /' >> "$OUT/failed"
  fi
fi

# 8. Every browser suite, each on a fresh headless server with a scratch HOME (e2e/run.mjs). A run
#    that failed runs once more alone; if it passes then, it is named as retried.
begin "browser suites" "8 min"
quiet e2e-npm.log sh -c 'cd e2e && npm ci --no-audit --no-fund'
if [ ! -x "$CHROME" ]; then
  (cd e2e && node_modules/.bin/browsers install chrome-headless-shell@133.0.6943.141 --path "$HOME/.cache/puppeteer") \
    >> "$OUT/e2e-npm.log" 2>&1
fi
quiet e2e.log suites all --out "$OUT/e2e"
st=$?
line=$(grep -E 'suites ok' "$OUT/e2e.log" | tail -1 | cut -d';' -f1)
parts=$(grep -c '^  PART:' "$OUT/e2e.log")
others=$(grep '^  PART:' "$OUT/e2e.log" | grep -vc 'live3b-proj')
grep '^  failed: ' "$OUT/e2e.log" | awk '{print $2, $NF}' > "$SCR/runs"
again=0
while read -r run log; do
  if suites "${run%-*}" "${run##*-}" --out "$OUT/e2e-retry" >> "$OUT/e2e-retry.log" 2>&1 < /dev/null; then
    echo "suite $run" >> "$OUT/flaky"
  else
    again=1
    { echo "suite $run"; echo "  $(first "$log" 'FAIL')"; } >> "$OUT/failed"
  fi
done < "$SCR/runs"
if [ $st -ne 0 ] && [ $st -ne 1 ] && [ $st -ne 3 ]; then
  record "browser suites" FAIL "run.mjs stopped (exit $st): see e2e.log"
elif [ $again -ne 0 ] || [ "$others" -gt 0 ] || { [ "$parts" -gt 0 ] && [ -n "${CLAP_PATH:-}" ]; }; then
  record "browser suites" FAIL "$line: see e2e.log"
  grep '^  PART:' "$OUT/e2e.log" | sed 's/^  PART: */suite /' | cut -c1-160 >> "$OUT/failed"
elif [ "$parts" -gt 0 ]; then
  record "browser suites" PART "$line (live3b without plugins)"
elif [ -s "$SCR/runs" ]; then
  record "browser suites" PASS "$line; passed on a retry"
else
  record "browser suites" PASS "$line"
fi

# 9. The offline renders (8 today) with the debug m2 from step 3 (it renders the release m2's bits),
#    against the one record every OS shares (PLAT-4).
begin renders "1 min"
want=$(grep -c . "$ROOT/e2e/renders.sha256")
if quiet renders.log env M2="$TARGET/debug/m2" e2e/baseline.sh --tree . "$OUT/renders" &&
   (cd "$OUT/renders" && shasum -a 256 -c "$ROOT/e2e/renders.sha256") > "$OUT/renders-check.log" 2>&1; then
  record renders PASS "$want of $want match e2e/renders.sha256"
else
  n=$(grep -c ': OK$' "$OUT/renders-check.log" 2> /dev/null)
  record renders FAIL "${n:-0} of $want match: see renders-check.log"
  grep -v ': OK$' "$OUT/renders-check.log" 2> /dev/null | sed 's/^/render /' >> "$OUT/failed"
fi

# 10. The timing gates ci/gates.txt names, in release: on a Mac by default, on Linux with --gates.
#     Only the reference Mac's timings are judged; elsewhere they are recorded (PLAT-1).
if [ -n "$GATES" ]; then
  begin gates "15 min"
  args=$(grep -v '^#' ci/gates.txt | head -1)
  # ci/gates.txt's line of m2-gates options: split into words on purpose.
  if quiet gates-build.log cargo build --locked --release -p m2-gates &&
     quiet gates.log env HOME="$OUT/home" "$TARGET/release/m2-gates" $args; then
    record gates PASS "$args"
  else
    record gates FAIL "$args: see gates.log"
    first "$OUT/gates-build.log" '^error' | sed 's/^/gates: /' >> "$OUT/failed"
  fi
  grep -E '^(PASS|FAIL|SKIP)  |^      FAIL  ' "$OUT/gates.log" 2> /dev/null | cut -c1-160 > "$OUT/gates.sum"
fi

# 11. The check changed nothing in the repo.
begin "repo unchanged" "seconds"
git status --porcelain > "$SCR/after"
if cmp -s "$SCR/before" "$SCR/after"; then
  record "repo unchanged" PASS ""
else
  record "repo unchanged" FAIL "$(diff "$SCR/before" "$SCR/after" | grep -c '^[<>]') entries changed: run git status"
fi

trap '' INT TERM
summary
[ "$fails" -eq 0 ]
```

What the owner pastes back looks like this (made-up numbers):

```
music2 check: PASS
commit   1a2b3c4d5e6f on main (clean)
machine  MacBookPro18,1, macOS 26.6.2, arm64
tools    rustc 1.99.0, node v24.19.0, 44 min
plugins  /Users/…/Library/Audio/Plug-Ins/CLAP (Surge XT 1.3.4, Airwindows Consolidated 0.9.2)

toolchain        PASS  rustc 1.99.0
web build        PASS
build            PASS
rust tests       PASS  748 passed, 0 failed
clippy           PASS
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
browser suites   PASS  29/29 suites ok
renders          PASS  8 of 8 match e2e/renders.sha256
gates            PASS  --server tx-data watcher
repo unchanged   PASS

gates:
  PASS  G11 tx-data (PLANE-5) on server
  PASS  G11 watcher (WRITE-19) on server

logs     /Users/…/m2-check/1a2b3c4d5e6f.tgz
```

What the script does not run: the two `#[ignore]` release tests (both passed on Linux at `f126d03`; run them when a
step's gates ask), and the desktop app itself (never launched).

### 7.2 The two stubs for the macOS lint (`ci/stub-cc`, `ci/stub-ar`)

The versions measured by `design-shaped` (`$SCRATCH/logs/dsh-stub-cc.txt`, `dsh-stub-ar.txt`). They answer both
compiler families' output flags, need no clang, and are only ever set for `aarch64-apple-darwin`, never for the host.

```sh
#!/bin/sh
# ci/stub-cc: a C compiler for compile checks only. It writes an empty object wherever it is asked
# to and succeeds. `cargo check` and `cargo clippy` never link, so C that a build script compiles
# for macOS is never used. Never use it to build.
out=
prev=
for a in "$@"; do
  case "$prev" in -o) out=$a ;; esac
  case "$a" in -Fo*) out=${a#-Fo} ;; /Fo*) out=${a#/Fo} ;; esac
  prev=$a
done
[ -n "$out" ] && : > "$out"
exit 0
```

```sh
#!/bin/sh
# ci/stub-ar: an archiver for compile checks only: an empty archive (the 8-byte ar header) at the
# first argument that ends in .a or .lib.
for a in "$@"; do
  case "$a" in *.a|*.lib|-out:*|/OUT:*) f=${a#-out:}; f=${f#/OUT:}; printf '!<arch>\n' > "$f"; exit 0 ;; esac
done
exit 0
```

### 7.3 The other files the check needs

```toml
# rust-toolchain.toml (S1)
[toolchain]
channel = "1.99.0"
components = ["clippy", "rustfmt"]
targets = ["wasm32-unknown-unknown"]
profile = "minimal"
```

The macOS target stays out of this file; `ci/check.sh` adds it on Linux only.

```
# ci/gates.txt (S1): the m2-gates options the Mac run times (one line; # starts a comment).
# The platform step: G11's durable commit and the watcher, whose code this step changes.
--server tx-data watcher
```

The lead sets this line per step: step 7 puts design-7 §12's gates here before its Mac run.

```sh
#!/bin/sh
# e2e/chrome-as-mac.sh (S2, mode 755): chrome-headless-shell telling pages it runs on a Mac. The code
# editor (Monaco) takes its keys from the user agent, and the suites test the Mac's keys (Q2).
exec "${E2E_CHROME_BIN:?chrome.mjs sets it}" \
  --user-agent="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/133.0.0.0 Safari/537.36" \
  "$@"
```

```sh
#!/bin/sh
# std/build.sh (S1): rebuild std/kernels/std.wasm from std/kernels-src (PLAT-5).
#   std/build.sh           rebuild it in place
#   std/build.sh --check   rebuild a copy in a temporary folder and compare it with the committed file
# The bytes depend only on the sources and the toolchain rust-toolchain.toml pins: m2-kernel is a
# member of the kernels' workspace (from S3), so no absolute path reaches the build.
set -e
STD=$(cd "$(dirname "$0")" && pwd)
build() { # <std folder> <target folder>: build, then print the wasm's path
  (cd "$1/kernels-src" && CARGO_TARGET_DIR=$2 cargo build --locked --release --target wasm32-unknown-unknown) >&2
  echo "$2/wasm32-unknown-unknown/release/m2_std_kernels.wasm"
}
if [ "${1:-}" = --check ]; then
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/m2-std.XXXXXX")
  trap 'rm -rf "$tmp"' EXIT
  # std/ and the pinned toolchain's file, without a local target folder (BSD and GNU tar both take --exclude)
  (cd "$STD/.." && tar --exclude std/kernels-src/target -cf - std rust-toolchain.toml) | (cd "$tmp" && tar -xf -)
  wasm=$(build "$tmp/std" "$tmp/target")
  if grep -q -e /Users/ -e /home/ "$wasm"; then echo "std.wasm: the rebuild holds a machine path"; exit 1; fi
  if ! cmp "$wasm" "$STD/kernels/std.wasm"; then echo "std.wasm: a rebuild differs from the committed file"; exit 1; fi
  echo "std.wasm: the same bytes, rebuilt in $tmp"
  exit 0
fi
cp "$(build "$STD" "${CARGO_TARGET_DIR:-$STD/kernels-src/target}")" "$STD/kernels/std.wasm"
ls -l "$STD/kernels/std.wasm"
```

Before S3, `--check` fails (its copy of `std/` lacks `core/kernel`, and the committed file is stale): the `std.wasm`
row says FAIL until S3, as §13.1 expects, and the repo is never touched.

```sh
#!/bin/sh
# ci/plugins.sh (Linux): the CLAP plugins the with-plugins checks use, Surge XT 1.3.4 and
# Airwindows Consolidated, unpacked into ~/.local/share/m2-test-plugins/clap, where ci/check.sh
# looks. No sudo. Never into ~/.clap or /usr/lib/clap: fx-fixes needs them empty. The archives
# stay in dl/ beside it: Airwindows' nightly zip is deleted upstream once a newer one comes, so
# that copy may be the only one. Every file is checked against ci/plugins.sha256.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SUMS=$ROOT/ci/plugins.sha256
P=${M2_TEST_PLUGINS:-$HOME/.local/share/m2-test-plugins}
SURGE=surge-xt-linux-x64-1.3.4.deb
AW=$(grep -o 'AirwindowsConsolidated-.*-Linux\.zip' "$SUMS")
mkdir -p "$P/dl" "$P/clap"
cd "$P/dl"
[ -f "$SURGE" ] || curl -fsSLO "https://github.com/surge-synthesizer/releases-xt/releases/download/1.3.4/$SURGE"
[ -f "$AW" ] || curl -fsSLO "https://github.com/baconpaul/airwin2rack/releases/download/DAWPlugin/$AW"
grep -e "  $SURGE\$" -e "  $AW\$" "$SUMS" | sha256sum -c --quiet
rm -rf unpacked "$P/SurgeXTData" && mkdir unpacked
dpkg-deb -x "$SURGE" unpacked/surge
python3 -I -m zipfile -e "$AW" unpacked/aw
cp "unpacked/surge/usr/lib/clap/Surge XT.clap" "$P/clap/"
cp -R unpacked/surge/usr/share/surge-xt "$P/SurgeXTData"   # Surge finds its data one folder up
find unpacked/aw -name 'Airwindows Consolidated.clap' -exec cp -R {} "$P/clap/" \;
rm -rf unpacked
(cd "$P/clap" && grep '\.clap$' "$SUMS" | sha256sum -c --quiet)
echo "plugins: $P/clap"
```

`ci/plugins.sha256` (S1): the archives and the plugins they unpack to.

```
a6e55064487f624147d515b9ae5fc79a568b69746675b2083abde628ca7bb151  surge-xt-linux-x64-1.3.4.deb
9144dc8b412179f862a6dd5d72e474d606d38376c2a404efefa2c5134174f12c  AirwindowsConsolidated-2026-10-11-8a67b80-Linux.zip
c923a148e888048021d5cb5ba866bb94927a17e8136547882a9b2efdc7118f20  Surge XT.clap
9bd1901122b4677b2797c43887a702d20ec722ade1d21a7bf38dcb0dc6b74c4d  Airwindows Consolidated.clap
```

`ci/plugins.sh` was run here offline from the kept archives into a scratch folder: both `.clap` files matched these
hashes (§11). In S1 the lead seeds `~/.local/share/m2-test-plugins/dl/` from `$SCRATCH/plugins/dl/` (about 260 MB)
before the first run, so the Airwindows zip is kept outside `$SCRATCH`, which gets cleaned. Until then that zip exists
only in `$SCRATCH/plugins/dl/` (checked at revision 2's critic pass), so no clean-up of `$SCRATCH` may run before it
is copied.

Also needed: `e2e/renders.sha256` and the eighth case in `e2e/baseline.sh` (S1), and `run.mjs` taking its default
Chrome from `chrome.mjs` (S2). `ci/check.sh`, `ci/stub-cc`, `ci/stub-ar`, `ci/plugins.sh`, `std/build.sh` and
`e2e/chrome-as-mac.sh` are committed executable (mode 755).

### 7.4 On this server

```sh
cd <repo>
$SCRATCH/tw check-<label> 'CARGO_TARGET_DIR=$SCRATCH/target-<label> M2_CHECK_OUT=$SCRATCH/check-<label> ci/check.sh'
# tw waits 9.5 minutes; a full check may take longer, so then: $SCRATCH/tw-wait <channel> <log> (tw prints it)
```

- The command has no double quotes or parentheses, so `tw`'s echo line stays intact (`ground/tests.md`).
- The plugins come from `~/.local/share/m2-test-plugins/clap` by themselves (`ci/plugins.sh`, once). Nothing
  server-specific is in the repo.
- `M2_CHECK_OUT` must be a folder of its own: the script refuses a folder an earlier check didn't make.
- The summary is in `$M2_CHECK_OUT/summary.txt`, every check's log beside it, and all logs in `$M2_CHECK_OUT.tgz`. It
  replaces `$SCRATCH/check.sh`.
- `--gates` adds the timing gates, recorded for information (PLAT-1).
- Delete the target folder after the workflow (20 to 60 GB).
- Time here: revision 1's script took **8 min 11 s** with a warm target folder on an idle server
  (`attack-verification`, `av-check1-*.log`). The kill test on both disks adds about 100 s *(estimate)*.

### 7.5 The Mac check by hand

- The owner runs `ci/check.sh` from the checkout, at the commit the lead names (§1's six steps). On macOS it also
  builds `m2-gates` in release and runs the gates in `ci/gates.txt`.
- **The Mac is green at a commit** when the summary's first line says `music2 check: PASS`. Every row is PASS:
  toolchain, web build, build, Rust tests (the app test and `main_thread.rs` among them), clippy, std kernels,
  std.wasm, browser suites, renders, gates, repo unchanged.
- **Not green:** `PASS without plugins` (the moved run-loop pump is only exercised by real plugins; the probe plugin
  never posts to the run loop), or a FAIL row. A row that says `passed on a retry` is listed under "passed only on a
  retry"; the lead decides whether it is a known flake (HANDOFF's s5-knobs, for one) or a real failure, and says which
  in `HANDOFF.md`.
- The same `e2e/renders.sha256` must pass on both OSes: that is the proof of PLAT-4 on the Mac.
- **After a green run** the lead writes in `HANDOFF.md`: "macOS: green at <commit> (ci/check.sh on <model line>,
  <date>)", with the gates' numbers recorded beside the M1's, the machine named.
- **Until then**, the step's last commit message carries
  `macOS: unverified since <first commit of this step> (last verified at 8aa301e; compile-checked from Linux).`
  and `HANDOFF.md`'s first table gains a row
  `| Platforms | Linux green at <commit> (ci/check.sh). macOS unverified since <commit>. Windows: designed only. |`
- **If a row fails:** "what failed" names each failing test or suite with its first failing line. The lead fixes it on
  Linux, checks the macOS lint, and asks for one more run. Only if that is not enough does the lead ask for the logs,
  which are already packed in one file, `~/m2-check/<commit>.tgz`, with one copy command to paste.
- **If the run was stopped** (Ctrl-C after a line said `running` for over 30 minutes), the summary names the check
  that was stuck and the tests libtest called slow that never finished.

### 7.6 When to ask the owner for a Mac run

As few as possible:

| When | Why | Ask? |
|---|---|---|
| The end of this step (after S9) | the moved Mac code, the macOS barrier's fallback, the new render hashes, the rebuilt `std.wasm`, the Chrome wrapper on a Mac, the two G11 gates | **yes, required** |
| The end of step 7, before it lands on `main` | its timed gates (design-7 §12, in `ci/gates.txt`) are judged on the Mac, and its merge moves `store.rs`'s Mach memory call and QoS into `m2-os` | yes |
| Before `taste` lands on `main` | it rewrites the Mac menus and the window's title bar | yes |
| Later: the end of every step, and before merging any branch that changes `core/os/src/mac/`, the shell's macOS code, `tauri.conf.json`'s macOS part or the app test | PLAT-6 | yes |
| A change of recorded render hashes or of `std.wasm` (the `sounds` merge, a std version bump), a toolchain bump, new versions of tauri, cpal, wasmtime or rquickjs | their effect on the Mac is covered by the next run a step needs anyway | fold into the next run |
| Linux-only work, or code that is the same on every OS | the Linux check and the macOS lint cover it; PLAT-4 makes renders shared | no |

Optional: if the owner happens to be at the Mac after S8, an early run would catch a mistake in the moved Mac code
sooner. Not required.

### 7.7 Timings

- Timed gates are judged on the reference Mac, in the owner's run of `ci/check.sh`, which runs `ci/gates.txt`'s gates
  in release. This step's list is `--server tx-data watcher`: OS-81 changes the gates' own code in S6 and S8, S8 changes
  how the FSEvents source reports events, and S6 changes the macOS barrier on failure. G11 `tx-data` (PLANE-5, p95
  under 20 ms) and G11 `watcher` (idle CPU under 1% of a core) must pass there.
- The owner's Mac is an M1 Pro, not a base M1; its numbers are recorded with the machine named, as 8a's gate 8 already
  is ("provisional until a base M1 exists").
- Linux timings (G3, G11, the watcher's CPU) are for information (PLAT-1), with `ci/check.sh --gates`. This server's
  disk has power-loss protection and no sound card, so its numbers are a best case and can't stand in for a laptop.

### 7.8 What changes on the Mac, on purpose

The step's last commit message lists these, so the owner's run is read against them:

| Change | Where | On the owner's Mac |
|---|---|---|
| `examples/resample` renders other bits, once | `audio.rs`, `resample.rs` (S4) | three recorded hashes change (§5.3) |
| `std.wasm` rebuilt from the kernels' workspace | S3 | every kernel's module hash changes once; no render changes |
| The durable sync falls back to `fsync` when a volume refuses `F_FULLFSYNC` | `mac/mod.rs` (S6) | saves work on such volumes instead of failing every time; APFS unchanged |
| A save without an atomic swap renames over the file in one step | `stage.rs` (S7, every OS) | exFAT sticks and network shares never show a missing file; APFS unchanged |
| New assets are pushed before they are named | `render.rs`, `audio.rs`, `m2`, `project.rs` (S7) | one more `fsync` per export or bounce |
| A walk a Rescan asked for counts no missed events | `graph/events.rs`, `watcher.rs` (S8) | a checkout of more than 4,096 files no longer moves a project to walks |

---

## 8. How step 7's merge, 8a, 8b, step 9 and Windows plug in

Each adds files or items in `m2-os`; none moves code that this step placed. Nothing here designs those steps.

| Step | Adds to `m2-os` (macOS and Linux, in its own slices) | Stays outside |
|---|---|---|
| step 7's finish | First, bug 1 in `core/play` (sort cpal's errors: an xrun is counted and play goes on, a device change makes a new player, only "invalidated" or "unavailable" stalls; the error callback only bumps counters, since on ALSA it runs on the audio thread): step 7's export `Hold` needs a player one xrun doesn't retire. Then, at the merge, a new `process` module: `memory() -> Option<Memory { footprint, mapped, address_space }>` (macOS `task_info(TASK_VM_INFO)`, moved from `store.rs`; Linux `/proc/self/status` and the CPU's address bits), `machine() -> String` (model, OS, CPU); and `thread::interactive()` (macOS QoS user-interactive; Linux nothing allowed here, reported). `ci/gates.txt` names design-7 §12's gates for the Mac run. | G7f's rules and records (gates); bug 1 in `core/play`. For G7f on Linux (`facts.md` §8): wasmtime's reservations run out of the 48-bit address space at about 5 typical step-7 renderers per process here; any `ulimit -v` stops them almost at once (3 at 16 GB); a distro that keeps the upstream `vm.max_map_count` (65,530) stops them near the same point. |
| 8a | a new `rt` module (`mac/rt.rs`, `linux/rt.rs`): `Group`, `on_device_thread`, `engine_thread`, `end_engine_thread`, `background_thread`, `cores`, `Wake` (`signal`, `wait`). Design-8a's `core/play/src/rt.rs` becomes `mac/rt.rs`. Linux: `SCHED_FIFO` directly or through rtkit, `RLIMIT_RTTIME` set by us at 200 ms with `SIGXCPU` ignored, `sched_yield` in spins, cores from sysfs within the affinity mask and cgroup quota (one CCD here). Refusals are reported, never fatal (this server grants none). `process::machine()` gains topology. | `fpenv` (FPCR, MXCSR) per CPU in `m2-runtime`; OS-61's silent player in `core/play`; a real device on this server needs `snd-aloop` or a PipeWire null sink (a system change: the owner's OK first) |
| 8b | as built by `next`: `m2_os::midi` (one file, `core/os/src/midi.rs`) wraps `midir`, which already hides CoreMIDI and ALSA, like cpal (its plan's Q1; before, this row kept `midir` outside the layer); design-8b §19's "CoreMIDI note input" reads "midir note input" (S9 edits the word). An input that won't open means "no MIDI devices", never an error: on this server `/dev/snd/seq` is `root:audio 0660` and opening it gives `EACCES` (measured by `attack-simplicity`). Tests inject notes at 8b.6's source level. | the key table (`menu.rs`'s `ITEMS` and one label helper in `native.ts`), ⌃ chords on macOS only, a menu per OS (GTK: Quit in File, About in Help, no Services or Hide), the reveal label, a PC-keys browser suite: all on `taste` |
| 9 | a new `child` module (the host process: `spawn_host`, `Shared`, `SharedWake`, `Child`); `main_loop` gains `add_fd`, `add_timer` and `WINDOW_API` (macOS CFRunLoop sources; Linux `epoll` over an eventfd, a timerfd, the plugins' fds and the X connection, which also replaces this step's Linux job queue; CLAP's `timer-support` and `posix-fd-support` offered from it, closing OS-27 and OS-31); `dirs::vst3`; `fs::will_need`; `thread::io_priority`; `process::keep_awake`, `process::input_access`; a small `time` module for `midi_instant`, when sub-block placement needs it | midir, memmap2, cpal input |
| Windows | `src/windows/` with every module, and one `cfg_attr` line in `lib.rs`; the lock one byte past the end; `sync_dir` answers `Ok` (write-through renames); `Stamp::of` from std's stable `Metadata`; NTFS's swap fallback (`MoveFileExW` with `REPLACE_EXISTING`) with retries; a message-pumping loop thread for `AppMain`. No caller changes. | the tests' `cfg(unix)` fixes; the build (MSVC, QuickJS under `cl.exe`, `llvm-rc`) |

**Windows starting list, measured by `design-shaped`:** with the stubs, `cargo clippy --target x86_64-pc-windows-msvc`
for the workspace without the desktop app compiles except 15 errors in 7 files, all Unix-only APIs (OS-68):
`crates/gates/src/server.rs` 1019, 1025, 1028, 1033; `crates/gates/src/server/watch.rs` 26;
`core/session/src/tx/hook_tests.rs` 5, 394; `kill_tests.rs` 9, 159; `tx/tests.rs` 660, 661, 842;
`crates/server/src/export.rs` 368; `crates/server/src/graph/preview_tests.rs` 5, 173, 180. The desktop app's build
script needs `llvm-rc`. After this step the `watch.rs` line is gone (OS-80) and every OS call sits in `m2-os`.

---

## 9. Attack response

Five attacks read revision 1. Each problem was checked against the code, the ground notes and the design before it was
taken in. "Accepted" means the fix is in this revision as proposed or close to it; "in part" names what was left out.

| # | Problem | Lens | Severity | Accepted or rejected | What changed, or why not |
|---|---|---|---|---|---|
| 1 | A save stopped halfway on a volume without an atomic swap can't be recovered as S7 designed it: Finish fails, Discard deletes the file, a half-saved `music2.toml` makes the project unopenable | durability | high | **Accepted** | Checked: `journal.rs:466/490`, `tx/mod.rs:1047`, `session.rs:233-250` (manifest check before the lock), `tx.rs` (the manifest is a Tx path). The move-aside step goes: compare, then one rename over the target; the rollback renames the before bytes back the same way (§3.1). Recovery needs nothing new; WRITE-16's rule edit is dropped. S7 shrinks to the new fallback, the kill test on both disks, and a case writing `music2.toml`. |
| 2 | Bounced and exported audio (and a new song's files) are never synced, but the Tx that places the audio is | durability | medium | **Accepted** | Checked: no `fsync` in `write_new_wav`, the provenance file, `write_as`/`name_new` or `create_at`. Four places push before naming and sync the folder (§3.1, S7); one sentence in WRITE-1. |
| 3 | The open-time tidy removes a killed commit's journal after a barrier that, on Linux, makes none of its renames durable | durability | low | **Accepted in part** | The scan pushes a completed journal's target folders before its barrier (§3.1, S7; WRITE-15's *Today*). **Rejected:** keeping a journal whose folder push failed until a later push works. A folder `fsync` fails only on a failing disk or a volume that can't sync folders, where nothing is durable anyway; the tracking would add state to every commit. |
| 4 | Every write wakes the watcher: inotify events are not grouped the way FSEvents' 10 ms latency groups them | watcher | medium | **Accepted** | The reader sleeps 10 ms after each read, with the mutex free; `flush` is unchanged (§3.4; WRITE-19's *Today*). |
| 5 | A Rescan doesn't repair lost watches, and the loss is then counted against events | watcher | medium | **Accepted in part** | Checked: `watcher.rs` `apply` records a Rescan walk's finds as unnamed, `fsevents.rs` clears hints past `MAX_HINTS`, so this happens on the Mac too. Taken in: an overflow runs the same rebuild a moved folder runs; every walk watches before it lists; a walk a Rescan asked for counts no misses (§3.4). **Rejected:** a new `Heard::Lost`. A watch that fails after start leaves a folder whose changes the 5 s walks find, and the existing two-miss rule then moves the project to walks; with the Rescan fix it counts the right things. The new variant would only make that switch faster on a machine out of inotify watches (recorded in §6.5). The loop it guarded against can't happen: a rebuild runs only on overflow or a moved folder, never on a Rescan. |
| 6 | No planned test would fail if watching new, moved or refilled folders were broken | watcher | medium | **Accepted, with one test changed** | Three of the four tests as proposed (a folder made after open, one made and filled at once, one renamed). The fourth needed a test-only `Events::lose_events()` in `m2-os`; instead a burst of more than 4,096 tracked files in a new folder makes a real Rescan on every OS, and the test checks it counts no miss and that a later write in that folder is heard. The overflow trigger shares the rebuild routine the rename test runs (§13.2, S8). |
| 7 | Fallbacks on non-APFS Mac disks are never tested, one is already broken, and the design's wording would quietly break two more | macos | high | **Accepted in part** | Checked: `ENOTSUP` 45 and `EOPNOTSUPP` 102 on macOS (`libc` 0.2.190); `ErrorKind::Unsupported` covers only `ENOSYS`, `EOPNOTSUPP` (std 1.99.0); `sync_all` is `F_FULLFSYNC` on Apple. Taken in: macOS's own lists as today's code has them, never `ErrorKind::Unsupported`; the clone's copy stays with the caller on any error but `AlreadyExists`; each list one function per OS with one portable test; the barrier falls back to `fsync` (§2.2, OS-02, OS-03, OS-09). **Rejected:** an exFAT disk image in the Mac check. The fallback logic is now tested on every OS through `Recorder { plain: true }`; only the error numbers are Mac-only, and the new test covers them. `hdiutil` without admin rights is unverified, and the Mac run should stay one plain command (recorded in §6.5). |
| 8 | The by-hand Mac check can hang or be stopped and leave nothing to paste back | macos | medium | **Accepted** | A line when each check starts; Ctrl-C records the running check as stuck and still prints the summary (smoke-tested here); the tests libtest called slow that never finished are named; the Mac stays awake (`caffeinate -i -w`); §1 tells the owner to plug in, keep the lid open, and press Ctrl-C after 30 minutes on one line. |
| 9 | Moving `bundled()` behind an OS switch breaks 3 Linux tests and makes the Mac app's file lookup a Mac-only check | macos | low | **Accepted** | Checked: `resources.rs:55-60` has no `cfg`, and its tests run here. `bundled()` stays; `dirs::bundled` is dropped (§2.4, OS-39). |
| 10 | The macOS barrier's "fallback" repeats `F_FULLFSYNC`, so every save fails on a volume that refuses it; PLAT-3 contradicts it | simplicity | medium | **Accepted** | The same finding as 7's third part. `fsync` when `F_FULLFSYNC` fails; PLAT-3 now says "the strongest call the volume accepts, never an emulation" (§6.2). |
| 11 | Two `fs` items can't be built on Windows as specified, so the Windows step would rewrite callers | simplicity | medium | **Accepted in part** | `sync_dir`: accepted; it answers `Ok` where the OS keeps new entries durable by itself, so `stage.rs` never changes for Windows. Stamps: the API was right and the Windows column wrong. `Stamp::of(&Metadata)` builds on Windows with std's stable fields (creation time, id 0), as today's non-Unix arm does. **Rejected:** a path-based `fs::stamp`. It would add a second stat per walked file (the walk needs to know folders from files) or a follow-links flag for the hash cache. `Stamp` still moves into `m2-os` as one type. |
| 12 | New rules say more than is true: PLAT-7 broken by the design's own folders; PLAT-2 writes a heavy layout into the contract | simplicity | low | **Accepted** | PLAT-7 now covers the folders music2 writes (read-only folders exempt); PLAT-2 names no layout; 3 files per OS instead of 8 (measured: nested modules are found beside `mod.rs`); `process::crash_quietly` is gone, and the two test children call `prctl` themselves (§2.1, §6.2). |
| 13 | 8b.10's MIDI keyboard has no place in the layer; its design says CoreMIDI, which PLAT-2 forbids, and Linux's sequencer is unreachable here | simplicity | low | **Accepted** | `midir` outside `m2-os`, like cpal; "won't open" means no devices; design-8b's word changes in S9 (§2.4, §8). No code now. |
| 14 | `ci/check.sh` runs `rm -rf` on its output folder with no guard | simplicity | low | **Accepted** | It replaces only a folder holding its `.m2-check` marker (smoke-tested here: a folder it didn't make is left alone, exit 2). |
| 15 | Timed gates have no Mac procedure, so step 7's Mac run can't judge step 7, and G11 is never re-run on a Mac after S6/S8 | verification | high | **Accepted** | On macOS `ci/check.sh` runs the gates `ci/gates.txt` names, in release, with one row and each gate's verdict in the summary; `--gates` does the same on Linux for information. This step: `--server tx-data watcher`. §6.1's acceptance text, §7.6 and §7.7 now name it. Still one command for the owner. |
| 16 | One flaky run on the Mac costs a second 30–45 minute owner run | verification | medium | **Accepted, retry changed** | Each failed test binary runs once more alone (cargo's own "to rerun pass" arguments, which also covers the no-harness tests), and each failed browser run too; a pass then shows as "passed on a retry" for the lead to judge. Each failure's first failing line is printed. Every log is packed into one `.tgz` without renders or kept projects. The Mac stays awake (with 8). |
| 17 | A run without plugins says PASS; the Linux plugin builds can't be fetched again | verification | medium | **Accepted** | The headline says `PASS without plugins`, which is not a pass (PLAT-6). `ci/plugins.sh` and `ci/plugins.sha256` install the recorded builds into `~/.local/share/m2-test-plugins` from kept archives, outside `$SCRATCH` (run here offline: hashes match); `ci/check.sh` finds them by itself. The summary names the plugins' versions (macOS) or whether they are the recorded builds (Linux). |
| 18 | HANDOFF's "long-path flake" always fails when the checkout path is over 48 characters | verification | low | **Accepted** | Checked: `ATOM_GET_STR_BUF_SIZE 64` (`quickjs.c:3821`), package modules named by absolute path. S1 names them as `display()` does (§4.1); S9 removes the line from HANDOFF's flakes. |
| 19 | In S1 and S2, `check.sh` runs the old `std/build.sh`, which ignores `--check` and rebuilds `std.wasm` in place | verification | low | **Accepted** | The new `std/build.sh` ships in S1; its row says FAIL until S3 (§5.4, §13). |

---

## 10. Choices between the two drafts

| Topic | `design-small` | `design-shaped` | Taken | Why |
|---|---|---|---|---|
| Layout of `m2-os` | one file per OS (`macos.rs`, `linux.rs`), split into folders later | one module per concern, one folder per OS | **shaped, made smaller** (revision 2) | per OS one `mod.rs` for the small concerns plus `watch.rs` and `main_loop.rs`; later files go beside them, so nothing moves |
| A Windows folder now | none (`compile_error!`) | `windows/` with `Unsupported` stubs and a Windows compile check | **small** | stubs would be thrown away, so each Windows piece would be written twice; the PLAT-2 test keeps OS calls inside `m2-os` |
| `cfg(unix)` code the same on macOS and Linux | stays at its call sites until Windows | moves into `unix.rs` | **shaped** | otherwise the Windows step has to find and move it |
| Trait or functions | functions | functions | both agreed | a trait has one implementation per build |
| Stubs for the macOS lint | `ci/nocc.sh` (needs clang for `-E`), system `ar` | `ci/stub-cc`, `ci/stub-ar` | **shaped** | no system dependency; both measured working |
| Event API | `Heard` callback; hint policy stays in the server | `Hints`, `take`, `put_back`, `MAX_HINTS` inside `m2-os` | **small** (inside shaped's `watch` module) | the hint limit and the filters are policy (PLAT-3); one place for them on every OS |
| inotify instances | one per process | one per watcher | **small** | the 128-instance limit is per user and shared; parallel tests could hit it |
| A watched folder moved within or out of the project | not covered (new and moved-in folders: watch them, then hint) | rebuild the watch map, rescan | **shaped**, with small's rule for new folders; revision 2 runs the same rebuild on overflow | a moved folder's watches keep stale paths; a rebuild is the simplest correct answer, and folder moves are rare |
| Plugins in the Linux app | not until step 9 ("plugins can't be hosted") | a loop thread of its own now | **shaped** | GTK never involved; the code exists (today's test thread); live3b passes without CLAP's timer and fd extensions (measured); the app's test then tests something on Linux |
| The AppKit test | macOS-only items under `cfg` | made portable, renamed `app_main_thread.rs` | **shaped** | it follows from the choice above, and ends lints 7–15 by using the items |
| Recovering a save stopped halfway | move the file back when the lock is taken | count it as at its before hash; Finish and Discard offered | **neither** (revision 2) | neither works: a Tx can't name `.music2/` paths, and a missing `music2.toml` stops the open before recovery. The middle state is removed instead (§3.1). |
| Folders (`dirs`) | `cache_dir` and `clap_dirs` only; `HOME` only | a full `dirs` module; honours `XDG_*` | **both in part**: home, songs, cache, clap; `HOME` only | every `$HOME` read in one place (Windows changes only `home()`); `bundled()` stays plain path code; `XDG_*` waits for Linux packaging, so tests stay isolated by `HOME` alone |
| FUSE volumes | network (walks, `lock.local`) | local | **small** | the safe side for sshfs; the cost on ntfs-3g is only walks |
| `FICLONE` on btrfs/XFS | `Unsupported` for now | built | **small** | no such volume here to test it; a body change later, no move |
| A folder sync refused by FUSE | no change | `EINVAL` → `Unsupported` | **small** | unverifiable here; no change in behaviour today. *Review D2: skipped by `sync_dir` itself, for every writer.* |
| Audio bug 1 and OS-61 | at step 7's finish / 8a | in this step (S7) | **small** | not OS-specific; bug 1 is the first item of step 7's finish, before `Hold` needs it |
| QuickJS `Math` and `**` | not mentioned | libm-backed | **shaped** | the decided rule covers every transcendental on the render path; song constants are on it |
| The host-math test's scope | `audio.rs` and `resample.rs` | all of `core/runtime/src` and `core/play/src` | **shaped** | catches new host math in any file step 7 or 8a adds; finds exactly today's 11 calls |
| `process::memory`, `machine`, `interactive` | at step 7's merge | built now | **small** (signatures fixed here) | no caller on `main` until step 7; the merge writes them once in place |
| Renders record | the Mac's hashes in its S0, three lines changed in its libm slice | the new hashes recorded at once | **small** | the deliberate change is one readable diff |
| Slice order | libm late; `std.wasm` early | libm first; `std.wasm` late; green after S4 | **mixed** | check, suites, `std.wasm`, math, then `m2-os` with the plugin thread first, so clippy gets past `m2-plugins` early (§13.2) |
| live3b with plugins on Linux | "unverified" | "51 of 51, measured" | **shaped** | the log confirms it (§11) |
| `s6-fixes.mjs:112` race | fix now | after `taste` merges | **small** | the trial merge is clean |
| `notes.mjs` against `taste` | expected clean | a conflict | **small** | measured clean (§11) |
| `CLAP_PATH` default on Linux | `~/.clap` | `$SCRATCH/plugins/clap` | **neither**: `~/.local/share/m2-test-plugins/clap`, filled by `ci/plugins.sh` (revision 2) | `~/.clap` must stay empty (`fx-fixes`); `$SCRATCH` gets cleaned; the recorded builds are kept outside it |
| macOS target in `rust-toolchain.toml` | yes | no; the script adds it | **shaped** | the pin file stays minimal; only Linux needs the cross target |
| Check script rows | no toolchain, plugin-folder or repo-unchanged rows | has them, and kernels' clippy | **shaped**, plus gates on the Mac (revision 2) | each catches a real mistake (a moved plugin, a stray write, an unpinned Rust); the Mac is where timings are judged |
| Owner questions | Q1–Q4 (Q3: no plugins in the Linux app) | Q1–Q3 | **shaped's three** | the Linux-app question goes away with plugins built |
| Contract rules | PLAT-1 to PLAT-5 | PLAT-1 to PLAT-8 | **merged to seven** | timing records folded into PLAT-1; no Windows compile rule |

---

## 11. Measured for this design

All on the Linux server at `f126d03`.

**Revision 1** (scratch clone `$SCRATCH/scratch-synthesize`, deleted afterwards; no build needed):
- **Trial merge of the suite lines.** On a clone of `main`: `notes.mjs:97` and `s6-fixes.mjs:112` replaced by the
  waits of §4.4, and `native.ts:16` by the new `isMac`. `git merge --no-commit` of `origin/taste`, `origin/wip/step7`
  and `origin/sounds`: no conflicts.
- **Trial merge of the whole layer's lines.** A marker comment on every line range this design changes (`stage.rs`,
  `lock.rs`, `watch.rs`, `hash.rs`, `tx/mod.rs`, `journal.rs`, the test children, `render.rs`, `project.rs`,
  `resources.rs`, `core/plugins/src/lib.rs`, `main_thread.rs`, `plugin_host.rs`, `graph.rs`, `watcher.rs`, the gates'
  labels and CPU time, `audio.rs`, `resample.rs`, `core/script/src/lib.rs`, the root and kernels' `Cargo.toml`,
  `std/build.sh`, `chrome.mjs`, `run.mjs`, `baseline.sh`, and the contract lines of §6), plus
  `git mv core/kernel std/kernels-src/kernel`, `graph/fsevents.rs` → `graph/events.rs`, the app test's rename, and new
  files. Conflicts: `taste` in `docs/architecture.md` (WRITE-17's *Today*); `wip/step7` in `docs/architecture.md`
  (PLANE-5's line, and the gates table around rows 7–8); `sounds` none. `Cargo.lock` was not simulated.
- **Claims checked in logs:** live3b with plugins: 51 passed, 0 failed (`live3b-plugins-1014-210631-833394.log`). The
  plugins' sha256 in `$SCRATCH/plugins/SHA256SUMS`. The macOS lint of the real tree exits 0
  (`dsh-cross-macos-clippy.log`). The `libm` renders (`dsh-renders-libm-SHA256SUMS`: `39f94764…`, `d752521b…`;
  `dsh-libm-*.log`: `b3daee50…`).
- **Claims checked in source:** `linux-schema.json` is byte-identical to `macOS-schema.json`; `renders.json` holds
  `cb4d39ba…` for `examples/resample`; `std.wasm` is `83cc5961…` and nothing outside `docs/` pins it; `m2-kernel`
  has no tests and inherits `workspace` fields; QuickJS's `Math` functions, `hypot` and `OP_pow` call the C library
  (lines cited in §5.2); no `Math.` or `**` in the SDK, std, examples or templates; the 11 host-math calls are the only
  ones outside test code in `core/runtime/src` and `core/play/src`; `fsevents.rs`'s flags are used only for rescans
  (the watcher reads the hint paths alone); `e2e/run.mjs`'s summary lines (`FAIL <run>`, `  PART: <run>`,
  `  failed: <run> (exit N) <log>`).

**By the attacks** (their logs in `$SCRATCH/logs/`): the design's S1–S3 and the ground patches in a scratch copy gave
29 of 29 browser runs (1,034 checks, twice), the with-plugins Rust tests passing, `std.wasm` rebuilt to the same bytes
in another source and target folder, the kernels' clippy clean, the renders unchanged by the rebuild, and revision 1's
`ci/check.sh` working under Linux bash in 8 min 11 s (`av-*.log`); a 40 MB file without disk blocks 35.2 s after a
Tx-like sync (`ad-delalloc-*.log`); 0 journal commits forced by `fsync` of an unchanged file after renames
(`ad-jbd2.log`); the inotify wake-ups with and without the 10 ms pause, and unwatched folders after an overflow
(`aw-probes.txt`); the same `std.wasm` metadata and bytes under a `rustc` that says it is an arm Mac
(`am-stdhost-*.log`).

**Revision 2** (scratch folder `$SCRATCH/scratch-revise`, deleted afterwards; no cargo build):
- **Checked in source, for each attack:** `stage.rs` 141-206 (the barrier's fallback calls `sync_all`; the clone copies
  on any error but `AlreadyExists`; macOS's rename list `ENOTSUP | EINVAL`), 640-700 (the fallback moves the target
  aside) and 752-775 (its rollback leaves the target missing between two renames); `Before` holds the before bytes in
  memory; `lock.rs` 228-236 (`ENOTSUP | EOPNOTSUPP | ENOLCK`); `session.rs` 233-250 (the manifest check comes before the
  lock and the journal scan); `core/song/src/tx.rs` (the manifest is a path a Tx may write); `journal.rs` 288-300 (the
  scan's barrier on the journal file, then the tidy); no `fsync` in `audio.rs` 73-89, `render.rs` `write_as`/`name_new`,
  the bounce's provenance or `project.rs` `copy_tree`; `watcher.rs` `plan`/`apply` (a Rescan's walk records unnamed
  changes) and `fsevents.rs` (hints cleared past `MAX_HINTS`); `resources.rs` `bundled()` has no `cfg`; `libc` 0.2.190
  (`ENOTSUP` 45, `EOPNOTSUPP` 102 on Apple); the pinned toolchain's std source in its docs (`ErrorKind::Unsupported`
  from `ENOSYS` and `EOPNOTSUPP` only; `F_FULLFSYNC` in Apple's `fsync`); `quickjs.c:3821` (`ATOM_GET_STR_BUF_SIZE
  64`); `core/script/src/lib.rs` (package modules named by absolute path); `crates/gates/src/main.rs` (sets
  `M2_AUDIO=none`, exits 1 when a gate fails) and `report.rs` (verdict lines start `PASS  `, `FAIL  `, `SKIP  `);
  `core/plugins/src/lib.rs` `search_paths()` (`$HOME/Library/…` and `/Library/…`).
- **`ci/check.sh` smoke test.** The §7.1 script in a scratch git repo, with stand-ins for `cargo`, `rustup`, `rustc`,
  `npm`, `node` and `m2-gates` (GNU bash 5.2; bash 3.2 not available here): a failed test binary and a failed suite
  that pass on a retry give `PASS …; passed on a retry` and are listed; ones that fail again are listed with their
  first failing line; SIGINT to the process group during a stuck test gives the row `stopped after 0 min (stuck?)`,
  the stuck test's name and the summary, exit 1; an `M2_CHECK_OUT` the check didn't make is left alone (exit 2); an
  unknown option gives exit 2; the Linux plugin line tells the recorded builds from changed ones; the `.tgz` holds the
  logs only.
- **`ci/plugins.sh`** run offline, from the kept archives copied into a scratch folder: both `.clap` files match the
  recorded hashes, and `SurgeXTData` sits beside the `clap` folder.

## 12. Not verified

| Claim | Why not | Settled by |
|---|---|---|
| The Mac renders `b3daee50…`, `39f94764…` and `d752521b…` after S4 | no Mac here | the owner's run at the step's end |
| `ci/check.sh` runs under macOS's bash 3.2 and BSD tools (`grep -m`, `tar` with a file list, `PlistBuddy`, `caffeinate -w`), in about 45 minutes with the gates | no Mac here; checked by reading only | the same run |
| The AppKit test needs a login session (not ssh) | not tried | the same run |
| The owner's plugins are in `~/Library/Audio/Plug-Ins/CLAP`, and Surge XT there is 1.3.4 | not visible from here | the script's first check and its plugins line |
| The owner's rustup is 1.28 or later (for `rustup toolchain install` with no argument) | not visible from here | the script falls back to `rustup show active-toolchain` |
| `std.wasm` is the same on an arm Mac | rustc's own output on an arm Mac can't be run here (cargo's metadata was checked) | the Mac run's `--check` |
| G11 `tx-data` and `watcher` pass on the Mac after S6 and S8 | no Mac here | the Mac run's gates |
| Which error a Mac volume gives when it refuses `F_FULLFSYNC` (the `fsync` fallback covers `ENOTSUP`, `EOPNOTSUPP`, `ENOTTY`, `EINVAL`) | no such volume here, nor on the owner's APFS disk | a user report; another error keeps today's behaviour (the save fails) |
| Turning on oxc's `exponentiation_operator` alone leaves other compiled output unchanged | the option was read in source, not run | S4 (renders and `renders.json` unchanged) |
| inotify keeps test 5's event cases green, and the four new folder tests pass on both OSes | inotify is not built yet | S8; the Mac run |
| The rebuild after `IN_Q_OVERFLOW` | an overflow can't be made in a test without a hook | the shared routine is tested through the rename test (S8) |
| No test reads a project file in the moment a fallback save used to leave it missing | the fallback runs only on volumes this server lacks | `Recorder { plain: true }` in the kill test (S7) |
| btrfs could make a journal's removal durable before its renames | no btrfs volume here | not needed: S7's push makes it impossible on any file system |
| The Linux app's plugin loop thread in the real app | the app is never launched here; the test runs its hook without a window | the test (S5); a Linux desktop later |
| `renameat2`'s answer on NFS, 9p, exFAT, SMB; `link` on vfat | no such volume here (making one needs root) | the survey's kernel reading; a user report |
| Stored spelling on case-folding Linux volumes | no such volume here | the Windows step or a report |
| A laptop drive's durable-commit cost on Linux | this server's drives have no volatile cache | a Linux laptop, when one exists |
| How much slower subnormals are on Zen 4 | not measured | 8a |
| The owner's Mac can copy a file to this server with `scp` (for the log pack) | not visible from here | asked only if a failure needs the logs |

---

## 13. Build slices (final)

### 13.1 What "green at a slice's end" means

Linux starts red: 11 test failures, clippy stopping at its first crate, 7 suites failing, renders 6 of 8, and
`std.wasm` stale. So at each slice's end `ci/check.sh` passes on Linux **except the baseline failures a later slice
owns, listed by name**; nothing that passed before fails; and the renders match the record, except in S4, the one
deliberate change. The list only shrinks.

| After | Rust tests failing | clippy | browser suites | renders | std rows |
|---|---|---|---|---|---|
| baseline | 11 | fails at lint 1 (15 lints) | 7 FAIL, live3b PART | 6 of 8 | std.wasm stale; the kernels' clippy never run |
| S1 | 11 | same | 7 FAIL; live3b in full (plugins from `ci/plugins.sh`, measured) | 6 of 8 against the Mac's record | std.wasm FAIL (stale until S3; the repo untouched) |
| S2 | 11 | same | **29 of 29** | 6 of 8 | same |
| S3 | 11 | same | 29 of 29 | 6 of 8 (unchanged by the rebuild) | **PASS** |
| S4 | 10 | same | 29 of 29 | **8 of 8** (the deliberate change) | PASS |
| S5 | 9 | lints 2–6 left (clippy now gets past `m2-plugins`) | 29 of 29 | 8 of 8 | PASS |
| S6 | 5 (the event tests) | lints 4–6 left | 29 of 29 | 8 of 8 | PASS |
| S7 | 5 | lints 4–6 left | 29 of 29 | 8 of 8 | PASS |
| **S8** | **0** (with 4 new watcher tests) | **clean: Linux fully green** | 29 of 29 | 8 of 8 | PASS |
| S9 | 0 | clean | 29 of 29 | 8 of 8 | PASS; the Mac run, gates included |

How the clippy column is read: until S8, `ci/check.sh`'s clippy row says FAIL, and with `-D warnings` clippy stops at
the first crate that fails, so it can't list what is left. At each slice's end the lead runs `cargo clippy --locked
--workspace --all-targets` without `-D warnings` and checks that it lists exactly the lints the column names, as
`ground/tests.md` did. Shared code in the crates Linux clippy can't reach yet is linted by the mac lint.

The mac lint passes at `f126d03` (measured) and must pass at the end of every slice. Until S6, live3a can still
crash on the file that goes missing during a save (once in 11 runs, `e2e.md`); its retry in the check tells, and the
lead names it.

### 13.2 The slices

| Slice | What | Fixes | Contract | Size *(estimate)* |
|---|---|---|---|---|
| **S1. The check and the records** | `ci/check.sh`, `ci/stub-cc`, `ci/stub-ar`, `ci/gates.txt`, `ci/plugins.sh`, `ci/plugins.sha256`; the plugins installed once from kept archives; `rust-toolchain.toml`; the new `std/build.sh` (`CARGO_TARGET_DIR`, `--check`); `e2e/renders.sha256` with `HANDOFF.md`'s 8 Mac hashes; `baseline.sh`'s eighth case; commit `linux-schema.json`; QuickJS package modules named as `display()` shows them, with the diagnostics test run once in a checkout path over 60 characters; `renders.json` unchanged | measures today's state; OS-51, 63, 65; the long-path test (attack 18); no in-place `std.wasm` rebuild (attack 19) | acceptance-gates text; the Platforms section (PLAT-1 and PLAT-6 as *Today*, PLAT-6's saying "Linux passes it from S8"; the rest "not yet"); the order-of-work entry | ~650, mostly scripts; ~30 in `core/script` |
| **S2. The browser suites** | `native.ts` (one line); `chrome.mjs` per OS and `chrome-as-mac.sh`; `run.mjs` takes Chrome from `chrome.mjs`; `notes.mjs:97` and `s6-fixes.mjs:112`, one line each; Q1's `make()` rule in `project.rs`, with a unit test | the 7 FAIL suites; OS-41, 73, 74, 75 | — | ~40 |
| **S3. `std.wasm` rebuilds the same** | `git mv core/kernel std/kernels-src/kernel` and the manifests; the kernels' clippy clean; one rebuild; renders, `renders.json` and `bit_identity` byte-identical | the std rows; OS-64 | PLAT-5 *Today*; ABI-1's path | ~20 and the rebuild |
| **S4. The same math everywhere** | `libm` for the 8 calls in `audio.rs` and 3 in `resample.rs`; QuickJS's `Math` and `**` through `libm`; `host_math.rs` and the QuickJS test; re-record `renders.json` (1 line) and `renders.sha256` (2 lines) in their own commit | test 10; renders 8 of 8; OS-45, 47 | RENDER-2 (rule and *Today*); PLAT-4 *Today* | ~120 |
| **S5. `m2-os`, and the plugin main thread** | the crate (`lib.rs`, `unix.rs`, `mac/mod.rs`, `linux/mod.rs`, both `main_loop.rs`), `thread` and `main_loop`; `main_thread.rs` over a `Loop`, with `token` and `is_process_main` kept as wrappers; `use_app_main_thread` and `plugin_host::install`'s body; the Linux app's loop thread; `app_main_thread.rs`; `dirs::clap` and `search_paths`; `thread::cpu_time` for the gates; `one_place.rs` with its allow-list | test 11; lints 1, 7–15; OS-23–27, 30, 32, 80 | HOST-2, HOST-3; PLAT-2 and PLAT-3 *Today* | ~150 moved, ~200 new |
| **S6. Files, the lock, identity and folders** | `fs` (with `Stamp` and the per-OS error lists and their portable test), `host`, `dirs` (home, songs, cache); the macOS barrier's `fsync` fallback; `sync_dir`'s meaning; callers in `stage.rs`, `lock.rs`, `watch.rs`, `hash.rs`, `tx/mod.rs`, `render.rs`, `project.rs`, `resources.rs` (`cache_dir` only); `prctl` in the two test children; the gates time `fs::barrier` and say "the durable sync" | tests 6–9 (8 on ext4); lints 2, 3; the kill test 473 s → 104 s; the vanishing file during saves on ext4; OS-01–19, 36, 37, 72, 81; attacks 7, 9, 10, 11 | PLANE-5; WRITE-15 (barrier and renames), 16 (machine id), 17 *Today*; FMT-16; FMT-20; RENDER-30; the layout block; PLAT-7 *Today* | ~300 moved, ~170 new |
| **S7. Saves that a crash can't break** | the fallback without the move aside: compare, then one rename over the target; its rollback from the before bytes; the kill test runs every case with `Recorder::default()` and `Recorder { plain: true }`, plus a case whose Tx writes `music2.toml` and `song.ts`; new files pushed before they are named and their folders synced (`write_as`, `write_new_wav`, the bounce's provenance, `create_at`); the scan pushes a completed journal's folders before its barrier | `ground/tests.md` surprise 1 (no fix offered after a stopped save) on exFAT, SMB, NFS and Windows; attacks 1, 2, 3 | WRITE-1; WRITE-15 *Today* (the fallback, the tidy); WRITE-16 *Today* (the kill test) | ~100 |
| **S8. File events** | `watch::Events` (FSEvents moved; inotify new, §3.4: the 10 ms pause, watch before list, the rebuild on a moved folder and on overflow); `graph/fsevents.rs` → `graph/events.rs`; a Rescan's walk counts no misses; four new tests in `watch_tests.rs`, each with the 5 s walk set to 60 s and `grace` to 300 ms so only an event can make a tick look, each asserting that its last write is taken in within 1.5 s, `misses == 0` after the grace, and `events` still true: a folder made after open (write a file there, wait, write it again), a folder made and filled at once (`create_dir_all` three deep, write, write again), a folder renamed in the project (write under its new name), a burst of more than 4,096 tracked files in a new folder (its Rescan walk takes them in; then write one of them again); no OS `cfg` in `graph.rs` or `watcher.rs`; G11's "watched by file-system events"; `one_place.rs`'s allow-list empty | tests 1–5 and the 4 new; lints 4–6: **Linux fully green**; OS-20–22, 81; attacks 4, 5, 6 | WRITE-19 *Today*; PLAT-2 *Today* | ~250 moved, ~350 new |
| **S9. Handoff and the Mac run** | `HANDOFF.md` (Linux green at `<commit>`, "macOS: unverified since <S1's commit>", how to run the check, the long-path line gone from the flakes); design-8b §19's "CoreMIDI" → "midir"; a two-line note at the top of design-8a
§6.3 that its `rt.rs`, `Sem` and QoS calls go into `m2-os` (`mac/rt.rs`, `linux/rt.rs`) as §8 says, and its overload
listener gives way to cpal's xrun errors (survey-later §5.5); the gates row and the deferred rows; commit and push; ask the owner for the Mac run (§1); record its result and the gates' numbers with the machine named | — | the acceptance-gates row; Deferred rows | docs |

Each slice is one or a few commits on `main`, each green as defined in §13.1, pushed once the slice passes. The whole
step is about 2 weeks of work *(estimate)*, then the Mac run.

Why this order:
- **The check first**, so every slice is measured the same way, with the plugins, in any checkout path, and without
  ever rebuilding `std.wasm` in place.
- **The suites second**: small, independent of `m2-os`, and they guard every later slice's UI behaviour.
- **`std.wasm` before the math**, so each deliberate change is alone: S3 changes kernel hashes but no render; S4
  changes three render hashes and nothing else.
- **The plugin main thread first among the `m2-os` slices**: its lint 1 stops clippy at `m2-plugins` today, so fixing
  it first lets clippy check more crates in every later slice (then S6 clears `m2-session`).
- **Saves after the disk calls**: S7 builds on S6's `fs::push`, `fs::sync_dir` and `fs::rename`.
- **Events last**: the largest new piece, built on a crate whose other parts are already settled.

### 13.3 The merges after this step

`HANDOFF.md`'s order stays: finish step 7, then `sounds`, then `taste`. A trial merge with marker edits on every line
this design changes, and the three renames (`core/kernel`, `fsevents.rs`, the app test), conflicts only in
`docs/architecture.md` (§11). `Cargo.lock` was not simulated; `branches.md` measured its conflict with step 7. Revision
2's new edits (`journal.rs`'s scan, `audio.rs`'s `write_new_wav`, `apps/m2/src/main.rs`'s bounce, `project.rs`'s
`create_at`, `core/script/src/lib.rs`'s module names) touch files the trial already marked, except `apps/m2/src/main.rs`.
Checked with `git diff main...origin/<branch>`: no branch has a hunk near those lines (`taste` changes
`apps/m2/src/main.rs` and `project.rs` elsewhere; `wip/step7` changes `audio.rs` and `render.rs` elsewhere; `sounds`
none of them).

| Branch | Conflict | How to resolve |
|---|---|---|
| `wip/step7` | `docs/architecture.md`: PLANE-5's rule line, and the acceptance-gates table (step 7 edits row 7; this step adds a row and edits "in CI") | keep both edits |
| | `Cargo.lock` (neighbouring lines: `m2-os`, `libm`) | take `main`'s lock, then `cargo build` adds step 7's two path edges; never `cargo update` |
| | `core/plugins/src/probe.rs` (6 calls to `main_thread::token()` and `is_process_main()`) | none: both stay as wrappers |
| | `core/runtime/src/audio.rs` | clean: the 8 `libm` edits land in step 7's moved code (`branches.md` §2.2); `host_math.rs` guards it. Check that step 7's moved `write_new_wav` keeps S7's push. |
| | `crates/gates/src/store.rs` (new: Mach `task_info`, QoS) | clean as text. The step 7 finisher moves its two `cfg` blocks into `m2_os::process::memory()` and `thread::interactive()`, makes G7f's memory rules fail when memory is `None`, and has records name their machine (§8). |
| | `apps/music2-desktop/src/main.rs` | none: this step doesn't touch it |
| | `tx/mod.rs`, `render.rs`, `gates/src/server.rs` | clean (trial); step 7's hunks are far from this step's lines |
| `sounds` | `std/kernels/std.wasm` (binary, once `main` has rebuilt it) | never take a side: rebuild with `std/build.sh`, run `--check`, check the renders, commit once. Then add `examples/sounds` to `PURE`, `baseline.sh` and both records, recorded on Linux; the next Mac run confirms them. |
| `taste` | `docs/architecture.md`: WRITE-17's *Today* (taste rewrote it for the Read only pill) | keep `taste`'s text, then apply this step's `MNT_LOCAL` edit to it |
| | `e2e/notes.mjs`, `e2e/s6-fixes.mjs`, `web/src/lib/native.ts` | clean (trial: one unchanged line separates `taste`'s change from this step's) |
| | after the merge | `taste`'s 3 new suites need no change (the Mac user agent covers them; `branches.md` §3.3). Re-run s8-tracks' "layout.json within a second" on the inotify watcher. The key table, the menu per OS, the reveal label and the PC-keys suite are `taste`/8b work. README's "You need a Mac" gains Linux then (left alone on `main` to spare `taste` a conflict). Mac run before the merge. |

---

## 14. Owner questions

Only real decisions. Each has a recommended answer. Q1 and Q2 are needed before slice S2 starts; Q3 can wait.

| # | Question | Recommended | Why |
|---|---|---|---|
| Q1 | On Linux, should a new song's folder name count as taken when a folder with the same name in other capitals exists ("exports 2" next to "Exports 2")? | **Yes, on every computer.** | The Mac already works this way, so nothing changes there. Song folders stay safe to copy between computers. The `fx-fixes` suite then passes unchanged (measured). If no: the suite asks the disk whether it ignores capitals. |
| Q2 | On Linux, the browser suites test the Mac's keys (⌘) and labels, by telling Chrome it is a Mac. Ctrl gets its own small suite later, with the key table on `taste`. OK? | **Yes.** | Two suite lines change on `main`, so `taste` merges without suite conflicts. The Mac is the reference. Until the key table, nothing tests the Ctrl keys a Linux user presses. |
| Q3 | Lock messages on the server say "Ubuntu-2404-noble-amd64-base". Rename the server to a name of your choice? | **Yes, if you like; skipping it is fine.** | It is a system setting only you should change: `sudo hostnamectl set-hostname <name>`, and the same new name on the two lines of `/etc/hosts` that name the old one (otherwise `sudo` warns that it can't find the host). Nothing in music2 depends on it: music2 tells computers apart by their machine id. |


### Owner answers (2026-10-14, after revision 2)

| # | Answer |
|---|---|
| Q1 | **Yes, on every computer**: a new song's folder name is taken when the same name in other capitals exists. |
| Q2 | **Yes**: on Linux the suites tell Chrome it is a Mac; a Ctrl suite comes later with `taste`'s key table. |
| Q3 | **Leave the host name** as it is. |
| Mac runs | **At the end of each step** (about 6 runs in all), as PLAT-6 and §1 say. |
| Desktop app on Linux | The owner **allows opening `music2-desktop` on an invisible Xvfb screen on the server** (never a real screen), once per step, with screenshots sent to them. So the Linux check gains an app row (S1, §16), and §12's "the app is never launched here" is settled by it from S5. |

---

## 15. Critic pass

Label `critic`, 2026-10-14, against revision 2 and `main` at `f126d03`. Read against the surveys, the ground notes, the
five attacks, `docs/architecture.md` and the code.

**What holds (checked).**
- Every piece OS-01 to OS-85 has exactly one row in §3 and a decision; the totals line is right (24, 20, 32, 4, 5).
  Each deferral names a step or a trigger.
- Every Linux failure in `ground/tests.md` and `ground/e2e.md` (11 Rust tests, 15 lints, 7 FAIL suites and live3b's
  PART, the empty desktop test, the 2 differing renders) maps to a slice in §4, and each slice's done-criteria can be
  checked here, except S9's Mac run.
- All 19 attack problems are in §9, and each accepted fix is in the text it names. The rejections hold.
- Every "old text" in §6.1 appears once, word for word, in `docs/architecture.md` (the layout block's line is in a code
  block, without backticks).
- The render-hash change is S4's alone: S1 records the Mac's hashes, S3 changes kernel hashes but no render (measured by
  `attack-verification`), and S4 changes three lines.
- PLAT-2's patterns match 18 files under `core/`, `crates/` and `apps/m2/` today (grep). 15 have hits outside test
  code, and each is moved by a slice (S5, S6 or S8) or is the gates' allowed function, so `one_place.rs`'s allow-list
  can be empty after S8. The other 3 (`files.rs`, `notes.rs`, `export.rs`) have hits only in `#[cfg(test)]` code.
- `ci/check.sh` as drafted still passes `bash -n` after the edits below.

**Small fixes made in this pass.**

| # | Where | Fix |
|---|---|---|
| 1 | §1 | "the same exported audio as the Mac": your Mac run confirms it (it is predicted until then). |
| 2 | §1 | `m2-os` holds "almost" everything: the app's window and menu code stays in the app (§2.4). |
| 3 | §1 | The Mac lint "type-checks" Mac code; a Mac is still needed to build it fully (no C, no link here). |
| 4 | §1, §7.1 step 0 | Plugins in a shared CLAP folder now stop the check at once (exit 2), instead of a 45-minute run that can't pass. |
| 5 | §1 | The Mac-run table gains "at the end of each later step", which §7.6 and PLAT-6 already require. |
| 6 | §2.2, error table, §12 | The macOS barrier falls back to `fsync` only when `F_FULLFSYNC` is refused (`ENOTSUP`, `EOPNOTSUPP`, `ENOTTY`, `EINVAL`). Any other error, such as `EIO`, is returned. Falling back on every error could report a failed flush as done, which breaks PLANE-5. The error-list test feeds `ENOTTY` and `EIO` too. |
| 7 | §6.1 | PLANE-5's new text names the macOS fallback, as WRITE-15's already did, so nothing is weakened without words. |
| 8 | §3.4 | An overflow of the one shared inotify queue loses events for every watcher in the process, so it rebuilds every watcher's root, and each gets `Rescan`. |
| 9 | §5.1 | `.powi(` is banned too (not correctly rounded; debug and release may differ), and "powi is exact" is gone. The probe plugin's synth and the gates' test signals are listed as left alone. |
| 10 | §5.2, §12 | oxc's option is now named (`es2016.exponentiation_operator`, read in `oxc_transformer` 0.153.0). "Changes no recorded hash" is marked as checked in S4. |
| 11 | §6.2 PLAT-6, §7.1 step 9 | No fixed "8" renders: `sounds`' merge adds one. The renders row counts the lines of `e2e/renders.sha256`. |
| 12 | §7.3 | The Airwindows zip exists only in `$SCRATCH/plugins/dl/` until S1 copies it (checked). |
| 13 | §8 | Step 7's row gives G7f the Linux facts on wasmtime's reservations (`facts.md` §8). |
| 14 | §13.1 | How the clippy column is read until S8: a run without `-D warnings`, since `-D warnings` stops at the first crate; the mac lint covers shared code meanwhile. |
| 15 | §13.2 S1 | PLAT-6's *Today* says Linux passes from S8, so the contract is true at every slice. |
| 16 | §13.2 S9 | A pointer in design-8a §6.3: its `rt.rs`, `Sem` and QoS calls go into `m2-os` (survey-later §5.5). |
| 17 | §13.3 | README's "You need a Mac" changes after `taste` merges (left alone on `main` to spare a conflict). |
| 18 | §14 | Q1 and Q2 are needed before S2. Q3 also needs `/etc/hosts` (it maps the old name here), or `sudo` warns. |

**Left for the lead** (bigger than a text fix; also returned to the workflow):
- PLAT-6 and §7.6 ask for a Mac run at the end of *every* step. The owner asked for as few as possible. Confirm that
  with the owner, or narrow it to steps with timed gates or macOS-only changes.
- Copy `$SCRATCH/plugins/dl/` to `~/.local/share/m2-test-plugins/dl/` before any clean-up of `$SCRATCH`.
- `ground/e2e.md` said that unpacking the plugins into a user folder needs the owner's OK. Confirm it was given before S1
  installs them in `~/.local/share`.
- The Mac's fallbacks on exFAT and SMB (rename, clone, lock and now the barrier's error list) stay unrun by any check
  (§6.5's exFAT-image row). Accept that, or plan the `hdiutil` probe once a Mac shows it needs no admin rights.


---

## 16. Lead's notes for the build (after the owner's answers)

- **The app row (Linux only).** `ci/check.sh` gains a row `app` after the desktop build, on Linux when `Xvfb` is on
  `PATH` (it is on the server); on macOS the row is skipped, because the app must never open on a real screen. It
  opens `music2-desktop` on an invisible display (`Xvfb :<free>`, a scratch `HOME`, `M2_AUDIO=none`,
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`, `LIBGL_ALWAYS_SOFTWARE=1`), twice: with `examples/first-song`, and with
  `examples/with-plugins` and the test plugins on `CLAP_PATH`. It passes when the app's log says
  `UI connected to the native shell` within 30 s, has no panic, and (from S5) never says `plugins can't be hosted`.
  It saves a screenshot of each run to the log folder with a tiny X11 tool built on the fly from `ci/xshot.c`
  (X11 and libpng; about 40 lines; the row still passes without the screenshot when the headers are missing, and
  says so). The lead sends the screenshots to the owner once per step.
- **The plugin archives.** Before any clean-up of `$SCRATCH`, copy `$SCRATCH/plugins/dl/` (Surge XT 1.3.4's
  `.deb`, Airwindows Consolidated `2026-10-11-8a67b80`'s zip, sha256 `9144dc8b…f12c`, deleted upstream since) to
  where S1's `ci/plugins.sh` keeps its archives.
- **Two libm hashes in the ground notes.** `facts.md` reports `39f94764…` for `examples/resample` with `libm`;
  `tests.md` reports `b3daee50…`. They patched different sets of calls or cases; S4 records whatever the final code
  renders, and §12's row names the Mac's confirmation.
