# Track ph: build notes

Written as the slices land. Plan: `ph-plan.md`. Worktree `~/m2-runs/wt/ph`, branch `ph`, from `main` at `258275b`.

## In short

| Slice | State | What it gives |
|---|---|---|
| 1. The plugin host process (HOST-3) | Built | Plugins run in a second program. If one crashes, the app keeps playing; the plugins go quiet, the app says so, and they come back by themselves |
| 2. Plugin windows | Built | A plugin's own window opens from its box. A knob turned there writes the code, latches a lane, or saves the plugin's settings (one Undo step) |
| 3. Latency compensation | Built | A plugin that plays late (a look-ahead limiter) is allowed. Every other path waits for it, so tracks stay on the beat, live and in exports. A song with no latency gets no delay: the same plan, the same bits |

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
| `ci/check.sh` at `982c68c` | **PASS**: 1577 Rust tests, 66 of 66 browser suites, renders 10 of 10, the app (first-song and with-plugins), clippy, the Mac lint, std. Logs: `~/m2-runs/sph/check-982c68c` |
| Disk at the slice's end | 2.2 TB free; this worktree's `target/` is 79 GB (kept for slices 2 and 3); no shared memory left in `/dev/shm` |
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

## Slice 2: plugin windows, state autosave, output events

### In short

| You can now | How |
|---|---|
| Open a plugin's own window | **Open window** on its box. **Close window** closes it. The window floats on its own |
| Turn a knob the code sets | The number in the code changes when you let go: one Undo step |
| Turn a knob a lane drives | It stays where you put it ("Latched"). **Re-enable** gives it back to the lane. Only while the song is open; never in an export |
| Turn any other knob | The plugin's settings save themselves a second later: one Undo step |
| Load a preset in the plugin | Knobs the code sets go back to the code's values, and the app says so (`plugin.param-reverted`) |

### What was built

| Part | Where |
|---|---|
| The main loop watches fds and runs timers (Linux: a `poll` loop; macOS: `CFRunLoopTimer`, `CFFileDescriptor`; Windows designed) | `core/os/src/main_loop.rs`, `linux/main_loop.rs`, `mac/main_loop.rs` |
| A plain window a plugin's view goes into (Linux: X11 through `libX11.so.6` opened at run time; macOS: `NSWindow` through the Objective-C runtime, the host process an accessory app; Windows designed) | `core/os/src/window.rs`, `linux/window.rs`, `mac/window.rs` |
| CLAP's `gui` (embedded), `timer-support` and `posix-fd-support` on the host's side; `MainCall::Window`, `Reply::Window`, `HostEvent::WindowClosed`, `Info.gui`; forwarded by the host process | `core/plugins/src/gui.rs`, `inprocess.rs`, `host.rs`, `remote/` |
| Gesture events in `Events`; the probe's window, and its scripted acts (`gesture-at`, `preset-at`, `dirty-at`) | `core/plugins/src/host.rs`, `probe.rs` |
| After each live call the plugin node holds, latches or sets back, and marks a changed state; atomics only | `core/runtime/src/lib.rs` (`heard`, `taken_over`), `plugins.rs` (`Overrides`, the set-back ring) |
| A number written in `plugin()` is a control (an origin, a cell, a knob) | `core/script/sdk/core.ts` |
| The session's side: leases and one Tx for held literals, latches and Re-enable, the set-back notice, autosave, an export saving first, the state's `plugins` | `crates/server/src/graph/plugin_window.rs` |
| Routes `POST /api/graph/plugin/window` and `/api/graph/plugin/latch` | `crates/server/src/routes_8b.rs` |
| The box's row: Open window / Close window, Latched, Re-enable; boxes titled by the plugin's name | `web/src/components/NodeGraph.tsx`, `crates/editor/src/view.rs` |

### Tests, each red first

The red diffs and their logs are in `~/m2-runs/sph/red/`.

| Test | Red by | Red result |
|---|---|---|
| `timers_and_fds_run_on_the_loop` (`core/os`) | timers and fds stubbed (`loop-stubbed`) | the timer fired 0 times |
| `a_plugin_window_opens_on_the_host_process_main_loop` (Xvfb `:96`) | the gui not offered (`window-no-gui`) | the open failed |
| `a_gesture_on_a_literal_commits_one_tx` | out events ignored (`literal-out-events-ignored`) | no lease |
| `a_gesture_on_a_lane_driven_parameter_latches_it` | no latch (`latch-none`); then the watcher not told (`told-none`) | the lane's 0.2 played; the windows weren't told |
| `a_dirty_plugin_autosaves_its_state_once` | no autosave (`autosave-none`) | never saved |
| `a_preset_change_outside_a_gesture_is_set_back` | no set-back (`set-back-none`) | the preset's 0.9 stayed |
| `an_export_commits_a_pending_autosave_first` | the export doesn't save first (`export-no-flush`) | the old state was on disk |
| `e2e/ph-window.mjs` (9 checks) | the row naming no plugin (`e2e-window-button-missing`) | no Open window button |

### Checks

| Check | Result |
|---|---|
| Renders | 10 of 10 |
| `m2 render examples/with-plugins` | within G7e's 1 dB per second of the base (worst second 0.22 to 0.30 dB; 0.23 at `c658b5e`) |
| `m2-gates plugins` (G1 to G5, G7), at `6888fd0` | PASS |
| `m2-gates --server export` (G7e), at `6888fd0` | PASS |
| clippy, the Mac lint | clean |
| Browser suites `ph-window`, `live3b` (with the plugins), `graph`, `ports` | 9 of 9, 60 of 60, 19 of 19, 46 of 46 |
| The app on Xvfb `:96`, examples/with-plugins | see below |
| `ci/check.sh` at `c658b5e` | **PASS**: 1584 Rust tests, 67 of 67 browser suites (`ph-window` among them, on its own Xvfb), renders 10 of 10, the app (first-song and with-plugins), clippy, the Mac lint, std. Logs: `~/m2-runs/sph/check-c658b5e.tgz` |
| Disk at the slice's end | 2.2 TB free; this worktree's `target/` is 96 GB (kept for slice 3); no shared memory left in `/dev/shm` |

The app on Xvfb `:96` (pictures in `~/m2-runs/app-shots/ph-2/`):

| Step | Result | Picture |
|---|---|---|
| Surge XT's window in the host process alone | draws, its LFO moving (timers work) | `surge-window-host-process.png` |
| Click the lead's Surge XT box, then **Open window** | Surge XT's window opens; the box says Close window | `run4/app-10s.png` |
| **Close window** | it closes; the box says Open window | `run4/app-20s.png` |
| `xkey` drags Filter 1 Cutoff (a lane drives it), then Filter 1 Resonance (code doesn't set it) | the box says Latched, Re-enable; the state lists "A Filter 1 Cutoff" latched at 0.66; `data/lead/lead.state` changed (52,004 bytes); one Undo puts the old file back, and then nothing is left to undo | `run3/app-20s.png`, `run3/hook2.txt` |

### Decisions taken while building

| What | Why |
|---|---|
| A number written in `plugin()` gets an origin in `@m2/core` | It had none, so it was no control: no Tx could write it (HOME-5). Now it is a cell and a knob like any value in code. The plugin hears it as an f32. No new API, so the SDK stays 1.2.0 |
| The "ring" is a fixed override table on the instance, a ring of set-back ids and a changed flag | The table is the truth for holds and latches, so a lost ring entry can never lose a gesture's end |
| The view names a plugin call's graph node, and titles its box by the plugin's name | The route and the state need the node; "plugin" said nothing |
| The watcher tells windows when the state's `plugins` changes | Found in the app: the box showed its row only after something else refreshed it |
| A set-back in the 3 calls after a state load says nothing | A plugin may say what a loaded state set |
| The e2e suite starts an invisible Xvfb of its own on Linux | Never a real screen; ci/check.sh's suites run with no `DISPLAY` |
| A ghost (a replaced plugin node playing out) holds and latches nothing | `ci/check.sh` at `b5980cc` found building one read a parameter's source by the new plan's index, out of bounds (`an_engine_without_the_arena_fails_the_keep_checks`, m2-gates); fixed in `c658b5e` |

### Owed

- macOS: the run loop's timers and fds, the `NSWindow`, the accessory app and its event pump are lint-checked only.
- The gaps this slice leaves are in `docs/gaps.md` (Loose ends, "Plugin windows").

## Slice 3: latency compensation (RENDER-17 to 21, HOST-6)

### In short

- **What you get:** a plugin or kernel that plays late no longer has to be refused. music2 delays every other path by the
  difference, so everything still lines up. An export starts at song time 0 and keeps its ending.
- **Songs with no latency:** nothing changes. No delay is made, so the plan and the bits are what they were.
- **Live:** a new latency (after a state load, or a plugin saying so) is made up for at the next activation, which the
  session makes at once. The playhead shows the position you hear.
- **Still refused:** latency inside a `poly` voice.

### What was built

| Part | Where |
|---|---|
| The planner: each node's offset (the latest arrival over its inputs), a delay on every other input, the song's latency `L`, and the nodes on a feedback loop that add latency | `core/runtime/src/latency.rs` |
| Delay nodes in the graph that plays: `FOp::Delay` (audio, control) and `FOp::NoteDelay` (notes), after the checked nodes, so every index stays | `core/ir/src/check.rs` (the ops), `latency::insert` |
| Their lines: a ring by the engine clock (a control's starts at its first value), a note ring sized from the lane's room; kept across activations by consumer, input and length | `core/runtime/src/delay.rs`, `plan.rs` (`Shape::Delay`, `Shape::NoteDelay`), `keep.rs`, `continue_delays` in `lib.rs` |
| Each plugin's latency kept in its instance: at activation, after a load, when it says so, after an export | `core/runtime/src/plugins.rs`, `lib.rs` (`latencies`), `crates/server/src/graph/actor.rs`, `export.rs` |
| A late plugin told the transport its offset earlier, from the transport's remembered runs | `delay.rs` (`Past`, `Told`), `PluginNode::process` |
| Offline: the first `L` frames dropped, `L` more rendered; `latency.changed` when a plugin's latency moved | `Renderer::process_on`, `Renderer::latency_moved`, `crates/editor/src/render.rs`, `apps/m2` |
| Live: the status says the position heard; a new latency makes the session activate again | `engine.rs` (`Engine::status`), `actor.rs` (`Open.relatent`) |
| The check refuses latency only inside a voice; `plugin.latency-uncompensated` is gone; `latency.in-feedback` is said | `core/ir/src/check.rs`, `actor.rs` |
| The probe's `latency-<n>` (plays `n` frames late) and `relatency-<m>-at-<k>` | `core/plugins/src/probe.rs` |

### Tests, each red first

The red diffs and their logs are in `~/m2-runs/sph/red/`.

| Test | Red by | Red result |
|---|---|---|
| Planner: a chain, two paths, a diamond, a loop (`core/runtime/src/tests/latency.rs`) | planner stubbed (`planner-stubbed`) | 4 of 5 fail: no offsets, no delays |
| `no_latency_makes_no_delay` (planner) | every node 1 frame late (`planner-forced-1`) | 1 delay and `L` 3 |
| `a_latent_plugin_stays_aligned_offline` | compensation stubbed (`compensation-stubbed`) | `L` 0, the paths don't cancel |
| `a_latent_plugin_stays_aligned_live` | same | same |
| `lanes_notes_and_time_arrive_late_by_the_offset` | same; the plugin's offset not passed (`offset-not-passed`); the note delay not read (`notes-not-delayed`) | told the transport of now; the song differs |
| `a_latency_change_applies_at_the_next_activation` | a load's change ignored (`change-ignored`) | no change listed |
| `a_no_op_activation_keeps_every_delay` | delays never continue (`delays-not-kept`) | fresh lines and fades |
| `delays_allocate_nothing_on_the_audio_thread` | a `Vec` in the delay line (`delay-allocates`) | the trap counted it |
| `no_fixture_gets_a_delay` (`core/play/tests/bit_identity.rs`) | every kernel 1 frame late (`no-fixture-forced-1`) | first-song: `L` 11, 7 delays |
| `a_latency_change_during_an_export_is_said_and_compensated_after_it` (`crates/server`) | the render's check stubbed (`export-change-ignored`); the session's scan removed (`live-change-ignored`) | no `latency.changed`; never activated again (timeout) |
| `unsupported_things_are_rejected_at_check_time` (`core/ir`) | the old refusal (`check-refuses-all`) | a top-level latent kernel refused |

A stress run with every std kernel 37 frames late ran the `m2-play` and runtime suites: every failure was a live render
compared with an offline one (they differ by `L` then), none a crash or an assertion inside the engine.

### Checks

| Check | Result |
|---|---|
| Renders | 10 of 10 |
| with-plugins against the base (G7e's rule) | worst second 0.47 dB apart |
| `~/m2-runs/sph/check.sh` (runtime, ir, plugins, play, editor, server) | 1112 passed; clippy and the Mac lint clean |
| `m2-gates plugins` (G1 to G5, G7) and `m2-gates --server export` (G7e) | PASS |
| `ci/check.sh --gates` at `b0ebc74`, first run (load 9 to 10 from other tracks) | every row passed but the gates: G11 tx-data's session part was 5.9 ms at p95 (bound 5) with 5,000 assets. Alone, 3 runs of 3 passed it (p95 1.5 to 1.8 ms) |
| `ci/check.sh --gates` at `b0ebc74`, second run (load 3 to 4) | **PASS**: 1597 Rust tests, 67 of 67 browser suites, renders 10 of 10, the app (first-song and with-plugins), clippy, the Mac lint, std, the 7 gate runs. Logs: `~/m2-runs/sph/check-b0ebc74.tgz` (the first run's: `check-b0ebc74-run1.tgz`) |
| Disk at the track's end | 2.1 TB free; this worktree's `target/` is 105 GB; no shared memory left in `/dev/shm` |

### Decisions taken while building

| What | Why |
|---|---|
| A delay is a node of the graph that plays, after the checked nodes | Keeping, ghosts, fades, overlays and partitions then treat it as any node; every checked index stays, so the session's names and maps hold |
| New plugin instances are made before the wiring | Their latency decides the delays, and is only known once one is activated |
| `latency.changed` asks each plugin again when the render is written | `m2 render` hears no plugin events; asking works for every render |
| A live latency change needs no restart | The session activates again with the kept instance; a plugin that asks for a restart (CLAP's way) is new at that activation anyway |
| A control's delay holds its first value before it starts | A lane or `time` read late must not read 0 before the song starts (a gain lane would mute) |
| The gates table's row "9" (D16) | Written here, for the track: G9a crash, G9b same bits, G9c alignment |

### Owed

- No installed plugin has latency (Surge XT and Airwindows have none), so the alignment was checked with the probe, exactly,
  not by ear.
- The gaps this slice leaves are in `docs/gaps.md` (Loose ends, "Latency compensation").


## The review, the merge and the final check

### In short

A review found 4 things. The big one is fixed, with two tests shown red first. Two small ones are fixed (one line
each). One is a line in `docs/gaps.md`. Then `main` (with `io`) was merged in, and `ci/check.sh --gates` passed.

| Finding | Size | What was done |
|---|---|---|
| ph-r1: only a live call had a limit. A main-thread call, an answer awaited or an offline call could wait for good, so one stuck plugin froze the session, or kept "Paused for export" until quit | big | Fixed (`46e32d4`): every wait has a limit (HOST-3's hang row) |
| ph-r2: a hold released during a gesture kept its override slot, so the plugin was never lent to an export again | small | Fixed (`09d6489`): `Overrides::end` frees a slot left with no flag, as `release` does |
| ph-r3: the host program's stamp didn't cover `host.rs`, which sets the slot's layout | small | Fixed (`5e44336`): `host.rs` is in the stamp |
| ph-r4: one broken `.clap` file makes every plugin say "not installed" | small | A line in `docs/gaps.md` |

### Decisions

| What | Why |
|---|---|
| One limit, 30 s (`remote::LIMIT`), for every call that isn't live: offline, on the main thread, or an answer awaited | Long enough for a big plugin's start or settings load. Short enough that a stuck plugin can't freeze the session for good. The owner's Mac may move it |
| A scan gets four times as long (2 minutes) | It loads every plugin's library; the first time, a Mac checks each one |
| A reply's limit runs from when it was sent; the watchdog checks every reply awaited | One place for every hang: `call`, the answers awaited (`latency`, `save_state`) and calls nobody waits for all end the same way. No caller changed |
| A stuck call ends the whole host process, as a live hang does | The call can't be taken back while the plugin holds the main loop. Every plugin comes back from its saved settings, and the person is told once (`plugin.crashed`, "stopped answering") |
| The export's stop flag isn't read inside the wait | It is set only when the app quits, and quitting ends the host process, which ends any wait at once |

### Tests, each red first

The red diffs and their logs are in `~/m2-runs/sph/red/`.

| Test | Red by | Red result |
|---|---|---|
| `a_main_thread_call_that_hangs_ends_its_process` (probe tag `hangs-activate`) | the watchdog's new checks stubbed (`limits-stubbed`) | the call never returned in 10 s |
| `an_offline_call_that_hangs_ends_its_process` | the same | the same |
| `a_hold_released_mid_gesture_frees_its_slot_at_the_end` | the old `Overrides::end` (`overrides-end-keeps`) | a slot still taken |

### The merge of `main`

`main` at `3b447b2` (`io`: disk streaming and recording audio) merged in at `dcf075d`. Conflicts: m2-os's re-exports
(both sides' names kept), `HANDOFF.md` and `docs/gaps.md` (both sides' lines kept). The first full check found one
clash the merge made: `io`'s `mac/av.rs` and this branch's `mac/window.rs` declared `sel_registerName` with two return
types, which the mac lint refused. Fixed in `659037f`, with a clippy finding in the probe's new tag.

### The final check

| Run | Result |
|---|---|
| First, at `dcf075d` | Failed 3 rows: clippy (the probe's new loop), the mac lint (the merge's clash), and "repo unchanged" (docs edited while it ran). Every other row passed: 1,628 Rust tests, 68/68 browser suites, renders 10 of 10, the 7 gate runs. Logs: `~/m2-runs/sph/check-dcf075d-run1.tgz` |
| Second, at `659037f` | **PASS**; 1 passed only on a retry: the browser suite `st-audio-fix` (from `studio`, not this branch's code; Known flakes). 1,628 Rust tests, 68 browser suites, renders 10 of 10, the app, clippy, the mac lint, std, the 7 gate runs, the repo unchanged. 39 minutes. Logs: `~/m2-runs/sph/check-659037f.tgz` |

The check's summary, word for word:

```
music2 check: PASS; 1 passed only on a retry
commit   659037f929ac on ph (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 39 min
plugins  /home/jerry/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)

toolchain        PASS  rustc 1.99.0
web build        PASS  
build            PASS  
app              PASS  the UI connected: first-song in 1 s, with-plugins in 1 s
rust tests       PASS  1628 passed, 0 failed
clippy           PASS  
pool models      PASS  29 passed, 0 failed
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
mac lint         PASS  the macOS code type-checks and lints
browser suites   PASS  67/68 suites ok, 1 failed; passed on a retry
renders          PASS  10 of 10 match e2e/renders.sha256
old engine (S)   PASS  built for G7f from a clean copy of 8aa301e, in /home/jerry/.cache/m2-check/s-8aa301e
gates            PASS  the 7 runs ci/gates.txt names
repo unchanged   PASS

passed only on a retry:
  suite st-audio-fix
```

### Pictures

On Xvfb :96, 1440x900: `~/m2-runs/app-shots/ph-659037f/` (`captions.txt` says what each shows). The song was made
20 minutes long so its export was still rendering when the plugin program was frozen (SIGSTOP,
`~/m2-runs/sph/tools/hang-export.sh`). 30 s later music2 ended it and playback went on. The export then finished
with the lead silent from the freeze and said only "Exported": a line in `docs/gaps.md`.
