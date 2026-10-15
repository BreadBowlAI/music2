# Step 7's finish: build notes

What each part of step 7's finish did, where it left the design, and what its checks said. The design is
`design-7.md` (revision 2; §18 for the slices, §19 for the owner's answers). Slices 1 to 3 and half of slice 4 were
built on the Mac and came over as one commit, `5079233` on `origin/wip/step7`. Each agent adds its own section.

## Rebase

**In short:** step 7 now sits on `main` (the platform layer) as one commit on branch `step7`. It builds on Linux. Two
tests needed a one-line fix each to compile against slice 4's half-done types. Nothing was cut. On a Mac, `m2-gates`
won't build until `store.rs`'s OS calls move into `m2-os`. 816 tests pass; the 5 failures are slice 4's unfinished
checks and that move. No regression was found. Step 7's gates G7a to G7d and the churn test pass here.

### The rebase

`git cherry-pick 5079233` onto `main` at `b90bacf`, in the worktree `~/m2-runs/wt/s7`. Committed as `c8117c3`
(`step7` = `main` + that commit). Three files conflicted:

| File | Conflict | How it was resolved |
|---|---|---|
| `docs/architecture.md` | PLANE-5's rule: the platform step changed the sync's words (`m2_os::fs::barrier`) and added the Linux numbers; step 7 changed "through a new renderer" to "through an activation that keeps every instance" | Both kept: the platform step's text, with step 7's words for RENDER-26's path |
| | The acceptance-gates table: the platform step added row 6b; step 7 changed row 7 (the count G7f measured, 6,058) | Both kept: row 6b, then step 7's row 7 |
| `core/session/Cargo.toml` | Both sides added a dev-dependency on the same line: `libc` (the kill tests, OS-72) and `m2-plugins` (the probe in `continues`' tests) | Both kept |
| `Cargo.lock` | Neighbouring lines (`m2-os` and step 7's edges) | `main`'s lock, then `cargo metadata` added exactly step 7's two edges: `m2-session` → `m2-plugins`, `music2-desktop` → `m2-play`. No `cargo update`. |

What the merge notes asked to check, and what was found:

| Where | Found |
|---|---|
| `core/runtime/src/audio.rs` | Merged clean. `write_new_wav` still pushes the file and syncs its folder (S7, `audio.rs:80-88`). Every transcendental call outside tests goes through `libm`; `host_math.rs` checks it (results below). |
| `core/plugins/src/probe.rs` | Merged clean. Step 7's new thread counter calls `main_thread::token()`, which `main` keeps as a wrapper. |
| `core/kernel/` → `std/kernels-src/kernel/` | Step 7 doesn't touch it. |
| `crates/server/src/graph/fsevents.rs` (gone on `main`) | Step 7 doesn't touch it. Nothing to carry by hand. |
| `crates/gates/src/store.rs` | Merged as it was: its Mach `task_info` and QoS calls sit under `cfg(target_os = "macos")`. `core/os/tests/one_place.rs` fails on them until they move into `m2_os::process` (platform design §8). Not done here. **Found by the mac lint:** `main` took `libc` out of `m2-gates` (`8d840ce`), so these lines no longer compile for macOS (`store.rs:172`, `:290`). Until the move, `m2-gates` doesn't build on a Mac. The same move fixes both. |
| `apps/music2-desktop` | Merged clean: the debug-build allocation counter (`m2_play::alloc`). |

### Making it build

`cargo build --workspace` built at once, with no warnings. `cargo test --workspace --no-run` failed in two tests
that slice 4 had not caught up with. Each is a one-line fix to follow a type slice 4 changed; neither changes what
the test checks:

| Test | Error | Fix |
|---|---|---|
| `core/runtime/src/tests/live.rs:491` (`prewarming_keeps_kernel_memory_flat`) | The build callback now returns `Make` (new, moved, or lent to an export), not `Option<u64>` | `Make::New(0)`: a fresh instance, as `Some(0)` meant |
| `crates/server/src/graph/tests.rs:1834` (`plays_handles`) | `PluginAt` is now a struct (`node`, `handle`, `inst`, `blob`), not a pair | `p.handle` for `p.1` |

Nothing of slice 4 had to be cut: everything it started compiles.

### Slice 4: what is there

Design §18's slice 4 list, item by item, at `deab06f`. **Done** means built as the design says (deviations named).
**Partial** means some of it is there. **Missing** means none of it. **Broken** means a check that now fails because
slice 4's code changed what it checks, and the check wasn't rewritten.

In short: the runtime and the server side are mostly built. Missing: the web's status words, the G7e gate and its
test, most of the slice's tests, and every contract edit but one. Three old checks are broken.

**State loads into kept instances (§10.2)**

| Item | State | Where |
|---|---|---|
| `Shape::Plugin` loses `state` | done | `core/runtime/src/plan.rs:40-47` |
| `PluginAt.blob` | done | `plan.rs:153-161` (`PluginAt` is now a struct: `node`, `handle`, `inst`, `blob`) |
| The per-node blob compare | done | `core/runtime/src/lib.rs:1592-1624`: each kept plugin's blob hash against the old plan's |
| The removed-blob rule (not kept) | done | `core/runtime/src/keep.rs:172`, `:183`; an export doesn't lend such an instance either (`plan.rs:198`) |
| The loads as prepare's last step | done, one gap | `lib.rs:1592-1624`, after the graph, ghosts, fades and fresh instances. The gap: a load that fails fails the prepare after earlier loads landed in live instances. Harmless today (the host then holds the new blob, so the next prepare's load is skipped, and `reloaded` was set), but the design says no load happens before a prepare that fails. |
| `PluginInstance.reloaded` | done | `core/runtime/src/plugins.rs:17-40`; read at the node's next call, which clears every last-sent value (`lib.rs:587-592`); set by a load (`lib.rs:1612`) and by the export's give-back (`crates/server/src/export.rs:221`) |
| The latency warning | done, as a notice | `Stats.latent` (`plan.rs:308-309`, `lib.rs:1614-1620`); the actor turns it into a notice naming `plugin.latency-uncompensated` (`crates/server/src/graph/actor.rs:709-716`). It is a message, not a Warning in the problems list. |
| The `holds` host fix | done | `core/plugins/src/inprocess.rs:150-152`, `:280-284`, `:345`, `:433`; doc `core/plugins/src/host.rs:33-36` |

**Export (§10.5)**

| Item | State | Where |
|---|---|---|
| `Msg::Hold` | done | `core/runtime/src/engine.rs:54`; waits for the window (`:397-399`); a panic waits for the resume (`:375-376`, `:446-451`); held renders nothing and the clock stops (`:527-530`); 5 ms fades out and in (`:563-568`) |
| `Status.held` | done | `engine.rs:127`, `:268-270` |
| `Player::hold` and `held` | done | `core/play/src/lib.rs:754-761`; `held` is published after the callback returns (`:478-479`) |
| `Basis::Borrow` and `reset_first` | done, renamed | `Basis::Lend` (`plan.rs:220-226`), `PlanInfo::lend` (`plan.rs:188-208`), `Make::Lent` (`lib.rs:772-773`), `reset_first` (`lib.rs:465`, `:593-595`), `Renderer::lending` (`lib.rs:1698-1707`) |
| The export's protocol and its guard | partial | `Lent` (`crates/server/src/export.rs:164-228`): hold, wait up to 2 s, then per instance `deactivate`, `load_state` (skipped when held), offline, `activate`; its `Drop` gives every instance back on every path, then resumes. Two differences from the design: (1) the hold, the wait and the host calls run on the actor's thread (`Lent::take`, from `Actor::export`, `actor.rs:1085-1122`), not on the export thread, so the session waits for them; (2) one failed host call makes the whole export use its own instances beside playback (with a notice), where the design makes only that node fresh and lists it in the export's problems. A failed reactivation goes to `restarts`, as designed. |
| `Open.exporting` | done | `actor.rs:146-148`; settles prepare nothing while it is set (`:497-501`, `:1391`) |
| `ExportDone` | done, as a callback | `Actor::export_done` (`actor.rs:1124-1131`), posted by `Lent`'s `done` (`crates/server/src/graph.rs:702-712`) |

**Status (§10.4)**

| Item | State | Where |
|---|---|---|
| "loading &lt;plugin&gt;" in the engine view | partial | Server: `Owner::loading` and `creating` (`core/plugins/src/host.rs:113-121`, set in `inprocess.rs:262`), `Activity` (`crates/server/src/graph.rs:157-163`, `:489-492`), in the state (`:509-517`) and `/api/graph/status` (`routes.rs:239-243`). Nothing tells clients when it changes; they see it only when they read. **Web: missing.** |
| "paused for export" | partial | Server: `Activity.paused = "exporting"` while held; export events carry `paused` (`export.rs:128`). **Web: missing** (nothing in `web/src` reads `paused` or `loading`). |

**The debug allocation counter (RENDER-28)**

| Item | State | Where |
|---|---|---|
| A counting allocator in `m2-play`, logged once a second | done | `core/play/src/alloc.rs:12`, `:72`; the callback marks its thread (`core/play/src/lib.rs:274-276`); debug builds of the app install it (`apps/music2-desktop/src/main.rs:27-35`); test `an_allocation_in_the_callback_is_counted` (`core/play/tests/no_alloc.rs:458`) |

**Gates**

| Item | State | Where |
|---|---|---|
| G7e server gate (`m2-gates --server export`) | missing | no `export` gate in `crates/gates` |
| G7e probe test `an_export_borrows_and_gives_back_the_live_instances` | missing | the probe's hooks are ready: `Stats.threads` and a fresh `done_to` at `activate` (`core/plugins/src/probe.rs:116-119`, `:482-483`) |
| with-plugins' `new_state` rewritten | **broken** | `crates/gates/src/plugins.rs:720-781` still checks slice 3's "a new Surge, the old one destroyed". The Surge is now kept and loaded, as slice 4 wants, so it fails: "0 created; 2 of 2 kept". The README's with-plugins row says the same old thing. |

**Contract edits (§15)**

| Item | State | Where |
|---|---|---|
| RENDER-1 | missing | `docs/architecture.md:96-99`: still "a second set of plugin instances" |
| RENDER-6a | missing | `:129-131`: no exception for a hold |
| RENDER-26b | missing | `:274-276`: still `LoadState` |
| RENDER-28's `Hold` | missing | `:285-290`; only the Engine shape's comment names it (`:321`, "from slice 4") |
| HOST-5's second half | missing | `:1151-1152`. Its *Today* is now wrong: it says the shape holds the state hash, which the code no longer does. |
| HOST-6 *Today* | missing | `:1154-1155` |
| HOME-4 *Today* | missing | `:1043-1045` |
| HOME-6 | missing | `:1057` |
| The `PluginHost::load_state` doc | done | `core/plugins/src/host.rs:33-36` |
| The Deferred plugin-windows row | missing | `:1231`: its trigger still waits for the arena |

**Tests (§18's list and §13)**

| Test | State | Where |
|---|---|---|
| `a_reload_after_another_load_is_never_skipped` | done, passes | `core/plugins/src/tests.rs:327` |
| `a_load_of_the_held_state_is_skipped` | done, passes | `core/plugins/src/tests.rs:305` |
| `a_state_load_resends_code_owned_parameters` | missing | |
| `a_state_change_loads_into_the_running_instance` | missing | |
| `swapping_two_plugin_keys_moves_the_instances` with distinct blobs | partial | `crates/server/src/graph/tests.rs:2277`: slice 2's form, no blobs; passes |
| The G7e probe test | missing | |
| `settles_during_an_export_wait_then_apply_once` | missing | |
| `lane_previews_play_during_an_export` | missing | |
| `loading_is_published_while_a_plugin_is_created` | partial | missing at the server; the plugins-level `an_owner_shows_what_it_is_loading` passes (`core/plugins/src/tests.rs:339`) |
| `a_hold_freezes_and_resumes_without_a_step` | missing | |
| `no_alloc.rs`'s hold case | done, passes | inside `the_audio_thread_never_allocates_with_plugins` (`core/play/tests/no_alloc.rs:364`): hold, a panic deferred to the resume |
| `a_changed_state_is_a_new_instance_and_the_old_one_goes_with_its_renderer` → "a changed state loads into the same instance" (§13's table) | **broken** | `core/runtime/src/tests/live/keep.rs:857`: the new plan keeps the instance, so it has no handle of its own (`index out of bounds` at `:890`) |
| The desktop app's "a changed state blob is a new instance" | **broken** | `apps/music2-desktop/tests/app_main_thread.rs:191-197`: waits for the old instance's `destroy`, which no longer comes (the state loads into it: `desktop-2` is the same instance). §13 doesn't list this one. |

### Tests and gates on Linux

At `deab06f`, on this server (Ryzen 9 7950X3D, 32 threads, Ubuntu 24.04), load average 2 to 9 (other work ran
beside it). Timings here are for information only: only the reference Mac's are judged (PLAT-1).

**`ci/check.sh`** (11 min; logs and the app's screenshots in `~/m2-runs/check-rebase/`): FAIL, 2 of 12 rows.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected with `first-song` and with `with-plugins` in 1 s each |
| rust tests | FAIL: 816 passed, 4 failed; each failed again on its retry, so none is a flake |
| clippy | PASS (whole workspace, `-D warnings`) |
| std kernels, std.wasm | PASS |
| mac lint | FAIL: `crates/gates/src/store.rs:172` and `:290` (`libc` isn't a dependency of `m2-gates` any more) |
| browser suites | PASS: 29 of 29 |
| renders | PASS: 8 of 8 match `e2e/renders.sha256` |
| repo unchanged | PASS |

The failures, sorted:

| Failure | Kind |
|---|---|
| `os_code_lives_in_m2_os` (`core/os/tests/one_place.rs`): `store.rs`'s Mach calls | expected: closes when they move into `m2_os::process` (platform design §8) |
| mac lint: `store.rs:172`, `:290` | the rebase: `main` dropped `libc` from `m2-gates`. Same fix as above. Not Linux-only: it stops a Mac build of `m2-gates`. |
| `a_changed_state_is_a_new_instance_and_the_old_one_goes_with_its_renderer` | slice 4's unfinished work (a test to rewrite) |
| `the_live_engine_passes_every_gate` (with-plugins' `new_state`) | slice 4's unfinished work (a gate check to rewrite) |
| `app_main_thread` (the desktop app's test binary) | slice 4's unfinished work (a test to rewrite) |

No real regression was found: every other test passes, the slice 1-3 ones included (38 `keep.rs` tests, 7 `continues`,
14 `delta_tests`, the `text.rs` and `sites` tests, G7c's three new tests, G7d, the churn test, `host_math.rs`,
`no_alloc.rs`).

**Step 7's gates** (release; logs in `~/m2-runs/gates-rebase/`):

| Gate | Result | Numbers |
|---|---|---|
| G7a, through the server (p50 from 3:00) | PASS | heard p50 101.9 ms, p95 113.8 ms from the request, compile included (bound 200); after the compile p95 52.1 ms; naming what continues p50 12.4 ms, preparing p50 2.1 ms; 1 or 0 new instances per edit |
| G7a's engine-rig twin | PASS | 26.7 ms after a 71 ms compile; 200 instances kept, 1 new, 0 ghosts, 2 fades; prepared in 14.3 ms; the effect heard (-53.6 dBFS) |
| G7b tails (p50 from 3:00) | PASS | 49 untouched tracks bit-identical for 5,625 blocks; 200 kept, 0 reset. The wrapped pluck, as a Tx and as a file save: the whole mix bit-identical, the fade window included; the pluck kept |
| G7b with `--no-arena` (must fail) | FAILS, as it must | 33 tracks differ; 0 kept, 201 new, 417 ghosts |
| G7c | PASS | `a_no_op_activation_changes_no_bit`, `an_activation_with_rebuilt_lane_data_changes_no_bit`, `an_identity_arrangement_activation_changes_no_bit`; recorded: `live_is_the_export_on_all_of_p16` (108 s), `p16_loops_ten_minutes_without_drift`. The design's `live_is_the_export_on_p50` and `a_no_op_activation_on_p50` don't exist. |
| G7d | PASS | `a_rebuilt_kernel_module_starts_from_init`, `a_wrong_shape_move_is_refused` |
| The churn test | PASS | `random_structural_edits_leak_nothing` |
| G7f part 1 (`store`, p50) | FAIL, not meaningful here | 6,058 kernel instances; build 220 ms (36.4 µs each); 4 renderers side by side; block p50 1,713 µs, p99 3,244, max 5,884; drop 119.7 ms. The rule compares with layout S's record, which is the Mac's: p99 "fails" (3,244 against 3,083 µs) and the build "fails" (220 against 103 ms). The memory rows show 0 MB and NaN%: `memory()` is None on Linux, so those rules pass without measuring anything. |
| G7f part 2 (`arena`, p50 from 3:00) | FAIL, not meaningful here | callback p99 14,013 µs against the Mac's HEAD record 12,910 (mean 7,205 against 8,936); the slowest activation block 3,819 µs, under the 5,333 µs bound (passes) |

**§12.8, gates that keep passing** (release):

| Run | Result |
|---|---|
| `m2-gates` (the engine rig on p16, the recorded run) | G1, G2, G3, G4, G5, G6, G7, G7b, G10 PASS. with-plugins FAIL only on its two "changed state (3b)" checks (slice 4, above); every other with-plugins check passes. |
| `m2-gates --server` (p16) | all PASS: G1 seek p95 19.0 ms; G3 p95 16.4 ms past the compile; G5 and G7a (p50 49.3 ms from the request, at 1:00); G8; G9; G11 `tx-data` (preview p95 8.3 ms idle, 8.0 ms busy, 8.7 ms with 5,000 assets; durable p95 2.9 ms idle) and `watcher` |
| `m2-gates plugins --quick --no-arena` (must fail) | FAILS, as it must: no instance kept |
| `no_alloc.rs`, `no_blocking.rs`, the step 6 Tx tests | PASS (in the test run) |

### Left for the next agent

1. Platform design §8 first: bug 1 in `core/play`; then `m2_os::process::memory()` and `machine()`,
   `thread::interactive()`; `store.rs` uses them, G7f's memory rules fail when memory is None, records name their
   machine. That fixes `one_place.rs` and the mac lint.
2. Slice 4's missing pieces above: the web's status words, G7e (gate and probe test), the missing tests, the three
   broken checks rewritten to slice 4's meaning, and the contract edits (HOST-5's *Today* is wrong now).
3. Judge the two export differences (host calls on the actor's thread; one failed call makes the whole export use its
   own instances) and the failed-load gap in prepare.
4. `ci/gates.txt`: design-7 §12's gates for the Mac run.
5. G7f on Linux can only be judged against a layout-S record made here (a worktree at `8aa301e`); none exists.

## Bug 1

**In short:** one xrun no longer stops play. The player now sorts the audio device's errors by kind. An xrun (a
dropout) is counted and play goes on. A change of output device makes a new player. Only a device that is gone
stalls it. The error callback only adds to a counter. Tests inject errors, since this server has no sound card.
Branch `s7-bug1`, from `step7` at `a06c543`; worktree `~/m2-runs/wt/s7-bug1`.

### What was wrong

Platform design §8's first row, and survey-later §2.2 (owner question O1: "now"). cpal reports CoreAudio's overloads
as errors. The player's error callback set one flag for every error, and `stalled()` read it. So one xrun let the
player go at the next action ("the audio device stopped responding"), which cut every tail. Step 7's export `Hold`
needs a player one xrun doesn't retire. On ALSA the same callback runs on the audio thread, and it printed there.

### What changed

| Where | What |
|---|---|
| `core/play/src/lib.rs` | `DeviceEvent::of` sorts cpal's `ErrorKind`: `Xrun`; `DeviceChanged`; `StreamInvalidated` and `DeviceNotAvailable` are `Gone`; every other kind is `Other`. cpal already turns each OS's xrun into `Xrun` (ALSA's `EPIPE` and suspends, CoreAudio's overload, PipeWire, JACK), so nothing more is needed per OS. |
| | The error callback (`Shared::device_error`) only adds 1 to an atomic count for its kind. It never allocates, locks or prints. |
| | `stalled()` is true for a device that is `Gone`, never for an xrun or another error. A device that stops calling back is still stalled after a second, as before. |
| | `Player::changed()` says the output moved to another device. `Player::device_errors()` gives the four counts. |
| | The stream's thread used to sleep forever. Now it wakes once a second and logs what the device reported, if anything ("music2: the audio device reported 3 dropouts"). |
| | `Driver::error()` gives a driven player an error, as cpal's callback would. Tests use it. |
| `crates/server/src/graph.rs` | `device()` lets a player go when it stalled, crashed, or its output changed (`let_go`). The action that meets it fails and says why: "the sound output moved to another device; it starts again with the next action". The next action, or the watcher's catch-up within 300 ms, loads the project into a new player where it was. |
| | `Activity` gains `xruns` and `device_errors` (since this player started). The engine view (`state()`) and `/api/graph/status` carry both, beside `paused` and `loading`. |
| `docs/architecture.md` | RENDER-28: a new bullet says what a stall is, and its *Today* line says how it is built and tested. |

### Choices (not in the design's words)

| Choice | Why |
|---|---|
| A device change uses the stall's path: the action that meets it fails, and the new player starts stopped at the same beat. | The project must be loaded into the new player before an action can go to it: a play or seek sent to an empty player is lost. This path already does that. Starting stopped is also safer: when headphones are pulled out, cpal moves the old player to the speakers, and the music stops there once it is let go (within about 300 ms, at the catch-up), as before this fix. |
| `HostUnavailable` and every other kind are only counted. | The design says only "invalidated" or "unavailable" stalls. If the device then stops calling back, the one-second rule stalls the player anyway. |
| An error with a text cpal formatted (rare: ALSA's unsorted errors) is freed in the callback. | cpal made that text on the same thread just before. An xrun never has such a text (checked in cpal 0.18.2's source), so the common path frees nothing; `the_device_error_callback_never_allocates` checks it. |
| Counts are per player. A new player starts from 0. | Simplest. Nothing reads them yet; 8b.8's DSP readout will. |
| The JSON names are `xruns` and `device_errors`. | snake_case, as the server's other keys. |

### Checks

On this server (Ryzen 9 7950X3D, Ubuntu 24.04), scratch `HOME`, `M2_AUDIO=none`, the test plugins in `CLAP_PATH`.

| Check | Result |
|---|---|
| `cargo test -p m2-play` | PASS: unit tests 13 of 13 (5 new), `no_alloc.rs` 6 of 6 (1 new), `bit_identity`, `churn`, `knobs`, `live` (12, 1 ignored as before), `no_blocking` |
| `cargo test -p m2-server` | PASS: 146 of 146 (2 new) |
| The old rule put back (any error stalls) | the new tests fail, as they must: `an_xrun_is_counted_and_play_goes_on`, `a_device_change_asks_for_a_new_player_and_plays_on_meanwhile` and the server's `an_xrun_is_counted_and_the_player_plays_on` |
| `cargo build --workspace --all-targets` (after the web build) | PASS, no warnings (2 min 13 s, a fresh `target/`) |
| `cargo clippy --workspace --all-targets -D warnings` | PASS |
| mac lint | FAIL only on the rebase's known `crates/gates/src/store.rs:172` and `:290` (5 errors, `libc`). Without `m2-gates` it passes, so `m2-play` and `m2-server` lint clean for macOS. |
| rustfmt | the new lines are clean; the files' old diffs are unchanged |
| `cargo test --workspace` (for information) | 824 passed, 3 ignored, 4 failed: the rebase's same four (`one_place.rs`; `a_changed_state_is_a_new_instance_and_the_old_one_goes_with_its_renderer`; with-plugins' two "changed state (3b)" checks in `the_live_engine_passes_every_gate`; `app_main_thread`). None is new. 824 is the rebase's 816 and the 8 new tests. |

The new tests (rule ids in their comments):

| Test | Where | Checks |
|---|---|---|
| `the_devices_errors_are_sorted_by_kind` | `core/play/src/tests.rs` | all 14 kinds of cpal 0.18.2 |
| `an_xrun_is_counted_and_play_goes_on` | same | 3 xruns and every other kind: counted, not stalled, the same bits as the offline render, messages still taken |
| `a_device_change_asks_for_a_new_player_and_plays_on_meanwhile` | same | `changed()`, not stalled, plays on bit for bit |
| `a_device_that_is_gone_stalls_the_player` | same | both kinds stall at once |
| `what_the_device_reported_is_said_in_words` | same | the log line |
| `the_device_error_callback_never_allocates` | `core/play/tests/no_alloc.rs` | 0 allocations or frees for 5 errors as cpal makes them |
| `an_xrun_is_counted_and_the_player_plays_on` | `crates/server/src/graph/tests.rs` | no action fails, the same player plays on, the counts in `activity()` and the view |
| `a_device_change_or_loss_makes_a_new_player_where_it_was` | same | each of the 3 kinds: the words, a new player at the same beat, its counts at 0 |

### Left for later

- No test here meets a real xrun: this server has no sound card. The Mac run has a device, but nothing forces an
  overload there either.
- A player let go still leaves its device thread and its open stream behind, as after a stall. Each device change
  now adds one. 8a's new player work is the place to close it.
- The web shows neither count yet (8b.8).
- A device change during an export lets the player go, as a stall does today. By reading the code (not tested):
  the export keeps its instances, a load waits while it runs (`actor.rs:499`), and `export_done` loads the project
  into the new player after it.

### The independent check

**In short:** no bug found in bug 1's code, so its code is unchanged. Every test and lint passes again here, and
probes that tried to break it passed. One flaky step 7 test showed up (not bug 1's). Points for later are below.
Run on this server (Ryzen 9 7950X3D, Ubuntu 24.04), scratch `HOME`, `M2_AUDIO=none`, the test plugins in `CLAP_PATH`,
other agents' jobs beside it (load about 10).

| Check | Result |
|---|---|
| `cargo test -p m2-play -p m2-server` | PASS: `m2-play` unit 13, `no_alloc` 6, `live` 12 (1 ignored), `bit_identity` 6 (1 ignored), `churn`, `knobs`, `no_blocking`; `m2-server` 146. Then `m2-server` 8 more times: 146 each time. |
| clippy, the whole workspace, `-D warnings` | PASS |
| mac lint (ci/check.sh's command) | FAIL only on `crates/gates/src/store.rs:172` and `:290` (5 errors, `libc`), as at the rebase. PASS with `--exclude m2-gates`. |
| The old rule put back (`stalled()` true on any error) | 4 of the new tests fail, not 3: also `a_device_change_or_loss_makes_a_new_player_where_it_was` (a change then says "stopped responding") |
| `m2-gates --server --quick` (release; Linux, for information) | G1 PASS (p95 20.5 ms), G3 PASS (p95 17.4 ms), G5 and G7a PASS (p95 18.9 ms; G7a 31.6 ms), G8 PASS (p95 20.9 ms). `drag` can't run with `--quick`: it edits `tracks/t05.ts`, which p3 has not (its code is from before step 7). G9 on p16: PASS. |
| `the_live_engine_passes_every_gate` | FAIL only on with-plugins' two "changed state (3b)" checks, as at the rebase (slice 4) |
| `cargo test --workspace --no-fail-fast` | 823 passed, 3 ignored. Failed: the rebase's four, and once `swapping_two_plugin_keys_moves_the_instances` (below) |
| The headless server's `/api/graph/status` and `/api/graph` | both carry `xruns` and `device_errors` (0 and 0) |

Probes (temporary, not kept), all passed:
- 40,000 errors from 4 threads while the callback plays: each counted, never stalled, the same bits.
- A change, then the device gone: the player is stalled, and changed too.
- A change with no action: the 300 ms catch-up alone lets the player go and loads a new one at the same beat.
- 10,000 xruns, then an edit: the same player takes the edit and is never let go.
- A knob drag across a device change: the drag goes on in the new player, and its commit is written.

Read in cpal 0.18.2's source: a macOS stream on the default output (as music2 opens one) reports `Xrun`,
`DeviceChanged` and `DeviceNotAvailable`, and `Other` only if the clock can't be read. ALSA and CoreAudio make an
xrun's error with no text that must be freed.

**A flaky step 7 test (not bug 1's).** `swapping_two_plugin_keys_moves_the_instances` (`crates/server/src/graph/tests.rs`,
from `c8117c3`) checks "none created" against the count of plugin instances made in the whole process
(`InProcess::counts`). Another test that makes probe instances at the same time adds to it. Run together with the
other four probe tests, it failed 59 times out of 60 ("left: 11, right: 3" to "10"). Alone it passed 5 of 5. Not
fixed here: a count per owner, or a lock the five probe tests share, would fix it.

**Points for later** (choices or bigger work, not fixed):
- Playback stops on a device change. When the catch-up lets the player go, nobody is told why, as after a stall.
  The owner may want play to go on in the new player.
- The stream's thread now sleeps a second at a time. Design-8a §6.5 plans to wake it with `unpark` to close the
  stream; `park_timeout` would serve both.
- A player let go keeps logging its counts. After a change, its stream plays silence on the new device, so an
  overload there is logged twice.
- An export can start on a changed player in the 300 ms before it is let go: the pause works, then the catch-up
  lets that player go during the export. Reading the code, the new player loads after the export. Slice 4's
  export `Hold` should test this.

## Slice 4

**In short:** slice 4 is built. A new plugin state now loads into the plugin that plays, and an export
of a song with plugins pauses playback and borrows them, on the export's own thread. The web's top bar
says "Paused for export" and "Loading Surge XT…". G7e passes, and so does every slice 4 test. Testing
with the real Surge XT found two real bugs, now fixed: after a state load, Surge kept its old filter
setting instead of the code's (HOME-4), and a borrowed export lost the lead. `ci/check.sh` fails only
on the two known `store.rs` lines. Branch `step7`, worktree `~/m2-runs/wt/s7`.

### What was done first

Bug 1's checker found a flaky test, `swapping_two_plugin_keys_moves_the_instances`. It counted every
plugin instance made in the whole test process. Now each `Owner` counts the instances it made
(`Owner::made`), and the test reads its own session's count. Run beside the other probe tests: 40 of
40 pass; the old count fails 10 of 10 (`cc84d6a`).

### What was built

| Item (design §18) | State | Where |
|---|---|---|
| State loads into kept instances, as prepare's last step | done | `Renderer::prepare` (`core/runtime/src/lib.rs`) |
| A load that fails | changed (deviation 1) | `Stats.unloaded`; the actor says why in a notice |
| `PluginInstance.reloaded` | changed (deviation 2): two calls | `core/runtime/src/plugins.rs` |
| The latency warning | a notice, as before; the code is now in the contract's table | `plugin.latency-uncompensated` |
| `holds` host fix | done (the Mac's) | `core/plugins/src/inprocess.rs` |
| `Msg::Hold`, `Status.held`, `Player::hold` and `held` | done; a hold now also completes when the engine idles or plays nothing | `core/runtime/src/engine.rs` |
| `Basis::Lend` (the design's `Borrow`) and `reset_first` | done (the Mac's) | `plan.rs`, `lib.rs` |
| The export thread's protocol and guard | done as designed: the hold, the wait and the host calls now run on the export's thread, not the actor's | `Lent` in `crates/server/src/export.rs` |
| One borrowed instance that can't be taken | done as designed: a new one plays its node, and the export carries `plugin.not-borrowed` | `Lent::take`, `Renderer::unlend` |
| Playback can't pause in 2 s | done as designed: nothing live is touched, a toast says so | `Lent::take` |
| `Open.exporting`, `ExportDone` | done (`ExportDone` is the guard's callback to the actor) | `actor.rs` |
| "loading" and "paused for export" | done: the web shows both in the top bar | `EngineNote` in `web/src/components/GraphCanvas.tsx` |
| Debug-build allocation counter | done (the Mac's) | `core/play/src/alloc.rs` |
| G7e: the server gate and the probe test | done | `m2-gates --server export`; `an_export_borrows_and_gives_back_the_live_instances` |
| with-plugins' `new_state` | rewritten: one load into the same Surge, the new patch's volume, the code's filter type kept, Surge's own saved state skipped | `crates/gates/src/plugins.rs` |
| Contract edits | done: RENDER-1, RENDER-6a, RENDER-26b, RENDER-28's `Hold`, the engine shape, HOST-5, HOST-6, HOME-4, HOME-6, the Deferred plugin-windows row, two new codes | `docs/architecture.md` |

### Found with Surge XT

Surge XT applies a loaded state at its next `process` call, not at the load. That call's parameter
values are lost, and every voice in it ends, notes started in that same call too. A temporary test drove
Surge directly through the host to show it: after a real load, the notes in the next call made exact
silence; one empty call first, and they sounded.

| What it broke | Fix |
|---|---|
| Live: after a state change, Surge's filter type read the patch's 0.005, not the code's (HOME-4) | The code's parameters now go again at the next **two** calls after a load. with-plugins checks it now; with one call it fails. |
| Export: when a borrowed instance really loads its state (it doesn't hold it), the export's first notes and the code's values were lost. With Surge alone the lead was silent; in with-plugins it was 6 to 15 dB off between the kicks | After a real load, the export makes one empty `process` call on its own thread before the render |
| What stays | Live, a note that starts in the very block after a state change doesn't sound in Surge. It can't be fixed on the audio thread. A state change is rare: an undo of a state write, an agent writing a blob. |

The probe can now act like Surge (`defer_load`), so CI tests show both fixes without Surge. Each test
fails with its fix taken out.

### Deviations from the design

| # | What | Why |
|---|---|---|
| 1 | A state load that fails doesn't fail the prepare. That plugin plays on as it left itself, a notice says why, and the next plan tries again. | The loads before it have landed in instances that play, so failing the prepare then would break "a prepare that fails changes nothing". |
| 2 | After a load, the code's parameters go again at the next two calls, not one. The blob's value plays for up to two blocks. | Surge applies a loaded state at its next call (above). A plugin that applies one later still needs HOME-5a's revert, which comes with plugin windows. |
| 3 | An export that really loads a state makes one empty `process` call before the render. | The same Surge behaviour; without it the export isn't exact (RENDER-1). |
| 4 | The export waits for the engine to have applied *its* hold (the stamp), then for `held`. | `held` alone can still say the last export's. A second export started straight after the first took the instances while the audio thread could still call them; `a_second_export_waits_for_its_own_hold` fails with the old wait. |
| 5 | "loading" isn't pushed to clients. The host's `Owner` keeps the list, and every read sees it as it is; the web polls the status five times a second. | The same thing for the user, with no new event. |
| 6 | `a_state_load_resends_code_owned_parameters` is in `core/runtime`, not `core/plugins`. | `m2-plugins` can't run a renderer: `m2-runtime` depends on it. |
| 7 | G7e's server gate holds the lead's cutoff lane still in its copy of with-plugins. | So the code's values are the same at the two saves "state intact" compares. |
| 8 | G7e's "each second within 1 dB of `m2 render`" is judged over the song's 8 seconds. The release tail after them is printed beside two `m2 render`s' own spread. | Two renders with new instances already differ there by up to 1.5 dB (Surge's random phases). |
| 9 | G7e has one more check: the lead between the kicks, beat by beat, within 6 dB of `m2 render`. Two renders differ by up to about 3 dB there. | Each second's level is mostly the kick, and it missed the silent lead above. This check sees it by 12 to 15 dB. |
| 10 | G7e also runs in `m2-gates --server` with no gates named. | It is quick (about 3 s), and skips without the plugins. |

### New tests

Beyond §18's list (all of which are there and pass):
- `a_state_that_fails_to_load_never_fails_the_prepare` (deviation 1).
- `an_instance_that_cant_be_borrowed_plays_anew_in_the_export`, `an_export_whose_playback_cant_pause_renders_beside_it` (§10.5's failures).
- `a_second_export_waits_for_its_own_hold` (deviation 4).
- `an_export_that_loads_a_state_lets_the_plugin_apply_it_first` (deviation 3).
- The G7e probe test's negative control: with no hold, the audio thread plays one buffer inside the window while the export thread waits there. It never lets two threads call one instance at once.
- live3b exports with-plugins from the UI on real Surge: playback pauses, the top bar says so, the capture is silent and the playhead still, the file is written, and play goes on (60 checks; 40 without plugins).

Tests that showed they can fail: each fix taken out made its test fail (the export deferral, the reload
flag, the warm-up, the two calls, the hold's stamp).

### Results

On this server (Ryzen 9 7950X3D, 32 threads, Ubuntu 24.04), with a scratch `HOME`, `M2_AUDIO=none`
and the test plugins, at `7f7cfd7`. The load average was under 3.

| Check | Result |
|---|---|
| Every slice 4 test, by name | PASS; the server's ten export and state tests 3 times |
| `cargo test --workspace --no-fail-fast` | 838 passed, 3 ignored, 2 failed: `os_code_lives_in_m2_os` (known), and the no-arena control in the gates' harness, which the changed-state check stopped. Fixed (`7f7cfd7`); the harness then passed 6 of 6. |
| clippy (`--workspace --all-targets -D warnings`) | PASS |
| mac lint | FAIL only on `crates/gates/src/store.rs:172` and `:290` (`libc`), as before. PASS without `m2-gates`. |
| with-plugins (`m2-gates plugins`, release) | PASS, every check, the new changed-state ones included |
| with-plugins with `--no-arena` (must fail) | FAILS, as it must: no instance kept |
| G7e (`m2-gates --server export`, release) | PASS 3 of 3 (below) |
| `ci/check.sh` (`~/m2-runs/check-s7-slice4/`, 8 min) | FAIL, 2 of 12 rows: rust tests (839 passed; only `os_code_lives_in_m2_os`) and the mac lint (`store.rs`). Every other row passed: app, clippy, std kernels, `std.wasm`, 29 of 29 browser suites (1,043 checks), 8 of 8 renders, repo unchanged. Nothing passed only on a retry. |

G7e's numbers (one run; Linux, for information only, PLAT-1):

| | |
|---|---|
| The export, until done | 267 ms; playback paused 256 ms |
| Held before the first host call | yes: the first call 320 ns after the engine said it held |
| While paused | 244 samples, the engine clock and the transport the same in all; silence (-240 dBFS) |
| Instances | the same two before and after, none made; each one's saved state the same after the resume |
| The resume | at the clock it held (48,384), 10.0 ms after the instances were back (bound 21.3) |
| Steps at the fades | 0.0134 out, 0.0059 in; bound 0.0534 (twice the music's own) |
| The file against `m2 render` | 18.00 s against 17.71 s; each second within 0.11 dB; the lead within 2.74 dB beat by beat (two renders: 2.13); 15 of 15 onsets; the tail 1.05 dB off (two renders: 0.66) |

### Left for later

- `crates/gates/src/store.rs`'s Mach calls still need `m2_os::process` (platform design §8). That one
  move fixes both `one_place.rs` and the mac lint.
- `ci/gates.txt`: add design-7 §12's gates, G7e included, for the Mac run.
- After an export, the plugins' voices are reset, as RENDER-1 says. A note held across the export stays
  silent until its next note-on. In live3b, a long chord stayed silent until a seek.
- A plugin that applies a loaded state later than its next call (a sampler loading in the background)
  could still win over the code's values. HOME-5a's revert, with plugin windows, is the real fix.
- The latency warning is a toast, not an entry in the problems list.
