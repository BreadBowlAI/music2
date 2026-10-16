# Kernels, stores and memories: the design that scales (label design-scale)

Research only. Nothing was built, run or measured for this note.

Citations follow `code-map.md`:
- a bare path is under `/home/jerry/m2-runs/idea3/src` (step 7 at `ed7fc8e`); `cut1.diff:N` is a line of `/home/jerry/m2-runs/idea3/cut1.diff`;
- `W/`, `E/`, `CL/`, `CG/` are wasmtime 49.0.2's crates, as in `external.md` (`W` = `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wasmtime-49.0.2/src`);
- `plan.md` = `~/m2-runs/speed/plan.md`, `refresh.md` = `~/m2-runs/s8a/refresh.md`, `results.md` = `~/m2-runs/speed/prof/results.md`;
- `code-map §N` and `external §N` are the two fact notes in this folder.

"Estimate" marks arithmetic on measured numbers or on counts taken from the code. "Unverified" marks what I could not check. Linux numbers are information only (PLAT-1).

---

## 0. The design in one table

| | This design | Step 7 today |
|---|---|---|
| **The unit: a sandbox** | one wasmtime `Store`, one instance, one linear memory. It holds the kernel states of **one box**: one top-level kernel, or one voice pool's kernels from one package (every voice) | one `Store`, instance and memory per kernel instance (RENDER-32) |
| Sandboxes in p50 | **200**: 50 pools and 150 top-level kernels. 6,058 kernel states | 6,058 |
| Sandboxes in p50 × 10 | 2,000. Two renderers reserve 16,250 GiB, 12.4% of the address space | 60,580 |
| Memory layout | wasmtime's defaults (4 GiB reservation, 32 MiB guards) plus `memory_may_move(false)`. No bounds checks. Spectre guard on, which costs nothing there | P2: 32 MiB, explicit checks, Spectre guard off |
| Growth | only while a sandbox is born, off the audio thread. Then it is sealed and never grows again | any time, up to 32 MiB per kernel |
| A kernel's state | made by `k_new` at birth only; `k_reset` makes it again in the same room; freed when its sandbox drops | one `static mut` per kernel type per instance |
| Ports | buffers the host places in the sandbox. Inside a pool, a kernel reads another kernel's output where it is | each port its kernel's own buffer; every port copied in or out |
| Live edits | today's machinery, unchanged. A kept box moves by swap; a new one is born off the audio thread; a replaced one plays 10 ms as a ghost and drops off the audio thread | the same |
| 8a | a box is in one partition, so one thread enters its sandbox per block. No new rule | the same |
| Effect on p50, Linux (estimates, §11) | callback −650 to −830 µs (9% to 11%); footprint about 23 to 30 MB plus the host's own share (from 195 MB); build at most about 182 ms | 7,312 µs; 195 MB; 216 ms |
| Later (each adds a concept) | grouping a track's top-level kernels, with compaction; one call per voice segment; in-place reads at the top level; voice chunks across threads; shared asset pages | — |

**Idea 3: changed.** The sandbox is the box, not the track. §0.1 says why.

### 0.1 Why the box and not the track

1. **Only the thread that holds a Store can make a state in it, and nothing in it is freed before it drops.** A call needs `&mut Store` (`W/runtime/func/typed.rs:100-105`). wasmtime frees instances only when their Store drops (`W/runtime/store.rs:139-145, 2425-2457`), and a memory never shrinks (external §A1). So a new kernel state must be made off the audio thread, in a sandbox that isn't playing: a new one. And a replaced state's memory comes back only when everything beside it has gone too.
2. **So a sandbox should hold exactly what an activation keeps or replaces whole.** That is RENDER-25's box: a top-level kernel, a voice pool, a plugin or a ring (`core/runtime/src/plan.rs:690-712`). With the sandbox as the box, the edit machinery doesn't change.
3. **Per box keeps almost all of idea 3's gain on p50.** Pools hold 5,908 of p50's 6,058 kernel instances (97.5%) and 2,095 of its 2,245 `k_process` calls a block (93%) (code-map §6.2). 6,058 memories become 200, the 4 GiB layout fits with room to spare, and copies between voice kernels go.
4. **Per track needs one of two things, both worse in the core** (table below):
   - **D**: making new states inside the live sandbox, on the audio thread;
   - **E**: a new sandbox per edit for the track ("generations"). E falls back to per box whenever a track is built up one edit at a time: each insert leaves a sandbox pinned by the effect it added. So it beats per box only after an open, or in an export. Making it win needs compaction, which is a second new concept. Both are kept for later (§12, L1), behind a measured trigger.
5. **8a: a box is in exactly one partition** (`refresh.md:188`). A track can be split into several partitions by depth (`docs/designs/step8/design-8a.md:199-210`). A sandbox per track would need those partitions chained.

| Layout | Sandboxes in p50 (× 10) | Live edits | Verdict |
|---|---|---|---|
| A. One per kernel instance (today) | 6,058 (60,580) | today's | can't take 4 GiB memories: the 25% rule allows 4,033 a renderer (code-map §2.3) |
| **B. One per box (this design)** | **200 (2,000)** | today's, unchanged | **chosen** |
| C. One per track per package, rebuilt whole when the track changes | 51 (510) | every kept node of an edited track starts from init. Its delay and reverb tails are cut, against RENDER-25's "an edit must not cut the tails of what it didn't touch" (`docs/architecture.md:255`) | rejected |
| D. One per track per package; new states made in the live sandbox at the block line | 51 (510) | constructors run on the audio thread at activation, and the kit needs an allocator that frees. Room must be grown ahead, since a grow is an `mprotect` (`W/runtime/vm/memory/mmap.rs:191-217`). Fresh pages fault on first touch (external §A4), and freed holes fragment | rejected: it puts building work on the audio thread, whose activation block already takes 3.1 to 4.2 ms of a 5.3 ms bound on Linux (`docs/architecture.md:330`) |
| E. One per track per package per activation ("generations") | 51 when fresh; after edits, up to B's count | an edit's new states go into a new sandbox; kept states stay where they were born and pin it, with its replaced states' memory | later (L1), with compaction |
| F. One per renderer | 1 | leaks every replaced state (`W/runtime/store.rs:139-145`) | rejected; no parallel partitions either |
| G. One shared memory imported by many Stores | — | only shared memories can do it. Creating one is off by default and tier 2: "security CVEs will not be issued" (`W/config.rs:3316-3335`). It needs a std rebuilt with atomics (external §C4) | rejected |

---

## 1. The unit

A **sandbox** is one `Store`, one instance of one package's module, and that instance's one memory. It holds the states of one box:
- **a top-level kernel**: one state;
- **a voice pool**: for each package its voice graph uses, one sandbox with that package's kernels for every voice. p50 uses only std, so it has one sandbox per pool.

Not sandboxes: hosted plugins (`Arc<PluginInstance>`), feedback rings, lanes, cells, fades and the other host nodes. They are unchanged.

| | p50 | p50 × 10 | From |
|---|---|---|---|
| pool sandboxes | 50 | 500 | one per `poly`; 464 voice slots (code-map §6.2) |
| top-level kernel sandboxes | 150 | 1,500 | 17 × 2 (drive, delay) + 16 × 1 (reverb) + 100 in the song (code-map §6.2) |
| Stores = instances = memories | 200 | 2,000 | one of each per sandbox |
| wasmtime's dummy default-caller instances | 200 | 2,000 | one per Store (`W/runtime/store.rs:779-816`) |
| kernel states | 6,058 | 60,580 | unchanged |
| address space, one renderer | 812.5 GiB | 8,125 GiB | × 4.0625 GiB a memory (§11.3) |
| two renderers, share of 131,072 GiB | 1.24% | 12.4% | the 25% rule holds |
| the most one renderer may hold under the 25% rule | 4,033 | | 32,768 / 4.0625 / 2 (code-map §2.3) |

The Rust shape, as a sketch:

```rust
struct Sandbox {                     // one box's kernel states (RENDER-32, new)
    store: Store<Limits>,            // one instance, one memory; Limits is a ResourceLimiter
    base: NonNull<u8>, len: usize,   // the memory, which never moves (cut 1's MemView, cut1.diff:53-60)
    kinds: Box<[KindFns]>,           // per kernel type it runs: typed new, reset, process, active
    id: u64,                         // unique: for messages and debug checks
}
struct KernelNode { inputs: Vec<Input>, seen: Vec<u64>, state: u32, kind: u16, at: Here }
enum Here { Own(Option<Box<Sandbox>>), Pool(u8) }  // a top-level kernel owns its sandbox; a voice kernel names its pool's
struct VoicePool { sandboxes: Box<[Box<Sandbox>]>, voices: Vec<VoiceSlot>, born: u64, shape: u64 }
```

- `Limits { cap }` implements `ResourceLimiter` (`W/runtime/limits.rs:32-74`), with one instance, one memory and one table, as `StoreLimits` does today (`core/runtime/src/lib.rs:202`). So `Sandbox` is `Send`, like today's `KernelInst` (`refresh.md:189`).
- The speed plan's cut 2 (no `Box` per kernel) and cut 3 (one Store per pool) fall out of this: a voice kernel is a handle in its node. Cut 1's kept address stays (`plan.md:9-12`).

---

## 2. Kernel ABI v1

### 2.1 Exports

Kernels still import nothing (`core/runtime/src/packages.rs:472-474`).

| Export (kernel `k`) | Wasm signature | Called | Thread |
|---|---|---|---|
| `m2_abi` | `() -> i32`, returns 1 | once per package load | actor |
| `memory` | the module's one memory | — | — |
| `m2_alloc` | `(size: i32, align: i32) -> i32` | at birth only. It gives the host room for port buffers, port tables and config blocks; 0 means no room | the builder |
| `m2_status` | `() -> i32` | after a trap, to say why: 0 none, 1 out of memory while born, 2 allocated while playing, 3 a reset needed more room than its birth, 4 a panic | whoever saw the trap |
| `k_ports` | `() -> i32` | `inputs << 8 \| outputs`, checked against the manifest | actor |
| `k_new` | `(cfg: i32) -> i32` | at birth only. Returns the state's address, or 0 on failure | the builder |
| `k_reset` | `(state: i32)` | back to just after `new`, in the same room, never growing memory | the thread that holds the sandbox |
| `k_process` | `(state: i32, frames: i32)` | every segment | the partition's thread |
| `k_active` | `(state: i32) -> i32` | as today (code-map §3.5) | the partition's thread; the device thread for `can_idle` |
| `k_latency`, `k_tail` | `(state: i32) -> i32` | at birth (ABI-2's queries) | the builder |
| reserved, optional | `k_save(state, buf, cap) -> i32`, `k_load(state, buf, len) -> i32`, `m2_run(list, frames)` | step 9, or later (§7, §12) | — |

### 2.2 The config block

The host writes it at an address it got from `m2_alloc`, then passes it to `k_new`. Little-endian; every field is 4 bytes.

| Offset | Field |
|---|---|
| 0 | `sample_rate` (f32) |
| 4 | `max_block`: 128 live; an offline kernel may get more |
| 8 | `ports`: the address of the port table, `u32 × (inputs + outputs)`, inputs first |
| 12 | `channels`: the address of `u8 × (inputs + outputs)`, each port's channel count (ABI-2) |
| 16 | `assets`: the address of `(addr: u32, len: u32) × n` |
| 20 | `n`: the number of assets |
| 24 | `flags`: bit 0 means offline |

### 2.3 Ports and buffers

- **A buffer** holds `channels × max_block` floats, channel-major as in v0 (`std/kernels-src/kernel/src/lib.rs:20-21`). It is sized by its own port's channel count, not always 2 (`kernel/src/lib.rs:47`).
- **The host places every buffer** in the sandbox at birth and writes its address into the state's port table. The kernel reads the table at each call. The host may change an entry between calls. The core never does, but later layouts can rewire without a format change.
- **Inside a pool's sandbox**, an input that reads another kernel of the same voice graph points at that kernel's output buffer: no copy, and no buffer of its own. The wiring of a pool never changes after birth: a pool's shape is its voice graph (`plan.rs:48-54, 87-106`).
- **Every other input** has its own buffer, and the host copies into it as today (`core/runtime/src/lib.rs:343-359`): a cell once per value (RENDER-22), anything else per segment.
- **Outputs.** Every top-level kernel's outputs are copied out to the host's buffers as today. Inside a pool, nothing is copied out: the poly sums the voice output in place, after each segment (`lib.rs:1434-1444`).
- **ABI-1 still holds:** inputs are read-only to kernels. It matters more now, because a kernel that writes an aliased input changes another kernel's output. The debug check that compares each input before and after each call stays (`lib.rs:288-326`), per state.

### 2.4 A state's life

- **Created** by `k_new`, only while its sandbox is born. The kit's allocator bumps through the memory and grows it with `memory.grow` as needed (https://doc.rust-lang.org/core/arch/wasm32/fn.memory_grow.html), off the audio thread. The state's *room* is whatever `new` allocated.
- **Reset** by `k_reset`. The kit rewinds its allocator to the start of the state's room and runs `K::new` again, writing the new value over the old one without dropping it. A bump allocator frees nothing anyway. It zeroes the outputs, as v0 does (`kernel/src/lib.rs:156-162`).
  - `K::new` "must not depend on history" (`kernel/src/lib.rs:52`), so it allocates the same sizes in the same order and lands in the same room (inference).
  - A reset that needs more room traps with status 3.
  - So v0's brief doubling during a reset (code-map §1.4) goes.
- **Dropped:** never alone. A state is freed when its sandbox drops. There is no `k_drop`.

### 2.5 The kit (`std/kernels-src/kernel`)

- **The `Kernel` trait doesn't change**, except for optional `latency` and `tail` with defaults. Every std `impl Kernel` stays as it is, since the kit already hides `HOST` behind `export!` (inference, code-map §8).
- **`export!`** makes the handle-taking exports above. `static mut HOST` goes (`kernel/src/lib.rs:181`), and `Host<K>` lives at the state's address.
- **`Io`** gives the same slices as today, read through the port table.
- **`abi!`** adds `m2_alloc`, `m2_status` and a `#[global_allocator]` (https://doc.rust-lang.org/std/alloc/index.html#the-global_allocator-attribute). It is a bump allocator with three modes:
  - **birth:** bump at the end, and grow the memory if needed;
  - **reset:** only inside the state's own room;
  - **playing:** refuse, returning null with status 2.
  A failed allocation aborts (`panic = "abort"`, `std/kernels-src/Cargo.toml`), which traps as `unreachable` (code-map §1.1). The status word says why. So "never allocate in `process`" (`std/kernels-src/src/lib.rs:3`) becomes a rule the kit enforces.
- **`-zstack-size=65536` stays.** There is one stack per sandbox now; only the comment's reason changes (`std/kernels-src/.cargo/config.toml`).
- **`verify_module`** checks v1: `m2_abi() == 1`, `memory`, `m2_alloc`, `m2_status`, and every declared kernel's exports with these signatures (`packages.rs:471-511` does v0's).

---

## 3. Allocation, limits, and what the user sees

### 3.1 Birth, in order, on the builder's thread

1. **Check the process's budget** (§3.3).
2. **Make the sandbox:** a new `Store<Limits { cap: 4 GiB }>`, then `Instance::new`.
3. **Make each state:** one for a top-level kernel; for a pool, voices × voice kernels, in voice order and graph order.
   - `m2_alloc` its port table, its config block and its own buffers, then write the config;
   - call `k_new(cfg)`. On 0 or a trap, read `m2_status` and fail, naming the node. Today a trap in `k_init` names nothing (code-map §3.2, step 5);
   - fill the port table with its own buffers, or with a producer's output for an edge inside the pool;
   - write the constant inputs, as today (`lib.rs:250-256`).
4. **Reset each state once.** This is today's prewarm (`lib.rs:1294-1310`), now a check that each reset fits its room.
5. **Seal:** `store.data_mut().cap = the memory's size now` (`W/runtime/store.rs:832`). From then on the limiter refuses a `memory.grow` before any system call: wasmtime asks the limiter first and returns `Ok(None)` (`W/runtime/vm/memory.rs:651-660`). The kernel sees −1.
6. **Touch every page** the states and buffers use, with one write per page. So no page is faulted in for the first time on the audio thread: anonymous pages fault on first write (external §A4).
7. **Keep the base and size** (cut 1). The memory never moves: `memory_may_move(false)` (`W/config.rs:1934-1967`), and the 4 GiB reservation covers wasm32's whole range.

### 3.2 What replaces the 32 MiB cap

- **The cap goes** (`lib.rs:87-88`). A sandbox may grow to wasm32's 4 GiB while it is born, and not at all afterwards.
- **std's largest state** is one `delay` at 384 kHz: 2 × 768,002 × 4 = 6.1 MB stereo (`std/kernels-src/src/lib.rs:493-495`). It no longer doubles during a reset.
- **The address space gets a budget for the whole process** (§3.3).

### 3.3 Limits

| Limit | Where it bites | What the user sees |
|---|---|---|
| A state can't get room while it is born: 4 GiB, or the OS refuses | building a plan: an open, an edit or an export | today's "couldn't get it ready to play" (`crates/server/src/graph/actor.rs:411-424`), with a detail that names the node and says memory: "t02's delay ran out of memory for its sound state". `out_of_memory` (`crates/server/src/graph.rs:382-387`) learns the kit's status, so the message calls it running out of memory |
| **The process's sandboxes reserve at most 25% of the address space.** That is RENDER-32's rule, made a check at birth. N = 25% of the OS's address space / 4.0625 GiB, from `m2-os` (PLAT-2) | building | the same message: "this song needs more sound sandboxes than music2 can hold at once (N); remove some tracks or effects" |
| A kernel allocates while playing, or a reset needs more room | the audio thread | the kernel traps. Live, that is today's trap: the song goes silent and the transport stops (`core/runtime/src/engine.rs:570-579`). **Added:** a notice that names the node and why (status 2 or 3). Today it says nothing (code-map §3.7). The next activation resets the box. An export fails with the same message |

---

## 4. A live edit, step by step

### 4.1 Prepare, on the actor thread

It prepares against the plan that plays (`Basis::Keep`, `actor.rs:552-570`).
1. **What is kept.** `keep.rs` decides which nodes continue and which boxes are kept, by RuntimeId and shape (`core/runtime/src/keep.rs:166-189`). Unchanged.
2. **Every new box is born** (§3.1): a top-level kernel's sandbox, or a pool with its sandboxes. All on the actor thread, in Stores no other thread has seen.
3. **The plan is built as today** (`lib.rs:1596-1791`): kept boxes as empty slots (`Make::Moved`), then ghosts, then fades and rewires.
   - A kept kernel node's host side (inputs, versions, the copies' addresses) is built new.
   - Inside a kept box, nothing changes: its port tables and buffers stay where they were.
4. **The entry** lists the moves, and the leftovers are sized (`plan.rs:257-324`). Unchanged.
5. **8a:** the schedule is built here (`design-8a.md:191-224`). Unchanged.

### 4.2 Activate, on the device thread at a block line

The pool is quiescent then (`design-8a.md:485-498`).
1. **Verify:** the plan's id, each old box there with its shape hash, each new slot empty (`plan.rs:554-578`). Unchanged.
2. **Move by `mem::swap`** (`plan.rs:690-712`): `Option<Box<Sandbox>>` for a top-level kernel, `Option<Box<VoicePool>>` with its sandboxes inside, plugins and rings. Only the box's type changes.
3. **Reset poisoned boxes as they move** (`plan.rs:595-604`):
   - a non-finite output resets the box's states with `k_reset` (a pool: its dirty voices, `lib.rs:1223-1243`);
   - a trap resets every state in its sandboxes (a pool: every voice), since a kernel that trapped may have written anywhere in its sandbox.
4. **Clock, lanes, cells, releases, window start:** unchanged (`plan.rs:605-637`).
5. **Retire the old renderer**, with every box that didn't move (`engine.rs:436-440`).

### 4.3 The window, its end, and the free

- **The window.** Ghosts play their old boxes. Fades are host nodes (RENDER-25a, `core/runtime/src/fade.rs`). Unchanged.
- **The end.** `end_fade` swaps the ghosts into the leftovers, which leave as `Retired::Nodes` (`plan.rs:645-669`; `engine.rs:488-495`). Unchanged.
- **The free.** `m2-play-drop` drops each `Box<Sandbox>`. Its Store frees the instance and the memory (`W/runtime/store.rs:2425-2457`; `core/play/src/lib.rs:693-711`).

### 4.4 Nothing allocates, locks or calls the OS on the audio thread

| On the audio thread | Why not |
|---|---|
| verify and moves | compares, and `mem::swap` over vectors sized when the plan was prepared (`plan.rs:584-640`) |
| `k_reset`, `k_process`, `k_active` | `TypedFunc::call` allocates nothing on its success path (`W/runtime/func/typed.rs:154-224`). A sealed memory can't grow, and a refused grow returns before any system call (`W/runtime/vm/memory.rs:651-660`). Every page the states use was touched at birth (§3.1, step 6) |
| copies and in-place reads | slices of memory touched at birth |
| `end_fade` | swaps into leftovers sized beforehand |
| drops | never here: `m2-play-drop` drops, or the object is leaked and counted (`engine.rs:345-353`) |
| a trap | allocates its error, as today, and that is the only such path (`engine.rs:573-574`) |
| a thread's first wasm call | `Engine::tls_eager_initialize` on every engine thread first (`W/engine.rs:782-799`; `design-8a.md:425-429`) |

### 4.5 Who touches which Store, and when

| When | Thread | Sandboxes |
|---|---|---|
| birth | the session actor, for every live plan and an export's renderer (`actor.rs:426-438, 1200-1205`); a CLI's main thread | only the new ones |
| hand-off | none: they travel inside `Box<Renderer>` in `Msg::Activate`, whose channel orders the builder's writes before the device thread's reads (`core/play/src/lib.rs:35, 47-48`) | — |
| activation | the device thread, while the pool is quiescent | the moved boxes (swap) and the poisoned ones (reset) |
| the graph | the thread that runs the box's partition, for that block only | that partition's boxes |
| the post-pass | the device thread, after the join | top-level kernels, for `can_idle` (`lib.rs:2174`) |
| a panic | the device thread, at a block line | every box |
| retiring | `m2-play-drop` | the dropped ones |
| an export | the `music2-export` thread's own pool (`design-8a.md:676-697`) | the export's own |

---

## 5. Voice pools, plugins, exports, seeks and resets

- **Voice pools.**
  - A pool is born whole, with one sandbox per package and every voice's kernels in it.
  - It moves whole by one swap, as now (RENDER-16, `plan.rs:693`).
  - A retriggered voice's reset is `k_reset` for each of its states, in the poly's task (`lib.rs:1454-1456`), and never grows memory.
  - Allocation, stealing and captures are unchanged (`lib.rs:1395-1412, 1167-1175`). Voices are summed in voice order, from the voice output's buffer where it is (`lib.rs:1434-1444`).
- **Plugins** are unchanged (HOST-5; `plan.rs:697-709`).
- **Exports.** An export has its own renderer, with every sandbox new (`crates/editor/src/render.rs:132-136`), so two p50 renderers hold 400 sandboxes. A lending export borrows plugins only, as now (`lib.rs:1793-1805`).
- **Seeks and resets.**
  - A live seek or loop wrap resets nothing (RENDER-9).
  - An offline seek resets every state, then replays from 0 (`lib.rs:1987-2000`).
  - A panic resets as today (`lib.rs:1210-1243`).
  - A new rate or a new player builds a new renderer.

---

## 6. 8a's partitions

- **Partitions stay modules** (`design-8a.md:179-224`). A box is one node, so it is in one partition. A pool's voices run inside its poly's task (`design-8a.md:33`). "A `Store` is entered by one thread at a time" (`design-8a.md:100`) holds with no new rule. Only the words of 8a's precondition change: one sandbox per box.
- **Fewer Stores per task.** In a block, p50's tasks enter 3 sandboxes per bass track (pool, drive, delay), 1 per pluck track and 2 per pad track. Today they enter 58, 156 and 145 Stores (code-map §7). The song enters 100, as today.
- **No shared pages.** Each sandbox is its own mapping, so no two partitions write the same page of kernel memory.
- **Voices across threads** (`design-8a.md:699-716`). When the trigger fires, a split pool is born as one sandbox per package per chunk of voices, and the chunk is the box a subtask runs (L4). This is what `refresh.md:75` already says about HV3.
- **At p50 × 10 the limit is the song's serial tail, not the layout.** The song partition becomes a chain of about 1,000 calls (500 gains, 499 adds, the limiter), each in its own sandbox.
  - At about 0.7 µs a call, that is about 0.7 ms a block, 26% of a 2,667 µs block period on one core. 0.7 µs is 612 ns of JIT code (external §D4) plus 94 to 100 ns of host code after the cuts (§11.1).
  - L1, L2 and a std `sum` with many inputs (L8) would shorten it.

---

## 7. How ABI v1 fits

ABI v1 is when the format freezes for other people's code. Every later layout in §12 fits this format unchanged.

| ABI-2 item (`docs/architecture.md:1170-1178`) | Where it goes |
|---|---|
| event-in and event-out ports | a port kind whose buffer holds a count, a capacity and fixed-size RENDER-14 events. The host copies events in before `process` and reads them out after, as with audio. Its room is sized at birth from the plan's caps, as plugin event lists are (`lib.rs:484-487, 966`) |
| read-only asset blobs given at init | the config block's `(addr, len)` list. The host copies each blob into the sandbox at birth, and debug builds check after each call that it hasn't changed, as for ABI-1. That is one copy per sandbox, fine for impulse responses and tables. **Sampler banks don't go in blobs:** they stream into audio input ports from the host's disk streaming (RENDER-30, step 9). Sharing one blob's pages among sandboxes is later (L5): it needs memories the host maps itself (`Config::with_host_memory`, `W/config.rs:1696-1704`). How wasmtime reports a write to a read-only page there is unverified |
| channel counts per port | the config's `channels` array; buffers are sized by it (§2.3) |
| state save and load | `k_save` and `k_load` are reserved. A load runs at birth, off the audio thread, in a new sandbox. A changed blob makes a new state, as a removed blob makes a plugin new (HOST-5). A save needs the thread that holds the sandbox: offline from an export's copy, or (later) at a block line into a buffer sized when the plan is prepared |
| latency and tail queries | `k_latency` and `k_tail`, at birth. Today there is only a manifest field (`packages.rs:44-45`) |
| offline kernels | a sandbox born for one job, on the render's thread, with its own `max_block` and buffers, and flag bit 0 |
| room for later layouts | port tables rewritable between calls; an optional `m2_run`; and ABI-5's "no dependence on where state is" (§9) |

---

## 8. Isolation

- **What a buggy kernel can harm:**
  - every state and buffer in its own sandbox. For a top-level kernel that is only itself, as today. For a voice kernel it is its pool's kernels from its package, in every voice;
  - what is downstream, through its output, as today. Non-finite output is caught live (RENDER-25's check, `lib.rs:1181-1185`);
  - its partition's time. Nothing stops a kernel that loops forever, today or here.
- **What it can't harm:** other sandboxes (other boxes, other packages, other tracks), host memory (guard pages trap), and other renderers.
- **std's kernels are safe Rust,** apart from the kit's checked `unsafe` (`kernel/src/lib.rs:103-120`). So in practice a std bug is a panic, hence a trap, not a corruption.
- **Spectre.** Kernels still import nothing, so they have no clock (RENDER-32's *Why*). The heap Spectre guard costs nothing where bounds checks are elided (`CG/shared/settings.rs:290-302`), so it is on. That closes the deferred item (`docs/architecture.md:1232`).
- **Against today,** isolation inside a pool coarsens from one kernel to the pool. Prior art does the same: faustwasm gives a polyphonic node's voices one memory (external §B).

---

## 9. Determinism

- **Thread count:** 8a's argument is unchanged (`design-8a.md:111-150`). A box is in one partition.
- **Bounds checks** change no arithmetic: the 8 recorded renders match across P1 and P2 (`docs/architecture.md:330`).
- **In place or copied, the values are the same.** A producer runs before its consumer, in plan order, in the same segment, and both use frames `0..n` of their buffers.
  - Test: a test-only switch copies every port, and p50 must render the same hash with it on and off.
- **New rule ABI-5:** a kernel's output never depends on where its state or buffers are.
  - Test: build with padding `m2_alloc`'d first; the render hash must not change.
  - The bump allocator is deterministic, so the same birth order gives the same addresses on every run, which makes faults reproducible.
- **The migration's own check.** std.wasm's bytes change, so every kernel hash changes (PLAT-5). The DSP doesn't, so the 8 recorded renders (`e2e/renders.sha256`) and the p16 render must keep their hashes.
- RENDER-33 and PLAT-4 are unaffected.

---

## 10. What changes

### 10.1 Code

| File | Change |
|---|---|
| `std/kernels-src/kernel/src/lib.rs` | ABI 1; handle-taking exports; `Host<K>` at the state's address; the three-mode bump allocator; `m2_alloc`, `m2_status`; `Io` over the port table |
| `std/kernels-src/src/lib.rs`, `src/fx.rs` | no change to any `impl Kernel` (inference); the `export!` lines stay |
| `std/kernels-src/.cargo/config.toml` | the comment only |
| `std/kernels/std.wasm` | rebuilt, noting that the kernel hashes changed (PLAT-5) |
| `core/runtime/src/packages.rs` | `KERNEL_ABI = 1`; `verify_module` for v1; `engine()` becomes wasmtime's defaults plus `memory_may_move(false)`, NaN canonicalisation off and relaxed SIMD off; `Layout` and the P2 branch go (`packages.rs:89-133`) |
| `core/runtime/src/lib.rs` | `KernelInst` becomes `Sandbox`, and `KernelInst::new` becomes birth. `KernelNode` holds `(state, kind, Here)`, and `VoicePool` owns its sandboxes. `KernelNode::process` copies only into a node's own buffers, and out at the top level. `prewarm` becomes birth's check. `kernel_memory` sums sandboxes, and `LIVE_KERNELS` counts sandboxes and states. `KERNEL_MEMORY`, `PORT_BYTES` and the overlap check (`lib.rs:236-248`) go |
| `core/runtime/src/plan.rs` | the box's type in `verify` and `take_from`; a trap's poison resets the whole sandbox |
| `core/runtime/src/keep.rs` | none |
| `crates/server/src/graph.rs`, `graph/actor.rs` | `out_of_memory` reads the kit's status; a notice when a kernel crashes live |
| `core/os` | the address space's size (PLAT-2) |
| tests | `no_alloc.rs` (no memory grows after birth; true by construction now), `churn.rs` (sandboxes and states back to zero), cut 1's `a_kernel_memory_never_moves` (kept), `every_std_kernel_keeps_its_inputs` (per state, aliased inputs included), the `keep.rs` tests. New: `in_place_equals_copies`, `placement_changes_no_bit`, `a_trapped_voice_resets_its_pool`, `an_allocation_while_playing_is_named` |
| `crates/gates/src/store.rs`, `arena.rs` | G7f with one layout, against S and HEAD |

### 10.2 Rules

| Rule | Change |
|---|---|
| RENDER-32 | rewritten; the draft is below |
| ABI-1 | the host places every port buffer; an input may be another kernel's output in the same sandbox; no overlap is possible, since the host places them all |
| ABI-2 | becomes v1's contents: §2 and §7 |
| ABI-3 | `m2_abi()` is 1; v0 is refused (old projects may break) |
| ABI-5 (new) | a kernel allocates only in `new`; `reset` stays in its room; nothing depends on where state is; still no imports |
| RENDER-25 | "instance" becomes "box"; a kernel's box is its sandbox; the shape is unchanged |
| RENDER-25a | none |
| RENDER-28 | the words are unchanged; *Today* adds that a sealed memory can't grow |
| RENDER-16 | a pool's kernels share one sandbox per package |
| RENDER-10 | after a trap, the box's whole sandbox is reset |
| design-7 §11 | superseded. §11.1's elimination still holds: shared stores and per-batch stores leak. Its last point, "the cost scales with memories, not stores", is the one this design acts on |
| design-8a §1 and its RENDER-32 *Why* edit (`design-8a.md:100, 883`) | "one sandbox per box" |
| the deferred Spectre item | done |

Draft of the new RENDER-32:

> **RENDER-32.** Sandbox layout: kernels run in sandboxes. A sandbox is one wasmtime `Store` with one instance and one linear memory, and it holds the kernel states of one box: a top-level kernel, or a voice pool's kernels from one package. A sandbox is born whole off the audio thread and sealed before it plays, after which its memory never grows. It moves whole between plans, is entered by one thread at a time, and is dropped whole off the audio thread, which frees its memory. Never a sandbox shared by two boxes. Memories take wasmtime's default reservation (4 GiB plus guards) and never move, so accesses are unchecked, and Cranelift's Spectre guard is on. The process's sandboxes MUST reserve at most 25% of its address space; a build that would pass that fails, saying so. G7f's bounds on block time, callback time, footprint and build stay, judged against S.

---

## 11. Expected effect on p50

### 11.1 Time per 512-frame callback (Linux, HEAD P2 = 7,312 µs)

| Part | Change, µs | Arithmetic and source |
|---|---|---|
| Stores per pool, address kept, no `Box` per kernel: the speed plan's cuts 1 to 3 | −290 to −350 | all four cuts measured −355 (−367 to −308) (`plan.md:38`), minus cut 4's −14 to −21 (`plan.md:13`), which this design doesn't make |
| bounds checks elided | −84 to −157 | O1's cost measured with the cuts: +105 (+84 to +157) (`plan.md:40`). On the Mac, O1 measured +141 (+111 to +177) (`plan.md:84`) |
| copies inside pools (estimate, below) | −275 to −325 | of the 408 µs of port copies |
| **total (assuming the parts add up; unverified)** | **−650 to −830** | 6,480 to 6,660 µs: 9% to 11% under HEAD, 5% to 8% under S (7,026) |

The copies, counted from the voice code (`std/src/instruments.ts:27-41, 105-116, 130-140`) and the ports in `std/m2pkg.toml`. A control that is captured or ramping is counted as copied at every run. Constants written once are not counted (`lib.rs:250-256`).

| Voice | Kernels | Inputs from kernels | Inputs from the host (pitch, gate, velocity, controls) | Outputs | Copies per run today | After |
|---|---|---|---|---|---|---|
| bass | 14 | 13 | 6 | 16 | 35 | 6 (83% fewer) |
| pluck | 13 | 13 | 5 | 15 | 33 | 5 (85% fewer) |
| pad | 12 | 11 | 5 | 14 | 30 | 5 (83% fewer) |

- **Voice copies a block:** 2,095 voice-kernel calls (code-map §6.2) × 2.50 to 2.54 copies a call = about 5,240 to 5,320.
- **Top-level copies a block,** counted the same way: 351 to 614.
  - The parts: drive 2 and delay 2 per bass track; reverb 2 to 3; each gain 2 to 4; each add 3 to 6; the limiter 4.
  - The ranges are mono against stereo.
- **The share removed, if time follows the count:** 0.83 to 0.85 × 89.5% to 93.8% = 74% to 80%. That is 303 to 325 µs of the 408 µs.
- **If each top-level copy weighs double** (full blocks, often stereo): 67% to 75%, so 274 to 306 µs.
- **Unverified:** that copy time follows the number of copies.

Not counted:
- tighter memory locality for the kernels' own code (5,661 µs), which is unmeasured;
- the host's per-voice buffers that in-place reads make unneeded.

At p50 × 10, that rate is 16.2 to 16.7 ms of work for each 128-frame block of 2,667 µs. That is about 6 cores' worth before the serial tail (§6), and more than the 4 cores 8a would use on an M1 (`design-8a.md:61-62`). Beyond about 10%, scale comes from cores and cheaper kernels, not from the layout.

### 11.2 Footprint

These are estimates. How today's footprint per instance splits is unverified (code-map §2.4).

| Part | Linux | Mac | Arithmetic |
|---|---|---|---|
| port buffers | 7.3 to 7.6 MB | the same | voice buffers left after in-place edges: 68 × 36 + 204 × 30 + 192 × 27 = 13,752, at 512 B mono = 7.04 MB. Plus 516 top-level ports at 0.5 to 1 KiB |
| delay lines and reverbs, all touched at birth | 8.3 to 14.8 MB | the same | delay: (2 × 48,000 + 2) × 4 B = 384 KB a channel × 17, mono or stereo (unverified which p50 uses). Reverb: 25,450 floats at 44.1 kHz scaled to 48 kHz ≈ 111 KB × 16 (`fx.rs:199-201, 264-270`) |
| small states and headers | about 0.6 MB | the same | 6,058 × about 100 B |
| fixed cost per sandbox (upper bound) | ≤ 6.4 MB | ≤ 19 MB | 200 × today's measured average per instance, 32.2 KB or 94.9 KB, which includes each instance's own ports and state |
| **total** | **about 23 to 30 MB** plus the host's share | **about 35 to 42 MB** plus the host's share | against 195 MB and 575 MB today. The host's own buffers and tables are inside those figures and not split out; they stay or shrink |

### 11.3 Address space

- **Per memory:** 32 MiB pre-guard + 4 GiB + 32 MiB guard = 4.0625 GiB (`W/runtime/vm/memory/mmap.rs:62-69, 102-107`; `E/tunables.rs:326-327, 266`).
- **p50:** 200 × 4.0625 = 812.5 GiB per renderer. Two renderers: 1,625 GiB, 1.24% of 131,072 GiB. Today: 0.29% under P2, and 37.9% on the Mac under P1 (`docs/architecture.md:330`).
- **p50 × 10:** two renderers, 16,250 GiB = 12.4%.
- **The ceiling:** 4,033 sandboxes per renderer, so p50 × 20 (4,000) is about the last size that fits.

### 11.4 Build

- Today: 216 ms; S: 181 ms (`plan.md`, part 1). The 35 ms between them is the cost of 6,058 Stores, about 5.8 µs each.
- With 200 Stores, that cost is about 1.2 ms, so the build is **at most about 182 ms**, plus touching the pages.
- Touching pages: 23 to 30 MB / 4 KiB is 5,600 to 7,300 first-touch faults, whose cost is unmeasured.
- 5,858 fewer instantiations save more on top of that. How much of S's 29.9 µs per instance is instantiation is unmeasured, so the lower end is unknown. Slice 0 measures it.

---

## 12. Cost, slices, and what is left for later

### 12.1 Slices

Sizes are rough estimates. Every slice ends green, with the 8 renders unchanged.

| # | Slice | Size |
|---|---|---|
| 0 | **Spike, thrown away.** The kit's v1 macro, a pool sandbox for p50's 8 voice kernel types, and Linux measurements of the build, the footprint split and the callback. One Mac run. Go or no-go against §11 | 2 to 3 days |
| 1 | The kit v1 and std rebuilt; `verify_module` v1; top-level kernels as one-state sandboxes; the engine at wasmtime's defaults with the Spectre guard on; sealing and status codes | about 1.5 weeks |
| 2 | Pools as one sandbox per package; voice kernels by handle; resets in place; a trap resets the pool; the ABI-1 debug check per state. HV3 lost that check (`refresh.md:560-565`) | about 1 week |
| 3 | In-place edges inside pools; the strict check in place; the `in_place_equals_copies` and `placement_changes_no_bit` tests | about 1 week |
| 4 | Limits and messages: the process budget, `out_of_memory`, the live crash notice | 3 to 5 days |
| 5 | Gates and rules: G7f with one layout, `no_alloc`, `churn`, the rule texts (§10.2), a recorded Mac run | about 1 week |

- **Total:** about 5 to 6 weeks.
- **Order:** it is independent of 8a, so either can go first. It replaces the speed plan's cuts 2 and 3. It must land before ABI v1 freezes in step 9, because it *is* ABI v1's layout.

### 12.2 Later, each behind its trigger

| # | Later | Concept it adds | Trigger |
|---|---|---|---|
| L1 | Group a track's top-level kernels: one sandbox per track, package and activation (E in §0.1), plus compaction. A plan sent while the engine idles (stopped, nothing active) makes the fragmented tracks' kernels new. It is inaudible then, and the crossfade covers it if play starts meanwhile | generations; compaction | a renderer needs more than half of its 4,033 sandboxes, or G12 shows the song's serial tail on the critical path |
| L2 | **One call per voice segment** (`m2_run`): the kit runs a host-written list of `(kind, state)`. Up to about 0.7 to 0.8 ms a callback (an upper bound): (2,095 − 150 to 175) × 4 ≈ 7,700 fewer host calls × 94 to 100 ns a call = 0.72 to 0.78 ms. The 94 to 100 ns is (766 + 416 + 37 − 290 to 350) / 9,250, the host's cost per call after the cuts. The dispatcher's own cost is unmeasured | an in-sandbox dispatcher | after this design lands, if G12 needs the time |
| L3 | Host nodes inside a voice (pitch, gate, velocity, cells) write straight into the pool's sandbox | host writes into kernel memory | after L2 |
| L4 | Voices across threads: a split pool gets one sandbox per chunk of voices | chunk boxes | `design-8a.md:699-716`'s trigger |
| L5 | Shared asset pages: host-made memories that map the asset cache read-only | a host memory creator | a sampler package with large banks (step 9) |
| L6 | Before third-party packages: a memory budget per package, a time limit for runaway kernels (wasmtime's epoch interruption; its cost is unverified), and resetting the whole sandbox on poison | budgets | the first non-std package |
| L7 | Saving a live state | a save at a block line | the first kernel that needs it |
| L8 | std: a `sum` with many inputs for buses, so the song's tail is a few calls, not 499 (`std/src/mix.ts:41`) | none in the layout | p50 × 10, or G12 |

### 12.3 What to measure first (slice 0)

1. What today's 32.2 KB per instance is made of (§11.2).
2. Instantiation's share of the build (§11.4).
3. The copy saving on p50 with in-place edges (§11.1).
4. The Mac's G7f on this layout, against S.

---

## 13. The weakest point

**Top-level kernels each still cost a whole sandbox:** a 4 GiB reservation, an instance and a Store for, say, a gain's 16-byte smoother.
- p50 hides this, because its kernels live in voices.
- A song made mostly of mixer channels and effect chains scales only to about 4,033 top-level kernels per renderer before the 25% rule refuses it.
- The song's serial mixer enters a different Store at every call.

So this design moves the limit from the voices to the top level; it doesn't remove it. Removing it means grouping a track's top-level kernels (L1), and that runs into the very difficulty idea 3 met: a kept state pins the sandbox it was born in. So grouping needs generations, and to be worth it, compaction.

Two smaller weaknesses:
- **Isolation inside a pool coarsens** to the pool.
- **The number of host-to-kernel calls stays at about 9,250 a callback,** about 100 ns each after the cuts, until L2.

---

## Attack (label attack-scale)

Read-only, like the rest of this note: nothing was built, run or measured. Citations follow the note's own conventions, plus `rust-docs/` = `~/.rustup/toolchains/1.99.0-x86_64-unknown-linux-gnu/share/doc/rust/html`: the pinned toolchain's own std docs and std source pages (`src/std/panicking.rs.html`). Severity: **high** = breaks as written, or would freeze a mistake into ABI v1; **medium** = wrong in a reachable case; **low** = wording, diagnosis or a bound.

### A.0 The verdict, first

**Sound with fixes** (A.9). The core idea, "the sandbox is the box", survives every attack on live edits, threads, determinism and address space. The breaks are in the details:
- the reset's allocator, as written;
- what a kept top-level box carries;
- what happens after a trap;
- in-place edges;
- an ABI v1 config block that can't grow.

All of them have small fixes. Three of the fixes also make the design simpler.

### A.1 wasmtime claims, checked against its source

| Claim | Verdict | Evidence |
|---|---|---|
| A refused grow returns before any system call | holds | Growing by 0 returns at once (`W/runtime/vm/memory.rs:614-616`). The limiter is asked first and a refusal returns `Ok(None)` (`:651-659`). The `mprotect` comes only after that, in `grow_to` → `make_accessible` (`W/runtime/vm/memory/mmap.rs:156-217`). `memory_grow_failed` isn't called on that path, so a limiter's log line can't run on the audio thread |
| 4.0625 GiB a memory, with no growth room | holds for std.wasm | A memory with no declared maximum gets the index type's, 4 GiB (`E/types.rs:2324-2331`). The growth reserve is zeroed when the maximum ≤ the reservation (`mmap.rs:81-92`). The request is pre-guard + reservation + guard (`mmap.rs:62-68, 102-105`) |
| No bounds checks at 4 GiB plus a 32 MiB guard | holds | `can_elide_bounds_check` needs reservation + guard ≥ 4 GiB (`E/types.rs:2399-2407`) and virtual-memory traps. Those are on by default on 64-bit (`E/tunables.rs:341`), and 64 KiB wasm pages are at least the Mac's 16 KiB (`E/types.rs:2376-2377`) |
| The Spectre guard costs nothing there | holds | "a no-op for any heap that needs no bounds checks" (`CG/shared/settings.rs:288-301`). The guard is emitted only on a checked access (`CL/bounds_checks.rs:179-212`). P1 is this layout, and it measured faster than P2 (O1 +85 to +105 µs, `plan.md:40`) |
| Freed only when the Store drops | holds | `W/runtime/store.rs:139-145, 2425-2457` |
| The default-caller dummy costs no memory | holds | "this dummy instance doesn't allocate tables or memories" (`store.rs:794-797`) |
| `TypedFunc::call` doesn't allocate | holds as far as read | Params and results sit in a stack union (`W/runtime/func/typed.rs:100-105, 154-224`). The rest of the path is as in external §A7; I did not re-read it |
| Sealing through `store.data_mut().cap` | holds | `data_mut` (`store.rs:832`). The limiter is `&mut T` (`core/runtime/src/lib.rs:203`). `ResourceLimiter: Send` (`W/runtime/limits.rs:32`), and it has `instances`, `tables` and `memories` (`:133-155`) |
| The memory never moves | holds | With `memory_may_move(false)`, growth past capacity is refused (`memory.rs:676-678`), and the base is asserted unchanged (`:663-664, 711-712`) |

### A.2 Live edits

1. **Activation by swapping the box: holds.**
   - `take_from` swaps only the box (`core/runtime/src/plan.rs:690-692`), and `verify` compares the box's shape (`plan.rs:554-560`).
   - A pool already moves whole, with its voice graphs inside (`lib.rs:388, 412-415`; `plan.rs:693`).
2. **A kept top-level box: breaks as written (high).**
   - *What the design says:* §1's sketch puts `state: u32` in `KernelNode`, and `Sandbox` has no shape. §4.1 step 3 says a kept node's host side, "the copies' addresses" included, "is built new".
   - *Why it breaks:*
     - a kept node is built `Make::Moved`, as an empty slot (`lib.rs:872`), on the actor, which never sees inside the box that plays;
     - activation moves only the box (`plan.rs:692`);
     - so nothing can fill a kept kernel's state address or its buffer addresses;
     - and `verify` reads `k.shape` from the box (`plan.rs:560`; today's `KernelInst.shape`, `lib.rs:153`).
   - *Fix:* a top-level box carries its `shape`, its `state` and its port addresses, as `KernelInst` carries `in_ptrs` and `out_ptrs` today (`lib.rs:159-160`). Voice kernel nodes may keep theirs in the node, since a pool's voice graphs move with it.
3. **Ghosts and the 10 ms window: holds.**
   - A ghost is an old box that plays in the new plan (`keep.rs:300-354`).
   - Old and new are different sandboxes, so nothing new arises.
4. **States made only at birth and freed only by a drop: holds.** This is the design's best property: with one box to one sandbox, "freed when it drops" is exact (`store.rs:2425-2457`).
5. **A reset after a trap: breaks (medium).**
   - *What the design says:* §4.2 step 3 resets every state of a trapped sandbox, "since a kernel that trapped may have written anywhere".
   - *Why that isn't enough:* `k_reset` rebuilds `K` and zeroes the outputs. It restores none of these:
     - the shadow-stack pointer. That is std.wasm's one mutable global (code-map §1.3), and a trapped call never restored it. This is inference: LLVM restores it in the epilogue, which a trap skips, and nothing in the host resets globals;
     - the kit's allocator words;
     - the `Host<K>` headers;
     - the port tables.
   - *So:* a reset after a trap doesn't bring the sandbox back to its birth, and each trap can shrink the 64 KiB stack. Today has the same latent fault (`plan.rs:595-604`).
   - *Fix, which is also simpler:* never enter a trapped sandbox again.
     - A trap sets a `dead` flag in the box, and a dead box plays silence, as an empty slot does (`lib.rs:334-341`).
     - The crash notice that this design adds names the node. The next plan then makes it new, off the audio thread, through a restart set like the one `keep.rs` keeps for plugins. That set holds plugin handles today (`keep.rs:171, 237`), so it needs keying by node.
     - A box poisoned only by non-finite output keeps today's reset in place.
6. **Exports and their two renderers: holds** (`crates/editor/src/render.rs:132-136`). The budget is in A.5.
7. **Seeks, loops and the panic: holds.**
   - A live seek resets nothing (`engine.rs:369-377`).
   - The panic's resets now allocate only inside each state's room. That is better than today's dlmalloc on the audio thread (code-map §1.4).
8. **A package rebuilt with a new hash: holds.**
   - The wasm SHA is in every shape (`plan.rs:35-39, 64-66, 87-106`), so every box that uses the package is made new.
   - Ghosts keep the old `Module` alive in their Stores.

### A.3 Real-time safety

1. **`memory.grow`: holds** once the sandbox is sealed (A.1).
2. **The reset's allocator: breaks as written (high).**
   - *(a) The header.* "`Host<K>` lives at the state's address", the room "is whatever `new` allocated", and a reset "rewinds … to the start of the state's room". If `k_new` allocated `Host<K>` inside the room, the new `K`'s first `Vec` lands on `Host<K>` itself: the port-table pointer and the room bounds.
   - *(b) The old value.* "Writing the new value over the old one without dropping it" makes the allocator hand out memory that the old `K`'s `Vec`s still own. GlobalAlloc's contract: "It is undefined behavior for the allocator to read, write, or deallocate any memory that is currently allocated" (`rust-docs/core/alloc/trait.GlobalAlloc.html`, Safety).
   - *(c) The same room.* "The same sizes in the same order" is inferred across two different inlined call sites, `k_new` and `k_reset`. The same page warns: "you cannot rely on global allocations actually happening … The optimizer may detect unused global allocations that it can either eliminate entirely or move to the stack".
   - *(d) Zeroing.* If the kit's `alloc_zeroed` skips zeroing because birth memory is fresh, a reset's `vec![0.0; n]` keeps the old delay line (`std/kernels-src/src/lib.rs:493-495`; `src/fx.rs:264-270`). The echoes would survive the reset, and live would differ from export.
   - *Fix:*
     - at birth, record the reset window: `K`'s heap only, after `Host<K>` is placed;
     - a reset is `drop_in_place(&mut host.kernel)` (dealloc is a no-op), then a rewind to the window, then one `#[inline(never)]` constructor shared with `k_new`, then `ptr::write`;
     - `alloc_zeroed` always zeroes outside birth mode;
     - status 3 stays the safety net.
3. **Faults on first touch: breaks in part (medium).**
   - *What step 6 misses:* it touches "every page the states and buffers use", but not the shadow stack, `[0, 64 KiB)` (code-map §1.3). `k_process` can go deeper than `k_new` and `k_reset` did, so a stack page can be touched first on the audio thread.
   - On Linux the data image is a copy-on-write memfd mapping (code-map §2.4). Its pages fault on first access too (inference).
   - *Fix:* read and write back one byte per page over `[0, the allocator's end)`. One loop covers the stack, the data and the heap. Its cost is in A.5.
   - *Not fixed by any layout:* pages can be reclaimed later (swap on Linux; compression on the Mac, unverified). With 200 sandboxes, `mlock` of each one's touched range becomes practical, later.
4. **Locks: holds.**
   - There are none on the audio thread's success path.
   - 200 `mmap`, `mprotect` and `munmap` calls replace 6,058 on other threads, which shrinks refresh.md's R15. How Linux's memory-map locks behave is unverified.
5. **The kit's allocator in native builds (low).**
   - m2-runtime's bench links std's kernels natively (`core/runtime/examples/bench.rs:14-15`; `core/runtime/Cargo.toml:21, 25`), and `abi!()` runs at crate level (`std/kernels-src/src/lib.rs:9`).
   - A `#[global_allocator]` inside `abi!` would become the bench's allocator, wasmtime's included. `memory_grow` exists only on wasm32, so the compiler would catch it.
   - *Fix:* say it. The allocator and the exports are `#[cfg(target_arch = "wasm32")]`, and `Host` keeps a native path for the bench.
6. **A trap: holds as today.** It is still the only path that allocates (`engine.rs:573-574`).

### A.4 Determinism

1. **Any thread count: holds.** A box is in one partition (`refresh.md:188`). 8a splits modules by depth, not nodes (`design-8a.md:199-210`).
2. **Live against export: holds with A.3 #2's fix.** A reset equals a birth only if `alloc_zeroed` zeroes and `Host<K>` survives.
3. **In place against copied: breaks in reachable cases (high, if in-place edges are kept).**
   - *Channels.* Today a mono source that feeds a stereo input is copied into both channels (`lib.rs:354`). An aliased stereo input over a mono buffer, which §2.3 sizes by its own port, reads channel 1 from whatever buffer follows it.
   - *Feedback.* A voice may hold a `feedback`: the checker refuses only plugins and `poly` inside voices (`core/ir/src/check.rs:562, 576`). Feedback reads the host's buffers after the graph (`lib.rs:1192-1205`). With nothing copied out, it reads stale values.
   - *The voice output* need not be a kernel.
   - *Strict mode* names the kernel during the copy out (`lib.rs:370`).
   - p50 has none of the first two, so its hash test wouldn't catch them.
   - *Fix:* defer in-place edges (A.7). If they are built, alias only kernel-to-kernel edges inside one sandbox whose consumer has no more channels than its producer, and copy out every output that anything else reads.
4. **ABI-5: holds.** Add: "no per-state data in statics, since every state in the sandbox shares them". std has none (a grep of `std/kernels-src/src` for `static`, `thread_local` and `OnceLock` finds nothing).

### A.5 Address space and footprint

1. **The Mac's 25% rule: holds.**
   - Two p50 renderers take 1.24%, and two p50 × 10 renderers take 12.4%.
   - *A caveat:* the Mac measured 49,637 GiB against 49,221 computed, which leaves 416 GiB unexplained (code-map §2.3). If that is per memory (416 / 12,116 = 35 MiB each), the ceiling is 16,384 / 4.097 = 3,999 a renderer, not 4,033, and p50 × 20 sits right at the edge. Which one it is, is unverified.
2. **The process-wide budget: breaks (medium).**
   - §3.3 counts the whole process's sandboxes at birth.
   - Retired boxes leave only when `m2-play-drop` wakes, every 5 ms (`core/play/src/lib.rs:693-711`), and an edit can be prepared during a pure song's export (code-map §5.6).
   - So near the ceiling, the same edit passes or fails depending on timing.
   - *Fix:* check each renderer against half the budget, 4,033 at 4.0625 GiB. Then a song either fits or it doesn't, every time, and the message can give the song's count. A failing `mmap` covers the rest, through today's message.
3. **Footprint: the arithmetic holds.** I re-checked every line of §11.2, including the reverb's 25,450 floats (`fx.rs:199-200`). Two additions:
   - Touching the stack (A.3 #3) adds at most 200 × 65,536 B = 13.1 MB at p50, on both platforms. Upper bounds: Linux 36 to 43 MB, Mac 48 to 55 MB.
   - "From 195 MB" isn't like for like. The 195 MB includes the host's buffers, which §11.2 leaves out ("plus the host's share", unmeasured).
4. **Removing the cap (medium).**
   - "A sandbox may grow to wasm32's 4 GiB while it is born", and step 6 then touches all of it.
   - One third-party `k_new` that asks for 4 GiB costs 4 GiB of RAM in every sandbox it runs in.
   - *Fix:* keep a birth cap in the same limiter field the seal uses, at a number the owner picks. That adds no concept, and L6 only tunes it.

### A.6 Isolation, ABI v1 and third-party packages

1. **Isolation: holds.**
   - One sandbox per package per pool keeps packages apart.
   - The guard covers host memory (`E/types.rs:2406`).
2. **The config block can't grow (high: it would freeze).**
   - It is 28 fixed bytes, with no length and no version.
   - ABI-2's offline kernels carry "a time map" (`docs/architecture.md:1176`), and §2.2 has no field for it.
   - RENDER-25's shape includes "the settings" (`docs/architecture.md:253`). But a kernel can read constants only after `k_new`, since §3.1 step 3 writes them afterwards. So a setting that sizes the state, such as a delay's maximum time, has no way in.
   - *Fix:* field 0 is the block's length in bytes; kernels ignore fields they don't know, and the host appends new ones. Add `settings: (addr, len)` and reserve an `(addr, len)` for the time map. Cheap now; later it would mean an ABI v2.
3. **The status codes (low).**
   - A formatted panic message allocates a `String` (`rust-docs/src/std/panicking.rs`: `FormatStringPayload::fill`, lines 619-628), but only when a hook asks for it.
   - std skips the default hook on wasm (`:813`). A custom hook gets `payload.get()` first (`:823`). So the kit must not install a hook, and a hook can't set code 4.
   - *Fix:*
     - the first cause wins;
     - while playing, the allocator reports 4 if `std::thread::panicking()` and 2 otherwise. The panic count is raised before any formatting (`:783`; inference);
     - status 0 after a trap means a panic or a plain bug.
   - Unverified: whether the abort runtime calls `take_box`, which would allocate (`:811-812` warns that it might).
4. **Assets and the sampler (medium, for planning).**
   - §7 sends sampler banks through audio ports, from RENDER-30's streaming.
   - RENDER-30 streams audio lanes, reading "around the transport" (`docs/architecture.md:299`). A sampler reads from wherever a note starts, at a rate that depends on its pitch.
   - Unverified: that ports can serve a sampler. If they can't, banks become blobs copied into every sandbox that uses them, and L5 is step 9's work, not later.
   - The format, `(addr, len)` per asset, holds either way.
5. **Event ports, latency, tail, save and load: hold** as reservations.
   - "Offline from an export's copy" would save a state replayed from 0, not the live one.
   - That is harmless while the exports are only reserved.

### A.7 Simplicity: the concepts, counted

| | Today | This design, as written | With the fixes (A.9) | Rival R1: one Store per pool, ABI v0 | Rival R2: one Store per track, ABI v0 |
|---|---|---|---|---|---|
| Unit of state | one instance per kernel | one sandbox per box | the same | a Store per pool, an instance per kernel | a Store per track |
| Layouts | two (P1 for the gate only, P2) | one | one | two | two |
| Bounds checks and the Spectre exception | yes (`docs/architecture.md:1232`) | no | no | yes | yes |
| Kernel state | `static mut` per type | handles | handles | statics | statics |
| Port buffers | the kernel's, `k_port`, the overlap check | the host's, a port table, `m2_alloc` | the same | today's | today's |
| Allocator | dlmalloc; resets reuse its free block, a property only tests show (code-map §1.4) | a bump allocator: three modes, sealed | the same, with the reset window | today's | today's |
| Memory cap | 32 MiB a kernel | none | a birth cap per sandbox | 32 MiB | 32 MiB |
| Touching pages at birth | no | yes | yes, the stack too | no | no |
| Address-space budget | the gate only | the whole process, at run time | per renderer, at run time | the gate only | the gate only |
| In-place edges | no | yes, plus a placement rule, two tests and the exceptions in A.4 #3 | deferred | no | no |
| After a trap | reset in place | the whole sandbox reset in place | a dead flag, then made new | reset in place | reset in place |
| A replaced state's memory | freed when its box drops | the same | the same | the same | kept until the track's Store drops (`store.rs:139-145`), against RENDER-32 |
| Through ABI v1's freeze | — | yes | yes | no: a v1 without handles freezes a memory per state, so the 4 GiB layout never fits (6,058 > 4,033) | no |

- **With the fixes:** about 9 concepts in and 8 out.
  - *In:* handle-taking exports, the config block, host-placed ports, the bump allocator, sealing, touching pages, the status word, the per-renderer budget, the dead flag.
  - *Out:* the second layout, the Spectre exception, `static mut`, `k_init` and `k_port` with the overlap check, dlmalloc's reuse, the doubling during a reset, 6,058 Stores and a `Box` per kernel, and the trap-time reset.
- Most of what comes in is what ABI v1 needs anyway. Only the allocator's modes and the page touching are new work, and both remove a risk today carries.
- In-place edges are the one addition that is purely an optimization. Under the owner's rule, they wait.
- **So:** with the fixes, about as many concepts as today, and fewer exceptions. R1 is less work now but brings in no handles before v1 freezes. R2 breaks RENDER-32 and gains nothing, since ABI v0 keeps a memory per state.
- **A simpler L1 to weigh** before generations and compaction: a second engine at P2, with explicit checks and the Spectre guard on, for top-level sandboxes only.
  - Estimate: P2 with the guard on cost +473 µs a callback (`docs/architecture.md:330`). Top-level calls are 150 of 2,245 (code-map §6.2), so 473 × 150 / 2,245 ≈ +32 µs, and up to double that (+63) because top-level calls run full blocks. Unverified that the cost spreads like this.
  - It costs a second compile of each package; the compile time is unmeasured (code-map §2.2).
  - It removes the 4,033 ceiling for top-level kernels, with no generations.

### A.8 The numbers

- **Right, re-checked:** the percentages in §0.1 (5,908 / 6,058 = 97.5%; 2,095 / 2,245 = 93.3%); §1's table; §11.3; §6's chain (1,000 × 0.7 µs = 700 µs, 26.2% of 2,667 µs); and L2. For L2, the profile's all-cuts column measured 167 + 643 + 98 = 908 µs of host code, m2-runtime code and trampolines (`results.md:88-91`). That is 908 / 9,250 = 98 ns a call, inside the design's 94 to 100 ns.
- **Cuts 1 to 3:** right from `plan.md:13, 38`.
- **Bounds checks:** −84 to −157 is the lockstep's figure (`plan.md:40`). The gate measured O1 at +87 (+75 to +104) with the cuts (`plan.md:48`). So the range is −75 to −157.
- **Copies inside pools: the arithmetic is right, the model isn't supported.**
  - Every step checks out: 2,095 × 2.50 to 2.54; 351 to 614; 74% to 80%; 303 to 325 µs. The per-port counts match `std/m2pkg.toml`: svf has three outputs, so a bass voice has 13 + 3 = 16.
  - But memcpy self time went 523 → 408 → 393 µs across S, HEAD and all the cuts (`results.md:92`), with the same copies every time. So it follows cache misses more than the number of copies.
  - Read −275 to −325 µs as a guess at the most it could save. Slice 0 measures it.
- **The total, with in-place edges deferred:** (−290 to −350) + (−75 to −157) = −365 to −507 µs. That is 6,805 to 6,947 µs: 5.0% to 6.9% under HEAD and 1.1% to 3.1% under S (7,026). The arithmetic is right; that the parts add up is still unverified.
- **The build:** the arithmetic is right (35 / 6,058 = 5.8 µs; × 200 = 1.2 ms; 182 ms). "At most" isn't shown.
  - Each state's birth makes several `m2_alloc` calls, where today's instance makes `k_ports`, a `k_port` per port and six export lookups (`lib.rs:206-213`).
  - The design touches fewer pages than S does.
  - So 182 ms is an estimate, not a bound.
  - *Simplification:* one `m2_alloc` per state, with the host laying out the table, the config and the buffers in one block.

### A.9 Verdict: sound with fixes

High:
1. **The reset's allocator.**
   - The window is `K`'s heap only, not `Host<K>`.
   - Drop the old `K` in place before the rewind.
   - One constructor, not inlined, shared with `k_new`.
   - `alloc_zeroed` zeroes outside birth (A.3 #2).
2. **A top-level box carries its `shape`, its `state` and its port addresses** (A.2 #2).
3. **The config block is length-prefixed,** with a `settings` slot and room for the offline time map (A.6 #2).
4. **Defer in-place edges.** If they are kept, alias only kernel-to-kernel edges in one sandbox with matching channels, and copy out whatever else reads an output (A.4 #3).

Medium:

5. **After a trap, a dead flag,** and the box is made new by the next plan. No reset after a trap (A.2 #5).
6. **Touch `[0, the allocator's end)`,** the stack included. Add up to 13.1 MB to §11.2 (A.3 #3, A.5 #3).
7. **The budget is per renderer,** not counted across the process (A.5 #2).
8. **Keep a birth cap per sandbox** in the limiter (A.5 #4).
9. **Plan L5, or a host-side sampler, in step 9** if banks are large (A.6 #4).

Low:

10. **Status codes:** the first cause wins, `panicking()` labels a panic, and the kit installs no hook (A.6 #3).
11. **ABI-5:** "no per-state data in statics" (A.4 #4).
12. **The allocator and the exports are wasm32-only** (A.3 #5).
13. **Restate the figures:**
    - callback −365 to −507 µs on Linux with in-place deferred;
    - footprint 36 to 43 MB on Linux and 48 to 55 MB on the Mac, plus the host's share;
    - build about 182 ms, as an estimate;
    - the ceiling 3,999 to 4,033 a renderer (A.8, A.5 #1).

**With these fixes, the design is the simplest of the four layouts that survives ABI v1's freeze.**
- It has one layout and one unit (the box) for moving, freeing, isolating and threading.
- It has no Spectre exception.
- The format leaves room for every later layout in §12.
- Its slices stand, minus slice 3 (in-place edges).
- Slice 0 should also measure what A.8 marks unmeasured: the copy model, and the stack depth that `k_process` reaches.
