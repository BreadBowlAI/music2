# design-simple: one sandbox per track and package

Label: design-simple. This is research only: nothing was built, run or measured for it.

How to read the citations:
- A bare path such as `core/runtime/src/lib.rs:332` is under `/home/jerry/m2-runs/idea3/src` (step 7, `ed7fc8e`). `cut1.diff:N` is `/home/jerry/m2-runs/idea3/cut1.diff`.
- `W/…`, `E/…` and `CG/…` are `wasmtime-49.0.2/src`, `wasmtime-environ-49.0.2/src` and `cranelift-codegen-meta-0.136.2/src`, all under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.
- `results.md` is `~/m2-runs/speed/prof/results.md`, `plan.md` is `~/m2-runs/speed/plan.md`, and `refresh.md` is `~/m2-runs/s8a/refresh.md`. `code-map §N` and `external §N` are the two notes beside this one.
- "Unverified" marks what I couldn't check. "Estimate" marks arithmetic on measured numbers or on counts from the code, and each one shows its sums.

---

## The design on one page

| | Today (step 7 with cut 1) | This design | p50, today → here |
|---|---|---|---|
| **Sandbox** (one Store, one instance, one memory) | one per kernel instance (RENDER-32) | one per **module** (a track, or `song`) **and package** it runs kernels from | 6,058 → **51** |
| **Kernel instance** | a whole wasm instance, with its state in a `static mut HOST` | a **state**: a byte range in its sandbox's memory, named by its **offset**. The code is shared | 6,058 → 6,058 states |
| **Memory layout** | P2: 32 MiB reservation, 64 KiB guards, explicit bounds checks, Spectre guard off | wasmtime's defaults (4 GiB reservation, 32 MiB guards: the hardware checks bounds, and the Spectre guard is on) plus `memory_may_move(false)` | 190 GiB reserved → 207 GiB |
| **Who places state** | the kernel, through Rust's allocator and `memory.grow` | the **host** picks each state's offset. The kit's allocator works inside that state during `init`, and only while the sandbox is being built | — |
| **Memory cap** | 32 MiB per kernel | 4 GiB per sandbox, wasm32's own limit. A memory never grows once it is built | — |
| **Ports** | each kernel's inputs copied in and outputs copied out at every call | read and written **in place**. The host copies only between sandboxes | ~8,980 calls with copies → 200 copies per callback |
| **Live edit** | each kept box (kernel, pool, plugin, ring) moves by swap | an untouched sandbox moves whole, by swap. A touched one is built anew, and at activation its kept states are **copied in at the same offsets** | — |
| **A trap** | the whole song goes silent and the transport stops, with no message | that sandbox goes silent and the session rebuilds it, as it does a plugin that failed (HOST-5) | — |
| **ABI** | v0: 7 exports per kernel, no handle | v1: `init`, `reset`, `process` and `active`, each taking a state's offset; ports go in as a table of addresses | — |

**The whole design, in nine sentences:**
1. A module is a track file or `song`: the `FNode.module` key that 8a's partitions are made from. It runs the kernels of each package it uses in one sandbox: one wasmtime Store, holding one instance of that package's wasm and its one memory.
2. A kernel instance is a state: a byte range in that memory, at an offset the host picks. The kernel's code is shared by every state.
3. Every signal a sandbox's kernels read lives in that memory. Another kernel's output there is read in place. The host writes everything else into it: lanes, notes, cells, and other sandboxes' outputs.
4. Memories use wasmtime's default 4 GiB reservation, so the hardware checks bounds. They never move, and they never grow once built.
5. A live edit, prepared off the audio thread, builds a new sandbox for each sandbox whose part of the plan changed. Every kept state goes at its old offset. Untouched sandboxes are reused.
6. At the activation's block line, untouched sandboxes move into the new plan by swap. Each kept state's bytes are copied from the old sandbox into the new one: a `memcpy`, with no call into wasm.
7. The old sandbox plays its ghosts for the 10 ms window, then leaves whole, which frees every removed state.
8. One thread at a time enters a Store: the thread that builds it until the plan is sent, then the thread running its module's partition.
9. A sandbox that traps goes silent, and the session rebuilds it from init, as it does a plugin that failed.

**Concepts this deletes:**
- the Store per kernel instance;
- the P1 and P2 layouts, and the gate comparing them;
- the 32 MiB cap per kernel (`KERNEL_MEMORY`);
- the Spectre-guard exception;
- the port copies inside a sandbox;
- `k_port` and the overlap check at build;
- the 2× peak a reset needs;
- `static mut HOST`;
- the whole renderer stopping on a trap.

**Concepts this adds:**
- the sandbox, one per module and package;
- the state offset;
- the rule "a kept state keeps its offset";
- the copy of kept states at activation.

**Idea 3, kept and changed.** I keep its unit (per track, per package), the 4 GiB reservation, states as slices of one memory, and sound passed without copies. Its open problem was live edits, and I solve them with one mechanism: rebuild the touched sandbox, and copy the kept states across at the same offsets. The alternatives I rejected are at the end.

---

## 1. The unit

| Thing | What it is | p50 |
|---|---|---|
| module | the `FNode.module` key (`core/ir/src/check.rs:110-113`): a track file, or `song`. It is what 8a labels its partitions by (`docs/designs/step8/design-8a.md:29-31, 230`) | 50 tracks + `song` = 51 |
| sandbox | (module, package): one `Store`, one instance of the package's module, one memory. A module with no kernels has none | 51 (p50 uses only std) |
| instances | 1 per sandbox, plus the dummy default caller that every Store has (`W/runtime/store.rs:779-816`) | 51 + 51 (today 6,058 + 6,058) |
| memories | 1 per sandbox | 51 (today 6,058) |
| states | 1 per kernel node, per voice inside a `poly` | 6,058, as today (code-map §6.2) |
| states per sandbox | bass track 4 × 14 + `drive` + `delay` = 58; pluck 12 × 13 = 156; pad 12 × 12 + `reverb` = 145; `song` 50 `gain` + 49 `add` + 1 `limit` = 100 | code-map §6.2 |

Why this unit:
- **It is the unit the rest of music2 already uses.**
  - 8a runs one partition per module.
  - The session names every node `<module>/<id>` (`core/ir/src/check.rs:110`), and a plugin continues only inside its own module (`core/session/src/tx/continues.rs:7, 57-61`).
  - A package is the unit of trust: one author's code.
- **It answers design-7 §11.1's objection to shared Stores** (`docs/designs/step7/design-7.md:1311-1328`). A sandbox lives exactly as long as one version of its module's part of the plan, so a replaced state leaves with the old version at the end of the window. The objection was that "one long-lived store keeps every replaced instance's memory".
- **It follows wasmtime's own advice.** wasmtime frees an instance only when its Store drops, and advises "have a Store correspond roughly to the lifetime of a 'main instance'" (`W/runtime/store.rs:139-145`).

---

## 2. The kernel ABI (v1)

Exports of a package module (`m2_abi() = 1`, as ABI-3 requires):

| Export | Signature | Meaning |
|---|---|---|
| `m2_abi` | `() -> i32` | 1 |
| `memory` | memory | the module's own memory. It needs no maximum |
| `m2_heap` | `() -> i32` | the first byte the host may use: above the module's shadow stack and static data |
| `{k}_ports` | `() -> i32` | `inputs << 8 \| outputs`, checked against the manifest once per package load, as today (`core/runtime/src/packages.rs:501-509`) |
| `{k}_init` | `(state: i32, args: i32) -> i32` | builds a state of kernel `k` at `state` from the `InitArgs` at `args`, and returns its size in bytes |
| `{k}_reset` | `(state: i32)` | rebuilds the state in place, exactly as `init` left it |
| `{k}_process` | `(state: i32, io: i32, off: i32, frames: i32)` | `io` points to one `Port` per input, then one per output, in manifest order. The kernel reads and writes frames `[off, off + frames)` of each channel |
| `{k}_active` | `(state: i32) -> i32` | as today |

- **`InitArgs`** is written by the host and read only during `init`. The first slice has `{ sample_rate: f32, block: u32 = 128, channels: u32 }`. ABI v1 appends a channel count per port and an asset list (§7) before it freezes.
- **`Port`** is 8 bytes: `{ addr: u32, stride: u32 }`. Channel `c` starts at `addr + 4·c·stride`.
  - `stride` is 128 for a buffer of its own.
  - It is 0 for a mono signal read as stereo, which today is a copy of channel 0 (`core/runtime/src/lib.rs:349-354`).
- **`off`** lets a voice's segment, or a transport run, use the block's own buffers. Today the host instead copies each segment to index 0 (`core/runtime/src/lib.rs:352-355`).

The contract, which replaces v0's "a kernel's state is simply the instance's memory" (`std/kernels-src/kernel/src/lib.rs:3-6`):
- **Where state lives.** A state lives in `[state, state + size)`. Mutable statics hold no state, since one instance holds many states.
- **Growing.** `init` may grow the memory, but only while the host builds the sandbox; its limiter refuses growth afterwards (§3). `reset`, `process` and `active` never allocate or grow.
- **Ports.** Inputs are read-only (ABI-1). The host lays out every buffer, so outputs never overlap inputs or each other.
- **Same size every time.** The same kernel with the same `InitArgs` gives the same size, and a reset gives the same layout.
- **No drop.** The host simply forgets the range, and the memory leaves with its sandbox.

**The kit** (`std/kernels-src/kernel/src/lib.rs`):
- **`trait Kernel` is unchanged** (`INPUTS`, `OUTPUTS`, `new`, `process`, `active`; `lib.rs:49-64`). So the 29 std kernels' code stays as it is.
  - This is an inference: they allocate only in `new`, and nothing in `src/lib.rs` or `src/fx.rs` is a `static` or allocates in `process`. Their one `push` is a ring write (`std/kernels-src/src/fx.rs:154-157`).
- **`Io` keeps its methods** (`lib.rs:67-121`) but reads `Port` entries instead of `Vec<Vec<f32>>`.
- **`export!`** (`lib.rs:176-221`) generates the exports above.
  - `init` writes a header (`sample_rate`, `channels`) at `state`, points the allocator just past it, runs `K::new`, and returns the end minus `state`.
  - `reset` does the same at the same place without dropping the old value (`ptr::write`). So a reset no longer builds the new state beside the old, which today doubles an allocating kernel's heap for a moment (code-map §1.4).
- **A `#[global_allocator]`** hands out memory by bumping a pointer inside the current state, during `init` or `reset` only. Freeing does nothing. An allocation anywhere else fails, aborts and traps, which turns std's "no kernel allocates in `process`" (`std/kernels-src/src/lib.rs:3`) into a check.
- **`m2_heap`** returns the linker's `__heap_base`. That wasm-ld defines it for this target and toolchain is unverified, but it is its usual behaviour.
- **The stack.** std keeps its 64 KiB stack (`std/kernels-src/.cargo/config.toml:1-3`); only its comment changes. Now 51 stacks are prefaulted instead of 6,058 being made (§3).

Gone from v0: `k_port` and the host's port checks (`core/runtime/src/lib.rs:226-248`), `static mut HOST` (`kernel/src/lib.rs:181`), and `max_block` as an argument.

---

## 3. Memory: who places it, the limit, what the user sees

One sandbox memory:

```
0            stack (std: 64 KiB)     data (std: 9,808 B)     m2_heap()
|-----------------------------------|-----------------------|---------------------------------------------------------|
                                                            host scratch | kept states and pools, at their old offsets |
                                                            new states above the highest kept end | buffers  → top
```

- **The builder places everything**, on the actor thread:
  - a scratch area for `InitArgs` and the port table of the current call;
  - each state, and each voice pool's region (its V × K states, then its voices' buffers);
  - every buffer: kernel outputs, imports from outside the sandbox, constants and cells.
  - Kept things go at their old offsets. New ones go above the highest kept end.
- **Growth.**
  - The builder grows the memory to cover the kept ranges (`Memory::grow`, `W/runtime/memory.rs:637`).
  - A new state's `init` grows it further through the kit's allocator.
  - The Store's data is a `ResourceLimiter` (`W/runtime/limits.rs:32, 69`). It allows growth while a `building` flag is set, up to 4 GiB, and records any growth it refuses. The builder clears the flag before it sends the plan.
  - So a live memory never grows, and growth's `mprotect` (`W/runtime/vm/memory/mmap.rs:191-217`) never runs on the audio thread.
- **Prefault.** The builder writes every page the sandbox will use:
  - it zero-fills each state range before `init` (and `init` writes it again);
  - it runs one reset on every state, as `prewarm` does today (`core/runtime/src/lib.rs:1294-1310`);
  - it writes every buffer, and touches the stack region once.
  - So no first touch faults on the audio thread (external §C2).
- **The limit is 4 GiB per sandbox.** That is wasm32's largest memory: 2^16 pages of 64 KiB (https://webassembly.github.io/spec/core/valid/types.html, memory types). There is no second setting. It replaces `KERNEL_MEMORY` (`core/runtime/src/lib.rs:87-88`) and StoreLimits' `memory_size` (`lib.rs:202`).
- **Past the limit, at build:**
  1. The limiter refuses the grow and records it.
  2. The kit's allocator gets null, Rust aborts, and `init` traps. The build fails.
  3. The builder reads the refusal it recorded, and the error says so, for example: "Track t03 can't hold its kernels: `std:delay` asks for 6.1 MB more than the 4 GiB a track's std kernels share."
  - **An edit** shows today's `Way::Unbuilt` notice ("…couldn't get it ready to play, so the version before it keeps playing…", `crates/server/src/graph/actor.rs:411-424`), with that detail. The out-of-memory match in `crates/server/src/graph.rs:382-387` learns the case: today a refused grow there isn't called memory at all (code-map §3.7).
  - **An open or an export** fails with the same text.
- **Live, nothing can grow.** A kernel that allocates in `process` traps, and §8's path follows: that sandbox goes silent, a notice names the track and the kernel, and the session rebuilds it.
- **Not a rule: total RAM.** Today's cap doesn't bound it either, since 6,058 × 32 MiB is 190 GiB. A budget for the session is left for later (§12).
- **Address space.**
  - Each memory takes 32 MiB of guard before it, the 4 GiB reservation and 32 MiB after: 4.0625 GiB (`E/tunables.rs:266, 326-327`; `W/runtime/vm/memory/mmap.rs:62-69`). No room for growth is added, because std declares no maximum (`mmap.rs:81-92`; external §D1).
  - p50: 51 × 4.0625 = 207.2 GiB per renderer. Two renderers take 414.4 GiB, which is 0.32% of 131,072 GiB. The 25% rule allows 32,768 GiB, 79 times as much.
  - The ceiling is 16,384 / 4.0625 = 4,033 sandboxes per renderer (code-map §2.3). It counts modules × packages, not kernels.

---

## 4. A live edit, step by step

**Which sandboxes an edit touches.** A sandbox of the old plan is *untouched* when the new plan has exactly the same kernel nodes of that package in that module, and:
- each continues one of them with the same `Shape` (`core/runtime/src/keep.rs:166-189`);
- they are wired the same way among themselves;
- their folded constants are the same;
- the same set of their inputs comes from outside the sandbox.

Cell values don't count, and neither does *which* outside node feeds an input, since the host writes both. Any other change makes the sandbox *touched*.

**Steps:**

1. **Prepare** (on the actor thread, while the old plan plays). Continues and `keep.rs` decide what is kept, the ghosts and the fades, as today (`keep.rs:251-455`).
   - **An untouched sandbox:** the new plan's nodes reuse its layout. The offsets come from the old `PlanInfo`, which the audio thread never writes (`core/runtime/src/plan.rs:136-140`). The `Entry` gets one move for the whole sandbox.
   - **A touched sandbox:** the builder
     - makes a new `Store` and instance;
     - lays out the kept states at their old offsets and the new ones above;
     - runs `init` on all of them, the kept ones too. A fresh copy of a kept state prefaults its pages, and serves as the fallback if the state was poisoned;
     - prewarms, writes the constants and cells, and clears `building`;
     - adds `Carry { from, to, ranges }` to the `Entry`.
   - A kept pool keeps its whole region at the same offsets: its states and its voices' buffers. Only its states go in `ranges`.
2. **Hand-off.** `Msg::Activate(Box<Renderer>)` goes over the bounded queue, as today (`core/runtime/src/engine.rs:41-58`).
3. **Activate**, on the audio thread at a block line, outside a window, with stash room (`engine.rs:411-422`):
   - **Verify:**
     - the plan's id;
     - each moved sandbox is present, with its version hash (a hash of its part of the plan, like today's shape hash, `plan.rs:551-578`);
     - each carried range lies inside both memories.
   - **Swap.** Each untouched sandbox moves in (an `Option<Box<Sandbox>>`, swapped as `take_from` swaps boxes, `plan.rs:690-712`). Host boxes move as today: a pool's `VoicePool`, plugins, rings.
   - **Copy.** For each range of each `Carry`, `copy_nonoverlapping(old_base + off, new_base + off, len)`. The bases are kept from the build, as cut 1's `MemView` keeps them (`cut1.diff:53-60`); `memory_may_move(false)` holds them in place.
   - **Poisoned nodes.** A range whose node was poisoned (its last block was non-finite) isn't copied: the node starts from its fresh `init`. That is RENDER-25's reset, done without a call. A poisoned node in an untouched sandbox gets `k_reset`, as today (`plan.rs:595-604`).
   - **Cells** are carried by writes into their buffers (`Cells::carry`, `plan.rs:631`).
   - **The old sandbox** of a touched one goes into the new plan's ghost slot when it has ghosts. Otherwise it stays with the old renderer, which is retired (`engine.rs:436-440`).
4. **The window**, 10 ms (RENDER-25a, `docs/architecture.md:258`):
   - Ghosts run in the old sandbox, with its old layout. What they read from outside it, such as a kept node now running in the new sandbox, the host copies into the buffer they read before: that node's old output buffer, which nothing else writes now. So a ghost "reads what it read".
   - Fades are host code. A fade reads the ghost's buffer in the old memory and writes the crossfade into the reader's input buffer in the new one.
5. **The end of the window.** `end_fade` (`plan.rs:645-669`) puts the old sandboxes in `Leftovers`, which leave as `Retired::Nodes` (`engine.rs:488-495`). `m2-play-drop` drops their Stores, which unmaps the memories (`core/play/src/lib.rs:693-711`). Every removed state is freed then: 10 ms after the activation, plus at most one 5 ms poll.

**Nothing on the audio thread allocates, locks or makes a system call:**
- An activation does swaps, `memcpy` between pages that are mapped and prefaulted, and writes of cell values.
- Kernel calls are the usual `process`, `active` and `reset`, plus `k_reset` on a poisoned node in an untouched sandbox. A call allocates nothing when it succeeds (`W/runtime/func/typed.rs:154-224`).
- No `memory.grow` runs: the limiter refuses it after the build.
- No Store drops there: they drop on `m2-play-drop`.
- The one exception is today's: a trap's error allocates (`engine.rs:573-574`).
- `core/play/tests/no_alloc.rs` gains two checks: an activation whose touched sandbox carries a pool and a delay, and "no memory grew live".

**Who touches which Store, and when** (a Store is entered by one thread at a time, since a call needs `&mut Store`: `W/runtime/func/typed.rs:100-105`):

| When | Thread | Which Stores | What |
|---|---|---|---|
| building a new sandbox | the session actor (live plans, and an export's renderer: `actor.rs:1200-1205`); a CLI's main thread | only the new ones | `Store::new`, `Instance::new`, growing, `init`, the prewarm reset, writing buffers |
| reading the old plan | the actor | none | offsets in `PlanInfo` only: host data |
| the hand-off | actor → device thread | moved inside `Box<Renderer>` | `Store<T>` is `Send` when `T` is (`W/runtime.rs:173-190`); never `Sync` |
| activation | device thread, at a block line, with no worker inside the job (design-8a.md:485-498) | old and new memories, through their raw bases; `k_reset` for a poisoned node in an untouched sandbox | swap, `memcpy`, cell writes |
| each block | whichever thread runs the module's partition (the device thread or a worker) | that module's sandboxes: the new ones, and the old one during a window | calls, and the host's writes into their memories |
| a copy between sandboxes | the consumer's task | reads the producer's memory after the producer finished | `memcpy` |
| the post-pass | device thread, after every partition is done | `can_idle`'s top-level `k_active` (refresh.md:110-121) | calls |
| panic | device thread, at a block line (`engine.rs:400-407`) | every sandbox | `k_reset` |
| retiring | `m2-play-drop` | the old sandboxes | drop, which unmaps |
| an export | the export's own pool (design-8a.md:676-697) | the export's own sandboxes | as each block |

**What p50's G7a edit costs at activation.** G7a adds and removes a `delay` on `t02`, ten times (design-7 §12.1). `t02` is a pluck track: `i = 1`, `reference.rs:84-89`.
- `t02`'s sandbox is touched. It carries the pluck pool's 156 states.
  - Each state is the kit's header plus a small struct, for example `Osc { sr, phase }` (`std/kernels-src/src/lib.rs:27-30`) or `Svf { sr, ic1, ic2 }` (`lib.rs:436-440`).
  - At 64 bytes or less each, that is at most about 10 KB (estimate).
- `song` is untouched: only *which* node feeds its import changed.
- At about 102 GB/s, 10 KB copies in about 0.1 µs. That figure is one M1 Max core in AnandTech's read-modify-write test, as quoted on their forum (https://forums.anandtech.com/threads/how-did-apple-achieve-such-high-memory-bandwidth-on-the-m1-pro-and-max.2599886/post-40654684). It is unverified for the M1 Pro, and for `memcpy`.
- For scale, the slowest activation block measured today is 2.4 to 4.2 ms, against a bound of 5.3 ms (results.md, the G7f table; `docs/architecture.md:330`).

---

## 5. Voice pools, plugins, exports, seeks and resets

**Voice pools**:
- **The host part is unchanged.** `VoicePool` (the slots, notes, `born`, `dirty`) is one box that moves by swap (`core/runtime/src/lib.rs:410-416`; RENDER-16). Its voice graphs name offsets, not instances.
- **Its states** are V × K states in a row, followed by each voice's buffers.
- **Voice cells.** A cell inside a voice has one value for every voice (`core/runtime/src/cells.rs:27-34`). Today it is written into every voice's graph (`cells.rs:376-386`); here it is one buffer per pool that every voice reads.
- **Kept pools.** A pool with the same `Shape::Poly` (`plan.rs:48-54`) has the same layout. So a kept pool keeps its offsets: in an untouched sandbox it moves with the sandbox, and in a touched one its states are carried.
- **Reusing a voice** resets its states in place, with a `k_reset` each, on the partition's thread, as today (`lib.rs:1454-1456`). A reset allocates nothing.
- **Captures.** An outer node in the same sandbox is read in place. Any other is copied in by the host, which today copies every capture (`lib.rs:1167-1175`).

**Plugins:**
- **Unchanged:** their lifecycle, `Arc` box, state loads and failure path (HOST-5; `core/runtime/src/lib.rs:914-985`).
- **Optional:** their audio slices can point straight into a sandbox's memory, so a plugin between two kernels costs no copy. Plugins are native code, trusted as any DAW's (`core/plugins/src/lib.rs:8-9`).

**Exports (two renderers):**
- **A pure song:** the export's renderer is built on the actor (`actor.rs:1200-1205`) with every sandbox new and nothing carried, then rendered on `music2-export`. p50 has 51 sandboxes in each renderer.
- **A song with plugins:** lending the live plugins (`lib.rs:1793-1805`) is unchanged.
- **Edits during an export.** Edits made during a pure export can still make a third, partial set (code-map §5.6), here of touched sandboxes only.

**Seeks and resets:**
- **Live:** a seek, a wrap or a stop resets nothing (RENDER-10).
- **Panic:** `k_reset` on every state (`engine.rs:400-407`).
- **Offline seek:** reset everything, then replay from 0 (`lib.rs:1987-2000`).
- **A new player or a rate change:** everything is built new, as today.

---

## 6. How 8a's partitions map onto it

- **One partition is one module** (design-8a.md:29-31). Its task enters that module's sandboxes (one per package), and only that task does, in that block.
- **8a's single-writer rule holds by construction** (design-8a.md:127). Every byte of a sandbox's memory is written by its own module's task: host nodes, fades and copies are all labelled with their consumer (design-8a.md:229-233).
- **One change: an added edge.** A module that is split by depth into `(m, 0)`, `(m, 1)`, … (design-8a.md:202-206) gets an edge `(m, d) → (m, d+1)`, so its partitions never run at the same time. Each added edge goes up in `d`, so the graph stays acyclic, by the same argument as design-8a.md:203-206.
- **Copies between sandboxes** are made by the consumer's task, after the producer has finished. The dependency gives the happens-before edge, so reading another memory races nothing.
- **Ghosts.** A removed module's ghosts run in its old sandbox, in a partition of their own (design-8a.md:230).
- **The precondition row changes** (design-8a.md:100): from "one Store per kernel instance" to "one Store per module and package".
- **Failure, live** (design-8a.md:363-372, 563): a trapped sandbox's outputs become silence, and its dependents run on (§8). Offline is unchanged: the render fails.
- **Unchanged:** `tls_eager_initialize` on each worker (refresh.md:66), and the audio workgroup.
- **Voices across threads** (design-8a.md:699-716) would give a heavy pool a sandbox of its own, or one per group of voices. The ABI allows any grouping, so that is host policy, not a format change.

---

## 7. How ABI v1 fits

- **Events.** An entry in the port table can point at an event buffer in the sandbox: `{ count, cap, events[cap] }` of RENDER-14's `Event`. The host writes event inputs before the call and reads event outputs after it. It is the same mechanism as audio.
- **Asset blobs.**
  - The builder copies each asset a sandbox's kernels use into its memory, once per sandbox and deduplicated by content hash. It places the asset like a state and lists `(addr, len)` in `InitArgs`.
  - An asset is read-only by contract; a write could harm only that sandbox.
  - A kept state may point into an asset, so a touched sandbox gets its assets at their old offsets. The builder copies them there from the asset cache; nothing is copied at activation.
  - Assets count toward the 4 GiB.
  - **"Outside the memory" options, and why not now:**
    - a `SharedMemory` shared by several Stores is a tier-2 feature that gets no CVEs (`W/config.rs:3316-3335`), and it needs atomics in std, so a nightly `-Zbuild-std` (external §C4);
    - host calls need imports, which kernels don't have (`core/runtime/src/packages.rs:470-474`), and they cost a call per read;
    - a read-only file mapping inside each reservation, through `Config::with_host_memory` (`W/config.rs:1701`), would share pages between tracks. Whether wasmtime accepts a foreign mapping there is unverified, so it is left for the sampler, if it needs it.
- **Channel counts per port** go in `InitArgs`, one per port. The `Port` stride already covers mono read as stereo.
- **State save and load:** `{k}_save(state, buf, cap) -> len` and `{k}_load(state, buf, len) -> i32`, in bytes the kernel defines and that don't depend on where the state is. They are for projects and presets.
  - The carry doesn't use them. It copies raw bytes between instances of the same module (the shape holds the module's SHA, `plan.rs:35-39`), at the same offset, so it needs no format.
- **Latency and tail:** `{k}_latency(state) -> i32` and `{k}_tail(state) -> i32`, in frames.
- **Offline kernels:** a `{k}_render` over whole buffers the host places, with the same `Port` shape and a larger frame count. Later.

---

## 8. Isolation: what a buggy kernel can harm

- **It can harm everything in its sandbox's memory:**
  - the other states of its package in its module;
  - the buffers there, including signals other kernels read;
  - the scratch, which is rewritten before every call;
  - its package's static data in that module.
- **It can't reach:**
  - other modules, or other packages: theirs are other memories in other Stores (`W/runtime/func.rs:1210-1213`);
  - anything beyond its memory: the 4 GiB reservation and its guard catch the access (`E/types.rs:2376-2407`);
  - host memory, or plugins;
  - growth live;
  - the host: it has no imports;
  - a speculative read past its memory. The Spectre guard is on again, and on the accesses that need no bounds check it costs nothing (`CG/shared/settings.rs:290-302`).
- **Against today:** a kernel can harm only itself, but one trap silences the whole song and stops the transport, with no message (`engine.rs:570-579`; code-map §3.7). Here harm can reach its package's states in its track, but a trap silences only that sandbox.
- **The failure path is the plugins' path** (HOST-5, `docs/architecture.md:1156-1157`):
  1. The sandbox has an atomic `dead`, like `PluginInstance::kill` (`core/runtime/src/plugins.rs:104-113`).
  2. On a trap, the host zeroes the sandbox's output buffers once and stops calling it.
  3. The actor finds it as it finds dead plugins (`heard_plugins`, `actor.rs:1120-1140`), says so, and puts it in `restarts`. The next activation builds the sandbox new, with nothing carried. One that fails again within 10 s stays silent (`Open.revived`).
  - So "or that trapped" leaves RENDER-10 and RENDER-25: a trapped sandbox is new, not reset.
  - A trap leaves the sandbox's shadow-stack global unwound, but it is never called again, so that doesn't matter.
- **An endless loop:** as today, there is no fuel or epoch. 8a's rule for a stuck task applies (design-8a.md:56).

---

## 9. Determinism

- **The copy is exact.** It is a byte copy of each state as of the activation's block line.
- **Reading in place changes no value.** In-place reads see exactly what the copies held, and a stride of 0 equals today's copy of channel 0. Offsets enter no std kernel's arithmetic: a kernel indexes its own `Vec`s, with bounds checks.
- **So these stay the same files:** the 8 recorded renders (`e2e/renders.sha256`) and the p16 render. Live playback with no edits must still equal the export (G7c).
- **A new risk.** A kernel that reads memory outside its own range gives output that depends on the layout, and a live layout after many edits differs from an export's fresh one.
  - std can't do this: `K` is safe Rust, and the kit's only `unsafe` is in `Io`.
  - For other packages, a gate catches it: render a song fresh, and again after a chain of edits that keep everything. G7c already says activations of the same revision change no bit.
- **Threads.** Each sandbox runs on one thread at a time, so the bits don't depend on the thread count, as design-8a §2.1 argues.

---

## 10. What changes

**Code:**

| File | Change |
|---|---|
| `std/kernels-src/kernel/src/lib.rs` | rewritten: v1 exports, the allocator inside a state, `Io` over `Port`s, `m2_heap` |
| `std/kernels-src/src/*.rs` | none (inference, §2) |
| `std/kernels/std.wasm` | rebuilt (ABI 1). Its SHA changes, so the first activation after the update starts every std state from init (RENDER-25) |
| `core/runtime/src/packages.rs` | `verify_module` checks the v1 signatures (`:467-509`). `engine_with` (`:103-133`) becomes the defaults plus `memory_may_move(false)`, and `Layout` goes |
| `core/runtime/src/lib.rs` | `KernelInst` (`:148-276`) becomes `Sandbox` (Store, base and size, functions per kernel type, version, `dead`) and `KernelNode` (sandbox, kind, offset, ports), held inline. Buffers move into sandbox memory for whatever kernels read. `KernelNode::process` (`:332-375`) copies nothing. `prewarm`, `kernel_memory` and `LIVE_KERNELS` count sandboxes. The pool build (`:882-907`) places regions |
| `core/runtime/src/keep.rs` | decides which sandboxes are touched, lays out kept offsets, and lists the carries |
| `core/runtime/src/plan.rs` | `Entry` gains sandbox moves and `Carry`. `verify`, `take_over`, `take_from` and `end_fade` (`:551-669, 690-712`) gain the swap and the copy |
| `core/runtime/src/cells.rs` | cells are written into sandbox buffers; one buffer per pool for voice cells |
| `core/runtime/src/engine.rs`, `crates/server/src/graph/actor.rs` | a trap marks its sandbox dead instead of stopping everything (`engine.rs:570-579`). `heard_plugins` also handles dead sandboxes |
| `core/runtime/src/parts.rs` (8a, new) | the edge `(m, d) → (m, d+1)`, and live failure that is silent and keeps going |
| `crates/gates/src/store.rs`, `arena.rs` | G7f compares against S only; there is no P2/P1 pair left |
| tests | `no_alloc.rs` (a carry, no growth live), keep tests for carries, churn counts sandboxes, a test that a trapped sandbox goes silent alone |

**Rules** (`docs/architecture.md`):
- **RENDER-32 (`:328`)**, rewritten: "Store layout: one wasmtime `Store` per module (a track, or the song) and package whose kernels it runs, holding one instance of that package's wasm and its one memory. Each kernel is a state in that memory, at an offset the host chooses. Memories use wasmtime's default reservation and guards, so bounds checks are elided and Cranelift's Spectre guard is on, and they never move, nor grow once built. A Store lives for one version of its module's part of the plan: an activation that changes that part builds a new one, copies each kept state into it at the same offset, and retires the old one after the window. Two p50 renderers MUST build side by side within 25% of the process's address space." The bounds on time, footprint and build stay, measured against S.
- **ABI-1 (`:1166-1168`):** "A kernel MUST NOT write an input's memory. The host lays out every port, so none overlaps another. Debug builds compare input memory before and after each call." The overlap check at build goes. `k_reset` no longer sees inputs at all.
- **ABI-2 and ABI-3 (`:1170-1180`):** v1 is §2's ABI plus the reservations, in §7's shapes.
- **RENDER-25 (`:253`):** "instance" becomes "state". The moves are whole sandboxes and copies of kept states. "Or that trapped" goes (§8).
- **RENDER-10 (`:158`):** "or that trapped" goes.
- **Unchanged in wording:** RENDER-25a and RENDER-28. RENDER-28's *Today* line gains the sandbox points (§4).
- **RENDER-16 (`:198-204`):** its *Today* line says a pool's states live in its module's memory, and the box is its host part.
- **HOST-5:** a note that kernel sandboxes share its failure path.
- **The deferred row** "Spectre guard back on before a non-std kernel runs" (`:1232`) is met, so it goes.
- **design-7 §11** (`docs/designs/step7/design-7.md:1309-1380`) is superseded. Its reason, a Store that outlives what it holds, is answered in §1.
- **design-8a:** the precondition row (`:100`), §3.2 (the added edge) and §4.3 (live failure).

---

## 11. Expected effect on p50 (Linux only; the Mac judges)

| | Today, HEAD with P2 | This design | Arithmetic and sources |
|---|---|---|---|
| Stores, instances, memories | 6,058 each | 51 each | §1 |
| Address space, one renderer | 190 GiB (measured) | 207 GiB | 51 × 4.0625 GiB |
| Two renderers against the 25% rule | 380 GiB, 0.29% (measured) | 414 GiB, 0.32% | 2 × 207.2 / 131,072 |
| Bounds checks in kernel code | +9.6 M instructions a callback; +85 µs (+67 to +109) on Linux, +141 µs on the Mac | none | P1's code: 78.2 M JIT instructions against P2's 87.8 M (results.md); plan.md table |
| A 512-frame callback | S + 204 to 212 µs (lockstep) | **S − 184 to S − 568 µs**, which is 6,836 down to 6,452 µs at S's 7,020 | see below |
| Footprint of one renderer | 195 MB on Linux, 575 MB on the Mac | about 31 to 104 MB on Linux, about 72 to 296 MB on the Mac (estimate) | see below |
| Build of p50 | 216 ms (S 181) | at most about 181 ms | see below |
| Copy at a G7a activation | 0 | at most about 10 KB | §4 |

**The callback**, step by step:
1. **The closest measured build** is "all cuts on P1": addresses kept, one Store per pool, the instance inline, the poison check at activations, and a 4 GiB layout. It measured S − 184 µs (−219 to −170; results.md, campaign E).
2. **That build still copies.** Its profile shows 393 µs of `memcpy` a callback (results.md, "all cuts P2" column).
   - Here, about 8,980 `k_process` calls a callback copy nothing (2,245 a block × 4 blocks; results.md).
   - What is left is 50 copies from the tracks into `song` per block, so 200 a callback.
   - If the `memcpy` time scales with the number of copies, the saving is 393 × (1 − 200 / 8,980) = **384 µs at most**.
   - How much of that comes back as cache misses inside the kernels, which now read their inputs where they were written, is unmeasured. So the range runs from 0 to 384.
3. **Further, unmeasured:** 51 Stores instead of 200 (another small saving); a few stores per call to fill the port table, and state reached through a pointer argument (small costs). HV4's 14 µs (results.md) is counted in step 1, but this design doesn't need HV4.
4. **The total** is S − 184 − (0 to 384). Against HEAD with P2 (S + 204 to 212), that is 388 to 780 µs less.
5. **The Mac** is unmeasured. Its bounds checks cost more (+141 µs) and so do its cache misses (results.md: "its P2 over P1 is +141 µs; here +50"), so the saving there is probably not smaller.

**The footprint**, step by step:
1. **The DSP state any design must keep**, at p50's 48 kHz (`core/session/src/reference.rs:74`):
   - 17 `delay`s × 96,002 floats × 4 B × 1 or 2 channels (`std/kernels-src/src/lib.rs:493-495`) = 6.5 to 13.1 MB;
   - 16 `reverb`s × 27,688 floats × 4 B = 1.77 MB. The comb and allpass lengths are scaled by 48/44.1 (`fx.rs:199-201, 264-270`): 11,994 + 1,700 + 12,194 + 1,800 floats;
   - so 8.3 to 14.8 MB in all.
2. **The rest, per instance today:** (195 − 8.3..14.8) MB / 6,058 = 29.8 to 30.8 KB on Linux, and (575 − 8.3..14.8) / 6,058 = 92.5 to 93.5 KB on the Mac.
3. **Almost all of that is per instance, not per Store.** On the Mac, S held one Store with the same 6,058 instances and took 554 MB, so a Store adds 3.5 KB (code-map §2.4).
4. **How it splits is unmeasured** between the instance itself (stack and data pages, port `Vec`s, allocator pages, wasmtime's structures) and the host's graph buffers, which stay.
5. **The estimate:** if 50% to 90% of it is the instance, Linux saves 6,007 × (15.2 to 27.3 KB) = 91 to 164 MB, leaving about 104 to 31 MB. The Mac saves 6,007 × (46.5 to 83.7 KB) = 279 to 503 MB, leaving about 296 to 72 MB.
6. **Smaller for other reasons:** a reset no longer doubles a delay's heap, and port buffers exist once instead of twice (code-map §1.2).

**The build:**
- S, with one Store and 6,058 instances, took 181 ms; HEAD, with 6,058 Stores, took 216 ms (plan.md, table B).
- Here there are 51 Stores and 51 instances. The 6,058 `init`s and prewarm resets remain, plus zero-filling 8.3 to 14.8 MB.
- So the build is at most about S's 181 ms. How much less depends on how much of S's 181 ms is instantiation, which is unmeasured.
- An edit's build is one instantiation per touched sandbox plus that sandbox's `init`s, where today it is one instantiation per new kernel.

---

## 12. The cost to build it, and what it leaves for later

**Slices**, each ending green:

| # | Slice | Size | Done when |
|---|---|---|---|
| 1 | Kit and ABI v1's shape; std.wasm rebuilt; `verify_module`; the engine's defaults | small: the kit is 236 lines today | the kit's native tests pass, and std.wasm loads |
| 2 | Runtime, fresh builds only: `Sandbox`, offsets, buffers in memory, in-place ports, copies between sandboxes, pools inside module memory, prewarm, strict checks in place | the largest: most of `lib.rs`'s kernel, graph and poly code (`:148-375, 750-1477, 1294-1322`) | the 8 renders and p16 are the same files; `bit_identity`; `every_std_kernel_keeps_its_inputs`; `no_alloc` in steady play |
| 3 | Live edits: which sandboxes are touched, layout, carry, verify and take over, ghosts in old sandboxes, retiring; failure as plugins fail | medium: `keep.rs`, `plan.rs:551-712`, `cells.rs`, `engine.rs`, `actor.rs` | G7b (49 untouched tracks bit-identical, all 200 stateful nodes kept), G7d, `no_alloc` through an activation, churn, keep tests |
| 4 | 8a: the added edge and silent failure | small, either way round | 8a's 1-against-N comparison |
| 5 | Gates and documents: G7f against S, RENDER-32 and the ABI rules | small | G7f's bounds; the Mac's run |

**Order.** After 8a, which is designed and ready around per-kernel Stores and needs only slice 4's small change; and before ABI v1 freezes in step 9. This is ABI v1's own shape, and a change of layout is cheap only before the freeze.

**Left for later**, each an optimization or a feature:
- **Placing new states in gaps** (first fit, by sizes learned from earlier builds), instead of above the top. It stops the slow creep of offsets (§13).
- **Copies spread over threads**, each partition copying its own kept states at its first task, for heavy tracks.
- **One call per sandbox per block.** A schedule run inside the wasm would remove most of the 9,250 host-to-wasm calls a callback, whose host side is 167 to 766 µs (results.md).
- **A budget for the session's kernel memory.**
- **Assets shared read-only** between sandboxes.
- **Heavy pools in sandboxes of their own**, for voices across threads.
- **ABI v1's other pieces:** save and load, events, offline kernels.
- **Fuel or epochs** against kernels that never return.

---

## 13. The weakest point

**The copy at activation runs on the audio thread, and it grows with all the kept state of every touched sandbox.**
- For p50's edits it is kilobytes.
- But editing one node of a heavy track copies the whole track's kept state. Take eight stereo 2-second delays at 192 kHz: 8 × 2 × 384,002 × 4 B = 24.6 MB for every edit of that track.
- At the quoted ~102 GB/s that is 0.24 ms. A copy from cold memory is slower, by a factor not measured.
- The activation block has 5.3 ms, of which 2.4 to 4.2 ms is used today (measured).
- Slice 3's gates must measure it on a heavy track. If that fails, the fix is to spread the copies over the partitions (§12), not to change the design.

**Two smaller weaknesses:**
- **Offsets creep.** A kept state's offset is fixed for as long as it lives, and new states go above the top. So a sandbox's used addresses grow slowly while a long-lived state sits above states that keep being replaced.
  - In p50's terms, a 768 KB stereo `delay` would have to be replaced about 5,500 times before 4 GiB is used: 4 GiB / 768 KB = 5,461.
  - The rule when it happens: a sandbox whose offsets would pass 4 GiB is built afresh with nothing carried. That track's package restarts, crossfaded.
  - Placing in gaps (§12) makes this practically never happen.
- **Isolation is coarser.** One buggy kernel can now spoil every state of its package in its track. A bug that reads outside its own range can make live playback differ from the export (§9). The trust boundary that matters, the package, still holds.

---

## Alternatives I rejected

| Unit | Sandboxes in p50 | Why not |
|---|---|---|
| per kernel instance (today) | 6,058 | The smell itself: 6,058 instances, stacks and copies of the data; bounds checks forced by the address space; copies at every port |
| per box: a voice pool, or a top-level kernel (cut 3 with one memory per pool) | 200 | It needs no new mechanism for live edits. But it keeps 150 sandboxes of one kernel each, keeps the copies at the top level, and puts a ceiling of 4,033 boxes per renderer that grows with the song's top-level kernels: a song as a program can generate thousands. Its one advantage, moving whole, is kept here for untouched tracks |
| per kernel type per module | 421 | 8 times as many sandboxes as per module, the same live-edit problem, and copies between types |
| per renderer | 1 | A Store runs on one thread at a time (`W/runtime/func/typed.rs:100-105`), so no 8a; and it frees nothing until it drops (design-7 §11.1) |
| per module, with no carry (an edited track restarts) | 51 | It cuts the edited track's tails and notes, against RENDER-25's reason (`docs/architecture.md:255`) |
| per module, with new states made inside the live sandbox | 51 | `init`, which is other people's code with no time bound, and first-touch faults would run on the audio thread. Avoiding the faults needs headroom touched in advance in every track |
| per module, by generations (each edit's new states in a sandbox of their own) | grows with edits | After many edits of one node each, it drifts back towards one sandbox per kernel, unless compacted, and compacting is a carry |
| one `SharedMemory` across Stores | — | A tier-2 feature with no CVEs (`W/config.rs:3316-3335`), and it needs atomics through a nightly build of std (external §C4) |
| states with no pointers (offsets only), and compaction | — | Kernels couldn't use `Vec` or pointers: a burden on every third-party author |

## Unverified

- That wasm-ld defines `__heap_base` for std's toolchain (Rust 1.99.0, `rust-toolchain.toml`).
- The memory bandwidth of the M1 Pro for a `memcpy` (the 102 GB/s figure is an M1 Max read-modify-write test, quoted secondhand), and the cost of a copy from cold memory.
- How p50's footprint per instance splits between the instance and the host's buffers (§11's 50% to 90%).
- How much of S's 181 ms build is instantiation.
- Whether p50's delays are mono or stereo (§11 gives both).
- That reaching state through a pointer argument costs the JIT code nothing measurable.
- Whether wasmtime accepts a foreign read-only mapping inside a memory's reservation (assets, §7).
- Whether the session can ever continue a node into another module. If it does, the node starts from init when its offset is taken there.

---

## Attack

Label: attack-simple. Read-only: nothing was built, run or measured. Citations follow this note's conventions (`W/`, `E/`, `CG/`, bare paths under `src`). `8a` is `docs/designs/step8/design-8a.md`. "Arithmetic" marks sums on measured numbers or on the design's own counts. I checked every wasmtime claim I rely on against the 49.0.2 source.

### Verdict

**Sound with fixes.** The unit and ABI v1's shape survive every attack below, and the address-space and bounds-check arguments are right. But five things break as written. Three of them are holes in the host's own memory safety, not the sandbox's (L2, L6, I2). The carry at activation doesn't scale to whole-song edits or to heavy states (L4). The claim to be simpler holds for objects and layouts (51 Stores, one layout). It does not hold for live edits, which gain a second keep decision on top of today's (S1).

Must fix, before slice 3 is green:
- **F1** No box that moves between plans may name a sandbox (L2).
- **F2** Check every number a kernel returns before a raw pointer uses it. A kept state whose fresh size differs isn't kept (I2, D3).
- **F3** Prefault by touching every page of `[0, top)` after the inits, not by zero-filling before them (R3).
- **F4** Bound the carry. A state above a size threshold gets a sandbox of its own and moves by swap. Gate the largest carry on the Mac (L4).
- **F5** No task reads another sandbox's memory while that sandbox's module may still run: the producer copies its exports out (L6).
- **F6** The limiter refuses table growth live, as it does memory growth (R2).
- **F7** The kit's allocator: bound a reset by the recorded size; make `alloc_zeroed` write zeros in a reset; align state offsets to a fixed boundary (R4, D2).

Should fix:
- **F8** Don't init kept states again. Touch their pages, copy them at activation, and `k_reset` a poisoned one in place. Clear a poisoned pool's voice slots as `reset_node` does (L3).
- **F9** Place new states first-fit by recorded sizes from the start (L10).
- **F10** Set `wasm_backtrace_max_frames(None)` in the live engine, and say that a trap costs a dropout (R6).
- **F11** `verify_module` requires one defined 32-bit memory with 64 KiB pages (I3).
- **F12** Before ABI v1 freezes, settle four things: asset hashes in the shape; a spike of shared read-only asset pages; which thread `_save` and `_load` run on; whether garbage-collected languages are in scope (A4, I4, I5).
- **F13** Correct the concept count (S1) and §11's callback arithmetic (N1 to N3).

With F4 the design becomes a hybrid: module sandboxes for small states, and a sandbox per heavy state. That hybrid, not the pure form, is what should go to the owner.

### Live edits

| # | Claim | Verdict | How, and the fix |
|---|---|---|---|
| L1 | An untouched sandbox moves whole, by swap; the activation verifies it | holds | It follows the pattern of `take_from`'s swaps (`plan.rs:690-712`) and `verify` (`plan.rs:554-578`). A pool's host box and its sandbox must pass or fail together. Otherwise the whole activation is refused, as today (`engine.rs:442-448`) |
| L2 | A kept pool's host box moves by swap while, in a touched sandbox, its states are copied into a new Store | **breaks** (host memory safety) | Today a voice kernel's Store and raw base live inside the box. `VoicePool` holds every voice's whole `Graph` (`lib.rs:388-416`), and cut 1's `MemView` sits in the `KernelInst` there (`cut1.diff:53-60`). Here the box and the memory part ways: the box goes to the new plan, and the old Store drops on `m2-play-drop` after the window. §10 puts the sandbox inside `KernelNode`, which a voice graph holds. A kept pool would then call through a pointer into unmapped memory: a use-after-free in host code. **F1:** nothing inside a moving box names a sandbox, a base or a per-plan address. The executor passes the module's sandbox into each call. `PolyNode::captures` already stays outside the pool for this reason, "so a pool moved into another plan reads that plan's nodes" (`lib.rs:423-425`). The same goes for captures read in place and for the voice cells' buffer |
| L3 | A touched sandbox re-inits every kept state, then the carry overwrites it | holds, at a cost | Prepare work grows with the touched track's whole state, not with the edit. A global edit, such as a change to a helper every track uses (song-as-program invites these), re-inits every state of the song. On p50 that is at most S's whole build, 181 ms (`plan.md:54`). On p50×10 it is about 1.81 s if the build scales linearly (arithmetic; how S's build splits is unmeasured). Today such an edit instantiates only the new nodes. **F8:** don't init kept states. Grow the memory over their ranges, touch them (F3), and copy them at activation. `k_reset` a poisoned state in place after its copy, as `take_over` does today (`plan.rs:595-604`). A poisoned pool also needs its host slots cleared, as `reset_node` does (`lib.rs:1228-1236`). "Not copied, so fresh" alone leaves voices mid-note on idle envelopes |
| L4 | The copy at activation is a `memcpy` of kilobytes | holds for G7a; **breaks** for whole-song edits and heavy states | The activation copies every kept byte of every touched sandbox, on the audio thread, in one block. The Mac's slowest activation block is 3.8 ms against 5.3 ms (`docs/architecture.md:330`), which leaves 1.5 ms. A carry of B bytes then needs a cold copy rate of at least B / 1.5 ms (arithmetic):<br>• G7a's 9,984 B: trivial.<br>• p50 with every track touched, 8.3 to 14.8 MB (§11): 5.5 to 9.9 GB/s.<br>• §13's heavy track, 24.6 MB: 16.4 GB/s.<br>• A third-party looper holding 5 minutes of stereo at 48 kHz, 300 × 48,000 × 2 × 4 = 115.2 MB: 76.8 GB/s.<br>• p50×10 with every track touched, 83 to 148 MB: 55 to 99 GB/s.<br>The M1 Pro's cold single-core copy rate is unmeasured. §4's 102 GB/s is an M1 Max read-modify-write figure, and here the source is cold: a delay line is touched only 128 frames a block. §13's fallback, spreading the copies over partitions, stays inside the same block and shares DRAM bandwidth (8a §7 calls p50 memory-bound). **F4:** a state above a size threshold gets a sandbox of its own and moves whole. The carry is then only small states: at the design's own ≤ 64 B a state, 6,058 × 64 = 0.39 MB for a p50 global edit (arithmetic). Slice 3's gate measures the largest carry on the Mac |
| L5 | Ghosts run in the old sandbox and "read what they read" | holds | `keep.rs` remaps a ghost's inputs to the continuing node in the new plan (`keep.rs:319-329`), so a host copy into the old input buffer gives the same values. It costs one extra copy per such edge per window block |
| L6 | One thread at a time per Store; the added 8a edge `(m, d) → (m, d+1)` | the edge holds; reads across sandboxes in a depth split **break** | Every edge between labels of a component raises `d` (`8a:202-206`), so the added edges keep the graph acyclic. But take 8a's own example (tracks `a` and `b`, 8a §3.6). The consumer `(a, 1)` copies `(b, 0)`'s output out of `b`'s memory while `(b, 1)` runs in `b`'s Store on another thread. A well-behaved kernel never writes `(b, 0)`'s buffers, but a buggy one can. The host's plain read then races a write: torn values in practice, undefined behaviour in Rust's memory model. A fade that reads a ghost's buffer in an old sandbox has the same problem. **F5:** as its last step, while it still holds the Store, a producer task copies every buffer another partition reads out of its sandbox. Consumers read only host buffers. This is needed at least for the modules 8a splits by depth. p50 has none, and applying it everywhere costs 50 copies a block |
| L7 | Exports, seeks, resets, loops, a rate change | holds | An export's renderer is built new on the actor (`actor.rs:1200-1205`). A live seek resets nothing (`engine.rs:369-377`). An offline seek resets and replays (`lib.rs:1987-2000`) |
| L8 | A package rebuilt with a new hash | holds | The shape holds the module's SHA (`plan.rs:35-39`), so every sandbox of that package is touched with nothing kept, and `m2_heap` may move. Both generations of those sandboxes live through the window |
| L9 | (§13's unverified item) a node continuing into another module | verified: it never does | `continues` pairs nodes by names that start with the module key (`core/session/src/tx/delta.rs:472`). Keyed plugins continue only "of the same module" (`core/session/src/tx/continues.rs:7`). Nothing regresses |
| L10 | Offset creep, with a sandbox that would pass 4 GiB built afresh with nothing carried | holds, but the remedy is audible | Creep needs edits rotating over distinct nodes; replacing the topmost state again reuses its place. But the remedy restarts the track's package, and large states reach it fast: 4 GiB / 115.2 MB = 37 rotations for the looper above (arithmetic). **F9:** place first-fit now, by the sizes `PlanInfo` already records. "Same size every time" makes those sizes valid |

### Real-time safety

| # | Claim | Verdict | How, and the fix |
|---|---|---|---|
| R1 | No `memory.grow` succeeds live, and a refusal makes no system call | holds | The limiter is asked before anything else, and its `Ok(false)` returns `Ok(None)` (`W/runtime/vm/memory.rs:649-659`). The libcall polls its future once, on the stack (`W/runtime/vm/libcalls.rs:243-262`, `vm::one_poll` at `:218`). The limiter must record a refusal without allocating, for example in an atomic |
| R2 | Nothing grows live | **gap** | A dynamic table grows with `elements.resize_with` on the calling thread (`W/runtime/vm/table.rs:651-657, 701-707`), which allocates. Rust doesn't emit `table.grow`, but a third-party module can. Today's `StoreLimits` sets no table-element limit either (`lib.rs:202`). **F6:** the same `building` flag refuses `table_growing` |
| R3 | The builder prefaults everything the audio thread touches | **breaks as written** | The builder can't zero-fill a new state's range before `init`, because `init` returns the size. "Touches the stack once" leaves out the data pages. On Linux these come from a copy-on-write image, mapped `PRIVATE \| NORESERVE \| FIXED` (`W/runtime/vm/sys/unix/vm.rs:190`), with no `MAP_POPULATE` anywhere in `W/runtime/vm`. So the first read of a constant that only `process` uses faults on the audio thread. Capacity that a kernel reserves in `new` and fills in `process` (a ring) is never written by `init` either. **F3:** after the inits and prewarm resets, touch every page of `[0, top)` (stack, data, states and buffers) with a volatile read and write-back of one byte. It changes no value |
| R4 | The kit's allocator runs on the audio thread | holds | A bump in `reset` is O(1). **F7:** bound it in `reset` by the size recorded in the header, or a nondeterministic kernel overwrites its neighbour. `alloc_zeroed` must write zeros in a reset (`vec![0.0; n]`, `std/kernels-src/src/lib.rs:495`), since only `init`'s pages are fresh |
| R5 | An activation does swaps, `memcpy` and cell writes, with no lock, system call or drop | holds | Stores drop on `m2-play-drop` (`core/play/src/lib.rs:693-711`) |
| R6 | A trap silences one sandbox, and the song plays on | holds, with costs it doesn't state | A trap captures a backtrace and a coredump stack on the trapping thread (`W/runtime/vm/traphandlers.rs:1013-1028`; 20 frames by default, `W/config.rs:585-600`). On the Mac the trapping thread also waits for wasmtime's normal-priority Mach-port helper (`refresh.md:192`). Today the song stops anyway. Here it plays on, so a trap is one dropout for every track. **F10:** set `wasm_backtrace_max_frames(None)` in the live engine. HOST-5's note says a kernel's trap may cost a dropout |

### Determinism

| # | Claim | Verdict | How, and the fix |
|---|---|---|---|
| D1 | std gives the same bits live and in export, on any thread count | holds | The copy is exact, and a stride of 0 equals today's copy of channel 0 (`lib.rs:354`). std's only `static` is the kit's `HOST` (grep of `std/kernels-src/src/`; `std/kernels-src/kernel/src/lib.rs:181`), and Rust doesn't reassociate float operations, so neither alignment nor offset changes a value. One Store runs on one thread at a time (`W/runtime/func/typed.rs:100-105`) |
| D2 | Offsets enter no kernel's arithmetic | holds for std; **fix** for other packages | After edits, the live layout differs from an export's fresh one. **F7:** align every state to a fixed boundary (64 B), so each state's inside is laid out the same wherever it sits. Code that branches on alignment then can't tell the difference. §9's fresh-against-edited gate stays for the rest |
| D3 | "Mutable statics hold no state" | **new risk** for third-party code | The bump allocator puts anything allocated during `init` inside the current state's range, including a table that a `OnceLock` static caches. If the new sandbox inits a different state first, the carry overwrites that table with another state's bytes. Every state of that package in the track then reads garbage. F2's size check catches it, since the first state's size includes the table. Document that nothing allocated may be reachable from a static |

### Address space and footprint

| # | Claim | Verdict | How |
|---|---|---|---|
| A1 | Two p50 renderers fit within 25% | holds | Each memory takes 32 MiB + 4 GiB + 32 MiB (`E/tunables.rs:326-327`; `W/runtime/vm/memory/mmap.rs:62-69, 102-107`). 2 × 51 × 4.0625 = 414.4 GiB, 0.32% of 131,072 (arithmetic). With the Mac's measured 4.097 GiB a memory (49,637 GiB / 12,116, `docs/architecture.md:330`), it is 417.9 GiB, also 0.32%. One correction: the first mapping never includes a growth reserve, whatever the module's maximum (`mmap.rs:102-107`). That, not std declaring no maximum, is why none is added |
| A2 | p50×10 | holds | 501 sandboxes: 2 × 501 × 4.0625 = 4,070.6 GiB, 3.1% (arithmetic). The ceiling, 4,033 modules × packages a renderer, is far off |
| A3 | Footprint of 31 to 104 MB on Linux, 72 to 296 MB on the Mac | the arithmetic is right; the range comes from the assumption | Every sum checks. The answer is set by "50% to 90% of the per-instance remainder is the instance", which no measurement narrows. One thing is missing: during an edit the touched track exists twice, in its old and new sandbox, until the window ends and the drop thread runs. Today only the new nodes are new |
| A4 | Assets are copied into each sandbox's memory | holds, but costly for the sampler | It costs RAM for each track that uses an asset, plus a re-copy at every rebuild of a touched sandbox, so at every edit of that track. Mapping one read-only copy into each reservation needs no ABI change, since a kernel sees only `(addr, len)`. wasmtime treats a fault anywhere inside a memory's accessible range as a wasm trap (`W/runtime/store.rs:1996-2040`), but a `MemoryCreator` that maps shared pages is unverified. **F12:** spike it before ABI v1 freezes. Also put each asset's content hash in the shape, or a kept state keeps pointing at an asset that changed |

### Isolation, ABI v1, third-party packages

| # | Claim | Verdict | How, and the fix |
|---|---|---|---|
| I1 | A kernel can't reach other memories, the host, or anything past its memory | holds | Stores and memories are separate. Elision needs reservation + guard ≥ 4 GiB (`E/types.rs:2376-2407`). The Spectre guard costs nothing where no check is emitted (`CG/shared/settings.rs:290-302`) |
| I2 | The host's port checks go (`lib.rs:226-248`) | **breaks as written** (host memory safety) | The host now writes, touches and copies through raw bases, at offsets built from numbers the kernel returns: `init`'s size, `m2_heap()`, an event buffer's `count`. A kernel that returns a size running past its memory makes the host write outside the sandbox. **F2:** check each number against the memory's size before use, and clamp `count` to `cap`. A kept state whose fresh size differs from its recorded one isn't kept |
| I3 | The default layout elides bounds checks | holds for std; **fix** for other packages | Elision needs a 32-bit memory with pages at least as large as the host's (`E/types.rs:2376-2407`). A memory64 or small-page module quietly gets the checks back. **F11:** `verify_module` requires one defined 32-bit memory with 64 KiB pages |
| I4 | `{k}_save` and `{k}_load` | **gap** | A call needs `&mut Store` (`W/runtime/func/typed.rs:100-105`), and the audio thread holds it, so the actor can't call `_save`. One shape fits this layout. Copy the state's bytes out at a block line. Then, off the audio thread, call `_save` in a scratch Store of the same module, with the bytes at the same offset. Loading a preset into a kept node is an activation in which that state is new. **F12:** v1 says which thread runs each call |
| I5 | Third-party packages after v1 | holds, but narrower than v0 | A state must be one contiguous range, with no state in statics. Rust and C with the kit's allocator fit. Faust's wasm backend fits exactly: its state is a `$dsp` struct (external §B). A garbage-collected runtime with one heap per instance (AssemblyScript, TinyGo) can't hold many states, while v0 accepts any language. **F12:** decide before the freeze. Either such languages are out of scope, or a package may declare "one state per instance" and get a sandbox per state, which the design's host-policy grouping allows |
| I6 | `trait Kernel` and the 29 std kernels stay unchanged | holds | `std/kernels-src/kernel/src/lib.rs:49-64`. std allocates only in `new` (`std/kernels-src/src/lib.rs:495`; `std/kernels-src/src/fx.rs:151, 267-269`) |

### Simplicity

**S1, the concept count:**

| | Listed | Missed |
|---|---|---|
| **Adds** | the sandbox; the state offset; "a kept state keeps its offset"; the carry | the touched/untouched test, a second keep decision per sandbox on top of `keep.rs`'s per node; a version hash per sandbox, and its check at activation; the limiter's `building` flag; the host copies into ghosts' old buffers; the 8a edge; the creep rule; a sandbox handle for `restarts`; the host's layout of each sandbox (scratch, port tables, pool regions) |
| **Deletes, because of the unit** | P1, P2 and their gate; `KERNEL_MEMORY`; the Spectre exception; the copies inside a sandbox | the first three come with any unit of at most 4,033 memories at 4 GiB, per box included |
| **Deletes, because of v1, with any unit** | `k_port` and the overlap check; `static mut HOST`; the reset's 2× peak | — |
| **Deletes, independent of the design** | the renderer stopping on a trap | a `dead` flag per Store gives this in today's layout too |

The design lists 4 added concepts; there are 12. It lists 9 deleted ones; 4 of them are due to the unit. So it is simpler than today in objects (51 Stores against 6,058) and in layouts (one instead of two), and more complex in live edits.

**S2, against its rivals:**
- **One Store per voice pool (cut 3), with today's ABI.** This is the smallest change, but the 6,058 memories stay, and with them P2, its bounds checks and the copies. It is an optimization, not an architecture.
- **One Store per track, with today's ABI.** Dominated. It keeps 6,058 memories, which the 25% rule holds on P2 (6,058 > 4,033). A kept instance can't move between Stores (`W/runtime/store.rs:184-195`), so a live edit must copy whole memories of at least 128 KiB each (the module's minimum, code-map §1.3). One pluck-track edit then copies 156 × 128 KiB = 19.5 MiB (arithmetic), against about 10 KB here.
- **Per box, with v1.** Each voice pool and each top-level kernel is a sandbox, and a pool's states live in its memory. This is the strongest rival on simplicity:
  - It has no touched test, no carry, no ghost copies, no 8a edge and no creep. Today's move by swap stays as it is, and an activation copies nothing, whatever a state's size.
  - It leaves copies at the top level. By §11's own model it saves 393 × (1 − 600 / 8,980) = 366.7 µs, against 384 µs here (arithmetic; 150 top-level calls a block × 4).
  - Address space: 2 × 200 × 4.0625 = 1,625 GiB, 1.24%.
  - Its one real cost is the ceiling of 4,033 boxes per renderer, which a program can reach. A 512-partial additive sound written at the top level is 512 oscillators, 512 gains and 511 `add`s: 1,535 boxes. Three such tracks pass the ceiling (4,605), where this design needs three sandboxes. The design's case for its unit rests on that ceiling, and F4 already moves it part of the way to per box.

### The numbers

| # | Estimate | Verdict |
|---|---|---|
| N1 | A callback of S − 184 to S − 568 µs | The sums are right: 7,020 − 184 = 6,836; 7,020 − 568 = 6,452; 393 × (1 − 200 / 8,980) = 384.2. The sources don't match. The 393 µs of `memcpy` is from the profile of HV1 + HV3 + HV4 on P2 (`results.md:84, 92`), while the base −184 is all four cuts on P1 (`results.md:52`). HEAD's own `memcpy` row differs by 67 µs between P1 and P2 (475 against 408, `results.md:92`), so the matching figure is unknown. The model also counts calls, not ports. Today a steady input isn't copied at all (`lib.rs:349-351`), so "8,980 calls with copies" overcounts |
| N2 | HV4 is "counted in step 1" | If this design leaves HV4 out, add back 14 µs on P2 to 21 µs on P1 (`plan.md:13`; `results.md:145-146`) |
| N3 | The slowest activation block is "2.4 to 4.2 ms" | This mixes machines. On the Mac, which judges, it is 3.0 to 3.8 ms (`docs/architecture.md:330`). 2.4 ms is Linux's G7f (`results.md:170`), and 4.2 ms is Linux with the owner's layout |
| N4 | Address space, the 25% margin (79×), the ceiling (4,033) | right (A1, A2; 32,768 / 414.4 = 79.1) |
| N5 | G7a's carry: about 10 KB, in 0.1 µs | The size is right (156 × 64 = 9,984 B). The time divides by a streaming bandwidth that a copy this small never reaches, but it is negligible either way |
| N6 | Heavy track: 24.6 MB in 0.24 ms | The sums are right: 8 × 2 × 384,002 × 4 = 24,576,128 B, divided by 102 GB/s. The bandwidth is the unverified part (L4) |
| N7 | Footprint | The sums are right; the split is assumed (A3). 6,007 = 6,058 − 51, and 15.2 to 27.3 KB is 50% to 90% of about 30.3 KB |
| N8 | Build "at most about 181 ms" | Plausible, not shown. The zero-fill (and F3's touch) is work S doesn't do. "At most" needs the 6,007 fewer instantiations to cost more than that work, which is unmeasured |
| N9 | DSP state of 8.3 to 14.8 MB | Right: 17 × 96,002 × 4 × (1 or 2) = 6.53 to 13.06 MB, plus 16 × 27,688 × 4 = 1.77 MB. The reverb's lengths follow `std/kernels-src/src/fx.rs:199-200, 264-270` |
| N10 | Creep: 4 GiB / 768 KB = 5,461 | right, in KiB (4,294,967,296 / 786,432 = 5,461.3) |

### What this attack couldn't verify

- The M1 Pro's cold single-core `memcpy` rate, which decides L4's thresholds.
- Whether a `MemoryCreator` can map one read-only asset into several memories, on both systems (A4).
- That the kit can link `__heap_base` with Rust 1.99.0. This is the design's own item. std.wasm exports no such global (code-map §1.3), which settles nothing either way.
- How long a trap takes on the Mac (R6).
