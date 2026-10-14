# Step 8a, mapped: partitions on a thread pool

A read-only survey for step 8a against `docs/architecture.md` (normative) at HEAD `cbf5eb9`, and
step 7's design (`.git/m2-step7/design-7.md`, revision 2), which will be built first. `core/runtime`,
`core/play`, `core/plugins`, `core/kernel`, `core/ir` and `crates/gates` are unchanged in the working
tree, so their pointers hold for HEAD and the tree; `core/session` and `crates/server` are being edited
by another workflow, so their pointers are to HEAD (`git show HEAD:…`). Nothing was built or run, and no cargo command
was used. Header and crate facts come from the local macOS SDK, `~/.cargo/registry` (cpal 0.18.2,
wasmtime 49.0.2, mach2 0.6.0, objc2-core-audio 0.3.2, libc 0.2.190) and `Cargo.lock` at HEAD. Every
cost marked *estimate* is unmeasured; §11 lists what to measure first.

Step 8a's line in the order of work:

> **8a:** parallel partitions on a thread pool, right after the arena, with renders that don't depend
> on the thread count.

Its gate (acceptance gates, step 8):

> p50 plays without dropouts at 128 frames and 48 kHz on the reference Mac, with the audio callback at
> no more than 70% of its block budget at p99 over 10 minutes; a 64-frame run is recorded (8a). Each
> pure fixture renders the same bits on 1 thread and on N.

And the CI gate that keeps running from 8a on: "renders don't depend on the thread count, from step
8a (RENDER-2)".

---

## 0. The short version

- **Today nothing can run in parallel.** One `Renderer` holds one wasmtime `Store` with every kernel
  instance in it (`core/runtime/src/lib.rs:1295`), and every kernel call takes `&mut Store`. A `Store`
  is not `Sync`. Step 7's slice 1 gives each kernel instance its own `Store` (design-7 §11.1). That is
  the precondition for 8a: with it, two partitions can call into wasm at once.
- **Any topological order gives the same bits.** Nodes talk only through their output buffers; no node
  reads another node's state; cells and lane schedulers run before the graph and feedback rings are
  written after it (design-7 §6.6 says the same, and the code bears it out, §4.1). So RENDER-2 holds
  under a parallel schedule if, and only if: every dependency edge is a happens-before edge; no two
  tasks write the same memory; the engine's own sums keep their order; and every thread runs with the
  same floating-point environment.
- **The only engine-level sum today is `poly`'s voice mix** (`lib.rs:1123-1150`): `out.fill(0.0)`, then
  `+=` each voice in voice-index order. Bus sums are std's `sum`, a left fold of `add` kernels
  (`std/src/math.ts:65-68`), whose order the graph fixes. Overlay mixes (MON-1) come in 8b and are
  consumer-side by the contract's own wording.
- **The parallelism is in the tracks.** p50 has 50 tracks and about 6,100 kernel instances, 97% of them
  inside `poly` voices. Per block it makes about 3,000 kernel calls (*estimate*), about 2.3 ms on one
  core at 128 frames, 86% of the 2.67 ms budget (*estimate*, from the bench's 0.76 µs a call). One
  partition per track gives about 50 tasks of 20 to 100 µs each, enough for 4 cores. The mixer (50
  strips, 49 `add`s, the limiter) is a serial tail of about 100 calls. Voice-level splitting is not
  needed for p50; it is needed for one heavy instrument in a sparse song.
- **Partitions are build-local scheduling, never identity.** They come from `FNode.module`, with
  module-level cycles merged (tracks can import each other's exports, so the module graph can be
  cyclic while the node graph is not). Since any topological order gives the same bits, any convex
  partition gives the same bits too, which tests can exploit with random partitions.
- **rayon is out.** It isn't in the tree, its `spawn` and `scope` allocate, and its sleeping workers
  are woken through a mutex and condvar. A small custom pool fits RENDER-28: fixed threads, per-block
  epochs, dependency counters, a one-shot ready queue, spin inside a block, sleep on a Mach semaphore
  between blocks, the device thread working too and never sleeping.
- **macOS wants the workers to be realtime and in the device's audio workgroup.** CoreAudio gives each
  device an `os_workgroup_t` (`kAudioDevicePropertyIOThreadOSWorkgroup`, `'oswg'`); auxiliary threads
  that work to the IO cycle's deadline must `os_workgroup_join` it, and `os_workgroup_max_parallel_threads`
  says how many to make. cpal 0.18.2 exposes neither the workgroup nor the device id, but the bindings
  needed (mach2, objc2-core-audio) are already in `Cargo.lock` through cpal; the five `os_workgroup_*`
  functions need hand-written `extern "C"`. The app's minimum macOS is 11.0, where these exist.
- **The device buffer is never set today.** `Player::start` opens the default config
  (`core/play/src/lib.rs:545-549`), so the buffer is whatever the device has (512 frames is common).
  The gate needs `BufferSize::Fixed(128)`, which cpal supports (`cpal-0.18.2/src/host/coreaudio/macos/device.rs:1009`).
- **At 64 frames the budget halves for the same work.** The engine renders fixed 128-frame blocks
  (RENDER-33 needs that grid), so at 64-frame device buffers every second callback renders a whole
  block in a 1.33 ms period. The "recorded" 64-frame run should say so.
- **Nothing measures callback time yet.** The gates' rigs run in simulated time; the server rig's
  silent player sleeps between fills and keeps no timings; `core/runtime/examples/bench.rs` times one
  synth offline. Step 7's G7f adds a block-time measurement and an "audio-like thread" (design-7
  §11.2). 8a needs a realtime gate: a callback-duration histogram, CoreAudio overload notifications and
  callback-time gaps, on the device at 128 and 64 frames, plus a paced variant for machines and CI
  without a device.
- **Per-thread state that matters:** the FP control register (FZ and rounding must be the same on
  every DSP thread, or bits change); wasmtime's per-thread setup (Mach exception ports, TLS:
  `Engine::tls_eager_initialize()`); macOS thread-local storage, whose first touch on a thread mallocs
  outside Rust's allocator (so the allocation trap can't see it); catching panics per task; plugin
  audio roles, which are already per call, not per thread.

---

## 1. How a plan is executed today (HEAD)

### 1.1 One block, end to end

```
CoreAudio IO thread
  Callback::fill(data)                         core/play/src/lib.rs:265   (catch_unwind at 282)
    take_held (Replace / Unload)               :300
    Live::fill → [Resampler] → Engine::process core/runtime/src/engine.rs:261
      per frame: if pending empty → Engine::block(inbox, retire)          engine.rs:391
        retire stash, finished fade, head/blocked, inbox: messages at the block line
        Engine::render                                                     engine.rs:465
          current.render_block(clock, false)                               lib.rs:1418
            transport.advance()          → runs (≤ MAX_RUNS = 2, a jump splits a block)
            cells.apply(&mut graph)      → dirty slots, ramps (cells.rs:283)
            for each notes lane: scheduler.run(runs, release_all)          sched.rs:305
            block(clock)                 → beats/seconds per frame, plugin RunInfo   lib.rs:1365
            graph.process(store, block, WHOLE, ..)                          lib.rs:847
            output frames, non-finite silenced (live) or an error (strict)
          [fading.old.render_block_lent(..) + linear 5 ms mix]              engine.rs:495-516
          idle check (8 silent blocks, can_idle)
      pending.pop() per frame
  publish (seqlock)
```

Every structural edit runs the old and new renderers whole for two blocks (the 5 ms crossfade). Step
7 deletes that (§2).

### 1.2 Flat, the order, and the Graph

- `m2_ir::check` makes a `Flat { nodes, order, out, out_kind, sidechain }` (`core/ir/src/check.rs:134`).
  `order` is Kahn's algorithm, lowest index first (`check.rs:839`), over `FOp::deps()` (`check.rs:86`):
  a kernel's or plugin's port inputs, a poly's lane and its captures' sources. A feedback input isn't a
  dependency: the ring carries it one block later.
- Node indices are module order, then node order inside each module. Each `FNode` has `module` (the
  module key), which is what partitions can be built from.
- `Builder::graph` (`lib.rs:596`) turns the Flat into a `Graph { exec, order, bufs, versions, version,
  resets }` (`lib.rs:553`): one `Exec` per node and one `Vec<f32>` per output port (`channels × 128`).
  `versions[i]` is a const's version (it changes only through a cell) or `COPY` for everything else.

### 1.3 What each `Exec` reads and writes in a block (`Graph::process`, `lib.rs:847-998`)

The loop takes each node's output out of `bufs` (`mem::take`, `lib.rs:859`), so it can write it while
reading the others, then puts it back.

| Exec | Reads | Writes | State it keeps |
|---|---|---|---|
| `Control{slot}` | its lane (`ControlLane::value_at` per frame) | its buffer | none |
| `Audio(AudioNode)` | its lane's clip renders, the runs | its buffer | taps, fade-in, continuation |
| `Notes`, `Const` | — | — (a const's buffer changes only in `cells.apply`) | none |
| `Time` | the block's beats or seconds | its buffer | none |
| `Kernel` | each port input whose source version changed since it last saw it | its wasm memory, then its buffers | wasm memory in the renderer's one `Store` |
| `Poly` | its lane's events, `outer` buffers through captures | its voices' graphs, its buffer | voices, `born`, `plan` |
| `Plugin` | its lane's events, its `in` buffer, parameter inputs sampled every 16 frames | its buffer, its event lists | `Arc<PluginInstance>`, `keys`, `ended`, `release`, last-sent params |
| `Missing` | its input | its buffer | none |
| `Feedback` | its own ring | its buffer | the ring (one block) |
| `NoteField`, `Capture` | the voice's note; an outer buffer | its buffer | none (voices only) |

After the loop, every feedback ring is written from its input's buffer (`lib.rs:960-973`). So within
a block a feedback node always reads the previous block, whatever order the nodes ran in.

### 1.4 `poly` voices (`PolyNode::process`, `lib.rs:1073`)

1. Offs for notes already in voices; voices whose Off landed on this block line end if quiet and not
   `active`.
2. Allocation, in event order: the first free voice, else the oldest by `born` (`lib.rs:1102`).
3. `out.fill(0.0)` (`lib.rs:1123`), then **for each voice in index order**: the voice graph runs in
   segments split where its new note starts (a reused dirty voice is reset first, `k_reset` on the
   audio thread), and each segment's output is added: `*o += x` (`lib.rs:1142`), with the voice's peak.
4. A voice whose Off passed ends when its block peak is under −80 dB and no kernel in it is `active`.

The voice graphs are eagerly built: a 12-voice pluck is 12 × 13 instances. Only voices with a note (or
given one this block) run. The covered frames of a voice in a block are contiguous: from its first
segment's start to 128.

### 1.5 Cells and lanes

- **Cells** (`cells.rs:283`): at each block line, before the graph, ramps end and dirty slots are
  applied (`write_const`: a step bumps the node's version, a ramp is copied for one block). Voice cells
  are written into every voice's graph. Offline renders get none. Serial, and proportional to what
  changed.
- **Notes lanes**: every `NoteScheduler::run` runs before the graph, in slot order (`lib.rs:1429-1433`),
  filling its `Events`. During the graph they are read-only.
- **Control lanes** are read inside the graph by `Exec::Control` nodes: `Arc<ControlLane>`, read-only.
- **Audio lanes** are read inside the graph by `Exec::Audio` nodes (step 7 moves them before it, §2).

### 1.6 Why nothing can run in parallel today

- One `Store<StoreLimits>` per renderer (`lib.rs:1293-1296`); `KernelNode::process` and `reset` take
  `&mut Store` (`lib.rs:166-182`). Two kernel calls can't overlap.
- `Graph::process`, `PolyNode::process` and `PluginNode::process` take `bufs: &[Vec<Buf>]` over the whole
  graph while writing one node's buffer. Run on two threads at once, that is aliasing a `&[T]` with a
  `&mut T`: undefined behaviour, not just a race.
- The two-renderer crossfade lends plugin outputs between renderers (`plugins::lend`), so the old
  renderer's pass reads the new one's buffers.

---

## 2. What step 7 changes that 8a builds on

Design-7 does not mention partitions (it removes RENDER-32's "per partition" fallback because
partitions don't exist yet, design-7 §15). What it builds changes 8a's ground as follows.

| Step 7 (design-7) | What it means for 8a |
|---|---|
| One `Store` per kernel instance (`KernelInst`), and `Graph` functions lose their `store` argument (§11.1, slice 1) | The precondition: kernels in different partitions can run at once. A whole voice pool moves as one box, so voices can run on different threads too. |
| `Plan` and `Lanes` inside `Renderer` (§4.2); `Plan` built off the audio thread by `prepare` | The partition schedule is plan-side: built in `prepare`, sized there, retired with the plan. |
| Audio players fill once per lane before the graph (§8.2); schedulers persist and note ids are engine-wide (`next_note` passed to each `run` in plan lane order, §8.1) | Lanes stay a serial pre-pass on the device thread, so note ids and players stay deterministic. Every lane is read-only during the graph. |
| No two-renderer crossfade, no `lend`, activations never coalesce (§7) | One plan per block. Only ghosts run twice during a window, and no instance is processed by two nodes. |
| Ghosts and fade nodes appended to the plan; the window's order, then `order_after` swapped in by `end_fade` (§6.4-6.6, §7.3) | The schedule needs two forms per plan, the window's and the one after, swapped at `end_fade` the way the order is. Ghosts take their old module's partition, fades their consumer's. |
| Kahn over the window's graph with the cycle rule (§6.6) | The node graph is acyclic in the window too; the partition graph still needs its own cycle merge (§3.2). |
| `poisoned` set right after each stateful top-level node runs (§7.7) | Per node, inside its partition: no shared state. |
| `Exec::Capture(u16)` into plan-side `PolyNode.captures` (§5.1) | Capture sources are the poly's dependencies; the poly task reads them, read-only. |
| `Exec::Fade` copies its new source after the window and "takes its version" (§7.4) | A node writes its own `versions[i]` during the pass. Fine if only the owner writes it and readers are downstream. |
| `take_over` at a block line on the audio thread (§7.2) | Runs while the pool is quiescent (between blocks), which the protocol must guarantee (§5.2). |
| G7f measures `render_block` time on p50 (mean, p99, max) and drop contention with an "audio-like thread" (§11.2) | 8a's single-thread baseline, and a harness to extend (§8). |

So 8a lands on slice 2's plan. A measurement spike can run on slice 1 alone (per-instance stores, the
old whole-renderer activation) to get scaling numbers early, on a branch.

---

## 3. Where the parallelism is

### 3.1 The p50 workload

From std's sources (`std/src/instruments.ts`, the step 7 runtime notes' count) and
`core/session/src/reference.rs` (tracks cycle `delay(bass(…))`, `pluck(…)`, `reverb(pad(…))`; the song is
`master(mixer({...}))`):

| Part | Count | Instances | Kernel calls a block (*estimate*: active voices × voice kernels + top level) |
|---|---|---|---|
| `delay(bass)` | 17 | 58 each (4 voices × 14 + 2) | ~1.5 voices × 14 + 3 ≈ 24 each, 408 |
| `pluck` | 17 | 156 each (12 × 13) | ~5 × 13 + 1 ≈ 66 each, 1,122 |
| `reverb(pad)` | 16 | 145 each (12 × 12 + 1) | ~7 × 12 + 2 ≈ 86 each, 1,376 |
| mixer and master | 1 | 100 (50 gain, 49 add, 1 limit) | 100 |
| **total** | | **≈ 6,060** (contract: 6,176) | **≈ 3,000** |

At 0.76 µs a call (`core/runtime/examples/bench.rs`: 16 voices × 5 kernels at 44× real time, so 60.6 µs
for 80 calls a block, copies included), that is about **2.3 ms a block on one core**, 86% of the 2.67
ms budget at 128 frames (*estimate*; p50's kernels are a mix of lighter and heavier ones than the
bench's, and the active-voice counts are guesses). The per-track spread is about 20 to 100 µs. p99
will be higher than the mean: chord changes overlap releases, so voice counts spike.

**Memory, too.** Each kernel call touches its port buffers (1 KiB a port: `PORT_BYTES`, `lib.rs:75`),
its state and the host-side buffers, roughly 4 to 8 KiB. About 3,000 calls a block is 12 to 24 MB a
block (*estimate*), more than an M1 P-cluster's 12 MB L2, in about 3,000 separate wasm memories (TLB
pressure). Scaling from 1 to 4 threads may be sub-linear for this reason; measure it (§11).

### 3.2 Partitions

**What a partition is.** A set of top-level plan nodes that runs as one task, in plan order, on one
thread, within one block. Requirements:

1. **Every node is in exactly one partition**, so no two tasks write the same exec or buffer.
2. **Convex**: no dependency path leaves a partition and comes back into it. Otherwise the task would
   have to wait halfway through.
3. **The partition graph is acyclic**, with an edge P → Q when a node of Q reads a node of P.
4. **Inside a partition, nodes run in the global order filtered to the partition**, which is a
   topological order of it (by convexity and 3).

Any partition meeting these gives the same bits (§4.1). The choice is only about speed.

**A first algorithm (in `prepare`, off the audio thread; Tarjan over about 2,500 nodes is well under a
millisecond):**

1. Label each top-level node with its module key (`FNode.module`). A design-7 ghost takes its old node's
   module; a fade node takes its consumer's (so edge transforms are consumer-side, as RENDER-2 and MON-1
   put overlay mixes).
2. Build the quotient graph over labels from `deps()` (fade and ghost inputs included). Feedback inputs
   aren't edges.
3. Merge each strongly connected component into one partition, named by its first module in plan
   order. Tracks can import each other's exports (`track()`'s handle makes an `import` node in whichever
   module reads it, `core/script/sdk/core.ts:1622-1632`), so track A reading `B.x` while B reads `A.y`
   is node-acyclic and module-cyclic.
4. Compute each partition's dependency count and its sorted consumer list; the initially ready ones
   (no dependencies) in decreasing estimated cost.
5. Store it in the plan: `Schedule { parts: Box<[Part]>, ready: Box<[u16]>, counters: Box<[AtomicU16]> }`,
   `Part { order: Box<[u32]>, deps: u16, consumers: Box<[u16]>, cost: u32 }`, with a window form and an
   after form when the plan has ghosts or fades.

Deterministic data structures throughout (`Vec`, `BTreeMap`): nothing in the plan should come from
`HashMap` iteration, so a plan's schedule is reproducible for debugging and timing, even though bits
wouldn't change.

**Refinements, each only if measured to matter:**

- **The root module is one serial tail.** In p50 the `song` partition waits for every track, then runs
  50 strips, 49 `add`s in a chain and the limiter: about 100 calls, 50 to 80 µs (*estimate*), 2 to 3%
  of the budget. Ways to shorten it: absorb a node whose inputs all come from one other partition into
  that partition (each mixer strip into its track), or give the IR a native N-ary sum (one pass, inputs
  in order) in place of 49 kernel calls.
- **Tiny partitions** (a track that is one const and one kernel) can merge into their single consumer
  to save scheduling overhead.
- **Cost estimates** for ordering ready partitions: kernel count, voices × voice kernels, plugins
  weighted heavily; then an exponential average of measured task times, kept by the executor (one
  `mach_absolute_time` pair a task).

### 3.3 Voices

A `poly`'s active voices are independent: each voice graph reads the lane's events and the captured
outer buffers (read-only) and writes only its own graph. Only the allocation (step 2 of §1.4) and the
sum are serial.

To split a big `poly` across threads with the same bits:

1. allocate voices, serially, in the poly's task;
2. run the voice graphs as subtasks (chunks of voices), each recording its covered range and its peak;
3. when all are done, sum in voice-index order over each voice's covered range, `out.fill(0.0)` first.

That is the same sequence of `+=` per frame as today (segments of one voice are disjoint and
contiguous), so the bits are equal however the voices are chunked. The poly's task helps run its own
chunks while waiting, so this can't deadlock.

p50 doesn't need it (per-track tasks are 20 to 100 µs against a 1.87 ms target). A producer's song with
one 16-voice supersaw pad would: that one task would be the critical path. Lay out the data for it in
8a (per-voice covered range, sum after), and build the split when a gate or a song shows the need.

Finer grains (branches inside a voice graph, single kernels) cost more in scheduling than they gain.

### 3.4 What stays serial (Amdahl)

| Where | Work | Cost a block at p50 (*estimate*) |
|---|---|---|
| before the graph, on the device thread | messages, `transport.advance`, `cells.apply` (dirty only), 50 `scheduler.run`s, audio player fills (step 7), `block()` (128 × `beat_at`, plugin run info) | 20 to 50 µs |
| scheduling | 51 task handoffs (a few atomics each), waking up to 3 workers (a Mach trap each) | 15 to 30 µs |
| the root partition | 100 kernel calls | 50 to 80 µs |
| after the graph | feedback ring writes, output frames and non-finite check, idle check, `poisoned` flags (step 7) | 5 to 20 µs |

About 100 to 180 µs serial. With 4 threads that puts p50 near 0.75 to 0.9 ms a block (*estimate*: 2.3
ms / 4 plus 10 to 20% imbalance plus the serial part), about 30% of the 128-frame budget and well under
the gate's 70% (1.87 ms), with room for p99 spikes. At 64 frames the rendering callback has 1.33 ms, so
the same 0.9 ms is about 65% of it: borderline, which is why the gate only records that run.

| Threads (device thread + workers) | p50 a block (*estimate*) | at 128 frames (2,667 µs) | at 64 frames (1,333 µs for a block) |
|---|---|---|---|
| 1 | ~2,300 µs | 86% | over |
| 2 | ~1,300 µs | 49% | 98% |
| 3 | ~950 µs | 36% | 71% |
| 4 | ~800 µs | 30% | 60% |

---

## 4. What must be deterministic (RENDER-2)

> The output MUST NOT depend on thread count or scheduling: every sum (buses, `poly` voices, overlay
> mixes) runs in plan order in the consuming partition.

### 4.1 Why any schedule gives the same bits, and the exact conditions

In one block, a node's output is a function of its inputs' buffers (this block), its own state, the
block's timing, the lanes and the cells. Feedback is the one edge into the past, and it is written after
every node has run. So two executions that run every node once, each after its dependencies, give the
same bits, provided:

1. **Happens-before along every edge.** A consumer reads a producer's buffer only after the producer's
   writes are visible: Release on task completion, Acquire before the consumer starts.
2. **No two tasks write the same memory.** Each node is in one partition; a node writes only its own
   exec, buffers and `versions[i]`.
3. **Shared inputs are frozen during the pass**: `Block`, lane events, control lanes, audio player
   outputs, cell values and const buffers (written in `cells.apply`, before), `versions` of consts.
4. **Post-pass work stays after the join**: feedback ring writes (a feedback node and its input can be in
   different partitions; the ring must be read before it is written), output frames, counters, the
   trace.
5. **Engine-level sums keep their order** (§4.2).
6. **The FP environment is the same on every thread** (§4.3).
7. **Nothing reads time or the thread**: no "skip voices when late", no adaptive quality, no thread-count
   branches in DSP code. A late block is a dropout, never a different render (RENDER-33 needs the live
   bits too).

Checked against HEAD's code: `Graph.version` changes only in `write_const` (cells, before the pass);
`Exec::Plugin` reads only its inputs (all in `deps()`), its lane's events and its own instance; plugin
instances are never shared between nodes after design-7 (no `lend`); kernels share no memory (each has
its own instance and, after slice 1, its own store; a std kernel's `static mut HOST` lives in its own
instance's memory). Nothing else in a block crosses nodes.

### 4.2 Sums

- **`poly` voices.** In voice-index order, starting from `out.fill(0.0)`. The zero fill is load-bearing:
  `0.0 + x` is `x` for every `x` except `-0.0` (`0.0 + -0.0 == +0.0`), so copying the first voice
  instead of adding it would change sign bits. A split poly (§3.3) sums after its voices finish, in
  the same order.
- **Buses.** std's `sum(...xs)` is `xs.slice(1).reduce((acc, x) => add(acc, x), xs[0])`
  (`std/src/math.ts:65-68`), and `mixer` sums its strips in the caller's key order
  (`std/src/mix.ts:31-41`). Each `add` is a node; the chain's order is the graph's. Nothing to do,
  except that a native N-ary sum, if added (§3.2), must add its inputs in input order inside one task.
- **Overlay mixes (MON-1, 8b).** An overlay gain is on a consumer port: a multiply where the consumer
  reads. Put it in the consumer's partition (a node like `Exec::Fade`, labelled by its consumer, §3.2).
  Solo and mute never sum into a shared buffer.
- **Never** accumulate into a buffer from two tasks, and never use atomic float adds: completion order
  would change the sum.

### 4.3 The FP environment, and denormals

- **WebAssembly requires IEEE 754 subnormals.** Cranelift compiles `f32` ops to native aarch64
  instructions, which obey `FPCR`: FZ (flush-to-zero), DN (default NaN), RMode (rounding). If any DSP
  thread had FZ set or a non-default rounding mode, its kernels would compute other bits, and nothing
  would say so.
- **Today it holds by accident.** New macOS threads start with the default `FPCR`. But a hosted plugin
  can set FZ in its `process` and not restore it (JUCE's `ScopedNoDenormals` does restore; not every
  plugin uses it), and nobody has checked what CoreAudio's IO thread has. RENDER-33's test drives the
  callback from a test thread (`core/play/tests/bit_identity.rs`), never the real IO thread, so a
  difference there would go unseen.
- **For 8a:** set `FPCR` to its canonical value (RMode nearest, FZ, FZ16, DN, AH off) at each worker's
  start and at the start of every block on every thread that runs DSP, and restore it after every plugin
  call on that thread (`mrs`/`msr fpcr` through `asm!`, a few cycles; RENDER-2 only covers pure songs,
  but a pure partition can run after a plugin on the same worker in an impure song). Debug builds assert
  it after each task.
- **Denormal speed.** std's kernels flush recursive state below 1e-15 (−300 dB) themselves
  (`std/kernels-src/src/fx.rs:31-35`), so tails end in exact zeros instead of crawling through
  subnormals, and FZ isn't needed for speed. Apple Silicon is believed to handle subnormals without the
  microcode stalls of older x86 parts; worth one measurement on the reference M1 with a kernel that
  doesn't flush (§11).
- **Unchanged:** relaxed SIMD stays off and NaN canonicalisation off (`core/runtime/src/packages.rs:93-98`).
  A NaN is an error in strict mode and silenced live.

### 4.4 Per-thread state, as an inventory

| State | Where | What 8a does |
|---|---|---|
| `FPCR` | each thread | canonical at start, each block and after each plugin call (§4.3) |
| wasmtime's per-thread trap setup | `lazy_per_thread_init`: on macOS with Mach ports (the default, `Config::macos_use_mach_ports`), `thread_set_exception_ports` on the thread's first wasm call (`wasmtime-49.0.2/src/runtime/vm/sys/unix/machports.rs:496`); wasmtime says it costs "a few hundred microseconds" | call `wasmtime::Engine::tls_eager_initialize()` (`src/engine.rs:799`) at each worker's start, then one real wasm call, before the worker takes blocks |
| macOS thread-local storage | the first touch of a TLV on a thread allocates its block with the system `malloc` (`core/plugins/tests/no_blocking.rs` says "a thread's first one allocates on macOS"), which Rust's global allocator never sees, so `no_alloc.rs`'s trap can't catch it | touch everything at worker start (the wasm call above covers wasmtime's TLS); pool code uses no `thread_local!` |
| the native stack | wasm runs on the calling thread's stack (wasmtime's `max_wasm_stack` defaults to 512 KiB); plugins too | spawn workers with 2 MiB stacks or more (Rust's default) |
| a plugin's audio role | `HostData.audio_owner` is set to the calling thread's token on every `process` and `reset` (`core/plugins/src/audio.rs:35-36`), and CLAP's `is_audio_thread` compares with it (`audio.rs:179-181`) | nothing: an instance can be processed from any pool thread, one call at a time, and is told the truth |
| a Rust panic | today caught once at the callback (`core/play/src/lib.rs:282`) | caught per task on every pool thread; the task counts as failed and completes, so the block always finishes (§5.4) |
| the allocation trap | `no_alloc.rs` sets a thread-local `IN_AUDIO` flag on the callback's thread | a pool start hook marks each worker, so the trap covers every pool thread |
| CPU placement | no hard affinity on Apple Silicon (`THREAD_AFFINITY_POLICY` is a hint, unsupported there) | the workgroup and the realtime policy keep workers on P-cores (§6.2) |

### 4.5 Errors in strict mode

Offline renders are strict: a non-finite sample or a trap fails the render, naming the node
(`strict_mode_names_the_kernel_that_broke`, `core/runtime/src/tests.rs:451`). In parallel, a failed
partition's dependents are skipped (they would read garbage), and independent partitions may also
fail. Report the error of the failed node earliest in the global order. That is exactly the serial
render's error: every node before it in the order ran fine in serial, so it runs fine in parallel, and
that node's inputs are the same. The error path may allocate (`anyhow`), as today's does.

Live mode keeps today's meaning: a trap makes the plan `broken` (silent) until the next activation,
and step 7's `poisoned` flag resets only the box that failed.

### 4.6 The tests that prove it

- **Thread count:** each pure fixture rendered offline on 1, 2, 3, 4 and 7 threads gives the hashes in
  `core/session/tests/renders.json` (checked by `fixture_renders_change_only_with_stds_version`,
  `core/session/src/gate_tests.rs:372` at HEAD). 7 is odd on purpose: more threads than partitions in small
  fixtures, uneven chunks.
- **Live against offline:** `live_is_the_export_on_every_pure_fixture` (`core/play/tests/bit_identity.rs`)
  with the live engine on N threads, the export on 1.
- **Any partition, any order:** a test mode that builds random convex partitions (random merges of
  modules, random absorptions) and runs ready tasks in a seeded random order with random yields; every
  render equal to the serial one. This is RENDER-2's strongest test, and it also catches a missing
  dependency edge.
- **Synthetic graphs that stress the rules:** two tracks importing each other's exports (a module cycle),
  a feedback whose input is in another partition, a capture from another module into a voice, a
  sidechain, a ghost and a fade in a window, a `poly` split across threads.
- **References:** p16 and p50 offline on 1 and N threads, compared by hash.
- **FP environment:** a test that sets FZ on a pool thread before a block (simulating a misbehaving
  plugin) and checks that the render is still bit-exact.

---

## 5. Realtime constraints, and the pool

### 5.1 The rules (RENDER-28, extended to every pool thread)

- No allocation or deallocation on any pool thread during a block, and none at all after start-up.
  Nothing a task touches is dropped there: replaced objects still leave through the device thread's
  retire path.
- No locks (mutex, rwlock, condvar, blocking channel) on any path a block takes.
- The device thread never sleeps and never waits for a sleeping thread. It may wait only for a task
  that is already running, and while it waits with nothing ready, it spins.
- The only system call in a block is waking a sleeping worker: a Mach `semaphore_signal`, a trap that
  never blocks the signaller. `mach_absolute_time` reads the commpage and isn't a system call.
- No I/O, no logging, no `thread_local!` in the pool's code; one grep test keeps it so, like
  `core/plugins/tests/no_blocking.rs`.
- Workers are realtime threads, joined to the device's workgroup (§6).
- No page faults in steady state (kernel memory is touched by prewarm, `lib.rs:1006`), and no
  `mmap`/`munmap` on pool threads.

### 5.2 The handoff, block by block

```text
device thread, at a block line (after messages and take_over):
    serial pre-pass: cells, schedulers, players, block timing
    for p in parts: counters[p] = parts[p].deps        (Relaxed; no task of the last block remains, see below)
    job = { plan, block, lanes, epoch + 1 }            (in pool memory; Release)
    push the initially ready partitions
    wake min(sleepers, ready − 1) workers              (semaphore_signal each)
    loop:
        pop → run(p) → complete(p)
        else if remaining == 0 (Acquire): break
        else: spin
    serial post-pass: feedback rings, output, flags

complete(p):
    for q in parts[p].consumers: if counters[q].fetch_sub(1, AcqRel) == 1 { push(q) }
    remaining.fetch_sub(1, Release)                    ← the last thing a task does

worker:
    loop:
        wait for a new epoch: spin briefly, then sleepers += 1, recheck the epoch, semaphore_wait
        loop: pop → run → complete, until remaining == 0 for this epoch (spin while nothing is ready)
```

- **Quiescence.** Because `remaining` is decremented after a task's last touch of plan memory (its
  consumers' counters), `remaining == 0` seen with Acquire means no worker will touch the plan again in
  this block. Then the device thread may apply an `Activate`, a `SwapLane` or `end_fade`, and retire the
  old plan. Workers between blocks touch only pool memory (the epoch, the queue, the sleeper count).
- **The ready queue** lives in the pool, sized at start (say 1,024 partitions; `prepare` merges
  partitions beyond that). Each partition is pushed once per block, so a one-shot array with an atomic
  tail (reserve a slot, write it, mark it with the epoch) and an atomic head is enough; entries are
  partition indices tagged with the epoch, never pointers. A worker that pops reads the job pointer
  only after a successful pop of this epoch.
- **Lost wake-ups.** A Mach semaphore counts, so a signal that arrives before the wait isn't lost; the
  worker rechecks the epoch after announcing itself as a sleeper.
- **Wake latency.** Waking a sleeping realtime thread costs tens of microseconds on Apple Silicon when
  its core is idle (*estimate*). Hide it: wake the workers first thing in the callback, before the
  serial pre-pass; the device thread starts on ready partitions at once, and workers join as they wake
  (the scheduler is work-conserving, so a late worker only costs its share).
- **Spinning.** Inside a block, idle workers spin (`std::hint::spin_loop`, or `wfe` on a monitored load
  through `asm!` for less power) until the block's tasks are done; sleeping mid-block would put a
  wake-up on the critical path. Between blocks, spin for a configurable time (0 to start with, then
  measured), then sleep, so the P-cores are free for the UI, the compile and `prepare` for most of each
  period. Workgroup accounting counts spinning as work, which makes the system raise clocks: harmless
  for the deadline, worse for battery.
- **One thread** is the same code with no workers: the device thread runs every partition in queue
  order, with no system call.
- **Panic** (`Msg::Panic`) resets about 6,100 kernels on the device thread today, in one block
  (`Graph::reset`), each `k_reset` re-running `K::new` (a delay rebuilds its buffer): a likely dropout at
  p50. The same executor can run the reset by partition. Not in the gate, but cheap once the pool exists.

### 5.3 Making it safe Rust

The shared graph needs interior mutability: `exec[i]`, `bufs[i]` and `versions[i]` each in an
`UnsafeCell`, the graph `unsafe impl Sync`, and one small module (`core/runtime/src/exec.rs`, say) that
hands a task `&mut` to its own nodes and `&` to its dependencies' buffers. The protocol in §5.2 is the
proof. To keep it honest:

- debug builds keep an atomic owner per node (the running partition), set when a task starts and
  checked on every read of another node's buffer (the producer must be done in this epoch);
- `bufs: &[Vec<Buf>]` parameters become a view type whose `get(src)` is the one unsafe read (it touches
  `Graph::process`, `PolyNode::process`, `PluginNode::process` and `plugin_events`);
- the pool's protocol (counters, queue, epoch, sleep and wake) is modelled with `loom` as a
  dev-dependency, separately from DSP: loom can't run wasmtime, and Miri can't either.

Partition-owned storage (each partition its own exec and buffer arrays, inputs addressed by partition
and local index) would need the same unsafe for cross-partition reads, and would change design-7's
global exec indexing everywhere (move lists, `PlanInfo.shapes`, ghosts). Keep the global layout.

### 5.4 What can go wrong at run time

| Failure | What happens | Handling |
|---|---|---|
| A task panics on a worker | the block would never finish, and the device thread would spin forever | `catch_unwind` per task on every thread; the partition counts as failed and completes; the plan goes `broken` |
| A kernel traps | as today: an error | the partition fails (§4.5); live, the plan goes `broken` and the node `poisoned` |
| A worker is preempted holding a task | the device thread waits for it (it can't take a running task) | realtime policy and the workgroup make it rare; count "late joins" and their wait; a dropout, never a deadlock |
| More workers than P-cores | a task lands on an E-core (about a third of the speed) and becomes the critical path | threads = `os_workgroup_max_parallel_threads` (§6.2), capped by `hw.perflevel0.physicalcpu`; measured task costs order the queue |
| The device changes (AirPods, sample rate, buffer size) | a new workgroup, a new period | workers leave the old workgroup and join the new one at a quiescent point, and update their time-constraint policy |
| Two engines in one callback (`Replace`'s `LiveFade`, `core/play/src/lib.rs:344-397`) | two jobs in a row | the pool runs jobs one after the other; nothing to do |
| An export of a pure song beside playback | an offline pool competes with the realtime one | offline threads at QoS utility or user-initiated, never realtime (§6.3) |

---

## 6. macOS specifics

### 6.1 The device thread and its buffer

- cpal's CoreAudio output runs `Callback::fill` on the device's HAL IO thread, which CoreAudio makes a
  time-constraint thread in the device's workgroup.
- `Player::start` opens `default_output_config()` (`core/play/src/lib.rs:545`), whose `buffer_size` is
  `BufferSize::Default`: the device's current buffer, often 512 frames. The silent player
  (`M2_AUDIO=none`) uses `M2_AUDIO_FRAMES`, 480 by default (`lib.rs:582-600`), and the gates set 512
  (`crates/gates/src/main.rs:69-71`).
- cpal 0.18.2 sets a fixed size through `kAudioDevicePropertyBufferFrameSize` after checking the
  device's range (`cpal-0.18.2/src/host/coreaudio/macos/device.rs:1009-1036`). 8a needs a player option
  (and an env var for the gate) to open at 128 and 64 frames. The owner-facing buffer setting is gaps.md
  row 31 (the Settings window); 8a doesn't need the UI.
- **The 128-frame grid.** At 128-frame device buffers each callback renders exactly one engine block.
  At 64, every second callback renders a whole block and the other only pops frames (`Engine::process`,
  `engine.rs:261-275`), so the rendering callback has half a block's time. A device at another rate
  than the project (the resampler, RENDER-31) sometimes renders two blocks in one callback. The gate is
  at 48 kHz on both sides.

### 6.2 Audio workgroups

From the SDK headers (`AudioToolbox.framework/Headers/AudioWorkInterval.h`, `os/workgroup_object.h`,
`os/workgroup_base.h`, `CoreAudio.framework/Headers/AudioHardware.h`):

- "Core Audio devices own a workgroup whose lifetime is the same as that of the device", read through
  `kAudioDevicePropertyIOThreadOSWorkgroup` (`'oswg'`; the caller releases it with `os_release`), or
  through an output unit's `kAudioOutputUnitProperty_OSWorkgroup` (2015).
- An auxiliary realtime thread that works within the device's IO cycle "should be joined to the device
  thread's workgroup" with `os_workgroup_join`. Threads at their own cadence instead create an interval
  with `AudioWorkIntervalCreate` and mark each cycle with `os_workgroup_interval_start` and `_finish`.
  That second form fits a paced test harness without a device (§8.2).
- `os_workgroup_join(wg, &mut token) -> c_int` and `os_workgroup_leave(wg, &mut token)` are documented
  as "real-time safe". Errors: `EALREADY` (already in a workgroup that doesn't nest), `EINVAL`
  (cancelled). Threads must leave in the reverse order they joined, before they exit.
- The join token is `struct { uint32_t sig; char opaque[36]; }` on LP64 (`__OS_WORKGROUP_JOIN_TOKEN_SIZE__`),
  40 bytes, owned by the caller.
- `os_workgroup_max_parallel_threads(wg, NULL)` is "the system's recommendation for maximum number of
  threads" for the workgroup on this hardware, ignoring current load. Available from macOS 11.0, the
  app's minimum (`apps/music2-desktop/tauri.conf.json:41`).

**Getting the workgroup through cpal.** cpal's macOS `Device` keeps `audio_device_id` `pub(crate)`
(`device.rs:361-364`) and exposes no workgroup. Two ways:

- `DeviceTrait::id()` gives the device UID; `kAudioHardwarePropertyTranslateUIDToDevice` turns it into
  an `AudioDeviceID`; then `AudioObjectGetPropertyData(id, 'oswg')`. Robust.
- Or read `kAudioHardwarePropertyDefaultOutputDevice` right after opening, as cpal does, and compare
  UIDs. Simpler, racy if the default changes in between.

objc2-core-audio 0.3.2 (already in `Cargo.lock` through cpal) has `AudioObjectGetPropertyData`,
`kAudioDevicePropertyIOThreadOSWorkgroup` and `AudioObjectAddPropertyListener` (for overloads, §8.2).
The `os_workgroup_*` functions and `os_release` have no binding in the tree: about 20 lines of `extern
"C"` against libSystem.

### 6.3 Thread policy and QoS

- **Workers:** `thread_policy_set(mach_thread_self(), THREAD_TIME_CONSTRAINT_POLICY, …)` with period =
  the buffer's period in Mach ticks (24 MHz on Apple Silicon: 2.667 ms is 64,000 ticks), computation
  about half to three quarters of it, constraint = the period, preemptible; then join the workgroup.
  mach2 0.6.0 has `thread_policy_set`, `thread_time_constraint_policy`, `mach_thread_self`,
  `mach_absolute_time`, `mach_wait_until`, `semaphore_create`, `semaphore_signal`, `semaphore_wait` and
  `semaphore_timedwait`. Re-apply the policy when the buffer size changes.
- **Everything else** has default QoS today (nothing in the tree sets one). With 4 P-cores busy with
  audio for part of each period, the rest matters:
  - the actor, compile and `prepare`: `QOS_CLASS_USER_INITIATED` (the code plane's 200 ms budget);
  - the dropper `m2-play-drop` and offline renders beside playback: `QOS_CLASS_UTILITY`;
  - libc 0.2.190 has `pthread_set_qos_class_self_np` and `qos_class_t`.
- **Counts:** `hw.perflevel0.physicalcpu` through `sysctlbyname` (libc). This machine, an M1 Pro
  (MacBookPro18,1), has 8 P-cores and 2 E-cores; the reference M1 has 4 and 4.

### 6.4 The VM map lock

`munmap` takes the task's VM map lock for writing; a page fault takes it for reading. With one `Store`
per instance (step 7), every retired instance is its own unmap on the dropper thread, and a realtime
thread that faults meanwhile waits. Design-7 already measures this (R13, G7f's drop part). With four
realtime threads, any one faulting stalls the block. Keep the steady state fault-free (prewarm), and
let G7f's drop measurement include a pool.

---

## 7. Crates

| Crate | In the tree | Use for 8a |
|---|---|---|
| rayon | no | not realtime-safe: `spawn` and `scope` box their jobs, sleeping workers are woken through a mutex and condvar, a caller outside the pool blocks on a lock latch, and there is no say over spinning, priority beyond a start handler, or the workgroup |
| crossbeam-utils 0.8 | yes | `CachePadded` for counters and the queue's head and tail; `Backoff` |
| crossbeam-channel 0.5 | yes | stays for the player's messages (`try_send`/`try_recv`); not for per-block handoff |
| mach2 0.6 | yes, through cpal on Apple | semaphores, `thread_policy_set`, Mach time |
| objc2-core-audio 0.3 | yes, through cpal on Apple | the device's workgroup, overload listener, buffer size range |
| libc 0.2.190 | yes | QoS, `sysctlbyname`, `pthread_mach_thread_np` |
| wasmtime 49 | yes | `Engine::tls_eager_initialize`; nothing locks on a call (its global code registry `RwLock` is read only on a trap, `src/runtime/module/registry.rs:401-425`) |
| audio_thread_priority | no | cpal's optional `realtime` feature pulls it on Linux, Android and Windows only (`cpal-0.18.2/Cargo.toml`); on macOS it would add little over `thread_policy_set` |
| loom | no | a dev-dependency for the pool's protocol tests (§5.3) |
| assert_no_alloc | no | the repo has its own trap (`core/play/tests/no_alloc.rs`) |

**So: a small pool of our own,** about 400 to 600 lines (*estimate*), in two parts:

- `core/runtime/src/pool.rs` and `exec.rs`: the threads, the epoch, the queue, counters, sleep and wake,
  the schedule runner, `catch_unwind`, a start hook. Plain Rust and atomics, so offline renders, the CLI
  and tests use it too. Its only OS call is the semaphore (behind a small trait, so Linux CI can use a
  futex or `Thread::unpark`).
- `core/play`: the macOS realtime setup run by the start hook (the policy, the workgroup, `FPCR`, wasmtime's
  eager init), and the device options (buffer size, thread count). Core keeps no CoreAudio vocabulary
  (P10).

The API shape, roughly: `Renderer::render_block(clock, exec: &Exec)`, where `Exec` is `Serial` or a
`&Pool`; `Engine::process` gets the pool from the callback that owns it; offline renders make a pool
of `threads` (default: the P-core count) with no realtime setup; `m2 render` and the gates get
`--threads N`.

---

## 8. The measurement harness

### 8.1 What exists

- **`crates/gates`** (`m2-gates`): G1 to G11 on rigs. The engine rig (`src/engine.rs`) drives
  `Player::driven` in simulated time: frames pass only when the rig fills, and work off the audio thread
  is timed on the wall clock (`src/rig.rs:1-5`). The server rig runs the app's server on the silent
  player in real time. `main.rs` forces `M2_AUDIO=none` and 512-frame buffers before anything starts
  (`main.rs:69-71`). `report.rs` prints checks with pass, fail, skip or info, and the build profile.
  `--reference 50` picks p50, `--minutes M` the loop length.
- **Nothing measures callback time.** Neither rig records how long a fill takes, and the silent player
  doesn't report late fills.
- **`core/runtime/examples/bench.rs`**: one 16-voice synth, offline, against native code.
- **Step 7's G7f** (`crates/gates/src/store.rs`, new, design-7 §11.2): `render_block` time on p50 from
  3:00 over 11,250 blocks (mean, p99, max), and an audio-like thread rendering a small song in 2.67 ms
  periods while a renderer drops. After slice 2 it plays p50 at 3:00 for 30 s and compares the p99
  callback with HEAD's.
- **`core/play/tests/no_alloc.rs`**: the allocation trap on the callback's thread only.

### 8.2 What 8a's gate needs: `m2-gates realtime` (G12)

Two modes, the same measurement:

- **`--device`**: opens CoreAudio at `--frames 128` (and 64) through the player, with `--threads N`, and
  plays p50 for `--minutes 10`. It must bypass the binary's `M2_AUDIO=none`, through an explicit player
  option, never the environment. The callback computes the song and writes silence to the device by
  default (dropouts are about time, not sound), with `--audible` to listen.
- **paced** (the default where there is no device, and in CI): the callback runs on a thread with the
  time-constraint policy, woken each period by `mach_wait_until`, with an interval workgroup from
  `AudioWorkIntervalCreate` that the pool joins (Apple's case 3). It approximates the device run; it
  can't see device overloads.

Measured:

1. **Callback duration**, `mach_absolute_time` at entry and exit of `Callback::fill`, into a fixed
   histogram (1 µs buckets to two periods, plus an overflow count) the callback owns and publishes
   through a seqlock: no allocation. Printed: p50, p99, p99.9, max, as microseconds and as a fraction of
   the period. The bound: **p99 ≤ 70% of the period** (1,867 µs at 128 frames and 48 kHz). Ten minutes
   at 128 frames is 225,000 callbacks, so p99 lets 2,250 be slower than 1,867 µs, but none late.
2. **Dropouts**, three signals, any one counts:
   - CoreAudio's overload notification: a property listener on `kAudioDeviceProcessorOverload` on the
     device (objc2-core-audio's `AudioObjectAddPropertyListener`; the listener only bumps an atomic);
   - a callback longer than the period;
   - a gap in the callbacks' host times (cpal's `OutputCallbackInfo::timestamp().callback`, `cpal-0.18.2/src/timestamp.rs:258-266`) of more than 1.5
     periods.

   The bound: **zero**.
3. **For the record:** the same at 64 frames, with each callback's duration against its own period and
   the rendering callbacks reported apart (they do all the work, §6.1); a sweep over 1, 2, 3 and 4
   threads; the share of each block spent in the serial pre-pass, in tasks, waiting for the last task,
   and waking workers; wake latency (signal to the worker's first pop); late joins; per-partition time
   for the slowest ten partitions.
4. **What played:** p50 looped over its 512 beats (4:02), so the 10 minutes include wraps; the transport
   checked to have moved by exactly the frames played; `non_finite` and `crashes` zero.

**Running it on this machine.** The reference is a base M1 (4 P-cores at 3.2 GHz, 12 MB L2 for the
cluster); this M1 Pro has the same Firestorm cores at the same clock, 8 of them in two clusters with 24
MB of L2 between them. A run here with the pool capped at 4 threads approximates the M1 for the gate,
and should say so in its first line, as step 6's G11 did. The load on this machine is high (load
average 9 to 15 during this survey), which a recorded run must avoid.

### 8.3 The thread-count gate in CI

"Each pure fixture renders the same bits on 1 thread and on N": the tests of §4.6, run in `cargo test
--workspace`, with N fixed (not the machine's core count) so CI is the same everywhere.

---

## 9. Contract edits 8a needs

| Rule | Now | Proposed |
|---|---|---|
| RENDER-2 | "every sum … runs in plan order in the consuming partition" | define a partition (a convex set of plan nodes run as one task in plan order, built per plan, never identity); say any topological schedule must give the same bits; name the `poly` sum's order and its zero start |
| RENDER-2a (new) | — | every thread that runs DSP uses the canonical FP environment (round to nearest, no flush-to-zero, no default NaN), set at each block and after each plugin call |
| RENDER-28 | "The audio thread MUST NOT allocate, lock, do I/O or wait on a compile." | the same for every engine thread (the device's and the pool's); the device thread never sleeps or waits for a sleeping thread; the one system call in a block is a non-blocking wake; debug and CI traps cover every pool thread |
| RENDER-28a (new) | — | pool threads are realtime and join the device's audio workgroup; their count comes from the workgroup's recommendation, capped by the P-core count; offline pools are never realtime |
| HOST-1, the `PluginHost` sketch | `process` and `reset` on "the audio thread" | on an engine thread: the device's or a pool worker, one call at a time per instance (CLAP's audio thread may differ between calls) |
| RENDER-32 | "… If that costs too much, one `Store` per partition." | design-7's edit (no shared store) stands; add the reason partitions need it: a `Store` can't be entered from two threads |
| `song-as-program.md` "Parallel (Not built yet)" | | the built form, once it lands |
| Acceptance gate 8 | as quoted | add how dropouts are counted (overload notifications, late callbacks, gaps) and that the 64-frame run is judged per rendering callback |

---

## 10. Risks and open questions

| # | Risk or question | Note |
|---|---|---|
| R1 | p50 single-thread is much slower than estimated, or scaling stops short of 4× (memory, TLB, the serial tail) | measure first (§11); then the root-tail fixes of §3.2; voice splitting doesn't help p50 |
| R2 | Wake latency of sleeping realtime workers eats the budget at 128 frames | wake first thing in the callback; measure; spin between blocks for a measured time if needed |
| R3 | Four realtime threads on a 4-P-core M1 starve the UI, the compile (code-plane budget) and `prepare` | workers sleep between blocks; QoS for the rest; a thread-count setting |
| R4 | A plugin leaves FZ set and changes kernel bits on the same worker | restore `FPCR` after every plugin call (§4.3) |
| R5 | A unsafe-aliasing bug in the shared graph | one module, debug owner checks, random-partition tests, loom for the protocol |
| R6 | The device or its workgroup changes while playing | rejoin at a quiescent point; a test with a simulated device change on the paced rig |
| R7 | The 64-frame run looks bad because half the callbacks render a whole block | it is recorded, not judged; pipelining a block ahead would add 128 frames of latency and is a separate decision |
| R8 | One heavy instrument dominates a producer's song (one task is the critical path) | voice splitting (§3.3), data laid out for it in 8a |
| R9 | Panic resets about 6,100 kernels on the device thread in one block | run the reset through the pool (§5.2) |
| Q1 (owner) | Should the thread count be a setting, like Logic's and Ableton's "processing threads", or always automatic? | automatic by default (the workgroup's recommendation), a setting later beside the buffer size (gaps.md 31) |
| Q2 (owner) | A CPU meter: the callback histogram makes a "DSP load" readout cheap | out of 8a; worth a gaps.md row |
| Q3 (design) | Spin or sleep between blocks? | sleep by default; the gate's run decides |

---

## 11. Measurements to take first

None were possible here (read-only, no cargo).

1. **p50 single-thread block time** at 3:00, release, on the reference M1: G7f's block-time part, after
   step 7's slice 1. Split it into the pre-pass, the graph, the root partition and the post-pass.
2. **Per-partition cost** on p50: per track type, mean and p99, and their spread, which decides whether
   per-module partitions balance on 4 threads.
3. **Scaling** 1, 2, 3, 4 (and 8 on the M1 Pro) threads, offline and in the paced rig, with a pool
   spike on slice 1.
4. **Wake latency** of a sleeping time-constraint worker in the workgroup, and the cost of
   `semaphore_signal` on the device thread.
5. **Subnormal cost** on the M1: a kernel without the −300 dB flush, decaying, against one with it.
6. **`FPCR` on CoreAudio's IO thread**, read in a callback; and after a Surge XT and an Airwindows
   `process` call.
7. **`os_workgroup_max_parallel_threads`** on the M1 and the M1 Pro, for the built-in output and for a
   USB interface.
8. **The 128-frame device buffer range** of the built-in output and of an interface.

---

## 12. Suggested order of building 8a

Each slice green; all after step 7's slice 2, except the spike.

0. **Spike (on a branch, after step 7's slice 1):** per-module partitions and a crude pool on the plain
   Flat order; offline p50 on 1 to 4 threads; numbers 1 to 3 of §11. Thrown away.
1. **The executor, offline:** `Schedule` in `prepare` (per module, SCC merge, window and after forms),
   the shared-graph view and its debug owner checks, the pool without realtime setup, strict-mode error
   choice, `--threads` for `m2 render` and the gates. Tests of §4.6 except live. Proves "each pure
   fixture renders the same bits on 1 thread and on N".
2. **Live:** the pool in `core/play` with the macOS setup (policy, workgroup, `FPCR`, wasmtime eager
   init, QoS for the other threads), the buffer-size option, the allocation trap on pool threads, the
   no-lock grep test, the bit-identity test on N threads, Panic through the pool.
3. **The gate:** `m2-gates realtime` with the device and paced modes, the histogram and the dropout
   signals; the recorded run at 128 and 64 frames on the reference Mac (or the M1 Pro capped at 4
   threads, saying so); contract edits of §9 with it.
4. **Only if 3 shows the need:** the root-tail fixes, voice splitting, spin between blocks.
