# Track ph: build notes

Written as the slices land. Plan: `ph-plan.md`. Worktree `~/m2-runs/wt/ph`, branch `ph`, from `main` at `258275b`.

## In short

| Slice | State | What it gives |
|---|---|---|
| 1. The plugin host process (HOST-3) | Built | Plugins run in a second program. If one crashes, the app keeps playing; the plugins go quiet, the app says so, and they come back by themselves |
| 2. Plugin windows | Not started | |
| 3. Latency compensation | Not started | |

## Step 0: the base sound

| What | Result |
|---|---|
| The 10 render records at `258275b` (debug `m2`) | 10 of 10 match `e2e/renders.sha256` |
| `m2 render examples/with-plugins`, twice | **Not the same bytes**: they differ from frame 13 (Surge XT). The worst second is 0.20 dB apart |
| So | "Same sound" for with-plugins is G7e's rule: each second's level within 1 dB of the base render (`~/m2-runs/sph/base-with-plugins.wav`) |

The machine was shared with other tracks all along (load 0.6 at the start, 11 to 13 during the gates). Timings here
are information only (PLAT-1).

## Slice 1: the plugin host process

### What was built

| Part | Where |
|---|---|
| Shared memory by name, and a wait and wake across processes (Linux futex without the private flag; macOS `__ulock` shared; Windows designed) | `core/os/src/ipc.rs`, `linux/ipc.rs`, `mac/ipc.rs` |
| The float mode, moved so the host process uses the same code | `core/os/src/fp.rs` (`m2_runtime::fpenv` re-exports it) |
| Standard output kept for the messages, exit at once, no core dump; real time with no workgroup | `core/os/src/process.rs`, `rt.rs` |
| `Remote`: the app's side. Starts the process at the first call that needs one; one reader thread; a watchdog | `core/plugins/src/remote/mod.rs` |
| The messages (a small codec of our own, and a build stamp both sides check) | `remote/wire.rs` |
| The shared-memory slot: copy in, ring, spin 20 µs, sleep; the host's audio thread puts the float mode back | `remote/slot.rs` |
| The host process: `InProcess` under `run_main`, one audio thread per instance | `remote/child.rs`, `src/bin/m2-plugin-host.rs` |
| `scan` and `counts` on the trait; `HostEvent::Crashed` and `Hung`; `MAX_CALL_EVENTS` | `core/plugins/src/host.rs` |
| Programs install `Remote` (`run_main`, `use_app_main_thread`), and serve when run with `--m2-plugin-host` | `core/plugins/src/main_thread.rs`, `lib.rs` |
| Probe tags `crash-at-<n>` and `hang-at-<n>` (`:<file>`: once) | `core/plugins/src/probe.rs` |
| Plugins found through the host; a node needing more than 16,384 events in a call is refused by name | `core/runtime/src/packages.rs`, `lib.rs` |
| One `plugin.crashed` notice per crash, naming the plugins | `crates/server/src/graph/actor.rs` |
| The app's quit ends the host process | `apps/music2-desktop/src/plugin_host.rs` |

### Tests, each red first

The red diffs and their logs are in `~/m2-runs/sph/red/`.

| Test | Red by | Red result |
|---|---|---|
| `two_mappings_share_their_bytes`, `a_wake_crosses_two_mappings` (`core/os`) | `FUTEX_PRIVATE_FLAG` (`ipc-private`) | the wake never arrived: 5 s timeout |
| `the_host_suite_passes_on_both_hosts` | `Remote`'s `process` stubbed (`remote-stubbed`) | the probe's bits differ |
| `a_crash_marks_every_instance_of_its_process` | instances not marked (`crash-unmarked`) | the other instance's call waits for ever (timeout) |
| `a_hung_call_ends_its_process` | watchdog stubbed (`watchdog-stubbed`) | timeout |
| `the_app_never_loads_plugin_code` | `InProcess` in `Remote`'s place (`no-plugin-code-inprocess`) | Surge XT mapped in the process |
| `a_crash_silences_its_process_and_the_app_plays_on` | the in-process host, as before (`server-crash-inprocess`) | the test process died (SIGABRT) |
| `a_hung_host_process_is_ended_and_said` | watchdog stubbed (`server-hang-no-watchdog`) | timeout |
| `an_export_borrows_instances_in_the_host_process` | the borrow forced to fail (`export-new-instances`) | a new instance, and `plugin.not-borrowed` said |
| `renders_through_the_host_process_are_the_same_bits` | the float-mode reset removed in the host (`same-bits-no-fp-reset`) | differs from frame 1 |
| `the_audio_thread_never_allocates_with_plugins_in_the_host_process` | a `Vec` push in the slot copy (`no-alloc-slot-push`) | the trap counted it |
| `app_main_thread` (the app's hook) | the hook not installing `Remote` (`app-hook-no-remote`) | no host process |

### Checks

| Check | Result |
|---|---|
| Renders | 10 of 10 |
| `m2 render examples/with-plugins` (through the host process) | worst second 0.13 to 0.18 dB from the base (1 dB allowed) |
| `m2-gates plugins` (G1 to G5, G7) | PASS |
| `m2-gates --server export` (G7e) | PASS |
| clippy, the Mac lint | clean |
| The app on Xvfb `:96`: with-plugins looping, `kill -9` of the host process | the app stayed open and kept playing; the toast said: "The plugin host crashed, so the plugins in it went quiet; the rest of the song plays on. music2 started Surge XT (Lead) and Airwindows Consolidated (Lead) again from the saved settings. (plugin.crashed)"; a new host process ran, and the engine load went back to its level with Surge XT playing (0.16). Pictures: `~/m2-runs/app-shots/ph-1/` (`app-after-kill.png` has the toast) |
| A call's round trip (release, Linux, loaded machine; `the_round_trip_is_measured`) | a probe call of 128 frames: 614 µs in process, 625 µs through the host process at the median (about 11 µs more), 670 µs at the 99th percentile (35 µs more), 986 µs at most |

### Decisions taken while building

| What | Why |
|---|---|
| `HostEvent::Hung` beside `Crashed` | The words for a hang say "stopped answering" (D5) |
| The host process writes no core dump | Linux's crash handler (apport) held a crashed process about 500 ms before its pipe closed |
| `crash-at-<n>:<file>` and `hang-at-<n>:<file>` | So the plugin crashes once, and the test can hear it come back |
| `the_app_never_loads_plugin_code` in a test binary of its own | The suite's in-process half scans, which loads every `.clap` into that binary |
| Only the new tests and the programs use `Remote` | Tests that read the probe's records keep `InProcess` (D8); `Graph::with_player_on` and `PackageSet::with_host` give a test the host process |
| The app's hook test now checks the host process, not the probe's records | They live in the host process; `core/plugins/tests/main_thread.rs` still checks HOST-2 there, on the in-process host under `run_main` |

### Owed

- macOS: shared memory, the `__ulock` wake and the app's re-run as the host process are lint-checked only.
- The gaps this slice leaves are in `docs/gaps.md` (Loose ends, "The plugin host process").
