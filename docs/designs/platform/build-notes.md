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
