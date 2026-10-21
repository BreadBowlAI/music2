# Attack on the platform design: verification and slices

Label `attack-verification`. Written 2026-10-14 on the Linux server, against `design.md` revision 1 at `f126d03`.

## In short

The plan's main claims hold here. I built them in a scratch copy and ran the design's own `ci/check.sh`:
- all 29 browser runs passed (twice);
- the with-plugins Rust tests passed;
- `std.wasm` rebuilt to the same bytes in another folder;
- the 8 renders matched.

Five problems are left. None of them changes the layer's shape.

| # | Problem | How bad | Fix, in one line |
|---|---|---|---|
| 1 | Timed gates have no Mac procedure. Step 7's Mac run can't judge step 7. | high | `ci/check.sh --gates '<args>'`: same command, same paste |
| 2 | One flaky run on the Mac means a second 30–45 min owner run. | medium | Retry once, print why, pack the logs, `caffeinate` |
| 3 | A run without plugins says PASS. The Linux plugin builds can't be fetched again. | medium | "PASS without plugins"; `ci/plugins.sh` with recorded hashes |
| 4 | HANDOFF's "long-path flake" always fails when the checkout path is over 48 characters. | low | Give QuickJS short package module names |
| 5 | In S1 and S2, `check.sh` calls the old `std/build.sh`, which rebuilds `std.wasm` in place. | low | Ship the new `std/build.sh` in S1 |

---

## 1. Timed gates have no Mac procedure (high)

**Where:** §7.1 (`check.sh` runs no gate), §7.6 (a Mac run at the end of step 7), §7.7 ("needs no Mac timing run"),
and §6.1's new text: "or, for timings, in a recorded run on the reference projects".

**Scenario.**
- Step 7 is next. design-7 §12 says: "Timed gates run in a recorded release run on the reference Mac (M1)".
- The lead now works on Linux. The owner runs only `ci/check.sh`. That script builds in debug and never runs
  `m2-gates`.
- So G7a to G7f can't be judged anywhere. The step 7 Mac run in §7.6 checks everything except what step 7 needs
  the Mac for.
- The same gap exists in this step:
  - OS-20 and OS-22 are marked **Built** in S8, not Moved: the FSEvents source gets the `Heard` callback, and the
    hints move into `events.rs`.
  - OS-81 changes the gates' own code in S6 and S8.
  - G11 `watcher` (idle CPU under 1% of a core) and G11 `tx-data` (PLANE-5, p95 under 20 ms) are judged on the
    Mac.
  - Even so, §7.7 says the step needs no Mac timing run. Nobody runs the changed gates code on a Mac before
    step 7 depends on it.

**Fix (simple).**
- Add one optional argument: `ci/check.sh --gates '<m2-gates arguments>'`.
- After the other checks, it builds `m2-gates` in release and runs it with those arguments, and saves the output
  in `gates.log`. Then it adds one summary row per gate: the verdict and the number. The machine line already
  names the computer, as PLAT-1 asks.
- The lead names the arguments when asking for a Mac run:
  - this step: `--server tx-data watcher`;
  - step 7: the list in design-7 §12.
- It is still one command and one paste. The gates already set `M2_AUDIO=none` themselves (`crates/gates/src/main.rs`
  line 74).
- Tell the owner to run it on mains power with other apps closed. Also fix §7.7 and the §6.1 text so they name this
  command.

## 2. One flaky run on the Mac means a second owner run (medium)

**Where:** §7.1 steps 4 and 8, §7.5 ("If a row fails").

**Scenario.**
- HANDOFF: on the Mac, `s5-knobs` failed 3 of 11 release runs.
- `check.sh` has no retry. One flaky run makes the headline `music2 check: FAIL`, and the lead can only ask again.
  That costs 30 to 45 minutes of the owner's Mac.
- For a real failure, the summary names the test but not why. §7.5 then asks the owner to find a log file and send
  it ("or its last 50 lines"). That is hard to do, and harder for a dyslexic owner.
- A MacBook on battery may go to sleep during a long unattended run *(unverified)*. Sleep breaks the tests that
  measure time and the 600 s suite timeouts.

**Fix.**
1. Re-run each failed browser run once on its own (`node e2e/run.mjs <suite> <kind>`), and each failed Rust test
   once by its exact name. A pass on retry shows as `PASS (flaky: s5-knobs-fix failed once)`. The lead decides.
2. Under each failed item in "what failed", print its first `FAIL` or panic line, at most 160 characters.
3. Pack every log into one small `~/m2-check/<sha>.tgz`. Leave out the `.wav` files and the kept project folders:
   a failed `fx-fixes` keeps a file of over 300 MB. Print one line that sends it.
4. On macOS, run the whole script under `caffeinate -i` (one `exec` line at the top).

## 3. A run without plugins says PASS, and the Linux plugins can't be fetched again (medium)

**Where:** §7.1 steps 0, 8 and the summary; §7.4 (CLAP_PATH passed by hand); §7.5 ("PART is acceptable when
plugins says none").

**Scenario A: the Mac.**
- S5 moves the macOS run-loop pump (`CFRunLoopRunInMode`) into `mac/main_loop.rs`.
- The probe plugin never uses the run loop itself. I checked: `probe.rs` has no dispatch, CFRunLoop or timer calls.
  Only real plugins such as Surge XT do.
- If the owner's plugins are not in `~/Library/Audio/Plug-Ins/CLAP`, `check.sh` sets no `CLAP_PATH`. live3b is then
  PART, and the with-plugins Rust parts skip without a word.
- The headline still says `music2 check: PASS`, and §7.5 accepts it. A pump that S5 broke would ship as "verified".

**Scenario B: the server.**
- §7.4 sets `CLAP_PATH` by hand. A run that forgets it, or an agent that copies the command without it, still prints
  `PASS`.

**Scenario C: the Linux plugin builds.**
- They exist only in `$SCRATCH/plugins`, unpacked by `$SCRATCH/get-plugins.sh`. Neither is in the repo.
- The Airwindows file is a nightly zip, and GitHub deletes the old zip (`ci.md`).
- `$SCRATCH` gets cleaned (the 390 GB episode). After a clean-up, the same build can't be fetched again. A newer
  nightly may change the sound or the saved state.

**Fix.**
- When the plugins line says `none`, the headline says `PASS without plugins`. The step's gate needs plugins in
  the end-of-step Mac run and in every server run.
- Commit `ci/plugins.sh` and `ci/plugins.sha256`. The script fetches Surge XT 1.3.4 and the recorded Airwindows zip
  and checks their hashes. It unpacks them into one user folder that no CLAP search path names, for example
  `~/.local/share/m2-test-plugins/clap`. On Linux, `check.sh` uses that folder as `CLAP_PATH` when it exists.
- Keep a copy of both archives outside `$SCRATCH`.
- Print each plugin's version in the summary. The Mac uses whatever versions the owner installed.

## 4. The "long-path flake" always fails in a long checkout (low)

**Measured.**
- My scratch checkout was `$SCRATCH/scratch-attack-verification/tree` (52 characters).
- There, `files::tests::diagnostics_say_where_in_the_project_each_problem_is` fails on every run with:
  `Could not find export 'nope' in module 'std/src/ds'`.
- The cause: QuickJS writes module names into a 64-byte buffer (`ATOM_GET_STR_BUF_SIZE 64`, `quickjs.c:3821`; the
  message is at line 31042).
- std's module name is the absolute path `<checkout>/std/src/dsp.ts`. So any checkout path longer than 48 characters
  fails.
- HANDOFF calls this a flake. `branches.md` met it in a scratch run.

**Scenario.**
- The review and skeptic agents must verify in `$SCRATCH/scratch-<label>/…` copies. With a label of about 16
  characters or more, `ci/check.sh` says `rust tests FAIL` at every slice. The agents either learn to ignore a
  FAIL, or chase a cause that isn't new.
- Users can get the same cut-off name if the app sits in `~/Applications`
  (`/Users/<name>/Applications/music2.app/Contents/Resources/std/src/drums.ts` is over 63 bytes) *(unverified in
  the app)*.

**Fix (S1, small).**
- Give QuickJS short package module names, such as `std/src/dsp.ts`. `display()` already makes that name. The
  loader maps it back to the file.
- Then the test passes in any folder. Remove the line from HANDOFF's list of flakes.

## 5. In S1 and S2, the check rebuilds `std.wasm` in place (low)

**Where:** §8.2. S1 adds `check.sh`, whose step 6 runs `std/build.sh --check`. But `--check` and the
`CARGO_TARGET_DIR` fix only come in S3.

**Scenario.** Today's `std/build.sh` ignores its arguments. It builds the wasm, then copies it over
`std/kernels/std.wasm`.
- **Without `CARGO_TARGET_DIR`** (a plain run by an agent, a scratch copy):
  - the committed `std.wasm` is replaced by a Linux build with other bytes (this machine's path, and the 10 stale
    bytes);
  - the same run's browser suites and renders then use that file;
  - "repo unchanged" fails;
  - a `git commit -a` in S2 would land an unplanned change to the kernel hashes. That breaks §8.2's rule that only
    S3 changes kernel hashes.
- **With `CARGO_TARGET_DIR`** (as in §7.4): the wasm build is wasted, and the row fails with a confusing
  `cp: cannot stat`.

**Fix.** Ship the new `std/build.sh` in S1, together with `check.sh`. Its `--check` then fails until S3. That is
exactly what §8.1's table expects ("std.wasm FAIL (stale)").

---

## Claims I checked and found sound (measured here)

Setup: a scratch copy of `f126d03` (made with `git archive`), with:
- the ground notes' patches (`renameat2`, `gettid() == getpid()`, `libm` in `audio.rs`, `prctl` in the kill test);
- the design's S2 changes, built as §4.4 writes them, with a simple form of Q1 in `make()`;
- S3's kernel move and the new `std/build.sh`;
- S1's files, and the design's `ci/check.sh`, copied word for word.

Logs are in `$SCRATCH/logs/av-*`.

| Claim (design section) | Result |
|---|---|
| S2: 29 of 29 browser runs with the Mac user-agent wrapper, the `native.ts` line, the two waits and Q1 (§4.4, Appendix C) | **29/29, 1034 of 1034 checks**, twice: once alone, once inside `check.sh`. `fx-fixes` passes with Q1 and no change to the suite. |
| The with-plugins Rust tests pass on Linux with `CLAP_PATH` (§7.4, Appendix C) | With the `gettid` fix: `main_thread` "ok (Surge XT too)"; m2-plugins 13 of 13; gates harness 6 of 6, both plugin tests run (not skipped); m2-play `live` 12 ok, including `a_song_with_plugins_plays_live_and_never_idles`. |
| `ci/check.sh` works under Linux bash (§7.1) | Every row and the summary come out as designed. "repo unchanged" PASS. It took **8 min 11 s** with a warm target folder on an idle server; the design estimates 15 to 25 min. |
| Changing `HOME` for the tests | Without `RUSTUP_HOME` set, rustup stops ("could not choose a version of cargo"). The draft exports `RUSTUP_HOME` and `CARGO_HOME` first, which is right. Keep it. |
| `rustup toolchain install` with no argument (§7.1 step 1) | rustup 1.29.1 installs from `rust-toolchain.toml` (into an empty, separate `RUSTUP_HOME`). |
| S3: kernels' tests and clippy `-D warnings` (Appendix C) | PASS, with `m2-kernel` now a member. |
| S3: `std/build.sh --check` (PLAT-5) | The same bytes rebuilt in another source folder and another target folder. The wasm names only relative paths (`src/fx.rs`, `kernel/src/lib.rs`). |
| S3: renders unchanged by the rebuild | `renders.json`, `bit_identity` and the 8 renders all unchanged. |
| The debug `m2` renders the recorded bits after `libm` (§7.1 step 9) | 8 of 8 match, including `39f94764…` and `d752521b…`. |
| The mac lint is worth having (§2.7) | It caught a lint in shared test code (the ground patch's `kill_tests.rs:492`). Linux clippy never reaches that code, because it stops at `m2-plugins`. |

Not measured here: anything on a Mac, the inotify source (not built yet), and S7.

Clean-up: my scratch folder (with a separate rustup, 1.2 GB) and my target folder (42 GB) are deleted. The working
tree is unchanged apart from this file.
