# 8a: build notes

What each slice of 8a did, what it measured, and where the logs are. The offline half's plan is `8a-plan.md`
beside this file (written as `~/m2-runs/s8a/plan.md`); the live half's is `~/m2-runs/s8a/plan-live.md`. Each
plan's Attack and Decisions win. The design is `design-8a.md`. Linux timings here are information only: only the
owner's Mac judges timings (PLAT-1).

## In short

| Slice | State | What it says |
|---|---|---|
| 0: the spike (thrown away) | done | **Go.** p50 on 4 threads is 2.75 to 2.99 times faster than step 7's loop by the mean, 3.3 to 4.2 times by the p99. The rule needs 2.5 and 2.0. Same bits on every thread count. |
| 1: the pool (`m2-pool`) | done | **Built and green**, nothing uses it yet. 17 unit tests, a grep test and 5 models (34,000 cases), about 4 s in all. Four models and 12 unit tests shown red by mutants of the pool. M5 (orderings too weak) stays green on x86, as planned: the Mac can catch it. Renders unchanged. |
| 2: the executor, offline | done | **Built and green.** Every offline render runs on many threads with the same bits; live playback through the same executor on one. On this server p50 renders 2.86 times faster on 4 threads by the mean, 3.41 by the p99; the executor costs nothing seen on one thread. |
| The review | done | **4 findings, all fixed**, each with a test shown red before its fix. The last check found a fifth: gate G10's timing passed only on its retry. Fixed too. No render changed. |
| The final check | passed | `ci/check.sh --gates` at `bc41693`: **every row, nothing passed only on a retry** (1,241 Rust tests, 49/49 browser suites, 10 of 10 renders, the 6 gate runs). |
| The Mac sweep | owed | four lines for the owner, once `8a` is on `main` (below). |
| R1: a mixer strip runs with its track | done | **Built and green, the same bits.** p50's song partition went from 154 nodes to 52, its time from about 300 to about 50 µs a block, the critical path from about 400 to about 160 µs; 4 threads about 15% faster (Linux, a busy server). |
| 3: live playback on engine threads | done | **Built and green.** Live playback renders on a pool of engine threads with the same bits as one thread: every pure fixture on 2, 4 and 7 threads, edits and a resampled song. Real time and the Mac's workgroup are asked for; this server refuses real time, so a device here plays on one thread. Nothing allocates. Every test shown red. |

## Slice 0: the spike

### In short

- **Go, by the plan's rule.** On CCD1 (the 32 MB cache), 4 threads render a p50 block 2.75 to 2.99 times faster
  than step 7's loop by the mean (S4), and 3.3 to 4.2 times by the p99 (P4). The rule asks for 2.5 and 2.0.
- **The bits never changed.** Every run on 2, 3, 4, 6, 8 and 16 threads, FIFO or sticky, on either die, paced or
  not, gave the same hash as step 7's loop.
- **What holds it back is the song's tail.** The mixer runs after every track: 226 µs a block on one thread. That is
  37% of the 4-thread block and 53% of the 8-thread block. 51 `std:gain` nodes are 211 µs of it.
- **More threads don't make more work.** On one die, the time spent in tasks is the same on 1, 4 and 8 threads
  (about 1,700 µs a block in all). So the gap to 4 times is the tail, not the cache.
- **FIFO stays.** Sticky placement was within 1 to 2% of FIFO, not the 5% the rule asks for.
- **Exports alone use one cache's cores** (`Cores.per_cache`, 8 here). 16 threads over both dies were no faster
  than 8 on one, and 8 over both dies were 9 to 12% slower.
- **Paced blocks are slower.** At 4 threads, a block every 2.67 ms takes 12% more by the mean and 24% more by the
  p99 than blocks back to back. The Mac run decides what to do about it.
- **The executor costs nothing seen on one thread:** −17 to +10 µs a block against step 7's loop, inside the
  noise of the same loop against itself (±30 µs).

### What was built, and where

| What | Where |
|---|---|
| Base | `main` at `d7207d2`, branch `8a-spike`, worktree `~/m2-runs/wt/8a-spike`. So the numbers are on the base the plan's Decision 1 asks for: 8b.3's gain on every mixer channel is in |
| Target | `/home/jerry/m2-runs/8a-scratch/spike-target`, `m2-gates` in release |
| Code (kept for reference, never merged) | `~/m2-runs/s8a/spike/spike.diff`: `core/runtime/src/spike.rs`, `crates/gates/src/spike.rs`, `m2-gates spike` |
| How every run was started | `~/m2-runs/s8a/spike/run.sh` (one step a log) |
| Logs | `~/m2-runs/s8a/spike/<step>-<time>.log` |
| Afterwards | the worktree, its target and the branch deleted |

The spike, in short:

- **Partitions:** one per module, from a fresh plan's `graph.order` (overlays included). A plan node goes with its
  module. An overlay goes with the module of the node it reads. Edges come from what each built node reads (a quick
  `reads()`). The spike checks the partition graph for a cycle first, and checks every read is earlier in its own
  partition or in a producer with a direct edge.
- **p50's shape:** 51 partitions (50 tracks and the song), 520 nodes. 50 roots, one sink (the song). No cycle.
- **Threads that live for the whole run.** N − 1 workers, each set up for wasmtime once. Per block the caller opens
  a job, wakes parked workers, works too, waits for the last task, closes the job, and waits until no worker is
  inside it.
- **FIFO:** one ready queue, roots by bottom level, a consumer pushed when its last input finishes. **Sticky:**
  root k's home is thread k mod N; a thread takes its own roots, then the queue, then other threads' roots.
- **The run:** p50 from 3:00 (`Transport::starting`, as G7f), 1 s not timed, then 30 s timed (11,250 blocks). Live
  mode (non-finite samples silenced), as G7f.
- **Each run hashes its output** and compares it with step 7's loop (`Graph::process`, run "old").
- Per thread: time in tasks, minor faults, involuntary switches and time waiting for a CPU, from `/proc`.
- **Turns** (`--turns`): every thread count as a side of `paired::in_turns`, 2 s turns, the first 188 blocks of each
  turn not judged. Each turn's mean and p99 against the old loop's in the same cycle, and the median over 15 turns.
  What disturbs one turn is left out. Added because other tracks loaded the server (below).

### The machine and its load

The CPU, read from sysfs: CCD0 is `0-7,16-23` (96 MB L3, the V-cache die, a lower clock), CCD1 is `8-15,24-31`
(32 MB L3). CPU n and n + 16 share a core. The main runs used `taskset -c 8-15`: CCD1's 8 cores, siblings left out.

**Hardware counters: none.** `perf` isn't installed and `perf_event_paranoid` is 4. CCD0 against CCD1 stands in
for cache size; faults, switches and CPU waits stand in for the rest.

**Other tracks loaded the server in bursts** while these ran: a server test binary on 24 cores, headless Chrome
end-to-end runs, `rustc`. The 1-minute load went from 2.3 to 28. Before each step the runner waited for a 5-second
sample under 4 busy CPUs and a 1-minute load under 8 (that load also counts this job's own spinning threads from
the run before). Some steps waited, up to 6 minutes; none reached the 10-minute limit. Bursts still came mid-run.
So each run also prints two numbers the plan's 2% CPU-wait rule can't see:

- **others on the mask:** CPUs busy on CCD1 beyond this process, on average;
- **siblings busy:** the SMT siblings (24-31), which share our cores.

A run is called **disturbed** below when those two add up to more than 1.5 CPUs, or a thread waited for a CPU more
than 2% of its time ("contended"). Disturbed runs were run again (B1, D, E, A3). The numbers below are undisturbed
unless marked.

### Run G: the honest baseline (old loop against the spike's executor, 1 thread, in turns)

| Run | Old loop: mean, p99 (µs) | Executor, 1 thread | Old loop again (A/A) | Executor − old, median of 15 turns | A/A |
|---|---|---|---|---|---|
| G1 (11:10, load 5 to 11) | 1,910, 4,389 | 1,861, 3,925 | 1,862, 3,933 | +10 µs (−686 to +137) | +3 µs |
| G2 (11:34, load 7 to 9) | 1,758, 3,442 | 1,740, 3,571 | 1,735, 3,468 | −17 µs (−159 to +43) | −31 µs |

Same bits on all three sides, both times. The executor's own cost on one thread can't be told from noise.

### Run A: scaling on CCD1 (FIFO, back to back)

Undisturbed back-to-back runs, µs a block (mean / p99). "Turns" is T1 at 11:37, the quietest run of all (CCD1
busy 2.9 CPUs with this job's own, siblings 0.3): each count against the old loop turn by turn.

| Threads | Back to back (mean / p99) | Runs | T1: mean / p99 | T1: speed-up by mean / by p99 (range over turns) |
|---|---|---|---|---|
| old loop | 1,673 / 2,736; 1,752 / 3,175; 1,926 / 4,475 | A3, E0, A1 | 1,847 / 4,596 | 1 |
| 1 | 1,746 / 3,366; 1,773 / 3,576 | E0, A3 | 1,736 / 3,299 | 1.01 (0.91 to 1.42) / 1.04 |
| 2 | 1,020 / 1,704 | A2 | 985 / 1,730 | 1.78 (1.49 to 2.54) / 2.19 |
| 3 | 809 / 1,399 | A2 | 726 / 1,142 | 2.39 (2.31 to 3.36) / 3.01 |
| 4 | 609 / 882; 639 / 1,046; 678 / 1,170 | E0, A1, A2 | 608 / 912 | **2.88** (2.72 to 4.01) / **3.70** |
| 6 | 504 / 739; 524 / 838 | A1, A2 | 481 / 662 | 3.62 (3.52 to 5.01) / 4.84 |
| 8 | 442 / 613; 452 / 671 | A1, A2 | 448 / 646 | 4.00 (3.17 to 5.65) / 5.26 |

A1's 1, 2 and 3 threads, A2's 1 thread and old loop, and A3's 2 to 8 threads were disturbed (2.0 to 13 CPUs of
others); they are in the logs and not used. E0 is A's configuration, run again at a quiet moment. The first T1
(11:13, a busier server) read 3.46 and 4.50 at 4 threads.

Per thread, undisturbed: minor faults 0 to 0.05 a block, involuntary switches under 0.06 a block, CPU wait under
1.2% (most under 0.1%). Time in tasks summed over the threads: 1,740 µs a block on 1 thread, 1,697 on 4 (E0),
1,741 on 8 (A1). **The work doesn't grow with the thread count on one die.**

### The go rule

S4 = the old loop's mean (G) ÷ the better 4-thread mean (A or B), on CCD1. P4 = the same with the p99.

| G | 4 threads | S4 | P4 |
|---|---|---|---|
| G1: 1,910 / 4,389 | A1: 639 / 1,046 | 2.99 | 4.20 |
| G2: 1,758 / 3,442 | A1: 639 / 1,046 | **2.75** | **3.29** |
| G2: 1,758 / 3,442 | E0: 609 / 882 | 2.88 | 3.90 |
| T1, paired turn by turn | | 2.88 | 3.70 |

Every reading is above 2.5 and 2.0: **go. Slices 1 and 2 as planned.**

### Run F: where the time goes (1 thread, CCD1, F3 at 11:37, undisturbed)

| What | Mean | p99 | Max |
|---|---|---|---|
| The block (1 thread) | 1,736 µs | 3,191 | 6,803 |
| Pre-pass (transport, lanes, `Block`) | 2.5 µs | | |
| Post-pass (feedback rings, output) | 0.9 µs | | |
| Work in 51 partitions | 1,727 µs | | |
| The song (the one sink, 154 nodes) | **226 µs** | 264 | 628 |
| The slowest track partitions (8 nodes each) | 48 µs | 95 to 98 | up to 432 |
| The fastest track partitions | 21 µs | | |
| Critical path (slowest track, then the song) | 274 µs | | |
| Bound at 2 / 4 / 8 threads (work ÷ n, or the critical path) | 863 / 432 / 274 µs | | |

No partition's p99 is near 667 µs (a quarter of a block), the trigger for voices across threads.

**The song's tail, by node kind** (F2: a run like F3 with each node timed, 1,747 µs a block):

| Node kind | Nodes | µs a block | Each |
|---|---|---|---|
| `std:gain` (each channel's `db`, and the master's) | 51 | **210.6** | 4.13 |
| `std:add` (`sum`'s left fold) | 49 | 15.6 | 0.32 |
| constants | 52 | 1.1 | 0.02 |
| `std:limit`, `std:stereo` | 2 | 1.0 | |

The whole graph, by kind: `poly` 50 nodes, 1,265 µs (25 each, voices included); `std:gain` 211; `std:drive`
17 nodes, 137; `std:reverb` 16, 69; `std:delay` 17, 19; overlays 50, 2.1.

**What the tail does to scaling.** A block on N threads is close to (work − tail) ÷ N + tail: (1,727 − 226) ÷ 4 +
226 = 601 µs on 4 threads (measured 608), 414 µs on 8 (measured 442 to 448). The tail is 37% of the 4-thread block
(E0: 228 of 609 µs) and 53% of the 8-thread block. That is far above the 15% the plan names for R1's
"a mixer strip joins the track it reads". The rule asks for that fix only in the 2.0 to 2.5 band, so it is **not**
part of slice 2. But it is cheap, and it is where the next speed-up is: with each channel's gain in its track's
partition, the tail would be about 21 µs (the 49 adds, the master), and 4 threads about 450 µs (S4 about 3.9;
*estimate*). The lead decides.

Also seen: one `std:gain` costs 13 times one `std:add`. 51 of them are 12% of a whole 1-thread block. That is std's
business (a std bump), not 8a's.

### Run B: sticky against FIFO (CCD1)

| Threads | FIFO, back to back | Sticky, back to back | T2 (in turns, busy server): FIFO / sticky, turn medians |
|---|---|---|---|
| 2 | 1,020 / 1,704 | 1,123 / 2,652 (B1, lightly disturbed) | 1,029 / 1,047 |
| 4 | 609 to 678 / 882 to 1,170 | 644 / 1,071 (B2); 665 / 1,274 (B1, lightly disturbed) | 626 / 619 by the mean; 994 / 990 by the p99 |
| 8 | 442 to 452 / 613 to 671 | 447 / 675 (B2); 482 / 794 (B1) | 441 / 449 |

Sticky is 1% faster at 4 threads at best, and 2% slower at 8. The rule asks for 5%: **FIFO.** The first B1 run was
contended (a build started mid-run, load 28) and was run again; both are in the logs.

### Run C: CCD0, the 96 MB cache

| Threads | CCD0, back to back (C) | CCD0, in turns (T3): mean, speed-up | CCD1 (A, E0, T1) |
|---|---|---|---|
| old loop | 1,754 / 2,670 | 1,732 | 1,673 to 1,847 / 2,736 to 4,596 |
| 1 | 1,741 / 2,447 | 1,724, 1.00 | 1,736 to 1,773 / 3,299 to 3,576 |
| 4 | 626 / 804 | 624, 2.78 | 608 to 639 / 882 to 1,046 |
| 8 | 451 / 634 | 447, 3.87 | 442 to 452 / 613 to 671 |

Quiet, the means are the same on both dies. The bigger cache gives a tighter p99 on 1 to 4 threads (2 to 40% lower,
by run), and the same on 8; not a faster mean. The step 7 numbers that read "+10 to 40% on a smaller cache" were
contention, as the refresh's Attack guessed. The V-cache die also boosts to a lower clock, so this isn't a clean
test of cache size alone.

### Run D: both dies (`0-15`), for the export count

| Threads | Both dies (D, run again quiet) | One die, CCD1 (A) |
|---|---|---|
| 8 | 496 / 740 (first D: 493) | 442 to 452 |
| 16 | 460 / 611 (first D: 739, contended) | — |

T4 (in turns, busy server): 4 threads 642, 8 528, 16 477 (turn medians) against T1's 8 on one die, 437.

By the plan's rule: 16 over both dies are slower than 8 on one, and 8 over both dies are 9 to 12% slower than 8
on one. **Exports alone use `Cores.per_cache`** (8 on this server). It's close: 16 over both dies are within 5%.

### Run E: paced, a block every 2,667 µs (CCD1)

| Threads | Paced (E, second run) | Back to back, minutes later (E0) | Paced over back to back |
|---|---|---|---|
| 1 | 2,013 / 7,904 (others 0.8 CPUs: lightly disturbed) | 1,746 / 3,366 | +15% mean, +135% p99 |
| 4 | 686 / 1,096 | 609 / 882 | **+12.5% mean, +24% p99** |

The first E (busier) read 2,441 / 7,463 and 712 / 1,255 against E0's first 1,878 / 3,942 and 608 / 897: the same
picture. In the paced 4-thread run every worker parks once a block, and its time in tasks rises from 424 to 483 µs
(+14%): caches and clocks go cold in the gaps, besides the wake-up itself. No 4-thread paced block went over the
period. On the Mac, the plan moves "spin between callbacks" into slice 3 if paced 1-thread p99 is more than 15%
above back to back; Linux says it likely will be.

### What this means for slices 1 and 2

| Decision | From | Built with |
|---|---|---|
| Go | S4 2.75 to 2.99, P4 3.29 to 4.20 | slices 1 and 2 as planned |
| Placement | B, T2 | FIFO (`pop` as planned; no sticky) |
| Exports alone | D, T4 | `Threads::Alone` counts `Cores.per_cache` (8 here, 16 cores in all) |
| R1, a mixer strip joins its track | F: tail 37% of the 4-thread block | not required by the rule; the lead decides (it would add about 1 to S4, *estimate*) |
| Voices across threads | F: largest partition p99 264 µs | not triggered |
| The executor's own cost | G: −17 to +10 µs, noise ±30 | within the 40 µs budget on Linux; the slice-2 gate measures it properly |

### Logs

All in `~/m2-runs/s8a/spike/`, with `run.sh` (each step's command) and `spike.diff` (the code).

| Step | Logs (time started, 2026-10-17) |
|---|---|
| A | `A1-1017-105753.log`, `A2-1017-105932.log`, `A3-1017-113202.log` (mostly disturbed), `E0-1017-112643.log`, `E0-1017-113054.log` |
| B | `B1-1017-110114.log` (contended), `B1-1017-111841.log`, `B2-1017-110246.log`, `T2-1017-111600.log` |
| C | `C-1017-112058.log`, `T3-1017-112157.log` |
| D | `D-1017-112312.log` (16 contended), `D-1017-112910.log`, `T4-1017-112353.log` |
| E | `E-1017-112505.log`, `E-1017-112948.log` |
| F | `F-1017-110933.log` (disturbed), `F2-1017-112754.log` (by node), `F3-1017-113705.log` |
| G | `G-1017-111028.log`, `G-1017-113413.log` |
| Turns | `T1-1017-111252.log` (busy), `T1-1017-113731.log` (quiet) |

## Slice 1: the pool

### In short

- **The pool is built:** `core/pool`, the crate `m2-pool`, with no dependencies. Nothing uses it yet. Slice 2's
  executor will.
- **What it does:** it runs a graph of numbered tasks on the calling thread and a few workers. Each task runs once,
  after every task it reads. A failed task skips what reads it; the rest still runs.
- **All green:** 17 unit tests, a grep test and 5 models. About 4 s from an empty target.
- **Shown red:** four of the five models, and 12 unit tests, each by a mutant of the pool (a small deliberate bug).
  The fifth model has no mutant of its own. The one mutant that stays green is M5, the orderings made too weak. The
  plan expected that: x86 keeps stores in order, so only an arm64 Mac can catch it. The Mac's check runs the same
  row.
- **Nothing else changed:** 10 of 10 renders match `e2e/renders.sha256`, and `no_alloc` passes.
- **Main was merged into 8a first** (the plan's Decision 1), as `acf15f2`.

### Commits

| Commit | What |
|---|---|
| `acf15f2` | Merge `main` (`d7207d2`) into `8a` |
| `f17e777` | The pool (`m2-pool`), its unit tests and the grep test. Tested alone: the staged tree, exported to a scratch folder |
| `cac8284` | The models, and their row in `ci/check.sh` |
| this one | These notes |

### What was built

| File | What |
|---|---|
| `core/pool/src/lib.rs` | "In short"; `Setup`, `Pool`, `Runner`, `Counters`, `Spread`; the models' hooks |
| `core/pool/src/tasks.rs` | `Tasks::new`: sorts the edges, counts inputs, finds a topological order (or a `Cycle`), bottom levels, roots and consumers (most critical first), the width. Sizes the run state |
| `core/pool/src/run.rs` | the block path: `wake`, `run`, `rest`, a run on many threads, `help`, `pop`, `complete`, the workers' loop, sleeping, the abort guard, and the run state's own steps |
| `core/pool/src/sync.rs` | std's atomics and `park`; with `--features models`, the same with a chaos point before each step |
| `core/pool/src/tests.rs` | 17 unit tests |
| `core/pool/tests/no_blocking.rs` | the grep test over `run.rs` |
| `core/pool/tests/models.rs` | the 5 models |
| `Cargo.toml`, `Cargo.lock` | the crate; `opt-level = 3` in the dev build (it will run on the audio thread). The lock gained only the `m2-pool` entry |
| `ci/check.sh` | row 6b, "pool models" |

### How a run works

1. `Tasks::new` is built once, off the audio thread. It holds the run state too, so a run allocates nothing.
2. `run` resets the state and queues the roots. With workers, and a graph wider than 1 (or spread on), it publishes
   the run, wakes up to `width − 1` sleeping workers, and works too.
3. Every thread takes the next ready task. When a task finishes, each consumer whose last input it was is queued.
4. The caller stays until every task is done. Then it closes the run and waits until no worker is inside it.
5. A worker leaves a run when nothing it could take is left. It spins a moment (`patience`, 50 µs offline), then
   parks. Only a waker that takes its `parked` flag signals it, so a worker that slept through many wakes wakes once.

### Where it differs from the plan

| Plan | Built | Why |
|---|---|---|
| `run(&mut self, tasks: &Tasks, …)` | `tasks: &mut Tasks` | The run state lives in `Tasks`. `&mut` stops two runs of one graph at once, and lets the reset be plain writes. Slice 2's task closure can still borrow the schedule's other fields beside it |
| `Tasks::new(…) -> Tasks` | `-> Result<Tasks, Cycle>` in every build; `Cycle` names the tasks on one circle | Decision 21 |
| `wake` at the block line | `run` also wakes, after it publishes the run | A sleeper sets its flag, then looks for a run; a waker publishes, then takes the flag. A wake *before* the publish can miss a worker on its way to sleep. M3 shows the hang |
| `Runner::counters` | also `Pool::counters`, readable during a run | Decision 8 |
| models: 2,000 cases | 34,000 in all, 4,000 to 10,000 a model | They take 2.3 s. More cases are more chances on the Mac |
| `run.rs` holds the block path | it also holds the run state's steps (reset, outcomes, panic slots, the spread count) | So the grep test sees the whole block path |
| — | `Tasks::roots()` and `is_empty()` | for tests and slice 2 |
| sticky placement, if chosen | not built | The spike chose FIFO |

The Decisions on the pool are all in: the abort guard only with workers, and `Setup.start` inside it (20, 10);
spread waits stop when the pool is dropped (10); the spread spin comes before each pop (9); one pool at a time in a
test binary, 60 s watchdogs that only catch hangs (11).

### Tests

No test has a time bound. Each waits on counters or events, under a 60 s watchdog.

| Test | Proves | Shown red by |
|---|---|---|
| `every_task_runs_once_after_its_inputs` | 500 random graphs (2 to 40 tasks) on `Runner::one()` and 1 to 8 threads, spread off and on: each task once, after its inputs; with spread, tasks ran on workers | M1 |
| `a_failed_task_skips_what_reads_it_and_the_rest_run` | random failures on 0 to 7 threads: the outcomes equal a serial reckoning | M4 |
| `a_panicking_task_finishes_the_run_then_is_raised` | two tasks panic, one on a worker: the rest ran or were skipped; `run` returns task 3's payload; the pool works after | U3 (the test binary aborts) |
| `a_panic_in_pool_code_aborts` | a child process with a panic in `complete`: it aborts | U4 (the child hangs: the watchdog fails it) |
| `on_one_thread_a_panic_in_pool_code_unwinds` | on `Runner::one()` the same panic unwinds (Decision 20) | — |
| `idle_workers_sleep_while_a_task_blocks` | while one task blocks, every other worker parks | U5 (hang) |
| `rest_sends_workers_to_sleep_at_once` | `patience` of an hour; after `rest()` every worker parks | U6 (hang) |
| `a_worker_held_off_wakes_once` | 1,000 wakes while a worker is busy: no worker gets more signals than its parks + 1 | U7 |
| `a_chain_wakes_nobody` | a chain (width 1) signals no worker and runs on the caller | U8 |
| `a_runner_outlives_its_pool` | after the pool is dropped, the runner runs everything itself, spread on | U9 (hang) |
| `dropping_a_pool_joins_every_worker` | after the drop, no worker is alive | U10 |
| `spread_puts_every_root_on_a_worker` | with spread, no root runs on the caller | U11 |
| `the_same_graph_builds_the_same_tasks` | edges shuffled and repeated give the same tasks | U12 |
| `the_most_critical_go_first_and_width_is_the_widest_layer`, `a_cycle_is_refused_and_named`, `a_pool_has_1_to_64_threads`, `an_empty_graph_runs_at_once` | the build's rules | — |
| `the_block_path_never_blocks` (grep) | `run.rs` has no allocation, lock, channel, thread-local or I/O | a `Vec::with_capacity` put in `pop` |

### The models and their mutants

The models run with a random yield or short spin before every atomic step and every park. Each mutant is a small
deliberate bug, made in a scratch copy of the crate.

| Mutant | The bug | Caught by | How |
|---|---|---|---|
| M1 | a consumer is queued at its first input's end | `each_task_once_after_its_inputs` (and the unit test) | case 0: 3 tasks ran before their inputs |
| M2 | `run` returns without waiting for workers to leave | `nothing_touches_a_job_after_run_returns` | the binary crashed (SIGSEGV), twice: a worker read the run from a stack `run` had already left. That is the use-after-free the rule prevents. The review found the model's own check could never fire; since `1be5e31` it fails on its own message (The review) |
| M3 | a worker parks without looking again after setting its flag | `no_lost_wake` | hung, 4 runs out of 4: at cases 273, 699, 1,142 and 1,159 of 4,000 |
| M4 | no skip for a failed task's consumers | `skips_are_exactly_the_descendants` (and the unit test) | case 1: task 5 Failed, not Skipped |
| M5 | the queue slots and input counts made `Relaxed` | — | **passed**, as the plan expected: x86 keeps stores in order. The Mac's check runs the same row on arm64 |

`back_to_back_runs_with_rest_between` has no mutant of its own. It runs 5,000 cases in 20 pools, waking and resting
at random between runs.

### Numbers (Linux, information only: PLAT-1)

Load (1 minute) 1.9 when timed.

| What | Time |
|---|---|
| The "pool models" row from an empty target (build, 17 + 5 + 1 tests, the models build's lint) | 4.5 s, 11 MB |
| The 5 models alone (34,000 cases) | 2.3 s |
| The unit tests | 1.1 s |

### Checks

- `cargo test --locked -p m2-pool`: 17 and 1 green. In release with `--features models`: 17, 5 and 1 green.
- `cargo clippy --locked -p m2-pool --all-targets`, with and without `models`: no warnings.
- `f17e777` tested alone, before the models were added.
- The row's shell code run with `check.sh`'s own helpers: a pass, and a made-up failure that "what failed" names.
- Renders: 10 of 10 match `e2e/renders.sha256` with the debug `m2`. `no_alloc`: 9 of 9.
- `Cargo.lock`: one build without `--locked` (`cargo check --offline -p m2-pool`, Decision 28) added only the
  `m2-pool` entry. Every other build was `--locked`.
- Scratch targets deleted afterwards. `df -h /`: 2.4 TB free.

### Logs

| What | Where |
|---|---|
| The mutants: each one's change (`.diff`), its run (`.log`), `results.txt`, `run.sh`, `mutate.py` | `~/m2-runs/s8a/mutants/` |
| The first run of M1 to M5, against the models at 2,000 cases (same verdicts) | `~/m2-runs/s8a/mutants/models-draft-2000-cases/` |
| Debug build; renders, `no_alloc`, tests and lints | `~/m2-runs/logs/8a-s1-build-1017-114455-2806073.log`, `8a-s1-checks-1017-120750-2997791.log` |
| The mutant runs | `~/m2-runs/logs/8a-s1-mutants-1017-120427-2972920.log`, `8a-s1-mutants2-1017-120957-3012654.log`, `8a-s1-m3x3-1017-121122-3020767.log` |

### For slice 2

- `Schedule::build` turns a `Cycle` into a failed prepare (Decision 21).
- The task closure calls `fpenv::canon()` itself. `Setup.start` does wasmtime's per-thread set-up and
  `background_thread()`. The pool knows nothing of them.

## Slice 2: the executor offline

### In short

- **Every offline render runs on many threads, with the same bits.** `m2 render`, `m2 bounce` and the app's
  Export Audio split the song into partitions (one per track, and the song) and run them on a pool of threads.
  Each pure fixture renders the same bits on 1, 2, 3, 4 and 7 threads; so do 30 s of p16 on 4, and 30 s of p50
  on 8.
- **Live playback still renders on one thread**, through the same code (`Runner::one()`: no thread, no heap).
  Every live test, `no_alloc` included, is unchanged.
- **How many threads:** `--threads N`, else `M2_THREADS`, else one cache's fast cores when nothing else plays
  (8 on this server, the spike's choice), and the slow cores (at least one) beside playback. A song with
  plugins renders on one thread for now.
- **One floating-point mode on every thread that renders** (RENDER-2a, new): subnormals kept, set at each
  block, each partition and after each plugin call. A plugin that leaves flush-to-zero set changes no bit.
- **The new gate, `m2-gates realtime --offline`,** times p50 on 1 to 8 threads and judges only the bits. On
  this server (information only, PLAT-1), held on CCD1: 4 threads render a p50 block 2.86 times faster than one
  by the mean, 3.41 times by the p99; 8 threads 3.91 and 4.68. The spike read 2.88 and 3.70 at 4. On one thread
  the executor costs nothing seen against step 7's loop: −9 µs a block (pairs from −72 to +55).
- **Found:** std 0.2's `svf`, `delay` and `follow` let subnormal numbers through in a long tail (OS-49). A
  runtime test names them; a long delay's block is 1.7 times slower 5 minutes after its note. That is std's to
  fix, not 8a's.
- **Main was merged** (polish's end, `a660c2b`) before the last check. `ci/check.sh --gates` passed at
  `33885a6`: every row, 1,239 Rust tests, 10 of 10 renders. One browser suite of polish's, `pl-mix`, passed only
  on its retry: not 8a's (below).

### Commits

| Commit | What |
|---|---|
| `5d19a37` | m2-os: `process::cores()`, `thread::background()`, `process::load_average()`, `process::waited()` |
| `0fd1b20` | Blocks render through partitions, on one thread: `Exec::reads`, labels, `parts.rs`, `view.rs`, `step()`, `render_block_on`; one schedule for a plan's whole life |
| `2dc89ce` | One floating-point mode on every thread that renders (RENDER-2a): `fpenv.rs`, the probe's `sets-fz` tag, two tests |
| `8d5dabe` | Offline renders on many threads give the same bits: `process_on`, `render_on`, `seek_on`, `Engine::process_on`, `threads.rs`, the 1-vs-N tests |
| `2bfc709` | rustfmt over the tests above (no test changed) |
| `e4588be` | `m2 render --threads`, `M2_THREADS`, `Threads` in `Render`, `lock::taken`, the export's choice |
| `d3c3e26` | `m2-gates realtime --offline`; the block split into pre-pass, graph and post-pass; `gauge.rs`; the OS-49 test |
| `bfe5873` | Contract: RENDER-2, RENDER-2a (new), RENDER-28, RENDER-32, gate 8's row; design-8a's as-built note |
| `3b0f504` | Merge `main` (`a660c2b`, polish's end) into `8a`. No conflict |
| `33885a6` | The grep test over `view.rs` and `fpenv.rs` |
| this one | These notes, and HANDOFF |

### What was built

| File | What |
|---|---|
| `core/runtime/src/parts.rs` | `Schedule` (a `m2_pool::Tasks` and `Parts`): one partition per module; where modules read each other both ways (Tarjan over modules), nodes split by module crossings in plan order; edges from `Exec::reads`; costs (kernel 2, plugin 40, a poly its voices' kernels, fades, overlays and copies 1); each partition's error with its place in plan order. Debug builds check every read is earlier in its partition or in one it waits for |
| `core/runtime/src/view.rs` | `Reads` (a node's reads), `Local` (a voice graph's), `GraphView` (raw pointers to the graph's per-node vectors; only one element ever borrowed), `Own`, `Reader` (debug builds check every read against the schedule). A compile-time check that `Exec: Send` |
| `core/runtime/src/fpenv.rs` | `canon()`, `is_canon()`, `Canon` (enter the one mode; the caller's back on drop, unwinding too); `asm!` on x86-64 and arm64, `compile_error!` elsewhere |
| `core/runtime/src/threads.rs` | `setup(background)`: each worker's start (wasmtime's thread state, the one mode, the background priority if asked); `background_refused()` |
| `core/runtime/src/gauge.rs` | gates only: `gauge_block` (a block's passes timed; its graph as the partitions, one partition, or step 7's loop), `profile_block` (each partition alone, timed), `parts()`, `width()`, `last_block()` |
| `core/runtime/src/lib.rs` | `Exec::reads`; `step()` (the old loop's body, for every graph); `Graph::process` over `step()` (voices, and the gates' old loop); `render_block_on` = pre-pass, `run_graph`, post-pass; `labels()`; `process_on`, `render_on`, `seek_on`; `canon()` after plugin calls and resets; `panic()` inside `Canon` |
| `core/runtime/src/plan.rs` | `order_after` gone; `end_fade` makes ghosts and the window's own overlays `Const(0.0)` in place |
| `core/runtime/src/engine.rs` | `Engine::process_on` |
| `core/os` | `process::Cores`, `cores()` (Linux: `linux/cores.rs`, sysfs inside the affinity mask and the cgroup quota; Mac: `hw.perflevel*`), `thread::background()`, `load_average()`, `waited()` |
| `core/plugins/src/probe.rs` | the `sets-fz` state tag: flush-to-zero set at the end of every `process` |
| `core/session/src/lock.rs` | `taken(dir)`: whether a writer holds the project (never makes `.music2/`) |
| `crates/editor/src/render.rs` | `Threads` (`Alone`, `Beside`, `Exactly`), `from_env`, `parse`, `count`; `threads_for`; `Render::new`, `lending`, `range` take a `Threads`; `Rendered.threads`, `hosts_plugins` |
| `apps/m2/src/main.rs` | `--threads` for `render` and `bounce`; the choice; "on N threads" in the output |
| `crates/server` | `export::export_threads`; the export and the reference render pass a choice |
| `crates/gates/src/realtime.rs` | `realtime --offline` |
| `docs/architecture.md`, `design-8a.md` | the contract edits; the as-built note |

### How a block runs now

1. **Pre-pass**, on the calling thread: the transport, cell values, lanes, the trace, the block's times. The
   workers are woken first, so they wake while it runs.
2. **The graph:** each partition is one task. A task runs its nodes in plan order. It waits for the partitions it
   reads. The pool runs ready tasks on the caller and its workers. A failed node stops its partition; what reads
   it is skipped; the rest runs. A panic is caught and raised again after the block.
3. **Post-pass**, on the calling thread: feedback rings, the output, the meters. On an error it writes nothing
   and returns the error first in plan order, as step 7's loop did.

A song with plugins runs every task on the caller, whatever runner it is given.

### Where it differs from the plan

| Plan | Built | Why |
|---|---|---|
| `Renderer::time_phases(bool)`, `last_phases()`, `profile_parts(blocks)`, `one_partition(bool)` | `gauge_block(clock, runner, How)` (`Parts`, `One`, `Old`), `profile_block`, `parts`, `width`, `last_block` | Nothing on the block path: the gates call their own block, made of the same pre-pass, graph and post-pass |
| `#[cfg(test)] Schedule::from_cuts` | `Schedule::cut`, not test-only | The gates' one partition is the order with no cut |
| `m2_os::rt` / `lock::held` | `thread::background`, `process::cores` (Decision 38); `lock::taken` | `lock::held` is already WRITE-18's message |
| `Threads::Alone` counts `fast` | counts `per_cache` | the spike's run D. On the M1 Pro `per_cache` is `hw.perflevel0.cpusperl2` (4, *estimate*): see "For the lead" |
| `export_threads(pure, idle, lending)` | `export_threads(env, idle, lending)` | `M2_THREADS` passed in keeps it pure. A song with plugins needs no flag: `Render` picks one thread for it (Decision 42) |
| `Render::lending` | takes a `Threads` too | one rule for every render; it always hosts plugins, so one thread |
| The OS-49 test passes at 0 subnormals | it names the kernels that make them: `std:delay`, `std:follow`, `std:svf` | They do. The test fails when a std bump fixes them, and then shrinks to none |
| G10's range render | on one thread | its records are step 7's, on one thread |
| `render_takes_threads_and_says_how_many` in `apps/m2/tests/render.rs` | `apps/m2/tests/threads.rs` | `render.rs`'s one test sets the environment, safely only alone |
| `p50_…` on 4 threads | on 8 | the export count here |
| §5.4's `p50-sends` | not built | Decision 32 |

Labels as built: a plan node its module; a ghost its old module; an overlay the module of the node it reads; a fade
the module of the node that reads it (a feedback doesn't count, Decision 23); a fade nobody reads (the output's)
its new side's, else its old side's; anything left, a label of its own (`""`).

Known limits, for slice 3 (Decision 41 and the spike):
- An export's thread count is chosen as it starts: Play pressed during an `Alone` export leaves it on its cores.
- Plugins render on one thread; a plugin song's plan never wakes a worker.

### Tests

Every test below was shown red once by a small deliberate bug (a mutant), its change and run kept in
`~/m2-runs/s8a/s2-red/` (`<name>.diff`, `<name>.log`; `mutate.py` puts the file back afterwards).

| Test | Where | Proves | Red by |
|---|---|---|---|
| `a_track_is_one_partition`, `modules_that_read_each_other_split_by_depth`, `every_edge_goes_forward` (500 random graphs), `the_schedule_is_the_same_for_the_same_plan`, `the_most_critical_partitions_go_first`, `a_chain_has_width_one` | `core/runtime/src/tests/parts.rs` | the build | `rt-depth` (no depth split: 2 tests), `rt-cost` (no costs), `rt-edges` (no edges: all 11) |
| `ghosts_fades_and_overlays_take_their_labels` | same | each label as §5.1 says, in a window with a ghost, fades, a fade-in and the window's own overlays | `rt-labels` |
| `reads_lists_every_read` | same | a voice capture, a missing plugin and kernels read across tracks; debug builds check every read | `rt-reads` (a poly's captures left out of `reads()`) |
| `a_failing_node_stops_only_what_reads_it` | same | the other track runs that block | `rt-old-loop` (step 7's loop) |
| `strict_names_the_first_failing_node_on_any_thread_count` | same | the error first in plan order, on 1, 4 and 7 threads, though the other is taken first | `rt-first-error` |
| `random_convex_partitions_give_the_same_bits` | same | 200 random songs with a feedback, any order cut into any runs, 64 blocks on 1, 4 and 7 threads | `rt-edges` |
| `cross_module_feedback_captures_and_sidechains_on_n_threads` | same | a capture, a sidechain and a feedback across tracks on 4 and 7 threads | (the same edges) |
| `a_song_with_a_plugin_runs_no_task_on_a_worker` | same | Decision 22 | `ed-plugins` shows the editor's side |
| `an_edit_window_renders_the_same_bits_on_1_and_n_threads` | `core/runtime/src/tests/live/keep.rs` | an edit's window on 4 threads: 5 ghosts, fades waiting for fade-ins, a kept poly, a ghost lane, a muted track deleted while muted, two tracks that read each other only in the window | `rt-fade-reads` |
| `ghosts_and_window_overlays_go_quiet_after_the_window` | same | `Const(0.0)` after the window; only the ghosts left the audio thread | `rt-end-fade` |
| `a_flush_to_zero_set_by_the_caller_changes_no_bit` | `core/runtime/src/tests/fpenv.rs` | a thread that flushes renders the same bits, and flushes again after | `fz-no-enter`, `fz-no-canon` |
| `a_flush_to_zero_left_by_a_plugin_changes_no_bit` | `core/play/tests/bit_identity.rs` | the probe's `sets-fz` | `fz-plugin` |
| `canon_enters_the_one_mode_and_leaves_the_threads_own` | `fpenv.rs` | `Canon` | — |
| `which_std_kernels_make_subnormals_in_a_five_minute_tail` | `tests/fpenv.rs` | OS-49 counted | `os49-found` (expecting none: the three kernels) |
| `the_executor_view_and_fpenv_never_block` | `core/runtime/tests/no_blocking.rs` | grep: no allocation, lock, channel, thread-local or I/O | `rt-no-blocking` (a `vec!` in `view.rs`) |
| `every_pure_fixture_renders_the_same_bits_on_1_and_n_threads` | `bit_identity.rs` | 5 fixtures whole on 2, 3, 4, 7 threads, spread; tasks on workers in each; reads across threads | `play-no-edges` (in release, where the debug check is off: first-song on 3 threads differs at frame 384) |
| `p16_renders_the_same_bits_on_1_and_4_threads`; `p50_…_1_and_8_threads` (`#[ignore]`) | same | 30 s each | — |
| `cores_are_read_from_sysfs`, `the_affinity_mask_and_quota_cut_them`, `cores_here_are_sane` | `core/os/src/linux/cores.rs` | a fake sysfs shaped like this CPU | `os-cores` |
| `a_background_thread_is_nice_10`, `waits_for_a_cpu_are_read`, `the_load_average_is_three_numbers` | `core/os` | | `os-nice` |
| `a_background_thread_has_background_qos`, `cores_on_this_mac_are_sane` | `core/os/src/mac/mod.rs` | the Mac's own (its check runs them) | — |
| `whether_the_lock_is_taken` | `core/session/src/lock.rs` | | `lock-taken` |
| `a_render_on_4_threads_writes_the_same_file`, `a_song_with_plugins_renders_on_one_thread`, `m2_threads_is_read_and_nonsense_refused` | `crates/editor/src/render.rs` | | `ed-threads`, `ed-plugins`, `ed-parse` |
| `render_takes_threads_and_says_how_many` | `apps/m2/tests/threads.rs` | `--threads 1` and `4` the same file; `M2_THREADS`; the flag wins; nonsense refused | `cli-flag` |
| `an_export_picks_its_threads` | `crates/server/src/export.rs` | §5.3's table | `srv-lending` |
| `the_sweep_fails_when_a_count_renders_other_bits`, `a_sweep_list_is_read_and_nonsense_refused`, `the_help_says_what_realtime_offline_measures` | `crates/gates` | | `gate-bits`, `gate-sweep`, `gate-help` |

Every existing test passes unchanged: the live engine now renders through the executor on one thread (`no_alloc`:
9 of 9; `live.rs`, `mute.rs`, the audition, overlay and tap tests).

Time (debug, Linux): `bit_identity` 35 to 38 s with the new tests (load 6 to 24); the OS-49 test 10 s; the
runtime's lib tests 10 to 12 s.

### Numbers (Linux, information only: PLAT-1)

The machine: AMD Ryzen 9 7950X3D. CCD0 is CPUs `0-7,16-23` (96 MB L3), CCD1 `8-15,24-31` (32 MB L3). The gate's
`cores:` line read "16 fast (8 per cache), 0 slow" not held, and "8 fast (8 per cache)" held on `8-15` (the
affinity mask counts). Both runs at `33885a6`, release, the server quiet after the last check.

**The sweep** (`m2-gates realtime --offline --reference 50`): p50 from 3:00, 1 s untimed, then 20 s (7,500
blocks) back to back, each count twice, the second pass in reverse order; medians of the two passes. µs a block.

| Threads | Held on CCD1 (`taskset -c 8-15`; load 1.2): mean / p50 / p99 / max | by mean, by p99 | Not held (load 1.8) | by mean, by p99 | tasks on workers |
|---|---|---|---|---|---|
| 1 | 1,754 / 1,714 / 3,059 / 4,927 | 1, 1 | 1,787 / 1,737 / 3,455 / 5,960 | 1, 1 | 0% |
| 2 | 979 / 962 / 1,589 / 2,417 | 1.79, 1.92 | 995 / 967 / 1,729 / 2,527 | 1.80, 2.00 | 48% |
| 3 | 732 / 725 / 1,107 / 1,515 | 2.40, 2.76 | 748 / 731 / 1,212 / 1,859 | 2.39, 2.85 | 64% |
| 4 | 614 / 607 / 898 / 2,533 | **2.86, 3.41** | 620 / 610 / 944 / 1,284 | 2.88, 3.66 | 73% |
| 6 | 496 / 491 / 712 / 1,432 | 3.54, 4.30 | 538 / 540 / 826 / 1,583 | 3.32, 4.18 | 82% |
| 8 | 449 / 446 / 653 / 838 | 3.91, 4.68 | 512 / 499 / 777 / 1,263 | 3.49, 4.45 | 86 to 87% |
| 16 | — | — | 499 / 490 / 655 / 1,223 | 3.58, 5.27 | 93% |

- **The same bits on every count**, both runs (`81dd175f739e5d71` over 1,920,000 samples); the pure fixtures through
  `Render` on 8 threads (held) and 16 (not held) match `e2e/renders.sha256`, 10 of 10.
- **The passes:** the pre-pass grows with the threads it wakes, 2.6 µs on 1 thread to 7.9 on 8 and 16.6 on 16;
  the post-pass is 0.9 to 1.1 µs.
- **Waits for a CPU:** under 3 µs a block, but 45.6 at 8 threads held on 8 CPUs (the gate's own threads share
  them) and 11.8 at 16 not held.
- **Across dies, as the spike found:** not held, 8 threads were 14% slower than 8 held on one die (512 against
  449), and 16 no faster than 8 on one die by the mean (499).

**Paced**, a block every 2,667 µs, the workers sent to sleep after each (as live will):

| Threads | Held: mean / p99 / max, blocks over the period | Not held |
|---|---|---|
| 1 | 1,807 / 3,170 / 5,738; 275 of 7,500 | 1,860 / 3,732 / 5,416; 466 |
| 4 | 685 / 1,114 / 3,042; 1 | 730 / 1,141 / 1,515; 0 |

Against back to back, held: one thread +3% by the mean and +4% by the p99; four threads +12% and +24%, as the
spike's run E (+12.5% and +24%). So on Linux the 1-thread paced p99 is within 15% of back to back here (the spike's
was disturbed), and 4 threads pay for waking each block. The Mac decides.

**Partitions** (each alone, 2,000 blocks): 51 partitions, 50 at once at most; the work 1,789 µs a block; the critical
path 279 µs; the bound on 2, 4 and 8 threads 895, 447 and 279 µs. The slowest: the song, 228 µs (p99 272); each
track 49 to 51 µs (p99 95 to 98). The song's tail is 13% of the work, 37% of the 4-thread block (228 of 614). No
partition's p99 is near 667 µs.

**The executor's own cost on one thread**, in turns over the same music (12 pairs, by the mean):

| | Held | Not held |
|---|---|---|
| step 7's loop | 1,753 µs a block | 1,775 |
| the partitions, over it | −9 µs (−72 to +55) | +13 (−65 to +147) |
| one partition, over it | −10 (−50 to +65) | −33 (−91 to +41) |
| the partitions over one partition | +6 (−63 to +55) | +28 (−36 to +171) |

Within the noise both times, and under the 40 µs budget.

**G7f in the last check** (not held): the app against S, by the mean, +63 µs a block (bound 80) and +192 µs a
callback (bound 320); step 7's Linux check read +20 and +86 at `ffc36aa`, −25 and −166 at `e108e46`. Both runs of
the gate above put the executor itself at about 0 against step 7's loop, in the same binary over the same music, so
the rise isn't the executor's. Where it comes from wasn't looked into: main has changed much since `e108e46` (8b.3's
51 mixer gains among it), and one G7f run moves by tens of µs. Linux numbers aren't judged (PLAT-1): the Mac's next
G7f run decides.

**Tails (OS-49):** a long delay's block takes 1.8 µs 1 s after its note and 3.0 µs 5 min after: 1.7 times, in both
runs. A long reverb's stays the same (1.02 to 1.06). The runtime test counts subnormal samples instead: std 0.2's
`svf` (84 million over 5 minutes in its three outputs), `delay` (18.8 million) and `follow` (13.9 million) make them;
none of the other stateful kernels does. A std bump that flushes their state is the fix (std's, deferred).

**p50 on 8 threads, bit for bit** (`p50_renders_the_same_bits_on_1_and_8_threads`, `#[ignore]`, release): 30 s, the
same bits, 28.6 s.

### Logs

| What | Where |
|---|---|
| The red runs: each mutant's change and run | `~/m2-runs/s8a/s2-red/` (`<name>.diff`, `<name>.log`, `mutate.py`) |
| Each commit's check (renders, tests, clippy, the Mac lint) | `~/m2-runs/logs/8a-s2-c{2,3,4,5,6}-1017-*.log`; the script `~/m2-runs/s8a/s2-check.sh` |
| The gate's first run (p16, a short sweep) | `~/m2-runs/logs/8a-gate-smoke-1017-133112-3616369.log` |
| The gate, held on CCD1 and not held | `~/m2-runs/logs/8a-offline-ccd1-1017-141710-3902468.log`, `8a-offline-free-1017-142032-3918668.log` (copies in `~/m2-runs/s8a/s2-gate/`) |
| p50 on 8 threads | `~/m2-runs/logs/8a-p50-bits-1017-142400-3935706.log` |
| The last check | `~/m2-runs/logs/check-8a-1017-134212-3696229.log`; its logs `~/m2-runs/check-8a-33885a6.tgz` |

### For the lead

- **Exports alone on the Mac.** `Threads::Alone` counts `Cores.per_cache`, as the spike chose here. On an M1 Pro
  that is `hw.perflevel0.cpusperl2`, 4 (*estimate*), not its 8 P-cores. The Mac sweep's "8 against 4" line decides:
  if 8 beat 4 by 10% or more there, `Alone` should count `fast` on a Mac (`m2-os` could report it as a field).
- **The song's tail is 37% of the 4-thread block.** R1 ("a mixer strip joins the track it reads") would move the
  51 gains into their tracks: about +1 to the speed-up at 4 threads (*estimate*, the spike's).
- **OS-49 is std's:** `svf`, `delay` and `follow` let subnormals through; a long delay's block is 1.7 times slower
  5 minutes into its tail on x86.
- **`pl-mix`** (polish's suite) failed once and passed on its retry: "its strip meter still reads as the header's:
  before the fader", all ten samples at the floor. Its check needs one of ten samples, 150 ms apart, above −24 dB
  on first-song's hats, which are short hits: in polish's own checks most samples read the floor too
  (`~/m2-runs/check-polish-2`, `-3`). A suite to make sturdier, not an 8a change.

## The review

### In short

- **A review of 8a's offline half confirmed 4 findings.** All 4 are fixed, each in its own commit with a test that
  failed before the fix and passes after.
- **The last check found one more:** gate G10's timing passed only on its retry. That is fixed too.
- **No render changed.** 10 of 10 renders match `e2e/renders.sha256`. Live playback is still on one thread.
- **Then the final check passed** at `bc41693`: every row, nothing passed only on a retry (below).

### The findings

| Finding | Severity | What was wrong | The fix | Commit |
|---|---|---|---|---|
| det-2 | low | Outside a block, some kernel and plugin calls ran without the one floating-point mode (RENDER-2a). A poisoned plugin's reset at an activation left an audio device's thread in the one mode for good. A kernel's reset, the idle check, a seek and a build computed in the calling thread's own mode | `Canon::enter()` at the top of `Renderer::take_over`, `can_idle`, `seek_on` and `build`, as `panic` had. RENDER-2a and `fpenv.rs` say so | `7a1d527` |
| pool-model-seal-moved | medium | The model `nothing_touches_a_job_after_run_returns` sealed a moved copy of the run state. A late worker read the old address, so the model's own check could never fail: a pool that returned early only crashed | The caller owns each case's tasks, so they never move. `Case::run` seals them the moment the run returns. Every case checks for breaches | `1be5e31` |
| bounce-place-sees-own-lock | low | `m2 bounce --place` took the project lock, then chose its threads. `lock::taken` saw its own lock as the app's, so it rendered beside playback: one thread here | The threads are chosen before the lock. The bounce line now says how many threads it used, as `m2 render`'s does | `aba8744` |
| alone-over-64-falls-to-one | low | With more than 64 cores behind one cache, the default asked for more threads than a pool has. `Pool::new` refused, and every default render quietly ran on one thread | `Threads::count` stops at `Pool::MAX_THREADS` (64), the one limit `--threads` and `M2_THREADS` use | `de3da0f` |
| G10 in the check | — | `the_live_engine_passes_every_gate` passed only on its retry: G10 compares one wall-clock timing of a range render from bar 64 with one from bar 1, within 10%. Load from other work fails it on its own | Each render is timed on a CPU (the wall clock less the time the process waited for one, where Linux says), the best of three of each, in turns | `bc41693` |

### Red before, green after

| Finding | Test | Before the fix | After |
|---|---|---|---|
| det-2 | `between_blocks_kernels_and_plugins_compute_in_the_one_mode` (`core/runtime/src/tests/live/keep.rs`), with a new hand-written kernel `t:tiny` that keeps a subnormal | each of the four guards taken out alone fails at its own line: the build, the activation, the idle check, the seek. All four out: fails at the build | passes |
| pool-model-seal-moved | `nothing_touches_a_job_after_run_returns`, against slice 1's mutant M2 (`run` returns without waiting for workers) | SIGSEGV, 3 runs of 3: never the model's own message | "case 1: the pool touched a run after it returned", 5 runs of 5. M1, M3 and M4 still caught; the pool unmutated green |
| bounce-place-sees-own-lock | `a_bounce_that_places_its_clip_isnt_beside_itself` (`apps/m2/tests/threads.rs`) | placed: 1 thread, not placed: 8 | the same count |
| alone-over-64-falls-to-one | `m2_threads_is_read_and_nonsense_refused` (`crates/editor/src/render.rs`), now with 72 cores behind one cache | 72 and 80 threads, which `Pool::new` refuses | 64 and 64 |
| G10 | `m2-gates --rig engine --quick range`, the debug build, before and after in turns | 2 of 8 runs failed on the server at load 6 to 22 (bar 64: 1.70 s against 0.95). Pinned to CPU 31 beside bursts of a busy loop there (0.3 to 1.5 s on, then off): 5 of 10 failed, then 2 of 10 | quiet: 10 of 10. Beside the same bursts: 10 of 10, bar 64 within 2% of bar 1 (0.81 to 0.87 s each). The best of three by the wall clock alone wasn't enough: 4 of 10 failed beside the bursts |

Each commit's own check (`~/m2-runs/s8a/s2-check.sh`: the debug `m2`, 10 of 10 renders, the crates' tests, clippy
on Linux and for the Mac) passed before it was made.

### Logs

| What | Where |
|---|---|
| det-2: the test, its red runs (`det2.py`), the commit's check | `~/m2-runs/logs/8a-r1-det2-*.log`, `8a-r1-det2-red-*.log`, `8a-r1-det2-check-*.log` |
| The model: M2 before and after, the other mutants (`run.sh`, a copy of slice 1's) | `~/m2-runs/s8a/review-red/` (`M2-before1..3.log`, `M2-after1..5.log`, `M1-after.log`, `M3-after.log`, `M4-after.log`, `results.txt`); `~/m2-runs/logs/8a-r2-*.log` |
| The bounce: red and green (`bounce.py`), the commit's check | `~/m2-runs/logs/8a-r3-*.log` |
| The thread count: red and green (`count.py`), the commit's check | `~/m2-runs/logs/8a-r4-*.log` |
| G10: before and after, quiet and beside bursts; the commit's check | `~/m2-runs/logs/8a-g10-*.log`, `8a-r5-g10-check-*.log` |
| The first final check (G10 passed only on its retry) | `~/m2-runs/logs/check-8a-fg-1017-151507-229683.log`; its logs `~/m2-runs/check-8a-fg-run1.tgz` |

## The final check

`ci/check.sh --gates` from `~/m2-runs/wt/8a` at `bc41693`, with `M2_CHECK_OUT=~/m2-runs/check-8a-fg`, 33 minutes,
load 4.0 at the start and 1.9 at the end: **PASS, every row, nothing passed only on a retry.** `main` (`70f9b61`) was
already merged in (`f0dc7ab`): it hasn't moved since.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS |
| app | PASS: the UI connected, first-song in 1 s, with-plugins in 1 s |
| rust tests | PASS: 1,241 passed, 0 failed |
| clippy, std kernels, std.wasm, mac lint | PASS (std.wasm rebuilt to the same bytes in another folder) |
| pool models | PASS: 23 passed, 0 failed |
| browser suites | PASS: 49/49 |
| renders | PASS: 10 of 10 match `e2e/renders.sha256` |
| old engine (S), gates | PASS: the 6 runs `ci/gates.txt` names |
| repo unchanged | PASS |

G7f (not judged here, PLAT-1): the app against S, by the mean, +47 µs a block (bound 80) and +78 µs a callback
(bound 320); the slowest activation block 3,493 µs (bound 5,333).

The first final check, at `de3da0f` (15:15, load 22 at the start), passed every row, but
`the_live_engine_passes_every_gate` passed only on its retry: G10, "bar 64 takes no longer than the same rendered
length from bar 1: 1.19 s against 0.85 s". That is the fifth fix above (`bc41693`); then the check ran again.

| What | Where |
|---|---|
| The final check | `~/m2-runs/logs/check-8a-fg-1017-161554-749344.log`; its logs `~/m2-runs/check-8a-fg/` (`check-8a-fg.tgz` beside it) |
| The first one | `~/m2-runs/logs/check-8a-fg-1017-151507-229683.log`; its logs `~/m2-runs/check-8a-fg-run1.tgz` |
| The check before the review, at `33885a6` (every row; polish's `pl-mix` passed on its retry) | `~/m2-runs/logs/check-8a-1017-134212-3696229.log`; its logs `~/m2-runs/check-8a-33885a6.tgz` |

## R1: a mixer strip runs with its track

### In short

- **The song's serial tail is now the sum and the master.** Each channel's pan and fader runs in its track's
  partition. On p50 the song partition went from 154 nodes to 52 (49 adds, the master's stereo, gain and
  limiter), and its time from 253 to 335 µs a block to 34 to 59. The critical path fell from 327 to 456 µs to
  133 to 196, and 4 threads got about 15% faster by the mean (Linux, a busy server: information).
- **Two small rules, no bit changed.** A node whose inputs all come from one other module, constants aside,
  runs with that module. A constant or a notes lane does nothing in a block, so it joins no partition, and
  reading it waits for nothing. 10 of 10 renders match; every 1-vs-N test passes.
- **Shown red:** the four new checks, each by a small deliberate bug.

### What was built (`c8ce2bc`)

| File | What |
|---|---|
| `core/runtime/src/lib.rs` | `Exec::idle()` (a `Const` or `Notes` node); `labels()` takes the order and walks it: a plan node whose running inputs all have one other label takes it; an overlay follows its source again. A debug assert: no node that reads a fade moves (Decision D3) |
| `core/runtime/src/parts.rs` | rule A in `Schedule::of` and `build` (D1): idle nodes get `NONE`, join no group, make no edge, and a label holding only constants makes no partition. The debug check names a running node with no partition |
| `core/runtime/src/gauge.rs` | `Part.nodes` is the list of nodes, not their count (tests read which nodes the song keeps) |
| `docs/architecture.md` | RENDER-2's words, and its *Today* |

What stays in `song`: each `std:add` of the sum (it reads two tracks), the master's chain, a ducker that reads
two tracks and what reads it (first-song's bass channel), and, in an edit's window, a strip that reads a fade.

One behaviour to know: a node that reads only another track moves whole, an instrument included. A pad whose
voices only capture the kick would run in the kick's partition. That is speed only, and no reference song does
it; `every_read`'s pad now also captures a tremolo of its own, so its voices still read the kick across
partitions (`reads_lists_every_read`).

### Tests

| Test | Where | Proves | Red by |
|---|---|---|---|
| `a_mixer_strip_runs_with_its_track` | `core/runtime/src/tests/parts.rs` | first-song's mixer, written as the compiler writes it: each fader and pan with its track, a follower of the kick with the kick, the ducker and the master in the song, constants in none; the same bits on 4 threads as 1 | `r1-no-strips` (rule B out: `song/kg` in `song`), `r1-const-parts` |
| `constants_run_in_no_partition` | same | a song's constant read by a track makes no edge, so no depth split; written constants and a notes lane in no partition | `r1-const-parts` (rule A out: three partitions, not two) |
| `the_song_tail_is_the_sum_and_the_master` | `core/play/tests/bit_identity.rs` | p16's song partition holds exactly 15 adds, the master's gain, limiter and stereo; only the master's fader is in the song | `r1-no-strips-p16` (17 faders in the song) |
| `whole()`, the shared check | `core/runtime/src/tests/parts.rs` | every running node of the order in one partition, idle ones in none (D2) | `r1-whole` (source kernels left out: "node 1 is in one partition if it runs") |
| `every_edge_goes_forward` | same | now with constants among 500 random graphs | — |

Every existing test passes, the 1-vs-N ones included. `a_track_is_one_partition` and the window's label check
accept "its module, or the one other module all its inputs come from".

### Numbers (Linux, information only: PLAT-1)

`m2-gates realtime --offline --reference 50 --sweep 1,4,8`, release, held on CCD1 (`taskset -c 8-15`). The
server was busy: other tracks' builds and tests. The base alone first, then two pairs back to back: the base then
R1, and R1 then the base. µs a block; the load is the 1-minute one as each run started.

| | Base, alone (load 1.2) | Pair 1: base (10.5) | Pair 1: R1 (11.8) | Pair 2: R1 (15.4) | Pair 2: base (5.9) |
|---|---|---|---|---|---|
| The song's tail | 253 (154 nodes) | 297 (154 nodes) | **59 (52 nodes)** | **34 (52 nodes)** | 335 (154 nodes) |
| Critical path (partitions alone) | 327 | 407 | **196** | **133** | 456 |
| 1 thread, mean / p99 | 2,336 / 4,085 | 2,420 / 5,385 | 3,177 / 7,632 | 3,635 / 15,015 | 3,305 / 9,056 |
| 4 threads, mean / p99 | 640 / 1,015 | 628 / 1,017 | **524** / 1,018 | **547** / 2,258 | 629 / 975 |
| 8 threads, mean / p99 | 498 / 801 | 712 / 2,416 * | **294 / 509** | **285 / 557** | 659 / 2,546 * |
| Bits | `81dd175f739e5d71`, 10 of 10 fixtures | the same | the same | the same | the same |

\* waited for a CPU over 1,000 µs a block: other work on the same cores.

- **What R1 changes changed, in both orders:** the song's tail fell from 253 to 335 µs to 34 to 59, the critical
  path from 327 to 456 µs to 133 to 196. The spike estimated a tail of about 20 µs.
- **4 threads: 524 to 547 µs against 628 to 640** by the mean (about 15% faster); **8 threads: 285 to 294 against
  498 to 712.** The p99s moved with the load more than with R1.
- The 1-thread lines moved with the load alone (the same base read 2,336 at load 1.2 and 3,305 at 5.9). Judge
  nothing here: the owner's Mac does (PLAT-1).

| Log | |
|---|---|
| Base alone | `~/m2-runs/logs/8a-r1-before-1017-175736-1430602.log` |
| Pair 1 | `~/m2-runs/logs/8a-r1-pair-1017-180852-1511403.log` |
| Pair 2 | `~/m2-runs/logs/8a-r1-pair2-1017-194502-2191968.log` |
| The commit's check | `~/m2-runs/logs/8a-s3-r1-check-1017-180442-1482167.log` |
| Red runs | `~/m2-runs/s8a/s3-red/r1-*.diff`, `.log` |

## Slice 3: live playback on engine threads

### In short

- **Live playback renders on several cores now, with the same bits.** The device's own thread runs each block
  on a pool, as exports do. Every pure fixture plays live on 2, 4 and 7 threads bit for bit as its export on
  one; so do edits (a ghost and fades) and a resampled song.
- **Workers are engine threads.** On a device they ask for real time at the device's period: on a Mac the
  time-constraint policy and the output's audio workgroup; on Linux `SCHED_FIFO` within `RLIMIT_RTPRIO`. A pool
  whose workers aren't all real time is dropped: one thread, and the player says why. This server refuses real
  time, so a device here would play on one thread.
- **When something goes wrong:** a task that panics on a worker crashes the player, as before. A worker that
  never comes costs only speed. A stuck task puts the device's thread to sleep on a `Wake` (no CPU), and the
  task's end wakes it.
- **Nothing allocates.** Every worker is under the allocation trap from its start to its stop, through play, a
  seek, an edit with a ghost and fades, Panic, stop and play: 0.
- **Park by default.** Workers sleep after each callback (`BETWEEN`, one line to switch to `Spin`).
- **Shown red: 20 bugs, each caught** by the test it was made for (a small deliberate bug each). Five small
  tests have no red run (smoke tests and arithmetic), the two Mac tests wait for the owner's Mac, and one model
  mutant (M7) can't fail by design.
- **Run 20 times on this busy server** (load 2.3 to 52): 520 test runs, none failed.
- **Main was merged twice:** at the start (`98620e7`), and once `next` landed (`739cfc6`): no conflict. The
  player's hooks are on `next`'s shape (`start_in`, `silent_as`, `Shared.frames`, `freed`).

### Commits

| Commit | What |
|---|---|
| `98620e7` | Merge `main` (`8a6d9e1`, the HANDOFF) |
| `c8ce2bc` | R1 (above) |
| `b64aefe` | m2-os: a `Wake`, and engine threads that run as audio threads |
| `6185578` | The pool's caller sleeps on a `Wake` while a task is stuck |
| `739cfc6` | Merge `main` (`85b069a`): `next` is on main |
| `5c449b6` | The player renders on engine threads: driven and silent |
| `ab41e57` | On a device, engine threads are real time, in the device's workgroup on a Mac |
| `2589c3c` | Debug builds run a player's workers under the allocation trap (as RENDER-28's new words say) |
| this one | Contract: engine threads; build notes: R1 and slice 3 |

### What was built

| Where | What |
|---|---|
| `core/os/src/rt.rs` | `Wake` (ring, wait), `Group` (`of_output`, `none`, `period`, `recommends`, `missing`), `Joined`, `engine_thread`, `end_engine_thread`, `on_device_thread`; the time-constraint policy in Mach ticks, tested on Linux (D22) |
| `core/os/src/linux/wake.rs`, `mac/wake.rs` | the wake: a futex on one word; a Mach semaphore. Each a file of its own, which a grep test reads whole (D20). A wait ended early waits again for what is left (D5) |
| `core/os/src/linux/rt.rs` | `SCHED_FIFO`: the device's thread at `min(RLIMIT_RTPRIO, 10)`, workers one lower when the limit is 2 or more (D7); the soft limit raised to the hard one first; with a limit of 0 it still asks (root may) |
| `core/os/src/mac/rt.rs` | the workgroup by the device's UID (`'uidd'`, then `'oswg'`; the default output's `'dOut'` when no UID), `os_workgroup_join` and `leave` with the token kept in the thread's own storage, `os_workgroup_max_parallel_threads`, `os_release`; `thread_policy_set` on `pthread_mach_thread_np(pthread_self())` (D11) |
| `core/os/src/mac/cf.rs` | the CoreFoundation calls, declared once for the watcher and the workgroup (D22) |
| `core/os/src/thread.rs`, `unix.rs` | `thread::foreign`: a closure on a thread made with `pthread_create`, an 8 MiB stack, its panic raised on the joiner (D20) |
| `core/pool` | the caller dozes on a `Wake` (`Doze`: a flag, one more look, then the wait; a task's end and a worker leaving ring it); `Setup.doze` and `Setup.stop`; the caller's `dozes`, `rung`, `timeouts`; `Runner::trip` (`Panic`, `Hold`), `runs`, `tripped_on` (D15). The pool now depends on `m2-os`: `Cargo.lock` gained that one line |
| `core/runtime/src/threads.rs` | `live(group, spin, mark)`: a live pool's `Setup` (wasmtime's thread state, the one mode, `engine_thread`, counted in `Joins`; `mark` for the trap); patience half a period (1.25 with spin), stuck and doze one period. `live_workers()`; `parse` and `from_env`, the one `M2_THREADS` parse (the editor's `Threads` uses it) |
| `core/runtime/src/fpenv.rs` | public: `Canon`, `canon`, `is_canon`, `mode()`, and `flush_for_tests` (hidden) |
| `core/play/src/engines.rs` | `LiveThreads`, `Between`, `BETWEEN`, `live_count` (D4's pure rule), `EngineThreads`, `Handover`, the report, `for_clock`, `for_device` (the first callback's period, D8; the workers' own reports decide, D4), `judged`, `after_callback`; in debug builds every worker under the allocation trap (`TRAP`) |
| `core/play/src/lib.rs` | short hooks: `Msg::Threads`, `Gone::Runner`, the callback's `runner` and `between`, `take_held`'s handover, `fill` (the mode as found, `Canon`, `after_callback`), `Live::fill` on the runner, `silent_with`, `driven_on`, `Driver::runner`, `engine_threads()`; on a device, the pool after the stream plays, and `serve((stream, pool))` so the stream stops before the pool joins (D9) |
| `core/play/src/alloc.rs` | `alloc::all()`: the process's count, for the workers' test |

How a live block runs now:

1. The callback enters the one mode, and keeps the device thread's own to put back at the end.
2. A held handover (`Msg::Threads`) is taken between runs, like a replace: the runner swaps in, and the device's
   thread sets itself up once (`on_device_thread`).
3. Each block renders on the runner, as an export's does. A song with plugins renders on the device's thread.
4. After the callback, `rest()` sends the workers to sleep (`Park`).

### Where it differs from the plan

| Plan | Built | Why |
|---|---|---|
| R1's rule B, then fades labelled again | rule B alone, and overlays follow their source again in the same walk | D3. An overlay of a node that moved would otherwise stay in a partition of its own |
| `Group::real_time()`, the count rule in `m2-play` knowing real time | no `real_time()`; the workers report, `judged` keeps the pool or drops it | D4 |
| The period from `'fsiz'` or the frames | the first callback's frames, which `next` already records (`Shared.frames`) | D8; so the record (slice 4) needn't move before the device commit |
| `the_live_count_follows_the_rule`, red by "real time refused ignored" | the count is pure (D4), red by the workgroup's advice ignored (`s3-count-rec`); real time refused is `judged`'s test, red by `s3-rt-ignored` | D4 moved the check |
| The silent player's runner put straight into the callback | handed over as a device's is, the first message on its queue | one path, so every silent player tests the handover |
| A ring "after `left − 1` in `complete`" | right after `complete` returns, in `run_one` | the same order; `complete` stays the run state's own step |
| `a_stuck_task_…`: rung ≥ 1 after the latch | its doze lasts an hour, so only a ring can end it; every doze was rung | a worker leaving the run rings it too; the unit test pins the task's own ring |
| `a_stuck_caller_dozes_until_the_task_ends` on 4 threads | on 2 (one worker) | with idle workers, their leaving rings the caller and hides a missing ring at the task's end |
| The edit for `edits_on_engine_threads_match_one_thread` | first-song with the duck made a drive, and the keys' channel playing the hats through a new delay | the first edit (a delay on the keys) never read a fade's old side across partitions, so `rt-fade-reads` passed it. Now the keys track plays out the window as ghosts that the song reads only through a fade |
| `Gone::Runner(Box<Handover>)` with `Handover` private | `Handover` public, hidden, its fields private | D14 |

### Tests

Each shown red once by a small deliberate bug (`~/m2-runs/s8a/s3-red/<name>.diff` and `.log`, made by
`mutate.py`, which puts the file back). No test has a time bound: each waits on counters under a 60 s watchdog.

| Test | Where | Proves | Red by |
|---|---|---|---|
| `a_wake_rung_before_its_wait_returns_at_once` | `core/os/src/rt.rs` | a ring before an hour's wait: it returns at once | `os-wake-late` (the take looks for the wrong value: hangs, 60 s) |
| `a_ring_from_another_thread_ends_the_wait` | same | an hour's wait ends at another thread's ring | `os-wake-nowake` (no `FUTEX_WAKE`: hangs) |
| `an_unrung_wait_times_out` | same | false after its time; one ring is taken once | — |
| `a_wake_works_on_threads_std_did_not_start` | same | both sides through `thread::foreign` | — (a smoke test) |
| `a_group_without_a_device_has_no_workgroup_and_says_so` | same | `Group::none`: no advice, plain words, nothing asked | — (a smoke test) |
| `the_time_constraint_policy_is_in_ticks` | same | 2,666,667 ns is 64,000 ticks at 125/3 | — |
| `an_engine_thread_gets_fifo_or_says_why` | `core/os/src/linux/rt.rs` | refused here (`RLIMIT_RTPRIO` 0, not root): `real_time` false, not FIFO; the granted branch is a desktop's | `os-fifo-claims` |
| `the_wake_never_blocks` | `core/os/tests/no_blocking.rs` | grep: no allocation, lock, thread-local, `park` or I/O in either wake file | `os-wake-alloc` (a `vec!` in `ring`) |
| `an_engine_thread_has_the_time_constraint_policy`, `a_worker_joins_and_leaves_the_default_outputs_workgroup` | `core/os/src/mac/rt.rs` | read back with `thread_policy_get`; the default output's workgroup, advice ≥ 1, join and leave | the owner's check runs them |
| `a_stuck_caller_dozes_until_the_task_ends` | `core/pool/src/tests.rs` | the caller dozes (an hour at most), the task's end rings it | `pool-no-ring` (hangs) |
| `a_worker_that_never_comes_is_not_waited_for` | same | 1,000 random runs while worker 3 never starts | `pool-wait-woken` (hangs) |
| `stop_runs_on_each_worker_before_it_exits` | same | each start has its stop after the drop | `pool-no-stop` |
| `a_tripped_task_panics_on_a_worker_and_is_raised` | same | run 3's payload names a worker; the pool runs on | `pool-trip-outside` (the binary aborts) |
| model `the_caller_is_woken` | `core/pool/tests/models.rs` | 5,000 cases, 2,243 dozes, every one rung, none timed out (10 s each) | M6 (`asleep` set after the second look: a doze timed out); M7 below |
| `live_on_n_threads_is_the_export_on_1` | `core/play/tests/bit_identity.rs` | every pure fixture, whole and a second, live on 2, 4 and 7 threads: the export's bits; tasks on workers; reads across threads | `s3-live-one` (`Live::fill` ignores the runner) |
| `edits_on_engine_threads_match_one_thread` | same | G7c's three no-op activations on first-song and p16 on 4 threads: the export's bits; an edit with ghosts and fades: 4 threads equal 1 | `rt-fade-reads` (a fade's old side out of `reads()`: the debug read check fires, the player crashes) |
| `a_resampled_song_on_n_threads_matches_one_thread` | same | first-song on a 44.1 kHz device: 4 threads equal 1 | — |
| `the_callback_mixes_in_the_one_mode` | `core/play/tests/engine_threads.rs` | a subnormal song through the resampler and a replace's fade, on a thread that flushes: the same bits; the thread's mode back after | `s3-no-canon` |
| `a_panic_on_a_worker_crashes_the_player` | same | a worker's panic at block 50: crashed, silence after | `s3-drop-payload` |
| `a_worker_that_never_comes_costs_only_speed` | same | 2 s of first-song while worker 3 never starts: the export's bits | `pool-wait-woken-live` (hangs: the timeout ends it) |
| `a_stuck_task_puts_the_device_thread_to_sleep_on_its_wake` | same | the callback on a `pthread_create` thread; a held task: the device thread dozes (an hour at most), a ring ends it; one thread's bits | `pool-no-rings` (no ring at all: the watchdog) |
| `a_player_let_go_joins_its_workers` | same | the silent player on 4 threads: 3 plain workers start, and are joined when it is let go | `s3-forget-pool` |
| `engine_threads_never_allocate` | `core/play/tests/no_alloc_workers.rs` | 4 engine threads under the trap from their start: play, seek, an edit with a ghost and fades, `Player::panic()`, stop, play: 0 | `s3-alloc-task` (a `Vec` in each partition) |
| `the_live_count_follows_the_rule` | `core/play/src/engines.rs` | asked; off a device 1; one L2, no more than the advice; this server 8 | `s3-count-rec` (the advice ignored) |
| `a_device_pool_is_kept_only_when_every_worker_is_real_time` | same | `judged`: not started, cancelled, refused real time: one thread, said; outside the workgroup: kept, said | `s3-rt-ignored` |
| `a_device_without_real_time_plays_on_one_thread` | same | `for_device` here: workers refused real time, so one thread, "refused real time"; no callback in a second: one thread | `s3-rt-ignored` |
| `a_device_player_let_go_stops_its_stream_and_frees_its_engine_elsewhere` (extended) | `core/play/src/tests.rs` | with a pool beside the fake stream: the workers are joined after the let go | `s3-serve-forget` |

**M7 survives, by design.** M7 rings before `left − 1`. The caller can then miss that ring, but the same
worker rings again when it leaves the run, after the caller set its flag: a late wake, never a lost one. The
model can't see lateness without a clock. The unit test catches a missing ring.

Times (debug, Linux): the three live bit tests 31 s together (load 1.3; the fixtures alone 24 s), within the
plan's 60 s; `engine_threads` 2 s; `no_alloc_workers` 1 s; the pool's models 6 s in release (2.3 s before; the new
model 3.5 s), still under the check row's "10 s".

### The live tests, run many times

**20 rounds, none failed**, 19:18 to 19:45, while other tracks built and tested: the 1-minute load went from
2.3 to 52 (round 11) and back to 4.7. Copies of the debug test binaries at `ab41e57`, so the worktree could
build meanwhile (`~/m2-runs/s8a/s3-repeat/run.sh`; the copies, 1.7 GB, deleted after). Each round ran 26 tests:

| Binary | Tests each round | Time each round |
|---|---|---|
| `bit_identity` | `live_on_n_threads_is_the_export_on_1`, `edits_on_engine_threads_match_one_thread`, `a_resampled_song_on_n_threads_matches_one_thread`, and the offline `every_pure_fixture_renders_the_same_bits_on_1_and_n_threads`, `p16_renders_the_same_bits_on_1_and_4_threads` | 62 to 138 s |
| `engine_threads` | all 5 | about 2 s |
| `no_alloc_workers` | `engine_threads_never_allocate` | about 1 s |
| the runtime's `tests::parts` | all 15 (R1's among them) | about 2.4 s |

That is 520 test runs, each comparing bits on 1 and N threads, or checking a worker, with no failure. Log:
`~/m2-runs/logs/8a-s3-repeat-1017-191814-1978988.log`.

### Checks

- Each commit's check (`~/m2-runs/s8a/s2-check.sh`): the debug `m2`, 10 of 10 renders against
  `e2e/renders.sha256`, the touched crates' tests, workspace clippy and the Mac lint (`--target
  aarch64-apple-darwin`), rustfmt on new files. All green.
- The merge of `next` checked alone (`m2-os`, `m2-pool`, `m2-runtime`, `m2-play`): green.
- `Cargo.lock`: one build without `--locked` (`cargo check --offline -p m2-pool`) added only `m2-os` to
  `m2-pool`'s entry. Every other build `--locked`.
- Disk at the slice's end: `df -h /` 2.3 TB free; this worktree's `target/` 104 GB (kept for slice 4).

| What | Log |
|---|---|
| R1 | `~/m2-runs/logs/8a-s3-r1-check-1017-180442-1482167.log` |
| m2-os | `8a-s3-c3-check-1017-182007-1597241.log` |
| the pool (and its models) | `8a-s3-c4-check-1017-182818-1663680.log` |
| the merge of `next` | `8a-s3-merge-check2-1017-183509-1723714.log` (the first, `8a-s3-merge-check-…`, failed on a draft file of mine left beside it, not on the merge) |
| the player | `8a-s3-c5-check2-1017-190806-1915032.log` (its Mac lint failed on `u64::from` of a `u64`; fixed, both lints and fmt run again green) |
| the device | `8a-s3-c6-check-1017-191420-1955995.log` |
| the trap on workers | `8a-s3-trap-check-1017-195235-2246146.log` |
| the red runs | `~/m2-runs/s8a/s3-red/`; commit 5's in one run: `8a-s3-red5-1017-185720-1842344.log` |

### For the lead

- **The Mac code first runs on the owner's Mac** (risk L1): the Mach semaphore, the workgroup by UID, the
  time-constraint policy. The Mac lint type-checks it; its two tests and G12's set-up line will say what failed.
- **This server refuses real time.** So the device path's kept-pool branch, and a granted `SCHED_FIFO`, never
  ran here; the refused branch did, end to end (`a_device_without_real_time_plays_on_one_thread`).
- **Slice 4 next:** the record (`timing.rs`), G12 live, `ci/gates.txt`, and the owner's Mac script (D23). The
  player already has what G12's set-up line needs: `Player::engine_threads()` (count, workers real time and
  joined, the device thread's own report, what is missing, `between`, the device thread's FP register as found).
- **The offline Mac sweep owed above** stays owed; D23 folds it into slice 4's `ci/sweep.sh`.

### For the owner's Mac (what slice 3 needs)

**In short:** once the lead has merged `8a` into `main`. About 15 minutes. Plugged in, lid open, other apps
closed, in Terminal on the Mac itself. It plays nothing aloud. Paste back `~/m2-8a.txt`. Slice 4 adds the live
run (G12) and puts every line into one script (`ci/sweep.sh`, D23); until then these lines stand alone.

```sh
cd ~/dev/suite2/music2 && git fetch && git switch main && git pull
cargo test --locked --release -p m2-pool --features models 2>&1 | tail -5 | tee ~/m2-8a.txt
cargo test --locked -p m2-os rt 2>&1 | grep -E '^test |test result' | tee -a ~/m2-8a.txt
sysctl hw.model hw.perflevel0 hw.perflevel1 | tee -a ~/m2-8a.txt
cargo run --locked --release -p m2-gates -- realtime --offline --reference 50 2>&1 | tee -a ~/m2-8a.txt
```

| Line | About | What it decides |
|---|---|---|
| models | 1 min | the pool's orderings and the caller's `Wake` on arm64 (M5 can only fail there). A FAIL: stop; the bug is fixed on Linux first |
| `m2-os rt` | 2 min with the build | the Mac's own code, run for the first time: the Mach semaphore wake, the time-constraint policy read back, the default output's workgroup joined and left. A FAIL: that code is fixed before anything live is judged; until then a Mac's workers run outside the workgroup, and G12 will say "not the designed set-up" |
| `sysctl` | seconds | `hw.perflevel0.cpusperl2` caps the live count: the P-cores of one L2 (4 on the M1 Pro, *estimate*) |
| offline sweep | 13 min with the build | S = 1-thread p99 ÷ 4-thread p99 (D23): S under 2.5 is a note to the owner that the base M1 is at risk; 8 threads 10% or more faster than 4 means an export alone uses every P-core; a partition's p99 over 667 µs is the trigger for voices across threads; the executor's own cost over 40 µs a block is trimmed before G7f's next run. The live count comes from slice 4's live sweep, not from this |

## The Mac sweep (owed)

For the owner, once the lead has merged `8a` into `main`. About 10 minutes. Plugged in, lid open, other apps
closed, in Terminal on the Mac itself.

| # | Do this | Why |
|---|---|---|
| 1 | Type the first line below and press Return | gets 8a's offline half from `main` |
| 2 | Type the second line and press Return | checks the pool on the Mac's own CPU (about 1 minute) |
| 3 | Type the third line and press Return | builds the gates (about 2 minutes), then the sweep (about 8) |
| 4 | Type the fourth line and press Return | the core counts, to check what the gate read |
| 5 | Paste back what is in `~/m2-8a-offline.txt` | that is all the lead needs |

```sh
cd ~/dev/suite2/music2 && git fetch && git switch main && git pull
cargo test --locked --release -p m2-pool --features models 2>&1 | tail -5 | tee ~/m2-8a-offline.txt
cargo run --locked --release -p m2-gates -- realtime --offline --reference 50 2>&1 | tee -a ~/m2-8a-offline.txt
sysctl hw.perflevel0 hw.perflevel1 | tee -a ~/m2-8a-offline.txt
```

What it decides (the plan's §6). S = 1-thread p99 ÷ 4-thread p99, back to back, the median of the two passes:

| Reading | Decision |
|---|---|
| A bits line says FAIL, or the models fail | Stop: a bug, fixed on Linux first |
| S ≥ 2.5 | Go: 8a's live half as planned, 4 live threads |
| 2.0 ≤ S < 2.5 | Go, with "a mixer strip joins its track" first (the spike's R1) |
| S < 2.0 | No live half: a study first, and a note to the owner |
| A count under 4 beats 4 | the live default is that count |
| Paced 1-thread p99 more than 15% above back to back | "spin between callbacks" moves into the live half |
| `hw.perflevel0.cpusperl2` isn't 4 | the live count rule changes to match |
| A partition's p99 over 667 µs | voices across threads |
| The executor's own cost over 40 µs a block | trimmed before G7f's next Mac run |
| 8 threads beat 4 by 10% or more | an export alone uses every P-core on a Mac (`fast`), not one L2's |

The Mac's full `ci/check.sh` is owed too before the live half: it runs the new Mac tests (`cores_on_this_mac_are_sane`,
`a_background_thread_has_background_qos`) and `fpenv`'s arm64 code under every test.
