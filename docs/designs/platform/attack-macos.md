# Attack on the platform design: macOS safety

Label `attack-macos`. Written 2026-10-14 on `jerry-ax102`, against `design.md` revision 1 and `main` at `f126d03`.
Nothing here ran on a Mac. Every fact was checked in source or measured here, unless it says *(unverified)*.

## In short

Three real problems. The rest of the macOS story holds up (the last section says what was checked).

| # | Problem | Severity | Fix, in one line |
|---|---|---|---|
| 1 | On a Mac, a project on a disk that isn't APFS (an external HFS+ or exFAT drive, a network share) uses fallback code that no check ever runs. One fallback is already broken today. The design's wording would quietly break two more. | high | List macOS's own error codes in the design, put each mapping in one small pure function, test it on both OSes. Fix the barrier's fallback. |
| 2 | The Mac check can hang or be stopped with nothing to paste back. It prints nothing for 10–15 minutes at a time, has no time limit, and doesn't keep the Mac awake. | medium | Print a line when each check starts; on Ctrl-C print the summary anyway; run under `caffeinate`. |
| 3 | Moving `bundled()` behind an OS switch breaks 3 tests on Linux. Fixing them would make them Mac-only, so Linux stops checking how the Mac app finds its files. | low | Leave `bundled()` where it is: it is plain path code, not OS code. |

---

## 1. Fallbacks on non-APFS Mac disks are never tested, and the design invites breaking them

**In short:** on APFS (the Mac's own disk) every special call works, so the tests and the owner's Mac run only take the
"it works" path. On HFS+, exFAT or a network share, macOS answers with error codes the design's text gets wrong.

### What macOS says, and what the design says

| Call (moved in S6) | Today on macOS (`stage.rs`, `lock.rs`) | The design's text |
|---|---|---|
| `fs::rename` Swap/Excl (`renamex_np`) | `ENOTSUP` or `EINVAL` → `Unsupported` (`stage.rs:169`) | OS-03: "`EINVAL`, `ENOSYS`, `EOPNOTSUPP` mean `Unsupported`, **as on macOS**" (`design.md:390`) |
| `fs::clone_file` (`clonefile`) | **any** error except `EEXIST` → copy instead (`stage.rs:192-206`) | "`Unsupported` where the volume can't clone (the caller copies)" (`design.md:135-137`) |
| `fs::try_lock` | `ENOTSUP`, `EOPNOTSUPP` or `ENOLCK` → can't lock (`lock.rs:231-236`) | OS-09: "`ENOTSUP` = `EOPNOTSUPP` (95)" (`design.md:401`) |
| `fs::barrier` (`F_FULLFSYNC`) | on failure, "its own sync": `f.sync_all()` (`stage.rs:141-153`) | "`F_FULLFSYNC` on macOS (its own sync on a volume without it)" (`design.md:127-128`) |

Facts behind the table (checked here):
- On macOS `ENOTSUP` is **45** and `EOPNOTSUPP` is **102** (`libc` 0.2.190, `src/unix/bsd/apple/mod.rs:2557, 2614`). On
  Linux both are 95. So "as on macOS" and "`ENOTSUP` = `EOPNOTSUPP`" are true only on Linux.
- Rust's `ErrorKind::Unsupported` covers only `ENOSYS` and `EOPNOTSUPP` (std 1.99.0, `sys/io/error/unix.rs`). macOS's
  `ENOTSUP` (45) is *not* `Unsupported`.
- On macOS, `File::sync_all` **is** `F_FULLFSYNC` (std 1.99.0, `sys/fs/unix.rs` `fsync`: `fcntl(fd, F_FULLFSYNC)` on
  Apple). So today's "its own sync" repeats the call that just failed. There is no fallback.

### Scenarios

| Situation on a Mac | What goes wrong |
|---|---|
| S6 writes one shared "is this unsupported?" helper in `unix.rs` (§2.1 asks for shared code there), using OS-03's list. A project lives on an exFAT stick. | `renamex_np` gives `ENOTSUP` (45, per the macOS man page *(unverified here)*). The helper doesn't know 45, so every save fails with "Operation not supported" instead of taking the move-aside fallback. |
| S6 maps clone errors with `e.kind() == Unsupported`, as the API doc suggests. A project lives on an external HFS+ drive. A plugin's state changes. | `clonefile` gives `ENOTSUP` (45). That is not `Unsupported` in Rust, so the caller doesn't copy, and the commit fails. Today it copies and works. |
| Today and after S6, unchanged: a project on a network share where `F_FULLFSYNC` is refused (SQLite's `os_unix.c` falls back to `fsync` for exactly this case on macOS *(which shares: unverified)*). | `barrier` fails twice and every commit fails. The design's S7 claim that "the Mac on exFAT or SMB" now recovers from a stopped save doesn't help: the save never gets that far. |

**Why nothing catches it.** The macOS lint type-checks the code but can't run it. Linux's error numbers hide the
difference. The owner's Mac run uses APFS temp folders, where every call succeeds. The tests' fault injection sits
above `Real`, so it never feeds a real macOS error code through the mapping.

### Fix (small, and mechanical on macOS)

1. In `design.md`, write each macOS list as today's code has it: rename `ENOTSUP | EINVAL`; clone: every error except
   `EEXIST`; lock `ENOTSUP | EOPNOTSUPP | ENOLCK`. Fix OS-03's "as on macOS" and OS-09's "=". Say: never use
   `ErrorKind::Unsupported` to test for these.
2. Each mapping is one pure function in the OS folder, taking an error number (for example `mac::fs::rename_unsupported(code)`).
   One portable test in `core/os` feeds it `libc::ENOTSUP`, `libc::EOPNOTSUPP`, `libc::EINVAL` and `libc::EEXIST` by
   name. On Linux it runs today; on the Mac it runs in the owner's check, with 45 and 102.
3. `mac/fs.rs` `barrier`: when `F_FULLFSYNC` fails, call `fsync` (that is `push`), as SQLite does. One line. Record it in
   the step's commit as a deliberate macOS change. (Or return `Unsupported` and let `stage.rs` push, to match PLAT-3.)
4. Optional, if the owner agrees: on macOS, `ci/check.sh` makes a small exFAT disk image (`hdiutil create -fs ExFAT`,
   `hdiutil attach -nobrowse`; no admin rights needed *(unverified)*) and runs one `core/os` test there that calls each
   `fs` function and accepts only `Ok` or `Unsupported`. That turns these fallbacks from "never run" into "run in every
   Mac check".

---

## 2. The Mac check can hang, or be stopped, with nothing to paste back

**In short:** the Mac run exists to find Mac-only bugs in moved code. The typical such bug (FSEvents flush, the main
dispatch queue, the run loop) is a hang, not a clean failure. The script as drafted handles a hang badly.

| What the draft does (`design.md` §7.1) | Effect on the owner's run |
|---|---|
| `quiet` sends all output to logs; `record` prints a line only when a check **ends** | Silent stretches of 10–15 minutes (cold build, then every Rust test). A hang looks the same as slow work. |
| No time limit on `cargo test` (libtest never stops a test; macOS has no `timeout` command) | A deadlock in a moved FSEvents or main-loop test runs forever. Only the AppKit test has its own 120 s limit. |
| The only trap is `rm -rf "$SCR"` on EXIT (`design.md:992`) | If the owner presses Ctrl-C, nothing is printed. The results so far sit in `~/m2-check/<commit>/results`, but the owner isn't told. |
| No sleep prevention | A 30–60 minute run on a MacBook can hit idle sleep (cargo holds no power assertion). Timed waits in tests and suites (10 s `eventually`, 5 s watcher waits, suite timeouts) can then fail for no real reason. |

**Scenario:** S8 moves FSEvents. A lock-order mistake makes `flush()` wait on the queue while `heard` waits for the
hints lock. On the Mac, `graph::watch_tests` hangs. The owner sees "build PASS" and nothing more for an hour, presses
Ctrl-C, and has nothing to paste. The lead learns only "it hung", and needs another full Mac run to learn where.

**Fix (bash 3.2 safe, about 15 lines):**
- Print a line when each check **starts**: `rust tests   running (usually 10 min, started 14:02)`.
- `trap` on INT and TERM: record the running check as `FAIL "stopped after N min (hung?)"`, then print the normal
  summary, so there is always something to paste.
- Add to "what failed" the names libtest prints in `test.log` as `test … has been running for over 60 seconds`.
- On macOS, re-run the script under `caffeinate -i` once (guarded by an environment variable). In §1's table add:
  "Keep the Mac plugged in with the lid open; if a line says `running` for over 30 minutes, press Ctrl-C once and paste
  what it prints."

---

## 3. `bundled()` behind an OS switch loses Linux's check of the Mac app's file lookup

**In short:** `bundled()` is plain path code that already works on every OS. The design moves it into `m2-os` and makes
it return `None` on Linux (OS-39, `design.md:481`). Three existing tests then fail on Linux.

- Today `crates/server/src/resources.rs:55-60` maps `X.app/Contents/MacOS/prog` to `X.app/Contents/Resources/<name>`.
  It has no `cfg` and no `libc`, so PLAT-2 doesn't require moving it.
- Its tests build a fake `music2.app` in a temp folder and run on Linux today:
  `the_app_bundle_finds_its_resources_next_to_the_program` (`resources.rs:101`),
  `an_override_wins_and_an_empty_one_is_ignored` (`:112`, its second check) and
  `a_symlinked_program_finds_the_bundle_it_points_into` (`:129`).
- With `dirs::bundled` returning `None` on Linux, all three fail. The easy repair is `#[cfg(target_os = "macos")]` on
  them. Then the only automated check of how the Mac app finds `std`, `templates` and `examples` runs only in the
  owner's rare Mac run, and no Mac check ever builds a real `.app`.

**Fix:** leave `bundled()` in `resources.rs`, unchanged, and drop `dirs::bundled` from §2.2. It is safe on Linux, since
a Linux program never sits in `X/Contents/MacOS/`. The Linux packaging step can add `<exe>/../lib/music2/<name>` as a
second candidate in the same function, with its own test. This makes S6 smaller.

---

## What was checked and holds

| Claim | How it was checked | Result |
|---|---|---|
| `std.wasm` rebuilds to the same bytes on a Mac (PLAT-5) | Cargo's `-C metadata` could include rustc's `host:` line. Built std in the S3 layout (`m2-kernel` inside the kernels' workspace) twice here: once normally, once through a `rustc` wrapper whose `-vV` says `host: aarch64-apple-darwin` (`~/m2-runs/logs/am-stdhost-*.log`) | Same metadata, same file names, same bytes (`909f4881…`). Cargo leaves the host out for wasm32 units. The file holds no machine path, only `kernel/src/lib.rs` and `/rustc/<hash>/…`. Whether rustc itself emits the same bytes on an arm Mac stays *(unverified)* until the Mac run. |
| `libm` gives the same bits on x86_64 and aarch64 | Read `libm` 0.2.16's source for `target_arch` and `target_feature` | Only exact operations differ per CPU (`sqrt`, `fma`, `rint`, `floor`…, all correctly rounded). The functions S4 uses (`cosf`, `sinf`, `powf`, `exp2`, `sin`, `cos`) run the same Rust on both. The predicted Mac hashes are well founded. |
| The Mac check's renders use the debug `m2`, the recorded hashes came from a release one | Read §5.1 and `audio.rs` | Fine after S4: `libm::exp2` is called by name, so LLVM can't swap `pow` and `exp2` between builds. All dependencies are built at opt-level 3 in dev (`Cargo.toml`), so `libm` is not slow on the audio thread in debug runs. |
| Wasm floats on x86_64 and arm | Read the design and wasmtime's settings | Holds: no fused multiply-add outside relaxed SIMD (off), IEEE everywhere. Only NaN sign and payload can differ (NaN canonicalization off), and the renderer silences non-finite samples. |
| The macOS lint covers the desktop app and the gates | Read `dsh-cross-macos-clippy.log` | Yes. Cargo prints one line per package, so `Compiling m2-gates` and `Compiling music2-desktop` (build scripts) cover their lints too. The run exited 0 with `-D warnings`. |
| The AppKit test still checks the Mac after it becomes `app_main_thread.rs` | Read the test | Yes, if each `process_main` check compares with `APP_ON_PROCESS_MAIN` (true on macOS). A forgotten `harness = false` would leave an unused `main`, which clippy `-D warnings` catches on Linux. |
| `cargo test` with a scratch `HOME` on the Mac | Searched the tests for `HOME`, git and `Library` | No test needs the real `HOME`. The AppKit test sets its own. `CLAP_PATH` is exported, so plugin tests still find the plugins. |
| The script's macOS-only lines (bash 3.2, BSD tools) | Read every line | No bash 4 features. `wc` padding is stripped. `shasum -c`, `mktemp -d`, `date +%s`, BSD `grep -o` and `tar --exclude` all work on macOS *(not run on a Mac)*. |

Not a problem for this step, noted for later: timing gates (PLANE-5, gate 8, step 7's G7a–G7f) can only be judged on the
reference Mac, and the design gives no one-command way for the owner to run them. Step 7's and 8a's designs need one.
