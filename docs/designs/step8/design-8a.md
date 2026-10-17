# Step 8a design: partitions on a thread pool

> **As built (offline half, branch `8a`).** The partitions, the pool (`core/pool`, `m2-pool`) and the executor are
> built, and every offline render uses them; live playback renders through the same executor on one thread. Six
> decisions differ from this design (the plan is `~/m2-runs/s8a/plan.md`; the build notes are `build-notes-8a.md`):
>
> | # | This design | As built |
> |---|---|---|
> | 1 | the device thread parks on a `Wake` | the caller of a run never parks: it spins, and sleeps in short steps only when a task is stuck. No `Wake`, no device-thread park |
> | 2 | loom models | loom isn't in the offline registry: the models use a small sync shim that yields and spins at random, with std threads, many times over, each shown red against a mutant |
> | 3 | an overlay goes with its consumer | an overlay goes with the track it carries; a fade with the node that reads it |
> | 4 | a panic in a task breaks the plan | it is caught, the block finishes, and it is raised again on the caller, so the player crashes and is replaced, as before |
> | 5 | edges from `Wiring`'s kept inputs | edges from `Exec::reads()`, what each built node reads |
> | 6 | plugins on worker threads | a song with plugins renders on one thread for now; plugins on workers come with live playback on all cores |
>
> Also from the spike: FIFO placement (sticky was within 2%), and an export alone uses one cache's fast cores
> (`Cores.per_cache`).

Revision 2. This design is checked against `docs/architecture.md` (normative) at HEAD `cbf5eb9`. It sits on top of the
step 7 design (`.git/m2-step7/design-7.md`, revision 2), which is built first. It uses the survey `understand-8a.md` in
this folder, and it departs from the survey wherever a simpler choice does the job; §11 lists where. Revision 2 answers
the fifteen problems raised against revision 1 (§15): every wait is now bounded, a panic outside a task aborts, the
thread count stays inside one L2 cluster, and the judged runs include the app's own path while it is edited.

Nothing was built, run or measured. Every number marked *estimate* comes from the survey's arithmetic, and §14 lists
what to measure first. No file in the repository was changed. `core/runtime`, `core/play`, `core/plugins` and `core/ir`
are unchanged in the working tree, so their line numbers hold for HEAD.

Step 8a's line in the order of work:

> **8a:** parallel partitions on a thread pool, right after the arena, with renders that don't depend on the thread count.

Its gate, and the CI gate that keeps running from 8a on:

> p50 plays without dropouts at 128 frames and 48 kHz on the reference Mac, with the audio callback at no more than 70%
> of its block budget at p99 over 10 minutes; a 64-frame run is recorded (8a). Each pure fixture renders the same bits on
> 1 thread and on N.
>
> renders don't depend on the thread count, from step 8a (RENDER-2).

---

## 0. The design on one page

**What runs in parallel.** Each plan's top-level nodes are split into *partitions*. A partition is one module (a track,
or the song). Where modules read each other in both directions, their nodes are split further by depth, so the
partitions never form a cycle (§3.2). Each partition is one task per block, and it runs its nodes in plan order on one
thread. A partition starts once every partition it reads from has finished. A `poly`'s voices stay inside its task. p50
has 51 partitions: 50 tracks of roughly 20 to 100 µs each, and the song's mixer as a serial tail of about 100 kernel
calls (*estimate*).

**Why the bits can't change.** Within a block, a node's output depends only on three things: its inputs' buffers from
this block, its own state, and inputs that are frozen for the block (timing, lanes, cells). Feedback reads the previous
block. So any order that runs each node after its inputs gives the same bits. The executor guarantees four things:
- a happens-before edge on every dependency;
- no two tasks write the same memory;
- every sum stays inside one node;
- every thread computes in the same floating-point mode.

A 1-thread render goes through the same executor, so the CI comparison of 1 against N tests the real code.

**The pool.** The pool is our own small one; rayon is not used. It has N−1 worker threads plus the device thread, which
works too. Each block goes like this:
1. The device thread wakes the sleeping workers at the block line.
2. It runs the serial pre-pass (cells, schedulers, players, timing) and publishes the job.
3. It works through a ready queue alongside the workers. When a task finishes, it decrements its consumers' dependency
   counters, and a consumer whose counter reaches zero joins the queue.
4. Once the last partition is done and no worker is still inside the job, the device thread runs the serial post-pass
   (feedback rings, output).

A worker spins only while there is work it could take, and otherwise sleeps on a Mach semaphore. The device thread spins
while partitions keep finishing. If none finishes for a whole period, a task is blocked inside a plugin, and it parks
until the block completes: a blocked plugin costs a dropout, as it does today, and no CPU. There are no allocations and
no locks. In a normal block the only system call is the wake.

**macOS.** Live workers are time-constraint threads joined to the output device's audio workgroup
(`kAudioDevicePropertyIOThreadOSWorkgroup`). There are as many as `os_workgroup_max_parallel_threads` recommends, but no
more than the performance cores that share one L2 cache, counting the device thread: 4 on the M1 and on the M1 Pro.
Offline renders (Export Audio, `m2 render`, bounce) use the same executor on plain threads, so exports get faster too.
Beside playback they keep to the efficiency cores.

**Measured how.** The player always keeps a histogram of callback durations, counts dropouts and counts each engine
thread's busy and spinning time. A dropout is any of:
- a CoreAudio overload notification;
- a callback that ran longer than its period;
- a gap between callbacks.

`m2-gates realtime` (G12) is judged on a base M1 at 128 frames, twice: the engine alone playing p50 for 10 minutes (the
gate's words), and the app's own path, the server with a browser open, while p50 is edited every 10 seconds. An export
beside playback is judged too. Runs at 64 and 512 frames are recorded.

**Three slices, each green:** the executor offline (with the 1-vs-N tests), then live playback on realtime workers, then
the gate. A fourth slice is built only if the gate fails, and it holds only measured fixes.

**Not built:** voices split across threads (p50 doesn't need it; §8 gives the trigger), parallelism inside a node, a
paced no-device mode, learned cost estimates, a thread count chosen per block, abandoning a late block, home threads for
plugins, `wfe` spinning, and Panic through the pool.

**Deleted or simplified:**
- design-7's `order_after` and the order swap at the end of a fade. There is one schedule per plan, and after the window
  ghosts are no-op constants.
- the top level of `Graph::process`, whose loop body becomes `step()`.
- every `&[Vec<Buf>]` parameter, which becomes one `Ports` view.
- the `m2-play` thread that parks with its stream forever: a replaced player now closes.

§11 has the full list.

---

## 1. What 8a stands on

8a is built on step 7's slice 2 (the arena). A throwaway measurement spike can run on slice 1 alone (§16, slice 0).

| From step 7 | What 8a does with it |
|---|---|
| One `Store` per kernel instance (design-7 §11.1). `Graph` functions lose their `store` argument | This is the precondition. A `Store` is entered by one thread at a time, so kernels in different partitions can run at once, and a moved box can run on any thread in any block |
| `Plan { graph, cells, out, entry, ghosts, fade, … }` built by `prepare` off the audio thread (§4.2, §6) | The partition schedule is built in the same place, sized there, and retired with the plan |
| Lanes run before the graph: schedulers fill events, audio players fill once per lane, and note ids are engine-wide (§8.1, §8.2) | Lanes stay a serial pre-pass on the device thread, so note ids and players stay deterministic. Every lane is read-only during the graph |
| No two-renderer crossfade, no `lend`, and activations never coalesce (§7) | One plan per block, and no instance is processed by two nodes |
| Ghosts and fade nodes appended to the plan, with Kahn's order over the window's graph and the cycle rule (§6.4 to §6.6) | Ghosts and fades are partitioned like any other node: a ghost by its old module, a fade by its consumer's (§3.3) |
| `take_over` and `end_fade` at a block line on the device thread (§7.2, §7.3) | They run between `run` calls, when the pool is quiescent by construction (§5.3) |
| `poisoned` set right after each stateful top-level node runs (§7.7) | It is set inside that node's task. Only that node is touched |
| G7f measures `render_block` on p50 on one thread (§11.2) | That is 8a's single-thread baseline |

---

## 2. The same bits on any thread count (RENDER-2)

### 2.1 The conditions

Within a block, a top-level node reads:
- its inputs' buffers, written this block by its dependencies (`FOp::deps`, `core/ir/src/check.rs:86`: port inputs, a
  poly's lane and its captures' sources, never a feedback input);
- its own state;
- inputs frozen before the graph runs: the `Block` (runs, beats, seconds, plugin run info), lane events, control lanes,
  audio player outputs, and constant buffers and their versions (`Cells::apply`).

Feedback rings are written after every node has run (`Graph::process`, `lib.rs:960`), so a feedback node always reads
the previous block. Two executions that run every node exactly once, each after its dependencies, therefore give the same
bits, provided all of the following hold. Each is a rule for the executor, and §4 and §5 show where it is kept.

1. **Happens-before on every edge.** A consumer starts only after its producers' writes are visible (§5.4).
2. **Single writer.** Each node is in exactly one partition and is written only by that partition's task: its exec, its
   buffers, its version and its `poisoned` flag.
3. **Frozen inputs stay frozen.** Everything listed above is written only by the device thread, before the job is
   published.
4. **Post-pass after the join.** Feedback ring writes, the output copy and its non-finite check, counters and the trace
   all run after every task has finished.
5. **Every sum stays inside one node** (§2.2).
6. **One floating-point mode on every thread** (§2.3).
7. **Nothing reads the time or the thread.** No voice is skipped when a block runs late, and nothing branches on the
   thread count. A late block is a dropout, never a different render. The pool's own timing (who spins, who sleeps)
   changes only which thread runs a task, never what it computes.

### 2.2 Sums

- **Buses** are std's `sum`, a left fold of `add` kernels in the caller's key order (`std/src/math.ts:65-68`,
  `std/src/mix.ts`). Each `add` is a node, so the graph fixes the order. Nothing changes.
- **`poly` voices** are summed inside the poly's own task, exactly as today: `out.fill(0.0)`, then `+=` for each voice in
  voice-index order (`lib.rs:1123-1150`). The zero fill matters: `0.0 + -0.0` is `+0.0`, so starting from the first voice
  would change sign bits.
- **Overlay gains (MON-1, step 8b)** are a node on a consumer's input, labelled with the consumer's module like a fade
  node (§3.3). So mute and solo never sum into a buffer that two tasks write.
- **Never** accumulate into one buffer from two tasks, and never add floats atomically. Completion order would change
  the sum.

### 2.3 The floating-point mode (new rule RENDER-2a)

WebAssembly requires IEEE subnormals. Cranelift emits native AArch64 float instructions, and those obey `FPCR`: FZ
(flush to zero), DN (default NaN) and RMode (rounding). Suppose a hosted plugin sets FZ in its `process` and doesn't
restore it. Every kernel run after it on the same thread then computes different bits, and nothing reports it. And
nobody has checked the mode CoreAudio's IO thread starts with: RENDER-33's test drives the callback from a test thread
(`core/play/tests/bit_identity.rs`), never the real one.

- `core/runtime/src/fpenv.rs` has two functions:
  - `canon()` writes `FPCR = 0` on aarch64 (round to nearest, no FZ, FZ16 or DN). On x86_64 it writes `MXCSR = 0x1F80`.
  - `is_canon()` reads it back.
- `canon()` runs:
  - at the top of `Callback::fill` and of `Renderer::render_block_on`, before the pre-pass. The pre-pass and post-pass
    do float work too: the audio players' fills and declicks (design-7 §8.2), `block()`'s beat and seconds, the
    Engine's and `LiveFade`'s mixing, and the resampler;
  - before every partition task, on every thread, the device thread included;
  - after every `host.process` and `host.reset` call.
  It costs a few cycles; at p50 that is about 50 tasks a block, plus each plugin call.
- Debug builds assert `is_canon()` after every task (inside its `catch_unwind`, §4.3) and in the post-pass.
- Relaxed SIMD stays off, and NaN canonicalisation stays off (`core/runtime/src/packages.rs:93-98`).

std's kernels flush their recursive state below −300 dB themselves (`std/kernels-src/src/fx.rs:31-35`), so FZ isn't
needed for speed.

---

## 3. Partitions

### 3.1 Definition

A **partition** is a set of top-level plan nodes that runs as one task per block, on one thread, in the plan's order
restricted to the set. A plan's partitions satisfy all of the following:

1. Every node in the plan's order is in exactly one partition.
2. No dependency path leaves a partition and comes back into it (convexity).
3. The graph between partitions is acyclic, with an edge P→Q when a node in Q reads a node in P.

Any partitioning that satisfies these gives the same bits (§2.1). The choice only affects speed. Partitions are derived
per plan and are never identity (P1).

### 3.2 Building them (`core/runtime/src/parts.rs`, new)

`Schedule::build(order, deps, labels, costs)` runs off the audio thread, in `prepare` and `Renderer::new`:

1. **Label** each node in the plan's order with an interned module key. `BTreeMap<&str, u32>` makes the numbering
   deterministic.
2. **Quotient graph.** For each node `i` and each dependency `j` with a different label, add the edge `label(j) →
   label(i)`. Feedback inputs aren't dependencies.
3. **Split cycles by depth.** Tracks can import each other's exports (`core/script/sdk/core.ts:1622-1632`), so the
   module graph can have a cycle even though the node graph has none (`kick` gates from `bass.env` while `bass` ducks
   from `kick.out`). Run Tarjan's algorithm over the labels. For a module in a strongly connected component with others,
   relabel each of its nodes `(module, d)`, where `d` is the largest number of module crossings on any path into the
   node: in plan order, `d(i)` is the largest `d(j) + [module(j) ≠ module(i)]` over its dependencies `j`, and 0 with
   none. An edge inside one module never lowers `d`, and an edge across modules always raises it. So every edge between
   two labels of the component goes up in `d`, the partitions are convex, and the graph between them is acyclic, with no
   merge. Modules outside such a component keep one label each, so an ordinary track is one partition.
4. **Lay out partitions.**
   - Nodes are grouped by partition, each group in plan order.
   - Partitions are numbered by the plan position of their first node.
   - A partition's cost is the sum of its nodes' costs: a kernel 1, a plugin 20, and a `poly` its voices × its voice
     kernels / 2.
   - A partition's **bottom level** is its cost plus the largest bottom level among its consumers. The partitions with
     no dependencies are listed by bottom level, highest first, with ties broken by number. Each partition's consumer
     list is deduplicated and sorted the same way, so `complete` pushes the most critical consumer first. A `verb`
     return track that 20 tracks send into then starts as soon as its sends are done, not after every other track.
   - Each partition's dependency count is the number of distinct producers.
   - The schedule's **width** is its widest layer when partitions are layered by their longest path from a root. No
     more than width − 1 workers are ever woken (§5.5).
5. **Check** (debug builds): every node's dependencies are in its own partition earlier in plan order, or in a
   partition it depends on.

The build costs O(nodes + edges): about 2,500 nodes at p50, well under a millisecond (*estimate*). It allocates, which is
fine off the audio thread. It touches no `HashMap` iteration, so a plan's schedule can be reproduced for debugging.

### 3.3 Labels for the nodes step 7 adds, and for 8b's

| Node | Label | Why |
|---|---|---|
| a plan node | its `FNode.module` | per-track parallelism |
| a ghost (design-7 §6.4) | its old node's module (`O.nodes[j].module`) | a ghost of a kept track runs in that track's partition; a ghost of a removed track gets one of its own |
| a fade node on input `p` of `i` (§6.5) | `i`'s label | edge transforms belong to the consumer (RENDER-2's "in the consuming partition") |
| the song output's fade | the label of the new output's node | the same rule |
| an overlay gain (8b, MON-1) | its consumer's label; for the song's output, `song` | the same rule; no shared sum |

The depth split of §3.2 step 3 applies to all of these alike.

### 3.4 One schedule per plan

Design-7 keeps two orders per plan: the window's (ghosts and fades included) and `order_after`, which `end_fade` swaps
in. 8a keeps **one schedule**, built over the window's graph, for the plan's whole life:
- After `end_fade`, each ghost's slot holds `Exec::Const(0.0)` (design-7 §7.3, step 1), which does nothing when it runs.
- After the window, a fade node reads only its new side (design-7 §7.4).

So the window's schedule stays correct. What it costs: a removed track's ghost partition runs empty until the next
activation (well under a microsecond a block), and a fade's partition still waits for its old producer. A cycle that
exists only in the window's graph (an edit that reverses which of two tracks reads the other) is split by depth, not
merged, so it costs a few more small partitions rather than a serialised pair of tracks. In exchange, `order_after` goes,
step 4 of `end_fade` goes, and nothing is swapped on the audio thread.

### 3.5 The data (in `Plan`, sized by `prepare`)

```rust
// core/runtime/src/parts.rs
pub(crate) struct Schedule {
    nodes: Box<[u32]>,            // top-level exec indices, grouped by partition, each group in plan order
    parts: Box<[Part]>,
    consumers: Box<[u16]>,        // Part::consumers ranges into it, most critical first
    roots: Box<[u16]>,            // no dependencies, most critical first
    width: u16,                   // the widest layer: at most width − 1 workers are woken
    part_of: Box<[u16]>,          // by exec index: debug owner checks
    pos: Box<[u32]>,              // by exec index: position in plan order, to pick the error to report (§4.3)
    run: RunState,                // per block, reset by the device thread before each run (§5)
}
struct Part { nodes: Range<u32>, consumers: Range<u32>, deps: u16, cost: u32, level: u32 }
struct RunState {
    waiting: Box<[AtomicU16]>,    // dependencies not yet complete
    skip: Box<[AtomicBool]>,      // a producer failed: run nothing, complete as failed
    queue: Box<[AtomicU32]>,      // one slot per partition (each is pushed once a block); EMPTY until written
    head: CachePadded<AtomicU32>, tail: CachePadded<AtomicU32>,
    left: CachePadded<AtomicU32>, // partitions not yet complete
    running: CachePadded<AtomicU32>, // tasks executing now (§5.5)
    failed: Box<[UnsafeCell<Option<(u32, anyhow::Error)>>]>,  // written only by the partition's own task
    #[cfg(debug_assertions)] done: Box<[AtomicU64]>,          // the block a partition completed in
    #[cfg(debug_assertions)] epoch: AtomicU64,
}
```

`CachePadded` comes from crossbeam-utils, which is already in the tree. At p50 this is a few kilobytes.

### 3.6 Worked examples

| Plan | Partitions |
|---|---|
| p50 | 50 tracks and `song`. The 50 track partitions are roots. `song` (50 gains, 49 `add`s in a chain, the limiter) waits for all 50. Width 50 |
| `examples/first-song` | `kick`, `bass`, `keys`, `hats` and `song`. `song`'s `follow(kick.out)` ducking is one more edge, kick→song |
| tracks `a` and `b` reading `b.x` and `a.y` | `(a, 0)` and `(b, 0)` side by side (each track up to its read of the other), then `(a, 1)` and `(b, 1)`, then `song`. Revision 1 merged them into one serial partition |
| p50 with track `t02`'s pluck wrapped in `delay(…)`, slice 2 names (design-7 §6.10) | the ghost of the old pluck and the new delay and pluck are in `t02`; the fade on the gain's input is in `song`. After the window, `t02` runs the ghost slot as a no-op |
| a song written entirely in `song.ts`, or `song` plus one track in a chain | width 1: one thread, and no worker is woken. Parallelism comes from tracks (§13, R8) |
| 20 tracks sending into a `verb` return track, 8 drums into a bus | the bus and `verb` have the highest bottom levels among the roots' consumers, so their producers run first and they start early (`--offline --sends`, §9.3) |

p50 on 4 threads, roughly, for one block (*estimate*). Each row is one thread:

```
device  |pre 30µs|t00 t04 t08 … (≈13 tracks, ≈575µs)          |song tail ≈80µs|post 10µs|
worker1 |wake…   |t01 t05 t09 … (≈12 tracks)                   |  (asleep: nothing it could take)
worker2 |wake…   |t02 t06 t10 …                                |
worker3 |wake…   |t03 t07 t11 …                                |
                  └────────── ≈ 2.3 ms of work / 4 ───────────┘   ≈ 0.75–0.9 ms in all, ≈ 30% of 2.67 ms
```

---

## 4. Running a partition

### 4.1 One step function, two views of the buffers

Today `Graph::process` (`lib.rs:847`) loops over the order. It `mem::take`s each node's output so that it can write that
output while reading the others through `&self.bufs`. Run on two threads, that would mean `&[Vec<Buf>]` aliased with a
`&mut Buf`, which is undefined behaviour and not merely a race.

- **`step(exec, out, version, ports, b, seg, note, lanes)`** becomes the loop's body, unchanged inside. Every read of
  another node's buffer or version goes through a `Ports` view:

  ```rust
  trait Ports { fn buf(&self, node: usize, port: usize) -> &[f32]; fn version(&self, node: usize) -> u64; }
  ```

  `PolyNode::process`, `PluginNode::process` and `plugin_events` take a `&impl Ports` in place of `bufs: &[Vec<Buf>]`
  and `outer: &[Vec<Buf>]`.
- **Voice graphs** keep today's loop (`take`, `step`, put back), with a safe `Local` view over `&self.bufs`, and their
  own feedback writes after the loop. A voice's `Exec::Capture(u16)` (design-7 §5.1) reads the outer graph through the
  `Ports` view the poly's task was given.
- **The top level** no longer loops. It runs through the schedule (§4.2), and its feedback ring writes become the
  render's post-pass.

### 4.2 The top-level view and the task (`core/runtime/src/view.rs`, new)

`GraphView` holds raw pointers to the top-level `exec`, `bufs` and `versions`. It is made from the `&mut Graph` that the
render holds for exactly the duration of one `pool.run`, and it is `unsafe impl Sync`. It has two operations. With each
partition's `failed` slot (written by its own task, read after the join), they are the executor's only unsafe code; the
rest is `fpenv`'s two instructions and `rt.rs`'s FFI:

```rust
// SAFETY: `i` is in partition `me`, which only this task runs this block.
unsafe fn own(&self, i: usize) -> (&mut Exec, &mut Vec<Buf>, &mut u64);
// SAFETY: `j` is earlier in `me`, or in a partition that completed before `me` was pushed (§5.4).
fn reader(&self, me: u16) -> Reader<'_>;   // impl Ports; debug builds check every read (§4.4)
```

A node never reads its own output during a block (the graph is acyclic, and feedback reads its ring), so `own(i)` and a
read of `j ≠ i` never alias.

The task the render hands the pool, for partition `p`:

```rust
let task = |p: u16| -> Result<(), (u32, anyhow::Error)> {
    let ports = view.reader(p);
    for &i in sched.nodes_of(p) {
        let (exec, out, version) = unsafe { view.own(i as usize) };
        step(exec, out, version, &ports, &block, Seg::WHOLE, None, &lanes).map_err(|e| (sched.pos[i as usize], e))?;
    }
    Ok(())
};
```

`Renderer::render_block_on(clock, pool)` then runs these phases:
- **Pre-pass:** `fpenv::canon()`, `pool.wake(want)`, then the transport, cells, lane schedulers and players, and the
  `Block`.
- **Graph:** `pool.run(&sched, &task, again)`, where `again` says whether another run follows in this callback (§5.5).
- **Post-pass:** feedback rings, the output and its non-finite check, and the error choice.

### 4.3 Errors and panics

- **A task stops at its first error.** The task records `(position in plan order, error)` in its partition's `failed`
  slot and completes as failed. Its consumers are marked `skip` and complete without running, and so do theirs.
  Partitions that don't depend on it run as usual.
- **The error reported** is the one at the lowest plan position. For a given plan, the set of failed and skipped
  partitions depends only on the plan's structure and the data, never on timing. So the reported error is the same on 1
  thread and on N. With a single failure, it is exactly the error that HEAD's serial loop reports.
  `strict_mode_names_the_kernel_that_broke` (`core/runtime/src/tests.rs:451`) keeps passing.
- **Live**, a failure makes the plan `broken` (silent), as today, and the node is `poisoned` (design-7 §7.7). The error
  allocated and is dropped where it is today: only on this path.
- **A panic in a task** is caught there with `catch_unwind`, on every thread, and counts as a failure. The region caught
  is `canon()`, `task(p)` and the debug `is_canon()` check after it, nothing more.
- **A panic in pool code** (`pop`, `complete`, an index into a queue slot or a consumer range, the debug `done[p]`
  write) is a bug. `run` and the worker loop each hold a guard whose `Drop` calls `std::process::abort()` when its thread
  is panicking. So `run` never unwinds out to `Callback::fill`'s `catch_unwind` while workers may still read the job (a
  use-after-free once `crash()` retires the plan), and no worker unwinds out of its loop with `inside > 0` (a device
  thread waiting forever). An abort is better than undefined behaviour. This matters most in the owner's
  `cargo run -p music2-desktop` build, the dev profile, where debug assertions and overflow checks are on in this path.
  `Callback::fill`'s own `catch_unwind` (`core/play/src/lib.rs:282`) stays for everything outside `run`.

### 4.4 Debug owner checks

In debug builds, `Reader::buf(j)` asserts that `part_of[j] == me` or that `done[part_of[j]]` equals this block's epoch.
That catches a missing dependency edge at the first read that would race. Release builds compile it out. The per-node
debug ABI-1 input check (`KernelNode::keep_inputs`) is per instance and needs no change.

---

## 5. The pool

### 5.1 Shape (`core/runtime/src/pool.rs`, new)

```rust
pub struct Pool { shared: Arc<Shared>, workers: Vec<JoinHandle<()>> }
pub struct Setup {
    pub start: Option<Box<dyn Fn(usize) + Send + Sync>>,  // on each worker, after the pool's own set-up (§6.3)
    pub stop: Option<Box<dyn Fn(usize) + Send + Sync>>,   // before it exits (leave the workgroup)
    pub patience: Duration,                               // how long a worker spins with nothing to take
    pub stuck: Duration,                                  // how long the device thread spins with no partition finishing
}
impl Setup { pub fn plain() -> Setup }                    // no hooks; 50 µs and 1 ms: offline renders and tests
impl Pool {
    pub fn new(threads: usize, setup: Setup) -> Result<Pool>;   // `threads` counts the caller: threads − 1 workers
    pub fn this_thread() -> &'static Pool;                      // no workers: the caller runs every task
    pub fn threads(&self) -> usize;
    pub fn counters(&self) -> Counters;                         // per thread: tasks, ns in tasks, ns spinning, parks (§9.1)
}
impl Drop for Pool { /* stop, wake every worker, join: never on an engine thread */ }

struct Shared {
    job: CachePadded<AtomicPtr<Job>>,   // null between runs
    inside: CachePadded<AtomicU32>,     // workers that may be touching the job
    again: AtomicBool,                  // another run follows in this callback (§5.5)
    wake: Box<[Wake]>,                  // one per worker
    device: Wake,                       // the device thread's: used only when a task is stuck (§5.6)
    stop: AtomicBool,
}
struct Wake { parked: CachePadded<AtomicBool>, sem: Sem }   // Sem: a Mach semaphore on macOS, a thread park elsewhere
struct Job<'a> { sched: &'a Schedule, task: &'a (dyn Fn(u16) -> Result<(), (u32, anyhow::Error)> + Sync) }
```

Each worker does the following at start, before its `start` hook:
- spawns with Rust's default 2 MiB stack, which leaves room for wasmtime's 512 KiB `max_wasm_stack` and for plugins;
- calls `wasmtime::Engine::tls_eager_initialize()`, because the per-thread trap setup costs hundreds of microseconds on
  the first wasm call;
- calls `fpenv::canon()`.

The block path lives in `pool/block.rs`. A grep test, like `core/play/tests/no_blocking.rs`, keeps it free of
allocation, locks, channels, `thread_local!` and I/O. Both kinds of `Sem` signal without a lock: `semaphore_signal` is a
Mach trap that never blocks the signaller, and `unpark` is an atomic plus a futex wake.

New dependencies:
- mach2 becomes a direct macOS dependency of `m2-runtime` and `m2-play`, and crossbeam-utils (for `CachePadded`) a
  direct one of `m2-runtime`. Both are already in `Cargo.lock`.
- loom is a new dev-dependency.

### 5.2 One run

```text
device thread                                     worker i (loop)
─────────────                                     ───────────────
at the block line: wake(want)                     sleep:
  for i in 0..want:                                 parked[i] = true                  (SeqCst)
    if parked[i].swap(false): signal(i)             if stop: leave
pre-pass (canon, cells, schedulers, players)        wait(i)
run(sched, task, again):                          spin until job ≠ null, or `patience` passes: then sleep
  abort guard (§4.3)                                inside += 1                       (SeqCst)
  reset the run state; push each root               j = job                           (SeqCst)
  again = another run follows in this callback      if j ≠ null: help(j)
  job = &Job                       (SeqCst)         inside −= 1                       (SeqCst)
  help(&Job) until left == 0                        if that left inside == 0 and left == 0: wake the device
  job = null                       (SeqCst)           thread if it is parked
  wait until inside == 0                            if again: back to "spin until job ≠ null"
post-pass                                           else: sleep
  (with no workers, `run` skips the publish and the waits)

help(j), on any thread:
  loop:
    pop p → running += 1; run_one(p); running −= 1; continue
    if left == 0: return
    a worker returns when the queue is empty and left ≤ running   (all that is left runs, or waits on what runs)
    a worker returns when left hasn't changed for `patience`; the device thread parks when it hasn't for `stuck` (§5.6)
    spin

run_one(p):
  if skip[p]: ok = false
  else: ok = catch_unwind(canon(); task(p); debug: is_canon()) is Ok(Ok)    (an Err fills failed[p])
  complete(p, ok):
    debug: done[p] = epoch
    for q in consumers(p), most critical first: if !ok { skip[q] = true }; if waiting[q].fetch_sub(1, AcqRel) == 1 { push(q) }
    left.fetch_sub(1, SeqCst)                     ← the last thing a task does to the run state
    if the device thread is parked: wake it       (SeqCst load, then swap: only on the stuck path)
```

**The ready queue** has one slot per partition, because each partition is pushed exactly once per block:
- *push:* `t = tail.fetch_add(1)`, then `queue[t].store(p, Release)`.
- *pop:* read `h = head`. If `h == tail`, or `queue[h]` still holds `EMPTY` (the slot is reserved but not yet written),
  there is nothing to take yet. Otherwise `head.compare_exchange(h, h + 1)`, and on success the slot's value belongs to
  the popper.

Within a block `head` only grows, and the reset happens at quiescence, so there is no ABA.

### 5.3 Quiescence: why `run` can return safely

The job, the schedule's run state and the graph are plan memory. The device thread retires plans at block lines, and
they are freed later on the dropper thread. So once `run` returns, no worker may touch any of it.
- Workers touch the job only between `inside += 1` and `inside −= 1`.
- The device thread stores `job = null` (SeqCst) and then waits for `inside == 0` (SeqCst). A worker does `inside += 1`
  (SeqCst) and then loads `job` (SeqCst). That is Dekker's pattern: either the device thread sees the increment and
  waits, or the worker sees null and touches nothing.
- A worker inside the job only runs `help`, which returns at `left == 0` at the latest. So the wait is a few
  nanoseconds, unless the worker was preempted at that very instant (§13, R9).
- Nothing unwinds out of `run` or out of a worker's loop: the abort guard (§4.3) turns a panic there into an abort.

So between `run` calls the device thread owns every plan, which is when `take_over`, `end_fade`, `SwapLane`, retiring and
`Cells::apply` run. Nothing in design-7 changes.

### 5.4 Happens-before, edge by edge

- **Pre-pass to any task:** the device thread's writes come before `job.store(SeqCst)`. A worker loads `job` (SeqCst)
  before it pops anything.
- **Producer to consumer:**
  1. The producer's writes come before its `waiting[q].fetch_sub(AcqRel)`.
  2. All the decrements of `waiting[q]` form one release sequence, so the last decrementer acquires every producer's
     writes.
  3. The last decrementer pushes `q` (Release), and the popper loads the slot (Acquire).
  4. A `skip[q]` store (Relaxed) made before a producer's decrement travels the same chain.
- **Every task to the post-pass:** each task ends with `left.fetch_sub(SeqCst)`, and the device thread loads `left ==
  0` with Acquire or stronger.
- **Block to block on a moved box:** a box processed on worker A in block *k* and on worker B in block *k+1* is ordered
  by A's completion, then the device thread's join, then the next publish, then B's pop. A `Store` is `Send`, and it is
  entered by one thread at a time.
- **Sleeping without a lost wake.** A sleeper stores `parked = true` (SeqCst) and then rechecks what it waits for (`stop`
  for a worker; `left` and `inside` for the device thread). A waker changes that state (SeqCst) and then swaps `parked`
  to false (SeqCst), signalling only when it was true. Either the sleeper sees the change and doesn't wait, or the waker
  sees the flag and signals. A sleeper that saw the change but finds its flag already taken waits once to consume the
  signal. So at most one signal is ever pending.

The protocol is written against a 6-line `sync` shim (std atomics, or loom's under `cfg(loom)`). Four loom models check
it, separately from DSP, because loom can't run wasmtime:
- each task runs once, after its dependencies;
- no worker touches a job after `run` returns;
- two jobs back to back, with `again` set between them;
- a sleeper and a waker race on `parked`, for a worker and for the device thread: no lost wake and no second pending
  signal.

The semaphore is a spin under loom: sleeping affects liveness, never safety.

### 5.5 Waking and sleeping

- **The wake comes first.** `wake` runs at the block line, before the pre-pass (20 to 50 µs at p50, *estimate*), so most
  of a sleeping realtime thread's wake latency is hidden. The device thread starts on ready partitions at once. Late
  workers only add their share, because the scheduler is work-conserving.
- **One signal per sleeping worker.** Each worker has a padded `parked` flag. The device thread signals a worker only
  when `parked.swap(false)` was true (§5.4). A worker that slept through blocks (a debugger pause, a busy machine) comes
  back with at most one signal pending, so it costs at most one `patience` spin, not one per block it missed.
- **`want` = min(workers, width − 1)**, with the schedule's width from §3.2. A chain (a song written in `song.ts`, or the
  song plus one track) wakes nobody.
- **A worker stays only while there is work it could take.** It leaves the job when the queue is empty and every
  partition left is running or waits on one that is (`left ≤ running`). So idle workers sleep through the song tail and
  through the end-of-block imbalance instead of spinning at realtime priority: at p50 that saves about 3 × (90 µs of tail
  + 30 µs of imbalance) a block (*estimate*). A worker still spins while partitions wait on running ones, because the
  next completion pushes work it can take.
- **Workers stay awake between the runs of one callback.** At a 512-frame buffer `Engine::process` renders four blocks
  back to back in one callback; a device at another rate pulls 64 frames at a time through the resampler; `LiveFade`
  runs two engines per block. The device thread sets `again` before each run when another follows in the same callback.
  A worker that finishes helping then spins, for at most `patience`, for the next job, instead of sleeping 20 µs before
  the next signal. After the callback's last run it sleeps at once.
- **`patience`** is half a device period live, and 50 µs offline (blocks run back to back offline, with `again` set for
  every block but the last). **`stuck`** is one device period live, and 1 ms offline.
- **No wake** is sent while the engine idles, holds or plays a broken plan, or when the schedule's width is 1.
- **The device thread waits only for tasks a running thread has taken,** and for `inside` to drain. It spins while
  partitions keep finishing. If no partition finishes for `stuck`, the callback is already a period late and a task is
  blocked (§5.6), so it parks on its own semaphore until a completion wakes it, then helps again. Parking never delays a
  block that could still make its deadline.

### 5.6 What can go wrong while running

| Failure | What happens |
|---|---|
| a task panics or traps, on any thread | caught there; that partition fails and its dependents are skipped; the block always finishes (§4.3) |
| a panic in pool code, outside a task | the abort guard ends the process (§4.3) |
| a worker is preempted inside a task | the device thread waits for it: a possible dropout, never a deadlock; the workgroup and the realtime policy make it rare (R9) |
| a task blocks inside a plugin: a JUCE-style plugin holds the lock its `process` takes while step 7's `load_state` (design-7 §10.2) loads samples | after `patience`, the workers with nothing to take sleep; after `stuck`, the device thread parks; the blocked thread sleeps in the plugin. The callback returns when the plugin does, and the next completion wakes the device thread to finish the block. That is HEAD's behaviour, where the plugin blocks the IO thread itself: a dropout, and no engine thread spins meanwhile, so the main thread doing the load keeps its P-core |
| a plugin never returns | the device thread stays parked, as HEAD's IO thread stays blocked. The server's stall path starts a new player and closes the old one (§6.5). The old pool's threads sleep and cost nothing; only the stuck threads are lost, as today |
| a woken worker arrives after its block ended | it spins for `patience` at most, finds nothing, and sleeps |
| two engines (`LiveFade`, `core/play/src/lib.rs:344-397`) or several blocks in one callback | each block is its own `run`, with `again` set between them |
| more threads than the plan can use | `want` is capped by the schedule's width |

---

## 6. Live playback (`core/play`)

### 6.1 Player options

```rust
pub struct Options {
    pub device: bool,            // false: the silent player
    pub rate: Option<u32>,       // the device's rate; default: its own
    pub frames: Option<u32>,     // device buffer; default: the device's own (silent: 480)
    pub threads: Option<usize>,  // engine threads, the device thread included; default: automatic (§6.3, §6.4)
    pub silence: bool,           // render, but write zeros to the device (the gate)
    pub capture: Option<CaptureSpec>, // gates only: keep the rendered output of a span, for §9.2's IO-thread check
}
impl Options { pub fn from_env() -> Options }   // M2_AUDIO=none, M2_AUDIO_FRAMES, M2_THREADS
// Player::start() is Player::start_with(Options::from_env()); Player::driven_on(rate, Pool) beside driven(rate).
```

- `frames` opens the stream with `BufferSize::Fixed(n)`. cpal 0.18.2 sets `kAudioDevicePropertyBufferFrameSize` after
  checking the device's range (`cpal-0.18.2/src/host/coreaudio/macos/device.rs:1009-1036`). Today `Player::start` uses
  the default config (`core/play/src/lib.rs:545`).
- `M2_AUDIO_FRAMES` now means the same thing for the device and for the silent player. The app's default buffer stays
  the device's own until the Settings window (gaps.md #31; owner question Q2).
- `Callback` owns its `Pool`. `Live::fill`, `LiveFade`'s fill and the new `Engine::process_on(out, inbox, retire, stamp,
  pool)` take `&Pool` from it. `Engine::process` keeps its signature and runs on one thread, so the runtime's own engine
  tests are unchanged. The pool is built on the `m2-play` thread when the stream opens, and dropped with the stream when
  the player closes (§6.5), never on the audio thread.

### 6.2 The device thread

The device thread is cpal's CoreAudio IO thread, which CoreAudio makes realtime and puts in the device's workgroup. It
does the following:
- sets the canonical floating-point mode at the top of each callback (§2.3);
- wakes the workers at each block line;
- runs the pre-pass, its share of the tasks and the post-pass;
- spins while running workers finish, and parks only on a stuck task (§5.5).

It also keeps the load record (§9.1). At 64-frame device buffers, every second callback renders a whole 128-frame block
(RENDER-33 needs the 128 grid, `engine.rs:261-275`) and the other only pops frames. Only the rendering callbacks wake the
workers.

### 6.3 The macOS realtime set-up (`core/play/src/rt.rs`, new, macOS only)

> **Platform step** (platform design §8): `rt.rs`, `Sem` and the QoS calls go into `m2-os` (`mac/rt.rs`, `linux/rt.rs`).
> Step 5's overload listener gives way to cpal's `ErrorKind::Xrun` errors on every OS (`platform/survey-later.md` §5.5).

When the stream opens:
1. **Device id.** Take `DeviceTrait::id()`, the device's UID (`cpal-0.18.2/src/host/coreaudio/macos/device.rs:431`), and
   translate it with `kAudioHardwarePropertyTranslateUIDToDevice` into an `AudioDeviceID`. objc2-core-audio 0.3.2 is
   already in `Cargo.lock` through cpal, and becomes a direct macOS dependency of `m2-play`.
2. **Workgroup.** Read `kAudioDevicePropertyIOThreadOSWorkgroup` (`'oswg'`) from it, and `os_release` it when the pool
   drops. The header says the caller releases it.
3. **Period.** `kAudioDevicePropertyBufferFrameSize` and the rate give the period in Mach ticks (24 MHz on Apple
   silicon, so 2.667 ms is 64,000 ticks).
4. **Thread count.** Explicit `threads` wins. Otherwise use `min(os_workgroup_max_parallel_threads(wg, NULL),
   hw.perflevel0.cpusperl2)`, at least 1: the performance cores that share one L2 cache. That is 4 on the reference M1,
   and 4 on the M1 Pro, whose 8 P-cores are two clusters of four (*estimate*; measurement 4 in §14). E-cores are never
   counted: a task on an E-core runs at about a third of the speed and becomes the critical path. One cluster keeps every
   engine thread on one L2 (R1), keeps a plugin's partition on the same cache from block to block, and leaves the M1
   Pro's other cluster to the UI, the compile and coreaudiod.
5. **Overload listener.** `AudioObjectAddPropertyListener(id, kAudioDeviceProcessorOverload)` with a C callback that
   only bumps an atomic. It is removed on drop.

Each worker's `start` hook:
1. `thread_policy_set(mach_thread_self(), THREAD_TIME_CONSTRAINT_POLICY)` with period = the device period, computation =
   half of it, constraint = the period, and preemptible. mach2 0.6.0 has the types.
2. `os_workgroup_join(wg, &mut token)`. The function is documented as real-time safe, and the token is 40 bytes held on
   the worker's stack. This is Apple's case 1 in `AudioWorkInterval.h`: a realtime thread triggered by code running on
   the device's IO thread. A failure (`EALREADY`, `EINVAL`) is recorded, and the worker runs on with the time-constraint
   policy alone. G12 doesn't pass a run in which a worker failed to join (§9.2).
3. The `stop` hook calls `os_workgroup_leave` before the worker exits.

There are only five `extern "C"` declarations to write: `os_workgroup_join`, `os_workgroup_leave`,
`os_workgroup_max_parallel_threads`, `os_release`, and the token type. The rest is in mach2, objc2-core-audio and libc.

If any step fails (no device id, no workgroup), the pool still runs, with time-constraint workers outside the workgroup,
and G12 prints what was missing. A device that changes underneath (headphones plugged in, a new rate) is handled as
today, by a new player, which brings a new pool and the new device's workgroup; the old player closes (§6.5).

### 6.4 The silent and driven players

- **Silent player** (`M2_AUDIO=none`, used by the server rig and e2e): the same pool with no realtime set-up, on **one
  thread** unless `M2_THREADS` says otherwise. HEAD's timing gates on the server rig (a seek audible within 50 ms, compile
  + 20 ms) were measured with one thread, and on a busy machine (the survey saw load 9 to 15 during agent work) spinning
  pool threads would make the silent clock fall behind. The server rig's gates take `--threads N` to run it on more.
- **`Player::driven_on(rate, pool)`:** the tests and the gates' engine rig choose their thread count. `driven(rate)`
  stays as it is, on one thread. A test that doesn't set a count runs on one thread; the 1-vs-N tests set their own.

### 6.5 Closing a player (new)

HEAD never drops a stream: the `m2-play` thread parks with it forever (`core/play/src/lib.rs:567-570`), and the server's
`retire()` only sends `Unload` (`crates/server/src/graph.rs:249-253`). With a pool, every crash, stall or device change
would leave N−1 realtime threads, a workgroup reference and an overload listener behind, and an afternoon of plugging
headphones in and out would collect dozens.
- `Player::close()` sends `Unload` as today, so the engine retires its plan through the dropper. It waits up to 1 s for
  the callback to report it, then sets a `closing` flag and unparks the `m2-play` thread.
- That thread drops the stream. The device stops, the `Callback` drops with it, and `Pool::drop` makes each worker leave
  the workgroup and joins them, off the audio thread. The listener and the workgroup are released there too.
- If the callback is stuck in a plugin (§5.6), the drop never returns, and only that parked thread is lost, as today.
- The server's `retire()` calls `close()`. The silent player's thread checks the same flag and exits.

---

## 7. Offline renders

- `Renderer::{render_block_on, process_on, render_on}(…, pool: &Pool)` are new. `render_block`, `process` and `render`
  keep their signatures and run on `Pool::this_thread()`, so the 40-odd existing callers and tests are unchanged and go
  through the executor on one thread.
- `m2_editor::render::Render` (Export Audio, `m2 render`, `m2 bounce`, range renders) owns a `Pool::new(threads,
  Setup::plain())`, created per render and dropped after it on the render's own thread. `threads` comes from
  `Render::threads(n)`, `m2 render --threads N` or `M2_THREADS`. Offline threads are never realtime (RENDER-28a).
- **The default count.** Realtime priority wins CPU time, but not the shared L2, the system cache or DRAM bandwidth, and
  p50 is memory-bound (R1). Eight offline threads streaming p50 flat out through the M1's 12 MB L2 and 68 GB/s would
  slow the live kernels. So:
  - a render started while the project plays in the app (the server's Export Audio of a pure song, design-7 §10.5; an
    agent's range render through the server, RENDER-4) runs at `QOS_CLASS_UTILITY` on as many threads as there are
    E-cores (`hw.perflevel1.physicalcpu`), which keeps it on the E-cores and their own L2;
  - a CLI render does the same when it finds the project's lock held by the app, since it can't tell whether the app is
    playing;
  - otherwise a render uses every P-core (`hw.perflevel0.physicalcpu`) at the default QoS.
  An explicit count always wins. G12's `--export` run judges it (§9.2).
- Bits are the same on any count, so provenance doesn't need to record the thread count. An export of p50 takes about
  3.5 minutes on one core, and roughly a fifth of that on 8 threads (*estimate*).

---

## 8. Voices across threads: not built

The heaviest p50 partition is a `reverb(pad)` track: about 7 active voices × 12 kernels + 2, which is about 86 calls,
about 65 µs. The critical path is the slowest track plus the song tail, about 150 to 180 µs, against a 1,867 µs target
(*estimate*). Splitting voices would shorten something that isn't the problem. Hosted plugins, which are the likeliest
heavy task, can't be split by the host at all.

- **Trigger.** G12's per-partition profile (§9.3), on p50 or on a song the owner brings, shows a single std `poly`
  partition above 25% of the 128-frame block (667 µs).
- **The rule that keeps the bits when it is built.**
  1. Allocate voices serially, in the poly's task.
  2. Run chunks of voices as subtasks. Each records the range of frames its voice covers in the block, and its peak.
  3. Once all have finished, sum into a zeroed `out` in voice-index order, over each voice's covered range.

  That is the same sequence of `+=` per frame as today, because one voice's segments are disjoint and contiguous.
- **Not prepared in advance.** 8a lays out no data for it.

---

## 9. Measuring gate 8a

### 9.1 What the player records, always (`core/play/src/load.rs`, new)

At each callback's entry and exit, `now_ns()` (`mach_absolute_time` underneath; no system call, no allocation). Then
Relaxed atomic adds into `Shared`:
- `hist_all[d / 4 µs]` and, when the callback rendered at least one block, `hist_render[d / 4 µs]`: 2,048 buckets each
  (up to 8.2 ms), plus an overflow count and the maximum;
- `late`: callbacks longer than their period;
- `gaps`: entry-to-entry intervals over 1.5 periods;
- `overloads`: from the listener (§6.3).

The pool adds its own counters, per engine thread: tasks run, nanoseconds in tasks, nanoseconds spinning in `help` or
waiting for a job, and parks (`Pool::counters`).

`Player::load()` returns a snapshot of both, and the difference between two snapshots describes a window. This costs
about two clock reads and two atomic adds per callback, and two clock reads per task. It is always on, so the same
numbers drive 8b.8's DSP readout (design-8b §10.3).

### 9.2 `m2-gates realtime` (G12, `crates/gates/src/realtime.rs`, new)

Every run writes p50 (`m2_session::reference::write`), loops the whole song (512 beats, 4:02, so 10 minutes include two
wraps) and plays it on the real device. The engine runs open the device through the explicit `Options { device: true,
… }`, never through the environment, so they bypass the `M2_AUDIO=none` that the binary sets for every other gate
(`crates/gates/src/main.rs:69-71`). `--server` starts the headless server (per `e2e/README.md`) with
`M2_AUDIO_FRAMES=128` and `M2_THREADS=N` and without `M2_AUDIO=none`, so it opens the device as the app does.

| Run | What | Verdict |
|---|---|---|
| `realtime` | the engine alone: `Player::start_with(Options { rate: Some(48_000), frames: Some(128), threads: Some(N), silence: !audible, … })`, compiled as the engine rig does, 10 minutes. These are the gate's words | judged |
| `realtime --server` | the app's own path: the headless server with the device at 128 frames and N engine threads, p50 open in a headless Chrome (playhead, and from 8b.8 the meters at 30 Hz), and one structural edit every 10 s for 10 minutes (G7a's edit of `t02`'s pluck, wrapped in `delay` and unwrapped in turn), so each edit's ghosts, fades and retirements land in the measured window | judged |
| `realtime --export` | `realtime` while the server exports p50, a pure song, beside playback (§7) | judged |
| `realtime --capture resample` | 10 s of `examples/resample` rendered on the real IO thread (kept inside `fill`, written to the device as zeros), compared bit for bit with its export: RENDER-33 and RENDER-2a on the thread CoreAudio gives us, which `bit_identity.rs` can't reach | judged |
| `--frames 64`, `--frames 512` | the same as `realtime` at other buffers. At 64, the rendering callbacks are also reported against their own 1,333 µs period. At 512, four blocks share each callback (§5.5) | recorded |
| `--sweep 1,2,3,4` | one minute per thread count, a table of p99 values | recorded |
| `--song <dir>` | another song, such as `examples/with-plugins` on 4 threads (R12) | recorded |

**Dropouts are counted from the first callback,** because a crackle just after pressing play is the one a producer
notices first. The p99 statistics leave out the first 5 seconds.

| Reported | Bound in a judged run (128 frames) |
|---|---|
| callback p50, p99, p99.9, max (µs and % of the period) | p99 ≤ 70% of 2,667 µs = 1,867 µs |
| dropouts: overloads + late + gaps, from the first callback | 0 |
| crashes, non-finite samples | 0 |
| frames played, compared with the transport's motion | 10 minutes or more, exact |
| set-up: machine model, P-cores and cluster size, workgroup recommendation, threads used, each worker joined or not, the `FPCR` seen on the IO thread | every worker joined; otherwise the run says "not the designed set-up" and doesn't pass |
| engine-thread CPU per block: time in tasks, time spinning, parks (§9.1) | printed |
| `--server`: each edit's compile time, the time from its compile to its first audible block, and the callback times of each activation and each retirement | each edit audible within G7a's 200 ms after its compile, and the compile within PLANE-4's 100 ms at p95, both with the pool live |

**`--like-m1`.** On a Mac bigger than the M1, it adds an M1-like load before playing: one busy loop at the default QoS
on every P-core beyond the M1's four, and streaming copies over a 1 GB buffer until the machine's measured bandwidth left
over is about the M1's 68 GB/s. Both are printed. It is for the stand-in only (owner question Q1).

A verdict of **pass** needs every judged run's bounds, on a base M1. On another Mac the verdict line says
"provisional (stand-in: <model>, `--like-m1`)" and the gate isn't marked passed.

### 9.3 `m2-gates realtime --offline`

`m2-gates realtime --offline` needs no device:
- it renders p50 from 3:00 for 30 seconds on 1, 2, 3, 4 and 8 threads, and reports the mean and p99 block time;
- it reports the share spent in the pre-pass, the graph and the post-pass, and each thread's task and spin time;
- `Renderer::profile_parts(blocks)`, a gates-only `#[doc(hidden)]` hook that times each partition run serially, gives the
  ten slowest partitions (mean and p99) and the song tail;
- `--sends` renders a p50 variant whose first 20 tracks send into a `verb` return track and 8 into a drum bus
  (`reference::write` gains the option), which shows the critical-path order (§3.2) at work.

This is the development loop, and it feeds §8's trigger.

### 9.4 The recorded run

- **Machine.** Release build, on power, built-in output at 48 kHz. For `realtime`, the machine otherwise idle (load
  average under 1; the survey saw 9 to 15 during other work). For `--server`, the server, the browser and the edits are
  the load.
- **On a base M1** (the reference Mac, owner question Q1): every judged run, then `--frames 64`, `--frames 512`, `--sweep
  1,2,3,4`, `--song examples/with-plugins` and `--offline`.
- **On the owner's M1 Pro** (MacBookPro18,1, 8 P-cores in two clusters, 2 E-cores): the same runs with `--like-m1`,
  recorded as provisional. Also `--offline --sweep` without the load, so the scaling curves of the two machines can be
  compared.
- **The numbers** go into gate 8's Today line and RENDER-2's.

---

## 10. The 1-vs-N tests (CI: `cargo test --workspace`)

The thread counts are fixed (1, 2, 3, 4 and 7), never the machine's. 7 is odd on purpose: it gives more threads than
small fixtures have partitions, and uneven chunks. A test hook (`#[doc(hidden)]`) can wake every worker whatever the
width, and record which thread ran each partition.

| Test | Where | What it proves |
|---|---|---|
| `every_pure_fixture_renders_the_same_bits_on_1_and_n_threads` | `core/play/tests/bit_identity.rs` | first-song, resample, starter and empty, whole songs, on 1/2/3/4/7 threads: bit-identical. The 1-thread render stays checked against `renders.json` by the session's existing test |
| `p16_renders_the_same_bits_on_1_and_4_threads` | same | 30 s of p16 in CI; all of p16 `#[ignore]` (recorded) |
| `live_on_4_threads_is_the_export_on_1` | same | RENDER-33 with the live engine on 4 threads, every pure fixture |
| `the_pre_pass_runs_in_the_canonical_mode` | same | the test thread sets FZ before calling `Callback::fill` on `resample`; the output still equals the export (§2.3) |
| `random_convex_partitions_give_the_same_bits` | `core/runtime/src/tests/parts.rs` | for many seeds: Kahn's order with seeded random tie-breaks, cut into random runs. An interval of a topological order is convex, and the graph between runs is acyclic, so every cut is a valid partitioning with many partitions. Random jitter before each task, every worker woken, on 4 and 7 threads: equal to the 1-thread render. From the thread records it asserts that at least two threads ran tasks and that some consumer ran on a different thread from one of its producers, so the test can't pass on a serial run |
| `modules_that_read_each_other_split_by_depth`, `a_partition_runs_its_nodes_in_plan_order`, `the_schedule_is_the_same_for_the_same_plan`, `every_edge_goes_forward`, `the_most_critical_partitions_go_first`, `a_chain_has_width_one` | `parts.rs` unit tests | §3.2; `every_edge_goes_forward` over random DAGs with random module labels, cycles included |
| `cross_module_feedback_captures_and_sidechains_on_n_threads` | `tests/parts.rs` | a feedback whose input is in another partition; a capture into a voice from another module; `follow(kick.out)` |
| `activations_on_4_threads_match_1` | `tests/live/keep.rs` | step 7's ghosts, fades and windows on 4 threads, sample for sample against 1 thread |
| `strict_names_the_first_failing_node_on_any_thread_count` | `tests/parts.rs` | §4.3, with two broken kernels in independent tracks |
| `a_panicking_task_completes_the_block` | `pool` tests | §4.3, §5.6 |
| `a_panic_in_pool_code_aborts` | `pool` tests | a `cfg(test)` hook panics in `complete`; the test runs itself as a child process and expects it to abort within 10 s, never to hang (§4.3) |
| `a_blocked_plugin_spins_no_thread` | `core/play/tests` | a fourth probe id, `m2.probe.blocks`, sleeps 100 ms in one `process` call, on a worker of a 4-thread pool. The block still renders the right bits, the callback returns once the probe does, and the pool's counters show at most `3 × patience + stuck` of spinning in that block, against the 300 ms or more of revision 1 (§5.6) |
| `a_stuck_plugin_leaves_every_engine_thread_asleep` | `core/play/tests` | the probe blocks until the test releases it; after 50 ms every other engine thread is parked (its counters stop); once released, `close()` returns and every worker has joined (§6.5) |
| `one_signal_per_sleeping_worker`, `workers_stay_awake_between_runs_of_one_callback` | `pool` tests | a worker held off for 100 blocks costs at most one spurious wake; at 512-frame callbacks each worker parks at most once per callback (§5.5) |
| `a_closed_player_joins_its_workers` | `core/play/tests` | §6.5: after `close()`, no engine thread of that player is left |
| `a_flush_to_zero_left_by_a_plugin_changes_no_bit` | `core/play/tests` | a third probe id, `m2.probe.sets-fz`, sets FZ in `process` and never restores it. A kernel chain in the same track (so the same partition and thread, after the probe in plan order) makes subnormals on purpose (`scale` by 1e-30, then by 1e-10, then back up by 1e40). Its output must equal the same chain's output in a song without the probe. Without RENDER-2a it would flush to zero (std's kernels never reach subnormals on their own) |
| `plugins_on_workers_are_told_the_truth` | `core/play/tests` | the probe's records: correct thread-check answers, no block processed twice, on 4 threads. The probe also keeps a C-style `thread_local` and records each thread's first touch, which G12's `--song examples/with-plugins` reports for real plugins (R12) |
| `no_alloc.rs` on a 3-worker pool | `core/play/tests/no_alloc.rs` | the existing scenario with workers under the trap. The `start` hook marks them, and their counts go to a global atomic, since `COUNT` is thread-local. It also covers a fresh worker's first block (§13, R10) |
| the pool's grep test | `core/runtime/tests/no_blocking.rs` (new) | `pool/block.rs`, `view.rs` and `fpenv.rs` hold no allocation, lock, channel, thread-local or I/O |
| the loom models | `RUSTFLAGS="--cfg loom" cargo test -p m2-runtime --release pool` | §5.4 |

---

## 11. What is deleted or simplified

**In the code (against HEAD and design-7):**

| Deleted or simplified | Replaced by |
|---|---|
| design-7's `Plan.order_after`, and step 4 of `end_fade` (the order swap) | one schedule per plan; ghost slots are `Exec::Const(0.0)` no-ops after the window (§3.4) |
| the top level of `Graph::process`: its loop, the `mem::take` and put-back of each buffer, and its post-loop ring writes | the schedule, `step()` and the render's post-pass; voice graphs keep the loop |
| `bufs: &[Vec<Buf>]` and `outer: &[Vec<Buf>]` on `PolyNode::process`, `PluginNode::process` and `plugin_events` | one `Ports` view |
| "the first error the loop meets" | the first failing node in plan order (§4.3); the same with one failure |
| `M2_AUDIO_FRAMES` applying to the silent player only | one meaning, for the device too |
| the "audio thread" singular, in comments and in RENDER-28 and HOST-2 | "engine threads" |
| the `m2-play` thread parking with its stream forever, and `retire()` that only unloads | `Player::close()` (§6.5) |
| revision 1's merge of modules that read each other into one partition | the depth split (§3.2): the same convexity, without serialising two tracks |
| revision 1's unbounded spinning: workers until `left == 0`, the device thread until done | the leave rule, `patience` and `stuck` (§5.5) |

**From the survey and from the problems raised, not built (each with its reason):**

| Not built | Why not |
|---|---|
| a ready queue in pool memory with epoch-tagged entries | the queue lives in the schedule, and the `inside` counter gives a simpler quiescence proof (§5.3) |
| two schedule forms per plan (window and after) | one is enough (§3.4) |
| a paced mode with no device (`AudioWorkIntervalCreate`) | every Mac has a built-in output; the offline sweep covers CPU-only development |
| voice splitting, and per-voice covered ranges laid out in advance | §8 |
| learned cost estimates (an exponential average of measured task times) | static costs and bottom levels order only the first ready partitions; measure first |
| a thread count chosen each block from the last block's measured time | a block's work hardly falls when tracks go quiet (every node runs every block), and a count that follows the last block would leave the first heavy block after a quiet stretch on too few threads. The width cap and the leave rule (§5.5) stop the spinning it would save |
| abandoning a block past a deadline (silence out, the job kept in plan memory, no messages applied until it drains) | the callback waits for a blocked plugin, as HEAD's does when the plugin blocks the IO thread, at the same cost: a dropout and no CPU (§5.6). Abandoning needs a second engine state and a second proof of `Hold`'s exclusivity |
| home threads for plugin partitions (per-thread queues, stealing only plugin-free partitions) | CLAP allows audio-thread calls from several threads, one at a time per instance, and other multi-threaded hosts move plugins between threads. A plugin's thread-locals are per binary and per thread, so their first touches number at most threads × plugin binaries, mostly in the first blocks after a load. The one-cluster cap (§6.3) keeps the caches shared. Slice 4, on a measured dropout (R12) |
| `wfe` spinning on the watched cache line | inline assembly for a power saving nobody has measured; the leave rule removes most of the spinning; slice 4 if the counters say so |
| QoS classes for the actor, compile and dropper threads | only if G12 or the code-plane budget shows contention (slice 4) |
| Panic reset through the pool | not in any gate; only if a Panic is measured to drop out at p50 |
| a native N-ary `sum`, or moving mixer strips into their tracks | the song tail is about 3% of the budget (*estimate*); slice 4 if measured |
| rayon, `audio_thread_priority` | not realtime-safe, or not needed on macOS (survey §7) |

**In the contract:** RENDER-32's "one `Store` per partition" fallback goes, as design-7 already proposes. The
song-as-program "Parallel (Not built yet)" paragraph becomes what is built.

---

## 12. Contract edits

Each edit lands in the slice that changes the code ("a change that breaks a rule changes this page first").

| Rule | Now | Proposed | Slice |
|---|---|---|---|
| RENDER-2 | "…MUST NOT depend on thread count or scheduling: every sum (buses, `poly` voices, overlay mixes) runs in plan order in the consuming partition." | "…MUST NOT depend on thread count or scheduling. A plan's top-level nodes are split into partitions, each run as one task per block, in plan order, on one thread. Partitions follow module keys; where modules read each other both ways, their nodes are split further by the number of module crossings that lead to them, so no dependency path leaves a partition and returns. They are rebuilt with every plan and are never identity. A node reads only buffers its dependencies wrote this block; feedback reads the last block. No two partitions write the same memory. Every sum runs inside one node: buses as the graph's own adds, a `poly`'s voices in voice order into a zeroed buffer, an overlay gain in its consumer's partition." *Today:* "matches since step 8a, on any number of threads; `every_pure_fixture_renders_the_same_bits_on_1_and_n_threads`." | 1 |
| RENDER-2a (new) | — | "Every thread that runs DSP MUST compute in the canonical floating-point mode: round to nearest, subnormals kept, no default NaN. The engine sets it at the start of each callback and each rendered block, before each partition, and after each plugin call. *Why:* WebAssembly requires subnormals, a plugin may leave flush-to-zero set for the next kernel on its thread, and the mode of the device's own thread is not ours to assume." | 1 |
| RENDER-28 | "The audio thread MUST NOT allocate, lock, do I/O or wait on a compile." Messages as listed | "Engine threads (the device's callback thread and the engine's workers) MUST NOT allocate, lock, do I/O or wait on a compile while they render. They wait only for work a running thread has taken: they spin while it progresses and sleep when it doesn't, so a task blocked in a plugin costs a dropout and no CPU. In a normal block the only system call wakes sleeping workers and never blocks. A panic outside a task aborts the process. Debug builds and CI run every engine thread under the allocation trap." The message list follows design-7's edit | 2 |
| RENDER-28a (new) | — | "Live workers are real-time threads at the device's period, joined to the output device's audio workgroup: as many as the workgroup recommends and no more than the performance cores that share one L2 cache, the device thread included. They sleep when there is nothing they could take. A replaced player closes its stream and joins its workers. Offline renders use plain threads, never real-time ones; a render started while the project plays runs at utility QoS on as many threads as there are efficiency cores, unless told otherwise." | 2 |
| HOST-2 | "…and process plugins on the audio thread." | "…and process plugins on an engine thread: the device's callback thread or a worker, one call at a time per instance; `is_audio_thread` holds for the thread making the call." | 2 |
| The `PluginHost` sketch, and the as-built comment | "// the audio thread" | "// an engine thread (the device's or a worker), one call at a time per instance" | 2 |
| RENDER-32 *Why* | "per-instance stores make instances movable between plans and partitions" | design-7's text, plus: "a `Store` is entered by one thread at a time, so per-instance stores let any partition run on any thread." | 1 |
| Acceptance gate 8 | as quoted | add: "Dropouts are counted from the first callback: the device's overload notifications, callbacks longer than their period, and gaps of more than 1.5 periods between callbacks. Every worker has joined the device's workgroup. p50 also plays without dropouts through the app's own path while it is edited every 10 seconds, with each edit within its 200 ms, and beside an export. The 64-frame run reports the callbacks that render a block against their own period." | 3 |
| `song-as-program.md`, "Parallel (Not built yet)" | | the built form, with the thread counts | 3 |

---

## 13. Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | Scaling falls short of 4×, because of memory traffic (about 3,000 kernel calls a block, each in its own wasm memory, *estimate* 12 to 24 MB touched a block, over the M1's 12 MB L2), the song tail, or a machine with two P-clusters | slice 0 measures it before anything is built; threads capped at one L2 cluster (§6.3); the judged run is on a base M1, and the stand-in runs under `--like-m1`; slice 4's first item; fewer threads if that is faster (the sweep shows it) |
| R2 | Wake latency of sleeping realtime workers eats into the 128-frame budget | the wake goes out before the pre-pass; workers stay awake between the runs of one callback; G12 shows it; spinning between callbacks is slice 4 |
| R3 | 4 realtime threads on a 4-P-core M1 starve the UI, the compile (PLANE-4's 100 ms, G7a's 200 ms) or `prepare` | workers sleep whenever there is nothing they could take: at about 30% load, about 70% of the P-core time is free; `--server` measures the compile and G7a with the pool live and is judged; slice 4's QoS item |
| R4 | A plugin leaves FZ or a rounding mode set, or CoreAudio's thread starts with one | RENDER-2a at the top of each callback too; its tests; the IO-thread capture check |
| R5 | An aliasing bug in the shared graph view | two unsafe functions in one file; debug owner checks on every read; random convex partitions with jitter, asserted to overlap; loom for the protocol |
| R6 | The device's workgroup can't be read through cpal (it hides the device id) | translate the UID; failing that, time-constraint workers without the workgroup, which G12 reports and doesn't pass |
| R7 | The 64-frame run looks poor, because every second callback renders a whole block in half the time | it is recorded, not judged; rendering ahead would add 128 frames of latency and is a separate owner decision |
| R8 | One heavy plugin, or a song written all in `song.ts`, is one partition and so the critical path | the profile names it; the host can't split a plugin; a one-module song runs as it does today, waking no worker |
| R9 | A worker is preempted inside a task, or at the instant it holds `inside` | the device thread waits: a dropout, never a deadlock; the workgroup and the time-constraint policy make it rare; G12 counts dropouts, whatever their cause |
| R10 | A worker's first block allocates: macOS thread-local first touch mallocs outside Rust's allocator, or wasmtime's lazy per-thread set-up | `tls_eager_initialize` and `canon()` at worker start; `no_alloc.rs` runs a fresh pool's first block under the trap; if it still allocates, the pool runs one warm-up job at start |
| R11 | The other workflow is editing `crates/gates`, `crates/editor` and `crates/server` | slices 1 and 2 stay in `core/*`, plus `render.rs`, `apps/m2` and the three lines of `retire()`; slice 3 is a new file |
| R12 | A hosted plugin's C++ thread-locals are first touched on a realtime worker mid-song, through the system malloc that Rust's trap can't see | first touches number at most threads × plugin binaries; the probe records them, and `--song examples/with-plugins` counts dropouts over 10 minutes; a dropout traced to one is slice 4's trigger for home threads |
| R13 | Edits retire many per-instance Stores, and their `munmap`s on the dropper take the VM map lock that a faulting realtime thread then waits on (design-7 R13) | `--server` lands edits in the judged window and prints each retirement block; pacing the dropper is slice 4 |
| R14 | An export beside playback contends for the L2, the system cache and DRAM | E-cores at utility QoS by default (§7); `--export` is judged |
| R15 | A blocked plugin parks the device thread past the end of its callback | the same dropout HEAD has when the plugin blocks the IO thread itself; `stuck` is a whole period, so parking never costs a block that could still be on time |

---

## 14. Measurements to take first

1. **The baseline:** p50 single-thread block time from 3:00, release (step 7's G7f part 1, on slice 1).
2. **Scaling (slice 0):** offline p50 on 1, 2, 3, 4 and 8 threads, on the M1 Pro and, as soon as there is one, a base M1.
3. **Partition costs (slice 0):** mean, p99 and maximum for each track type and for the song tail, which decide
   whether per-module partitions balance on 4 threads.
4. **The device (slice 2):** `os_workgroup_max_parallel_threads`, `hw.perflevel0.cpusperl2` and the buffer-size range of
   the built-in output (are 64 and 128 accepted?), on the M1 Pro and the M1.
5. **Wake latency (slice 2):** from `semaphore_signal` at the block line to the worker's first pop, with idle cores.
6. **`FPCR` (slice 2):** its value on the CoreAudio IO thread, and after a Surge XT and an Airwindows `process` call.
7. **A fresh worker's first block (slice 2):** whether it allocates under the trap (R10).
8. **Plugin first touches (slice 3):** how many and how long, for Surge XT and Airwindows on 4 threads (R12).
9. **Engine-thread CPU per block (slice 3):** tasks against spinning, at 128 and at 512 frames (§9.1).

---

## 15. Attack response

Fifteen problems were raised against revision 1.

| # | Problem | Verdict | Why | Where it changed |
|---|---|---|---|---|
| P1 | A blocked task turns every engine thread into an unbounded realtime spin, and stall recovery leaves the old IO thread spinning forever | **Accept the problem; bound every wait, without abandoning blocks** | Confirmed: revision 1's `help` returned only at `left == 0`, and HEAD never drops a stream. Workers now leave after `patience` with nothing to take, and the device thread parks after `stuck` until a completion wakes it, on its own semaphore, with a system call only on that path. The proposed abandonment (silence out, the job in plan memory, no messages until it drains) is **rejected**: the callback waiting for a blocked plugin is HEAD's behaviour today, with the same dropout and now no CPU; abandoning needs a second engine state and a second `Hold` proof. The probe test is taken, with the callback returning when the plugin does | §0, §5.1, §5.2, §5.5, §5.6, §6.5, §10 (`a_blocked_plugin_spins_no_thread`, `a_stuck_plugin_leaves_every_engine_thread_asleep`), §11, §12 RENDER-28, R15 |
| P2 | The M1 Pro stand-in can pass a gate the reference M1 fails | **Accept** | The stand-in is most generous exactly where R1 says the risk is: bandwidth, system cache, two L2s, spare P-cores. The verdict needs a base M1; the M1 Pro under `--like-m1` is provisional and labelled so; both machines run `--offline --sweep`; every worker joining is part of the pass | §9.2, §9.4, §12 gate 8, §17 Q1, R1, R6 |
| P3 | The judged run times the engine alone on an idle machine, not p50 playing while a producer works | **Accept** | Live-coding while playing is the owner's case. Dropouts count from the first callback. `realtime --server` (the server, a browser, a structural edit every 10 s for 10 minutes) is judged, and it re-measures G7a and PLANE-4 with the pool live and prints each activation and retirement block | §9.2, §9.4, §12 gate 8, R3, R13 |
| P4 | A panic in pool code outside the task's `catch_unwind` hangs the engine or frees a plan under live workers | **Accept** | Only convention kept it out, and the owner's dev build has overflow checks and debug assertions on in this path. An abort guard in `run` and in the worker loop; only the task and its debug FP check are caught; a subprocess test expects an abort, not a hang | §4.3, §5.2, §5.3, §5.6, §10 (`a_panic_in_pool_code_aborts`), §12 RENDER-28 |
| P5 | The spin and wake policy burns P-cores and battery without speeding anything up | **Accept (a) and (c) and the CPU readout; reject the per-block thread count and `wfe`** | (a) A worker leaves when the queue is empty and `left ≤ running`, so nobody spins through the tail. (b) `want` is capped by the schedule's width, so chains wake nobody. A count derived from the last block's time is rejected: every node runs every block, so the work hardly falls in quiet passages, and a following count would leave the first heavy block after one on too few threads. (c) The default stays within one L2 cluster: 4 on the M1 Pro, not 8. `wfe` is inline assembly for an unmeasured saving: slice 4. G12 prints engine-thread CPU per block | §3.2, §3.5, §5.5, §6.3, §9.1, §9.2, §11 |
| P6 | Exports and agents' range renders beside playback can drop out the live engine | **Accept** | Realtime priority doesn't win the L2 or DRAM, and p50 is memory-bound. A render started while the project plays (or a CLI render that finds the lock held) runs at utility QoS on the E-core count; an explicit count wins; `--export` is judged | §7, §9.2, §12 RENDER-28a, R14 |
| P7 | Plugins migrate between threads every block, so their thread-locals allocate on realtime workers mid-song | **Accept the measurement; defer home threads to slice 4 behind a trigger** | CLAP allows several audio threads, one call at a time; other hosts migrate plugins; first touches are per binary and per thread, so bounded and mostly early. Home threads need per-thread queues and a rule to wake a parked home thread. The probe gains a `thread_local`, with-plugins runs on 4 threads for 10 minutes, and the one-cluster cap keeps the caches shared | §6.3, §9.2, §10, §11, §14, R12 |
| P8 | The random-partition test probably degenerates to one partition, and the fixture runs barely overlap | **Accept** | Random labels plus a cycle merge collapse to one partition. Random cuts of a random topological order are convex by construction; every worker is woken; the test asserts that two threads ran tasks and that a consumer ran on another thread than a producer. The label test stays separately | §10 |
| P9 | With buffers over 128 frames, workers park and wake again for every block of a callback | **Accept** | The device thread sets `again` before each run when another follows in the callback; workers spin (bounded) between them and sleep after the last; a 512-frame run is recorded | §5.1, §5.2, §5.5, §9.2, §10 |
| P10 | Wake signals pile up while a worker is starved, then cause a long realtime spin storm | **Accept** | One padded `parked` flag per worker, signalled only on `swap(false) == true`: at most one pending signal | §5.1, §5.4, §5.5, §10, §11 |
| P11 | The ready order ignores the critical path, so bus and return tracks start last | **Accept** | Bottom levels are O(E) at build time; roots and consumer lists are sorted by them; `--offline --sends` shows it | §3.2, §3.5, §3.6, §9.3, §10 |
| P12 | `canon()` doesn't cover the device thread's pre-pass and mixing | **Accept** | `canon()` at the top of `Callback::fill` and `render_block_on`; a debug check in the post-pass; a test with FZ set before `fill`; the real IO thread's capture of `resample` compared with its export | §2.3, §4.2, §6.2, §9.2, §10, §12 RENDER-2a |
| P13 | Retired players never drop their pools, so each new player adds N−1 realtime threads | **Accept, without a timeout** | `Player::close()`; the `m2-play` thread drops the stream, which joins the pool off the audio thread. A stuck callback's drop simply never returns, losing only that thread, as today, so no timeout is needed | §6.5, §10, §11, §12 RENDER-28a |
| P14 | Non-realtime pools spin-wait under oversubscription in the silent player and tests | **Accept** | The silent player and tests default to one thread; the bounded waits of P1 also park a non-realtime device thread after `stuck` | §6.4, §5.5 |
| P15 | Module-level cycle merging serialises whole tracks, and cycles that exist only in a window persist for the plan's life | **Accept** | The depth split is a small dynamic programme over plan order, applied only inside strongly connected components, so ordinary tracks stay one partition each, and a window-only cycle costs a few small partitions, not a serial pair | §3.2, §3.4, §3.6, §10, §12 RENDER-2 |

---

## 16. Build slices

Coordination: slices 1 and 2 touch only `core/runtime`, `core/play`, one file in `crates/editor` (`render.rs`, which the
other workflow isn't editing now), `apps/m2`'s `--threads`, and in slice 2 the three lines of `retire()` in
`crates/server/src/graph.rs`, which rebase onto that workflow's changes. Slice 3 adds one new file to `crates/gates`, one
registration line, and the `--sends` option of `reference::write`. 8a starts once step 7's slice 2 has landed, and can run
beside step 7's slice 3, since they share no files. It rebases onto step 7's slice 4, which touches `PluginNode` and
export.

### Slice 0: the spike (2 days, on a branch, thrown away)

On step 7's slice 1 (per-instance stores), take per-module partitions straight from `Flat.order` and a crude pool
(`std::thread::scope` per block, which allocates and is offline only). Render p50 offline from 3:00 on 1, 2, 3, 4 and 8
threads. It gives measurements 1 to 3 of §14 before any design commitment is coded. If the 4-thread speed-up is under
2.5×, §13's R1 mitigations come first.

### Slice 1: the executor, offline (about 2 weeks)

**What lands:**
- `parts.rs` (the schedule, §3): labels for ghosts and fades, the depth split, bottom levels and width, built in
  `prepare` and `Renderer::new`;
- `pool.rs` and `pool/block.rs` (§5): the leave rule, `patience` and `stuck`, the `parked` flags, `again`, the abort
  guard and the counters, with the sync shim and the loom models;
- `view.rs` (`GraphView`, `Ports`, the debug owner checks);
- `fpenv.rs`, with `canon()` at the top of `render_block_on`;
- `step()` split out of `Graph::process`, with the voice loop kept;
- `Renderer::*_on`, and `Engine::process_on`;
- `Callback` holding `Pool::this_thread()`, so live behaviour is unchanged;
- `Render::threads`, `m2 render --threads`, `M2_THREADS`, and the default counts of §7;
- `order_after` deleted.

Contract: RENDER-2, RENDER-2a, RENDER-32's *Why*.

**Tests:** every §10 row except the live, plugin-on-worker, player and `no_alloc` rows.

**Stays green because:** one thread runs a topological order, so every offline render keeps its bits (`renders.json`),
and every existing test runs unchanged through `Pool::this_thread()`. The live engine still runs on its one thread.

**Proves:** "each pure fixture renders the same bits on 1 thread and on N", and the CI gate.

Size *estimate*: about +1,100 / −200 lines in `core/runtime`, and about 80 in `crates/editor` and `apps/m2`.

### Slice 2: live on realtime workers (about 1 week)

**What lands:**
- `Options` and `start_with`, `BufferSize::Fixed`, `driven_on`;
- `rt.rs` (device id, workgroup, recommendation, cluster size, period, time-constraint policy, join and leave, overload
  listener);
- the live pool in `Callback`, with `canon()` at the top of `fill`, the wake at the block line and `again`;
- `load.rs` (the histogram, dropout counts and the pool's counters) and `Player::load()`;
- a one-thread non-realtime pool for the silent player;
- `Player::close()`, and the server's `retire()` calling it.

Contract: RENDER-28, RENDER-28a, HOST-2 and the sketch.

**Tests:** `live_on_4_threads_is_the_export_on_1`, `the_pre_pass_runs_in_the_canonical_mode`, `no_alloc.rs` on workers,
the FZ, blocking and stuck probes, `plugins_on_workers_are_told_the_truth`, `a_closed_player_joins_its_workers`,
`activations_on_4_threads_match_1`, and `live.rs`'s random edits and jumps on 4 threads.

**Stays green because:** the bits are the same by slice 1, every live test is already parameterised by pool, the silent
player keeps one thread, and `Player::start()` reads the same environment as before.

**Proves:** the live half of RENDER-2, and RENDER-28 on every engine thread.

Size *estimate*: about +700 lines in `core/play`, and about 120 in `core/plugins` (the probe ids).

### Slice 3: the gate (about 1 week, plus the recorded runs)

**What lands:** `crates/gates/src/realtime.rs` (§9.2, §9.3) with `--server`, `--export`, `--capture`, `--like-m1`,
`--sweep`, `--song` and `--offline --sends`; `Renderer::profile_parts`; `--threads N` for the other gates (the engine
rig's `driven_on`, and the server rig's `M2_THREADS`); the recorded runs (§9.4).

Contract: gate 8's wording, the Today lines with the numbers, and song-as-program.

**Proves:** gate 8a: p50 with no dropout at 128 frames, from the first callback, with p99 at most 70%, alone, while
edited and beside an export, on a base M1; a 64-frame run recorded.

Size *estimate*: about +750 lines.

### Slice 4: only if slice 3's gate fails (each item on its own measurement)

In order of likely payoff:
1. move each mixer strip into the partition of the track it reads, when that track is its only cross-partition input;
2. pace the dropper's `munmap`s (R13);
3. spin between callbacks for a measured time;
4. QoS classes for the actor, compile and dropper threads;
5. home threads for plugin partitions (R12);
6. `wfe` spinning;
7. move audio player fills into their lane's partition (the stems variant);
8. Panic through the pool;
9. voice splitting (§8).

**In all,** about 4 weeks, with the recorded runs last.

---

## 17. Owner questions

Only the decisions this design can't make on its own. Each has the design's answer.

| # | Question | The design's answer |
|---|---|---|
| Q1 | Gate 8 is judged on the reference Mac, a base M1. Your M1 Pro has twice the memory bandwidth, two L2 caches and four spare P-cores, so it can pass where the M1 fails. Can the judged runs use a base M1 (an M1 Mac mini is enough)? | Yes: borrow or buy a used M1 Mac mini for the recorded runs. Until then the M1 Pro runs under `--like-m1` and the verdict says "provisional"; the gate isn't marked passed on it |
| Q2 | Should the app open the device at a small buffer (128 or 256 frames) by default, now that it can? | Not in 8a: keep the device's own default, and decide with the Settings window. Small buffers matter most for playing live (gaps.md #24), which comes after audition |
