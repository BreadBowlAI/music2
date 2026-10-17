# 8a's live half, the gaps lane: build notes

What the gaps lane built, how it was tested, and where the logs are. The plan is
`~/m2-runs/s8a/plan-live.md` §5 (its Attack and Decisions win: D24, D27). The lane ran beside slice 3 on its own
branch (`8a-gaps`, worktree `~/m2-runs/wt/8a-g`, from `8a` at `98620e7`), with its own target
(`~/m2-runs/8a-scratch/target-gaps`) and scratch HOME (`~/m2-runs/8a-scratch/g-home`).

## In short

- **A sound that breaks no longer stops the song.** Only that sound goes silent; the rest plays on, bit for bit,
  on any number of threads. Before, the whole song went silent and stopped, with no word.
- **The app says so, once:** which sound, on which track, that the rest plays on, and that Transport ▸ Stop All
  Sound (⌘.) starts it again. A sound over its 32 MB says it ran out of memory.
- **Exports don't change:** a sound that breaks still fails the export, naming it. No render record changed.
- **`pl-mix` no longer flakes** on the hats' short hits.
- Every new test was shown red first, by a small bug or by the code before this lane.

| # | Commit | State | What it says |
|---|---|---|---|
| G1 | `9ebc8d4` | done | **Only the sound that crashed goes silent.** The plan marks it; the next activation or Stop All Sound resets it. |
| G2 | `d77164e` | done | **"bass/delay ran out of memory: a sound may use up to 32 MB"**, live, at a build and in an export. Before: "wasm trap: unreachable". The app no longer tells the user to close other apps for it. |
| G3 | `6e1d29e` | done | **A toast names the sound and its track**, says the rest plays on and what starts it again. Once for each sound. A browser suite plays a real crash in the app. |
| G4 | `e66e6cd` | done | **`pl-mix` reads until a hat shows.** 10 runs of 10 pass; the old check would have failed 7 of 10. |
| G5 | this commit | done | RENDER-25's words and *Today*, `docs/gaps.md`, these notes. |

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

## G3: the app says which sound stopped, on which track, and what brings it back

### In short

- **What the user sees** (a toast, the server's notices, as for a plugin that fails, HOST-5):

  | Why | The words |
  |---|---|
  | It crashed | "The instrument on Hats stopped working, so music2 made it silent. The rest of the song plays on. To start it again, choose Transport ▸ Stop All Sound (⌘.)." |
  | Over its 32 MB | "The delay on Bass ran out of memory (a sound may use up to 32 MB), so music2 made it silent. The rest of the song plays on. To start it again, choose Transport ▸ Stop All Sound (⌘.)." |

- **What it is:** a kernel by its name (`std:delay`: "The delay"), a `poly` as "The instrument", anything else by its
  id. **Where:** a track by its label ("on Bass"), the song's own module "in the song".
- **Once for each sound in a plan.** A new plan starts with no marks, so the same sound can be told again. Stop All
  Sound clears a mark (G1), so a sound that fails again after it is told again.
- **No automatic restart** (the plan's Q20): a kernel's trap comes from its input, so the same input traps again.
  The user decides, with Stop All Sound or an edit.

### What was built

| Where | What |
|---|---|
| `crates/server/src/graph/stopped.rs` (new) | `heard(inner, open)`: the plan sent last's `Stopped::news()`, each told once, into the notices. `said`, `sound_named` |
| `crates/server/src/graph/actor.rs` | one line beside `heard_plugins()`: `super::stopped::heard(&actor.inner, actor.open.as_ref())`. It runs before every job; the watcher's catch-up makes one every 300 ms |
| `crates/server/src/graph.rs` | `mod stopped;` (one line) |
| `e2e/sound-stops.mjs`, `e2e/trapwasm.mjs` (new) | the browser suite and its wasm patch (below); a row each in `e2e/run.mjs` and `e2e/README.md` |

Left as it is: the notices read the plan **sent** last. In the few milliseconds before the engine takes a new plan, a
sound that stops in the old one is never told. The old plan leaves at once, so nobody hears it for long.

### A real crash in the app (the browser suite)

std's kernels don't trap. So the suite makes one that does: `trapwasm.mjs` adds a function to a copy of
`std/kernels/std.wasm` (no locals, `unreachable`) and points the `noise_process` export at it. The suite runs its
own headless server with `M2_STD` on that copy, on a copy of first-song: only Hats' instrument uses `noise`. So
the whole chain runs for real: the trap on the audio thread, the mark, the actor's look, the notice, the toast.

| # | Check |
|---|---|
| 1 | the copy of std is a valid module whose noise kernel traps at once (node's own `WebAssembly`) |
| 2, 3, 4 | first-song opens on it; nothing is said before Play; Space plays |
| 5 | the toast's words, exactly |
| 6 | all of them on screen, none cut off |
| 7, 8 | the song plays on; Kick's header meter moves while Hats' stays at the floor (12 reads) |
| 9 | said once: the toast isn't replaced by another |
| 10, 11 | after it goes, ⌘. (Stop All Sound): the instrument starts again, stops again, and the toast comes again |
| 12 | no console errors |

### Tests

| Test | Where | Proves | Red by |
|---|---|---|---|
| `a_kernel_that_stops_is_told_once_by_sound_and_track` | `crates/server/src/graph/stopped_tests.rs` (new) | a song of Bass (a saw through a delay), Hats (std's hat) and std's mixer, playing on a driven player; the test marks nodes of the plan sent last, as the audio thread would: "The delay on Bass…", once; "The gain in the song…" and "The instrument on Hats…", in node order; an edit that makes a new plan (another oscillator: a value alone is a cell write) tells the delay again; the song plays on | `g-no-heard` (the actor's line out: nothing told); `g-told-twice` (`news()` never marks a node told: the delay told again with the next) |
| `out_of_memory_is_told_as_memory` | same | kind 2's words | `g-kind` (kind 2 told as a crash); `g-no-heard` |
| `sound-stops` (12 checks) | `e2e/sound-stops.mjs` | above | the page and server of `98620e7`, before this lane (below) |

The suite red first, on the server of `98620e7` (before this lane, built in a throwaway worktree, its page the
same), load 12.5: 6 of 12 fail. Nothing is said, the song stops (`playing` false), and every meter falls to the
floor: the old "whole song silent" gap. Log: `~/m2-runs/s8a/g-red/sound-stops-base.log`. Then on this lane's
server, 5 runs of 5 pass (load 8.7 to 4.8; `~/m2-runs/logs/g-stops-5-*.log`), about 7 s each. A picture of the
toast: `~/m2-runs/s8a/g-red/sound-stops-toast.png`.

### Checks

`g-check.sh m2-runtime m2-server m2-play`, load 14.6 at the start: renders 10 of 10 OK; tests all pass (runtime
176 + 1 ignored, server 238, play's suites); clippy clean, Linux and the Mac lint; the new files formatted. Log:
`~/m2-runs/logs/g3-check-1017-183107-1689097.log`. The run before it failed clippy only
(`cloned_ref_to_slice_refs` in the new test), fixed.

## G4: pl-mix waits for a hat before it reads the meter past the fader

### In short

- **The flake:** `pl-mix` read Hats' strip meter and header meter 10 times, 150 ms apart, and needed one read over
  −24 dB with the fader at −30 dB. first-song's hats are 45 ms hits, which the meter shows for a frame or two.
  So all ten reads could land between hits.
- **Measured:** with the plan's fix (150 ms apart, on until one is loud, 40 at most), 10 runs of 10 passed, but the
  first loud read came at read 10, 10, 10, 12, 12, 12, 12, 15, 25 and **37**. So the old 10 reads would have
  failed in 7 of 10 runs, and 40 was too close. Log: `~/m2-runs/logs/g-mix-10-1017-183443-1721006.log`.
- **Built:** reads 50 ms apart, at least 10, on until one is loud, 12 s at most. Every read must still match the
  header's within 1.5 dB. The same for the check before the fader. The words print how many reads, the first ten
  and the loud ones.
- **10 runs of 10 pass**, load 2.7 at the start: the first loud read at read 10 to 33 (1.7 s at most). Log:
  `~/m2-runs/logs/g-mix-10b-1017-183830-1746598.log` (the red run, then the 10).

### Tests

| Test | Where | Proves | Red by |
|---|---|---|---|
| `pl-mix` (47 checks, as before) | `e2e/pl-mix.mjs` | above | `g-meter-post`: the strip meter reads the song's output (`channels={[OUT]}` in `Mix.tsx`'s `Strip`, past every fader): both meter checks fail, every read a mismatch ("-2/-60 … -1/-60"); the clip dot checks too. The page rebuilt after |

`e2e/README.md`'s `pl-mix` row says the new reads.

## G5: the contract and gaps.md

| Where | What |
|---|---|
| `docs/architecture.md`, RENDER-25 | its last sentence: "Live, a node that fails (a kernel's trap, a voice's, a kernel that asks for more than its memory) goes silent alone and stays silent until the next activation or a panic resets it; the rest of the plan plays on, and the app names the sound and its track. Offline the render fails, naming the node." *Today*: how, where, the words, and why a kernel isn't started again on its own, unlike a plugin (HOST-5) |
| `docs/gaps.md` | "What works today" gains: keep playing when one sound breaks |
| `HANDOFF.md` | not touched: the track takes the two gap lines out of "Left for later" when it merges this lane |

## For the lead

| # | What | Why it matters |
|---|---|---|
| 1 | **Files outside the plan's list.** `core/runtime/src/gauge.rs` (`run_graph` and `run_part` take the `Stopped`; `pre` gives a block, so two `let Some(b)` lines go). `render_block_on`: its `let Some(b) = self.pre(..)` line became `let b = ..`, the line after the runner lines, which the plan gives slice 3. `core/runtime/src/tests/cell.rs` (one test read `broken()`), `core/runtime/src/tests.rs` (the memory test's kernel). `crates/server/src/graph.rs`: D24's line and `mod stopped;`. `crates/server/src/graph/tests.rs`: one test after `running_out_of_memory_is_said_plainly`. `e2e/run.mjs` and `e2e/README.md`: one row each, after `pl-projects` | merge: a conflict, if any, is one line apart |
| 2 | **The words say Stop All Sound, not "your next change to the code, or Undo".** A value change (a knob, a number) is a cell write: no new plan, so the sound would stay silent. A panic resets every instance (RENDER-10), so it now clears `stopped` too: one action that always works | the plan's Q20 holds: no automatic restart |
| 3 | **A toast lasts 4 s**, and this one is three lines. Hovering holds it. For a dyslexic owner that may be short; plugin notices have the same 4 s. A longer time for long server notices is a `web/src/lib/store.ts` and `Toasts.tsx` change, outside this lane | an owner decision, or a small polish item |
| 4 | The notices read the plan **sent** last. In the milliseconds before the engine takes a new plan, a sound that stops in the old one isn't told | rare and short; the old plan leaves at once |
| 5 | A voice that fails stops the whole `poly`, every voice ("The instrument on Hats") | by design: one box |
| 6 | `Engine::crashes()` now counts only an error that reaches the engine live, which can't happen today. Only tests read it | could go later |
| 7 | `HANDOFF.md`'s two gap lines (Left for later, step 6) can go at the merge | the track's job |
| 8 | Disk: the lane's targets (`target-gaps`, and `target-gaps-base` for the suite's red run) are deleted at the end; the throwaway worktree `wt/8a-g-base` too | L7 |
