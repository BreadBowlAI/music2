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
