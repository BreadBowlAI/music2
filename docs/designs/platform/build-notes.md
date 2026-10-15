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

## S2 to S5 on `main`

S2 to S5 were built side by side, each from `512a96a`. They came onto `main` in order, each rebased onto the one
before, so `main` is one line. Each one built (`cargo build --workspace`) and passed its own tests before `main` moved
to it.

| Slice | Conflicts | Its own tests after the rebase |
|---|---|---|
| S2 | none (it was a fast-forward) | `project::` unit tests 19 of 19; `tsc` clean |
| S3 | `build-notes.md` only: both kept | std kernels' tests and clippy; `std/build.sh --check` gives the committed bytes; `bit_identity` 3 pass, 1 ignored; test 10 still prints resample `e2e93ef0…` |
| S4 | `build-notes.md` only: both kept. RENDER-2 and PLAT-4 merged cleanly beside S3's PLAT-5. | `host_math`; `song_math_is_libm_on_every_os`; `gate_tests` 11 of 11 (test 10 passes); `bit_identity`; the 8 renders match `e2e/renders.sha256` |
| S5 | `build-notes.md` only: both kept. Git merged `Cargo.lock` itself; main's lock with cargo's new edges gives the same file. | `m2-os` (with `one_place.rs`), `m2-plugins` (test 11: "main_thread: ok (Surge XT too)"), `m2-gates` and the app test, with the test plugins |

The rebase gave the slices' commits new hashes. The notes above name the old ones:

| Slice | Old → new |
|---|---|
| S2 | unchanged: `c60372a`, `1b50386` |
| S3 | `319c804` → `cc6a4e9`, `e891a0b` → `145d251`, `bd608d7` → `2cb26bf`, `c99992e` → `b04e0d9` |
| S4 | `23db463` → `d501be2`, `01edbf1` → `7559c6f`, `fa09cf0` → `b3a50c5`, `00ff1c7` → `a856c55`, `ce2c10a` → `d0687ff` |
| S5 | `9d200a4` → `8d840ce`, `eefa929` → `96ba069`, `0a68727` → `b82de37`, `4890b64` → `6150310` |

**Fixed here**, small points the checkers found and left:

| What | Why |
|---|---|
| `e2e/env.mjs`'s header says `exe` goes through `chrome-as-mac.sh` off the Mac | It said only "E2E_CHROME, else puppeteer's Chrome", untrue since S2. |
| `e2e/fx-fixes.mjs:57`'s comment: "exports 2" is taken because a name is taken in any capitals on every OS | It said macOS can't tell the two apart, which is no longer the reason (Q1). The check is unchanged. |
| `ci/check.sh`'s clippy row counts lints only | It counted cargo's "could not compile" and "build failed" lines too, so one lint showed as 3 or 4 findings. Only the note changes, not when the row fails. |

**Left for the lead**, as the checkers reported them:
- `create_at` (`m2 new <dir>`) makes exactly the folder it is given, so on Linux it can make `night` beside `Night`.
  §4.4 and OS-41 put Q1's rule in `make()` only.
- Song code: `a ** b` on BigInt variables now throws (oxc lowers `**` to `Math.pow`), and `**` inside `eval` or
  `new Function` still uses the OS's `pow`.
- `apps/music2-desktop/src/main.rs` still says the plugins' main thread is AppKit's (§2.3 keeps this step out of it).
- Flakes not caused by a slice: `m2-plugins`' `surge_plays_notes` (Surge XT's output starts below −140 dB a few
  samples early), `m2-runtime`'s `cell_heard_never_runs_ahead` under heavy load, and G10's timing in
  `the_live_engine_passes_every_gate` under load.

**The check** at `7e20eba` (`ci/check.sh`, 8 min on a quiet server, logs in `~/m2-runs/check-integrate/`): exactly
§13.1's S5 row, with no retries.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected in 1 s with `first-song` and with `with-plugins`, and the app hosts the plugins (no "can't be hosted"; the screenshot shows no "Needs attention") |
| rust tests | FAIL: tests 1 to 9 of §4.1 by name (the 4 watch tests, `a_lane_written_in_two_halves_never_plays_empty`, the hook, stage and kill tests, the preview test); 700 passed. Test 10, test 11 ("main_thread: ok (Surge XT too)") and the app test pass. |
| clippy | FAIL at lint 2, `core/session/src/lock.rs:213` ("2 findings": lints 2 and 3 stop `m2-session`). Without `-D warnings` it lists exactly lints 2 to 6. |
| std kernels, std.wasm, mac lint | PASS |
| browser suites | PASS: 29 of 29, 1,034 of 1,034 checks; live3b 51 of 51 with the plugins |
| renders | PASS: 8 of 8 match `e2e/renders.sha256` |
| repo unchanged | PASS |

## S6. Files, the lock, identity and folders

**Built**

| What | Where |
|---|---|
| `fs`: `push`, `barrier`, `sync_dir`, `rename` (`Swap`, `Excl`), `clone_file`, `file_id`, `file_id_of`, `same_volume`, `is_local`, `stored_name`, `try_lock`, `make_writable`; one `Stamp` and `FileId` | `core/os/src/fs.rs` |
| Each OS's error lists: `rename_unsupported`, `lock_unsupported`, and macOS's `barrier_refused` | `core/os/src/mac/mod.rs`, `linux/mod.rs` |
| macOS: `F_FULLFSYNC`, and `fsync` when a volume refuses it (before, `sync_all`, which is `F_FULLFSYNC` again); `renamex_np`, `clonefile`, `MNT_LOCAL`, `getattrlist` and `gethostuuid`, moved | `mac/mod.rs` |
| Linux: `renameat2` with `RENAME_EXCHANGE` and `RENAME_NOREPLACE` (new); `is_local` by `statfs`'s type (NFS, SMB, CIFS, SMB2, 9p, Ceph, AFS, Coda, FUSE); `/etc/machine-id`; no clone, no stored name | `linux/mod.rs` |
| What both share: `fsync`, the folder sync, stamps, file ids, the write bit, the host name, local time | `unix.rs` |
| `host`: `machine_id`, `name`, `local_time`; `dirs::songs`, `dirs::cache` (`~/.cache/music2` on Linux) | `host.rs`, `dirs.rs` |
| Every `Disk` call of `Real` is one call into `m2_os::fs`; the copy when a clone fails stays here | `core/session/src/tx/stage.rs` |
| The lock, the volume checks, the machine id and the clock from `m2-os`; `host_id()` stays (the machine id, else the host name) | `core/session/src/lock.rs`, `session.rs` |
| `watch::Stamp` is `m2_os::fs::Stamp`; the hash cache saves the same four numbers from it; `spelled()` asks `fs::stored_name` | `core/session/src/watch.rs`, `core/song/src/hash.rs` |
| A Tx's spelling check finds the disk's name by `fs::file_id` | `core/session/src/tx/mod.rs` |
| Export into `assets/` names the file by `fs::rename(Excl)`, not a hard link; the part file's lock and its sweeper use `fs::try_lock` | `crates/editor/src/render.rs` |
| The songs and cache folders from `m2_os::dirs`; copies made writable by `fs::make_writable` | `crates/server/src/project.rs`, `resources.rs` |
| `m2 new` refuses a name taken in other capitals on every OS, in the Mac's words ("already exists"), with a unit test (the lead's decision on Q1) | `project.rs` (`create_at`, `taken`, `create_at_refuses_a_name_taken_in_other_capitals`) |
| No core dump from the kill tests' children on Linux (`prctl(PR_SET_DUMPABLE, 0)`) | `core/session/src/tx/kill_tests.rs` (`no_core_dump`), `hook_tests.rs` |
| The gates time `fs::barrier` and say "the durable sync alone (F_FULLFSYNC on macOS, fsync on Linux)" | `crates/gates/src/server.rs`, `main.rs`, `README.md` |
| The eight files leave `one_place.rs`'s allow-list; 3 are left, all S8's | `core/os/tests/one_place.rs` |
| PLANE-5 (rule and *Why*), WRITE-15, 16 and 17 *Today*, FMT-16, FMT-20, RENDER-30, the layout block, PLAT-2, 3 and 7; Q1's rule is written into PLAT-7 | `docs/architecture.md` |

**Measured**

| What | Result |
|---|---|
| A file going missing during saves. A temporary test (not committed) saved `song.ts` 1,000 times while another thread read it in a loop. | On a disk that can't swap (`Recorder { plain: true }`, which is how Linux saved before this slice): 8,888 of 3,188,491 reads found it missing. With `renameat2`'s swap: 0 of 3,169,767. |
| The kill test, alone | 92.9 s (473 s before, `ground/tests.md`). In the check, `m2-session`'s whole test binary took 93 s. |
| The mac lint reads `mac/mod.rs` | A type error put there for a moment failed it at once. |
| Linux's network types | All 9 equal the kernel's `linux/magic.h`. No network or FUSE volume is mounted here, so `is_local` saying no is untested on Linux (as design §12 says). |
| The hash cache's folder | After the check, `hashes.json` is in the scratch `HOME`'s `.cache/music2`; no `Library/Caches` was made. |
| `m2 new night` beside `Night` (the debug `m2`, scratch `HOME`, a relative path) | "error: night already exists" |

**Deviations**

| What | Why |
|---|---|
| The barrier's error list is tested in `mac/mod.rs`, not in the portable test | Only macOS has the barrier's fallback, so Linux has no such list. The portable test in `fs.rs` feeds the rename and lock lists by name. It leaves out `EOPNOTSUPP` for renames: Linux lists it (it is `ENOTSUP` there), macOS's list is today's (`ENOTSUP`, `EINVAL`). |
| `try_lock` and `same_volume` are portable code in `fs.rs` | `try_lock` is std's `File::try_lock` (`flock`, as before) with each OS's error list. `same_volume` compares two `FileId`s. Neither needs code per OS. |
| One helper, `kill_tests::no_core_dump()`, that both test children call | The design says each child calls `prctl` itself. It is still test code, not an `m2-os` item (attack 12), with one SAFETY comment instead of two. |
| `lock::host_name()` is gone | Its two callers call `m2_os::host::name()`. `host_id()` stays, as §2.3 says. |
| `m2-session` has `libc` as a dev-dependency only | Only the test children use it now, so product code can't call `libc` there. |
| The Tx spelling check finds the disk's name by `fs::file_id`, which follows links; before, by `lstat` | It is the design's API. Only the refusal's message can change: on a disk that folds capitals, when the folder also holds a link to the same file, the message may name the link. |
| `lock.local` says "this computer", not "this Mac" | Linux shows it too now that `is_local` has a Linux body. |
| `host::name()` gives "this computer" when the OS gives no name | As `lock.rs` did. |
| `create_at` and `make()` share one helper, `taken()` | The lead's decision on Q1 for `m2 new`. |

**The check** at `6f9f602` (`ci/check.sh`, 8 min, logs and the app's screenshots in `~/m2-runs/check-build-S6/`):
exactly §13.1's S6 row, on its first run, with no retry.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected in 1 s with `first-song` and with `with-plugins`, and the app hosts the plugins (the screenshot shows no "Needs attention") |
| rust tests | FAIL: tests 1 to 5 of §4.1 by name (`an_idle_project_is_watched_without_a_stat`, `own_commits_make_no_batch_with_events`, `events_that_miss_changes_give_way_to_walks`, `an_event_wakes_an_idle_watcher_and_a_batch_wakes_it_when_due`, `a_lane_written_in_two_halves_never_plays_empty`); 714 passed (700, tests 6 to 9, and 10 new tests). |
| clippy | FAIL at lint 4, `crates/server/src/graph/watcher.rs:55` ("3 findings": lints 4, 5 and 6 stop `m2-server`). Without `-D warnings` it lists exactly lints 4 to 6 (`wake`, `backstop`, `deaf`). |
| std kernels, std.wasm, mac lint | PASS |
| browser suites | PASS: 29 of 29, 1,034 of 1,034 checks, none on a retry; live3b 51 of 51 with the plugins |
| renders | PASS: 8 of 8 match `e2e/renders.sha256` |
| repo unchanged | PASS |

**Left for the lead**
- Only the Mac run tests the macOS bodies: the barrier's `fsync` fallback, the error numbers 45 and 102, and the moved `renamex_np`, `clonefile`, `getattrlist` and `gethostuuid` calls. The mac lint only type-checks them.
- `m2 new NIGHT/../night` beside `Night` is refused, but on Linux it first makes an empty `NIGHT`: `create_at` makes the parent folder before any check. That is older than this slice and needs `..` in the path.

**The independent check** (label `check-S6`, at `5ef6613`; logs in `~/m2-runs/check-check-S6/`)

| What | Result |
|---|---|
| The diff against §13.2's S6 row (and §2, §3.1 to 3.3, §3.7, §6.1) | All of the row is there, and the lead's `create_at` decision. Nothing extra but the deviations above. No test is weaker. No OS code outside `core/os` but S8's 3 files and G11's `refused`. The macOS code says what it said, with the barrier's `fsync` fallback as the one change meant; one small side effect is left for the lead (below). |
| `ci/check.sh` | §13.1's S6 row, with one retry that is not the vanishing file: `live3a` failed "dragging across the ruler loops beats 4–12" and passed on its retry. That is `HANDOFF.md`'s ruler-drag flake: setting a loop writes nothing to disk (`Graph::set_loop` only tells the player). Rust tests: 714 passed, 5 failed (tests 1 to 5 of §4.1 by name); `m2-session`'s binary, the kill test included, 93.4 s. clippy fails at lint 4; without `-D warnings` it lists exactly lints 4 to 6. Suites 29 of 29 after the retry, live3b 51 of 51. Renders 8 of 8. toolchain, web build, build, app, std rows, mac lint and repo unchanged PASS. |
| A FUSE volume, mounted without root (a squashfs image through `snapfuse`) | `is_local` says no, and `same_volume` with `/tmp` says no. `m2 check` and `m2 render` work on a song there; `m2 set` refuses in plain words (the volume is read-only, so the lock can't be made). `/`, `/tmp`, `/dev/shm`, `/proc`, `/sys`, `/run` count as local. |
| Odd names | Swap and create-only rename work with a name that is not UTF-8, one with a line break, spaces, a leading dash, 255 bytes, an emoji. A name too long, a NUL in a path and a rename across volumes are errors, never `Unsupported` (which would make a caller fall back). A create-only rename onto a dangling link says `AlreadyExists` and leaves the link. |
| 32 threads at once | 32 writers each swapping a file 2,000 times while 32 readers read: 0 of 10.2 million reads found it missing. 32 threads racing a create-only rename onto one name, 200 times: exactly one winner each time. 32 threads taking one lock 500 times each: never two holders. |
| Saves through the session (a temporary test, not committed) | 1,000 saves of `song.ts` with 4 readers: with the swap, 0 of 10.8 million reads missing; on a disk that can't swap (`Recorder { plain: true }`), 181,704 of 10.8 million (S7's part). |
| A second run, under load | `m2-session`'s tests twice at once while 32 threads spin: both pass (121 each, the kill test included, 245 s); `m2-os`'s tests 5 times meanwhile: all pass. |
| `m2 new` beside `Night` (the debug `m2`, scratch `HOME`) | `night`, `NIGHT`, `./night`, `night/`, `night/.` and an absolute `.../nIGHT` are refused, and so are `DANGLING` beside a dangling link `dangling`, `üML` beside `Üml`, `CAFÉ` beside `Café`: "already exists" each time. |
| Suites near saves, alone, 8 runs each | `live3a`, `s6-write`, `s6-fixes`, `export`, `fx-fixes`: 8 of 8. `notes`: 7 of 8, then 16 of 16 (below). |
| The hash cache | The check's scratch `HOME` has `.cache/music2/hashes.json` and no `Library/`. |

| Fixed | Why |
|---|---|
| `is_local` says no for the kernel's own AFS (kAFS) too: its number, `0x6B414653` (`AFS_FS_MAGIC`), joins the list, with a test that feeds the list by libc's names | The list had only OpenAFS's `AFS_SUPER_MAGIC` (`0x5346414F`), from design §3.2. This kernel's `kafs.ko` holds `0x6B414653` once and `0x5346414F` never, so a kAFS volume counted as local: no `lock.local` (and from S8, events trusted where only walks see other computers' writes), where WRITE-17 says AFS is a network volume. |

Left for the lead:
- `notes` failed "the timeline shows it shorter" once in 25 runs (8 alone, 16 more alone, 1 in the check). It waits until the file on disk says 16 beats, then measures the clip on screen once, without waiting for the screen. That is the file-then-screen race §4.4 fixed at `notes.mjs:97`. No earlier check log in `~/m2-runs` shows it. S6 doesn't change the order (the file is renamed before the revision goes out); an `until` around the width in `notes.mjs:155` would close it. Left alone: suite lines on `main` are kept few for `taste`'s merge.
- macOS only: when `clonefile` works but the push after it fails, `stage.rs` now copies, which fails with "File exists". The commit still fails, but its error names the copy, not the disk's error (before S6 it named the push's error).
- `m2 new` (and New Song since S2) now lists the parent folder first. In a folder that can be written but not listed (mode 300), `m2 new dropbox/song` fails with "reading dropbox: Permission denied"; before S6 it made the song. Rare, and the capitals rule can't be checked there.
- The real home on this server holds `~/Library/Caches/music2/hashes.json`, written 2026-10-14 20:29, before this slice, by a run with the real `HOME`. Not from this check; left as it is.

The check again at `8616d15`, with the fix (logs in `~/m2-runs/check-check-S6b/`): exactly §13.1's S6 row, with no retry.
Rust tests 715 passed (one more: the new test), the same 5 failed; `m2-session`'s binary 93.5 s. clippy fails at lint 4,
with only lints 4 to 6. Suites 29 of 29, renders 8 of 8, and every other row PASS.

## S7. Saves that a crash can't break

**Built**

| What | Where |
|---|---|
| On a disk that can't swap, a save compares the file, then renames the new one over it in one step. Before, it moved the file aside first, so a crash in between left it missing. | `core/session/src/tx/stage.rs` (`rename_all`) |
| The rollback of such a save writes the before bytes it holds in memory to `old-<k>` (pushed), checks the file still holds the save's bytes, and renames the before bytes over it in one step | `stage.rs` (`put_before`) |
| The kill test runs every case on a disk that swaps and on one that can't (`Recorder { plain: true }`), as two tests that share one body; a new case `manifest` writes `music2.toml` and `song.ts` | `core/session/src/tx/kill_tests.rs` |
| `a_failure_at_any_call_changes_nothing` runs on both disks too, and checks that `song.ts` is never renamed away or removed | `core/session/src/tx/stage_tests.rs` |
| The scan at the lock pushes a completed commit's folders (as its step 8 does), then one barrier, then removes its staging and journal, all through the session's disk; a new test checks that order | `core/session/src/tx/journal.rs` (`tidy_completed`), `stage.rs` (`push_parents`), `session.rs`, `tx/mod.rs`; `a_completed_journal_goes_after_its_folders_are_pushed_and_a_barrier` |
| A new WAV is pushed and its folder synced | `core/runtime/src/audio.rs` (`write_new_wav`) |
| The bounce's provenance is pushed and `assets/` synced (and the project folder when `assets/` is new), before `--place` names the WAV | `apps/m2/src/main.rs` |
| An export's part file is pushed before it gets its name, and its folder synced after | `crates/editor/src/render.rs` (`write_as`) |
| A new song's files are pushed and every folder synced, the song's own entry last | `crates/server/src/project.rs` (`fill`, `push_tree`) |
| WRITE-1 (files made outside a Tx), WRITE-15 *Today* (the fallback, the tidy at the lock), WRITE-16 *Today* (the kill test on both disks) | `docs/architecture.md` |

**Measured**

| What | Result |
|---|---|
| The kill test, both disks, side by side | Both pass, 130 s (the two tests run at once). In the check, `m2-session`'s whole binary took 147 s (93 s at S6). |
| The no-swap kill test against the old move-aside fallback (put back for a moment, not committed) | Fails at `several (no swap) 22`: "song.ts changed since … Check it by hand", no fix (`ground/tests.md`'s kill point 22). The `manifest` case alone fails at kill point 16: "has no music2.toml, so it isn't a music2 project". With S7's fallback both pass. |
| `strace` of the debug `m2` (scratch `HOME`): `m2 new … --example resample` | Every file `fsync`ed, then each folder, then the song's folder, then its parent `songs/`. |
| `m2 bounce … --place chops.clips@16` | The WAV `fsync`ed, `assets/` synced, the provenance `fsync`ed, `assets/` synced; only then the placing Tx (journal, barrier, `RENAME_EXCHANGE` onto `clips.json`). |
| `m2 render` into `assets/` | The part file `fsync`ed, then `renameat2(RENAME_NOREPLACE)`, then `assets/` synced. |

**Deviations**

| What | Why |
|---|---|
| The rollback writes `old-<k>` first, then checks the file, then renames (the design: check, write, rename) | The check sits right before the rename, so an outside write between them is as unlikely as on the way forward. |
| The scan keeps the completed journals when its barrier fails (before, it removed them anyway) | The removal must never be durable before the renames (WRITE-15). A rollback whose barrier fails already does the same. No new state: the next lock tries again. |
| `journal::scan` takes `Option<&mut dyn Disk>` (a writer's disk) instead of `tidy: bool` | The tidy goes through the session's disk, so a test can see its order. A reader passes `None` and still can't change anything. |
| `fill` pushes, so New Song (`create`) gets it as well as `m2 new` (`create_at`) | Both share `fill`; a new song from the app needs it as much. |
| The bounce also syncs the project folder when it made `assets/` | Otherwise `assets/` itself could be lost, with the WAV in it. |
| `write_new_wav` removes the WAV when its push or folder sync fails | As for a failed write: "a failed write leaves nothing behind". |
| An export whose folder sync fails after the file is named is an error, and the file stays | It is named and whole; the error tells nothing should rely on it. A folder `fsync` fails only on a failing disk (§9, attack 3). |
| `m2-runtime` and `m2` gain an `m2-os` dependency | For `fs::push` and `fs::sync_dir`. Two lines in `Cargo.lock`; both already built `m2-os` through other crates. |
| `a_failure_at_any_call_changes_nothing` names the failed call by `start + k` | It printed `calls[k]`, the wrong call. |

Shared files this slice touches, each edit small: `Cargo.lock` (2 lines), `docs/architecture.md` (WRITE-1, WRITE-15 and WRITE-16, 3 lines), this file (this section). Not `ci/check.sh`, not `one_place.rs`.

**The check** at `758e583` (`ci/check.sh`, 14 min while other slices' checks ran; logs and the app's screenshots in
`~/m2-runs/check-build-s7/`): §13.1's S7 row, with one known flake.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected in 1 s with `first-song` and with `with-plugins` |
| rust tests | FAIL: tests 1 to 5 of §4.1 by name (`an_idle_project_is_watched_without_a_stat`, `own_commits_make_no_batch_with_events`, `events_that_miss_changes_give_way_to_walks`, `an_event_wakes_an_idle_watcher_and_a_batch_wakes_it_when_due`, `a_lane_written_in_two_halves_never_plays_empty`), and `m2-plugins`' `surge_plays_notes` once ("nothing before the note's sample offset"), which passed on its retry. 716 passed: S6's 715 and S7's 2 new tests, less the flake. |
| clippy | FAIL at lint 4, `watcher.rs:55` ("3 findings"). Without `-D warnings` it lists exactly lints 4 to 6 (`wake`, `backstop`, `deaf`). |
| std kernels, std.wasm, mac lint | PASS |
| browser suites | PASS: 29 of 29, 1,034 of 1,034 checks, none on a retry; live3b 51 of 51 with the plugins |
| renders | PASS: 8 of 8 match `e2e/renders.sha256` |
| repo unchanged | PASS |

`surge_plays_notes` is the flake the S2 to S5 notes name. Run alone 40 times on this checkout: 38 passed, 2 failed with
the same message. S7 changes nothing in `core/plugins` or `core/os`.

**Left for the lead**
- The kill test makes `m2-session`'s binary about 50 s longer. Splitting it per case would run the cases side by side;
  left as the design's two runs of one test, the name the contract cites kept.
- The bounce's failure path keeps its old gap: when the provenance can't be written (now also when its push or the
  folder sync fails), the WAV goes but the provenance file stays, so a retry with that name is refused. Older than
  this slice.

**The independent check** (label `check-s7`, at `a0b217e`; logs in `~/m2-runs/check-check-s7/`)

| What | Result |
|---|---|
| The diff against §13.2's S7 row (and §3.1, §6.1) | All of the row is there: the fallback without the move aside, its rollback from the before bytes, the kill test on both disks with the `manifest` case, the four places that push before naming, the scan's push before its barrier, and the three contract edits word for word. Nothing extra but the deviations above. No test is weaker. No OS code outside `core/os`: S7 only calls `m2_os::fs`, and `one_place.rs` passes with its allow-list untouched. `core/os` is not touched, so the macOS code says what it said. |
| `ci/check.sh` (9 min, while S8's check ran beside it) | §13.1's S7 row, with one suite retry that S7 didn't cause (below). Rust tests: 717 passed, 5 failed (tests 1 to 5 of §4.1 by name), no retry; `m2-session` 123 of 123 in 140.9 s. clippy fails at lint 4; without `-D warnings` it lists exactly lints 4 to 6. Suites 29 of 29 after the retry. Renders 8 of 8. toolchain, web build, build, app, std rows, mac lint and repo unchanged PASS. |
| The old move-aside put back (in a scratch copy) | The no-swap kill test fails at `several (no swap) 22` with no fix offered (`ground/tests.md` surprise 1), and `a_failure_at_any_call_changes_nothing` fails too. With S7's code both pass. |
| Recovery on a disk that can't swap (a temporary change: the kill test reopens, and runs Finish and Discard, on `Recorder { plain: true }` too) | Every case passes at every kill point. |
| Two failures in a row at every call of the multi-file commit, both disks (the second often hits the rollback's first step) | `song.ts` is never missing. A rollback that fails is reported at once, and its Discard (and Finish when offered), run on the same kind of disk, puts every target back: 4 such rollbacks on the swap disk, 5 on the other. |
| 1,000 saves of `song.ts` while 32 threads read it | Without a swap: 0 of 59.9 million reads found it missing (S6's check: 181,704 of 10.8 million before S7). With the swap: 0 of 63.5 million. |
| The tidy at the lock, killed at each of its 15 disk calls (a child opens a project holding two completed journals) | The next open tidies both, offers nothing, and the project holds the commit. |
| A second run under load | `m2-session`'s tests (with these probes) twice at once, load average about 70: both pass, 126 of 126, 275 s. |
| `notes`, the suite that needed the retry | It failed "typing 4 bars makes the clip 16 beats long" (two requests refused, HTTP 400) and passed on its retry. Alone it passed 12 of 12, and 10 of 10 with 32 busy threads. The suite waits for two undos on disk, then types into the length field at once; the likely cause is a clip on screen that is still older than the file. S7 doesn't change that path (ext4 swaps, as at S6). |

| Fixed | Why |
|---|---|
| `m2 bounce` into a folder in `assets/` (`vox/keys`, a name `relink.rs` uses) syncs each folder it made in the one above (`apps/m2/src/main.rs`) | The slice synced only `assets/`'s own entry, as if the WAV were always right in `assets/`. strace showed `assets/vox/` new, and never synced in `assets/`: a placing Tx could be durable while `vox/` is not, so the lane names a file a power cut can lose (design §3.1). strace after the fix: `vox/lead` with neither folder there syncs `vox/`, `assets/`, the project; `drums/kick/one` with `assets/` there syncs `kick/`, `drums/`, `assets/`; a name right in `assets/` syncs only `assets/`. `m2`'s tests pass. |

Left for the lead:
- An export into a folder that can be written but not listed (mode 300) now fails after the file is whole: `m2 render`
  says "couldn't write …: Permission denied" and exits 1, and the WAV is there (tried). Before S7 it passed: the
  folder sync can't open the folder. Into `assets/`, `write_for` would then say the file "is there already". Only
  files a Tx names need the sync; an export outside the project could let a failed sync pass.
- New Song, `m2 new`, the bounce and exports now fail on a volume that refuses a folder `fsync`, while commits go on
  (`push_parents` ignores a failed folder sync). Linux's `vboxsf` has no folder `fsync` (from the kernel's source, not
  run here); the Mac's volumes are unknown. Design §9 attack 3 counts a failed folder sync as a failing disk.
- `m2 new a/b/song` makes `a/` and `b/`, and syncs only `b/`, the song's parent. A power cut could lose the whole
  song, but nothing outside it names it.
- The committed kill test reopens and runs Finish and Discard on the disk that swaps in both runs, so recovery on a
  disk that can't swap is untested there. It passes when changed (above): 2 lines.
- The Windows step: `push_tree` opens each file read-only to push it; `FlushFileBuffers` needs write access.

The check again at `7ec6404`, with the fix (logs in `~/m2-runs/check-check-s7b/`): exactly §13.1's S7 row, with no
retry. Rust tests 717 passed, the same 5 failed; `m2-session`'s binary 138.7 s. clippy fails at lint 4, with only lints
4 to 6. Suites 29 of 29 (1,034 of 1,034 checks; live3b 51 of 51 with the plugins), renders 8 of 8, and every other row
PASS.

## S8. File events

**Built**

| What | Where |
|---|---|
| `watch::Events` (`start`, `flush`, and drop: nothing is heard after it), `Heard` (`Path`, `Rescan`) | `core/os/src/watch.rs` |
| macOS: the FSEvents stream, moved from `graph/fsevents.rs`; its flags become `Heard` (dropped, root changed, mount, unmount, and the root's entry with "must scan subfolders" → `Rescan`) | `core/os/src/mac/watch.rs` |
| Linux: inotify, new. One instance per process and one reader thread (`m2-watch`) that rests 10 ms after each read with the instance free. Each folder is watched before it is listed. A new folder is watched with the folders under it, then named. A watched folder that moves (`IN_MOVE_SELF`, not the root), and a queue overflow, make the watcher watch its project again, let go of what that didn't reach, and get `Rescan`. A flush reads until `EAGAIN`. | `core/os/src/linux/watch.rs` |
| `graph/fsevents.rs` becomes `graph/events.rs`, the same on every OS: the hints, `MAX_HINTS`, the filters, take, put back, the event count, the wake | `crates/server/src/graph/events.rs` |
| No OS `cfg` left; `source()` asks for a local volume and `Events::start`, and says why it walks | `crates/server/src/graph/watcher.rs`, `graph.rs` |
| A walk a `Rescan` asked for counts no misses and clears the waiting ones; `WatchStats::rescans` counts those walks | `watcher.rs` |
| The 4 new tests, as §13.2 says (60 s walk, 300 ms grace, the last write taken in within 1.5 s, `misses == 0`, `events` still true) | `crates/server/src/graph/watch_tests.rs` |
| 9 tests in `m2-os`: 4 for every OS (a write named once `flush` returns, a renamed folder, the root going, `Send + Sync`), 5 for Linux (folders not entered, a file named at each write while open, two watchers sharing a folder, a folder moved out, a real overflow) | `core/os/src/watch.rs`, `linux/watch.rs` |
| G11 says "watched by file-system events" | `crates/gates/src/server/watch.rs`, `crates/gates/README.md` |
| `one_place.rs`'s allow-list is empty | `core/os/tests/one_place.rs` |
| WRITE-19's *Today* (§6.1's three edits), PLAT-2's and PLAT-3's *Today* | `docs/architecture.md` |

**Test 5** (`a_lane_written_in_two_halves_never_plays_empty`, design §4.1 "unverified until S8"). inotify does name each
write separately, so the test's one-revision branch is never taken on Linux.

| Measured | Result |
|---|---|
| Its events cases, 20 runs | 20 of 20: at 400 ms, two revisions (the half keeps the last data, with `lane.invalid`; then all 8 notes); at 60 ms, one |
| What the watcher heard (a temporary print, removed) | The event count rose after the first half (1 → 2 or 3) and again after the second (→ 4 or 5). Each half opened its batch in a tick its events started (a look at the named path), long before the 5 s walk. |
| `a_file_written_while_open_is_named_at_each_write` (new, Linux) | Each of two writes to a file still open is named by its own event (`IN_MODIFY`), before the close |

So the "one revision" case at 400 ms is for FSEvents, which names such a file when it is closed. The test stays as it is.

**Each new test catches what it is for** (a change put in for one run, then taken out)

| Broken on purpose | Fails |
|---|---|
| New folders never watched | made after open, made and filled at once, the burst |
| A renamed folder keeps its old paths (no path update, no rebuild) | the rename test, and 2 `m2-os` tests |
| A `Rescan`'s walk counts misses, as before S8 | the burst test, with `misses: 4094` |

**Repeats**

| What | Result |
|---|---|
| The 4 new tests and tests 1 to 5, each run alone 20 times | 20 of 20 each |
| The same with 32 threads spinning (load average up to 39) | 20 of 20 each |
| `m2-server`'s whole test binary (135 tests, one inotify instance shared by all) | 5 of 5 |

**G11 on Linux**, for information (PLAT-1; `m2-gates --server tx-data watcher`, release, a quiet server; the log is
`~/m2-runs/logs/build-s8-gates-g11.log`)

| Gate | Result |
|---|---|
| `tx-data` | PASS: to the first block that plays a write, p95 8.5 ms; the barrier alone p95 0.1 ms |
| `watcher` | PASS: watched by file-system events; idle, 0.10% of a core (2 walks in 10 s, 197 ticks that looked at nothing); an outside write played p50 159 ms, p95 213 ms in place, p50 166 ms, p95 173 ms as an atomic save. By walks alone: 3.43%, p50 217 to 218 ms. |

**Deviations**

| What | Why |
|---|---|
| `Hints::paths` is a set; FSEvents' flags are no longer kept per path | Only the rescan read them, and that is decided in `mac/watch.rs` now. The watcher reads the paths alone (design §11). |
| `WatchStats` gains `rescans` | The burst test checks that its walk was a `Rescan`, so it can't pass by never reaching the limit. |
| A test-only `Watch::flush` | The burst test makes sure the new folder is watched before it fills and that every name is heard before the next tick. Otherwise whether the names pass 4,096 depends on timing. |
| `source()` still asks `m2_session::lock::local_volume` | It is `fs::is_local` plus the session's test fault. The design names `fs::is_local`. |
| A folder that can't be read (`EACCES`) is skipped, like one gone (`ENOENT`, `ENOTDIR`) | A walk can't list it either. Failing the start for it would move the whole project to walks. |
| macOS doesn't use `enter` | FSEvents watches the whole tree; `events.rs` filters with `walked()`, as before. The API says a source may name paths in folders it doesn't enter. |
| `m2-os`'s watch tests share a test-only lock; the overflow test takes it alone | An overflow gives every watcher in the process a `Rescan`. |
| WRITE-19's added sentence also names the new tests; PLAT-3's *Today* changes too | The *Today* line names a test for each claim. PLAT-3 said "file-system events move in with S8". |

The overflow rebuild, which §12 says can't be tested without a hook, is tested in `m2-os`: the test holds the
instance's lock (test code) while more than 16,384 events queue, then makes a folder whose event is lost. After the
overflow that folder is watched, and a write in it is named.

The mac lint caught one mistake while this was built: in `mac/watch.rs` the `heard` parameter hid FSEvents' callback
of the same name. The callback is `callback` now.

**Shared files** (S7, "script" and "flakes" are built beside this): `docs/architecture.md` (WRITE-19's *Today*, and
the *Today* lines of PLAT-2 and PLAT-3), `core/os/tests/one_place.rs` (S8's three lines leave `NOT_YET`), one `mod`
line and the header in `core/os/src/lib.rs`, `linux/mod.rs` and `mac/mod.rs`, and this section. `Cargo.lock` and
`ci/check.sh` are unchanged (`m2-server` already used `m2-os`).

**The check** at `edcee5f` (`ci/check.sh`, 10 min, logs and the app's screenshots in `~/m2-runs/check-build-s8/`):
`music2 check: PASS`, on its first run, with no retry. That is §13.1's S8 row: Linux fully green.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected in 1 s with `first-song` and with `with-plugins`; the plugins hosted (no "Needs attention" in the screenshot) |
| rust tests | PASS: 733 passed, 0 failed (S6's 715, tests 1 to 5, the 4 new watcher tests, and the 9 new `m2-os` tests) |
| clippy | PASS. Without `-D warnings` it reports nothing. |
| std kernels, std.wasm, mac lint | PASS |
| browser suites | PASS: 29 of 29, none on a retry |
| renders | PASS: 8 of 8 match `e2e/renders.sha256` |
| repo unchanged | PASS |

Watched by events, not walks: neither app log says "watching … by walking it" (they keep the app's stderr), and
neither do the headless server's logs of `notes` and `s4-clips`, run once more with `--keep` (the check doesn't keep
them). Both suites passed (52 of 52, 28 of 28).

**Left for the lead**
- Only the Mac run tests the moved FSEvents code; the mac lint only type-checks it. It is meant to behave as before.
- Running out of inotify watches after start (`ENOSPC`) gives a `Rescan` and leaves the folder to the 5 s walks; a
  change there is then a miss, so the two-miss rule moves the project to walks. Untested: lowering the limit needs root.
- One inotify instance and its reader thread live until the process ends, by design.
- §13.3 still applies after `taste` merges: re-run s8-tracks' "layout.json within a second" on this watcher.

**The independent check** (label `check-s8`, at `e4e1504`; logs in `~/m2-runs/check-check-s8/`)

| What | Result |
|---|---|
| The diff against §13.2's S8 row and §3.4 | All of the row is there, and nothing extra but the deviations above. Tests 1 to 5 are unchanged, and no test is weaker. No OS code outside `core/os`: `one_place.rs` passes with an empty list. The macOS code says what it said: the same FSEvents calls, flags, flush and drop; each flag gives the same hint as before, now split between `mac/watch.rs` and `events.rs`. |
| `ci/check.sh` | `music2 check: PASS`, with 2 retries, neither in this slice's area: `m2-plugins`' `surge_plays_notes` (the known flake), and `live3b` once (below). Rust tests 732 passed, then the flake on its retry; clippy PASS, and without `-D warnings` it reports nothing; 29 of 29 suites after the retry; renders 8 of 8; toolchain, web build, build, app, std rows, mac lint and repo unchanged PASS. No app or server log says the watcher fell back to walks. |
| Test 5 on Linux (design §4.1, "unverified until S8") | Verified: inotify names each write of a file still open. A copy of its events case at 400 ms that asks for exactly two revisions (not committed) passed 20 of 20 alone and 20 of 20 under load: the half (4 notes, `lane.invalid`), then all 8. Only an event can make that first revision: the next walk was 5 s away, and the case lasts under 2 s. So the test's "one revision" branch is for FSEvents; Linux never takes it. |
| The 4 new tests and tests 1 to 5 | 20 of 20 each, run alone; 20 of 20 each with 32 threads spinning (load average 23 to 68) |
| `m2-server`'s whole test binary, two copies at once, 3 times, with 32 threads spinning | all pass (136 tests each) |
| `m2-os`'s tests with 9 probes (not committed), 20 runs alone and 20 under load | Every watch test and probe passed every run. The probes: a tree 60 deep made at once (every level watched, a write at the bottom named); 3,000 folders made at once (all watched, a write in each named); two folders swapped with `RENAME_EXCHANGE`; a folder moved out, then made again; a folder renamed and written into before the reader could look (named by its new path); a folder renamed to `.lib` (let go, nothing heard); names that aren't UTF-8 (not watched), with a line break, of 255 bytes; drop racing a writer, 200 times (nothing heard after drop); 32 threads starting and dropping watchers on nested folders while folders come, move and go (no watch left after). |
| `live3b` alone, 8 runs, and 8 more with 32 threads spinning | 16 of 16 |

| Fixed | Why |
|---|---|
| A rebuild keeps watching past a folder it can't watch (`watch_under`) | The walk that adds watches stopped at the first watch that failed (`ENOSPC` when the user's watches run out, `ENOMEM`). A rebuild (a folder moved, an overflow) then let go of every folder it hadn't reached yet, though they were watched: with one such folder, only the project folder stayed watched. Design §3.4 says only that folder is left to the walks. The walk now goes on, and still asks for a walk. New test `a_folder_that_cant_be_watched_leaves_the_others_watched`, with a test-only list of folders whose watch fails; it fails 5 of 5 without the fix. |
| `a_file_written_while_open_is_named_at_each_write` takes the watch tests' shared lock | The overflow test's `Rescan` could land between its halves. 3 of 100 runs of `m2-os`'s tests failed; run beside the overflow test alone, 84 of 100. After the fix, 0 of 100 each. |

The check again at `02a9164`, with the fix (logs in `~/m2-runs/check-check-s8b/`): `music2 check: PASS`, with no
retry. Rust tests 734 passed, 0 failed (one more: the new test); clippy PASS, and without `-D warnings` nothing; 29 of
29 suites; renders 8 of 8; every other row PASS. That is §13.1's S8 row.

Left for the lead:
- `live3b` failed once in the first check: "its release writes db(-32) into song.ts" (it waits 4 s). Other jobs had
  the load average at 45 to 74 then, and its slow save compiled for 3,248 ms (1,400 to 1,900 ms in quiet checks). The
  server's log says nothing (no miss, no walk). It passed on its retry and 16 of 16 times after. Not traced to this
  slice; watch for it.
- `thread::tests::cpu_time_grows_while_the_thread_works` (S5's) failed 1 of 20 runs with 32 threads spinning, and once
  in 100 runs while only other jobs loaded the server: it needs 10 ms of CPU within 30 ms. A load-sensitive test
  outside this slice.
- Test 5's comment still says events name a file written while open only when it is opened and closed. True for
  FSEvents, not for inotify. Left as it is.
- Only the Mac run shows two things the new tests assume of FSEvents: that one `FSEventStreamFlushSync` delivers a
  write made just before it (`m2-os`'s portable tests assert it after one flush), and that a burst of 4,196 files is
  named file by file (if FSEvents folds it into one folder event, the burst test's walk is not a `Rescan`).
- When inotify's limits run out at start, the watcher says "file-system events didn't start (No space left on device
  (os error 28))", or "Too many open files" for instances. True, but cryptic.
- A folder the watch walk can't list (`read_dir` fails, out of file descriptors say) leaves the folders under it
  unwatched, with no `Rescan`. Their changes are found by the 5 s walks, as misses. Rare.

## Script. Song code's sandbox

Three loose ends S4 left in `core/script`, found by its checker. Built from `e1606e1` (the end of S6) on the branch
`plat/script`, beside S7, S8 and "flakes".

**Built**

| What | Where |
|---|---|
| `**` and `**=` are lowered to `__m2_pow`, not `Math.pow`: oxc still lowers to `Math.pow`, and a small pass renames the calls oxc made (theirs have no place in the source; a `Math.pow` the code wrote has one) | `core/script/src/lib.rs` (`strip`, `Pow`) |
| `__m2_pow` in the prelude: each side converted as JS converts it (`-(-x)` is JS's ToNumeric), numbers through the libm `Math.pow`, BigInts through QuickJS's own `**` (exact integer arithmetic, no C library) | `PRELUDE` |
| `eval`, and the constructors of plain, async, generator and async generator functions, throw with a message in `Math.random`'s style; each refusal keeps its constructor's name and prototype, so `instanceof Function` still works | `PRELUDE` |
| `performance` is removed, as `Date` is (see the deviations) | `PRELUDE` |
| The prelude is named `<prelude>` in stack frames, and an error it throws is placed at the code that called it | `host`, `caught` |
| An import of a name its module doesn't export names the module in full, as people see it, at the import (also the circular and ambiguous forms of QuickJS's message) | `State::export_error`, `State::import_at` |
| Tests: `exponents_are_javascripts`, `song_code_cannot_make_code_from_text`, `a_missing_export_is_named_in_full_at_the_import` | `core/script/src/tests.rs` |
| RENDER-2's rule says song code reads no clock and no randomness and makes no code from text; its *Today* says how | `docs/architecture.md` |
| The sandbox's line in "Settled while building" | `docs/song-as-program.md` |

Nothing in the SDK, `std/src`, the examples, the templates, the tests' songs or the browser suites uses `eval`,
`Function`, a function's `.constructor` or `performance` in song code (grep over every tracked file outside `web/`,
which is the UI, not song code; the suites' own `$eval` and `Date.now()` run in Node). No `.ts` file there uses `**` or
`Math.pow`, so the rename changes no compiled song, and the 8 renders are unchanged. Modules load through QuickJS's C
API (`JS_Eval`), which the prelude doesn't touch: every build test loads the SDK and std after it.

**Measured** (a temporary probe test, not committed; the same song before and after)

| Song code | Before | After |
|---|---|---|
| `a ** b` with `a = 2n`, `b = 3n` | TypeError: cannot convert BigInt to number | `8n` |
| `a ** b` with `a = 2n`, `b = 3` | TypeError: cannot convert BigInt to number | the same: it is what QuickJS's own `2n ** b` throws |
| `x ** y` in a function that declares `const Math = 1` | TypeError: not a function | `8` |
| `x ** y` beside `const Math = { pow: () => 42 }` | `42` | `8` |
| `eval("0.1 ** 0.7")`, `new Function("return 0.1 ** 0.7")()` | `0.199526231496888` (glibc's `pow`) | refused: "eval isn't available: songs run only the code in their files, so they render the same on every computer. Write the code in a file instead." |
| `0.1 ** 0.7`, `Math.pow(0.1, 0.7)` | `0.19952623149688797` (libm) | the same |
| `performance` | on the global object (QuickJS-ng's; its `now()` reads the clock) | gone: `typeof performance` is `"undefined"` |
| `import { bar } from "./tracks/<a long name>"`, with no `bar` there | "Could not find export 'bar' in module './tracks/a-very-long-folder-name-to-push-the-module-name/past-s'", no place | the full name, without `./`, at `song.ts:2:10` |
| `eval("1")` at the top of a package module (describe), with the refusal in place | without the prelude's name: placed at `eval_script:20:24`, a line of the prelude (so was `Math.random`'s error) | placed at `pkg/src/index.ts:2:6` |

Each piece was taken out once, and its test failed: the rename (`exponents_are_javascripts`), `-(-x)` (the same test,
on `{ valueOf } ** { valueOf }`), numbers through QuickJS's `**` instead of libm (`song_math_is_libm_on_every_os`:
`0.1 ** 0.7` and `y **= -89 / 12` wrong), the `eval` refusal, the constructor refusals, removing `performance`, the
prelude's frame name (`song_code_cannot_make_code_from_text`), and the export message
(`a_missing_export_is_named_in_full_at_the_import`).

**Deviations**

| What | Why |
|---|---|
| `**` is lowered to a helper of its own, `__m2_pow`, instead of `Math.pow` handing BigInts on (the lead's suggestion) | With `Math.pow` as the target, `Math.pow(2n, 3n)` would give `8n` where JS throws, and a `Math` the song declares would still change `**`: oxc calls the nearest `Math` in scope (measured above, a bug S4's lowering brought). The rename is one small pass over oxc's output. A song that declares its own `__m2_pow` changes its own `**`, as one that declares `__m2s` breaks its call sites; nothing it can reach differs between computers, so that is no determinism hole. |
| `performance` is removed too | QuickJS-ng puts `performance.now()` on the global object: a clock song code could read, as it could `Date` before. Nothing uses it. Found by listing the sandbox's globals. |
| The contract has no rule that named the clock and randomness, so RENDER-2 gets one sentence: song code reads no clock and no randomness and makes no code from text | RENDER-2 is the rule about a render's bits, and already names song code's math. |
| The prelude has a name in stack frames, and `caught` skips its frames | Without it, an error the prelude throws for code that has no project frame (a package module's top level, in describe) was placed in the prelude. `Math.random`'s error took the same path. |
| `files.rs`'s diagnostics test: its case "No location at all" was the missing export, which now has a place | The missing-export case now checks the place (`tracks/lead.ts`, line 2, column 10) and keeps its check that the package's path is shortened. The case without a place is now a value thrown that isn't an error (`throw 1`), which has no stack; it checks the message too. |
| The import's place is found by parsing the files the build loaded, only when this error happens | QuickJS's error names only the module imported from. The first import of that name from that module, in bytewise order of the files (the project's first), skipping type-only imports. About 45 lines, with the full name. |

**Shared files** (S7, S8 and "flakes" are built beside this): `docs/architecture.md` (RENDER-2's rule and *Today*,
one sentence each), `crates/server/src/files.rs` (the end of one test), `docs/song-as-program.md` (one line), and this
section. No `Cargo.lock`, `ci/check.sh` or `one_place.rs` change.

**Left for the lead**
- `WeakRef` and `FinalizationRegistry` are reachable. QuickJS frees most garbage at once, by reference counts, the
  same on every computer; garbage in cycles waits for its cycle collector, which runs after a number of bytes
  allocated, as the OS's `malloc` counts them (`js_trigger_gc`). So the moment a `WeakRef` to a cycle empties could
  differ between macOS and Linux, and so could whether a build near the 256 MB limit fits. No song uses them;
  removing the two would be one line beside `Date`.

**The check** at `7d37b6b` (`ci/check.sh`, 13 min while other slices' checks loaded the server, logs and the app's
screenshots in `~/m2-runs/check-build-script/`): §13.1's S6 row, with this slice's 3 new tests passing, and one known
kind of flake that passed on its retry.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected in 1 s with `first-song` and with `with-plugins` |
| rust tests | FAIL: tests 1 to 5 of §4.1 by name (the 4 watch tests and `a_lane_written_in_two_halves_never_plays_empty`), as at S6; 718 passed (S6's 715 and the 3 new tests) |
| clippy | FAIL at lint 4, `crates/server/src/graph/watcher.rs:55` ("3 findings"). Without `-D warnings` (`cargo clippy --locked --workspace --all-targets`, run after the check) it lists exactly lints 4 to 6: `wake` (`watcher.rs:55`), `backstop` (`:134`), `deaf` (`:216`). |
| std kernels, std.wasm, mac lint | PASS |
| browser suites | PASS: 29 of 29 after one retry; live3b 51 of 51 with the plugins |
| renders | PASS: 8 of 8 match `e2e/renders.sha256` |
| repo unchanged | PASS |

The retry: `attention-attn` failed one check, "the move's toast Undo says it can't any more", and passed 56 of 56 on its
retry. Run alone 10 more times on this build: 10 of 10 passed. The step waits until the files on disk show the undo,
then clicks the toast's Undo, whose answer depends on the revision the page holds (`Attention.tsx`): while the undo's
revision hasn't reached the page, it asks the server to undo instead of saying it can't. Most likely that happened
under load (the file-then-revision race S2 and S6's checker met in other suites); this is read from the code, not
caught in the act. This slice changes nothing on that path. No earlier check log in `~/m2-runs` shows this failure.

**The independent check** (label `check-script`, at `198cdbb`; logs in `~/m2-runs/check-check-script/`)

| What | Result |
|---|---|
| The diff against the task (the three loose ends) | All three are there. Nothing extra but the deviations above. No test is weaker: `files.rs`'s export case now checks a place and still checks the short package path, and its no-place case is `throw 1`. Only `core/script` and docs change: no OS code, no macOS code. |
| `ci/check.sh` (8 min) | §13.1's S6 row with the 3 new tests: Rust tests 718 passed, 5 failed (tests 1 to 5 of §4.1 by name). clippy fails at lint 4; without `-D warnings` it lists exactly lints 4 to 6. Suites 29 of 29 after one retry: `attention-attn`, the same check as in the builder's run (below). live3b 51 of 51. Renders 8 of 8. Every other row PASS. |
| Everything reachable from `globalThis` and the four kinds of function (a temporary test walking own keys, prototypes, getters and setters: over 500 objects) | No `eval` or `Function` constructor that still makes code: each one found throws the refusal. |
| `**` in static and instance class fields, private fields (`this.#p **= …`), `super.g ** …`, default parameters, destructuring defaults, optional chains, `arr[i++] **= …`, templates, `a ** b ** c`, getters; with `-0`, `NaN ** 0`, `1 ** Infinity`, strings, `null`, `true`, a Symbol | All as JS defines them. Numbers give libm's bits (`10 ** -0.7` is `0.19952623149688797`). |
| 32 threads, 4 builds each, of a song with BigInt `**`, number `**` and a refused `eval` | 128 builds, all the same IR. |
| A missing export from a 75-byte module name with accents, from a name with `'` in it, and the circular form | The full name, at the right place. |
| A missing export, where a file that sorts first imports the same name only for types | **Wrong place** (fixed, below). |
| An export name over 63 bytes | QuickJS cuts it too: the message keeps the cut name and has no place (left, below). |
| A TypeScript `enum` in song code | **The process aborts** in a debug build. Older than this slice: the same on `main` (left, below). |

| Fixed | Why |
|---|---|
| `import_at` reads each file's imports after the TypeScript strip the build runs (`Transformer`, as in `strip`), so it sees only the imports the build keeps. Its own type-only checks went: the strip drops those. Test: `a_missing_export_is_named_in_full_at_the_import`, with this case and the circular form, which had no test. | The strip drops an import that only types use. With `import { Foo }` used as a type in `tracks/a.ts` and as a value in `tracks/b.ts` (where it fails), the error pointed at `tracks/a.ts:1:10`, whose import is fine. Now `tracks/b.ts:1:10`. The new case failed without the fix. |

The `attention-attn` retry: both checks of this branch failed the same check once, "the move's toast Undo says it
can't any more". No other check log in `~/m2-runs` (19 of them) has it. So I ran the suite alone, taking turns between
`main`'s build (`8616d15`, without this slice) and this branch's:

| Load | `main` | this branch |
|---|---|---|
| idle (load about 3) | 8 of 8 passed | 8 of 8 passed |
| 12 busy loops sharing the suite's 8 cores (load up to 38) | 9 of 10: one run failed 12 checks of another kind, at the load's peak | 10 of 10 passed |

It did not come back on either. The slice changes nothing on that path: the move and its undo change only data, so
they run no QuickJS. The builder's reading of the code stands: the suite clicks the toast's Undo as soon as the files
on disk show the undo, which can be before the undo's revision reaches the page.

**Left for the lead**
- **A TypeScript `enum` in song code aborts a debug build** (the tests, and the app or server from `cargo run`). It
  happens on `main` too, and nothing in std, the SDK, the examples or the templates uses `enum`. oxc 0.153's enum
  transform asserts (in debug builds only) that the semantic was built `with_enum_eval(true)`, which `strip` doesn't
  do. The panic happens inside QuickJS's module loader, which can't unwind, so the whole process stops ("panic in a
  function that cannot unwind"). A release build doesn't stop, but oxc says string members then get wrong reverse
  mappings (oxc#21667). The plain fix, `with_enum_eval(true)`, folds constant members in Rust, and a `**` there would
  use Rust's `powf`, the OS's `pow` (PLAT-4). So it needs a choice: refuse `enum` with a clear message, or fold with libm.
- An export name over 63 bytes: QuickJS cuts it as it cut module names, so the message keeps the cut name and has no
  place. Rare; matching the name by its first bytes would find the import.
- `WeakRef` and `FinalizationRegistry` (the builder's point above): I agree. The other globals left (`queueMicrotask`,
  `SharedArrayBuffer`, `Atomics`, `atob`, `btoa`, `DOMException`, ...) are the same on every computer, and
  `Atomics.wait` can't block here (QuickJS's `can_block` is off), so it can't outlast the 10 s limit.
