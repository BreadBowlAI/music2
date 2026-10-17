# 8a's live half, the gaps lane: build notes

What the gaps lane built, how it was tested, and where the logs are. The plan is
`~/m2-runs/s8a/plan-live.md` §5 (its Attack and Decisions win: D24, D27). The lane ran beside slice 3 on its own
branch (`8a-gaps`, worktree `~/m2-runs/wt/8a-g`, from `8a` at `98620e7`), with its own target
(`~/m2-runs/8a-scratch/target-gaps`) and scratch HOME (`~/m2-runs/8a-scratch/g-home`).

## In short

| Gap | State | What it says |
|---|---|---|
| G1: a sound that crashes while the song plays | done | **Only that sound goes silent.** The rest plays on, bit for bit. Before, the whole song stopped. |
| G2: a sound over its 32 MB | done | **It says "ran out of memory"**, with the sound and the cap. Before: "wasm trap: unreachable". The app no longer tells the user to close other apps for it. |

## G1: a kernel that crashes while the song plays goes silent alone

### In short

- **Before:** a kernel that trapped while the song played made the whole song silent, and stopped it. Nothing said
  why. The renderer stayed `broken` until the next plan.
- **Now:** only that node stops. Its outputs are silence at once, and it is never called again. What reads it reads
  silence. Every other node plays on, bit for bit, on any number of threads.
- **The plan marks it.** Each plan has a `Stopped`: one byte per top-level node (0 playing, 1 crashed, 2 out of
  memory) and a count of stops. The session reads it (G3).
- **What brings it back:** the next activation, which resets it as it moves it (as before), or Stop All Sound,
  which resets every instance (RENDER-10). A reset that fails again leaves it stopped.
- **Exports don't change.** Offline, a failing node still fails the render, naming the node.
- **No render changed:** 10 of 10 records match.

### What was built

| Where | What |
|---|---|
| `core/runtime/src/stopped.rs` (new) | `Stopped` (`stop`, `why`, `news`, `clear`), `Why` (`Crashed`, `OutOfMemory`), `OutOfMemory` (for G2). The audio thread only stores into atomics |
| `lib.rs`, `run_part` | live (`!b.strict`): a node whose step fails is marked in `Stopped`, its box set stopped, its outputs zeroed; the partition goes on. Strict: unchanged |
| `lib.rs`, the boxes | `KernelInst.stopped`, `VoicePool.stopped`: a stopped box plays silence and calls nothing. In the box, not the node: it moves with the box, and the reset that comes with a move clears it |
| `lib.rs`, `step` | a stopped box stays poisoned, so the next activation resets it as it moves it |
| `lib.rs`, `reset_node` | a reset that succeeds clears `stopped`. One that fails on a stopped box leaves it stopped, and the panic goes on |
| `lib.rs`, `Graph::active` | a stopped kernel isn't asked: it holds nothing that will sound, and isn't called again |
| `lib.rs`, `Renderer` | `broken` and `broken()` gone; `pre` always gives a block; `panic` clears the marks of what plays again |
| `plan.rs` | `Plan.stopped` and `PlanInfo.stopped` (one `Arc`); `take_over`: a moved box whose reset fails stops alone, marked (was: the whole plan `broken`) |
| `engine.rs` | the render's `Err` arm can't happen live now: counted, the block silent, no transport stop |
| `gauge.rs` | `run_graph` and `run_part` take the plan's `Stopped` (one argument each) |

Where it differs from the plan:

- **`stopped` lives in the box** (`KernelInst`, `VoicePool`), not in `KernelNode` and `PolyNode`. It moves with what
  failed, and a move resets it. It also keeps `tests/parts.rs` (R1's) unchanged: its `KernelNode` literal still
  builds.
- **Stop All Sound starts a stopped sound again.** The plan's words said "your next change to the code, or Undo,
  starts it again". That is true only for a change that makes a new plan. A value change (a knob, a number in the
  code) is a cell write: no new plan, so the sound would stay silent. A panic already resets every instance
  (RENDER-10), so it now clears `stopped` too. That gives the user one action that always works (G3's words).
- **`gauge.rs` changes by two arguments and two lines** (`pre` gives a block now). The plan listed it as slice 3's,
  but slice 3 doesn't edit it, and `run_graph`'s arguments are the lane's.

### Tests

Each new test was shown red once by a small deliberate bug, its change and run kept in `~/m2-runs/s8a/g-red/`
(`<name>.diff`, `<name>.log`; `mutate.py` puts the file back afterwards).

| Test | Where | Proves | Red by |
|---|---|---|---|
| `a_trapped_kernel_goes_silent_alone` (replaces `a_trapped_kernel_is_reset_at_the_next_activation`) | `core/runtime/src/tests/live/keep.rs` | `t:trap` on track a, a sine through a delay on track b: after the trap a's node is 0, b's output is a run with no trap's bit for bit, the song is b exactly, the transport plays, `crashes()` is 0, the mark is `Crashed` (none without the trap) | `g-live-fails` (live fails as strict did: "its track is silent") |
| `a_stopped_kernel_starts_again_at_the_next_activation` | same | an edit on track b: the stopped kernel is reset as it moves (`resets` 1) and plays again; the new plan has no mark | `g-stop-unpoison` (a stopped box's poison cleared: `resets` 0) |
| `stop_all_sound_starts_a_stopped_kernel_again` | same | a panic: it plays again, its mark goes; a second trap marks it again and `news()` tells it again, once | `g-panic-keeps-stopped` |
| `a_trap_on_any_thread_count_gives_the_same_bits` | same | the trap on 1 thread and on 4 (spread): the same output, the same mark | — |
| `a_trap_still_fails_an_export_and_names_the_node` | same | strict: the render fails, "a/lat crashed" | `g-strict-swallow` |
| `a_kernel_that_writes_its_input_fails` (changed) | `tests/cell.rs` | live, the kernel that broke ABI-1 stops alone, marked; the render goes on | — |
| `a_plugin_that_fails_live_goes_silent_alone` (changed) | `keep/plugins.rs` | `!cur.broken()` gone; the rest unchanged | — |

### Checks

`~/m2-runs/s8a/g-check.sh m2-runtime m2-play m2-server` (a copy of `s2-check.sh` with the lane's paths), load
6.0 at the start: renders 10 of 10 OK; tests all pass (runtime 174 + 1 ignored, server 235, play's suites);
clippy clean, Linux and the Mac lint. Log: `~/m2-runs/logs/g1-check-1017-181044-1523245.log`. Its first run
failed only its rustfmt step, on lines the tree had before (`rustfmt` follows a file's modules, and the tree
isn't rustfmt-clean). The script now checks only new files, as `s2-check.sh` does.

## G2: a kernel over its 32 MB says it ran out of memory

### In short

- **Before:** a kernel that asked for more than its 32 MiB got -1 from `memory.grow`. Its own code then trapped,
  and the message said "wasm trap: wasm `unreachable` instruction executed". Nothing said "memory".
- **Now:** the grow traps at once, and the error is `OutOfMemory`: "bass/delay ran out of memory: a sound may use
  up to 32 MB". Live, the node stops alone, marked as out of memory (G1's path, `Why::OutOfMemory`). Offline the
  render fails in those words. At a build (its init or first reset), the build fails in them.
- **The app's own "out of memory" words don't catch it** (A6, D24). Those say the computer ran out and tell the user
  to close other apps. A sound over its own cap isn't that. `out_of_memory` in `graph.rs` now passes it by.
- **No std kernel comes near the cap** (`every_std_kernel_fits_its_memory_at_the_highest_rate`), so no render
  changes: 10 of 10.

### What was built

| Where | What |
|---|---|
| `core/runtime/src/lib.rs` | `Cap`, the store's data: a `ResourceLimiter` (one instance, memory and table; a memory grow past `KERNEL_MEMORY` sets `refused` and traps). `ran_out`: an error from a call into a kernel becomes `OutOfMemory` when `refused` is set. Every call into a kernel goes through it: instantiate, `init`, `ports`, `port`, `process`, `reset`, `active` |
| `core/runtime/src/stopped.rs` | `OutOfMemory { node }`, its words; `Why::of` reads it (`downcast_ref`, through any context) |
| `crates/server/src/graph.rs` | `out_of_memory`: false when the chain holds an `OutOfMemory` (one line, D24) |

The test kernel of `a_kernel_memory_never_moves` grew a page at every call, past the cap, and relied on the -1. It
stops at the cap now (`memory.size` < 512), and still proves the memory never moves while it grows to it.

### Tests

| Test | Where | Proves | Red by |
|---|---|---|---|
| `a_kernel_over_its_memory_says_it_ran_out` | `keep.rs` (`t:hog`: grows 600 pages, 37.5 MiB, while its input is over 0.5, and traps on -1) | strict: exactly "a/lat ran out of memory: a sound may use up to 32 MB", an `OutOfMemory`; live: the node is silent, the other track the same bits as with no grow, the mark `OutOfMemory` | `g-oom-today` (the limiter as before: -1, no mark: "a/lat crashed: … wasm trap: wasm `unreachable` instruction executed"); `g-oom-unnamed` (the trap but no mark) |
| `a_kernel_over_its_memory_at_its_build_says_so` | same (`t:fat`: its reset grows 600 pages) | the build fails in the same words | `g-oom-today` (the build succeeded); `g-oom-unnamed` |
| `a_sound_over_its_memory_is_not_the_computer_running_out` | `crates/server/src/graph/tests.rs` | `plainly` leaves it in its own words; `unbuilt` says "couldn't get it ready to play", with them as the detail, and never "computer" | `g-oom-computer` (D24's line taken out: "the computer ran out of memory for it. Close other apps…") |
| `a_kernel_memory_never_moves` (changed) | `core/runtime/src/tests.rs` | as before, up to the cap | — |

### Checks

`g-check.sh m2-runtime m2-play m2-server m2-editor`, load 6.6 at the start: renders 10 of 10 OK; tests all pass
(runtime 176 + 1 ignored, server 236, editor 284, play's suites); clippy clean, Linux and the Mac lint. Log:
`/home/jerry/m2-runs/logs/g2-check-1017-181854-1584955.log`.
