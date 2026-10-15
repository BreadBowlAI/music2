# The platform layer: build notes

What each slice built, where it left the design, and what its check said. The design is `design.md` (revision 2, §13
for the slices, §16 for the lead's notes).

## S1. The check and the records

**Built**

| What | Where |
|---|---|
| The full check, the same script on both OSes, with the Linux-only app row (§16) | `ci/check.sh` |
| The two stubs for the macOS lint | `ci/stub-cc`, `ci/stub-ar` |
| The gates the Mac run times | `ci/gates.txt` |
| The test plugins, installed without sudo from kept archives, and their hashes | `ci/plugins.sh`, `ci/plugins.sha256` |
| The screenshot tool the app row builds on the fly | `ci/xshot.c` |
| The pinned toolchain | `rust-toolchain.toml` |
| `std.wasm` built into `CARGO_TARGET_DIR`; `--check` rebuilds a copy elsewhere and compares | `std/build.sh` |
| The Mac's 8 render hashes, the one record both OSes use; the eighth case | `e2e/renders.sha256`, `e2e/baseline.sh` |
| Linux's Tauri schema, so a Linux build leaves the tree clean (OS-51) | `apps/music2-desktop/gen/schemas/linux-schema.json` |
| Package modules named `std/src/dsp.ts`, not by their absolute path (attack 18) | `core/script/src/lib.rs` |
| The acceptance-gates text, the Platforms section (PLAT-1 to PLAT-7), the order-of-work entry 6b | `docs/architecture.md` |

The plugin archives were copied (not moved) from `~/m2-runs/plugins/dl/` to `~/.local/share/m2-test-plugins/dl/`, and
`ci/plugins.sh` unpacked them there; both `.clap` files match `ci/plugins.sha256`.

The long-path test: `files::tests::diagnostics_say_where_in_the_project_each_problem_is` in a checkout whose path is
81 characters fails before the change ("in module '/home/jerry/m2-runs/scratch-build-S1/a-checkout-path-over-sixty'")
and passes after it.

**Deviations**

| What | Why |
|---|---|
| `panicked()` in `ci/check.sh` matches `thread 'x' (1234) panicked at` | Rust 1.99 prints the thread's id after its name. The draft's pattern never matched, so "what failed" would have named no panic line. |
| "What failed" gives a suite's first `FAIL` line, or else the error that stopped it | A suite that throws (attention, at S1) prints no `FAIL` line, so the draft printed an empty line for it. |
| `std/build.sh --check` ends with "the rebuild in another folder failed" when cargo fails | The check shows the log's last line as the row's note; cargo's last line alone ("No such file or directory") says nothing. |
| `display()` joins a package file's parts with `/` | The module name is now what `home()` reads; `/` keeps it the same on every OS (the old `home()` replaced `\` itself). |
| The app row: Xvfb picks a free display itself (`-displayfd`); the screenshot is taken 5 s after the UI connects; a Linux without Xvfb shows the row as SKIP | §16 left these open. A fixed display number could clash with another run on the server. |
| The app row's "never says plugins can't be hosted" is a note in the row until S5, not a failure | As the task and §13.1 say: the Linux app can't host plugins until S5. S5 turns the note into a failure. |
| The check's steps are renumbered (the app row is step 4) | It runs right after the build, as §16 says. |

**The check** at `a504a60` (`ci/check.sh`, 13 min, logs and the app's screenshots in `~/m2-runs/check-build-S1/`):
exactly §13.1's S1 row.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected in 1 s with `first-song` and with `with-plugins`; the app can't host plugins yet (S5) |
| rust tests | FAIL: the 11 of §4.1 by name (10 tests, and the `main_thread` binary); 686 passed |
| clippy | FAIL at its first lint, `core/plugins/src/main_thread.rs:217`. Without `-D warnings` it lists exactly §4.2's 15 lints. |
| std kernels | PASS (tests and clippy) |
| std.wasm | FAIL: stale until S3; the repo untouched |
| mac lint | PASS |
| browser suites | FAIL: the 7 of §4.4 (code, notes, fixes-graph, attention, export, fx-fixes, s6-write), each again on its retry; live3b in full, 51 of 51, with the plugins |
| renders | FAIL: 6 of 8 match the Mac's record (the two `examples-resample` renders differ, as before S4) |
| repo unchanged | PASS |

A trial run just before (while this file was being written, so its "repo unchanged" failed) also saw two known
flakes, each passing on its retry: `live3a` ("dragging across the ruler loops beats 4–12", HANDOFF's ruler-drag flake)
and `notes-proj` (the file-then-revision race S2 fixes). Its logs: `~/m2-runs/check-build-S1-trial.tgz`.

## S2. The browser suites

**Built**

| What | Where |
|---|---|
| The pinned Chrome for this OS (OS-73); `run.mjs` takes it from there | `e2e/chrome.mjs`, `e2e/run.mjs` |
| Off the Mac, Chrome tells pages it is a Mac, so Monaco takes the Mac's keys (OS-75, Q2) | `e2e/chrome-as-mac.sh` (mode 755) |
| `isMac` follows the platform a page is given (OS-74): the ⌘E tooltip in the browser suites | `web/src/lib/native.ts` |
| A new song's folder name is taken in any capitals, on every OS (OS-41, Q1), with a unit test | `crates/server/src/project.rs` (`make()`, `a_name_taken_in_other_capitals_is_taken`) |
| Wait for the revision to move before counting it (the file-then-revision race) | `e2e/notes.mjs:97`, `e2e/s6-fixes.mjs:112` |
| How the browser is picked, in two sentences | `e2e/README.md` |

The new unit test fails without the rule ("exports 2" is made beside "Exports 2") and passes with it. Before the full
check, `run.mjs all` with no `E2E_CHROME` (so the new Linux default) gave 29 of 29 ok, 1,034 of 1,034 checks.

**Deviations:** none. Left as the design has it: `create_at` (`m2 new <dir>`) still makes exactly the folder it is
given, so on Linux it makes `exports 2` beside `Exports 2`. Q1's rule is for `make()`, New Song's numbering.

**The check** at `c60372a` (`ci/check.sh`, 17 min, logs in `~/m2-runs/check-build-S2/`): exactly §13.1's S2 row.

| Row | Result |
|---|---|
| toolchain, web build, build, app | PASS (the app as at S1: it can't host plugins yet, S5) |
| rust tests | FAIL: the same 11 of §4.1 (10 tests, and the `main_thread` binary); 687 passed, one more than S1 (the new test) |
| clippy | FAIL at its first lint, `core/plugins/src/main_thread.rs:217`. Without `-D warnings` it lists exactly §4.2's 15 lints. |
| std kernels, mac lint | PASS |
| std.wasm | FAIL: stale until S3; the repo untouched |
| browser suites | PASS: 29 of 29, live3b in full with the plugins. One run passed on its retry, below. |
| renders | FAIL: 6 of 8 (the two `examples-resample` renders, until S4) |
| repo unchanged | PASS |

The retry: `s4-meter-fix` crashed once reading `data/hats/hits.json` while the server saved it (`ENOENT`). That is the
file that goes missing for a moment during each save on Linux until S6 (OS-03, `ground/e2e.md`), the race §13.1 names
for live3a. Not this slice: S2 changes nothing on that path, the run passed 8 of 8 on its retry, and in the run before
the check it passed too.

## S3. `std.wasm` rebuilds the same

**Built**

| What | Where |
|---|---|
| The kernel kit moved into the kernels' workspace (`git mv`), with plain `version`, `edition` and `license` | `std/kernels-src/kernel/` (was `core/kernel/`) |
| The two paths to it | root `Cargo.toml` (`m2-kernel`), `std/kernels-src/Cargo.toml` |
| `std.wasm` rebuilt once, on purpose: `83cc5961…` (143,471 bytes) to `909f4881…` (143,431 bytes). It holds no machine path now, only `kernel/src/lib.rs`, `src/fx.rs` and rustc's own paths. | `std/kernels/std.wasm` |
| PLAT-5's *Today*; ABI-1's path to the kit | `docs/architecture.md` |

Both lock files stay as they were (`--locked` passes): a path package's entry has no path in it.

**What it changes.** Every std kernel's module hash changes once (the commit says so, PLAT-5). No render changes:

| Proof | Before (S1) | After |
|---|---|---|
| The renders.json test's printed record | `examples/resample` `e2e93ef0…`, the other three as recorded | the same four hashes (the test still fails on resample: test 10, S4's) |
| `bit_identity` | 3 pass, 1 ignored | the same; all 4 also pass in release with the ignored one included (106 s) |
| The 8 offline renders | 6 of 8 match the Mac's record | the same 8 files, byte for byte (resample `d902bacf…`, from4 `ea243ccc…`, as at S1) |
| `std/build.sh --check` | FAIL: its copy lacked `core/kernel` | PASS. The same bytes in three folders: the worktree, `/tmp/m2-std.*`, and a longer scratch path with another target folder |

`909f4881…` is also what `attack-macos` built in this layout, with a `rustc` that says it is an arm Mac.

**Beyond the design's list** (small; each keeps the tree true after the move)

| What | Why |
|---|---|
| README's layout table and `docs/song-as-program.md` name the kit at `std/kernels-src/kernel` | They named `core/kernel`, which no longer exists. Neither line is near a hunk of `taste`, `sounds` or `wip/step7`. |
| `std/build.sh`'s header loses "Until then --check fails" | It is no longer true. |
| A two-line "why" above `m2-kernel` in `std/kernels-src/Cargo.toml` | So nobody moves it back out of the workspace. |

**For the merge with S2, S4 and S5.** This slice shares only `docs/architecture.md` (ABI-1 and PLAT-5, one line
each) and this file with them. The design's §5.1 names `core/kernel/src/lib.rs:235` as host math left alone; it is
`std/kernels-src/kernel/src/lib.rs:235` now. `one_place.rs` (S5) reads `core/`, which no longer holds the kit; the kit
has no OS code.

**The check** at `319c804` (`ci/check.sh`, 22 min while three other slices' checks ran; logs in
`~/m2-runs/check-build-S3/`): §13.1's S1 row with the std.wasm row now PASS, and one flake.

| Row | Result |
|---|---|
| toolchain, web build, build, app | PASS (the app's UI connected in 1 s, both songs) |
| rust tests | FAIL: the 11 of §4.1 by name, as at S1. Also `tests::cell::cell_heard_never_runs_ahead` (m2-runtime) failed once, "drained while writing", and passed on its retry. A flake: see below. |
| clippy | FAIL at its first lint, `main_thread.rs:217`. Without `-D warnings` it lists exactly §4.2's 15 lints. |
| std kernels | PASS (tests and clippy, `m2-kernel` now among them) |
| std.wasm | **PASS**: rebuilt to the same bytes in another folder |
| mac lint | PASS |
| browser suites | FAIL: 6 of §4.4's 7, each again on its retry. `notes-proj` passed this time (its race, which S2 fixes); live3b in full, 51 of 51 |
| renders | FAIL: 6 of 8 match, the same 8 files as at S1 |
| repo unchanged | PASS |

The cell test's flake: its last check asks that the reader drained more than once while the writer wrote 200,000
cells. The load average was about 36 to 43 on 32 threads then (four slices' checks at once), so the writer can finish before the
reader gets a turn. This slice changes no code it runs (`CellSlots` in `m2-runtime`). Re-run afterwards: 50 of 50
alone, and m2-runtime's whole test binary 10 of 10. No earlier log kept here shows it failing.

**The independent check** (label `check-S3`, at `e891a0b`; logs in `~/m2-runs/check-check-S3/`)

| What | Result |
|---|---|
| The diff against §13.2's S3 row, §5.4, ABI-1 (§6.1) and PLAT-5 (§6.2) | All there. Nothing else changed beyond the three small edits above: no test, gate, rule or macOS code. `core/kernel` is named nowhere outside `docs/designs/`. |
| `ci/check.sh` (13 min) | §13.1's S1 row with the std.wasm row PASS. Rust tests: the 11 of §4.1 by name (686 passed). Clippy stops at lint 1; without `-D warnings` it lists the 15 lints of §4.2. Suites: 6 of §4.4's 7 (`notes-proj` passed: its race, S2's); live3b 51 of 51. Renders 6 of 8, the same 8 files as the builder's run (resample `d902bacf…`, from4 `ea243ccc…`, as in `ground/facts.md`). The renders.json test prints resample `e2e93ef0…` and the other three as recorded. The cell test passed. |
| `std/build.sh --check` in a second checkout (a longer path, `TMPDIR` under `/home`), and a rebuild in place there into another target folder | The committed bytes each time, with no `/home/` path in them. One line added to `kernel/src/lib.rs`, or one byte changed in `std.wasm`, makes `--check` fail. |
| The old and new `std.wasm` | The same 176 exports and the same imports. |
| The kernels' clippy in a fresh target folder, for the host and for `wasm32-unknown-unknown` | Clean. |
| `bit_identity` in release, the ignored test included | 4 of 4 (94 s). |

**Fixed by the check** (a follow-up commit)

| What | Why |
|---|---|
| `std/build.sh`'s `build()` turns a relative target folder into a full path before it runs cargo | cargo runs inside `std/kernels-src`, so it built into `std/kernels-src/<folder>` while `cp` read `<folder>` from where the script was run. `CARGO_TARGET_DIR=target std/build.sh` failed. Worse: when that folder already held an older `m2_std_kernels.wasm`, the script copied the old file over `std.wasm` and said nothing (shown in a scratch copy with a planted file). A relative `TMPDIR` made `--check` fail the same way. Now all of them build the committed bytes; `--check` still fails on a changed byte. |

**The check again** at the fix (`bd608d7`, 14 min, logs in `~/m2-runs/check-check-S3-fix/`): the same rows. All 7 of
§4.4's suites failed this time; `notes-proj` passed on its retry. Renders: the same 8 files. One new flake, not this
slice's: `m2-plugins`' `tests::surge_plays_notes` failed once ("nothing before the note's sample offset") and passed
on its retry. Its code is the same as at S1. Run alone it fails 6 times in 150, and the whole test binary 4 in 60:
in those runs Surge XT's output starts a few samples early, at 1e-14 to 3e-8 (below −140 dB), where the test asks
for exact zeros. Left for the lead (S5 changes the plugin host).

## S4. The same math everywhere

**Built**

| What | Where |
|---|---|
| The audio clip path's 8 math calls through `libm`; pitch is `libm::exp2` by name | `core/runtime/src/audio.rs` |
| The live resampler's 3 calls through `libm` | `core/play/src/resample.rs` |
| Song code's 22 `Math` functions as native functions over `libm`, with JS's `pow` and QuickJS's `hypot` fold | `core/script/src/lib.rs` (`math`) |
| `**` and `**=` lowered to `Math.pow` (oxc's `env.es2016.exponentiation_operator`) | `core/script/src/lib.rs` (`strip`) |
| The test that bans host math in what `m2-runtime` and `m2-play` compile | `core/runtime/tests/host_math.rs` |
| The test that song math gives `libm`'s bits: 31 values, and one constant in the IR | `core/script/src/tests.rs` (`song_math_is_libm_on_every_os`) |
| `libm = "0.2"` (0.2.16, already in `Cargo.lock`) | the root `Cargo.toml`, used by `core/{runtime,play,script}` |
| RENDER-2's rule and *Today*; PLAT-4's *Today* | `docs/architecture.md` |
| The three changed hashes, in their own commit | `core/session/tests/renders.json`, `e2e/renders.sha256` |

**Measured**

- `host_math.rs` before the change found exactly design §5.1's 11 calls (8 in `audio.rs`, 3 in `resample.rs`); after it,
  none.
- The QuickJS test before the change: 23 of its 31 values were wrong on Linux (glibc's last bit). With only the `**`
  lowering left out: 2 wrong, the two `**` cases. So the test catches either half missing.
- oxc's `**` option changes no other output: all 38 tracked `.ts` files outside `web/` (the SDK, std, examples,
  templates, test fixtures) compile to the same JS with it on and off (a temporary test, not committed). This settles
  design §12's row on it.

| Record | Entry | Before (the Mac at `8aa301e`) | After S4 (Linux) |
|---|---|---|---|
| `renders.json` (the fixture test) | `examples/resample` | `cb4d39ba…` | `b3daee50ee7e7193fc6d240f02384fcda83ccbe33b5c20fbf5904005b4aeb58e` |
| `e2e/renders.sha256` | `examples-resample.wav` | `6ac6fcb1…` | `39f9476492e997a86aec5f94b6d0edf56c098e275186260915a3830bd9cfe0da` |
| same | `examples-resample-from4-for6.wav` | `a360d04c…` | `d752521b47b66bcb9076f43715baedaa5c018f428ec90546465e6cae031560eb` |

The debug and the release `m2` render the same 8 WAVs; the other 6 are unchanged. All three equal design §5.3's
prediction. The Mac must give the same three (unverified until the owner's run).

**The two libm hashes in the ground notes (§16).** They don't disagree: they hash different things. `tests.md`'s
`b3daee50…` is the fixture test's hash of a debug render's samples (`renders.json`). `facts.md`'s `39f94764…` is the
sha256 of the WAV file `m2 render` writes (`e2e/renders.sha256`). The final code gives both.

**Deviations**

| What | Why |
|---|---|
| `host_math.rs` follows each crate's module tree from `lib.rs` and blanks `#[cfg(test)]` items, instead of reading every file under `src` | `core/runtime/src/tests.rs` and `tests/` are test code only through `#[cfg(test)] mod tests;` in `lib.rs`, and hold host math (`tests.rs:599`). Following the module tree leaves exactly them out. Its brace matching skips strings, characters and comments. |
| It also bans the `f32::sin(x)` and `f64::sin(x)` forms | The method form alone would miss them. |
| `libm` is a workspace dependency | The repo's way for a crate several members use; one line in the root `Cargo.toml`. |

**Shared files** (S2, S3 and S5 are built beside this): `Cargo.toml` (one line after `libc`), `Cargo.lock` (`"libm"`
in three dependency lists), `docs/architecture.md` (RENDER-2's rule and *Today* lines, PLAT-4's *Today* line), and
this section.

**The check** at `01edbf1` (`ci/check.sh`, 18 min, logs in `~/m2-runs/check-build-S4/`): §13.1's S1 row changed only
by S4, as asked.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS, as at S1: the UI connected with `first-song` and `with-plugins`; the app can't host plugins yet (S5) |
| rust tests | FAIL: the 10 of §4.1 that S4 leaves, by name (9 tests, and the `main_thread` binary); 689 passed (S1's 686, test 10, and the 2 new tests). `fixture_renders_change_only_with_stds_version` passes. |
| clippy | FAIL at its first lint, `core/plugins/src/main_thread.rs:217`, as at S1. Without `-D warnings` it lists exactly §4.2's 15 lints. |
| std kernels | PASS |
| std.wasm | FAIL, as at S1: the rebuild in another folder can't find `core/kernel` until S3 moves it; the repo untouched |
| mac lint | PASS |
| browser suites | FAIL: 6 of S1's 7 known (code, fixes-graph, attention, export, fx-fixes, s6-write), each again on its retry. `notes-proj` passed this time: its known file-then-revision race (S2 fixes it) passed by luck. live3b in full, 51 of 51. |
| renders | **PASS: 8 of 8** match `e2e/renders.sha256` |
| repo unchanged | PASS |

One known flake, named: `s4-clips-fix` crashed once and passed 28 of 28 on its retry. It read `data/kick/hits.json` in a
poll loop while a save had renamed it aside (`ENOENT`): the missing-file gap of `ground/e2e.md` (row "any suite that
reads a project file in a poll loop", which names s4-clips), which S6 closes. Not this slice's code: S4 changes only
math. Run alone 10 more times on this build: 9 passed in full, 1 crashed the same way.

**Independent check** (label `check-S4`, at `fa09cf0`; `ci/check.sh`, 13 min, logs in `~/m2-runs/check-check-S4/`):
§13.1's S1 row changed only by S4. Rust tests: the 10 S4 leaves (9 tests and the `main_thread` binary), 689 passed,
`fixture_renders_change_only_with_stds_version` passes. Clippy: lint 1 first; without `-D warnings` exactly §4.2's 15,
none in S4's crates. std kernels PASS, std.wasm FAIL (S3), mac lint PASS. Browser suites: 23 of 29, the 6 key and
case suites S2 owns failing; `notes-proj` passed (its race, as in the builder's run); live3b 51 of 51. Renders 8 of 8.
Repo unchanged. A release `m2` also renders the 8 to `e2e/renders.sha256`.

Probes: the QuickJS test fails with 23 wrong when both halves are off and with the 2 `**` cases when only the lowering
is off; `host_math.rs` fails when the WSOLA `cosf`, the gain `powf`, the pitch `powf` or a resampler `cos` is put back,
and passes with host math inside `#[cfg(test)]`.

| Fixed in `host_math.rs` (a follow-up commit) | Why |
|---|---|
| Bans `.asinh(`, `.acosh(` and `.atanh(` too | Rust 1.99's `asinh` and `acosh` call the OS's `asinh` and `acosh`, and `atanh` its `log1p` (read in std's source). Before, `(0.5f32).asinh()` in `audio.rs` passed the test. |
| Bans `f32::sin` and the rest by path without a bracket | `.map(f32::sin)` calls the OS's `sin`, and passed. Clippy's `redundant_closure` turns `.map(\|x\| f32::sin(x))` into that form. |
| Fails when a `.rs` file under `src` is not in the module tree | A `mod` line the parser didn't know made the test skip the file in silence: with `mod resample; // x` or `pub(super) mod resample;`, a `cos` put back in `resample.rs` passed. The parser now takes any visibility and a comment, and a form it still can't read fails the test. |

Reported, not fixed (design choices or bigger than a fix): song code's `**` on BigInt variables now throws (`2n ** 3n`
with literals still works; oxc lowers only the others to `Math.pow`); `**` inside `eval` or `new Function` skips the
lowering and uses the OS's `pow`.

The check again at `00ff1c7` (the fix; logs in `~/m2-runs/check-check-S4-fix/`): the same rows, plus one flake that
passed on its retry: `tests::cell::cell_heard_never_runs_ahead` ("drained while writing": the writer finished before
the first drain, while other checks loaded the server). S4 doesn't touch it.

## S5. `m2-os`, and the plugin main thread

**Built**

| What | Where |
|---|---|
| The crate `m2-os`: the API in portable files, one folder per OS, what macOS and Linux share in `unix.rs` | `core/os/src/` |
| Thread identity and CPU time: `token` (`pthread_self`), `is_process_main` (macOS `pthread_main_np`, Linux `gettid() == getpid()`), `cpu_time` (OS-24, 25, 80) | `thread.rs` |
| The plugin main loop: `Where`, `Loop`, `APP_ON_PROCESS_MAIN`. macOS: the main dispatch queue and the `CFRunLoop` pump, moved (OS-23). Linux: a job queue; the app's runs on a thread of its own, `m2-app-main` (OS-26, 27) | `main_loop.rs`, `mac/main_loop.rs`, `linux/main_loop.rs` |
| `dirs::home` and `dirs::clap`; plugins are found on `CLAP_PATH`, then in the OS's CLAP folders (OS-32) | `dirs.rs`, `core/plugins/src/lib.rs` |
| The plugin executor over a `Loop`, with no OS `cfg` left; `token` and `is_process_main` kept as wrappers; `use_process_main_thread` is now `use_app_main_thread` | `core/plugins/src/main_thread.rs` |
| The app's hook, and its test on every OS, renamed `app_main_thread.rs`; only AppKit's run loop stays macOS-only (OS-30) | `apps/music2-desktop/src/plugin_host.rs`, `tests/app_main_thread.rs` |
| The gates' thread CPU time from `m2-os`; `m2-gates` no longer needs `libc` | `crates/gates/src/server/watch.rs` |
| PLAT-2's test, with an allow-list of 11 files (8 for S6, 3 for S8) | `core/os/tests/one_place.rs` |
| The app row fails when the app says plugins can't be hosted | `ci/check.sh` |
| HOST-2, HOST-3, and PLAT-2's and PLAT-3's *Today* | `docs/architecture.md` |

**Deviations**

| What | Why |
|---|---|
| `dirs::home()` is built here, not in S6 | `dirs::clap()` needs it. S6 adds `songs` and `cache` beside it. |
| `one_place.rs` ignores comments, also matches `cfg!(unix)` and `cfg!(windows)`, skips modules declared under `#[cfg(test)]` (`oracle.rs`, `props.rs`, `testing.rs`), and fails when a file on the allow-list names no OS any more | A comment that names `libc::` is not OS code. The last rule makes each slice remove its own lines, as §2.8 says. |
| Linux's `AppMain` loop starts on any thread; macOS's refuses to start off the main thread | Only on macOS is that loop the main thread. The app calls it on the main thread anyway. |
| `run_main` now checks it runs on the process's main thread on Linux too | macOS always did; every caller is a `main`. |

**The check** at `9d200a4` (`ci/check.sh`, 17 min, logs and screenshots in `~/m2-runs/check-build-S5/`): S1's row, changed only
by S5, plus two flakes that are not S5's (below).

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected in 1 s with `first-song` and with `with-plugins`; nothing says plugins can't be hosted |
| rust tests | FAIL: tests 1 to 10 of §4.1 by name, and the G10 flake below; 694 passed. Test 11 passes, Surge XT included (`main_thread: ok (Surge XT too)`), and so does the app test on Linux (`app_main_thread: ok (… 32 main-thread calls on the app's loop thread …; with-plugins played, carried across a rebuild and closed on the app's loop thread)`). |
| clippy | FAIL at lint 2 (`core/session/src/lock.rs:213`): it gets past `m2-plugins` now. Without `-D warnings` it lists exactly lints 2 to 6. |
| std kernels | PASS |
| std.wasm | FAIL: stale until S3 |
| mac lint | PASS |
| browser suites | FAIL: 6 of S1's 7 (code, fixes-graph, attention, export, fx-fixes, s6-write); `notes-proj` passed (below); live3b in full, 51 of 51 |
| renders | FAIL: 6 of 8 (the two `examples-resample` renders, until S4) |
| repo unchanged | PASS |

| Flake | Evidence that it isn't S5's |
|---|---|
| `the_live_engine_passes_every_gate`: G10's "bar 64 takes no longer than the same length from bar 1" said 1.48 s against 1.31 s | A timing check, run while other slices' checks loaded the server (load average 12 to 17). It passed on the check's own retry, and 3 of 3 times more alone. S5 changes nothing on the render path. |
| `notes-proj` passed, where S1's check failed it twice | The file-then-revision race S2 fixes (§4.4) depends on timing. It passed 5 of 5 times alone on this build, and S3's check, which doesn't touch saves either, passed it too. |

**The Linux app with plugins.** Opened once more by hand on Xvfb with `examples/with-plugins` and the test plugins: its
song has no problems (S1's screenshot showed two `plugin.missing`), and it plays. Screenshots in `~/m2-runs/check-build-S5/`:
`app-first-song.png`, `app-with-plugins.png`, `app-with-plugins-playing.png`.

**The independent check** (label `check-S5`, at `eefa929`; logs in `~/m2-runs/check-check-S5/`)

| What | Result |
|---|---|
| The diff against §13.2's S5 row | All of the row is there, and nothing else. No test is weaker: the app test checks the same on macOS, and now also the thread. No OS code outside `core/os` but the 11 files on the allow-list and G11's `refused`. |
| `ci/check.sh` | S1's row changed only by S5. app PASS. Rust tests: tests 1 to 10 of §4.1 fail, 695 pass; test 11 and the app test pass with Surge XT. clippy stops at lint 2; without `-D warnings` it lists exactly lints 2 to 6. std kernels PASS, std.wasm FAIL (until S3), mac lint PASS. Suites: S1's 7, with `notes-proj` passing on its retry (S2's race, "the whole drag is one write"); live3b 51 of 51. Renders 6 of 8. Repo unchanged. |
| Test 11 and the app test with the test plugins | 5 and 3 runs more: all pass. |
| The Linux app on Xvfb with `examples/with-plugins` | Both plugin libraries are loaded in the app's process, and its `m2-app-main` thread runs. Without `CLAP_PATH`, none is loaded. |
| The mac lint | A type error and a missing function put in `core/os/src/mac/` pass Linux's check and fail the mac lint. |
| `one_place.rs` | It fails on `target_os`, `$HOME`, an `extern "C"` block outside `refused`, and a file left on the allow-list with no OS code; comments and test modules don't count. It missed a `cfg` with an OS inside a longer condition: fixed below. |
| 32 threads calling `test_main_thread()` at once, 20 times | One loop thread is left, and it serves. |

| Fixed | Why |
|---|---|
| `one_place.rs` also fails on a `cfg`, `cfg!` or `cfg_attr` whose condition names an OS anywhere (`unix`, `windows`, `target_family`, `target_vendor`), with a test | PLAT-2 says "no `cfg` on the OS", but the test knew only the exact `cfg(unix)`, `cfg(not(unix))` and `cfg(windows)`. `#[cfg(all(unix, not(test)))]` or `#[cfg(not(windows))]` outside `m2-os` passed. Today's code passes the stricter test as it is. |

Left as they are, for the lead:
- `apps/music2-desktop/src/main.rs` still says the plugins' main thread is AppKit's, which is true on macOS only. §2.3 keeps
  this step out of that file.
- The app row passes when the app finds no plugins; only "can't be hosted" fails it, as §16 says. The probe above is what
  shows they load.
- On macOS, `run_main` after `use_app_main_thread` is now an error (before, both were the main queue). Nothing does that.

The check again at `0a68727`, with the fix (logs in `~/m2-runs/check-check-S5b/`): the same rows. Rust tests 696 passed
(one more: the new test), the same 10 failed. Suites: 6 of S1's 7 failed again, and `notes-proj` passed. `live3a`
failed "dragging across the ruler loops beats 4–12" once and passed on its retry. That is `HANDOFF.md`'s ruler-drag flake,
which S1's trial saw too; `live3a` hosts no plugin, so S5's loop only idles there. Alone it failed 1 of 6 runs more.
