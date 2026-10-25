# The Mac's first full check of the agents tracks: build notes for branch `mf3`

**In short**
- The owner's Mac (MacBookPro18,1, M1 Pro, macOS 27.0.1) ran `ci/check.sh` at `a1e3c77` on 2026-10-25. It passed 13 of 16 rows. The Rust tests, the browser suites and the gates failed.
- Branch `mf3` fixes what failed, from the logs and the code, on Linux. The Mac-only code is checked by the mac lint here; the Mac's next run shows the rest.
- **Fixed in the code:** macOS read a plugin host's background state wrongly (M1, M2); a song's build counted its wait for a CPU as work and stopped listens as "endless loops" (M7); `m2` printed a fix's command with the song's real path, not the one it was given (M5).
- **Fixed in the tests and the gates:** the listen tests ran ten listens at once, which a machine never does (M6, M7); a test ended a drag after a fixed time (M7); G13's listens were too long to land in its minute on the Mac's slow cores (M3); the check named a line of JSON as a failed test (M8); G12's set-up line read as if the floating-point register were wrong (M4).
- **Not fixed:** the Mac's output was at 44.1 kHz again, so G12 can't pass its set-up until it is set to 48 kHz (M4). `bus-mix` failed on the Mac too, with no cause in its log (M9). `sx-follow` failed once, its page closed under it, and passed on its retry (M7).
- Two loose ends found on the way are in [gaps.md](../../gaps.md): how long a listen waits on a Mac's slow cores is the owner's call, and a background plugin host's audio threads still ask for real time.

## What failed, and what changed

The logs are in `~/m2-runs/mac-a1e3c77/` on the build server. Each fix's test was seen failing first where Linux can show it; the logs are in `~/m2-runs/mf3/`.

| Id | What failed on the Mac | Why | What changed | Shown here |
|---|---|---|---|---|
| M1, M2 | `m2_listen_runs_at_background_priority`: the plugin host "normal". `a_host_asked_to_runs_at_background_priority`: `Some(false)` | The host puts itself in the background (`setpriority` on itself). Darwin keeps that apart from a state another process sets, and `getpriority(PRIO_DARWIN_PROCESS, pid)` asked of another process reads only the second (xnu's `get_background_proc`). The listen asking about itself read "background"; about its host, "normal". | `in_background` reads `proc_pidinfo`'s flags, which hold both (`PROC_FLAG_DARWINBG`, `PROC_FLAG_EXT_DARWINBG`): `core/os/src/mac/mod.rs` | Mac-only: the mac lint passes. Linux reads each thread's policy, which the host sets itself, so no Linux test can show it. |
| M3 | G13: "listens beside it: 0 landed", so no listen said its priority | One listen of 8 bars of p50 takes 13.6 s of one fast core here. On the Mac it runs on the two efficiency cores at the lowest priority, and it was still at work when the minute ended. | G13's listens are of 2 bars (33-34, 5.0 s here), so several land in the minute; the load beside playback is the same, a listen at work all the time. G13 says how long the one at work when the run ended had run, and gives no mean when none landed (it said "-0.0 s"). With M1's fix, a listen's host is read right. | G13 here: PASS, 18 listens landed, 3.3 s each, all in the background with their hosts; `what_was_still_at_work_when_the_run_ended_is_said` failed first |
| M4 | G12's set-up: "its FP register 0x0: not the designed set-up: the output plays at 44100 Hz" | The gate asks nothing of the register: it is the device thread's own floating-point mode, printed as information, which each callback puts back (RENDER-2a). FPCR 0 is arm64's default, the mode music2 renders in. What failed is the rate: the output was at 44.1 kHz, as at `e7518f4`. | The line starts with what is wrong, and names the mode as information. The owner sets the output to 48 kHz in Audio MIDI Setup. | `the_set_up_says_what_is_wrong_first` failed first |
| M5 | `the_app_killed_mid_commit_names_its_fixes`, twice: the fix lines named `/private/var/…/song` | A forwarded commit that the app left half done is told with the app's real path (`App.dir`, canonical). On a Mac the temp folder `/var` is a link to `/private/var`. Every other command prints the path as it was named. | `App` keeps the path as named (`named`) for what it prints: `apps/m2/src/app.rs` | `the_fixes_name_the_song_as_it_was_named` names the song through a link on Linux too, and failed first with the Mac's message |
| M6 | `a_busy_app_never_holds_up_a_listen`, twice: "it didn't wait for the app" | Not the app: the listen of one bar didn't end in its minute. Each listen test has a home of its own, so `~/.music2/listen.lock` never kept their listens apart, and ten ran at once, each at background priority, on the two efficiency cores. The test binary took 734 s, and 197 s on its retry; on two Linux cores it takes 16 s. | The listen tests take turns (`one_listen`, and the nextest group `listens`), as AGENT-7 says a machine does. A listen that fails or is ended now says what it said. | here 25 s for the binary, on all cores or two |
| M7 | Passed on the retry: four listen tests, the knob drag test, `sx-follow` | The four listens: "the song took too long to build (is there an endless loop?)". A build may run 10 s, and the clock counted the time it waited for a CPU. The knob drag test ended the drag after a fixed 1.5 s; `m2`, slow to start beside the other tests, reached the app after it and never waited. `sx-follow`: Chrome's page closed 2 s in ("Target closed", during a `$$eval`); the log says no more, and the retry passed 22 of 22. | A build's limit is 10 s of its own thread's CPU time (`Limit` in `core/script/src/lib.rs`). The knob drag test ends the drag once `m2` said "waiting: you're dragging". `sx-follow` is listed in HANDOFF's known flakes. | `a_build_that_waits_for_a_cpu_is_no_endless_loop` (in `SCHED_IDLE` beside a busy thread) failed first with the Mac's message; the knob drag test failed first with `m2` started 2 s late, the Mac's message |
| M8 | "test 1" in what failed | The check took every line of four spaces and a word as a failed test's name, and a panic message's JSON held `    1` (`"bars": [1, 1]`). | Only the list after a binary's last `failures:` line names what failed. The tally moved to `ci/tally.sh`, tried by `ci/tally-test.sh` in `ci/pr.sh lint`. | `ci/tally-test.sh` failed first, 0 of 3 |
| M9 | `bus-mix-fix`: "with nothing sent, the Reverb's meter stays at the floor", twice | Not known. On Linux it failed 3 of 31 runs; on the Mac 2 of 2. The check's message doesn't say the level it read, so the log shows no cause. | Nothing: it stays in the nightly tier. Its message could say the level it read, so the next failure shows it. | — |

## Choices

1. **macOS reads `proc_pidinfo`, not a state set from outside.** The parent could also set the host's background state from outside (`setpriority` with the host's pid), so that `getpriority` reads it. But `in_background` answers for any process, whoever put it in the background; the flags hold both ways.
2. **The build's limit is CPU time, read only once the clock passed it.** A thread never works longer than the clock runs, so the many checks QuickJS makes before then cost one clock read, as before. An endless loop is still stopped after 10 s of its own work; on a starved machine that may take longer by the clock, but it never takes anything from anyone.
3. **The listen tests take turns; they don't run at normal priority.** AGENT-7 says one listen at a time on a machine; the tests broke that, not the listen. Running them at normal priority would test something else.
4. **G13's listens are shorter, not its run longer.** The 8 bars were chosen so that "several finish in the minute" on Linux (build notes of `ag-listen`, choice 15); 2 bars keep that on the Mac. The listen loop is at work all the time either way, so the load beside playback is the same.
5. **M4 needs the owner, not code.** The gate is right to say 44.1 kHz isn't the designed set-up.

## For the next Mac run

What should change, row by row:
- **rust tests**: `m2_listen_runs_at_background_priority` says `"plugin_host": "background"`; `a_host_asked_to_runs_at_background_priority` passes; the forward tests pass, the listen tests pass on their first run, and the listen binary takes minutes at most, not 12.
- **gates**: G13 says how many listens landed (several, each of 2 bars), that each and each plugin host ran in the background, and how long one takes on the efficiency cores. G12 passes its set-up only with the output at 48 kHz.
- **what failed** names no "test 1".
- If a listen test still fails, its message now says what `m2 listen` said.

## Checked

- Each fix's test, seen failing first (logs in `~/m2-runs/mf3/fail-first-*.log`), then passing.
- The mac lint (`cargo clippy --target aarch64-apple-darwin --workspace --all-targets -D warnings`, as `ci/check.sh` runs it): passes.
- G13 on Linux at the fix: PASS, 18 listens and 97 edits landed.
- `ci/check.sh --pr`: see the branch's last line in HANDOFF.
