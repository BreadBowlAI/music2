# Step 9, synth and plugin turns 1 to 3: the plan (track `ph`)

Written 2026-10-18. Read only; nothing built. Base: `ph` at `258275b` (= `main`), worktree `~/m2-runs/wt/ph`.
Read: `docs/architecture.md` (HOST-1 to HOST-7, RENDER-1, 2a, 17 to 21, 25, 26b, 28, HOME-4 to 6, MON-6, FMT-17,
PLAT-1 to 6, the diagnostics table, Order of work 9, Deferred), `HANDOFF.md`, `docs/gaps.md`, and the code named below.

## In short

- **Three slices, in this order.** Each ends green on Linux, with `ci/check.sh` run through `~/m2-runs/tw`.

  | # | Slice | What you get |
  |---|---|---|
  | 1 | The plugin host process | Plugins run in a second program. If one crashes, the app keeps playing; that plugin goes quiet, says so, and comes back by itself |
  | 2 | Plugin windows | A plugin's own window opens and closes from its box. Turning a knob there saves: in the code, as a latch, or in its saved settings (one Undo step) |
  | 3 | Latency compensation | Plugins that delay their sound (a look-ahead limiter) are allowed, and every track stays on the beat, live and in exports |

- **Same sound.** No render record changes. `examples/with-plugins` renders the same bytes as before. A song with no
  latency gets no delay at all, so its plan is what it was.
- **One plugin path.** The app, `m2`, the gates and the headless server all use the host process. The in-process
  host stays only inside that process (HOST-2 says the host process keeps its rule), and in tests that read the
  probe's own records.
- **macOS:** its code (shared memory, the wake, AppKit windows) is lint-checked only. Windows: designed for, not built.
- **Slice 3 may run beside slice 2**, in a second worktree from `ph` after slice 1, merged back (never rebased).

## 0. Before anything

| Step | Command or rule |
|---|---|
| Environment | every cargo or music2 command: `CARGO_HOME=/home/jerry/.cargo RUSTUP_HOME=/home/jerry/.rustup`; every run: `HOME=~/m2-runs/sph/home M2_AUDIO=none M2_MIDI=none` |
| Base sound | at `258275b`: render the 10 records (`e2e/baseline.sh`), and `m2 render examples/with-plugins` **twice** with the test plugins. Write both hashes in the build notes. If the two runs differ, say so; then "same bytes" falls back to G7e's 1 dB per second |
| Per-commit check | `~/m2-runs/sph/check.sh <crates>`, copied from `~/m2-runs/s8a/s2-check.sh` with this worktree and scratch: builds `m2` and `m2-plugin-host`, the 10 renders, with-plugins against the base hash, the crates' tests, clippy, the Mac lint, fmt of new files |
| Red runs | each new test is run once red, by a small stub or bug in `~/m2-runs/sph/red/<name>.diff`, log beside it |
| Long jobs | `TW_WAIT=1 ~/m2-runs/tw ph-<what> '<cmd>'` (tmux `m2-sh`); `uptime` first, said in the notes |
| Screens | the app and every window test only on Xvfb `:96` (`~/m2-runs/sph/tools/app-shot.sh`, scratch paths under `sph`) |
| Disk | `df -h /` at each slice's end; delete the second worktree's `target/` when it is merged |
| Never | touch `main`'s checkout or other worktrees; push; rebase; `cargo update`; delete under `~/m2-runs/plugins` |
| Notes | `docs/designs/step9/build-notes-ph.md`, written as the slices land |

---

## 1. Slice 1: the plugin host process (HOST-3)

**Goal.** Plugins run in a separate process. Audio and events cross in shared memory; main-thread calls cross as
messages; the process runs its own main loop. A crash or a hang makes that process's instances silent with
`plugin.crashed`; the app plays on; each instance comes back at the next activation from its committed state (HOST-5).
Offline renders give the same bits.

**How it works.**
- `m2_plugins::Remote` implements `PluginHost` (HOST-1). It starts its process at the first call that needs one,
  so a pure song starts none, and starts a new one after a crash.
- The process is the same program run again with `--m2-plugin-host` (`run_main` and `use_app_main_thread` look for
  it first and serve instead). Tests use a small `m2-plugin-host` binary in `m2-plugins`. Inside, the process is
  today's `InProcess` under `run_main`: every main-thread call on its main loop (HOST-2).
- Messages: framed binary on the child's stdin and stdout (std pipes, no OS code), with request ids. One reader
  thread in the app routes replies, and sends each `HostEvent` to its `Owner` (owner ids on the wire).
- Audio: one shared-memory slot per active instance (`m2_os::ipc::Shm`): a header (call word, status, "asked to be
  processed"), the transport, 2 × `max_frames` in and out, events in (CLAP's own layout, `MAX_CALL_EVENTS`) and out
  (256). `process` and `reset` copy in, ring, spin about 20 µs, then sleep on the word; the host process has one
  audio thread per instance, asleep on its slot. After each plugin call that thread puts the one float mode back
  (RENDER-2a). No allocation, lock or I/O on the app's side (RENDER-28): a fixed slab of slots, as `InProcess` has.
- Crash: the reader sees the pipe close. Every instance of that process is marked crashed, and waiters are woken.
  `process` returns `Status::Error`, so today's "fails live" path runs (`PluginInstance::kill`, `Open.restarts`).
  The new `HostEvent::Crashed(h)` makes the actor raise `plugin.crashed` (Error, on the session), once per crash,
  naming the plugins. Hang: a live call that waits 500 ms (RENDER-28's stall time) makes a watchdog kill the
  process; the same path follows. Offline calls wait with no limit but end on a crash, which fails the render.
- Finding plugins moves behind the host: the trait gains `scan()`, and `describe_plugin` and `m2 plugins` use it.
  The app never loads plugin code.
- The probe learns tags `crash-at-<n>` (aborts its process at its n-th call) and `hang-at-<n>`, sent in its state.

| Crate | Files |
|---|---|
| `m2-os` | new `src/ipc.rs`, `linux/ipc.rs`, `mac/ipc.rs`: named shared memory (`shm_open` + `mmap`; Windows: `CreateFileMappingW`), wait and wake on a word in it (Linux: futex without the private flag; macOS: `__ulock_wait`/`__ulock_wake` with the shared flag; Windows: a named event). `src/fp.rs`: the float mode, moved from `core/runtime/src/fpenv.rs` |
| `m2-plugins` | new `src/remote/{mod,wire,slot,child}.rs`, `src/bin/m2-plugin-host.rs`; `host.rs` (`scan`, `HostEvent::Crashed`, `MAX_CALL_EVENTS`); `main_thread.rs` (the `--m2-plugin-host` hook; programs install `Remote`); `probe.rs` (tags); `tests/no_blocking.rs` reads `remote/slot.rs` too |
| `m2-runtime` | `packages.rs` (`describe_plugin` through the host), `fpenv.rs` (uses `m2_os::fp`), `lib.rs` (a node needing more than `MAX_CALL_EVENTS` is refused at build, naming it) |
| `m2-server` | `graph/actor.rs` (`Crashed` → `plugin.crashed`, in plain words) |
| apps | `m2/src/main.rs` (`plugins` through the host), `music2-desktop/src/plugin_host.rs` (install; quit ends the process), `tests/app_main_thread.rs` (now: the app's hook starts the host process) |
| docs | `architecture.md`: HOST-1's trait, HOST-2 (the app runs no plugin main loop), HOST-3 and HOST-5 Today, RENDER-1 Today, the `plugin.crashed` row, Deferred, Order of work 9 |

| Test (red first) | Where | Red by |
|---|---|---|
| `the_host_suite_passes_on_both_hosts`: create, activate, state load (skipped by hash), save, latency, `main_call`, reset, and the probe's output bits, on `InProcess` and on `Remote` | `core/plugins/tests/remote.rs` | `Remote` stubbed |
| `a_crash_silences_its_process_and_the_app_plays_on`: two probes, one `crash-at-50`; both silent, one `plugin.crashed`, both sound again after the next activation, from their committed state | `crates/server/src/graph/tests.rs` | old code: the test process dies |
| `a_hung_host_process_is_ended_and_said` (`hang-at-50`) | same | watchdog stubbed: times out |
| `renders_through_the_host_process_are_the_same_bits`: a probe song (notes, a lane, literals, the `sets-fz` tag) offline on both hosts; with-plugins too when `CLAP_PATH` has it | `core/play/tests/bit_identity.rs` | float-mode reset removed in the child |
| `the_app_never_loads_plugin_code` (Linux fixture: no `.clap` in `/proc/self/maps` after rendering with-plugins) | `core/plugins/tests/remote.rs` | old code maps Surge XT |
| the allocation and lock trap over two probes on `Remote`, through an activation that moves both | `core/play/tests/no_alloc.rs` | a `Vec::push` in the slot copy |
| `an_export_borrows_instances_in_the_host_process`, counted by `Owner::made` | `crates/server/src/graph/tests.rs` | export path forced to make new ones |

**Done when:** the tests above are green; renders 10 of 10; `m2 render examples/with-plugins` matches the base hash;
`m2-gates plugins` (G1 to G5, G7) and `m2-gates --server export` (G7e) pass; on Xvfb `:96` the app plays with-plugins,
and `kill -9` of the host process leaves the app open, says what happened in plain words, and the lead plays again
(pictures in `~/m2-runs/app-shots/ph-1/`); the call's round trip is measured and written down (information, PLAT-1);
`ci/check.sh` passes.

---

## 2. Slice 2: plugin windows, state autosave, output events (HOST-4, HOST-7, HOME-5, 5a, 6, MON-6)

**Goal.** Open and close a plugin's own window from its box in the app. A knob turned there reaches a literal Tx or
an override latch. A changed state becomes the song's state file and sidecar through one undoable Tx.

**How it works.**
- The main loop gets fds and timers: `Loop::watch(fd, ..)` and `Loop::timer(period, ..)` in `m2_os::main_loop`
  (Linux: a `poll` loop over the job queue's wake, the fds and a timer heap; macOS: CFRunLoop sources and timers;
  Windows designed: the message loop and `SetTimer`). The host process offers CLAP's `timer-support` and
  `posix-fd-support` from it, which Linux plugins need for their windows (HOST-3; the Deferred row closes).
- Windows: `m2_os::window`. Linux: X11 through libX11 opened at run time (`dlopen`), so a run without a screen never
  needs it; a top-level window, `WM_DELETE_WINDOW`, its connection fd watched. macOS: an `NSWindow` through the
  Objective-C runtime; the host process is an accessory app (no Dock icon) running `[NSApp run]`. Windows designed:
  an `HWND`.
- Behind `PluginHost` (HOST-7): `MainCall::Window(Open { title } | Close)`, and `HostEvent::WindowClosed(h)` when the
  person closes it. Inside: CLAP `gui` embedded (`is_api_supported`, `create`, `get_size`, `set_parent`, `show`;
  `hide`, `destroy`). A plugin with no window says so in words.
- App: `POST /api/graph/plugin/window {session, node, open}`. The graph state gives each plugin node
  `window: "open" | "closed" | "none"`. The plugin's box gets "Open window" / "Close window".
- Output events (HOST-4): `Events` also reads CLAP's gesture begin and end. Each plugin node puts what its call
  output on a fixed ring (no allocation); the actor reads it at its tick.

  | The parameter is | A gesture in the window |
  |---|---|
  | a literal in code | begins a lease on that literal; values update it; the gesture's end commits one Tx (HOME-5). Meanwhile the node stops sending the code value; a refused Tx sends it again (HOME-5a) |
  | driven by a lane or a cable | sets an override latch on (instance, CLAP id) (MON-6): the node stops sending the driven value; "Re-enable" in the box clears it. For this session only. Never in an export |
  | not set by code | changes the plugin's state: autosave |

  A change outside a gesture to a parameter code owns is set back at the next block, with `plugin.param-reverted`.
- Autosave (HOME-6): `HostEvent::Dirty`, or a gesture's end on a state parameter, starts a 1 s quiet timer per
  instance. Then `save_state`, and `m2_editor::data::plugin_state` writes blob and sidecar as one Tx (FMT-17), one
  Undo step. The load that follows is skipped by hash. An export first commits any pending autosave (RENDER-1).

| Crate | Files |
|---|---|
| `m2-os` | `main_loop.rs`, `linux/main_loop.rs`, `mac/main_loop.rs` (fds, timers); new `window.rs`, `linux/window.rs`, `mac/window.rs` |
| `m2-plugins` | `inprocess.rs` (host `timer-support`, `posix-fd-support`, `gui`), new `gui.rs`, `host.rs` (`MainCall::Window`, `WindowClosed`, gesture events), `remote/` (forwards them), `probe.rs` (a `gui` that records `set_parent` and gives a size; tags `gesture-at-<n>:<id>=<v>`, `preset-at-<n>`) |
| `m2-runtime` | `lib.rs` (`PluginNode`: out-event ring, hold during a gesture, latch flags, set back), `plugins.rs` |
| `m2-server` | `graph/actor.rs` (out events, autosave timers, `WindowClosed`), new `graph/plugin_window.rs` and `_tests.rs`, routes (window, latch re-enable), `export.rs` (commit pending autosave first) |
| web | `components/NodeGraph.tsx` (`NodeCard` buttons, the latch mark), `lib/graph.ts`; new `e2e/ph-window.mjs` |
| docs | HOST-4, HOST-7, HOME-5, 5a, 6, MON-6, RENDER-1 Today; Deferred rows; the step's gates row |

| Test (red first) | Where | Red by |
|---|---|---|
| `timers_and_fds_run_on_the_loop`: a timer fires at its period, a readable fd runs its callback, both on the loop's thread | `core/os` | not built |
| `a_plugin_window_opens_on_the_host_process_main_loop` (Xvfb `:96`; without a screen it says why): the probe's `set_parent` gets an X window that exists and is mapped, on the main thread; Close destroys it; the window manager's close sends `WindowClosed` | `core/plugins/tests/window.rs` | `gui` not offered |
| `a_gesture_on_a_literal_commits_one_tx` (probe `gesture-at` on `gain_1`): one undoable Tx; no code value sent during it | `crates/server/src/graph/plugin_window_tests.rs` | out events ignored |
| `a_gesture_on_a_lane_driven_parameter_latches_it`: latched value kept; Re-enable plays the lane again; an export never hears it | same | no latch |
| `a_dirty_plugin_autosaves_its_state_once`: a storm of Dirty, one Tx after 1 s quiet; undo loads the old blob; the save isn't loaded back | same | no autosave |
| `a_preset_change_outside_a_gesture_is_set_back` (`preset-at`): `plugin.param-reverted` | same | no set-back |
| `an_export_commits_a_pending_autosave_first` | same | export doesn't wait |
| `e2e/ph-window.mjs`: the lead's box opens and closes Surge XT's window through the route | browser suite | button missing |

**Done when:** the tests are green; on Xvfb `:96` Surge XT's window opens and closes from the lead's box; `xkey`
drags its filter cutoff knob (lane-driven: a latch shows) and a knob code doesn't set (one Undo step saves it)
(pictures in `~/m2-runs/app-shots/ph-2/`); renders 10 of 10 and with-plugins unchanged; G5 and G7e pass; the Mac lint
passes; `ci/check.sh` passes.

---

## 3. Slice 3: latency compensation (RENDER-17 to 21)

**Goal.** Plugins and kernels report latency. Parallel paths are delayed so tracks stay aligned, live and offline.
The probe proves it. Songs with no latency keep their bits.

**How it works.**
- Each instance has a latency: a plugin's measured at activation and after each load; a kernel's declared (`KernelSig`).
- New `core/runtime/src/latency.rs`: from the flat graph, each node's offset is the latest arrival over its inputs;
  each input is delayed by the difference. Lanes, notes and time are inputs like any other, so they arrive late by
  the same amount (RENDER-18); a plugin's transport is its node's offset earlier (HOST-6). The song's latency `L`
  is its output's arrival. Feedback edges are cut; a latent node on a loop raises `latency.in-feedback`
  (RENDER-20). All zero: no delay is made, so the plan is unchanged.
- Delays are built-in nodes the planner inserts: `Delay` for audio and control buffers (a fixed ring), `NoteDelay`
  for note events (sized from the lane's room). Kept across activations by (consumer, input, frames), as overlays
  are; a new length is a new node under the activation's crossfade (RENDER-25a). No allocation on the audio thread.
- Offline (RENDER-19): `L` is measured once after init and state loads. A render drops its first `L` frames and
  renders `L` more. A `HostEvent::Latency` during it gives `latency.changed` on the render (RENDER-17).
- Live: a new latency applies at a plan activation (a restart, or a state load, re-plans); the status and playhead
  report the position minus `L`.
- RENDER-21's refusals go in the same change: `core/ir/src/check.rs` (kernel latency), `describe_plugin`, and the
  `plugin.latency-uncompensated` notice (`plan.rs` `latent`, `actor.rs`).
- The probe learns `latency-<n>`: it reports `n` and delays its output by `n` frames.

| Crate | Files |
|---|---|
| `m2-runtime` | new `latency.rs` and `tests/latency.rs`; `plan.rs` (delays in the plan, kept by key), `lib.rs` (delay nodes, plugin transport offset, offline drop and extend), `plugins.rs` (latency), `engine.rs` and `transport.rs` (status minus `L`) |
| `m2-ir` | `check.rs` (refusal removed; latency in a `poly` voice still refused) |
| `m2-plugins` | `probe.rs` (`latency-<n>`) |
| `m2-server`, `m2-play` | `graph/actor.rs` (`latency.changed`, the uncompensated notice gone), the status's position |
| docs | RENDER-17 to 21 Today, the latency rows of the table, Deferred, Order of work 9, the gates row |

| Test (red first) | Where | Red by |
|---|---|---|
| offsets and delays for a chain, two parallel paths, a diamond, a loop (warns), all zero (none) | `core/runtime/src/tests/latency.rs` | not built |
| `a_latent_plugin_stays_aligned_offline`: a sine through `latency-100` against the same sine inverted sums to exactly 0; sample 0 is song time 0; the last `L` frames are kept | same | compensation stubbed |
| `a_latent_plugin_stays_aligned_live`, and the status reports the position minus `L` | `core/runtime/src/tests/live/keep/plugins.rs` | same stub |
| `lanes_notes_and_time_arrive_late_by_the_offset`: a probe after a latent one is told the transport `n` earlier and gets its notes `n` later | same | offset not passed |
| `a_latency_change_applies_at_the_next_activation`; offline, `latency.changed` | same, and `crates/server/src/graph/tests.rs` | change ignored |
| `no_fixture_gets_a_delay` | `core/play/tests/bit_identity.rs` | a 1-frame delay forced |

**Done when:** the tests are green; renders 10 of 10 and with-plugins unchanged; G5 and G7e pass; a song with
`latency-100` exports aligned (listened: the two paths cancel); the contract's latency rules say "matches";
`ci/check.sh --gates` passes at the track's end, and `HANDOFF.md` says "macOS: unverified since <commit>".

---

## 4. Decisions (defaults; the owner can change any later)

| # | Decision | Why |
|---|---|---|
| D1 | One host process per app process, all plugins in it | Simple. A crash silences every plugin in it; one process per plugin is a gap |
| D2 | The same program run again with a flag; `m2-plugin-host` only for tests | Nothing new to bundle in the Mac app; the two sides never differ in version |
| D3 | A call is synchronous: one slot per instance, a ring of one call | Same order and same bits as today; plugins already render on one thread |
| D4 | One audio thread per instance in the host process, asking for real time as engine threads do | Concurrent renders (an export beside playback) need no locking |
| D5 | A live call that waits 500 ms is a hang: the process is ended and it is `plugin.crashed` | RENDER-28's stall time; the words say "stopped answering" |
| D6 | Messages over std pipes, with a small codec of our own | No new crate; blobs go as bytes |
| D7 | `scan` joins the trait; the contract's trait text is updated | The app must never load plugin code |
| D8 | Tests that read the probe's records keep `InProcess` (`test_main_thread`); the rest and every program use `Remote` | Those tests check what runs inside the host process |
| D9 | Mac wake: `__ulock` with the shared flag | Works from macOS 11 (the app's minimum), as libc++ and Rust's std use it |
| D10 | Shared memory by name, removed once both sides have it | One shape on every OS (Windows: a named mapping) |
| D11 | X11 opened at run time; Mac windows through the Objective-C runtime | No new crate, no X11 needed to build or to run headless |
| D12 | Windows float on their own, not tied to the app's window | Tying them needs the app's window id: a gap |
| D13 | Latches last for the session; no "Write to code" yet | MON-6's persistence and that button are gaps |
| D14 | A fixed `MAX_CALL_EVENTS` (16,384) per slot; a node needing more is refused at build, by name | Shared memory can't grow on the audio thread |
| D15 | The float mode moves to `m2_os::fp` | The host process must use the same code (RENDER-2a) |
| D16 | The gates table gets a row "9": crash (G9a), same bits (G9b), alignment (G9c) | So the step has gates like the others |

## 5. Risks

| Risk | What we do |
|---|---|
| Surge XT renders other bits in the host process | Test it first in slice 1; float mode and thread set-up are the usual causes. Stop and look before going on |
| The base with-plugins render isn't the same twice | Said in step 0; fall back to G7e's 1 dB per second |
| Each call costs a round trip (an estimate: 10 to 40 µs) | Measured and written down; Linux is information only, the Mac judges (PLAT-1) |
| Surge XT's window doesn't draw on Xvfb | The probe's window test still proves the path; the picture is then owed |
| Mac code unrun (wake, shared memory, AppKit) | Lint only; "macOS: unverified since <commit>" until the owner's run |
| Slice 2 is the biggest | If it runs long, the latch's Re-enable button and the set-back go to `gaps.md`, not the core path |
| New `HostEvent` variants break exhaustive matches | Fix each; they are few (`actor.rs`, `gates/src/engine.rs`) |

## 6. To `docs/gaps.md` instead (one line each)

- One host process per plugin, for finer crash isolation.
- Host-process audio threads joining the Mac output's audio workgroup.
- Plugin windows tied to the app's window, their place remembered, resizing by hand.
- Wayland windows (X11, through XWayland, only).
- Latches kept in `.music2/` for keyed plugins; "Write to code" for a latch; latches on std knobs (MON-6).
- Recording plugin gestures as automation (HOST-4's third target).
- Autosave for a plugin with no key and no `key()` frame (no state path): it says it can't save.
- Latency inside a `poly` voice stays refused.
- Delayed notes in flight at a seek play out late, like a tail.
- Shared memory left behind when both processes die at once.
- `run_main`'s main loop in the app's process now serves nothing: fold it away.
