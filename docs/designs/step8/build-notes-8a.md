# 8a's offline half: build notes

What each slice of 8a's offline half did, what it measured, and where the logs are. The plan is
`~/m2-runs/s8a/plan.md` (its Attack and Decisions win). The design is `design-8a.md`. Linux timings here are
information only: only the owner's Mac judges timings (PLAT-1).

## In short

| Slice | State | What it says |
|---|---|---|
| 0: the spike (thrown away) | done | **Go.** p50 on 4 threads is 2.75 to 2.99 times faster than step 7's loop by the mean, 3.3 to 4.2 times by the p99. The rule needs 2.5 and 2.0. Same bits on every thread count. |
| 1: the pool (`m2-pool`) | — | — |
| 2: the executor, offline | — | — |
| The last check | — | — |
| The Mac sweep | owed | after slice 2 |

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

(to come)

## Slice 2: the executor offline

(to come)

## The last check

(to come)

## The Mac sweep (owed)

(to come)
