# 8a, offline half: the plan (slices 0, 1, 2)

Written 2026-10-17, read-only on the repo (git log, show, diff and grep; nothing built or run). Base: branch `8a` at
`e571a5f` (= `main`), worktree `~/m2-runs/wt/8a`. Starts from `~/m2-runs/s8a/refresh.md` (written against `ed7fc8e`);
its Attack section wins wherever it differs from the refresh. Also read: `docs/designs/step8/design-8a.md` (rev 2),
`docs/architecture.md`, `docs/designs/step7/build-notes.md`, `docs/designs/platform/design.md` §5.6 and §8,
`HANDOFF.md`, `docs/designs/step8/backend-plan.md`. "Estimate" marks a number nobody measured.

## In short

- **This track builds 8a's offline half.** Slice 0: a spike, thrown away. Slice 1: the pool, a crate of its own
  (`m2-pool`). Slice 2: the executor, used by every offline render, plus `m2 render --threads`, `M2_THREADS`,
  `m2_os::rt::{cores, background_thread}` and `m2-gates realtime --offline`. Live playback still renders on **one**
  thread, through the same executor. Slice 3 (live on engine threads) and slice 4 (gate 8) are the next track.
- **What changed on main since `ed7fc8e` matters in five places.** Overlays are a new node kind with window-only
  copies. Meter taps read buffers after the graph. `Ports` is now a name in `lib.rs`. `Player::load()` is taken.
  G7f now measures S fresh in every run. HV3 and HV4 did **not** land, so two of the Attack's worries are moot.
  `core/os`, `core/plugins` and `crates/editor/src/render.rs` did not change.
- **Six decisions differ from design-8a.** They come from the Attack, or from what this server lacks.
  1. The caller of a run never parks. On a stuck task it sleeps in short steps. So there is no `Wake` and no
     device-thread park, and no thread handle can go stale.
  2. No loom: it is not in the offline registry. The models use a small sync shim that yields and spins at random,
     with std threads, many times over. Each model is shown red against a named mutant.
  3. An overlay goes in the partition of the track it carries, not its consumer's. A fade goes with its consumer.
  4. A panic in a task is caught, the block finishes, and the panic is raised again on the caller after the join.
     So the player crashes and is replaced, as today, instead of going silent for good.
  5. Edges come from `Exec::reads()`, what each built node reads. Not from `Wiring`, and not from `FOp::deps`.
  6. Songs with plugins render on one thread in this track. Plugins on worker threads come with slice 3.
- **Every 1-vs-N test proves a task ran on a worker.** A test mode (`spread`) wakes every worker, keeps the caller
  off the first tasks, and counts tasks on workers and edges that cross threads. Each test asserts both are above 0.
- **After this track the owner runs one command on the Mac (about 5 minutes).** It is the go or no-go for live
  playback on all cores. The command and its rule are in §6.
- **Time** (agent time, *estimate*): slice 0 and slice 1 in parallel, about half a day each; slice 2 about a day;
  the final check about half a day. About 2 days in all.

---

## 1. What changed on main since `ed7fc8e`, and what each change means for 8a

103 commits. Paths checked: `core/runtime`, `core/play`, `core/plugins`, `core/os`, `crates/gates`,
`crates/editor/src/render.rs`, `apps/m2`, `crates/server/src/graph.rs`.

| Change | Commits | Where | What it means for 8a |
|---|---|---|---|
| **Kernel calls keep their memory's address** (speed cut 1, "HV1") | `444fc66` | `lib.rs`: `MemView` (`unsafe impl Send`), `struct Ports<'a>`, `KernelInst::ports`, `KernelInst::prewarm` checks the address; `PackageSet.engine` is crate-private | Still sound on workers: the pointer moves with its store, and `GraphView::own` gives one thread at a time. Never add `Sync`. **`Ports` is taken**: design-8a's view trait is named `Reads` here. Kernel calls got cheaper, so every 1-thread baseline must be measured fresh (spike). |
| **Speed cuts 3 and 4 held** (one Store per voice pool; the non-finite scan at activations) | HANDOFF "Left for later" | not on main | One `Store` per kernel instance, voices included. The per-block non-finite scan stays inside each node's step, so it runs in parallel. The refresh's R12/R13 and Attack §A.4 don't apply now. If cut 4 ever lands, keep its scan inside the tasks. |
| **Monitoring overlays** (mute, solo, mute in song) | `516a7bf`, `0e39f1b`, `ca51150`, `6e388a4`, `f3a1085`, `fdf261f` | `overlay.rs` (new): `place`, `process`; `Exec::Overlay { src, target, cur, step }`; `Renderer::build` pushes them after the fades; `Plan.window_overlays`, `window_held`; `Cells` gains overlay cells; `take_over` copies held gains | A new exec kind: `reads()` lists its `src`. **Label: the source's partition** (Q4). Celled sites get their `target` from `Cells::apply`, in the pre-pass, so it is frozen during the graph; `cur` is the node's own state. **`order_after` now also drops the window-only overlays.** With one schedule, `end_fade` must turn them into `Exec::Const(0.0)`, as it does ghosts. |
| **Meter taps** | `61fa05d`, `cc5d549`, `7104a5a`, `a8199ff` | `taps.rs` (new): `Taps::write` after the graph in `render_block`; `fetch_max` on atomics; live plans only | Taps read buffers after the graph: they go in the **post-pass**, after the join, on the caller. No change of bits. They stay there (Q10). |
| **`time.playing`** | `8b7b7e9`, `09b2dfd` | `Exec::Time(TimeOf::Playing)` reads `Block.runs` | Reads the frozen `Block` only. No edge. |
| **Live notes (audition)** | `d04e8bf`, `ef034dd`, `81e953b`, `107887a` | `engine.rs`: `Msg::Live`, applied at the block line; `Renderer::live`; `sched.rs` live table, `LIVE_ROOM` in poly and plugin capacities | Applied at the block line, before the pre-pass: the pool is idle then. Schedulers stay read-only in tasks. Nothing to do. |
| **The DSP load** | `98c5beb` | `core/play`: `Load`, `Player::load()`, `Shared.longest`, `late`, `running`; `Player::idle()` | **`Player::load()` is taken.** Slice 3's histogram is `Player::timing()` (the Attack's name). `late` already counts long callbacks. `Player::idle()` lets the server pick an export's thread count in this track. |
| **The `sounds` merge, std 0.2** | `3d4a552`, `6591ecd`, `dd30acd`, `2796b13`, `e8bba0e` | 22 new std components; `examples/sounds`; 10 render records (was 8) | Five pure fixtures for the 1-vs-N tests: first-song, resample, sounds, starter, empty. OS-49 (subnormals on x86) needs a measurement on the larger std (slice 2). |
| **G7f measures S fresh** (the speed work to `ffc36aa`) | `5b77ae4` … `ffc36aa`, `a3214f6` | `crates/gates`: `baseline.rs`, `s/` (S's runner), `paired.rs` (`in_turns`, at least 3 sides), `store.rs`, `arena.rs` rewritten | G7f judges the app against S by the mean: 80 µs a block, 320 µs a 512-frame callback. Slice 2 puts the 1-thread executor inside "the app". The Mac at `cbac57c` read −19 µs a block and −196 µs a callback. Slice 2 measures its own cost as a paired line. G7f stays on one thread. `paired::in_turns` is reused for that line. Room on the Mac is about 100 µs a block; this plan's budget for the executor is 40 µs. |
| **`ci/check.sh` PART rows** | `3320557`, `33812c4` | `ci/check.sh` | The new "pool models" row follows the same `begin`/`record` rules. |
| **New CLI edits** (`mute`, `clips`, `unautomate`) | `b4cc256`, `41dabc7`, `7c7a415`, `bc761ef` | `apps/m2/src/edits.rs`, `main.rs` | No render change. Only `render` and `bounce` gain `--threads`. |
| **Server modules** (audition, clips, meters, overlay) | `cc5d549`, `0170b82`, … | `crates/server/src/graph.rs`, `graph/*.rs` | Meters read taps: unchanged. `retire()` is unchanged (live is slice 3). Export and agent renders gain a thread choice in `graph/actor.rs` (3 call sites). The `8b-ui` track also edits `crates/server`: keep this edit to those lines. |
| `core/os`, `core/plugins`, `crates/editor/src/render.rs` | — | no change | The refresh's reading of them holds. |

---

## 2. What this track builds with: the Attack folded in

| Topic | Refresh or design-8a said | Built here | Slice |
|---|---|---|---|
| Spike shape | `std::thread::scope` per block | Threads that live for the whole run, a barrier per block. CCD1 (32 MB L3) is the main result. Sticky against FIFO placement. Paced 128 frames against back-to-back | 0 |
| The pool's crate | `core/pool`, loom models | `core/pool` (`m2-pool`, std only). No loom (not in `~/.cargo/registry`): a sync shim with chaos yields, std threads, many runs. A mutant shown red for each model | 1 |
| Who owns the pool | the callback | `Pool` (owns and joins the workers) and `Runner` (the one handle that runs jobs; `Send`, not `Clone`). After its pool drops, a `Runner` runs every task itself | 1 |
| The caller's sleep | a `Wake` (Mach semaphore) for the device thread | **The caller never parks.** It spins while tasks finish. After `stuck` with none finishing, it sleeps in steps of `stuck / 4`. Workers park with std's `park`; std starts them, so their handles never go stale | 1 |
| `again` across four layers | design-8a | `Runner::rest()`: workers sleep at once. Without it they spin `patience`, then sleep | 1 |
| Errors | `anyhow` in the pool | A task returns `bool`. The runtime keeps its own error slot per partition | 1, 2 |
| A panic in a task | a failure: the plan goes `broken` | Caught; the block finishes; raised again on the caller after the join (the lowest task number's). `Callback::fill_in` then crashes the player, as today | 1, 2 |
| Graph edges | `Wiring`'s kept inputs | `Exec::reads()`: what each built node reads. Used by the schedule, the build's debug check and the debug read check | 2 |
| The view | `GraphView` (raw pointers) or `UnsafeCell` per node | Raw pointers taken once from `&mut Graph`; only single elements are ever borrowed, never a whole `Vec`. No Miri here (no nightly toolchain) | 2 |
| Labels | overlay with its consumer | Plan node: its module. Ghost: its old module. Fade: the node that reads it. Overlay: **the node it reads**. Output's fade: its new side's | 2 |
| One schedule per plan | delete `order_after` | Deleted. `end_fade` makes ghosts **and window-only overlays** `Exec::Const(0.0)` | 2 |
| FP mode | `canon()` per callback, block, task, plugin call | `fpenv::Canon` at each rendered block, **restoring the caller's mode at the block's end** (Attack §A.9). `canon()` before each task and after each plugin call. The callback-level guard (resampler, `LiveFade`) is slice 3 | 2 |
| Plugins on workers | yes | Not in this track: a song with plugins renders on one thread. Slice 3 adds the truthful `is_audio_thread` and the probe tests | 2 |
| Thread counts | utility QoS beside playback | `background_thread()` = `QOS_CLASS_BACKGROUND` on the Mac, nice +10 on Linux. Counts in §5.3 | 2 |
| The executor's own cost | G7f would hide it | A paired line in `realtime --offline`: per-module partitions against one partition, on one thread | 2 |
| OS-49 | unchecked | Measured in `realtime --offline` (a reverb tail); decided by the rule in Q16 | 2 |
| 1-vs-N tests | might run all on the caller | `spread` mode; every test asserts tasks ran on workers | 2 |
| Wall-clock bounds in pool tests | 3 tests | None. Counters and events only | 1 |
| Mac checkpoint after slice 2 | "recommended" | **Required**, before any live work (§6) | after 2 |
| `machine()` topology | in `machine()` | Not in it: `cores:` on a line of its own | 2 |

Left for slice 3 or 4, as the Attack placed them: `is_audio_thread` for every engine thread, workgroups, the
foreign-thread test, the per-pool allocation count, OS-61, the live count falling to 1 when real time is refused,
`Player::timing()`, RENDER-28/28a and HOST-2's edits, G12's verdict tests that must fail, the provisional words.

---

## 3. Slice 0: the spike (thrown away)

**Goal:** before anything is built, see whether per-module partitions scale on p50, and where the time goes.
About half a day, beside slice 1.

### 3.1 Where

| What | Where |
|---|---|
| Worktree | `git -C ~/m2-runs/wt/8a worktree add --detach ~/m2-runs/wt/8a-spike HEAD` |
| Target | `CARGO_TARGET_DIR=/home/jerry/m2-runs/8a-scratch/spike-target`; build only `-p m2-gates --release` |
| HOME | `/home/jerry/m2-runs/8a-scratch/home`, with `M2_AUDIO=none` |
| Logs | `~/m2-runs/s8a/spike/` (one log a run), plus `spike.diff` (the code, kept for reference) |
| Afterwards | numbers into build notes "Slice 0"; `git worktree remove --force ~/m2-runs/wt/8a-spike`; delete the spike target; `df -h /` |

### 3.2 The code (spike quality, no tests)

- `m2_runtime::Renderer::spike(...)`, `#[doc(hidden)]`: partitions = one per `FNode.module`, from `Flat.order` of
  a fresh plan (no ghosts). Edges from what each built node reads (a quick `reads()`).
- **Threads that live for the whole run.** N−1 workers spawned once. Each calls
  `wasmtime::Engine::tls_eager_initialize()` once. A spin barrier per block (offline: allocation is fine).
- Two placements:
  - **FIFO:** one ready queue, roots by bottom level, consumers pushed as their inputs finish.
  - **Sticky:** roots sorted by bottom level, root k's home is thread `k mod N`. A thread takes its own roots
    first, then the shared queue, then steals others' roots. The same plan and count give the same homes each
    block.
- A gates subcommand `spike` (`crates/gates/src/spike.rs`): p50 from 3:00 with `Transport::starting` (as G7f),
  1 s untimed, then 30 s timed (11,250 blocks).
- Each N-thread run hashes its output and compares with the 1-thread run. A mismatch means the spike's partitions
  are wrong: fix before reading any number.

### 3.3 Where CCD1 is

Read from sysfs, never assumed:

```sh
for c in /sys/devices/system/cpu/cpu[0-9]*; do
  echo "$(cat $c/cache/index3/size) $(cat $c/cache/index3/shared_cpu_list)"; done | sort -u
```

Today: `98304K 0-7,16-23` (CCD0, V-cache) and `32768K 8-15,24-31` (CCD1). SMT siblings
(`topology/thread_siblings_list`): CPU n and n+16. **Main result: `taskset -c 8-15`**, CCD1's 8 physical cores,
siblings left out.

### 3.4 The runs

| Run | Pinned | Threads | Placement | Mode | What it answers |
|---|---|---|---|---|---|
| A (main) | CCD1 `8-15` | 1, 2, 3, 4, 6, 8; two passes, the second in reverse order | FIFO | back-to-back | scaling on a 32 MB cache |
| B | CCD1 | 2, 4, 8 | sticky | back-to-back | sticky against FIFO |
| C | CCD0 `0-7` | 1, 4, 8 | best of A/B | back-to-back | how much the 96 MB cache helps (cache sensitivity) |
| D | both dies `0-15` | 8, 16 | best | back-to-back | crossing dies: the default count for exports |
| E | CCD1 | 1 and 4 | best | **paced**: one block every 2,667 µs (`sleep_until`), workers parked between blocks | the 128-frame paced number no machine has yet |
| F | CCD1 | 1 | — | per-partition timing | each partition's mean, p99 and max; the ten slowest; the song tail; pre-pass and post-pass |
| G | CCD1 | 1 | — | step 7's `Graph::process` loop against the spike's 1-thread executor, alternating 2 s turns | the honest baseline: speed-ups are against the old loop |

Each run prints: mean, p50, p99 and max block (µs); the speed-up against run G's old loop, by mean and by p99; for
each thread its busy time, its minor faults and its involuntary switches per block (`getrusage(RUSAGE_THREAD)`), and
how long it waited for a CPU (`/proc/thread-self/schedstat`, second field).

**Hardware counters:** none. `perf` is not installed, and `kernel.perf_event_paranoid` is 4, so no unprivileged
counters. No sudo (that changes the system). C against A stands in for cache sensitivity; faults and waits stand in
for the rest. The Attack's "bandwidth or TLB?" question stays open; the numbers decide what to do anyway.

**Load:** run `uptime` before each run and log it. Start only when the 1-minute load is under 4. Wait at most
10 minutes (a Monitor until-loop), then run anyway and say so. A run whose threads waited for a CPU more than 2% of
their busy time is run once more; if it still is, keep it, marked "contended", with its load.

### 3.5 Go or no-go (Linux decides only whether to go on; the Mac decides whether 8a can pass)

S4 = old loop's mean block (G) ÷ 4 threads' mean (A or B, the better), on CCD1. P4 = the same with p99.

| Reading | Then |
|---|---|
| S4 ≥ 2.5 and P4 ≥ 2.0 | **Go.** Slices 1 and 2 as planned. |
| 2.0 ≤ S4 < 2.5, or P4 < 2.0 | **Go, with R1 first.** Slice 2 also builds "a mixer strip joins the track it reads" (design-8a slice 4, item 1) if F shows the song tail above 15% of the 4-thread block. |
| S4 < 2.0 | **No-go.** Stop the track. Write to the lead with A to G. Nothing else is built until the lead decides. |

Two more choices come from the spike:
- **Placement:** sticky if B beats A by 5% or more at 4 threads by the mean, and is no worse at p99. Else FIFO.
- **Default count for exports alone:** every fast core (`Cores.fast`) unless D's 16 threads are slower than A's
  8, or D's 8 slower than A's 8 by more than 10%; then `Cores.per_cache`.

---

## 4. Slice 1: the pool (`m2-pool`)

About half a day. Nothing uses it yet, so everything stays green.

### 4.1 Files

| File | What |
|---|---|
| `core/pool/Cargo.toml` | `name = "m2-pool"`, no dependencies; `[features] models = []` |
| `core/pool/src/lib.rs` | "In short" doc; `Tasks`, `Setup`, `Pool`, `Runner`, `Outcome`, `Counters`, `Spread` |
| `core/pool/src/tasks.rs` | building the task graph: consumers (deduplicated, most critical first), input counts, roots, bottom levels, width; the run state, sized here |
| `core/pool/src/run.rs` | the block path: `wake`, `run`, `help`, `pop`, `complete`, `rest`, the worker loop, the abort guard. No allocation, lock, channel, thread-local or I/O |
| `core/pool/src/sync.rs` | the shim: std's atomics, `park` and `unpark`; with `models`, the same with a chaos point before each operation (a seeded yield or a short spin) |
| `core/pool/src/tests.rs` | unit tests (§4.4) |
| `core/pool/tests/no_blocking.rs` | grep test over `run.rs`, like `core/play/tests/no_blocking.rs` |
| `core/pool/tests/models.rs` | `#![cfg(feature = "models")]`: the models (§4.5) |
| `Cargo.toml` | `m2-pool = { path = "core/pool" }`; `[profile.dev.package.m2-pool] opt-level = 3` (it runs on the audio thread) |
| `ci/check.sh` | the "pool models" row (§4.6); the header's list of rows |
| `docs/designs/step8/build-notes-8a.md` | "Slice 1" |

`CachePadded` from crossbeam is not needed: a 4-line `#[repr(align(128))] struct Padded<T>(T)` keeps the crate
std-only.

### 4.2 The API

```rust
/// A graph of numbered tasks: edge (a, b) means b reads what a wrote. Its run state is sized here.
pub struct Tasks { .. }
impl Tasks {
    pub fn new(costs: &[u32], edges: &[(u32, u32)]) -> Tasks; // bottom levels from costs; acyclic (debug-checked)
    pub fn len(&self) -> usize;
    pub fn width(&self) -> usize;                 // the widest layer: at most width − 1 workers are woken
    pub fn consumers(&self, t: u32) -> &[u32];    // most critical first
    pub fn outcome(&self, t: u32) -> Outcome;     // after a run: Ran, Failed or Skipped
}
pub struct Setup {
    pub start: Option<Arc<dyn Fn(usize) + Send + Sync>>, // on each worker, first (m2-runtime: wasmtime's set-up, canon)
    pub patience: Duration,                       // how long a worker spins with nothing to take
    pub stuck: Duration,                          // how long the caller spins with no task finishing
}
impl Setup { pub fn plain() -> Setup }            // 50 µs and 1 ms: offline renders and tests
pub struct Pool { .. }                            // owns the workers (2 MiB stacks, named m2-pool-<i>); joins them on drop
impl Pool { pub fn new(threads: usize, setup: Setup) -> io::Result<(Pool, Runner)> } // threads counts the caller; 1..=64
pub struct Runner { .. }                          // Send, not Clone: only one thread can publish a job
impl Runner {
    pub fn one() -> Runner;                       // no worker, no heap: the audio thread may make one per call
    pub fn threads(&self) -> usize;
    pub fn wake(&mut self, width: usize);         // at the block line, before the pre-pass
    pub fn run(&mut self, tasks: &Tasks, task: &(dyn Fn(u32) -> bool + Sync)) -> Result<(), Box<dyn Any + Send>>;
    pub fn rest(&mut self);                       // workers sleep now, not after `patience`
    pub fn counters(&self) -> Vec<Counters>;      // per thread: tasks, parks, signals received (off the audio thread)
    #[doc(hidden)] pub fn spread(&mut self, on: bool);
    #[doc(hidden)] pub fn spread_report(&self) -> Spread; // tasks run on workers, edges whose tasks ran on two threads
}
```

`run`'s promise, in its doc:
- It returns once every task has run, failed or been skipped, and no worker can touch `tasks` or `task` again
  (`inside` and Dekker's pattern, design-8a §5.3).
- A task returning `false` fails. Everything that reads it, directly or not, is skipped. The rest runs.
- A panicking task counts as failed. After the join, `run` returns the payload of the lowest-numbered task that
  panicked. The caller raises it again.
- A panic in pool code aborts the process (a guard in `run` and in the worker loop).
- **The caller never parks.** It spins while tasks keep finishing. After `stuck` with none finishing, it sleeps in
  steps of `stuck / 4` and checks again. It calls no `thread::current()`, `park()` or other per-thread state, so any
  thread may call `run`, one std did not start included.
- A worker leaves a job when the queue is empty and every task left is running (`left ≤ running`), or after
  `patience` with nothing finishing. It never enters the same job twice. Then it spins `patience` for the next job,
  or until `rest()`, and parks.
- At most `min(workers, width − 1)` workers are woken. A worker is signalled only if its padded `parked` flag swaps
  from true. std's park token holds one signal, so a worker that slept through many wakes wakes once.

**Spread** (tests and gates only): every worker is woken, whatever the width. The caller takes no root until every
root has been taken by a worker. Before each task, a seeded spin of 0 to 2,000 iterations varies the interleaving.
The runner counts, over all its runs, the tasks run on workers and the edges whose two tasks ran on different
threads. With one thread, spread does nothing.

**Sticky placement**, only if the spike chose it: root k's home is thread `k mod threads`. A thread takes its own
roots first (one atomic cursor per thread), then the shared queue, then other threads' roots. The rest of the
protocol is unchanged.

### 4.3 What stays out of `m2-pool`

No wasmtime, no `anyhow`, no OS calls, no FP mode. The runtime's task closure calls `fpenv::canon()` itself, and
its `Setup.start` hook does wasmtime's per-thread set-up and `background_thread()`.

### 4.4 Unit tests (the normal `cargo test`; no wall-clock bound in any)

| Test | Proves | Fails without |
|---|---|---|
| `every_task_runs_once_after_its_inputs` | 500 random DAGs (2 to 40 tasks), 1 to 8 threads, spread on and off. Each task checks its producers' done flags (Acquire), then sets its own. Each runs once. With spread and 2 threads or more, tasks ran on workers | the input counts, the push after the last decrement |
| `a_failed_task_skips_what_reads_it_and_the_rest_run` | the skipped set equals the failed tasks' descendants, computed serially | skip propagation |
| `a_panicking_task_finishes_the_run_then_is_raised` | a task panics on a worker; every other task ran or was skipped; `run` returns the lowest panicking task's payload | the per-task `catch_unwind` (the worker dies, and the run waits: the test's 10 s watchdog fails it) |
| `a_panic_in_pool_code_aborts` | the test re-runs itself as a child with a `#[cfg(test)]` hook that panics in `complete`. The child must end in an abort within 10 s, not hang and not succeed | the abort guard |
| `idle_workers_sleep_while_a_task_blocks` | a task waits on a latch. The test polls `counters()` until every other worker has parked (10 s watchdog), then opens the latch. The run returns | the leave rule and `patience` |
| `rest_sends_workers_to_sleep_at_once` | `patience` set to 30 s. After `rest()`, every worker parks (5 s watchdog) | `rest()` |
| `a_worker_held_off_wakes_once` | 1,000 wakes while a worker is busy in a long task: it receives at most one more signal than it parked | the `parked` flag |
| `a_chain_wakes_nobody` | width 1: no worker is signalled | the width cap |
| `a_runner_outlives_its_pool` | the `Pool` is dropped; the `Runner` still runs every task itself | the stopped path |
| `dropping_a_pool_joins_every_worker` | a `start` hook counts threads alive; 0 after the drop | `Drop for Pool` |
| `spread_puts_every_root_on_a_worker` | spread on: each root ran on a worker | spread |
| `the_same_graph_builds_the_same_tasks` | edges in any order give the same consumer lists, roots and width | sorting |

### 4.5 The models (`--features models`, release, in their own row)

The shim puts a chaos point before every atomic operation and every park: a seeded `yield_now` or a spin of up to
64 iterations. Each model runs many small cases with 2 to 4 threads. Liveness is a 10 s watchdog per case.

| Model | Checks |
|---|---|
| `each_task_once_after_its_inputs` | as the unit test, 2,000 cases |
| `nothing_touches_a_job_after_run_returns` | after `run` returns, the case seals the job; with `models`, every pool access to a job checks the seal and records a breach; the model asserts none |
| `back_to_back_runs_with_rest_between` | runs of different graphs back to back, `rest()` between some at random |
| `no_lost_wake` | `rest()`, then `wake` and a spread run, which needs a worker to take the roots: it must finish |
| `skips_are_exactly_the_descendants` | random failures |

**Shown red before the slice lands**, each against the model that must catch it (patches kept in
`~/m2-runs/s8a/mutants/`, logs in the build notes, never committed):

| Mutant | Caught by |
|---|---|
| M1: push a consumer at its first input's completion | `each_task_once_after_its_inputs` |
| M2: `run` returns without waiting for `inside == 0` | `nothing_touches_a_job_after_run_returns` |
| M3: a worker parks without checking again after setting `parked` | `no_lost_wake` |
| M4: no skip for a failed task's consumers | `skips_are_exactly_the_descendants` |
| M5: the queue slot and the input counts made `Relaxed` | may pass on x86 (TSO). Say so; the Mac's check runs the same row on a weak-memory CPU |

### 4.6 The `ci/check.sh` row

After clippy (row 6), before the std rows:

```sh
# 6b. The pool's models: m2-pool's protocol under a sync shim that yields and spins at random before
#     each atomic step, in release, many times (core/pool/tests/models.rs). The crate has no
#     dependencies, so this builds in seconds. loom isn't used: it isn't in the offline registry.
begin "pool models" "1 min"
if quiet pool-models.log cargo test --locked --release -p m2-pool --features models &&
   quiet pool-lint.log cargo clippy --locked -p m2-pool --features models --all-targets -- -D warnings; then
  record "pool models" PASS "$(tally "$OUT/pool-models.log" pool-)"
else
  record "pool models" FAIL "see pool-models.log and pool-lint.log"
  failures pool- "$OUT/pool-models.log" >> "$OUT/failed"
fi
```

No retry: a model failure is a bug. Same `$TARGET`: cargo keeps the feature build apart, so nothing else
rebuilds, and it is a few MB. Budget: under a minute on Linux. The Mac runs the same row.

### 4.7 Checks and commits

Checks: `cargo test --locked -p m2-pool`; the row's two commands; the five mutants shown red. Commits:
1. "A pool of threads that runs a graph of tasks (m2-pool)", with its unit tests and grep test.
2. "The pool's models, and their row in the check".
3. "Build notes: slice 1".

---

## 5. Slice 2: the executor, offline

About a day. Live playback goes through the executor on one thread (`Runner::one()`), so every live test is
unchanged. Offline renders get threads.

### 5.1 `m2-runtime`

**`Exec::reads(&self, f: impl FnMut(usize))`** (`lib.rs`): each node whose buffer this node reads during a block.

| Exec | Reads |
|---|---|
| `Kernel` | each `Input::Port` `src.node` (its buffer and its version) |
| `Plugin` | `audio_in`, and each parameter's `Input::Port` |
| `Missing` | its input, if a port |
| `Poly` | each capture's `node` |
| `Fade` | `old` and `new`, if ports |
| `Overlay` | `src.node` |
| `Feedback` | **nothing**: its input is read in the post-pass |
| `Control`, `Audio`, `Notes`, `Time`, `Const` | nothing (lanes and `Block` are frozen) |
| `NoteField`, `Capture` | voice graphs only |

**Labels** (in `Renderer::build`, after every `rewire`): one module name per exec node.
- plan node `i < n`: `flat.nodes[i].module`;
- ghost `n + g`: `w.ghosts[g].1.module`;
- fade `f`: the module of the node that reads it (from `w.rewire`); the output's fade: its `new` side's node;
- overlay: the module of its `src` (Q4).

**`core/runtime/src/parts.rs`** (new):

```rust
pub(crate) struct Schedule {
    tasks: m2_pool::Tasks,       // one task per partition
    nodes: Box<[u32]>,           // exec indices grouped by partition, each group in plan order
    starts: Box<[u32]>,          // partition p is nodes[starts[p]..starts[p + 1]]
    part_of: Box<[u32]>,         // by exec index
    pos: Box<[u32]>,             // by exec index: its place in plan order (which error is reported)
    labels: Box<[(Box<str>, u16)]>, // by partition: module and depth (profile, tests)
    errors: Box<[UnsafeCell<Option<(u32, anyhow::Error)>>]>, // by partition, written only by its task
}
impl Schedule {
    pub(crate) fn build(order: &[usize], exec: &[Exec], module_of: &[&str]) -> Schedule;
    #[cfg(test)] pub(crate) fn from_cuts(order: &[usize], exec: &[Exec], cuts: &[usize]) -> Schedule;
}
```

Build: intern modules (`BTreeMap`, so numbering is the same each time); quotient edges from `reads()`; Tarjan over
modules; inside a strongly connected component, depth `d(i) = max(d(j) + [module(j) ≠ module(i)])` over its reads,
in plan order (design-8a §3.2); costs: kernel 2, plugin 40, poly `voices × voice kernels`, fade, overlay and copies
1, the rest 0. Debug builds check: the order respects every read, and every read is in the node's own partition
earlier in plan order or in a partition with a direct edge to it. Built in `prepare` and `Renderer::new`, off the
audio thread, kept in `Plan.schedule`, retired with the plan.

**`core/runtime/src/view.rs`** (new):
- `pub(crate) trait Reads { fn buf(&self, node: usize, port: usize) -> &[f32]; fn version(&self, node: usize) -> u64; }`
- `GraphView<'g>`: raw pointers to `exec`, `bufs`, `versions`, `poisoned`, taken once from `&'g mut Graph`;
  `unsafe impl Sync`. `unsafe fn own(&self, i) -> Own` (that node's exec, buffers, version and `poisoned` flag,
  `&mut`). `fn reader(&self, sched, me) -> Reader`. **Never a reference to a whole `Vec` while a run is on**, only to
  single elements: so a `&` to node j beside another thread's `&mut` to node i is sound.
- `Reader` implements `Reads`; debug builds check each read as above (own partition earlier, or a producer).
- `Local` implements `Reads` over a voice graph's `&self.bufs`, as today's voice loop.

**`step()`**: the body of today's `Graph::process` loop, taking `Own`, `&impl Reads`, the outer view for
captures (`&dyn Reads`), `Block`, `Seg`, the note and the lanes. `KernelNode::process`, `PolyNode::process`,
`PluginNode::process`, `plugin_events`, `fade` and `overlay::process` take a `Reads` in place of
`&[Vec<Buf>]` and `&[u64]`. Voice graphs keep their loop (`Graph::process`) over `step()`.

**`core/runtime/src/fpenv.rs`** (new; `cfg(target_arch)`, which PLAT-2 allows, and `asm!`):
- `canon()`: `MXCSR = 0x1F80` on x86-64, `FPCR = 0` on arm64. `is_canon()`.
- `Canon::enter() -> Canon` saves the caller's mode, sets the canonical one, and puts the caller's back on drop
  (unwinding too).
- Any other CPU: `compile_error!` naming RENDER-2a.

**The executor** (`lib.rs`):

```rust
pub fn render_block_on(&mut self, clock: u64, runner: &mut Runner) -> Result<&[[f32; 2]; BLOCK]>
// _mode = Canon::enter(); transport; cells; broken → silence;
// runner.wake(width) first, then lanes, the trace, Block (the pre-pass);
// runner.run(&schedule.tasks, &task): task(p) = canon(); for i in p's nodes { step(own(i), reader(p), ..) }; debug is_canon();
// after the join, a panic's payload is raised again (resume_unwind): the player crashes, as today;
// post-pass: feedback rings, the lowest-position error (non-strict: broken, as today), output, taps.
pub fn process_on(&mut self, out: &mut [[f32; 2]], runner: &mut Runner) -> Result<()>;
pub fn render_on(&mut self, seconds: f64, runner: &mut Runner) -> Result<Vec<[f32; 2]>>;
pub fn seek_on(&mut self, to: u64, runner: &mut Runner) -> Result<()>;
// render_block, process, render, seek: the same with &mut Runner::one().
```

Also:
- `fpenv::canon()` after `host.process` (`lib.rs` ~729) and `host.reset` (~754).
- `plan.rs`: `Plan.schedule`; `order_after` deleted; `end_fade` puts `Exec::Const(0.0)` in every ghost slot and
  every window-only overlay (`window_overlays`, `window_held`), and swaps nothing. `set_overlay` is never called on
  them after the window (`set_overlays` runs only before a plan is sent).
- `engine.rs`: `Engine::process_on(out, inbox, retire, stamp, runner)`; `process` calls it with `Runner::one()`.
  Used by the window tests now and by the player in slice 3.
- `threads.rs` (new, small): `pub fn setup(background: bool) -> m2_pool::Setup`: the `start` hook calls
  `wasmtime::Engine::tls_eager_initialize()`, `fpenv::canon()`, and if asked `m2_os::rt::background_thread()`
  (a refusal is kept in an atomic, for the gate to print).
- Gates-only, `#[doc(hidden)]`: `Renderer::time_phases(bool)` and `last_phases()` (pre, graph, post, in ns);
  `Renderer::profile_parts(blocks) -> Vec<PartTime>` (each partition timed, run serially);
  `Renderer::one_partition(bool)` (the whole order as one task, for the executor's own cost line).
- `pub use m2_pool::{Pool, Runner, Setup}` from `m2-runtime`, so callers need one dependency.

### 5.2 `m2-os`: the `rt` module

| Item | macOS (`mac/rt.rs`) | Linux (`linux/rt.rs`) |
|---|---|---|
| `pub struct Cores { pub fast: usize, pub slow: usize, pub per_cache: usize }` | — | — |
| `pub fn cores() -> Cores` | `hw.perflevel0.physicalcpu`, `hw.perflevel1.physicalcpu` (0 when absent), `hw.perflevel0.cpusperl2`; `hw.physicalcpu` on a Mac with one kind of core | physical cores (unique package and core id) inside `sched_getaffinity`, cut to the cgroup v2 `cpu.max` quota (rounded down, at least 1); `per_cache` = the most physical cores sharing one last-level cache inside the mask; `slow` = 0 |
| `pub fn background_thread() -> io::Result<()>` | `pthread_set_qos_class_self_np(QOS_CLASS_BACKGROUND)` | `setpriority(PRIO_PROCESS, gettid, 10)` (raising nice is always allowed) |
| `process::load_average() -> Option<[f64; 3]>` | `getloadavg` (`unix.rs`) | same |
| `process::waited() -> Option<Duration>` | `None` | time this process's threads waited for a CPU (`/proc/self/task/*/schedstat`) |

`machine()` is unchanged; the gates print `cores:` on a line of its own (refresh R11).

### 5.3 Offline renders: how many threads

`crates/editor/src/render.rs`:

```rust
pub enum Threads { Alone, Beside, Exactly(usize) }   // Exactly: 1 to 64
impl Threads {
    pub fn from_env() -> Result<Option<Threads>>;      // M2_THREADS; "0" or "x" is an error in plain words
    pub fn count(self, cores: &Cores) -> usize;        // Alone: fast (or per_cache, by the spike); Beside: slow, at least 1
}
// Render::new, lending and range take `threads: Threads`. A song with plugins renders on 1 thread (this track).
// Render owns its (Pool, Runner); beside playback the workers run at background_thread().
// Rendered gains `threads`: how many were used.
```

| Who renders | Threads |
|---|---|
| `m2 render` (exact, to a file) | `--threads N`, else `M2_THREADS`, else `Alone` |
| `m2 render --preroll` (an agent's range render, maybe beside the app) | `--threads N`, else `M2_THREADS`, else `Beside` |
| `m2 bounce` | as `m2 render` |
| The server's Export Audio, pure song | `M2_THREADS`, else `Beside` while the player plays or rings (`!Player::idle()`), else `Alone` |
| The server's export that borrows plugins (it holds playback) | 1 (plugins: this track) |
| The server's agent render (`graph/actor.rs` `render`) | `M2_THREADS`, else `Beside` |
| Tests | `Exactly(n)` always; never the machine's count |
| The live and silent players | 1 (slice 3 reads `M2_THREADS` for them) |

On this server: `Alone` = 16, `Beside` = 1. On the M1 Pro (*estimate*, the Mac prints it): `Alone` = 8, `Beside` = 2.

`apps/m2`: `--threads N` for `render` and `bounce`; the USAGE line; the output says it: "rendered 6.0s to out.wav
in 0.52s (11× real time) on 8 threads, peak −3.1 dBFS", or "on 1 thread: songs with plugins render on one for now".
Bounce uses `Renderer::render_on` with a pool from the same rule.

`crates/server`: a small pure function `export_threads(pure, idle, lending) -> Threads` in `export.rs`; three call
sites in `graph/actor.rs` (around lines 1228, 1258, 1261) pass the choice.

### 5.4 The offline gate: `m2-gates realtime --offline`

`crates/gates/src/realtime.rs` (new; G12's offline part); registered in `main.rs` as `(false, "realtime")`; USAGE
and `README.md`. Options: `--offline` (the only mode this track builds; without it: "live runs come with slice 3"),
`--reference 50` (default), `--sweep LIST` (default `1,2,3,4,6,8`), `--seconds S` (default 20).

| Part | What it prints | Verdict |
|---|---|---|
| Set-up | `machine:`, `cores: 8 fast (4 per L2), 2 slow`, the load average, background refused or not | info |
| Sweep | for each count, two passes (the second in reverse order): mean, p50, p99, max block; speed-up by mean and by p99; pre, graph, post; tasks on workers (share); time waited for a CPU (Linux) | info |
| Bits | each count's output hash against 1 thread's | **judged: FAIL if any differs** |
| Paced | 1 thread and 4: one block every 2,667 µs, `rest()` after each, as live will: mean, p99, blocks over the period | info |
| Partitions | count, width, total work, critical path, the bound at 2, 4 and 8 threads (work ÷ n or the critical path, the larger), the ten slowest partitions (mean, p99), the song tail | info |
| The executor's own cost | per-module partitions against one partition, both on one thread, in paired turns (`paired::in_turns`, three sides: modules, one partition, one partition again as the A/A control), by the mean, µs a block | info |
| Sends | `p50-sends` (§5.4.1) on 1 and 4 threads: mean, p99, when the `verb` partition starts | info |
| Tail (OS-49) | `songs::tail(Fx::LongReverb)`: the mean block 1 s and 30 s after the note ends, and their ratio | info |

Unit tests: `the_sweep_fails_when_a_count_renders_other_bits` (the verdict function on made-up hashes);
`a_sweep_list_is_read_and_nonsense_refused`; `the_help_says_what_realtime_offline_measures`.

#### 5.4.1 `p50-sends`

`core/session/src/reference.rs`: `Reference` gains `sends: bool` (`P16`, `P50` set it false; `name()` adds
`-sends`). With it, tracks 0 to 19 also send into a `verb` return track and tracks 20 to 27 into a `drums` bus, both
mixed into `song`. Test: `the_sends_variant_reads_twenty_tracks_into_verb` (its IR). This part is the first to cut if
time runs short; it then moves to slice 4.

### 5.5 Tests (each shown red once, logged in the build notes)

| Test | Where | Proves | Fails without |
|---|---|---|---|
| `every_pure_fixture_renders_the_same_bits_on_1_and_n_threads` | `core/play/tests/bit_identity.rs` | first-song, resample, sounds, starter, empty: whole song plus 1 s, offline, on 1, 2, 3, 4 and 7 threads with spread; equal bits; tasks on workers > 0; edges across threads > 0 when the plan has two partitions or more | the executor, the edges, spread |
| `p16_renders_the_same_bits_on_1_and_4_threads` | same | 30 s of p16 in CI; `p50_…` `#[ignore]`, recorded | same |
| `random_convex_partitions_give_the_same_bits` | `core/runtime/src/tests/parts.rs` | 200 seeds: a random graph of std kernels (with a feedback), a random topological order cut into random runs (`Schedule::from_cuts`), 64 blocks on 4 and 7 threads with spread, against the 1-thread render; tasks on workers and edges across threads asserted | the happens-before edges |
| `a_track_is_one_partition`, `modules_that_read_each_other_split_by_depth`, `every_edge_goes_forward`, `the_schedule_is_the_same_for_the_same_plan`, `the_most_critical_partitions_go_first`, `a_chain_has_width_one` | `core/runtime/src/tests/parts.rs` | §5.1's build; `every_edge_goes_forward` over random DAGs with random labels, cycles between modules included | each step of the build |
| `ghosts_fades_and_overlays_take_their_labels` | same | an activation with a ghost, a waiting fade, a fade-in and overlay sites: each label as §5.1 says | the labelling |
| `reads_lists_every_read` | same | the `Reader`'s debug check, which every test above runs, plus one test rendering every fixture and an edit window in a debug build: the check never fires. Shown red by taking `Overlay` out of `reads()` | `reads()` |
| `cross_module_feedback_captures_and_sidechains_on_n_threads` | same | a feedback whose input is in another partition; a capture into a voice from another module; `follow(kick.out)`; 1 against 4 and 7 | edges across modules |
| `strict_names_the_first_failing_node_on_any_thread_count` | same | two failing kernels in independent tracks: the error names the one first in plan order, on 1, 4 and 7 threads | the lowest-position rule |
| `a_failing_node_stops_only_what_reads_it` | same | a node fails in track `a`; track `b`'s output that block is what a clean render gives (it ran) | the executor (the old loop stopped at the failure) |
| `an_edit_window_renders_the_same_bits_on_1_and_n_threads` | `core/runtime/src/tests/live/keep.rs` | `Engine::process_on` on 1 against 4 with spread, sample for sample, through an activation with ghosts, waiting fades, fade-ins, a kept poly, a ghost lane, a cycle between modules that exists only in the window, and a muted track deleted while muted (`window_held`) | labels for step 7's and 8b's nodes; `end_fade` |
| `ghosts_and_window_overlays_go_quiet_after_the_window` | same | after the window those slots are `Const(0.0)`; the bits match the window-free render | `end_fade`'s change |
| `a_flush_to_zero_set_by_the_caller_changes_no_bit` | `core/runtime/src/tests` | the test thread sets FZ and DAZ (`fpenv`'s `cfg(test)` helper), renders a chain that makes subnormals (`scale` by 1e-30, then 1e-10, then 1e40), equal to the render without; afterwards the thread's mode is FZ again | `Canon::enter` and its restore |
| `a_flush_to_zero_left_by_a_plugin_changes_no_bit` | `core/play/tests/bit_identity.rs` | a new probe id `m2.probe.sets-fz` (its own FZ setter in `core/plugins/src/probe.rs`) runs before the same chain in its track; output equals the chain's without the probe | `canon()` after plugin calls |
| `the_executor_view_and_fpenv_never_block` | `core/runtime/tests/no_blocking.rs` (new) | grep over `view.rs` and `fpenv.rs`: no allocation, lock, channel, thread-local or I/O | a guard: shown red once with a `vec!` put in `view.rs` |
| existing `no_alloc.rs`, `live.rs`, `mute.rs`, the audition, overlay and tap tests | `core/play`, `core/runtime` | the live engine now renders through the executor on one thread: no allocation, same bits | — |
| `cores_are_read_from_sysfs`, `the_affinity_mask_and_quota_cut_them`, `cores_here_are_sane` | `core/os/src/linux/rt.rs` | a fake sysfs tree with this CPU's shape (2 caches × 8 cores × 2 threads): 16 fast, 8 per cache; mask `8-15`: 8 and 8; mask `8,24`: 1; quota 2.5: 2 | the parser |
| `a_background_thread_is_nice_10` (Linux), `a_background_thread_has_background_qos` and `cores_on_this_mac_are_sane` (Mac) | `core/os` | the calls work; the Mac ones run in the owner's check | the calls |
| `a_render_on_4_threads_writes_the_same_file`, `a_song_with_plugins_renders_on_one_thread`, `m2_threads_is_read_and_nonsense_refused` | `crates/editor/src/render.rs` | the same bytes; the count used; the parse | `Threads` |
| `render_takes_threads_and_says_how_many` | `apps/m2/tests/render.rs` | `--threads 1` and `--threads 4` write the same file and print "on 4 threads"; `M2_THREADS=2` prints 2; `--threads` wins; `--threads 0` refused | the flag |
| `an_export_picks_its_threads` | `crates/server/src/export.rs` | the choice table in §5.3 | `export_threads` |
| the renders row | `ci/check.sh` | the debug `m2` now renders on the machine's `Alone` count (16 here); its 10 files must still match `e2e/renders.sha256` | — |

### 5.6 Contract edits (`docs/architecture.md`, in the slice's last commit)

| Rule | The edit |
|---|---|
| RENDER-2 | Replace "every sum (buses, `poly` voices, overlay mixes) runs in plan order in the consuming partition" with: "A plan's top-level nodes are split into partitions, each run as one task per block, in plan order, on one thread: one per module (a track, or the song). Where modules read each other both ways, their nodes are split further by the number of module crossings that lead to them, so no dependency path leaves a partition and comes back. A fade belongs to the node that reads it, a ghost to its old module, an overlay to the track it carries. A node reads only buffers its inputs wrote this block; feedback reads the last block. No two partitions write the same memory. Every sum runs inside one node: buses as the graph's own adds, a `poly`'s voices in voice order into a zeroed buffer. When a node fails, what reads it is skipped and the other partitions still run that block; the error reported is the one first in plan order." *Today:* "Since 8a's offline half, offline renders run on any number of threads with the same bits (`every_pure_fixture_renders_the_same_bits_on_1_and_n_threads`, each render checked to run tasks on worker threads). Live playback renders through the same executor on one thread." |
| RENDER-2a (new) | "Every thread that renders MUST compute in one floating-point mode: round to nearest, subnormals kept, no default NaN (MXCSR `0x1F80` on x86-64, FPCR 0 on arm64). The engine sets it at the start of each block it renders and before each partition, and again after each plugin call, and puts the calling thread's own mode back at the block's end." *Why:* "WebAssembly requires subnormals; a plugin may leave flush-to-zero set for the next kernel on its thread; a thread we didn't start keeps its own mode." *Today:* "since 8a's offline half (`core/runtime/src/fpenv.rs`; the two flush-to-zero tests). The player's own mixing outside a block (the resampler, the 5 ms fade between engines) comes with live playback on all cores." |
| RENDER-32 *Why* | add: "A `Store` is entered by one thread at a time, so per-instance stores let any partition run on any thread." |
| RENDER-28 *Today* | add one sentence: "Since 8a's offline half the live engine renders each block through the partition executor on one thread (`Runner::one`: no thread, no heap); `no_alloc.rs` runs it under the trap." |
| Gate 8's row | add: "*8a's offline half:* each pure fixture renders the same bits on 1, 2, 3, 4 and 7 threads, on Linux." |

HOST-2, RENDER-28's words, RENDER-28a, the Deferred rows and gate 8's provisional words wait for slices 3 and 4.
`design-8a.md` gets a short "As built (offline half)" note at its top listing the six decisions of "In short"; `song-as-program.md` waits
for slice 4.

### 5.7 Checks and commits

Per commit: the touched crates' tests with a scratch HOME (`HOME=/home/jerry/m2-runs/8a-scratch/home
M2_AUDIO=none cargo test --locked -p m2-pool -p m2-runtime -p m2-play -p m2-editor -p m2 -p m2-os -p m2-gates
-p m2-session -p m2-server -p m2-plugins`), workspace clippy, and for `core/os` the Mac lint
(`CC_aarch64_apple_darwin=ci/stub-cc AR_aarch64_apple_darwin=ci/stub-ar cargo clippy --locked --target
aarch64-apple-darwin -p m2-os -p m2-runtime -p m2-plugins --all-targets -- -D warnings`), and the renders
(`M2=target/debug/m2 e2e/baseline.sh --tree . <out>`, then `shasum -a 256 -c e2e/renders.sha256` there).

Commits, each green:
1. "Each node says what it reads; a plan's partitions are built with it" (`reads()`, labels, `parts.rs`, its tests;
   built but not yet used).
2. "Blocks render through the partition executor, on one thread" (`view.rs`, `step()`, `render_block_on`,
   `order_after` gone, `end_fade`; every existing test unchanged).
3. "One floating-point mode on every thread that renders (RENDER-2a)" (`fpenv.rs`, the probe id, two tests).
4. "Offline renders on many threads give the same bits" (`*_on`, `Engine::process_on`, spread, the 1-vs-N tests).
5. "m2-os says how many cores there are, and lowers a thread's priority".
6. "m2 render --threads, M2_THREADS, and exports beside playback on the slow cores".
7. "m2-gates realtime --offline: the sweep, paced blocks, partitions, the executor's cost" (and `p50-sends`).
8. "Contract: RENDER-2, RENDER-2a, RENDER-32; build notes: slice 2".

**The track's last check**, once: from `~/m2-runs/wt/8a`,
`TW_WAIT=3500 ~/m2-runs/tw check-8a 'M2_CHECK_OUT=$HOME/m2-runs/check-8a ci/check.sh --gates'`. Then the offline
gate on Linux, twice, with `uptime` first: pinned to CCD1 (`taskset -c 8-15`) and not pinned:
`TW_WAIT=1800 ~/m2-runs/tw 8a-offline 'uptime; HOME=/home/jerry/m2-runs/8a-scratch/home M2_AUDIO=none taskset -c 8-15 target/release/m2-gates realtime --offline --reference 50'`.
Numbers and log paths go in build notes "Slice 2". Then HANDOFF: one short entry (8a's offline half on branch `8a`,
Linux green at `<commit>`, the Mac sweep owed).

---

## 6. The Mac run after this track (about 5 minutes; required before slice 3)

For the owner. Plugged in, lid open, other apps closed, in Terminal on the Mac itself.

| # | Do this | Why |
|---|---|---|
| 1 | Type the first line below and press Return | gets 8a's offline half from `main` |
| 2 | Type the second line and press Return | builds the gates (about 2 minutes), then runs the sweep (about 3) |
| 3 | Type the third line and press Return | the core counts, to check what the gate read |
| 4 | Paste back what is in `~/m2-8a-offline.txt` | that is all the lead needs |

```sh
cd ~/dev/suite2/music2 && git fetch && git switch main && git pull
cargo run --locked --release -p m2-gates -- realtime --offline --reference 50 2>&1 | tee ~/m2-8a-offline.txt
sysctl hw.perflevel0 hw.perflevel1 | tee -a ~/m2-8a-offline.txt
```

No `--like-m1`: this run measures the M1 Pro as it is (the Attack's §A.7).

What it decides. S = 1-thread p99 ÷ 4-thread p99, back-to-back, the median of the two passes, on the M1 Pro:

| Reading | Decision |
|---|---|
| The bits line says FAIL | Stop. A bug: fixed on Linux before anything else |
| S ≥ 2.5 | **Go:** slice 3 as planned, 4 live threads |
| 2.0 ≤ S < 2.5 | **Go, R1 first:** slice 3 starts with "a mixer strip joins its track" and sticky placement (if not built); the owner is told the base M1's verdict is at risk |
| S < 2.0 | **No-go for live:** no slice 3; an R1 study (which partitions, bandwidth or TLB) and a note to the owner |
| A count under 4 beats 4 | the live default is that count |
| Paced 1-thread p99 more than 15% above back-to-back | "spin between callbacks" moves into slice 3 |
| `hw.perflevel0.cpusperl2` is not 4 | the live count rule (design-8a §6.3) changes to match |
| A partition's p99 above 667 µs (25% of a block) | the trigger for voices across threads (design-8a §8) |
| The executor's own cost above 40 µs a block | trimmed in slice 3 before G7f's next Mac run (its room there is about 100 µs) |
| 8 threads beat 4 by little (< 10%) | exports alone use `per_cache` on the Mac too |

Linux's numbers from §5.7 sit beside the Mac's in build notes "Slice 2", with the machine named (PLAT-1).

---

## 7. Open questions, each with the decision to build with

The lead decides; none waits for the owner.

| # | Question | Build with |
|---|---|---|
| Q1 | loom is not in the offline registry | A chaos sync shim, std threads, many runs, each model shown red. Weak-memory bugs may only show on the Mac's run of the same row |
| Q2 | A `Wake` in `m2-os` (Attack) or none? | None. The caller never parks (it sleeps in steps on a stuck task); workers use std's `park`. Say in build notes that platform §8's `Wake` was not needed. Slice 3 measures std's wake on a Mac real-time worker (R14) |
| Q3 | Raw pointers or `UnsafeCell` per node for the view? | Raw pointers from `&mut Graph`, single elements only, in one file. No Miri (no nightly here) |
| Q4 | Overlay label: consumer or source? | Source: a track's mute runs in the track's partition and shortens the serial tail. Bits can't tell |
| Q5 | Plugins on worker threads offline now? | No. One thread for songs with plugins until slice 3's truthful `is_audio_thread` and probe tests |
| Q6 | How does the CLI know the app plays? | It doesn't: no lock probe (taking the lock even briefly could turn the app read-only). Range renders default to `Beside`, full renders to `Alone` |
| Q7 | A panic in a task | Raised again on the caller after the join: the player crashes and is replaced, as today |
| Q8 | `again` or `rest()`? | `rest()` |
| Q9 | How to see the executor's cost inside G7f? | The one-partition switch and a paired line in `realtime --offline` |
| Q10 | Taps inside tasks or after? | After the join, as now (about 5 µs a block, *estimate*) |
| Q11 | `Ports` is taken | The view trait is `Reads` |
| Q12 | The feedback ring loop scans every node in the post-pass | Kept as it is (today's cost). A list of feedback nodes only if the profile shows it |
| Q13 | Window-only overlays after the window | `Const(0.0)`, like ghosts |
| Q14 | Sticky or FIFO? | FIFO, unless the spike shows sticky 5% faster at 4 threads and no worse at p99 |
| Q15 | `--sends` touches `core/session` | Yes, one field; cut first if short of time |
| Q16 | OS-49 (subnormals on x86) | Measured by the tail line. Late tail within 10% of early: closed for std (its flush rule works), said in platform §5.6's follow-up. Above: name the kernel; a std rule waits for a std bump (Deferred, with that trigger) |
| Q17 | `M2_THREADS` for the player now? | No: offline only, until slice 3 |
| Q18 | Topology in `machine()`? | No: `cores:` on its own line |
| Q19 | `Alone` on Linux: every core or one cache's? | Every fast core, unless the spike's run D says otherwise |
| Q20 | Where `fpenv` lives | `m2-runtime` (platform §8); the probe has its own FZ setter |
| Q21 | `Engine::process_on` before slice 3? | Yes: the window test needs it |
| Q22 | Timing every task | Off, unless a gate asks (`time_phases`, `profile_parts`) |
| Q23 | One thread runs partitions in queue order, not plan order | Accepted; `renders.json` and `e2e/renders.sha256` guard it |
| Q24 | G7f with the executor | G7f stays on one thread. If the executor's line reads above 40 µs a block on Linux, trim before the track ends |
| Q25 | Speed cuts 3 and 4, if they land later | Cut 3: a voice pool's store is still one box in one partition. Cut 4: keep its scan in the tasks (Attack §A.4) |

---

## 8. Risks for this track

| # | Risk | What is done |
|---|---|---|
| T1 | The spike says no-go | The track stops after slice 1 (the pool is still useful); the lead decides |
| T2 | The executor's own cost eats G7f's room on the Mac | The paired line in slice 2; a 40 µs a block budget; the Mac sweep prints it |
| T3 | Other agents load the server during measurements | `uptime` and CPU waits logged; repeat once; Linux numbers are information (PLAT-1) |
| T4 | `crates/server`, `docs/architecture.md`, `HANDOFF.md`, `Cargo.lock` or `ci/check.sh` conflict with `8b-ui` or `taste-next` | Small, local edits; merges only; for `Cargo.lock`, take main's and let cargo add `m2-pool` |
| T5 | A 1-vs-N test flakes on a busy machine | Spread asserts are summed over whole renders (thousands of blocks); no time bounds anywhere |
| T6 | The models miss an ordering bug on x86 | Said in the row's doc; the Mac's check runs them on arm64 |
| T7 | Disk | One target in `~/m2-runs/wt/8a`; the spike's deleted after slice 0; `df -h /` at each slice's end |
| T8 | The renders row now renders on 16 threads in a debug build | A free cross-check against the 1-thread records. If the row gets slow, it sets `M2_THREADS=4`; decide from its time |

---

## 9. Order, time and where things go

| Step | Time (agent, *estimate*) | Depends on |
|---|---|---|
| Slice 0 (spike) | half a day | — |
| Slice 1 (pool) | half a day, beside slice 0 | sticky or FIFO: from slice 0 (FIFO first; sticky is a change to `pop` only) |
| Slice 2 (executor offline) | about a day | slice 1; slice 0's go |
| Last check, Linux offline gate, build notes, HANDOFF | half a day | slice 2 |
| The owner's Mac sweep (§6) | 5 minutes | the lead merging `8a` to `main` |

Build notes: `docs/designs/step8/build-notes-8a.md`, sections "In short", "Slice 0: the spike", "Slice 1: the pool",
"Slice 2: the executor offline", "The last check", "The Mac sweep (owed)". Each holds its numbers, the load at the
time, and the log paths (`~/m2-runs/logs/`, `~/m2-runs/s8a/spike/`, `~/m2-runs/check-8a/`). Committed with its slice.

---

## 10. Files read

`~/m2-runs/s8a/refresh.md` (with its Attack); in `~/m2-runs/wt/8a` at `e571a5f`: `HANDOFF.md`,
`docs/architecture.md` (RENDER-2, 22a, 28, 32, 33, MON-1, HOST-2, PLAT-1 to 7, the gates),
`docs/designs/step8/design-8a.md`, `backend-plan.md`, `docs/designs/platform/design.md` §5.6 and §8,
`core/runtime/src/lib.rs`, `overlay.rs`, `taps.rs`, `cells.rs`, `engine.rs`, `plan.rs` (`end_fade`), `keep.rs`
(`Wiring`), `core/play/src/lib.rs` (diff), `load_tests.rs`, `core/play/tests/bit_identity.rs`, `common/mod.rs`,
`core/os/src/lib.rs`, `thread.rs`, `process.rs`, `tests/one_place.rs`, `crates/editor/src/render.rs`,
`apps/m2/src/main.rs`, `crates/server/src/graph.rs` (diff), `export.rs`, `crates/gates/src/main.rs`, `store.rs`,
`paired.rs`, `report.rs`, `songs.rs`, `core/session/src/reference.rs`, `ci/check.sh`, `ci/gates.txt`, `Cargo.toml`,
`Cargo.lock`. The diff `ed7fc8e..e571a5f` over the paths in §1. This server: sysfs caches and siblings,
`perf_event_paranoid` (4), no `perf`, no Miri, no loom in `~/.cargo/registry`, `ulimit -r` and `-e` 0, load 1.0 to
1.6 while reading.

---

## Attack (plan)

Written 2026-10-17, read-only: the code at `e571a5f` (the 8a worktree) and `main` at `d7207d2`. Nothing built or run.
Load while reading: 0.25, 0.46, 1.01.

### In short

- **The base is stale.** `main` is 113 commits past `e571a5f`: 8b-ui and taste-next landed, with 8b.3's gain on every
  mixer channel (`std/src/mix.ts`, `std.wasm`). The spike would measure a song tail that no longer exists.
- **Three things in the plan can't work as written.** The spike skips overlay nodes and can hang on a module cycle.
  The FZ test multiplies by `1e40`, which is not an `f32`. Two pool tests need `counters()` and `wake` while `run`
  holds `&mut Runner`.
- **One premise is wrong.** `m2 render` already takes the project lock for a moment (`journal_problems`), and writers
  retry for 250 ms (`lock::RETRY`, C19). So the CLI can see that the app holds the project, and should render
  `Beside` then.
- **The Mac run proves less than the plan says.** It runs none of the arm64-only code's tests, and its one judged line
  (1 thread against N) can't see a wrong `FPCR` value, as both sides set the same one.
- **Two gates can't see what they measure.** The OS-49 tail stops long before std's state could go subnormal. The
  executor's cost line never compares with the old loop.
- **Flake and hang risks:** "edges across threads > 0" isn't guaranteed; spread hangs when no worker can take roots;
  several spinning pools share one test binary with a 2 s wait in it.
- No bit change found. No deadlock found in the protocol as described, beyond the spread case.

### A.1 Must change before building

| # | What is wrong | Where | Why it matters |
|---|---|---|---|
| 1 | The base is `e571a5f`; `main` is `d7207d2`. Changed since: `std/src/mix.ts` and `std.wasm` (a `gain` node on every channel, a `db` on master), `apps/m2/src/main.rs` (+121: track and layout commands, USAGE), `graph/actor.rs` (the export lines moved about 10), `core/play/src/lib.rs`, `docs/architecture.md`, `HANDOFF.md`, `Cargo.lock` | §1, §3 | p50's serial tail in `song` is now 50 gains, the sum and master: the spike's run F and the go rule read it. Merge conflicts grow with every commit made on the old base |
| 2 | The spike takes partitions "from `Flat.order`". The graph's order is `w.order` after `overlay::place`, which adds an overlay node at every place a track enters `song`, in every plan, offline too (MON-1). Those nodes aren't `FNode`s, so they have no module | §3.2 | Skipped overlays leave their consumers reading zeros. The spike's 1-thread and N-thread runs are wrong alike, so its hash check passes |
| 3 | One partition per module, with no depth split, is cyclic wherever two modules read each other | §3.2 | The pool then never runs those tasks: a hang, not a mismatch |
| 4 | The FZ tests' chain scales by `1e-30`, `1e-10`, then `1e40`. `1e40` overflows `f32` (max 3.4e38): the constant is `+inf` | §5.5 | The output is inf or NaN, and the strict render fails on any thread count: the test can never pass |
| 5 | Q6 says a lock probe "could turn the app read-only". It can't: writers retry a held lock for 250 ms for exactly this (`core/session/src/lock.rs`, `RETRY`), and `m2 render` already probes through `compile()` → `journal_problems` | §5.3, Q6 | An agent's exact `m2 render` while the owner plays would take every P-core at default QoS beside live audio |
| 6 | §6 runs only the gate and `sysctl`. Q1, T6 and M5 rely on "the Mac's check runs the models on arm64"; `fpenv`'s arm64 `asm!`, the probe's arm64 FZ setter, `cores()` and `background_thread()` on macOS first run there too | §6 | The Bits line compares 1 thread with N; both go through the same `canon()`. A wrong FPCR value (FZ or DN left set) passes it, and live playback on `main` would differ from every record |
| 7 | The per-commit test command and the spike set `HOME` to a scratch folder without `CARGO_HOME` and `RUSTUP_HOME` | §3.1, §5.7 | rustup and cargo then look under the scratch HOME: no toolchain, no registry. `ci/check.sh` exports both itself (line 79), so only the plan's own commands break |

### A.2 Tests and gates that can't fail, can't pass, or flake

| # | Problem | Where |
|---|---|---|
| 8 | `idle_workers_sleep_while_a_task_blocks` polls `counters()` during a run, and `a_worker_held_off_wakes_once` calls `wake` during a run. Both are `Runner` methods, and `run(&mut self)` holds the only `Runner` | §4.2, §4.4 |
| 9 | "Edges across threads > 0" isn't guaranteed. The thread that finishes a producer pushes its consumer and is the first to look for work, so it usually takes it itself. Whether the spread spin comes before or after the pop decides this, and the plan doesn't say. Per seed in `random_convex_…` (64 blocks, sometimes one cut or no edge) it can be 0 legitimately | §4.2, §5.5 |
| 10 | Spread waits for workers to take every root. With no live worker (a `Runner` whose `Pool` dropped, a worker whose `start` panicked) the caller waits for ever | §4.2 |
| 11 | Several 1-vs-N tests in one binary each keep 4 to 7 threads busy (spread spins, `patience`). m2-runtime's lib tests include `keep.rs`'s `eventually` (2 s bound) beside them | §5.5 |
| 12 | The OS-49 line times a reverb tail 1 s and 30 s after the note. std flushes recursive state below 1e-15 (−300 dB, `std/kernels-src/src/fx.rs`); subnormals start near 1e-38. A size-0.98 reverb is still in the normal range at 30 s. Both readings time normal numbers, so "within 10%" closes OS-49 on no evidence | §5.4, Q16 |
| 13 | `a_background_thread_is_nice_10` fails when the tests run under `nice` above 10: going from 15 to 10 lowers nice, which needs privilege | §5.5 |
| 14 | `bit_identity`: 5 fixtures × 6 whole-song renders in a debug build with debug read checks, plus 30 s of p16 twice. That may add minutes to the "rust tests" row | §5.5 |
| 15 | `ghosts_and_window_overlays_go_quiet_after_the_window`: "the bits match the window-free render" names no render that exists once `order_after` is gone | §5.5 |
| 16 | `a_render_on_4_threads_writes_the_same_file` and the CLI test: on a song of width 1 no worker ever runs, so they prove only the flag | §5.5 |

### A.3 Bits, the audio thread, deadlocks

| # | Finding | Where |
|---|---|---|
| 17 | Some reads point at nodes outside the order: an overlay "whose source doesn't run" is placed first (`overlay::place`), and unused nodes are `Exec::Const(0.0)`. They have no partition, so `part_of` has no entry for them | §5.1 `parts.rs` |
| 18 | Today an error block returns before the feedback rings, the output and the taps (`done?`, then `return Err`). The plan's post-pass writes the rings and then picks the error. Rings are boxes the next plan takes over from a broken renderer | §5.1 executor |
| 19 | `end_fade` must replace the window-only overlays in place. `Leftovers.exec` is sized for the ghosts only (`Plan::leftovers`), so pushing an overlay there would allocate on the audio thread | §5.1, `plan.rs` |
| 20 | The abort guard also covers `Runner::one()`, where no other thread exists and unwinding is safe. Live, a debug assertion in pool code would kill the whole app in the owner's `cargo run` build, where today the player crashes and is replaced | §4.2 |
| 21 | `Tasks::new` checks acyclicity only in debug builds. In release a cycle makes `run` wait for ever on the export thread. The bottom levels need a topological order anyway | §4.2 |
| 22 | "Songs with plugins render on one thread" lives only in `crates/editor`. Tests, gates and `render_on` callers can still pass a pool with a plugin plan | §5.3 |
| 23 | A fade read only by a `Feedback` node goes with that node's module. The ring reads it after the join, so this only makes a module cycle (and a depth split) for the rest of the plan's life | §5.1 labels |
| 24 | `host.reset` is also called inside `PluginNode::process` (`reset_first`, about line 674), and `Renderer::panic()` resets plugins between blocks, outside any `Canon` | §5.1 |
| 25 | `GraphView`'s `unsafe impl Sync` hides from the compiler whether `Exec: Send` holds | §5.1 `view.rs` |

### A.4 Not green alone, and the order of commits

| # | Finding |
|---|---|
| 26 | Slice 2's commit 1 is "built but not yet used": a schedule that nothing reads is dead code, and clippy's `-D warnings` fails |
| 27 | Commit 4 needs `threads::setup`, which calls `m2_os::rt::background_thread()` from commit 5 |
| 28 | A new workspace crate changes `Cargo.lock`, and every build here is `--locked`, which refuses that. The plan says how only for the final merge (T4) |

### A.5 Measurements

| # | Finding |
|---|---|
| 29 | The executor's cost line compares per-module partitions with one partition. Both pay the executor's fixed costs (`Canon`, `run`, `catch_unwind`, the `Reads` step), which is what G7f's room pays for. `Graph::process` stays for voice graphs, so the old loop is still there to compare with |
| 30 | The spike's timed runs overlap slice 1's builds, tests, models and mutants. The same agent loads the server it waits on |
| 31 | §6's "about 3 minutes" is low: 12 sweep runs of 20 s, each with a p50 build, two paced runs in real time, paired turns and the tail. About 10 minutes is likelier |
| 32 | `p50-sends`' "when the `verb` partition starts" needs task start times, which no hook in §5.1 gives |
| 33 | Slice 2 "about a day" holds the executor, `fpenv` on two CPUs, `m2-os` on two OSes, the CLI, the server, a five-part gate and about 25 tests. Two days is likelier |

### A.6 Other tracks

| # | Finding |
|---|---|
| 34 | `polish` is in flight (`~/m2-runs/wt/pl`, from `29430e8`): Mix strips, renaming a track's key, recent projects, Save As, drum names. It likely touches `apps/m2/src/main.rs` (USAGE), `crates/server`, `HANDOFF.md`, `docs/architecture.md`, perhaps std. taste-next and 8b-ui are on `main` now, so merging `main` covers them |
| 35 | A new probe id `m2.probe.sets-fz` changes the probe's `plugin_count` and every listing of found plugins in tests. The probe already selects behaviour by a state tag (`fails-process`, `probe::tagged`) |
| 36 | The server's `Graph::render` is not an agent render: only tests and the gates' G7e use it (`git grep` on `main`) |
| 37 | Server tests export with `Alone` (16 threads here). Edition 2024 makes `set_var` unsafe, so a test can't set `M2_THREADS` in-process. Their tiny songs wake no worker, so this is spawns only |

### A.7 Small things

| # | Finding |
|---|---|
| 38 | `m2_os::rt` sits beside `m2_os::thread`, which already has `interactive()`. One function more there reads better than a new module |
| 39 | Linux `per_cache`: `index3` isn't always the last-level cache. Read each cache's `level` |
| 40 | `Pool::new` can fail (thread limit). An export should then render on one thread and say so |
| 41 | An `Alone` export that starts while the player is idle keeps its threads awake if the owner presses Play mid-export |
| 42 | The editor's `pure(flat)` counts a missing plugin as a plugin. `Renderer::hosts_plugins()` is the truer test for one thread |

### A.8 Checked and fine

- **No bit change from task order on one thread.** No step writes state another node reads: kernels, polys, plugins,
  fades and overlays write only their own exec, buffers, version and `poisoned`. Lanes, `Block` and cells are written
  before the graph. `LaneSlot`, `NoteScheduler` and `AudioPlayer` hold no `Cell`, `RefCell` or `Rc`, and the task
  closure's `Sync` bound checks them.
- **The depth split gives an acyclic partition graph.** A cycle P→Q→P inside one SCC would need d(Q) > d(P) > d(Q).
- **The error reported is the old loop's.** Every node before the first failing one runs with the same inputs and
  doesn't fail, and nothing it reads fails.
- `one_place.rs` doesn't flag `target_arch` or `asm!`. `m2-runtime` already depends on `m2-os`. `paired::in_turns`,
  `songs::tail(Fx::LongReverb)`, `Player::idle()` and `probe::tagged` exist. `std:scale` exists.
- Since `e571a5f`, `main` changed nothing in `core/runtime`, `core/os`, `core/plugins`, `crates/gates`, `ci/`,
  `core/session/tests/renders.json` or `e2e/renders.sha256`.
- An aborting child writes no core file here (`ulimit -c` 0; cores go to apport). Disk: 2.4 TB free.
- loom is not in `~/.cargo/registry`: confirmed.

## Decisions

The decision to build with, for each item above.

| # | Build with |
|---|---|
| 1 | Merge `main` into `8a` now, as a merge commit, before the spike builds. Merge again before handing back. Every number in the build notes is taken on the merged base |
| 2 | The spike runs `graph.order` from `Renderer`'s plan, with overlays labelled by their source's module (as slice 2 labels them). Each N-thread hash is compared with the old loop's output (`Graph::process`, run G), not with the spike's own 1-thread run |
| 3 | The spike checks its partition graph for a cycle before rendering and stops with a message naming the modules. p50 has none today (no track reads `song`) |
| 4 | The chain is ×1e-30, ×1e-10 (1e-40, subnormal), ×1e30, ×1e10, from a signal near 1. Flushed, it ends at 0; kept, near 1 |
| 5 | A full `m2 render` and `m2 bounce` render `Alone`, unless the project's lock is held, then `Beside`. Add `lock::held(dir) -> bool` to `core/session/src/lock.rs`: open `.music2/lock` only if it exists (never make `.music2/`), `try_lock`, drop. A test: held by a `ProjectLock`, it says true; free, false; the lock is free after. Q6 changes to this |
| 6 | The gate gets a second judged line: the five pure fixtures rendered through `Render` on the sweep's largest count, hashed as `e2e/baseline.sh` does, must match `e2e/renders.sha256`. §6 gets one more line, before the gate: `cargo test --locked --release -p m2-pool --features models`. The Mac's full `ci/check.sh` stays owed before slice 3, written so in HANDOFF |
| 7 | Every command that sets `HOME` also sets `CARGO_HOME=/home/jerry/.cargo RUSTUP_HOME=/home/jerry/.rustup`. The last check uses `M2_CHECK_OUT=/home/jerry/m2-runs/8a-scratch/check` |
| 8 | `Pool::counters(&self)` reads the shared state; `Runner::counters` stays for the gates. The held-off test calls a crate-private signal function (`#[cfg(test)]` access in `src/tests.rs`) |
| 9 | In spread mode the random spin comes before each pop, so another thread can take a pushed consumer. Each render asserts tasks on workers > 0 (certain under spread). Edges across threads are summed over a whole test, across fixtures, counts and seeds, and asserted > 0 once |
| 10 | Spread is on only while the pool has live workers; otherwise it does nothing, as with one thread. `Setup.start` runs inside the worker's abort guard |
| 11 | Tests that make a pool take one static lock per test binary, so one pool spins at a time. Watchdogs are 60 s: they catch hangs, never slowness |
| 12 | OS-49 by a count, not a clock. A runtime test renders an impulse, then 5 minutes of silence, through every stateful std kernel (delays with feedback, reverbs, filters, chorus, phaser, envelope followers, envelopes), and counts subnormal samples in every node's output. 0 closes OS-49 for std 0.2. Above 0 names the kernel, and a std rule waits for a std bump (Deferred, with that trigger). The gate prints block times at 1 s and 5 min as information only. Q16 changes to this |
| 13 | `background_thread()` on Linux sets nice to at least 10 and never lowers it. The test asserts ≥ 10 |
| 14 | Measure `bit_identity`'s time in slice 2. Over 60 s in a debug build: whole songs on 1, 2 and 7 threads, and only the first 10 s on 3 and 4 |
| 15 | That test asserts three things: the slots are `Const(0.0)`; `Leftovers.exec` holds exactly the ghosts; and 1 thread equals 4 through and after the window |
| 16 | Both use `examples/first-song` (5 partitions) |
| 17 | `Schedule::build` treats a read of a node outside the order as frozen: no edge. The debug `Reader` allows it. A read at or past `exec.len()` is a debug assertion |
| 18 | On an error block the post-pass reports the error and writes nothing else (no rings, output or taps), as today |
| 19 | `end_fade` uses `mem::replace` with `Exec::Const(0.0)` in place for window-only overlays and drops the old value (it holds no heap). Only ghosts go into `Leftovers.exec` |
| 20 | The abort guard applies only when the runner has a pool. `Runner::one()` lets a panic unwind, so live keeps today's crash and replacement |
| 21 | `Tasks::new` returns an error on a cycle in every build. `Schedule::build` turns it into a failed prepare |
| 22 | `render_block_on` runs every task on the caller when the plan hosts a plugin, whatever runner it is given. A test: a probe song through a 4-thread pool with spread runs no task on a worker |
| 23 | A fade read only by a `Feedback` node takes its `new` side's module |
| 24 | `canon()` after every `host.reset` too, the `reset_first` one included. `Renderer::panic()` runs inside `Canon::enter()` |
| 25 | `view.rs` holds a compile-time check that `Exec: Send` |
| 26 | Fold commit 1 into commit 2: "Blocks render through partitions, on one thread" |
| 27 | Commit order: m2-os (cores, background) first, then the runtime commits, then CLI and server, then the gate, then the contract |
| 28 | Slice 1's first build runs once without `--locked`: `cargo check --offline -p m2-pool`. `git diff Cargo.lock` must show only the `m2-pool` entry (and in slice 2, `m2-runtime`'s new dependency, added the same way). Every other build is `--locked` |
| 29 | The cost line has three sides in turns: the old loop (a gates-only switch that runs the top level through `Graph::process`), one partition, and per-module partitions. Build notes also put the last check's Linux G7f (`store`, `arena`, run by `--gates`) beside step 7's Linux run (41 of 80 µs), as information |
| 30 | While the spike times runs on CCD1 (`8-15`), this track's own builds and tests run under `taskset -c 0-7,16-23`. Log `uptime` as planned |
| 31 | §6 says "about 10 minutes". `--seconds` stays at 20 |
| 32 | Cut `p50-sends` now and move it to slice 4, as the plan already allows. Q15 falls away; `core/session` isn't touched |
| 33 | Slice 2 is planned at two days. After the merged commit 2 fixes the API, two lanes: (a) the runtime's tests and the gate; (b) m2-os, the editor, the CLI and the server |
| 34 | Keep shared files small: in `crates/server`, only `export.rs` and the call lines in `graph/actor.rs`; in `apps/m2/src/main.rs`, the render and bounce arms and one USAGE line; contract and HANDOFF edits in the last commit. Merge `main` before the last check |
| 35 | No new probe id. A state tag `sets-fz` (as `fails-process`), loaded through a state file. The probe sets FZ at the end of `process`, after its own math |
| 36 | `Graph::render` takes `Alone` like a lone export. §5.3's table calls it "the server's reference render (tests, G7e)" |
| 37 | Accepted: `export_threads` is a pure function with its own test, and tiny test songs wake no worker |
| 38 | `m2_os::thread::background()` beside `interactive()`, and `m2_os::process::cores()`. No `rt` module |
| 39 | `per_cache` reads each `cache/index*/level` and uses the highest |
| 40 | When `Pool::new` fails, `Render` renders on one thread and `Rendered.threads` says 1 |
| 41 | Accepted for this track: the thread count is chosen when the export starts. Written in build notes as a known limit, for slice 3 |
| 42 | `Render` picks one thread by `Renderer::hosts_plugins()`, after it builds the renderer |
