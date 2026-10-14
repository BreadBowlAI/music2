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
