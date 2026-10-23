# 8a refresh: design-8a against the engine step 7 built

Written 2026-10-16. Read-only: nothing was built, run or changed. Checked against:
- branch `step7` at `ed7fc8e` (worktree `$SCRATCH/wt/s7`), its `docs/architecture.md`, `HANDOFF.md`,
  `docs/designs/step7/design-7.md` and `build-notes.md`, `docs/designs/step8/design-8a.md` and `understand-8a.md`,
  `docs/designs/platform/design.md` §8;
- wasmtime 49.0.2, cpal 0.18.2, clap-sys 0.5.0, mach2 0.6.0 and objc2-core-audio 0.3.2 in `~/.cargo/registry`;
- the speed workflow's own logs: `$SCRATCH/speed/prof/` (lockstep runs, callgrind, patches HV1, HV3, HV4) and
  `$SCRATCH/speed/sg/runs/` (G7f with S measured fresh, branch `s7-sg` at `5b77ae4`).

design-8a (revision 2) was written on 2026-10-14, before step 7 was built. "Estimate" marks a number nobody measured.

## In short

- **design-8a's core still holds.** Partitions by track. A small pool of our own. The same bits on any thread count.
  Real-time workers in the device's audio workgroup on the Mac. Step 7 built everything it stood on.
- **About a third of its details must change.** Most are small. The ones that matter:
  1. OS code moves into `m2-os` (PLAT-2): the Mach semaphore, the thread policy, the workgroup, the core count.
  2. `Player::close()` is not needed: step 7's fix K1 already stops a let-go player. Only the pool's owner is new:
     the stream's thread owns the pool, not the callback, because cpal's ALSA stream drops the callback on the audio
     thread.
  3. The overload listener is not needed: bug 1 already counts CoreAudio's overloads as xruns.
  4. The partitioner needs each node's inputs. Step 7 works them out to order the edit window, then throws them
     away (`keep.rs: window`). Keep them.
  5. Step 7 added a node design-8a never labelled: a new node's 5 ms fade-in. It goes with its consumer, like
     every fade.
  6. The pool's protocol goes in its own small crate, so its loom tests can run in every check cheaply.
- **Plugins can run on worker threads.** In CLAP the audio thread is a role, one thread at a time per plugin. The
  host already hands the role over at each call.
- **wasmtime Stores can run on worker threads.** A `Store` moves between threads, and a call needs `&mut`, so one
  thread at a time. Each new thread needs wasmtime's set-up before its first kernel call. Nothing calls it today,
  so even step 7's audio thread does it inside its first callback.
- **The numbers say 8a is needed.** On the owner's M1 Pro, one thread takes 2.2 ms for an average p50 block and
  3.1 to 3.2 ms at p99 (measured before the owner's layout, which adds about 1% on Linux). A block at 128 frames
  lasts 2.67 ms, and gate 8 allows 1.87 ms at p99. So 8a must be at least 1.7 times faster at p99, and about 2.5
  to 3 times to have room. The speed workflow's patches save 3 to 4%: welcome, but not the lever.
- **The biggest risk is still scaling.** On this server the same work costs 10 to 40% more on a core with a smaller
  or shared cache. The work follows memory, so 4 threads on one cache give less than 4 times. The throw-away spike
  comes first.
- **Slices:** 0 spike, 1 the pool, 2 the executor offline, 3 live on engine threads, 4 the gate, 5 only for a
  measured failure. About 5 weeks, after the speed branch, `sounds` and `taste` land.

---

## 1. The table: each part of design-8a against step 7 as built

Verdicts: **holds** (build as written), **change** (build it, but differently), **obsolete** (step 7 did it, or it
no longer applies).

| # | design-8a part | Verdict | What changes | Evidence (step 7 as built) |
|---|---|---|---|---|
| 1 | §1: one `Store` per kernel instance; `Graph` functions take no store | holds | If the speed branch's HV3 lands, a voice pool's kernels share one `Store`. Still one `Store` per box, still one thread at a time | `core/runtime/src/lib.rs`: `KernelInst` (its own `Store<StoreLimits>`), `KernelInst::new`, `Graph::process`; `$SCRATCH/speed/prof/patches/hv3.diff` |
| 2 | §1: the schedule is built with the plan, off the audio thread, and retired with it | holds | Build it in `Renderer::build`, which both `Renderer::new` and `prepare` call | `lib.rs`: `Renderer::build`; `plan.rs`: `Plan` |
| 3 | §1: lanes are a serial pre-pass; note ids are engine-wide | holds | — | `lib.rs`: `Renderer::render_block` (schedulers in slot order with `next_note`, then `AudioPlayer::fill`) |
| 4 | §1: no two-renderer crossfade, no `lend` | holds in the engine, **change** around it | The engine plays one plan. But the player still crossfades two whole engines for 5 ms on `Replace` and `Unload` (a project opened, a rate change): two engine runs in one callback, one after the other. "Lend" now means an export borrowing plugin instances while the engine is held | `engine.rs`: `Engine::render`; `core/play/src/lib.rs`: `Callback::render`, `LiveFade`, `fade_out`; `plan.rs`: `Basis::Lend` |
| 5 | §1, §3.2: ghosts and fades are plain nodes, partitioned like the rest | **change** | `keep.rs: window` builds every node's inputs (`deps`) for Kahn's order, then drops them. `Wiring` must keep them by exec node, so the partitioner reads the same edges the order used and can't drift from it | `keep.rs`: `wire`, `window` (local `deps`); `lib.rs`: `Renderer::build` pushes ghosts, then fades |
| 6 | §3.3: labels (plan node, ghost, fade, the output's fade, overlay) | **change**: add a row | Step 7 added a new node's input fade-in (from silence over 5 ms, O5, RENDER-25a): an `Exec::Fade` with `old = 0`. Its consumer is the new node: label it with that node's module | `keep.rs: window` (`ins`, `waits`); `lib.rs: fade` |
| 7 | §3.4: one schedule per plan; delete `order_after` and the order swap | holds | `end_fade` puts `Exec::Const(0.0)` in every ghost's slot, and a fade reads only its new side after the window, so the window's schedule stays right for the plan's life | `plan.rs`: `Renderer::end_fade` (`mem::swap` of `order` and `order_after`), `Plan.order_after`; `lib.rs: fade` |
| 8 | §2.1: the conditions for the same bits | holds | Step 7 adds readers and writers, all inside one node: a fade writes its own version; a plugin node reads its lane's scheduler (`sounding_note`, read-only) and its instance's atomic flags; `poisoned[i]` is written by node i's own task. Nothing crosses nodes during the pass | `lib.rs`: `fade`, `PluginNode::process`, `Graph::process`; `core/runtime/src/plugins.rs`: `PluginInstance` |
| 9 | §2.2: every sum inside one node; the poly's zero fill | holds | Unchanged: `out.fill(0.0)`, then `+=` in voice order. The hold's fades and `LiveFade`'s mix work on the output after the graph, on one thread | `lib.rs: PolyNode::process`; `engine.rs: Engine::render`; `core/play: Callback::render` |
| 10 | §2.3: RENDER-2a, one floating-point mode | holds, not built | Nothing in the tree sets or checks `FPCR` or `MXCSR`. `fpenv` stays in `m2-runtime`: it names a CPU, not an OS, which PLAT-2 allows (platform design §8 says the same) | grep finds no `fpcr` or `mxcsr` in `core/` |
| 11 | §4.1: one `step()`; every read of another node through a `Ports` view | holds, wider | More functions read other nodes' buffers than design-8a listed: `KernelNode::process` (buffers and versions), `fade` (new in step 7), `PolyNode::process`, `PluginNode::process`, `plugin_events`, and `Missing` and `Capture` inline. All take the view | `lib.rs`: those functions; `Graph::process` still `mem::take`s each output |
| 12 | §4.2: `GraphView` with two unsafe functions | holds | `own(i)` also hands out `poisoned[i]` | `lib.rs`: `Graph` (`exec`, `bufs`, `versions`, `poisoned`) |
| 13 | §4.3: the error at the lowest plan position wins; a panic outside a task aborts | holds | Live, a plugin's error no longer breaks the plan: that node goes silent alone, through an atomic flag | `lib.rs: PluginNode::process` (`inst.kill()`, plug-2); `core/play: Callback::fill_in` (`catch_unwind`) |
| 14 | §5: the pool (wake first, the leave rule, `patience`, `stuck`, `inside`, `again`) | holds; **change** where it lives | `Sem` (a Mach semaphore) and `mach2` inside `m2-runtime` break PLAT-2. Use std's `Thread::park` and `unpark` (portable; signalling takes no lock), or a `m2_os::rt::Wake` only if the Mac measures std's as slower. Put the protocol in a crate of its own (`core/pool`, no wasmtime), so the loom tests rebuild little | architecture.md PLAT-2; `core/os/tests/one_place.rs` |
| 15 | §5.1: each worker starts with a 2 MiB stack, `tls_eager_initialize`, `canon()` | holds; **add** the device thread | wasmtime 49 has `Engine::tls_eager_initialize`. On Linux it maps a 256 KiB signal stack; on the Mac it sets the thread's exception ports. Nothing in the tree calls it, so the device thread does this inside its first callback today. Call it at the top of every fill too: after the first, it is one thread-local read | wasmtime-49.0.2 `src/engine.rs` (`tls_eager_initialize`), `vm/traphandlers.rs` (`tls::raw::initialize`), `vm/sys/unix/signals.rs` and `machports.rs` (`lazy_per_thread_init`) |
| 16 | §5.3: moves and retirements only between runs | holds | `take_over`, `end_fade`, `SwapLane`, a hold and a resume all apply at the block line, before `render` | `engine.rs`: `Engine::block`, `try_apply`, `resume`; `plan.rs: take_over` |
| 17 | §6.1: player options, `BufferSize::Fixed`, `driven_on` | holds | `Player::start` still opens the device's default config | `core/play: Player::start` |
| 18 | §6.1: "`Callback` owns its `Pool`" | **change** | cpal's ALSA stream drops the callback on the audio thread (said in `impl Drop for Callback`). A pool dropped there would join threads on the audio thread. The thread that owns the stream owns the pool: `serve` on `m2-play`, the silent player's thread, the `Driver` in tests. It drops the pool after the stream. The callback only borrows it | `core/play`: `impl Drop for Callback`, `serve`, `Player::silent`, `Driver` |
| 19 | §6.3: the Mac set-up (device id, workgroup, period, thread count, time-constraint, join) | holds; **moves** to `m2-os` | It becomes `core/os/src/mac/rt.rs` (platform design §8). The five `extern "C"` lines live there. cpal 0.18.2 has `DeviceTrait::id()`; objc2-core-audio has `kAudioDevicePropertyIOThreadOSWorkgroup` | platform `design.md` §8; cpal `src/traits.rs` (`fn id`); objc2-core-audio `AudioHardware.rs` |
| 20 | §6.3 step 5: an overload listener | **obsolete** | Bug 1 sorts cpal's errors: CoreAudio's overloads arrive as `ErrorKind::Xrun` and are counted. G12 reads `Player::device_errors().xruns` | `core/play`: `Shared::device_error`, `DeviceEvent::of` |
| 21 | §6.4: the silent player on one thread | holds | — | `core/play: Player::silent` |
| 22 | §6.5: `Player::close()` | **obsolete** | K1 built it another way. Every `Player` clone holds a token. When the last goes, `serve` waits for the fade out, then drops the stream, and the callback's drop sends its engine to the dropper. 8a only adds: drop the pool after the stream | `core/play`: `Player.alive`, `serve`, `impl Drop for Callback`; test `a_player_let_go_stops_its_stream_after_its_fade_out` |
| 23 | §7: offline renders on a pool; default thread counts | holds; one refinement | An export of a song with plugins holds live playback, so the live pool sleeps: that export may use the P-cores. Only a pure song's export and range renders run beside playback (utility QoS, E-cores) | `engine.rs: Msg::Hold`; `crates/editor/src/render.rs` (`Renderer::lending`); `crates/server/src/export.rs` (`Lent`) |
| 24 | §8: voices across threads, not built | holds | If HV3 lands, a pool's voices share one `Store`, so a later split needs a `Store` per chunk of voices. Write that into the trigger | `hv3.diff` |
| 25 | §9.1: the load record | holds | Its "overloads" are bug 1's xrun counts | `core/play: Shared.device_errors` |
| 26 | §9.2: G12, `m2-gates realtime` | holds | The M1 Pro's verdict is "provisional (`--like-m1`)" until a base M1 exists (the owner's decision, already in the design). The gate opens 128 frames itself; the app keeps the device's default (the owner's decision). Add: on Linux, G12 runs on the silent player for information, so the harness is tested before the Mac run | `HANDOFF.md`, "Decided earlier" |
| 27 | §9.4: `machine()` gains the topology | **change** | A new part in `machine()` stops old records matching: `same_machine` compares every part but the OS. Print the topology on a line of its own | build-notes, "m2-os process: Left for later"; `crates/gates/src/report.rs: same_machine` |
| 28 | §10: the tests | mostly hold | `no_alloc` on workers: mark workers in debug builds only, as K5 did for the callback, and count through `alloc`'s global `ALL`. `a_closed_player_joins_its_workers` becomes "a player let go joins its workers". Add step 7's cases (§4.4) | `core/play/src/alloc.rs` (`ALL`, `set_in_callback`) |
| 29 | §11: what is deleted | partly obsolete | `order_after` still goes. `Player::close()` was done by K1 | as above |
| 30 | §12: contract edits | hold; **rebase** the words | RENDER-28 and HOST-2 grew since. RENDER-32 now states the owner's layout and no longer has a "per partition" fallback | `architecture.md`: RENDER-28, RENDER-32, HOST-2 |
| 31 | §13: risks | hold; **add** four | The device thread's first kernel call; a trap on a Mac worker waits for wasmtime's helper thread; the pool's owner under ALSA; G7f must stay on one thread (§6) | below |
| 32 | §14, measurement 1: the baseline | **done** | M1 Pro, p50 block on one thread: mean 2,195 to 2,227 µs, p99 3,083 to 3,197 µs | architecture.md RENDER-32 *Today* |
| 33 | §16: coordination ("the other workflow is editing `crates/gates`…") | **obsolete** | Now: start after the speed branch, `sounds` and `taste` have landed (HANDOFF's order) | `HANDOFF.md`, "What to do next" |

---

## 2. design-8a's plan, in brief

| Part | What it says |
|---|---|
| The goal | Gate 8: p50 plays with no dropout at 128 frames and 48 kHz on the reference Mac, with the callback at most 70% of its period at p99 over 10 minutes (1,867 µs); a 64-frame run is recorded. And the CI gate: each pure fixture renders the same bits on 1 thread and on N (RENDER-2). |
| The parallel scheme | A plan's top-level nodes are split into **partitions**, one per module (a track, or the song). Where modules read each other both ways, their nodes are split further by depth, so partitions never form a cycle. Each partition is one task per block, run in plan order on one thread. A partition starts when every partition it reads has finished. A `poly`'s voices stay in its task. p50 has 51 partitions: 50 tracks and the song's mixer, a serial tail of about 100 kernel calls. |
| The pool | Our own, not rayon. N−1 workers plus the device thread, which works too. At each block line: wake the workers first, run the serial pre-pass, publish the job, everyone takes ready partitions from a one-shot queue with dependency counters, then the serial post-pass. Waits are bounded: a worker sleeps when nothing is left it could take (`patience`); the device thread parks only if nothing finishes for a whole period (`stuck`), so a blocked plugin costs a dropout and no CPU. `inside` plus Dekker's pattern make sure no worker touches a plan after the run. A panic in pool code aborts. |
| Determinism | Any order that runs each node after its inputs gives the same bits, if: every edge is a happens-before edge; one writer per node; frozen inputs stay frozen; the feedback rings and the output are written after the join; every sum stays inside one node; every thread uses one floating-point mode (new rule RENDER-2a, `fpenv::canon()` at each callback, block, task and plugin call); nothing reads the time or the thread. The reported error is the one at the lowest plan position. A 1-thread render goes through the same code. |
| The workers' real-time set-up (Mac) | Time-constraint threads at the device's period, joined to the output device's audio workgroup. Count: the workgroup's recommendation, at most the P-cores that share one L2 (4 on the M1 and the M1 Pro). Offline renders use plain threads; beside playback, utility QoS on the E-cores. |
| What runs where | Device thread: messages, cells, schedulers, players, timing, its share of tasks, rings, output. Workers: tasks. Offline: the render's own pool. |
| How it is measured | The player always keeps a histogram of callback times, late callbacks, gaps and dropouts. G12 (`m2-gates realtime`) is judged on a base M1: the engine alone for 10 minutes; the app's own path while p50 is edited every 10 s; beside an export; the IO thread's bits against the export. 64 and 512 frames, a thread sweep and a song with plugins are recorded. `--like-m1` loads a bigger Mac like an M1, and its verdict is "provisional". |
| Slices | 0: a spike, thrown away. 1: the executor offline, with the 1-vs-N tests. 2: live on real-time workers. 3: the gate. 4: only if the gate fails, each item on its own measurement. About 4 weeks. |
| Not built | Voices across threads, parallelism inside a node, a paced mode with no device, learned costs, a thread count per block, abandoning a late block, home threads for plugins, `wfe` spinning, Panic through the pool. |

---

## 3. Step 7's engine as built, and how each part fits a parallel schedule

### 3.1 One block as built, and where it runs in 8a

| Step | Today (file: function) | In 8a |
|---|---|---|
| The callback starts; a held `Replace` or `Unload` applies | `core/play`: `Callback::fill_in`, `take_held` | device thread; add `thread_ready()` (wasmtime's set-up, once) and `canon()` |
| Each chunk of up to 8,192 frames, under `catch_unwind` | `Callback::fill_in`, `Callback::render` | device thread |
| Per frame, at each block line: stash retries, `end_fade` once the window is over, waiting messages, then the inbox | `engine.rs: Engine::process`, `Engine::block`, `try_apply` | device thread; the pool is idle here by construction |
| Held: silence, the clock stands still. Idle: a `woken` check | `engine.rs: Engine::render` | device thread; no job, no wake |
| Transport, cells, a broken plan's silence | `lib.rs: Renderer::render_block` | device thread, pre-pass (the wake goes out first) |
| Schedulers in slot order, audio players fill, the trace | `Renderer::render_block`, `Renderer::record` | device thread, pre-pass |
| Block timing and plugin run info | `Renderer::block` | device thread, pre-pass |
| The graph | `Graph::process` (one loop over `order`) | **partitions on every engine thread** |
| Feedback rings | the second loop in `Graph::process` | device thread, post-pass |
| Output copy, non-finite check | `Renderer::render_block` | device thread, post-pass |
| The hold's fades, the idle check (`can_idle` calls each top-level kernel's `active`) | `Engine::render`, `Renderer::can_idle` | device thread, post-pass |
| The old engine's 5 ms fade out (`LiveFade`), the resampler, publishing | `Callback::render`, `Live::fill`, `Callback::publish` | device thread; each engine's blocks are their own runs, with `again` between them |

### 3.2 Kept instances: the arena and `Keep`

- **What moves:** four kinds of box: a kernel instance (its own `Store`), a whole voice pool, a plugin instance, a
  feedback ring (`plan.rs: take_from`). Lanes move the same way (`Renderer::take_over`, its lane loop).
- **When:** at a block line, in `Engine::try_apply` → `Renderer::take_over`, after `verify` checks every shape hash.
  Every move is a swap. The pool is idle then, so no worker can see a half-moved plan.
- **Across threads:** a box may run on worker A in block k and on worker B in block k+1, or land in another
  partition in the next plan. The chain "A's completion, the join, the move, the next publish, B's pop" orders it.
  Nothing is pinned to a thread.
- **Resets at entry:** a poisoned box is reset in `take_over` (`reset_node`, a `k_reset` on the device thread). Rare,
  serial, unchanged. If the speed branch's HV4 lands, the non-finite scan also moves here (`take_from` reads the old
  output), out of the hot loop: that suits 8a, since it runs while the pool is idle.
- **Prepare** (`Renderer::build` through `keep::wire`) runs on the actor's thread. The schedule is built there too.

### 3.3 Ghosts, fades and the 5 ms fade-ins

| Node | Built in | Label in 8a | Why it is safe |
|---|---|---|---|
| A ghost (a replaced node playing out the window) | `keep.rs: window` clones the old `FNode`, so it keeps `module` | its old module; a ghost of a removed track gets its own partition | it reads only nodes the window wired it to; after the window it is `Exec::Const(0.0)` and does nothing |
| A fade on a changed edge (crossfade over 5 ms, after a 5 ms wait when anything fades in) | `window` (`changed`, `waits`); `Renderer::build` pushes `Exec::Fade` | the consumer's module (from `Wiring.rewire`) | it reads both sides, written this block by its inputs; it writes only its own buffer and version; `b.window` is frozen for the block |
| The output's fade | `window` (`Edge::Out`, `w.out`) | the module of the new output's node | as above |
| A new node's input fade-in (from silence over 5 ms, O5) | `window` (`ins`) | the new node's module, its consumer | as above; `old` is the constant 0 |

- The window lasts `fade + wait` frames (`plan.rs: window_over`). Ghosts and fades are partitioned the same in it
  and after it: one schedule.
- A cycle the window alone makes (a reorder of two kept effects) is broken by `window` itself, by replacing the nodes
  on it. A cycle between modules (track A reads B, B reads A) is 8a's depth split, unchanged.
- An activation's block runs the ghosts and fades in parallel too. Measured on Linux on one thread, an activation's
  block costs about what an ordinary block does (p50 1.8 to 2.3 ms; the slowest 3.2 to 4.2 ms;
  `$SCRATCH/speed/sg/runs/*.log`), so it should scale like one.

### 3.4 Holds and exports

- **A hold runs nothing.** `Engine::render` returns before `render_block` when held, so there is no job and no wake.
  The 5 ms fades out and in act on the output (post-pass).
- **`held` stays true to its word.** It is published after the callback's work (`Callback::publish`). With a pool,
  that work ends with the join (`inside == 0`), so once `held` is true no worker can touch a plugin instance. The
  export's host calls (deactivate, offline mode, activate, on the main thread) stay safe. G7e's probe test must run on
  a 4-thread player to prove it.
- **The resume** (`Engine::resume` → `Renderer::resumed`) marks the borrowed plugin nodes to start their held notes
  again (O4). It runs at a block line, while the pool is idle.
- **A panic while held** waits for the resume (`panic_held`). Serial, unchanged.
- **The export** renders with `Renderer::lending` on the export's thread. In 8a it gets its own offline pool. Its
  borrowed plugin instances then run on that pool's threads, in offline mode, one call at a time: CLAP allows it.
  While the live engine is held its pool sleeps, so the export may use the P-cores. A pure song's export doesn't
  hold: it runs beside playback, so it takes utility QoS on the E-cores (design-8a §7).

### 3.5 Plugin nodes, and CLAP's rule for the audio thread

**Can a plugin's `process` run on a worker? Yes.**

| Question | Answer | Evidence |
|---|---|---|
| What CLAP allows | The audio thread is a role, not one OS thread. A host may use a thread pool, so `process` may run on different threads over time, but never on two at once for one instance. (From CLAP's `thread-check.h`, as remembered: the header isn't on this machine; clap-sys has only the bindings.) | clap-sys 0.5.0 `ext/thread_check.rs` |
| Does the host tell the truth? | Yes. Each `process` and `reset` stores the calling thread's token as the instance's audio owner, and `is_audio_thread` compares with it. So it answers right on any worker. `start_processing` happens on the first call after `activate`, on whichever thread makes it | `core/plugins/src/audio.rs`: `process`, `reset`, `is_audio_thread` |
| Can a worker reach the instance without a lock? | Yes: two atomic loads (`core`), and a grep test keeps the audio side free of locks, allocation and thread-locals | `core/plugins/src/inprocess.rs: core`; `core/plugins/tests/no_blocking.rs` |
| One call at a time? | Yes: a plugin node is one node in one partition, run by one task | §3.1 of design-8a |
| What runs on the main thread meanwhile? | `load_state` into a kept, playing instance during prepare (RENDER-26b, `Renderer::build`'s loads), which CLAP allows. A plugin that locks inside `process` while it loads blocks its worker: design-8a's bounded waits make that a dropout and no spinning, as today. `deactivate` and render-mode changes happen only while no renderer calls the instance | `lib.rs: Renderer::build`; `core/plugins/src/host.rs: PluginHost` |
| A plugin that errors | Its node goes silent alone, through an atomic flag; the song plays on | `lib.rs: PluginNode::process` (`inst.kill()`) |
| What can still go wrong | A plugin's own thread-locals are first touched on a new worker, through the system malloc the trap can't see; a non-conforming plugin may assume one audio thread. Both are measured, not designed away (design-8a R12): the probe records first touches, and `with-plugins` runs 10 minutes on 4 threads | design-8a §10, §13 |

### 3.6 wasmtime Stores on worker threads, under the owner's layout (O1)

| Question | Answer | Evidence |
|---|---|---|
| One `Store` per what? | Per kernel instance, voice kernels included. Each memory reserves 32 MiB with a 64 KiB guard, never grows or moves; the Spectre guard is off. If HV3 lands: one `Store` per voice pool for voice kernels. Either way it is one per box, and a box lives in one partition | `lib.rs: KernelInst::new`; `packages.rs: engine_with` (`Layout::P2`); `hv3.diff` |
| `Send` and `Sync` | `Store<StoreLimits>` is `Send`; `Memory` and `TypedFunc` are handles. So `KernelInst` is `Send` with no unsafe code, and the renderer already moves from the actor's thread to the audio thread. A call needs `&mut Store`, so one thread at a time; `GraphView::own` gives that. HV1 adds a raw pointer and `unsafe impl Send`: still sound, since the pointer moves with its store. Never add `Sync` | `lib.rs: KernelInst`; `hv1.diff` |
| Thread-local state | wasmtime keeps the current call in a thread-local. Its first touch on a Mac thread may allocate through the system, out of the trap's sight. Linux: a 256 KiB signal stack is mapped per thread, with a destructor. Mac: the thread's exception ports are set. All of it happens at the first kernel call, or earlier at `tls_eager_initialize()` | wasmtime-49.0.2 `vm/traphandlers.rs`, `vm/sys/unix/signals.rs`, `vm/sys/unix/machports.rs` |
| The trap handler on Linux | Signal handlers for the whole process, installed when the `Engine` is made. Each thread runs the handler on its own signal stack. Works on any thread after its set-up | `signals.rs` |
| The trap handler on the Mac | Mach exception ports: one port for the process and one wasmtime helper thread. The faulting thread waits until the helper answers. On a real-time worker, a trap therefore waits for a normal-priority thread: a dropout, but only when a kernel crashes, and then the plan goes silent anyway | `machports.rs` (its header, `lazy_per_thread_init`) |
| Locks on the call path | None found. wasmtime is built without async, GC or the component model (`default-features = false`); its code registry is read only on a trap | `Cargo.toml` (wasmtime features); understand-8a §7 |
| Stack | wasm runs on the caller's stack, up to 512 KiB by default. Spawn workers with 2 MiB, Rust's default | understand-8a §4.4 |
| Memory growth | None in steady state: no room to grow, never moves, and `prewarm` resets every kernel in prepare | `packages.rs: engine_with`; `lib.rs: Graph::prewarm` |

### 3.7 Determinism: sums and who owns each buffer

- **One writer per buffer.** A node writes only its own exec, buffers, version and `poisoned` flag, inside its own
  task. Cells write constants in the pre-pass. Rings are written in the post-pass.
- **Every sum inside one node.** A bus is std's chain of `add` kernels, so the graph fixes the order. A poly sums its
  voices in voice order into a zeroed buffer. A fade mixes two inputs in one node. Overlay gains (8b) are nodes
  labelled with their consumer. Never two tasks into one buffer, never an atomic float add.
- **Frozen inputs.** `Block` (with `window`), lane events, control lanes, audio players' blocks, constant buffers and
  their versions are written before the job is published. m2-runtime has no interior mutability outside atomics (no
  `Cell`, `RefCell` or `Rc` in `core/runtime/src`), so the lanes can be read from many threads.
- **One floating-point mode** (RENDER-2a): not built yet; the pre-pass does float work too (players, beat maths), so
  `canon()` goes at the top of each fill as well as each task and after each plugin call.
- **The guard:** with one thread the executor runs partitions in queue order, not plan order. The 8 recorded renders
  (`e2e/renders.sha256`) and `renders.json` then check that the order change moved no bit.

### 3.8 The player: the callback, `Driver`, `Shared`, the dropper

| Part | As built | In 8a |
|---|---|---|
| `Callback` | owns the `Live` (engine and resampler), the old `Live` fading out, the channels and the scratch buffers; `fill` marks the thread for the debug allocation counter | borrows the pool; `thread_ready()` and `canon()` at the top; the load record around it |
| `Driver` | tests call `fill` on their own thread | `Player::driven_on(rate, threads)`: the `Driver` owns a pool |
| `Shared` | the published atomics (a seqlock for the position), bug 1's error counts, `held`, `quiet` | gains the load record (histogram, late callbacks, gaps); xruns already there |
| The dropper `m2-play-drop` | polls every 5 ms and drops what the callback retires | unchanged; its `munmap`s now race N real-time threads (risk R15) |
| `serve` on `m2-play` | holds the stream; logs device errors once a second; when let go, waits for quiet, then drops the stream | also owns the pool, and drops it after the stream (joining the workers off the audio thread) |
| The silent player's thread | fills on a clock until let go | owns a 1-thread pool unless `M2_THREADS` says more |

### 3.9 What the speed workflow's numbers mean for 8a

| Measure (p50 from 3:00, one thread) | M1 Pro (the Mac's records) | Ryzen 7950X3D (Linux, information only) | Source |
|---|---|---|---|
| Block, mean | 2,195 µs (S), 2,227 µs (P1) | 1,795 to 2,399 µs | RENDER-32 *Today*; `speed/sg/runs/*.log` |
| Block, p99 | 3,083 µs (S), 3,197 µs (P1) | 3,447 to 4,651 µs | same |
| 512-frame callback, mean | 8,936 µs (S), 9,141 µs (P1): 84% of 10.7 ms | step 7: 7,150 to 7,470 µs pinned to one quiet core; 7,246 to 9,557 µs in G7f | RENDER-32; `speed/prof/logs/ls-C1.log`, `ls-D.log` |
| 512-frame callback, p99 | 12,910 to 13,817 µs: over the period | 13 to 18.6 ms | same |
| The slowest activation block | 3.0 to 3.8 ms (P1) | 3.2 to 4.2 ms (one run under load: 7.9 ms) | RENDER-32; `speed/sg/runs` |
| The speed patches against step 7 | — | HV1 with HV3, and with HV4: 240 to 270 µs less a 512-frame callback (60 to 70 µs a block, 3 to 4%). That brings step 7 back to the cost before it (S), or just under | `ls-C2.log`, `ls-D.log` (step 7 +157 to +204 µs over S; the patched trees −38 to −114 µs) |
| The same callback on a smaller or shared cache | — | 7.0 ms on CPU 4 (96 MB L3), quiet; 7.6 to 9.9 ms on CPU 12 (32 MB L3, a higher clock) with other work beside it (load 6 to 10): +10 to 40% | `ls-C1.log`, `ls-B.log`, `ls-C1-ccd1-contended.log`; sysfs L3 sizes |
| Where the work goes | — | wasm kernels: about half the instructions and most last-level cache misses (their own state). `Graph::process`: about 5% of the instructions but most L1 misses (the copies in and out of 6,000 memories). The owner's layout adds about 20% kernel instructions but under 2% time | `speed/prof/vg/m1-diff.txt`; G7f "against P1" lines |

The Mac's numbers are for S (before step 7) and P1 (step 7's first layout). The owner's layout (O1) hasn't run on
the Mac yet. On Linux it costs 10 to 33 µs a block more than P1, so the Mac's figures should hold within a few
percent.

What this means:
1. **One thread can't pass gate 8 anywhere.** The Mac's p99 block (3.1 to 3.2 ms) is longer than the 2.67 ms
   period. 8a is the only lever.
2. **How much 8a must give.** At least 1.7 times at p99 on the M1 Pro (3,197 → 1,867 µs), more on a base M1 with
   half the memory bandwidth. With 4 threads (*estimates*, serial part 100 to 180 µs): at 3 times, p99 about 1.2 ms
   (45%); at 2.5 times, about 1.4 ms (54%); at 2 times, about 1.75 ms (66%), just inside. Two threads won't do.
3. **The work follows memory, not instructions.** Bounds checks that add 20% instructions cost under 2% time, while
   a smaller or shared cache costs 10 to 40%. So 4 threads on one L2 will scale less than 4 times. This is
   design-8a's R1, now with evidence: the spike must measure before anything is built, and the one-L2 cap is right.
4. **The tail.** p99 is 1.4 (Mac) to 1.9 (Linux) times the mean. If spikes come from many tracks at once (a chord
   change), they spread over threads; if from page faults or other programs, they don't. The per-partition profile
   must show which.
5. **The speed patches** are small and fit 8a: each stays inside one node's work. HV4 takes a scan out of the hot
   loop. HV3 ties a pool's voices to one `Store`, which only matters for voice splitting (not built). Land them first:
   8a's `step()` and `Ports` change the same functions.
6. **64 frames** (recorded only): the rendering callback has 1,333 µs. At 1.2 to 1.4 ms (*estimate*) it would be at
   90 to 105%. Expect that run to look poor; it is not judged.

---

## 4. Refreshed slices

Each slice passes `ci/check.sh` on Linux, every row, with no retry. Order: after step 7 is on `main`, the speed
branch, the `sounds` merge and the `taste` merge (HANDOFF's order). About 5 weeks (*estimate*).

| Slice | What lands | Stays green because | Checks and gates |
|---|---|---|---|
| **0. Spike** (2 days, a branch, thrown away) | per-module partitions straight from `Flat.order`, `std::thread::scope` per block; offline p50 from 3:00 for 30 s on 1, 2, 3, 4 and 8 threads, pinned to one CCD and across both | nothing is kept | numbers only: scaling, per-partition costs, the song's tail. Rule: under 2.5 times at 4 threads on one CCD, R1's fixes come first |
| **1. The pool** (3 to 4 days) | a new crate `core/pool` (`m2-pool`): the pool, the one-shot queue, the run state, `inside`, `again`, the leave rule, `patience` and `stuck`, the abort guard, the counters, std `park`/`unpark`, a `sync` shim, the loom models. It knows only a graph of numbered tasks (each task's consumers, its count of inputs, the roots by priority, the width) and runs it; `m2-runtime` decides what the tasks are | nothing uses it yet | `ci/check.sh`; a new row, loom (`RUSTFLAGS="--cfg loom" cargo test -p m2-pool --release`, with its own target folder): each task once after its inputs; nothing touched after `run` returns; two runs back to back; no lost wake and no second signal. Pool unit tests: a panicking task completes the block; a panic in pool code aborts (child process); one signal per sleeping worker |
| **2. The executor, offline** (2 weeks) | `m2-runtime`: `fpenv.rs` (`FPCR`, `MXCSR`), `parts.rs` (labels, depth split, bottom levels, width) from `Wiring`'s kept inputs, `view.rs` (`GraphView`, `Ports`, debug owner checks), `step()` split out of `Graph::process`, `Renderer::*_on(pool)`, `Engine::process_on`, `thread_ready()`; `order_after` and the swap deleted. `m2-os`: `rt::cores()` and `rt::background_thread()`. `Render::threads`, `m2 render --threads`, `M2_THREADS`, the default counts. The offline half of G12 (`m2-gates realtime --offline`, `--sweep`, `--sends`, `Renderer::profile_parts`). Live still runs on one thread | one thread runs an order that respects every input, so the 8 renders keep their bits, and every test runs through the same executor | `ci/check.sh` (renders 8 of 8, bit identity, `no_alloc` unchanged, clippy, the Mac lint, `one_place.rs`); the new determinism tests (§4.4); `ci/check.sh --gates` on Linux, every gate as before (they all still play on one thread); the offline sweep recorded on Linux. Contract: RENDER-2, RENDER-2a, RENDER-32's *Why* |
| **3. Live on engine threads** (1.5 weeks) | `m2-os` `rt`: `Group`, `on_device_thread`, `engine_thread`, `end_engine_thread`, each refusal reported. `m2-play`: `Options` (`frames`, `threads`, `silence`, `capture`), `Player::start_with`, `BufferSize::Fixed`, `driven_on`; the pool owned by the stream's thread; `thread_ready()` and `canon()` at each fill; the wake at the block line, `again`; `load.rs` and `Player::load()`; workers marked for the debug allocation counter; `--threads N` for the existing gates (the engine rig's `driven_on`, the server rig's `M2_THREADS`) | the bits are the same by slice 2; the silent player keeps one thread; `Player::start()` reads the same environment | `ci/check.sh`; `no_alloc.rs` on a 3-worker pool, a fresh worker's first block included; the probes (sets FZ, blocks 100 ms, stuck); `plugins_on_workers_are_told_the_truth`; a player let go joins its workers; G7e's probe test on 4 threads; `ci/check.sh --gates` on Linux, then G7a, G7b and G7e again with `--threads 4`: G7a's 200 ms beside the workers, G7b's untouched tracks bit for bit, G7e's hold with workers; G7f on one thread; G11. Contract: RENDER-28, RENDER-28a, HOST-2 and the `PluginHost` sketch |
| **4. The gate** (1 week, then the Mac run) | G12's live runs: `realtime`, `--server`, `--export`, `--capture`, `--frames 64` and `512`, `--sweep`, `--song`, `--like-m1`; G12 on the silent player on Linux (paced, information); `ci/gates.txt` lines for the Mac, G7a, G7b and G7e there with the live default thread count | it adds a gate and its lines | `ci/check.sh --gates` on Linux (G12's device runs say "no output device" and skip; the paced and offline runs print); then the owner's Mac run (§5). Contract: gate 8's words, the *Today* lines, `song-as-program.md` |
| **5. Only for a measured failure** | in order of likely payoff: mixer strips into their tracks' partitions; pace the dropper's `munmap`s; spin between callbacks; QoS for the actor, compile and dropper; home threads for plugins; `wfe`; players' fills in their lane's partition; Panic through the pool; voice splitting | each on its own measurement | G12 again |

### 4.1 The OS pieces in `m2-os`

All behind one portable module, `core/os/src/rt.rs`, with `mac/rt.rs` and `linux/rt.rs` (platform design §8). Each
does what the OS allows or says what it can't (PLAT-3); a refusal is reported, never fatal.

| Function | macOS | Linux |
|---|---|---|
| `cores()` | `hw.perflevel0.physicalcpu`, `hw.perflevel0.cpusperl2`, `hw.perflevel1.physicalcpu` | the cores sharing the last-level cache, from sysfs, inside the affinity mask and the cgroup's quota (one CCD here) |
| `Group::of_output(uid, period)` | the device's `AudioDeviceID` from cpal's UID (`kAudioHardwarePropertyTranslateUIDToDevice`), its workgroup (`kAudioDevicePropertyIOThreadOSWorkgroup`, released on drop), the period in Mach ticks, `os_workgroup_max_parallel_threads` | the period, and the real-time priority to ask for |
| `on_device_thread(&Group)` | nothing to do: CoreAudio made the IO thread real-time and put it in the workgroup; checks and reports | `SCHED_FIFO` on cpal's ALSA thread, if `RLIMIT_RTPRIO` allows; else reported |
| `engine_thread(&Group) -> Joined` | `THREAD_TIME_CONSTRAINT_POLICY` (period, half of it as computation, the period as constraint, preemptible), then `os_workgroup_join` (the 40-byte token on the worker's stack) | `SCHED_FIFO` if allowed; else reported |
| `end_engine_thread(Joined)` | `os_workgroup_leave` | nothing |
| `background_thread()` | `QOS_CLASS_UTILITY` | nice +10 (always allowed) |

- **Linux without root.** Raising a thread needs `RLIMIT_RTPRIO` (for `SCHED_FIFO`) or `RLIMIT_NICE` (for a negative
  nice), or rtkit over D-Bus. This server has `RLIMIT_RTPRIO` 0, `RLIMIT_NICE` 0 and no rtkit, so every request is
  refused here, and its timings stay information (PLAT-1). Build the direct `SCHED_FIFO` call only (about 15 lines).
  rtkit goes to the Deferred list: it can't be run here, and code never run isn't finished. (cpal's own `realtime`
  feature, which uses rtkit, is off in music2.)
- **Waking.** std's `park` and `unpark` need no OS code (a futex on Linux; on Apple, a dispatch semaphore, from
  memory of std's source). A `Wake` in `m2-os` comes only if the Mac measures std's wake as slower.
- **No topology in `machine()`.** Print `cores()` on a line of its own in every gate run, so old records still match.

### 4.2 The default thread counts

| Where | Count |
|---|---|
| Live, Mac | `min(workgroup's recommendation, P-cores per L2)`, at least 1: 4 on the M1 and the M1 Pro (*estimate*; the Mac confirms) |
| Live, Linux | the cores sharing the last-level cache (8 on one CCD here) |
| Silent player, tests | 1, unless `M2_THREADS` or the test sets it; the 1-vs-N tests fix 1, 2, 3, 4 and 7 |
| Offline, the app not playing, or the export holds playback | the P-core count, default QoS |
| Offline beside playback (a pure song's export, a range render, a CLI render that finds the lock held) | the E-core count, `background_thread()` |
| Any | an explicit count wins; the schedule's width caps how many are woken |

### 4.3 What runs where, refreshed

| Thread | What it does | Priority |
|---|---|---|
| Device thread (CoreAudio's IO thread; cpal's ALSA thread) | messages at block lines, the pre-pass, its share of tasks, the post-pass, `LiveFade`, the resampler, publishing, the load record | CoreAudio's own; Linux `SCHED_FIFO` if allowed |
| Engine workers (N−1) | partition tasks: kernels, polys (voices and their sum), plugin calls, fades, ghosts | Mac: time-constraint, in the device's workgroup; Linux: `SCHED_FIFO` if allowed |
| `m2-play` (`serve`) | owns the stream and the pool; logs device errors; when let go, stops the stream, then joins the workers | default |
| `m2-play-drop` | drops what the callback retires | default (utility QoS only if measured, slice 5) |
| Plugin main thread | create, activate, `load_state`, deactivate, destroy | unchanged |
| Actor (prepare) | `Renderer::prepare`, and the schedule with it | default |
| An export or render's thread, and its offline pool | `Renderer::lending` or `new`, on its own pool | see §4.2 |

### 4.4 The tests, refreshed

The determinism tests (new gates in CI; thread counts fixed, never the machine's):

| Test | Where | Proves |
|---|---|---|
| `every_pure_fixture_renders_the_same_bits_on_1_and_n_threads` | `core/play/tests/bit_identity.rs` | RENDER-2's thread rule, the CI gate: first-song, resample, starter and empty on 1, 2, 3, 4 and 7 threads |
| `random_convex_partitions_give_the_same_bits` | `core/runtime` tests | random cuts of a random order, jitter before each task, every worker woken; asserts two threads ran tasks and a consumer ran away from its producer |
| `an_edit_window_renders_the_same_bits_on_1_and_n_threads` (new, step 7's) | `core/runtime/src/tests/live/keep.rs` | ghosts, changed-edge fades that wait, input fade-ins, a kept poly, a ghost lane, and a cycle between modules that exists only in the window: the live engine on 1 against 4 threads, sample for sample |
| `a_hold_and_resume_on_n_threads_matches_1` (new) | `core/play/tests/live.rs` | the hold's fades, the frozen clock and the resume's note-ons (O4), with the probe plugin |
| `live_on_4_threads_is_the_export_on_1` | `bit_identity.rs` | RENDER-33 with workers |
| `the_pre_pass_runs_in_the_canonical_mode`, `a_flush_to_zero_left_by_a_plugin_changes_no_bit` | `core/play/tests` | RENDER-2a |
| `p16_renders_the_same_bits_on_1_and_4_threads` (30 s in CI) and, recorded, `p50` on 1 and 4 | `bit_identity.rs` | the reference songs |

The others design-8a §10 lists stay, with three changes: `no_alloc.rs` counts workers through `alloc`'s global count;
"a closed player" becomes "a player let go" (K1's path); G7e's probe test runs on a 4-thread player. Add
`a_trap_on_a_worker_is_caught` (strict names the node; live goes silent), which the Mac run proves for Mach ports.

### 4.5 The gates

| Gate | When | How 8a affects it |
|---|---|---|
| Renders 8 of 8, bit identity, `no_alloc`, `no_blocking`, `one_place.rs` | every slice | must not change; the 1-vs-N tests join them |
| G7a (an edit heard within 200 ms), G7b (untouched tails bit for bit), G7e (the export's hold) | every slice; from slice 3 also with `--threads 4` | with workers live: G7a re-checks the compile beside them; G7b and G7e on 4 threads |
| G7f (the Store layout's cost) | every slice | stays on one thread: it compares layouts, and its records are single-threaded |
| G11 | every slice | unchanged |
| G12, gate 8 | slice 4; judged only on the Mac | §5 |

---

## 5. What the owner's Mac must confirm

At the end of 8a, the usual `ci/check.sh` run, then the gate's own runs. The verdict on the M1 Pro is "provisional
(stand-in: MacBookPro18,1, `--like-m1`)" until a base M1 exists (the owner's decision).

| # | What | It should show |
|---|---|---|
| 1 | `ci/check.sh` | PASS, every row: the 1-vs-N tests, `no_alloc` on workers, the trap on a worker caught (Mach ports), the new `m2-os` Mac tests, the loom row |
| 2 | `m2-gates realtime --like-m1`: the engine alone, 10 minutes at 128 frames | no dropout from the first callback; p99 at most 1,867 µs |
| 3 | `realtime --server --like-m1`: the app's own path, p50 edited every 10 s | no dropout; each edit heard within 200 ms; the compile within 100 ms at p95 |
| 4 | `realtime --export --like-m1` | no dropout while a pure song exports beside it |
| 5 | What the runs print about the Mac | the workgroup's recommended threads and P-cores per L2 (expected 4); every worker joined; the `FPCR` on CoreAudio's thread; 128 and 64 frames accepted; the first callback's set-up time |
| 6 | The recorded runs | 64 and 512 frames; the sweep over 1 to 4 threads; `with-plugins` on 4 threads for 10 minutes (plugins' first touches); the offline sweep without `--like-m1` |
| 7 | By ear (about 5 minutes) | p50 in the app plays smoothly for a minute; `with-plugins` in the debug build never prints "the audio thread allocated" |

**One extra, short run (the lead's choice).** After slice 2, five minutes of `m2-gates realtime --offline --sweep
1,2,3,4,8` on the Mac would show the M1 Pro's real scaling before the live work starts. Recommended: it is the
cheapest way to meet R1 early.

---

## 6. Risks and open questions

Each has a recommendation; the lead decides.

| # | Risk or question | Recommendation |
|---|---|---|
| R1 | Scaling falls short of what gate 8 needs: the work follows the cache (+10 to 40% on a smaller or shared one), so 4 threads on one L2 give less than 4 times | The spike first; cap threads at one L2 (one CCD on Linux); land the speed patches first; ask for the one short Mac sweep after slice 2 |
| R2 | The p99 tail doesn't shrink with the mean (page faults, other programs) | `profile_parts` reports p99 per partition and the critical path; G12 judges p99; slice 5's first items if it fails |
| R3 | An activation's block at 128 frames: on one thread the slowest are 3.0 to 4.2 ms, over a 2.67 ms period, and `--server` counts every late callback | It runs its ghosts and fades in parallel, so it should scale like any block; G12 `--server` prints each activation's block, split into `take_over` and the render |
| R4 | The device thread does wasmtime's per-thread set-up inside its first callback (a 256 KiB map on Linux, exception ports on the Mac). True of step 7 today | `thread_ready()` at the top of every fill (one thread-local read after the first); the Mac prints the first callback's time; accept once per player |
| R5 | The pool dropped with the callback on ALSA's audio thread | The stream's thread owns the pool and drops it after the stream |
| R6 | A trap on a Mac worker waits for wasmtime's normal-priority helper thread | Accept: only when a kernel crashes, and the plan goes silent then; test that the trap is caught on a worker |
| R7 | Plugins' thread-locals first touched on new workers; plugins that assume one audio thread | design-8a's probe and the 10-minute `with-plugins` run; home threads only for a traced dropout |
| R8 | A plugin that locks inside `process` while step 7's prepare loads its state on the main thread | design-8a's bounded waits: a dropout, no spinning, as today |
| R9 | Linux real time can't be checked here (no device, no rtkit, no limits) | Direct `SCHED_FIFO` only, refusals reported; rtkit to the Deferred list with the trigger "someone plays music2 live on a Linux desktop"; Linux timings stay information |
| R10 | G7f would mean something else with threads | Pin G7f to one thread |
| R11 | `machine()` with topology breaks every record | Topology on its own line |
| R12 | The speed branch rewrites the same hot functions (`KernelNode::process`, `Graph::process`'s scan, `KernelInst`'s store) | Land it before slice 2; slice 2 rebases onto it |
| R13 | HV3 ties a pool's voices to one `Store` | Accept; voice splitting's trigger says it needs a `Store` per chunk |
| R14 | std's `park` wakes a real-time worker more slowly than a Mach semaphore | Measure the wake in slice 3 on the Mac; a `Wake` in `m2-os` only if it is worse |
| R15 | The dropper's `munmap`s now race N real-time threads (design-7 R13) | G7f's drop test with a 4-thread player as the other side; pace the dropper only if measured (slice 5) |
| R16 | A Panic resets about 6,000 kernels serially on the device thread: likely a dropout at p50 (true today) | Record one Panic in G12 `--server`; reset through the pool only if it drops out (slice 5) |
| R17 | An export beside playback fights for the cache | A held export (plugins) uses the P-cores, since live sleeps; a pure song's export uses the E-cores at utility QoS; `--export` is judged |
| R18 | The 64-frame run looks bad (about 90 to 105%, *estimate*) | Recorded, not judged, as the gate says |
| R19 | `sounds` brings 6 new kernels; if one lets its state go subnormal, x86 slows and FZ is not allowed | At the `sounds` merge, check each new kernel uses std's `flush` (`std/kernels-src/src/fx.rs`) |
| R20 | No base M1 | The owner's decision stands: provisional on the M1 Pro under `--like-m1` |
| Q1 | The pool in its own crate, or inside `m2-runtime`? | Its own crate: loom then rebuilds only it, so the loom row can join every check |
| Q2 | The player's 5 ms `LiveFade` runs two engines in one callback | Accept: both use the same pool, one run after the other, with `again` |
| Q3 | Keep `Wiring`'s inputs, or work them out again from each `Exec`? | Keep them: one source of truth with Kahn's order |

---

## 7. Contract edits, refreshed

Each lands in the slice that changes the code.

| Rule | Today's words | The edit | Slice |
|---|---|---|---|
| RENDER-2 | "…every sum (buses, `poly` voices, overlay mixes) runs in plan order in the consuming partition." | design-8a §12's text, with fades named among the nodes a sum may not cross | 2 |
| RENDER-2a (new) | — | design-8a §12's text | 2 |
| RENDER-32's *Why* | the owner's layout (O1), no "per partition" fallback | add: a `Store` is entered by one thread at a time, so any box can run on any engine thread | 2 |
| RENDER-28 | "The audio thread MUST NOT…", with step 7's bullets (cells, the queue, stalls, the trap) | "engine threads", design-8a §12's wait rule and abort, merged into today's bullets; the trap covers every engine thread | 3 |
| RENDER-28a (new) | — | design-8a §12's text, with the count rule of §4.2 here and "a player let go joins its workers" | 3 |
| HOST-2 and the `PluginHost` sketch | "…process plugins on the audio thread" | "on an engine thread: the device's or a worker, one call at a time per instance" | 3 |
| Gate 8 | as quoted in design-8a | design-8a §12's additions, with "provisional on a stand-in" | 4 |
| `song-as-program.md`, "Parallel (Not built yet)" | the plan | what was built | 4 |
| Deferred (new row) | — | rtkit for Linux engine threads; trigger: someone plays music2 live on a Linux desktop | 3 |

---

## 8. Files read

- `$SCRATCH/wt/s7`: `HANDOFF.md`; `docs/architecture.md` (RENDER-2, 28, 32, 33, HOST-1, 2, PLAT-1 to 7, the gates);
  `docs/designs/step7/design-7.md`, `build-notes.md`; `docs/designs/step8/design-8a.md`, `understand-8a.md`,
  `design-8b.md` (its 8a needs); `docs/designs/platform/design.md` §2.8, §3.8, §3.10, §5.5, §5.6, §8.
- Code: `core/runtime/src/lib.rs`, `engine.rs`, `plan.rs`, `keep.rs`, `plugins.rs`, `packages.rs`;
  `core/play/src/lib.rs`, `alloc.rs`, `tests/no_alloc.rs`, `tests/no_blocking.rs`, `tests/bit_identity.rs`;
  `core/plugins/src/host.rs`, `audio.rs`, `inprocess.rs`, `tests/no_blocking.rs`; `core/os/src/lib.rs`, `thread.rs`;
  `crates/gates/src/arena.rs`; `ci/gates.txt`; `std/kernels-src/src/fx.rs`.
- Crates: wasmtime 49.0.2 (`Cargo.toml` features, `src/config.rs`, `src/engine.rs`, `vm/traphandlers.rs`,
  `vm/sys/unix/signals.rs`, `machports.rs`); cpal 0.18.2 (features, `traits.rs`, `host/alsa/mod.rs`); clap-sys 0.5.0;
  mach2 0.6.0; objc2-core-audio 0.3.2.
- Speed workflow: `$SCRATCH/speed/prof/logs/*.log`, `vg/m1-diff.txt`, `patches/hv1.diff`, `hv3.diff`, `hv4.diff`;
  `$SCRATCH/speed/sg/runs/*.log`; branch `s7-sg` at `5b77ae4`.
- This server's limits: `ulimit -r` 0, `ulimit -e` 0, no rtkit on the system bus.

---

## Attack

Written 2026-10-16, read-only. I checked the refresh against `step7` at `ed7fc8e` (`$SCRATCH/wt/s7`), the speed
workflow's patches and logs, cpal 0.18.2, wasmtime 49.0.2 and the platform design. I also read CLAP's
`thread-check.h` online (the header isn't on this machine). Nothing was built or run.

### In short

- **No bit changes.** I found no way a parallel schedule changes a render or live output. Every read of another node
  goes through an edge, sums stay inside one node, and frozen inputs stay frozen (§A.2).
- **Five things must change before anyone builds.**
  1. **The device thread must not sleep with std's `park`.** On CoreAudio's thread its first use allocates, and the
     handle it needs goes stale when cpal moves the stream to a new default device. Then a stuck block can sleep
     forever. Keep a `Wake` in `m2-os`, as platform §8 already lists (§A.3, item 1).
  2. **A panic inside a task would silence the song for good.** Today a panic marks the player crashed, and the
     server makes a new one. With per-task catches it becomes a "broken" plan, which nothing watches (§A.3, item 2).
  3. **The partitioner's edges.** Step 7 builds no edge list for a fresh plan, or for an edit with no fades. Take
     the edges from what each built node reads (`Exec`), one function used by the schedule and by the debug check
     (§A.1, row 5).
  4. **Tests and gates that can't fail, or that flake.** The 1-vs-N fixture tests may never run a task on a
     worker. The global allocation count races with two tests that allocate on purpose. Wall-clock bounds in pool
     tests will flake on this busy server. G12's verdict code first runs on the Mac (§A.5, §A.6).
  5. **The numbers are kinder than the facts.** A base M1 has a third of the M1 Pro's memory bandwidth and system
     cache, not half. Dividing p99 by the speed-up is optimistic. The spike as written measures its own thread
     start-up. Make the short Mac sweep after slice 2 required, not optional (§A.8).
- **Missing.** Two items the platform design gives 8a: OS-61 (no device: play silently and say so) and OS-49 (x86
  subnormals). Three platform §8 items were dropped without a word: `Wake`, `RLIMIT_RTTIME`, `sched_yield`. Also:
  "utility" QoS doesn't keep an export off the P-cores; CLAP's `is_audio_thread` means "any audio thread"; the
  blocks next to a prepare run about 50% slower; 8b.3 lengthens the song's serial tail (§A.9).

### A.1 Verdicts on the refresh's own items

**"In short" list**

| Item | Verdict | Why, and how |
|---|---|---|
| 1. OS code into `m2-os` | **keep**, but keep `Wake` there | `one_place.rs` doesn't list `target_arch` or `asm!`, so `fpenv` can stay in `m2-runtime`. Drop std's `park` for the device thread (§A.3, item 1). |
| 2. No `Player::close()`; the stream's thread owns the pool | **keep** the first; **change** the second | K1 is real (`core/play/src/lib.rs:668-691`; `tests.rs:478`, `:502`). But "the callback only borrows it" can't be a plain borrow. cpal's CoreAudio `Stream::drop` doesn't stop the callback by itself. A monitor thread may hold an upgraded `Arc` of the stream (`cpal-0.18.2/src/host/coreaudio/macos/mod.rs:302`), and then the `AudioUnit` and the callback drop later, on that thread, after `serve` has dropped the pool. Split the type in two. `Pool` holds the join handles and joins on drop. `Runner` holds an `Arc` of the shared state, `run(&mut self)`, and is owned by the callback, so only one thread can ever publish a job. After a stop, a `Runner` runs every task itself. `Pool::this_thread()` must keep no state, because two renders may share it at once. |
| 3. No overload listener | **keep** | Confirmed: cpal registers `kAudioDeviceProcessorOverload` and reports `ErrorKind::Xrun` (`mod.rs:100-118`, `:183-192`), and it registers it again after a reroute. |
| 4. Keep `Wiring`'s inputs | **change** | `window` returns before it builds `deps` when nothing fades (`keep.rs:378`). `Wiring::fresh` (`keep.rs:73`) has none, and `Renderer::new` (every export) uses it. So there is no list to keep on most plans. Write one `Exec::reads()` that lists every buffer a built node reads: kernel and plugin inputs, plugin params, `Missing`, a poly's captures, a fade's two sides. The schedule uses it, the debug `Reader` checks each real read against it, and a debug check says the order respects it. That is the true source: 8b.4's sidechain inputs will add reads that `FOp::deps` may not list. |
| 5. Label a new node's fade-in with its consumer | **keep** | Checked: `ins` fades have `old = Const(0.0)` (`keep.rs:372-375`). |
| 6. The pool in its own crate | **keep** | Give it a generic error slot, or only a failed flag, so `anyhow` stays in `m2-runtime`. Bound loom's search (`LOOM_MAX_PREEMPTIONS=2` or 3, small graphs), or the row costs minutes. Delete its target folder after each run (the 390 GB lesson). |
| Plugins on workers | **keep** | CLAP's header, read today: "The audio-thread is symbolic … the plugin.process() call may be scheduled on different OS threads over time … only one OS thread is the audio-thread at a time in a plugin instance." **Change one detail:** `is_audio_thread` "Returns true if "this" thread is one of the audio threads", which is host-wide. m2 answers true only for this instance's last caller (`core/plugins/src/audio.rs:179`). Today all instances share one audio thread, so the two meanings agree. With workers they don't: a plugin that keeps one host pointer for all its instances gets "no" on a real audio thread. Also answer true for any engine thread now rendering: a small fixed table of `pthread_self` tokens, filled at worker start and at each fill, read with atomics. |
| Stores on workers | **keep** | `KernelInst` is `Send` with no unsafe code; HV1's pointer moves with its store; HV3 makes one `Store` per `VoicePool` (checked in `hv3.diff`). |
| The numbers say 8a is needed | **keep** the conclusion; **change** the framing | See §A.8. |
| Scaling is the biggest risk | **keep**; **reframe** the evidence | See §A.8, item 3. |
| Slices, about 5 weeks | **change** | See §A.7. Add about 3 days. |

**Section 1's table** (33 rows). Rows I don't name hold as written. I checked 1-3, 6-9, 15-17, 20-22 and 33
against the code.

| Row | Verdict | Why, and how |
|---|---|---|
| 5 | **change** | As item 4 above. |
| 7 | **keep**, verified | `order_after` is the window's order minus ghosts, so fades stay in it (`lib.rs:1645`). After the window a fade copies `new` exactly (`lib.rs:1046-1058`). A ghost slot's `Const` does nothing (`lib.rs:1118`). One schedule gives the same bits. |
| 12 | **change** | `GraphView` over `Vec<Vec<Buf>>` through raw pointers is fragile: making a `&self.bufs` while another thread holds a `&mut` to one element is UB under Rust's aliasing rules, even when the elements differ. Give each node's buffers and version their own `UnsafeCell`, so a shared `&[UnsafeCell<…>]` is sound by construction. If a nightly toolchain is OK for one row, run the view's unit tests (with a fake step) under Miri. Loom checks atomics, not aliasing. |
| 13 | **change** | See §A.3, item 2. |
| 14 | **change** | Keep design-8a's `Wake`, in `m2-os` (§A.3, item 1). |
| 18 | **change** | `Pool` and `Runner`, as in item 2 above. |
| 19 | **keep**; **add** a Mac question | Read the workgroup after the device's IO has started, not when the pool is built. Each fill, check that it wasn't cancelled (`os_workgroup_testcancel`, cheap). cpal's default-output stream moves to a new device inside the same stream (`mod.rs:319-321`, "rerouted automatically"), which brings a new IO thread and a new workgroup. The player is replaced then, but until that happens the workers sit in the old group. |
| 23 | **change** | On Apple silicon only *background* QoS is kept on the E-cores. *Utility* threads also run on P-cores when they are free (public measurements; verify on the Mac). Live workers sleep about half of each period, so a utility export would take those P-cores and pollute the L2 that live uses. Use `QOS_CLASS_BACKGROUND` for the export's render threads, and keep the file writer at default QoS: background also slows disk I/O. |
| 26 | **change** | See §A.6. |
| 28 | **change** | `ALL` (`core/play/src/alloc.rs:21`) is one count for the whole process. `an_allocation_in_the_callback_is_counted` and `the_trap_counts` (`no_alloc.rs:498`, `:517`) allocate under the trap on purpose, in the same test binary, in parallel threads. A worker test that reads `ALL` would fail at random. Give each pool its own counter (each worker counts into its pool's), or put the worker test in its own test file, which is its own process. |
| 31 | **change** | Add the risks of §A.9. |
| 32 | **keep**, with a note | The baseline comes from back-to-back `render_block` loops (G7f's `store`), not from 128-frame callbacks paced on a clock (§A.8, item 2). |

### A.2 Determinism: no bit changes found

What I checked, in `core/runtime/src/lib.rs`:
- **Each read of another node** is an input that a dependency lists: `KernelNode::process` (inputs and versions),
  `plugin_events` (port params), `PluginNode::process` (audio in), `Missing`, `Capture` (the poly's captures are in
  `FOp::deps`, `core/ir/src/check.rs:95-101`), and `fade` (both sides, `keep.rs:415-420`).
- **Frozen during the graph:** lanes (`value_at` is pure, `core/song/src/lane.rs:906`), schedulers
  (`sounding_note` only reads), audio players, `Block`, cells (applied before the graph), and plugin flags
  (atomics, `plugins.rs`). No `Cell`, `RefCell`, `Rc` or `thread_local!` outside tests in `core/runtime/src` or
  `core/song/src`.
- **Written only after the join:** feedback rings (`lib.rs:1193-1205`) and the output. `poisoned`, `versions` and
  the buffers are one per node. Neighbouring entries in `poisoned` and `versions` share cache lines across
  partitions: false sharing, which is a speed matter and not a bits one.
- **A behaviour change to write down** (not a bits one, since 1 and N threads agree): today a live trap stops the
  loop at the failing node (`done?`, `lib.rs:1190`). In 8a, partitions that don't depend on it still run that block.
  So kept boxes carry one more block of state into the next plan. Say so in RENDER-2's edit, and test it in
  `a_trap_on_a_worker_is_caught`.

### A.3 Real-time safety: allocation, locks, waits, deadlock

1. **The device thread's sleep (the `stuck` path).** Std's `park()` reads `thread::current()`. On a thread std
   didn't start (CoreAudio's IO thread), the first call makes a `Thread` handle: an allocation and a TLS
   destructor. That is from memory of std's source; `rust-src` isn't installed here. A worker that wakes the device
   thread must also hold that handle. cpal's default-output stream changes its IO thread inside one stream (row 19),
   so a stored handle can point at the old thread. The new IO thread, parked on a stuck block, is then never woken.
   The callback never returns, and the player stalls for good. Today's tests can't see this: every test fill runs
   on a std thread. Fix: keep design-8a's `Wake` in `m2-os` (`mac/rt.rs`: a Mach semaphore; `linux/rt.rs`: a futex
   on an `AtomicU32`). Platform §8 already names `Wake (signal, wait)`. A semaphore doesn't care which thread waits.
   Std's `park` stays fine for workers, which std starts. An even simpler option: on the stuck path the device
   thread sleeps in short steps (`thread::sleep(period / 4)`) and needs no wake at all. That path is already a
   dropout. Add a test: drive `Driver::fill` from a thread made with `pthread_create` (tests may use OS code) under
   the trap, through a first fill, a stuck block and its wake.
2. **A panic inside a task.** Today `Callback::fill_in` catches it (`core/play/src/lib.rs:390`) and calls `crash()`
   (`:519`). That sets `crashed`, and the server lets the player go and makes a new one (`crates/server/src/graph.rs:402`).
   design-8a makes a task panic "a failure", so the plan goes `broken` (`lib.rs:1906`). Nothing outside tests reads
   `broken`, so the song stays silent until the next edit, with no message. Fix: a task panic is kept apart from a
   task error. After the join, the device thread calls `crash()` as today. The task records which node panicked,
   so `poisoned` marks it too.
3. **`again` threaded through four layers.** Whether "another run follows in this callback" isn't known inside
   `Engine::process`: the resampler's closure pulls engine frames chunk by chunk, and `LiveFade` adds a second
   engine. Simpler: every run leaves the workers spinning for at most `patience`, and `Callback::fill_in` calls
   `runner.rest()` once at its end, which sends them to sleep. That is one call in one place, and one more loom case
   (`rest` racing a publish).
4. **Debug marking of workers.** Mark a worker only around its work, not for its whole life. Thread exit frees std's
   `Thread` handle, which the counter would count.
5. **Once-per-thread system calls.** `thread_ready()` maps a 256 KiB signal stack on Linux (wasmtime
   `signals.rs:498-580`) and sets exception ports on the Mac. Linux's `SCHED_FIFO` call happens inside the first
   callback too. RENDER-28's new words ("in a normal block the only system call is the wake") should exempt "once
   per thread, on its first fill".
6. **Checked and fine:** CLAP's `token()` is `pthread_self` with no TLS (`core/os/src/thread.rs`). `audio.rs`
   reaches each instance through atomics, with an `UnsafeCell` per instance. wasmtime is built without async, GC or
   components (`Cargo.toml:39`). `tls_eager_initialize` does nothing after its first call (`traphandlers.rs:1186-1193`).
   ALSA's `Stream::drop` joins its thread (`cpal …/alsa/mod.rs:1389-1398`), so on ALSA the callback is gone before
   `serve` drops the pool.

### A.4 Plugin and Store rules

- CLAP: confirmed, with the `is_audio_thread` fix of §A.1.
- Stores: fine. **One thing to fix before landing the speed patches:** HV3 turns off the debug ABI-1 input check for
  every pooled voice kernel. `keep_inputs` returns at once when the kernel has no store of its own (`hv3.diff`
  around line 118). That check is a contract rule (RENDER-24). Check voice kernels through the pool's store instead.
- HV4 moves the non-finite scan into `take_from`. That is serial, on the device thread, in the activation's block,
  over every moved box (about 2,500 at p50). In 8a that scan can't run in parallel, while the per-block scan it
  replaces did. It runs in the block that G12 `--server` judges. The refresh calls this a fit; it is the opposite.
  Measure `take_over` with HV4 on its own line in G12 `--server`, or scan in the tasks instead.

### A.5 Slices that can't be green alone, and flaky checks

- **Pool tests with wall-clock bounds** will flake here. `ci/check.sh` allows no retry, and this server runs other
  timing work (load 6 to 15 in the logs). These are: `a_blocked_plugin_spins_no_thread` (spin ≤ 3 × patience +
  stuck), `a_stuck_plugin_leaves_every_engine_thread_asleep` ("after 50 ms"), and
  `workers_stay_awake_between_runs_of_one_callback` (at most one park per callback). Count CPU time
  (`m2_os::thread::cpu_time()` exists) or count events, not wall time, or move them to the gates.
- **R5 can be tested without a device.** `core/play/src/tests.rs:408` already has a stream that runs the callback
  on its own thread and drops it there, like ALSA's. Extend `a_device_player_let_go_…` to own a pool, and check that
  every worker joined after the callback dropped.
- **Slice 2 changes G7f.** Live and offline both go through the 1-thread executor (atomics per partition,
  `catch_unwind` and `canon()` per task, the `Ports` step). G7f compares the app with S, the commit before step 7,
  built fresh (`s7-sg`). So the executor's own cost lands inside "the layout's cost", which has 80 µs of room; Linux
  used 41 of them (`sg/runs/final-1.log`). Record the executor's 1-thread cost as its own paired line in slice 2:
  the old loop against the executor, one thread, same build. Then the Mac's G7f can be read.
- **Slice 2 holds Mac-only code that only compiles here:** `cores()` and `background_thread()`. PLAT-6 allows that.
  But by the same rule the refresh uses to defer rtkit ("code never run isn't finished"), Linux's `SCHED_FIFO`
  success path can't run here either: `RLIMIT_RTPRIO` is 0. Be consistent. Build only the Linux refusal path (it
  runs here), and defer `SCHED_FIFO` with rtkit under one trigger. Or ask the owner once for `snd-aloop` with a
  raised limit (platform §8 already says that needs the owner's OK).

### A.6 Gates that can't fail

- **The 1-vs-N fixture tests may run everything on one thread.** First-song's 5 partitions take 5 to 20 µs each,
  which is less than a busy machine's wake time. The device thread can finish the block before a worker wakes, and
  then the CI gate proves nothing. Only `random_convex_…` checks for overlap. Use design-8a's test hook in every
  1-vs-N test (wake every worker; in test mode the device thread waits until each worker has taken a task), and
  assert that a task ran on a worker in each render.
- **G12's verdict first runs on the owner's Mac.** On Linux its device runs say "no output device" and skip, and the
  paced runs only print. A wrong period, a counter never bumped, or a p99 off by a bucket would pass on the Mac.
  Unit-test the verdict on load records made up by hand, with one that must **FAIL** for each cause (an xrun, a late
  callback, a gap, a worker that didn't join). Inject a slow block (a test hook that sleeps 5 ms) in the paced
  Linux run, and check it is reported. On the Mac, "no output device" must be a FAIL, not a skip.
- **"Provisional" is weaker than it reads.** macOS has no thread affinity on Apple silicon, so the "one L2" cap is
  a thread count, not a place. On the M1 Pro, 4 workers may spread over both P clusters (24 MB of L2) and its 24 MB
  system cache. `--like-m1`'s busy loops run at default QoS, so real-time threads take their cores whenever they
  want. Say in the verdict line that the stand-in can't reproduce the M1's single 12 MB L2 and 8 MB system cache.

### A.7 Slices, refreshed again

| Slice | Verdict | Changes |
|---|---|---|
| 0. Spike | **change** | (a) No `std::thread::scope` per block. Every new thread pays wasmtime's set-up (map, protect and register a signal stack, then unmap at exit) and a spawn and a join, every 2.67 ms, and that cost counts against parallelism. Use threads that live for the whole run with a simple barrier (offline, so allocation is fine). (b) Make CCD1 (32 MB L3) the main Linux result. CCD0's 96 MB V-cache may hold p50's whole working set, which no Mac has. (c) Record `perf stat` LLC and dTLB misses per block for 1 and 4 threads. (d) Try "sticky" placement (each partition goes first to the thread that ran it last block) against the FIFO queue. With about 18,000 small pages touched a block, TLB reach may matter more than balance. (e) Time 1 thread paced at 128 frames (the silent player, `M2_AUDIO_FRAMES=128`) against back-to-back. (f) Rule: Linux numbers only decide whether to go on. The Mac sweep after slice 2 decides whether 8a can pass. |
| 1. The pool | **keep**, with `Wake` | `m2-pool` takes a `Wake` from `m2-os` (std's `park` under loom). Add `rest()` and the `Pool`/`Runner` split. |
| 2. Executor, offline | **change** | Add `Exec::reads()`, `UnsafeCell` buffers, the executor's own G7f line, and the OS-49 measurement (§A.9). Then a **required** Mac checkpoint: `m2-gates realtime --offline --sweep 1,2,3,4` (5 minutes, no `--like-m1`), plus `sysctl hw.perflevel0` and the paced 1-thread run. Live work starts only after it. |
| 3. Live | **change** | Add `Wake`, the foreign-thread trap test, per-pool allocation counts, the workgroup read after IO starts, the task-panic → `crash()` path, OS-61, and the thread count falling back to 1 when real time is refused. |
| 4. The gate | **change** | Add the verdict tests that must fail (§A.6). Record a resampled run (44.1 kHz project on a 48 kHz device: some callbacks render two blocks). Print the blocks during each edit's compile and prepare apart from the activation's block. |
| 5. Only if measured | **keep** | Move "mixer strips into their tracks' partitions" up the list if the spike shows the tail matters after 8b.3 (§A.9). It is a small labelling rule: a node whose only input from outside its module comes from one track joins that track. That keeps partitions convex. |

### A.8 Gains overstated

1. **A base M1 is further from the M1 Pro than the refresh says.** Apple's published figures: 68 GB/s against 200,
   and an 8 MB system cache against 24. That is a third, not "half" (design-8a's Q1 says "twice" too). If the work
   follows memory, as the refresh argues, the base M1's 1-thread p99 is likely well above 3.2 ms. Then the speed-up
   needed is above 1.7×, maybe above 2.5×.
2. **No 128-frame paced number exists on any machine.** Every 1-thread figure is a loop of blocks with no gap
   (`store` gate) or 512-frame `Driver::fill` loops (`arena`). Live callbacks come every 2.67 ms, with sleeping
   workers, caches others have used in between, and changing clocks. Measure it paced (slice 0 on Linux, the slice-2
   Mac checkpoint).
3. **The scaling evidence is mixed up.** "+10 to 40% on a smaller or shared cache" compares CCD0 (96 MB) with
   CCD1 (32 MB), and CCD1 ran with load 6 to 10 beside it (`ls-C1-ccd1-contended.log`). So cache size and contention
   are mixed. Sharing a cache doesn't make the block's working set bigger. Work bound by memory *latency* often
   scales well across cores, since more misses are in flight. Work bound by *bandwidth* doesn't. A rough guess:
   6,000 instances touching 2 to 8 KB each is 12 to 48 MB a block, which is 4 to 18 GB/s at 2.67 ms, and three
   times that at the 0.9 ms 8a wants. On a base M1 that nears its 68 GB/s. So R1 is real, but for bandwidth and TLB
   reasons, not "a shared cache". The spike's perf counters (§A.7) should say which.
4. **p99 ÷ speed-up is optimistic.** A parallel block ends when its slowest thread ends. Any one thread's hiccup
   (an interrupt, a page fault, coreaudiod, a late wake) delays the whole block, so tails grow with thread count.
   Treat 2× as a fail, not "just inside". Plan for 2.5× as the floor.
5. **The serial tail grows before 8a starts.** 8b.3 lands with `taste`, before 8a. It makes every channel's `db`
   always present, so each channel has a gain node, and a panner once pan is written (design-8b lines 41, 268).
   All of them sit in `song`. The 100 to 180 µs serial estimate predates that.

### A.9 Missing from the refresh

| What | Why it matters | Recommendation |
|---|---|---|
| **OS-61**: no device, or a device whose `default_output_config` fails, should give the silent player and one message | The platform design gives it to 8a (`platform/design.md:627`, §8 row); gaps.md #13 ("an unplugged audio interface locks you out of your song"). On this server the config call fails, not the device lookup (`ground/facts.md:285`). `Player::start` still fails (`core/play/src/lib.rs:741-743`). | Slice 3, in `Player::start_with`. |
| **OS-49**: subnormals on x86 | Platform §5.6 says "8a decides" a deterministic rule in std, and measures the cost on Zen 4 (`design.md:947-955`, :1898). The refresh only checks `sounds`' kernels (R19). | Slice 2: time a decaying reverb tail on this server. Then decide (std rule, or a written deferral with the owner's OK). |
| Platform §8 items dropped without a word | `Wake (signal, wait)`, `RLIMIT_RTTIME` at 200 ms with `SIGXCPU` ignored, `sched_yield` in spins (`design.md:1737`). | `Wake`: keep (§A.3). `RLIMIT_RTTIME` and `sched_yield`: they only matter once `SCHED_FIFO` is granted, so defer them with it, and say so in the Deferred row. |
| The blocks around an edit | G7f's logs: the block just before each activation runs at p50 2.6 to 3.7 ms, against 1.8 ms for a normal block (`sg/runs/*.log`, "the block before it"). The prepare ran on the same thread and left caches cold, even with one untimed block between (`arena.rs:217`). In the app the prepare and the "what continues" naming (10 to 32 ms) run on another thread, but on a base M1 they share the one P-cluster's L2 with all 4 engine threads. | Add to R3. G12 `--server` prints the blocks during each compile, naming and prepare. If they fail, slice 5's QoS item for the actor comes first. |
| Thread count when real time is refused | Linux's live default is 8 threads, but this server and stock desktops refuse `SCHED_FIFO`. Normal-priority workers wake late and can be preempted holding `inside`, which a time-shared desktop makes worse than one thread. | Live count = 1 when the engine-thread request is refused, on any OS, unless set by hand. Count physical cores, not SMT threads. |
| `Player::load()` | "load" already means loading a project in `Shared` (`loads_sent`). The owner reads plain words. | Name it `Player::timing()`. |
| FPCR on CoreAudio's thread | `canon()` changes CoreAudio's thread mode and never puts it back. | Save it at fill entry and restore it at exit (two instructions). It's polite, and if CoreAudio wanted FZ it keeps it. |
| Resampled projects | At 128 frames, a 44.1 kHz song on a 48 kHz device has callbacks that render two blocks. | Recorded in G12 (§A.7). Not judged. |
| Disk | A loom target folder and spike builds | Delete them at each slice's end (memory: 390 GB). |

### A.10 Risks and questions: verdicts

| # | Verdict | Note |
|---|---|---|
| R1 | **keep**, reframe | Bandwidth and TLB, not "a shared cache" (§A.8, item 3). |
| R2 | keep | |
| R3 | **change** | Add the blocks during prepare, and HV4's serial scan (§A.4, §A.9). |
| R4 | keep | Add RENDER-28's once-per-thread words (§A.3, item 5). |
| R5 | keep | Testable now with the fake ALSA stream (§A.5). |
| R6, R7, R8 | keep | |
| R9 | **change** | Fall back to one thread when refused. Be consistent about code that can't run here (§A.5). |
| R10, R11, R13, R15, R16, R18 | keep | |
| R12 | keep | Fix HV3's lost ABI-1 check first (§A.4). |
| R14 | **drop** | Build `Wake` from the start. The risk then can't happen, and std's handle problem goes too. |
| R17 | **change** | Background QoS, not utility (§A.1, row 23). |
| R19 | **change** | It is OS-49, an 8a item (§A.9). |
| R20 | keep | Fix the words: a third of the bandwidth and system cache, not half. |
| Q1 | keep | Generic error slot. |
| Q2 | keep | With `rest()` in place of `again`. |
| Q3 | **change** | Edges from `Exec::reads()`, not from `Wiring` (§A.1, item 4). |

### A.11 Contract edits

- **HOST-2:** "an engine thread … one call at a time per instance". Also: `is_audio_thread` answers true on any
  engine thread now rendering, as CLAP words it ("one of the audio threads").
- **RENDER-28:** add "once per thread, on its first fill" for the set-up system calls. Add that a panic in a task
  still makes the player crash, as today.
- **RENDER-28a:** "background QoS" in place of "utility". The live count falls to 1 when real time is refused.
- **RENDER-2:** add that independent partitions still run in a block where one fails.
- **Gate 8:** the provisional line says the stand-in can't reproduce the M1's cache and bandwidth.
