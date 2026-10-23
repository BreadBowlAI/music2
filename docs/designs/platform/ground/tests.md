# Rust tests, clippy and builds on Linux: the root causes

Ground work for the platform design. Measured on the Linux server (Ubuntu 24.04, glibc 2.39, Ryzen 9 7950X3D, ext4; `/tmp`
is on that ext4) at f126d03, with rustc 1.99.0. Logs are in `$SCRATCH/base-f126d03/` (the lead's baseline) and
`$SCRATCH/logs/gt-*.log` (mine).

## In short

- **11 failures, 4 causes.** 10 cargo tests fail, and the plugin main-thread test (no harness) panics. A missing Linux
  piece triggers every one. None is an engine bug. One of them also uncovers a real session bug (surprise 1 below).

  | Cause | Failures | OS-nn | Fix | Checked here |
  |---|---|---|---|---|
  | No file events on Linux | 5 | OS-20, 21, 22 | an inotify event source | by code and the stats the tests print |
  | No atomic swap or "create only" rename on Linux | 4 | OS-03 | `renameat2` | **yes**: all 4 pass with it |
  | "Is this the main thread?" is always "no" off macOS | 1 | OS-24 | `gettid() == getpid()` | **yes**: passes with it |
  | The audio clip path uses the system's math library (`cosf` and others) | 1 | OS-45 | the `libm` crate, then record the hashes once | **yes** on Linux; the Mac still needs its run |

- **Two surprises the surveys did not predict.**
  1. **The fallback save is not crash-safe.** Without a swap, a save moves the old file aside, then moves the new one in.
     A crash between the two leaves the file missing. Recovery then says "changed since, check by hand" and offers no
     fix. This is a bug in the session, on every OS: the Mac on exFAT or SMB, Linux on NFS or exFAT, and Windows
     always (NTFS has no atomic swap). Linux exposed it because Linux took the fallback for every save.
  2. **Each deliberate crash costs about 1.1 s here.** Ubuntu hands every core dump to apport, even with `ulimit -c 0`.
     The kill test crashes its child about 200 times. It takes 473 s with dumps and 104 s without. The fix is one call
     in the test child: `prctl(PR_SET_DUMPABLE, 0)` (OS-72).
- **Clippy finds 15 lints, not 1.** All are dead code: macOS-only branches with no Linux twin yet. The baseline saw only
  the first, because it stops clippy at `m2-plugins`, and every later crate depends on that one. Each lint goes away
  when its Linux piece lands.
- **The desktop app's tests pass, but test nothing on Linux.** 0 unit tests; the AppKit test prints "macOS only".
- **The release build works.** `cargo build --release --workspace` builds everything, the desktop app included, with
  the same warnings as clippy. No Linux-only problem.
- **The two long engine tests pass on Linux.** Live playback equals the export bit for bit, and 10 minutes of looping
  don't drift (both `#[ignore]`d, run in release).

## What I ran

| Run | What | Result |
|---|---|---|
| Lead's baseline | `cargo test --workspace --exclude music2-desktop` at f126d03 | 686 pass, 10 fail, 4 ignored; `main_thread` panics (`test.log`) |
| Single reruns | each failing test alone, `RUST_BACKTRACE=1 --nocapture` | the same failures, the same lines (`gt-kill-*.log` and below) |
| Clippy, `-D warnings` | `cargo clippy --workspace --all-targets --keep-going -- -D warnings` (desktop included) | fails at `m2-plugins` (1 lint); `--keep-going` can't pass it, since everything depends on it (`gt-clippy-D-full.log`) |
| Clippy, warnings only | the same without `-D warnings`, so every crate is checked | 15 lints, listed in §2 (`gt-clippy-warn-full.log`) |
| Desktop tests | `cargo test -p music2-desktop` | exit 0; 0 unit tests; "appkit_main_thread: macOS only" (`gt-desktop-test2-*.log`) |
| Release build | `cargo build --release --workspace` | ok in 1 m 37 s (`gt-release-full.log`) |
| Scratch fixes | a copy of f126d03 with `renameat2`, `gettid` and `libm` (the diff, with the later dump fix and the fallback experiment: `gt-scratch-fixes.diff`) | 690 pass, 6 fail: the 5 watch tests and the render hash (to record); `main_thread: ok` (`gt-xtest-full.log`) |
| Kill test timing | the kill test alone, with and without core dumps | 473 s with dumps, 104 s with `PR_SET_DUMPABLE 0` (`gt-xkill*.log`) |
| Fallback kill points | the kill test's 5 cases on a disk that can't swap (`Recorder { plain: true }`) | 10 kill points over 4 cases offer no fix (`gt-xkill2-*.log`) |
| Math compare | glibc against the `libm` crate on the exact inputs `examples/resample` uses | `cosf` differs on about 1%; `pow` agrees (`gt-mathcmp.log`, with the program) |
| Abort cost | a C program that calls `abort()`, 20 times, dumpable or not | 21.8 s against 0.013 s |
| Ignored tests | `cargo test --release -p m2-play --test bit_identity --test live -- --ignored` | both ok (§5; `gt-ignored-*.log`) |

Other agents were building at the same time; the load average was 8 to 21.

---

## 1. The failing tests

| # | Test | What fails | Root cause on Linux | OS-nn | Fix |
|---|---|---|---|---|---|
| 1 | `graph::watch_tests::an_idle_project_is_watched_without_a_stat` | `watch_tests.rs:38` `assert!(stats(&graph).events, "a temp folder is on a local volume")` | `source()` (`watcher.rs` 176-193) returns `Source::Walk` off macOS: `mod fsevents` exists only on macOS (`graph.rs:40`). So `events` is false. | OS-20, OS-21, OS-22 | inotify source |
| 2 | `graph::watch_tests::own_commits_make_no_batch_with_events` | `:67` `assert!(s.events && s.scoped > 0)`; it printed `events: false, walks: 21, scoped: 1` | the same | OS-20 | inotify source |
| 3 | `graph::watch_tests::events_that_miss_changes_give_way_to_walks` | `:238`, at n = 5: `assert!(s.events && s.misses == 1)`; it printed `events: false, misses: 0` | the same: with no events, nothing can be missed | OS-20 | inotify source |
| 4 | `graph::watch_tests::an_event_wakes_an_idle_watcher_and_a_batch_wakes_it_when_due` **(not predicted)** | `:264` `assert!(t.elapsed() < 1 s, "woken by the event")`; it waited 5.000056 s | only `fsevents.rs:56` calls `Wake::wake()`. Off macOS nothing wakes the watcher, so it sleeps its full 5 s. | OS-20 | the inotify reader calls `wake.wake()` for each named path |
| 5 | `graph::tests::a_lane_written_in_two_halves_never_plays_empty` **(not predicted)** | `tests.rs:996` `assert_eq!(graph.watch_stats().unwrap().events, !walk)`, in its third case (events, 60 ms) | no events. Its first two cases (walks, 60 ms and 400 ms) pass on Linux. | OS-20 | inotify source; then see the note below |
| 6 | `tx::hook_tests::a_commit_that_fails_after_its_preview_accepts_nothing` | `hook_tests.rs:148` `calls[renames[1]].contains("extra.json")` | `Real::rename` (`stage.rs` 155-174) returns `Unsupported` for `Swap` and `Excl` off macOS. So every replace takes the fallback: the old file goes to `.music2/tx/<jid>/old-k`, then the new one comes in. The second rename into the project is then the fallback's, not `extra.json`. | OS-03 | `renameat2` |
| 7 | `tx::stage_tests::a_rollback_that_fails_is_reported_at_once` **(not predicted)** | `stage_tests.rs:254` wants "couldn't be undone"; got "song.ts, … couldn't be saved: injected" | The test fails the second rename into the project and the call after it. On Linux these are the fallback's `staged → song.ts` and the rollback's `mkdirs`, whose error the rollback ignores. The rollback then succeeds, so nothing is "interrupted". | OS-03 | `renameat2` |
| 8 | `tx::kill_tests::killed_at_every_commit_step_the_project_recovers` **(not predicted)** | `kill_tests.rs:174` "several 22: Discard is always offered"; recovery said "song.ts changed since … check it by hand", with no fixes | Killed at call 22: the fallback had moved `song.ts` aside to `old-0` and not yet moved the new file in. `song.ts` is missing. Recovery (`journal.rs` `at`, 183-201) compares the target with its before and after hashes. Missing matches neither, so it reads as "changed since". | OS-03 is the trigger. The gap is a session bug (below). | `renameat2` makes it pass on ext4. Recovery must also learn the fallback's state. |
| 9 | `graph::preview_tests::an_interrupted_commit_after_its_preview_points_to_needs_attention` **(not predicted)** | `preview_tests.rs:780` wants "couldn't be undone … Needs attention"; got "… couldn't be saved: injected. It played for a moment and was taken back." | The test fails `spare.json Excl`, then the next call that matches `hits.json Swap` (the rollback's swap back). On Linux the forward replace took the fallback, so its rollback is two `Plain` renames. They never match, the rollback succeeds, and the edit is "taken back". | OS-03 | `renameat2` |
| 10 | `gate_tests::fixture_renders_change_only_with_stds_version` | `gate_tests.rs:384` `recorded == now` | `examples/resample` renders `e2e93ef0…` here and `cb4d39ba…` on the Mac. The other 3 fixtures match the Mac. The audio clip path calls the system's math library in `core/runtime/src/audio.rs` 191, 253, 283, 294, 297, 411, 415, 424. glibc and Apple round some of those results differently in the last bit (see 1a). | OS-45 | `libm` crate for these calls; record `renders.json` once |
| 11 | `core/plugins/tests/main_thread.rs` (no harness) | `main_thread.rs:78` "init ran off the main thread: … process_main: false, says_main: true" | `is_process_main()` returns `false` off macOS (`main_thread.rs` 69-72). The probe records it (`probe.rs:352`). The call did run on the real main thread: the host's own check (`says_main`) is right. | OS-24, OS-28 | `gettid() == getpid()` on Linux |

**Checked with the fixes.** In the scratch copy, tests 6 to 9 and 11 pass with these two changes:

```rust
// stage.rs, Real::rename, after the macOS arm
#[cfg(target_os = "linux")]
{
    let flags = if how == How::Swap { libc::RENAME_EXCHANGE } else { libc::RENAME_NOREPLACE };
    let (a, b) = (cpath(from)?, cpath(to)?);
    if unsafe { libc::renameat2(libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), flags) } == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    return match e.raw_os_error() {
        Some(c) if c == libc::EINVAL || c == libc::ENOSYS || c == libc::ENOTSUP => Err(io::Error::new(ErrorKind::Unsupported, e)),
        _ => Err(e),
    };
}

// main_thread.rs (m2-plugins gains `libc`)
#[cfg(target_os = "linux")]
pub(crate) fn is_process_main() -> bool {
    unsafe { libc::gettid() == libc::getpid() }
}
```

`RENAME_NOREPLACE` gives `EEXIST` when the name is taken, which is `AlreadyExists`, as `RENAME_EXCL` gives on the Mac.
`gettid` and `getpid` are plain system calls: no allocation, no lock. The no-alloc and no-blocking tests pass with it.

### 1a. The render hash (test 10)

| Call in `audio.rs` | Inputs in `examples/resample` | glibc 2.39 against the `libm` crate 0.2.16 |
|---|---|---|
| `cosf`, the stretch window (191) | 2048 points | 22 differ |
| `cosf`, fades in and out (294, 297) | 0.01, 0.02, 0.1 and 0.3 s at 48 kHz | 5 of 481, 11 of 961, 67 of 4801, 186 of 14401 differ |
| `sinf`/`cosf`, the audio lane's 5 ms declick at a seek, wrap or stop (411, 415, 424) | sampled fade lengths 64 to 4096 | 236 of 19154 differ |
| `pow(2, pitch / 12)` (253) | pitch 12, -5, 7 | all the same |
| `powf(10, gain / 20)` (283) | gain 0 everywhere | 1, exact |

- With `libm` for all eight calls, Linux renders `examples/resample` as **`b3daee50ee7e7193fc6d240f02384fcda83ccbe33b5c20fbf5904005b4aeb58e`**.
  `first-song`, `empty` and `starter` keep their hashes, the Mac's.
- That is not the Mac's `cb4d39ba…` either, so Apple's math also differs from `libm`'s on some of these inputs. As decided, the Mac's
  recorded hash changes once. The Mac must then render `b3daee50…` too. *(unverified: needs the macOS runner)* `libm`
  is plain Rust arithmetic, and Rust never fuses or reorders floating point, so it should.
- glibc on x86_64 picks an FMA build of `cosf` at run time when the CPU has FMA. This CPU has it. So two x86 machines
  can differ even with glibc. `libm` removes that too.
- The same rule should cover `core/play/src/resample.rs` 46-49 (f64 `sin`/`cos`). It is live only (the device's rate),
  not export. glibc and `libm` differ on 6,385 of 200,000 sampled inputs there. `crates/editor/src/render.rs:307`
  (`log10` for a peak in dB) is a number shown to the user, not audio. `reference.rs:180` uses `powi`, which is Rust's
  own code on every OS.
- The kernels' math already gives the same bits on both machines, for these songs: 3 of 4 fixtures and 6 of 8 offline
  renders match the Mac bit for bit (`base-f126d03/renders/SHA256SUMS` against HANDOFF.md). Only the two `examples-resample` renders differ.

### 1b. Surprise 1: the fallback save is not crash-safe (test 8)

The kill test kills the commit at every disk call, reopens the project, and checks that recovery offers Discard.
I ran its 5 cases on a disk that can't swap (`Recorder { plain: true }`), as Linux was at f126d03:

| Case | Disk calls | Kill points with no fix | Where |
|---|---|---|---|
| several | 51 | 2 (22, 23) | `song.ts` moved aside, before the new one moved in |
| blob | 23 | 2 (19, 20) | the same for `data/lead/lead.state` |
| two | 53 | 2 (24, 25) | the same for `song.ts` |
| code | 26 | 4 (16, 17, 20, 21) | the same for `tracks/hats.ts`, then `song.ts` |
| dataonly | 46 | 0 | it replaces no file |

- **What the user sees.** After a crash or power cut at that moment, the file is missing. The app says "A commit was
  interrupted, and song.ts changed since, so it can't be finished or undone safely. Check it by hand". It offers
  neither Finish nor Discard. The old bytes are safe: in `.music2/tx/<jid>/old-0`, and in the journal (inline text, or
  a blob).
- **Who hits it.** Any volume without an atomic swap: the Mac on exFAT, FAT or SMB; Linux on NFS, 9p, CIFS, exFAT
  (survey §1); Windows on NTFS always. The sibling report `e2e.md` saw the same window from outside: "a saved file
  vanishes for about 0.2 ms during each save", and live3a crashed on it once.
- **Fix (session, not the platform layer).** Recovery should know the fallback's middle state: target missing, and
  `.music2/tx/<jid>/old-<k>` holding the before bytes. That state can be finished (move the staged file in) or
  discarded (move `old-k` back). The kill test should then run each case twice, with a swapping disk and with
  `plain: true`, so the fallback stays tested on every OS. *(Another way: a fallback in which the target never goes
  missing. That needs a hard link, which FAT lacks.)*
- With `renameat2`, the kill test passes on this ext4: every case is offered, silent and finished at least once.

### 1c. Surprise 2: deliberate crashes are slow here (OS-72)

- The kill test runs its own binary as a child about 200 times and has it call `abort()`. The hook test does it once.
- This server's `core_pattern` is `|/usr/share/apport/apport …`, and apport is enabled. The shell's `ulimit -c` is 0, yet
  the kernel still pipes each dump to apport: the shell reports "(core dumped)" for each one. Apport writes nothing (no
  files in `/var/crash` or `/var/lib/apport/coredump`), but each dump costs about 1.1 s.
- Measured: 20 aborts took 21.8 s; with `prctl(PR_SET_DUMPABLE, 0)` first, 0.013 s.
- The kill test alone: **473 s** with dumps, **104 s** with `PR_SET_DUMPABLE 0` in its child. The baseline's 36 s for
  all of `m2-session` was short only because the kill test failed at its 23rd child.
- **Fix:** a small test helper, "this child crashes on purpose", that calls `prctl(PR_SET_DUMPABLE, 0)` on Linux
  (and turns off crash reports on the other OSes). Call it in `kill_tests::child` and `hook_tests::hook_child`. The
  survey's `RLIMIT_CORE = 0` is not enough on Ubuntu, as shown above. Changing `core_pattern` is a system setting; the
  owner's call, and not needed.

### 1d. A note for the inotify work (test 5)

The test's own comment says FSEvents names a file written while open "only when it is opened and closed". inotify
names every write (`IN_MODIFY`) and the close (`IN_CLOSE_WRITE`). The test's promise, "the lane never plays empty",
should still hold, but its event-mode cases must be re-run once inotify lands. The walk-mode cases already pass here.

---

## 2. Clippy on Linux, whole workspace

`cargo clippy --workspace --all-targets --keep-going -- -D warnings` stops at the first lint, in `m2-plugins`. Every
crate after it depends on `m2-plugins`, so `--keep-going` cannot reach them. To see them all, I ran the same command
without `-D warnings`. The list is the same set `-D warnings` would refuse. There are no `clippy::` lints: all 15 are
rustc's dead-code and unused-variable lints, from macOS-only code.

| # | Lint | File:line | Target | Root cause | OS-nn | Fix |
|---|---|---|---|---|---|---|
| 1 | function `install` is never used | `core/plugins/src/main_thread.rs:217` | lib, lib test | only the macOS arms call it. The Linux arm of `run_main` calls `EXEC.set` directly. | OS-26, OS-27 | let `run_main` install through `install` on every OS; the Linux app executor (GTK's main loop) uses it too |
| 2 | unused variable `dir` | `core/session/src/lock.rs:213` | lib | `local_volume` reads `dir` only in its macOS `statfs`/`MNT_LOCAL` arm | OS-15 | a Linux arm: `statfs` `f_type` (NFS `0x6969`, SMB `0x517B`, CIFS `0xFF534D42`, SMB2 `0xFE534D42`, 9p `0x01021997`, Ceph `0x00C36400`) or the type in `/proc/self/mountinfo` |
| 3 | function `cpath` is never used | `core/session/src/tx/stage.rs:106` | lib | only `renamex_np` and `clonefile` (macOS) use it | OS-03, OS-06 | the `renameat2` arm uses it (checked: the lint is gone with it) |
| 4 | method `wake` is never used | `crates/server/src/graph/watcher.rs:55` | lib | only `fsevents.rs:56` calls it | OS-20 | the inotify source calls it |
| 5 | field `backstop` is never read | `crates/server/src/graph/watcher.rs:134` | lib | read only in the macOS `Source::Events` arm (402) | OS-21 | the Linux `Events` arm reads it |
| 6 | field `deaf` is never read | `crates/server/src/graph/watcher.rs:216` | lib test | read only in the macOS flush branch (275) | OS-21 | the same |
| 7 | constant `HUNG` is never used | `apps/music2-desktop/tests/appkit_main_thread.rs:28` | test | only the macOS `main` (277) uses these items. On Linux `main` prints "macOS only" (319-322). | OS-30 | now: put them under `#[cfg(target_os = "macos")]`. Later: a Linux twin of the test under GTK's main loop, once the app has a Linux executor (`xvfb-run` is installed here) |
| 8 | function `is_process_main` is never used | `…/appkit_main_thread.rs:115` | test | the same | OS-30, OS-24 | the same |
| 9 | function `pthread_main_np` is never used | `…/appkit_main_thread.rs:117` | test | the same (glibc has no `pthread_main_np`; it links only because it is unused) | OS-30, OS-24 | the same |
| 10 | function `project` is never used | `…/appkit_main_thread.rs:125` | test | the same | OS-30 | the same |
| 11 | constant `A` is never used | `…/appkit_main_thread.rs:147` | test | the same | OS-30 | the same |
| 12 | function `eventually` is never used | `…/appkit_main_thread.rs:150` | test | the same | OS-30 | the same |
| 13 | function `work` is never used | `…/appkit_main_thread.rs:159` | test | the same | OS-30 | the same |
| 14 | function `with_plugins` is never used | `…/appkit_main_thread.rs:241` | test | the same | OS-30 | the same |
| 15 | function `copy` is never used | `…/appkit_main_thread.rs:265` | test | the same | OS-30 | the same |

`music2-desktop`'s own code (`src/`) has no lint on Linux. Neither do `m2-ir`, `m2-song`, `m2-kernel`, `m2-runtime`,
`m2-play`, `m2-script`, `m2-editor`, `m2-gates` or `m2`.

## 3. `cargo test -p music2-desktop` on Linux

| Target | Result | Why |
|---|---|---|
| unit tests (`src/main.rs`) | ok, 0 tests | there are none |
| `tests/appkit_main_thread.rs` (no harness) | ok: prints "appkit_main_thread: macOS only" | the whole test is macOS-only (OS-30), with 9 warnings (§2, 7-15) |

- **It passes, but checks nothing on Linux.** HOST-2 in the app is untested here.
- In the real app on Linux, `plugin_host::install()` fails ("the app's main-thread executor is built for macOS only"),
  so `main` prints "plugins can't be hosted" and goes on (OS-26). Not a test failure; a missing piece.
- Building the app writes an untracked `apps/music2-desktop/gen/schemas/linux-schema.json` (OS-51). Commit it or
  ignore it.

## 4. `cargo build --release --workspace` on Linux

| What | Result |
|---|---|
| Build | ok, 1 m 37 s; `music2-desktop` (47 MB), `m2`, `m2-gates` built |
| Warnings | the same lib warnings as §2: `install`, `dir`, `cpath`, `wake`, `backstop` |
| Linux-only problems | none |

The system libraries the build needs (GTK 3.24.41, WebKitGTK 4.1 2.52.6, ALSA 1.2.11) are installed. Packaging (deb,
AppImage; OS-50) was not tried.

## 5. The ignored tests (release only)

`cargo test` skips 4 tests. I ran the two that check the engine, in release, as they ask:

| Test | Result on Linux | Time here |
|---|---|---|
| `core/play/tests/bit_identity.rs` `live_is_the_export_on_all_of_p16` | ok: live playback is the export, bit for bit | 105 s |
| `core/play/tests/live.rs` `p16_loops_ten_minutes_without_drift` | ok | 117 s |
| `core/runtime` `tests::stretch_quality_report` | not run: it only prints measurements | — |
| `core/kernel/src/lib.rs` doc example (line 33) | not run: marked `ignore` in the docs | — |

So live and export agree on Linux too, at the same commit.

---

## 6. Tests that pass on Linux but check less there

| Test | Why it passes without checking | OS-nn |
|---|---|---|
| `graph::watch_tests::a_case_only_rename_settles_as_one_path` | ext4 is case-sensitive, so the old spelling no longer opens. The bug it guards against cannot happen here. | OS-19, OS-71 |
| `tx::stage_tests::outside_writers_are_caught_at_the_rename` | at f126d03 both of its runs took the fallback; with `renameat2` it tests both paths again | OS-03 |
| `tests::surge_plays_notes`, `tests::airwindows_processes_audio` (m2-plugins), Surge in `main_thread.rs` | they skip when the plugin is not installed, and the Linux CLAP folders (`~/.clap`, `/usr/lib/clap`) are not searched yet; neither exists here | OS-32 |
| `appkit_main_thread` | prints "macOS only" | OS-30 |
| the durable-sync paths (`push`, `barrier`, `sync_dir`) | they work on ext4, but no test can see durability | OS-01, 02, 05 |

## 7. What to build, in order

| Order | Piece | Fixes | Size | Checked |
|---|---|---|---|---|
| 1 | fs: `Real::rename` through `renameat2` | tests 6-9, lint 3 | ~15 lines | yes |
| 2 | threads: `is_process_main` through `gettid() == getpid()`; `run_main` installs through `install` | test 11, lint 1 | ~10 lines | test 11 yes; lint 1 by reading |
| 3 | math: `libm` crate in `audio.rs` (and `resample.rs`); record `renders.json` once | test 10 | ~10 lines | yes on Linux; Mac pending |
| 4 | test support: "crashes on purpose" helper (`PR_SET_DUMPABLE 0`) | kill test 473 s → 104 s | ~10 lines | yes |
| 5 | watch: inotify event source | tests 1-5, lints 4-6 | the largest piece | no |
| 6 | lock: `local_volume` on Linux | lint 2 | ~20 lines | no |
| 7 | desktop: macOS-only items under `cfg`; later the Linux executor and its test | lints 7-15 | small, then larger | no |
| 8 | session (not the platform layer): recover the fallback's middle state; kill test with `plain: true` too | surprise 1 | small to medium | the gap: yes; the fix: no |

## Facts and their sources

- Measured here: every failure line and message above; the clippy list; the desktop test and release build results;
  the scratch runs with the fixes; the abort timings; the glibc and `libm` comparison; `/proc/sys/kernel/core_pattern`;
  `ulimit -c` = 0; inotify `max_user_watches` 997,324 and `max_user_instances` 128; `vm.max_map_count` 1,048,576.
- Read in source: the code paths named with file and line.
- *(unverified)*: that the Mac renders `b3daee50…` with `libm`; how long the kill test takes on the Mac; inotify's
  effect on test 5's event-mode cases.
