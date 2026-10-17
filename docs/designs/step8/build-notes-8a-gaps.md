# 8a's live half, the gaps lane: build notes

What the gaps lane built, how it was tested, and where the logs are. The plan is
`~/m2-runs/s8a/plan-live.md` §5 (its Attack and Decisions win: D24, D27). The lane ran beside slice 3 on its own
branch (`8a-gaps`, worktree `~/m2-runs/wt/8a-g`, from `8a` at `98620e7`), with its own target
(`~/m2-runs/8a-scratch/target-gaps`) and scratch HOME (`~/m2-runs/8a-scratch/g-home`).

## In short

| Gap | State | What it says |
|---|---|---|
| G1: a sound that crashes while the song plays | done | **Only that sound goes silent.** The rest plays on, bit for bit. Before, the whole song stopped. |

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
