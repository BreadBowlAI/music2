# Kernel layout for music2: one sandbox per box

Label: decide. This is research only. Nothing was built, run or measured for this report. Linux numbers are information only. The Mac judges (PLAT-1).

**Since this was written** (2026-10-16, evening): cut 1 landed on step 7 as `444fc66`. G7f now times S fresh in every run. On Linux the app is +55 µs a callback over S when held on one CPU die, and +97 µs when not (the bound is 320). One run's callback number is only good to about ±150 µs. Step 7's build notes ("Speed") have the details. The estimates below are unchanged.

## In short

- **Recommendation:** give each *box* one *sandbox*.
  - A *kernel* is a small sound part, such as a filter. A *box* is one kernel outside an instrument, or one instrument's voices.
  - A *sandbox* is a closed room for kernel code: the code can touch only its own memory.
  - Today every kernel, in every voice, has a sandbox of its own. In p50, the 50-track test song, that is 6,058 sandboxes. This design needs 200.
- **Why:**
  - Live edits keep working exactly as they do today.
  - 200 sandboxes are few enough for wasmtime's safe 4 GiB memory layout. There the hardware catches stray memory accesses. So the extra checks in kernel code (*bounds checks*) go, and the *Spectre guard* comes back on at no cost. That guard must be on before other people's kernels run.
  - A crash mutes one part, not the whole song.
  - Kernel ABI v1 (the contract with kernel code) freezes in step 9. If it freezes with one state per sandbox, the safe layout can never fit later.
- **Gain (estimates, Linux):**
  - about 375 µs a callback faster than step 7 as built (about 5%);
  - but cut 1 of the speed plan lands first (cut 2 was dropped on 2026-10-16), and it already brings the callback back to within about 30 to 40 µs of S, the speed before step 7. On top of cut 1, this design gains roughly 190 to 210 µs (about 3%). On the Mac, the bounds checks alone cost 141 µs;
  - probably much less memory, but that is unmeasured;
  - the same sound, bit for bit.
- **Cost:** about 4.5 to 6 weeks of work (a rough estimate). Much of slice 1 (the kernel kit and std on ABI v1) is work step 9 needs anyway.
- **Main limit:** a renderer can hold about 4,000 sandboxes, and every kernel outside an instrument takes one. p50 uses 200. A mix with 8 effects on every track fits about 366 tracks. A song past the limit is refused with a clear message, and the version before keeps playing (§2.7, §8).
- **When:** decide before ABI v1 freezes in step 9. Build it after step 8a (playing on several CPU cores), after a short throwaway test build (a *spike*) that measures it.
- **Can the details be changed later?** Yes. Kernels and the plan never see the layout, so it can be tuned later behind them (§11).
- **You decide:** yes or no to this layout, and the open questions in §8. It undoes the 32 MiB layout you chose on 2026-10-15 (RENDER-32), so it needs your sign-off.

## Words used here

| Word | Meaning |
|---|---|
| kernel | a small sound part written in WebAssembly (wasm), such as a filter or an envelope |
| sandbox | a closed room for kernel code: one wasmtime `Store`, one *instance* (a running copy of a package's code) and one *memory* (the only bytes that code can touch) |
| box | what a live edit keeps, moves or throws away whole. Step 7 already has four kinds: a top-level kernel, a voice pool, a plugin and a feedback ring (`src/core/runtime/src/plan.rs:690-712`) |
| voice pool | an instrument's set of voices; each voice plays one note at a time |
| state | one kernel's own data, such as a filter's two numbers or a delay line |
| bounds check | an extra test in the code before each memory access: "is this address inside the memory?" |
| guard | address space that is reserved but never usable, so the hardware catches a stray access |
| Spectre guard | extra code that stops a kernel reading host memory through CPU tricks (speculation) |
| trap | a kernel crash: wasmtime stops the call |
| ABI | the fixed contract between the host and kernel code: which functions exist and what they mean |
| Store | wasmtime's owner object for running code (wasmtime is the library that runs the kernels). A Store frees nothing it holds until it is thrown away whole |
| package | a set of kernels shipped as one wasm file. std is music2's own |
| p50 | the 50-track test song that the speed numbers use |
| callback, block, µs | a *callback* is one request from the sound card for the next 512 frames (about 10.7 ms of sound at 48 kHz). The engine plays it as four *blocks* of 128 frames. A µs is a millionth of a second |
| audio thread | the thread that must deliver each callback on time. It must never wait, and never ask the system for memory |
| plan, renderer | a *renderer* is one built copy of the song, ready to play. Its *plan* is its parts and their order. Playing holds one renderer; an export holds a second. A live edit builds a new plan and swaps it in |
| address space | the range of addresses a program may reserve. Reserving costs no RAM, but the total is limited: 131,072 GiB on Linux. music2's rule lets two renderers use at most 25% of it |
| footprint | the RAM a renderer really uses |
| birth, seal | a sandbox is *born* off the audio thread, and its kernels may take memory then. Then it is *sealed*: its memory can never grow again |
| prewarm | one reset of every state while the song is built, so a problem shows then, not while it plays |
| ghost, fade | after a live edit, a removed part plays on for 10 ms as a *ghost*, while old and new sound crossfade (a *fade*) |
| 8a, partition | step 8a plays the song on several CPU cores. A *partition* is the part one core plays in a block: one track, or the song's mixer |
| S, HEAD, P1, P2 | *S* is the engine just before step 7, the speed reference. *HEAD* is step 7 today. *P1* is wasmtime's 4 GiB layout, used only by a gate. *P2* is the app's 32 MiB layout, the owner's choice O1 |
| cuts 1 to 4 | the speed plan's four fixes (`plan.md:9-13`): 1 keeps each memory's address; 2 keeps the instance inline; 3 gives each voice pool one Store; 4 checks for broken output only at activations |
| RENDER-32, ABI-1, HOST-5, ... | rules in `src/docs/architecture.md`, by number. G7b, G7c and G7f are step 7's gates: tests with bounds |

Citations:
- `src/` is the repo at `ed7fc8e` (step 7, before its speed work). Read a file with `git show ed7fc8e:<path>`. The line numbers are from that commit.
- `W/`, `E/` and `CG/` are `wasmtime-49.0.2/src`, `wasmtime-environ-49.0.2/src` and `cranelift-codegen-meta-0.136.2/src`, under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.
- `results.md`, `plan.md`, `refresh.md`, `cut1.diff` and the notes are in this folder, as they were when this was written (§12).

---

## 1. The problem in plain words

Today every kernel instance lives in its own sandbox. That is rule RENDER-32 (`src/docs/architecture.md:328`).

| p50 today | Number | Source |
|---|---|---|
| Sandboxes (one Store, instance and memory per kernel) | 6,058 | code-map §6.2 |
| ... of them inside voice pools | 5,908 (97.5%) | code-map §6.2 |
| Kernel calls in a 512-frame callback | about 9,250 | `results.md:62-63` |
| Address space each memory reserves | 32.125 MiB: a 64 KiB guard, 32 MiB, then a 64 KiB guard | `src/core/runtime/src/packages.rs:120-121`; code-map §2.3 |
| ... for one renderer | 190 GiB | 6,058 × 32.125 MiB (code-map §2.3) |
| Footprint of one renderer | 195 MB on Linux, 575 MB on the Mac | `results.md:173`; `src/docs/architecture.md:330` |

What is a problem, and what is not:

| Item | A problem? | Why |
|---|---|---|
| 6,058 sandboxes, most holding tiny states. A `gain` is two floats and a flag (`src/std/kernels-src/src/lib.rs:353-376`) | **yes** | Each sandbox repeats a stack, the module's data and a Store. Cold Stores were step 7's extra cost, and cut 1 removes most of it (`plan.md:9`). Each instance costs about 32 KB on Linux and 95 KB on the Mac (code-map §2.4) |
| The 32 MiB blocks | **yes** | wasmtime's safe layout needs 4 GiB per memory (`E/types.rs:2399-2407`). The 25% rule then allows 4,033 memories per renderer, but p50 has 6,058 (code-map §2.3). With 4 GiB each, two p50 renderers took 37.9% of the Mac's address space and failed the rule (`src/docs/architecture.md:330`). That is why the owner chose 32 MiB |
| The bounds checks that the 32 MiB blocks force | **yes** | +9.6 M instructions a callback, which is +12% of the kernels' own instructions (87.8 M against 78.2 M, `results.md:109`). That costs +85 to +105 µs on Linux and +141 µs on the Mac (`plan.md:40, 84`) |
| The Spectre guard is off | **yes, before other people's kernels run** | It must go back on before a non-std kernel runs (`src/docs/architecture.md:1232`). Turned on with 32 MiB blocks, it cost +473 µs a callback and failed the bound (`src/docs/architecture.md:330`) |
| ABI v0 holds only one state per kernel type per instance | **yes, and urgent** | The state is a `static mut HOST` (`src/std/kernels-src/kernel/src/lib.rs:181`), and no call names a state. So no layout can change without a new ABI, and ABI v1 freezes in step 9 |
| One crash stops the whole song, with no message | **yes** | `src/core/runtime/src/engine.rs:570-579`; code-map §3.7 |
| Memory may grow on the audio thread | small | Only a reset that grows can do it. Today two things prevent it: the build's prewarm, and Rust's allocator reusing its freed block. Only tests show that second property (code-map §1.4) |
| Port copies: 408 µs, 5.6% of a callback | **no**, not an architecture problem | It is a speed item, listed under Later (§7) |
| 9,250 kernel calls a callback | **no**, not now | At most about 83 ns of host code per call (external §A7) |
| Live edits | **no**, they work | Tails stay intact and nothing allocates on the audio thread (RENDER-25, RENDER-28). Keep that machinery |
| Address space today (0.29% for two renderers) | **no** | It is fine, but only because of the 32 MiB blocks (`src/docs/architecture.md:330`) |

---

## 2. The recommendation: one sandbox per box

### 2.1 The picture

```
 p50: one renderer, 200 sandboxes (today 6,058)

 bass track (x17) ...... 3 sandboxes
     [voice pool box: 4 voices] ---> sandbox: 4 x 14 = 56 states
     [drive box]                ---> sandbox: 1 state
     [delay box]                ---> sandbox: 1 state
 pluck track (x17) ..... 1 sandbox
     [voice pool box: 12 voices] --> sandbox: 12 x 13 = 156 states
 pad track (x16) ....... 2 sandboxes
     [voice pool box: 12 voices] --> sandbox: 12 x 12 = 144 states
     [reverb box]               ---> sandbox: 1 state
 song module ........... 100 sandboxes: 50 gain, 49 add, 1 limit
 plugins, feedback rings: no sandbox (unchanged)

 Inside one sandbox (one Store + one instance of std.wasm + one memory):

 0        64 KiB      ~74 KiB                               sealed end                 4 GiB
 | stack  | std data  | state | state | ... | settings, port tables, port buffers | no access |
 |<-- the module's -->|<----------- made at birth, then sealed ------------>|
 A 32 MiB guard sits before and after the 4 GiB. A stray access traps this box only.
```

The counts come from code-map §6.2. The layout of std.wasm's stack and data comes from code-map §1.3.

### 2.2 The unit

- **A sandbox belongs to exactly one box.**
  - A top-level kernel owns one sandbox.
  - A voice pool owns one sandbox per package its voices use. p50 uses only std, so each pool has one.
  - Plugins and rings own none.
- **A sandbox lives exactly as long as its box.**
  - It is born off the audio thread.
  - It moves between plans by swap.
  - It is dropped whole off the audio thread, which frees its memory.
  - This follows wasmtime's own advice: a Store should last about as long as one "main instance" (`W/runtime/store.rs:139-145`).
- **One thread enters a sandbox at a time.** A call needs `&mut Store` (`W/runtime/func/typed.rs:100-105`), and a box sits in one 8a partition (`refresh.md:188`).

| | p50 | p50 × 10 | Source |
|---|---|---|---|
| Pool sandboxes | 50 | 500 | one per `poly` |
| Top-level kernel sandboxes | 150 | 1,500 | 17 × 2 + 16 × 1 + 100 in the song |
| All sandboxes | 200 | 2,000 | |
| Kernel states | 6,058 | 60,580 | unchanged |
| Address space, two renderers | 1,625 GiB, 1.24% | 16,250 GiB, 12.4% | × 4.0625 GiB per memory (`W/runtime/vm/memory/mmap.rs:62-69`), out of 131,072 GiB |

### 2.3 The kernel ABI v1

§2.3 to §2.5 are for whoever builds it. To decide, §2.1, §2.2, §2.6 and §2.7 are enough.

**The module (one per package):**

| Item | Shape | Why |
|---|---|---|
| `memory` | defined by the module, 32-bit, 64 KiB pages, not shared; checked at load | the hardware checks need a 32-bit memory whose pages are at least the computer's own (`E/types.rs:2377, 2399-2407`). "Defined" and "not shared" keep one memory to a sandbox, outside wasmtime's tier-2 shared memories |
| `m2_abi()` | returns 1 | v0 modules are refused (ABI-3) |
| `m2_alloc(size, align)` | returns an address, or 0 for "no room"; works only while the sandbox is being born | the host gets room for each state's settings, port table and port buffers from the module's own allocator |
| imports | none (assets are open question 1, §8) | a kernel still has no clock and no host calls |

**Per kernel type `k`:**

| Export | Meaning |
|---|---|
| `k_ports() -> i32` | `inputs << 8 \| outputs`, checked against the manifest, as v0 does |
| `k_new(cfg) -> state` | builds one state, and may take memory. Works only while the sandbox is being born. Returns the state's address, or 0 on failure |
| `k_reset(state)` | puts the state back as `k_new` and `k_load` left it: it clears the running state, and keeps the settings, assets and any loaded state (design-safe's fix A1). Takes no memory, reads no asset, and does a bounded amount of work |
| `k_process(state, io, from, to)` | reads inputs and writes outputs for frames `[from, to)` of the block |
| `k_active(state) -> i32` | returns 1 while sound already taken in can still come out, as in v0 |
| reserved, optional | `k_latency(state)`, `k_tail(state)`, `k_save(state, dst, cap)` and `k_load(state, src, len)`. Their shapes are fixed now; each is built when a kernel needs it |

**`cfg`, the settings block.** The host writes it.
- It starts with its own length, and new fields are only ever added at the end (design-scale's attack, A.6 #2).
- Every list in it (the channel counts, the assets) is given as an address and a count, never written inline. So each field keeps a fixed place when fields are added (design-safe's attack, §3).
- Its fields are:
  - the sample rate;
  - `max_block` (128 live);
  - flags (such as "offline");
  - a channel count per port;
  - settings that size the state (address, length);
  - assets (a handle and a length each);
  - a saved state for `k_load` (address, length);
  - room reserved for the offline time map.

**`io`, the port table.**
- It holds one buffer address per port *channel*, inputs first.
- So one mono buffer can serve both channels of a stereo input. That is the same up-mix today's copy does (`src/core/runtime/src/lib.rs:353-355`; design-safe's attack, A2).
- An event port's entry points at `{count, cap, events}`.

**Five rules every kernel keeps:**

1. Take memory only in `k_new`. Once the sandbox is sealed, its memory can't grow.
2. `k_reset` takes no memory and leaves the state as `k_new` and `k_load` left it. The package check compares a reset state with a fresh one.
3. Never write an input (ABI-1). Read the port addresses from `io` at each call, and never keep them.
4. Write every frame of `[from, to)` on every output channel, and touch only the channels a port has.
5. Keep no per-state data in statics, because every state in the sandbox shares them. And let no static point at memory a state took (design-simple's attack, D3).

**The Rust kit** (`src/std/kernels-src/kernel/src/lib.rs`):
- `trait Kernel` keeps `new`, `process` and `active`. `new` now takes the settings block.
- It gains `reset(&mut self)`. By default, the kit builds the state again with `new`, from the settings it kept. That is right for the 25 std kernels that hold no `Vec` (code-map §1.3). A kernel whose `new` does heavy work, such as reading an asset, or that loads a saved state, writes its own `reset`. Otherwise that work runs on the audio thread at every voice reuse and Panic (design-safe's attack, A1).
- `delay`, `reverb`, `chorus` and `widen` write their own `reset`, which zeroes their lines. If one of them forgets, its `new` asks for memory during the build's prewarm. The build then fails and names it. This never happens live.
- Ten std kernels touch channel 1 whatever their channel count. One is `gain` (`src/std/kernels-src/src/lib.rs:388`); design-safe's attack (A2) lists the others. They loop over their own channels instead.
- The kit's allocator exists only on wasm32. It hands out memory by moving a pointer forward, and only inside `k_new` and `m2_alloc`. Anywhere else it refuses, which traps. It frees nothing.
- `static mut HOST` goes.

### 2.4 How a state is made and freed

**Birth** happens on the actor thread, or wherever the renderer is built (code-map §7). The steps run in this order:

1. **Make a new Store and instance.** The Store's limiter is wasmtime's hook that approves each growth (`W/runtime/limits.rs:32-74`). It allows growth up to the birth cap. It must also allow the memory to be created in the first place (`W/runtime/vm/memory.rs:344-353`).
2. **Make each state, in plan order:**
   - `m2_alloc` one block for its settings, port table and port buffers: one call per state (design-scale's attack, A.8);
   - write the settings;
   - call `k_new`.
   - The host checks that every address it gets back lies inside the memory.
3. **Seal.** The limiter now refuses every memory growth and every table growth. wasmtime asks the limiter before it makes any system call (`W/runtime/vm/memory.rs:651-659`; for tables, `W/runtime/vm/table.rs:651-657`).
4. **Prewarm.** Run one `k_reset` per state, after the seal. A reset that would need memory fails here, at build, and the error names the node.
5. **Touch every page** of the memory once, with a volatile read and write-back of one byte. Then the system never has to stop the audio thread to supply a page: no first touch and no copy-on-write fault can happen there (external §A4).
6. **Keep the memory's base address**, as cut 1 does (`cut1.diff:53-60`). With `memory_may_move(false)` the memory never moves (`W/runtime/vm/memory.rs:663-664, 674-678`).

**Reset** happens in place, on the thread that holds the box. Its callers are:
- voice reuse;
- Panic;
- a box whose output went non-finite;
- an offline seek.

**Free:** a state is never freed alone. When a box retires, `m2-play-drop` drops its sandbox, and the memory goes back to the operating system whole (`W/runtime/store.rs:2425-2457`; `src/core/play/src/lib.rs:693-711`).

### 2.5 A live edit, step by step

| Step | Thread | What happens | Same as today? |
|---|---|---|---|
| 1. Prepare | actor | `keep.rs` decides what is kept, what is new, and what becomes a ghost or a fade (`src/core/runtime/src/keep.rs:166-189, 251-455`) | yes |
| 2. Birth | actor | each new box gets new sandboxes (§2.4). Kept and ghost slots are left empty (`src/core/runtime/src/lib.rs:1623-1627`) | yes, with "sandbox" in place of "instance" |
| 3. Send | actor to device | `Msg::Activate(Box<Renderer>)` goes on the bounded queue (`src/core/play/src/lib.rs:47-48`) | yes |
| 4. Activate | device, at a block line | check the plan's id and each box's shape; move each kept box by `mem::swap`; run `k_reset` on a box whose output went non-finite (`src/core/runtime/src/plan.rs:551-640`) | yes. A box that trapped is not reset (see below) |
| 5. Window, 10 ms | device and 8a's workers | ghosts play in their old sandboxes; fades are host code (RENDER-25a) | yes |
| 6. End | device | `end_fade` moves the ghosts to the leftovers (`src/core/runtime/src/plan.rs:642-669`) | yes |
| 7. Free | `m2-play-drop` | the old sandboxes drop, and each memory is unmapped whole | yes, with 200 unmaps instead of 6,058 |

- **The activation itself doesn't change.** Only the type of the box changes. A kept box's sandbox is neither copied nor rebuilt.
- **A top-level kernel box carries its own shape, state address and port addresses.** So the box is complete when it moves (design-scale's attack, A.2 #2).
- **A voice pool's sandbox lives inside the pool's box**, beside its voices.
- **On the audio thread there are only swaps, compares and kernel calls.**
  - A successful call allocates nothing (`W/runtime/func/typed.rs:154-224`).
  - A sealed memory can't grow.
  - The one exception is today's: a trap's error allocates (`src/core/runtime/src/engine.rs:573-574`).

**A trap (a crashed kernel):**
1. **The box is marked dead** with an atomic flag, as `PluginInstance::kill` does (`src/core/runtime/src/plugins.rs:104-113`).
   - It is never called again, and its outputs play silence.
   - The rest of the song plays on.
2. **The session is told, and the box is rebuilt.**
   - The session puts the box in its restarts, and the catch-up activation builds it new at once, off the audio thread.
   - This is HOST-5's path for a plugin that fails live (`src/docs/architecture.md:1156-1157`; `heard_plugins`, `src/crates/server/src/graph/actor.rs:1120-1142`).
   - A box that fails again within 10 s stays silent, and the user is told once.
3. **Offline**, a trap still fails the render and names the node.

**Why not reset a crashed box?** A trap leaves the kernel's hidden stack pointer wrong, and `k_reset` can't put it back (design-scale's attack, A.2 #5; design-safe's attack, fix A3).

### 2.6 What replaces the 32 MiB cap

| Limit | Checked | Value (a proposal; the owner decides) | Why |
|---|---|---|---|
| Memory one sandbox may take while it is born (the birth cap) | by the limiter, at birth | 256 MiB | std's largest state is one `delay` at 384 kHz: 6.1 MB in stereo (code-map §1.4). A 12-voice pool with a delay in every voice would need 12 × 6.1 = 73.7 MB |
| Growth after the seal | by the limiter | none at all | this keeps RENDER-28 by construction |
| Sandboxes per renderer | at build | about 4,000 | Two renderers must fit within 25%. On Linux that is 32,768 / 2 / 4.0625 = 4,033. The Mac measured 4.097 GiB per memory (49,637 / 12,116), so there it is 16,384 / 4.097 = 3,999 |
| A sandbox's hard limit | wasm32 itself | 4 GiB | https://webassembly.github.io/spec/core/valid/types.html |
| Assets (samples, impulse responses) | step 9's loader | their own budget | they sit outside every sandbox (open question 1) |
| The whole session's RAM | — | later (§7) | today's cap doesn't bound it either: 6,058 × 32 MiB is about 189 GiB |

### 2.7 What the user sees when a limit is hit

- Every limit is met while a plan is built, off the audio thread.
- The version before keeps playing, through today's path (`Way::Unbuilt`, `src/crates/server/src/graph/actor.rs:411-424`).
- The error becomes typed, so the server stops guessing from words. Today `out_of_memory` matches strings (`src/crates/server/src/graph.rs:382-387`).

| Case | When | What the user sees (proposed wording) |
|---|---|---|
| A part asks for too much memory | an open, an edit or an export | "Your last change is saved, but music2 couldn't get it ready to play, so the version before it keeps playing. The delay on 'bass 3' needs 300 MB of sound memory, and one sound part may use 256 MB." |
| Too many sandboxes | the same | "... This song needs 4,200 separate sound parts outside instruments, and music2 can play 4,000 at once. Remove some effects or tracks." |
| A part crashes live (it may have tried to take memory) | while playing | "The reverb on 'pad 2' stopped working, so music2 muted it and started it again." The rest keeps playing |
| It crashes again within 10 s | while playing | "The reverb on 'pad 2' stopped working again, so music2 left it silent. Open the song again to give it another try." This is today's plugin wording (`actor.rs:1142`) |
| A crash during an export | exporting | the export fails and names the part, as today |

Compare today: a refused memory grow becomes a crash whose message never says "memory". Live, it silences the whole song with no message at all (code-map §3.7).

---

## 3. Why it is simple

**Removed (10):**
- one Store, instance and memory per kernel instance (6,058 in p50);
- two memory layouts (P1 and P2), and the gate that compares them;
- the 32 MiB cap per kernel;
- explicit bounds checks, and the Spectre-guard exception;
- `static mut HOST`: one state per kernel type per instance;
- `k_port`, and the port-overlap check at build;
- the reset that briefly needs twice a delay's memory, and the reliance on the allocator reusing its freed block;
- memory growth at any time;
- one crash silencing the whole song;
- the speed plan's cuts 2 and 3 as separate decisions (this design includes both).

**Added (10):**

| Added | Needed by |
|---|---|
| the sandbox, owned by one box. This widens something step 7 already has rather than adding a new kind of thing | this layout |
| birth and seal, with the birth cap: memory grows only while a sandbox is being born | this layout |
| touching every page at birth | this layout |
| `m2_alloc`, and the kit's allocator that works only at birth | this layout |
| a sandbox count per renderer: the 25% rule turned into a check | this layout |
| the state handle: every call names its state | any ABI v1 |
| the settings block and the port table | any ABI v1 |
| the five kernel rules (ABI-5) | any ABI v1 with many states in a sandbox |
| the dead flag after a crash, and the rebuild, borrowed from plugins (HOST-5) | crash safety, in any layout |
| typed limit errors, and the notices of §2.7 | clear messages, in any layout |

- **Ten out, ten in.** Only five of the ten are this layout's own. The rest is ABI v1 work or crash safety, which any layout needs.
- **The live-edit machinery doesn't change.** `keep.rs`, `verify`, the swap, ghosts, fades and retiring stay as they are.
- **One unit does every job.** The box is what moves, what frees memory, what one thread enters, and what a crash mutes.
- **No simpler design was found that meets every goal.** Each of the layout's five pieces serves one goal:
  - the sandbox per box keeps live edits as they are;
  - birth and seal keep memory growth, and so system calls, off the audio thread;
  - the page touch keeps page faults off it;
  - `m2_alloc` keeps one allocator in each memory;
  - the count keeps the 25% rule.
  - The simpler rivals each fail a goal: one Store per pool with ABI v0 freezes one memory per state, and one sandbox per track makes live edits harder (§6).

---

## 4. What it fixes (estimates until measured)

| | Today (HEAD, P2) | Recommended | How the estimate is made |
|---|---|---|---|
| Sandboxes | 6,058 | 200 | a count (code-map §6.2) |
| Address space, two p50 renderers | 380 GiB, 0.29% | 1,625 GiB, 1.24% | 2 × 200 × 4.0625 GiB, out of 131,072 GiB |
| Bounds checks in kernel code | +9.6 M instructions a callback | none | `E/types.rs:2399-2407`; `results.md:109` |
| Spectre guard | off | on, at no cost | it is "a no-op for any heap that needs no bounds checks" (`CG/shared/settings.rs:290-302`) |
| Callback, Linux | S + 212 µs | **about S − 163 to S − 170 µs** (estimate) | see "The callback estimate" below |
| ... against HEAD | | **about 375 to 382 µs less** (about 5%) | 212 + 163 = 375; 212 + 170 = 382 (medians from campaign E). With the base's own range (−219 to −170) it is about 360 to 420. design-scale's attack adds parts from other runs and gets 365 to 507 (its A.8) |
| ... against cut 1, which lands first | | **roughly 190 to 210 µs less** (about 3%) | cut 1 measured S + 31 µs (big-cache half) and S + 41 µs (small-cache half) in G7f's gate on 2026-10-16 (`$SCRATCH/speed/build/logs/gates-A-arena-*.log`): 31 + 163 = 194, 41 + 170 = 211. Rough: the gate and campaign E are different runs and methods |
| Callback, Mac | O1's bounds checks cost +141 µs there (`plan.md:84`) | that part goes | the total is unmeasured; the Mac must measure it |
| Footprint, one renderer | 195 MB on Linux; 575 MB on the Mac, measured under P1, since the Mac hasn't run P2 yet (`src/docs/architecture.md:330`) | **about 45 to 62 MB on Linux, and 45 to 69 MB on the Mac** (estimate), plus parts nobody has split out | see "The footprint sum" below |
| Build of p50 | 216 ms (S 181) (`plan.md:54`) | unknown; plausibly under 199 ms | All cuts together (200 Stores, still 6,058 instances) built in 199 ms against 220 (`results.md:172`). Here there are only 200 instances. Not shown |
| Drop of a renderer | 84.6 ms (`results.md:174`) | expected lower | 200 unmaps instead of 6,058; unmeasured |
| A crash, live | the whole song goes silent, the transport stops, and there is no message | one box is muted and rebuilt, and the user is told | `src/core/runtime/src/engine.rs:570-579` |
| Memory growth on the audio thread | possible (a reset that grows) | impossible after the seal | §2.4 |

**The callback estimate:**
- The nearest measured build is "all cuts on P1": S − 184 µs (−219 to −170) (`results.md:52`).
- Like this design, that build has a 4 GiB layout, kept addresses, one Store per pool, and no heap pointer (`Box`) per kernel instance (cut 2).
- Unlike this design, it still has 6,058 instances and memories, and v0's call shape. §10 measures the difference (#1 and #6).
- It also has cut 4 (−14 to −21 µs, `results.md:46, 145`), which this design doesn't make. Adding that back gives −184 + 14 = −170 and −184 + 21 = −163. This mixes layouts: −14 is cut 4 on P2, and −21 on P1 (`results.md:145-146`). The base is P1, so −21 is the matching figure, and its own range (−82 to +12) crosses zero.

**The footprint sum** (Linux; the Mac adds rounding to its 16 KiB pages):
- Each sandbox's module part, touched whole, is 2 wasm pages = 131,072 B (code-map §1.3). Times 200, that is 26.2 MB.
- Delay lines and reverbs at 48 kHz: 8.3 to 14.8 MB (design-simple §11; checked in its attack, N9).
- Port buffers inside the sandboxes: 19,916 ports (design-safe §1) × 512 B (mono) to 1,024 B (stereo) = 10.2 to 20.4 MB.
- Small states: about 0.6 MB. That is 6,058 × about 100 B, an assumption from design-scale §11.2.
- Total, low end: 26.2 + 8.3 + 10.2 + 0.6 = **45.3 MB**.
- Total, high end: 26.2 + 14.8 + 20.4 + 0.6 = **62.0 MB**.
- The Mac adds at most 200 × 2 × 16 KiB = 6.6 MB, so its high end is **68.6 MB**.
- **Not counted:** the host's own buffers (they stay) and wasmtime's own structures. Today's 195 MB includes both, and no one has split them out. So don't read a ratio from these numbers.

---

## 5. What it costs to build, and what stays the same

| # | Slice | Done when | Size (rough estimate) |
|---|---|---|---|
| 0 | A throwaway spike that measures the list in §10 | it gives a go or no-go against §4 | 2 to 3 days |
| 1 | Kit v1 and std rebuilt; `verify_module` for v1; the shapes of ABI-2's reservations fixed | std's tests pass; the 8 recorded renders and the p16 render are the same files | about 1.5 weeks |
| 2 | Sandboxes in the runtime, as one sandbox layer the plan calls (§11): top-level and pool boxes; birth, seal and page touch; wasmtime's default layout with `memory_may_move(false)` and the Spectre guard on; P1 and P2 removed | the 8 renders, p16, G7b (tails), G7c (live equals export), `no_alloc` and `churn` pass | about 1 to 2 weeks |
| 3 | Failure and limits: the dead flag, HOST-5's restart path for kernel boxes, the notices, the birth cap, the sandbox count, typed errors | a crashing test kernel mutes only its own box, and the rest is bit-identical | 3 to 5 days |
| 4 | Gates and rules: G7f with one layout, against S; the rule texts (§8); a Mac run | G7f's bounds hold on the Mac | about 1 week |

- **Total:** about 4.5 to 6 weeks (0.4 to 0.6 + 1.5 + 1 to 2 + 0.6 to 1 + 1). The slice sizes come from design-scale §12.1, without its in-place slice.
- **ABI-2's reservations** (events, assets, save and load, latency and tail, offline kernels) get their shapes in slice 1. They are built in step 9 with the kernels that use them, starting with the sampler.

**What stays the same:**
- `keep.rs`, and how `verify`, the swap, `end_fade` and retiring work (`src/core/runtime/src/plan.rs:551-712`).
- Ghosts and fades (RENDER-25a), the shape hash (RENDER-25) and cells (RENDER-22).
- Voice allocation, voice stealing and captures (`src/core/runtime/src/lib.rs:1364-1477`).
- Plugins: all of the HOST rules.
- Seeks, loops and Panic (RENDER-9, RENDER-10).
- Exports, with their two renderers.
- 8a's partitions and its precondition.
- The kernels' DSP arithmetic, and so the sound, bit for bit.

---

## 6. The other designs, and why not

How the scores work:
- Each design was scored after its attack's fixes were applied.
- Scores run from 1 (poor) to 5 (best).
- Simplicity counts three times, because it is the owner's priority. So the best possible total is 40.

| Design | Simple (× 3) | Live edits, real time | Scale | Fit with 8a, ABI v1 | Isolation, third parties | Cost | Total |
|---|---|---|---|---|---|---|---|
| **design-safe**: one sandbox per box; only the host allocates; assets read through an import | 4 | 5 | 3 | 4 | 5 | 3 | **32** |
| **design-scale**: one sandbox per box; kernels allocate only at birth | 4 | 4 | 3 | 4 | 3 | 4 | **30** |
| **design-simple**: one sandbox per track and package | 2 | 2 | 5 | 3 | 2 | 2 | **20** |

How the totals add up:
- safe: 4 × 3 + 5 + 3 + 4 + 5 + 3 = 32;
- scale: 4 × 3 + 4 + 3 + 4 + 3 + 4 = 30;
- simple: 2 × 3 + 2 + 5 + 3 + 2 + 2 = 20.

**Why design-safe scored so:**
- *Simple, 4:* one unit for everything, and "only the host allocates" is one clear rule. But it needs a measuring pass in `new`, slabs instead of `Vec`, two engines, and a relocation rule (best dropped).
- *Live edits, 5:* step 7's arena untouched; nothing grows, allocates or calls the OS live. Its fixes are small: a crashed box is replaced, the limiter allows creation, tables are sealed.
- *Scale, 3:* about 4,000 sandboxes per renderer, counted by top-level kernels.
- *Fit, 4:* it fits ABI-2's whole list, but needs fixes A1, A2 and A4 before the freeze.
- *Isolation, 5:* packages never share a memory; assets sit outside every memory; deadlines for third-party code; the host trusts nothing it reads back.
- *Cost, 3:* std's heap kernels move to slabs; it adds two engines and an import.

**Why design-scale scored so:**
- *Simple, 4:* the same unit and live edits. But it has a three-mode allocator, status codes, and in-place wires (its attack deferred them).
- *Live edits, 4:* the arena is unchanged and memory is sealed. But its reset, as written, was undefined behaviour (fixed).
- *Scale, 3:* the same ceiling.
- *Fit, 4:* no 8a change. Its settings block needed a length prefix, and copying assets into each sandbox is weak for large sample banks.
- *Isolation, 3:* the box limits the harm, and there is a birth cap. But there is no plan for runaway kernels until later.
- *Cost, 4:* about 5 to 6 weeks, with std's kernel code unchanged.

**Why design-simple scored so:**
- *Simple, 2:* a neat picture. But live edits gain a second keep test, a copy on the audio thread, and rules for creeping offsets. Its attack's fix F4 turns it into a hybrid of two units.
- *Live edits, 2:* after its fixes it still copies kept states on the audio thread at each edit of a track. Only fix F4 keeps that copy small; without it a 115 MB looper would need 76.8 GB/s. Before its fixes it had three holes in the host's own memory safety.
- *Scale, 5:* 51 sandboxes; the ceiling counts tracks, not effects.
- *Fit, 3:* the ABI shape is good, but 8a needs an added edge and copies out of the producer (F5).
- *Isolation, 2:* a bug spoils its package's states across the whole track, and the layout depends on edit history.
- *Cost, 2:* the largest; new live-edit machinery on top of the runtime rewrite.

**Why one per box, and not one per track (design-simple):**

| | One per track and package (design-simple, with its fixes) | One per box (this report) |
|---|---|---|
| Sandboxes at p50 | 51 | 200 |
| What a live edit adds | a "touched" test per sandbox on top of `keep.rs`; a copy of kept states on the audio thread; copies into ghosts' old buffers; a rule for creeping offsets; an added 8a edge. Its attack counts 12 new ideas, not the 4 it lists (S1) | nothing: today's swap, unchanged |
| A large state, such as a long delay or a looper | needs a sandbox of its own (fix F4), or its copy at each edit outgrows the activation block (L4). So the design becomes a mix of two units | moves by swap, whatever its size |
| The ceiling | counts tracks: far away | counts top-level kernels: about 4,000 (§8) |
| Time saved, by design-simple's own model | 384 µs | 367 µs (its attack, S2). Both figures read wires in place, which this report leaves for later |

- design-simple's own attack calls one per box "the strongest rival on simplicity" (S2).
- Its fix F4 already moves it part of the way to one per box.
- Per track's one real gain is the ceiling. §7 lists three ways to lift that later (the `sum` kernel, the checked engine, grouping). None of them changes the ABI, so nothing about it must be decided now.

**What the recommendation takes from each design:**
- **From design-safe (the base):**
  - the unit and its life cycle;
  - a crashed box is replaced, like a plugin;
  - per-channel port addresses;
  - reset puts the state back without allocating. That is its fix A1, which follows Faust's split of constants from running state (https://faustdoc.grame.fr/manual/compiler/);
  - typed limit errors;
  - assets outside every memory.
- **From design-scale:**
  - Kernels take memory with ordinary code while they are born: `Vec` in Rust, `malloc` in C. Every audio API asks for this. CLAP's `activate` is where "the plugin may allocate memory" (https://github.com/free-audio/clap/blob/main/include/clap/plugin.h). The seal enforces it, and it removes design-safe's measuring pass and slabs.
  - The settings block with a length prefix.
  - The sandbox count per renderer.
- **From design-simple:** nothing in the core. Its unit scales best, so it stays an option for later. But its live edits are the hardest part of all three designs.
- **Left out on purpose:**
  - in-place wires (§7);
  - two engines with deadlines (§7);
  - the relocation rule, which design-safe's own attack advised dropping (its §3).

**Units rejected outright:**

| Unit | Why not |
|---|---|
| one per kernel instance (today) | 6,058 memories can't take the 4 GiB layout. So the bounds checks stay, and the Spectre guard can't come back cheaply (code-map §2.3) |
| one Store per voice pool, with ABI v0 (the speed plan's cut 3) | it is a speed cut, not a layout. It keeps 6,058 memories, the bounds checks and v0 (`plan.md:12`) |
| one Store per track, with ABI v0 | a replaced instance's memory stays until the track's Store drops (`W/runtime/store.rs:139-145`). That goes against RENDER-32's reason |
| one Store per renderer | it frees nothing until it drops. And with one thread at a time there is no 8a (`W/runtime/func/typed.rs:100-105`) |
| one shared memory across Stores | it is a tier-2 wasmtime feature with no security fixes: "security CVEs will not be issued" (`W/config.rs:3316-3335`). It also needs a nightly Rust std built with atomics (external §C4) |
| one module per track, with its kernels compiled together (static linking) | Every edit compiles the track again, and the module's hash changes. So every kernel in the track starts from init and its tails are cut (RENDER-25; the shape holds the module's hash, `src/core/runtime/src/plan.rs:35-39`), unless states are copied across modules. It would also put two packages in one memory, which ends the wall between them. It needs a merging tool (Binaryen's wasm-merge) that isn't installed here, and how long a compile takes is unmeasured (external §C4; code-map §2.2). Its gain, direct calls between kernels, mostly comes from the runner inside the sandbox (§7), with no compile at each edit |
| one instance with a memory per kernel (multi-memory) | It shares nothing new: every instance already shares its compiled code through the one `Module` (code-map §2.2). The cost is the memories, and each one still reserves its own 4 GiB (external §A3), so 6,058 memories fail the 25% rule as today. Also, wasm code names its memory by a fixed number in each load and store (one heap per memory index, external §A3), so one copy of a kernel's code can't serve many memories |

---

## 7. Later: left out on purpose

None of these changes the ABI. Each one waits for its trigger.

| Later | Kind | What it might give (estimate) | Trigger |
|---|---|---|---|
| **In-place wires inside a voice pool**: a kernel reads another kernel's output where it is, with no copy | speed | Up to about 300 µs a callback on Linux (design-scale §11.1: 274 to 325; design-safe §11: 297 to 356). That is a guess at the most. Copy time follows cache misses, not the number of copies: it went 523 → 408 → 393 µs with the same copies (`results.md:92`). Both estimates start from 393 µs, a P2 column. At HEAD the same copies took 475 µs under P1 and 408 under P2, and this layout is like P1, so the figure can move by tens of µs either way | the Mac needs the time |
| **One call per voice segment**: a small runner inside the sandbox calls the voice's kernels | speed | About 140 µs. With cuts 1, 3 and 4 on P2 (the profile has no column with all four), wasmtime's host code is 167 µs, × (1 − 325 / 2,312) (design-safe §12 and its attack §8). It could reach about 750 µs if m2-runtime's per-call work goes too (design-scale L2; unverified) | after in-place wires |
| **A `sum` kernel with many inputs in std**, for the mixer | scale | the song's 49 `add` kernels become 8 (design-safe's attack, §5) | p50 × 10, or the serial tail in 8a |
| **Getting past the ceiling**: std's small top-level kernels on a second, bounds-checked engine | scale | It lifts the limit of about 4,000, so the limit counts instruments, not effects. It costs about +6 to +7 µs if the cost follows calls (150 / 2,245 × 85 to 105). At most it costs O1's +85 to +105 µs on Linux (design-safe's attack, §5) | a real song passes half the ceiling |
| **Grouping a track's small top-level kernels**, with generations and compaction | scale | fewer sandboxes; it needs a relocation rule | only if the line above isn't enough |
| **Deadlines (wasmtime epochs) and a guarded engine for other people's kernels**, at build time too | safety | stops a kernel that never returns (`W/config.rs:744-762`); its cost is unmeasured | before the first third-party package |
| **Assets shared as read-only pages** instead of copies | memory | one copy of a sample bank for every sandbox. Whether wasmtime accepts this is unverified | large sample banks |
| **Voices across threads**: a big pool split into chunk sandboxes | speed | follows 8a's own trigger (`src/docs/designs/step8/design-8a.md:699-716`) | a pool on 8a's critical path |
| **The pooling allocator, a compile cache, and copy-on-write on the Mac** (`Module::deserialize_file`, `W/config.rs:2273-2276`) | speed | faster builds and opens; unmeasured | slow builds |
| **A smaller std stack** than 64 KiB | memory | at most 200 × 64 KiB = 13.1 MB at p50 | footprint |
| **A RAM budget for the session**, and locking touched pages in RAM | safety | — | when users hit it |
| **Loading a state into a kept kernel; saving a live state** | feature | — | the first kernel that needs it |

---

## 8. Risks, open questions, and the rules it changes

**Risks:**

| Risk | How bad | What covers it |
|---|---|---|
| The ceiling of about 4,000 sandboxes counts top-level kernels. A mix with 8 effects a track fits about 366 tracks (4,033 / 11). A song as a program can make thousands: a 512-partial additive sound at the top level is 1,535 boxes, and three such tracks pass the ceiling | medium | a clear message at build; the `sum` kernel and the checked engine (§7) |
| Isolation inside an instrument gets coarser: a bug in one std kernel can spoil the other kernels in its pool | low | std is safe Rust. faustwasm also gives a polyphonic node's voices one memory (https://github.com/grame-cncm/faustwasm/blob/master/src/FaustWasmInstantiator.ts) |
| A crash still costs one dropout, and now the song keeps playing, so the dropout is heard. On every system, a trap takes the read side of a global lock (`W/runtime/vm/traphandlers.rs:994`; `W/runtime/module/registry.rs:401-415`), whose write side wasmtime takes briefly as it loads or drops a package's code (`:425-445`). On the Mac, a trap also waits for wasmtime's normal-priority helper thread (`refresh.md:192`) | low | set `wasm_backtrace_max_frames(None)` for live play (`W/config.rs:585-600`); the 10-second rule |
| Kernel authors must allocate only in `new` and keep no per-state statics. Languages with one garbage-collected heap per instance (AssemblyScript, TinyGo) can't hold many states | medium, for third parties | open question 3 |
| The footprint split and the build time are unmeasured | low | the spike (§10) |
| On the Mac, each memory takes 35 MiB more address space than computed (49,637 against 49,221 GiB; code-map §2.3). The cause is unknown | low | the ceiling is 3,999, not 4,033; the spike measures it |
| Pages touched at birth can still be swapped out or compressed later | low | the same as today; locking them in RAM is under Later |
| The kit's allocator may not link on Rust 1.99.0 for wasm32 | low | unverified; slice 1 finds out |

**Open questions for the owner:**
1. **Assets: an import, or a copy?**
   - With one import, `m2.asset_read(handle, offset, dst, len)`, assets live once, outside every memory (design-safe §7.2).
   - Or the host copies them into each sandbox at birth (design-scale §7).
   - The import scales to multi-gigabyte sample banks and to streaming. Copying is simpler, but costs RAM and counts against each sandbox's 4 GiB.
   - *Recommendation:* the import. Decide when the sampler is designed in step 9, before the freeze. The spike measures what one call costs.
2. **The numbers:**
   - the birth cap (256 MiB proposed);
   - the sandbox limit (about 4,000 per renderer).
   - The two attacks disagree on whether to count per renderer or per process. Per renderer is predictable: a song either fits or it doesn't, every time (design-scale's attack, A.5 #2). But a rebuild during an export can briefly hold a third set (design-safe's attack, A5).
3. **Other languages:** are garbage-collected languages out of scope for v1? Or may a package say "one state per sandbox" and get that? That would be a host choice (design-simple's attack, I5).
4. **Before the freeze:**
   - which thread runs `k_save` and `k_load`;
   - whether latency and tail may depend only on the settings;
   - offline kernels working in chunks. A 32-bit memory holds about 93 minutes of stereo float in and out at 48 kHz (2^32 / 16 B a frame; design-safe's attack, §3).

**Rules it changes** (in `src/docs/architecture.md`):

| Rule | Change |
|---|---|
| **RENDER-32** (`:328`) | rewritten: "Sandbox layout: kernels run in sandboxes. A sandbox is one wasmtime `Store` with one instance of one package's module and its one memory, and it holds the kernel states of exactly one box: a top-level kernel, or a voice pool's kernels from one package. It is born off the audio thread and sealed before it plays; after that its memory and tables never grow, and it never moves. It moves whole between plans, is entered by one thread at a time, and is dropped whole off the audio thread, which frees its memory. Never a sandbox shared by two boxes or across plans. Memories take wasmtime's default reservation, so the hardware catches stray accesses, no bounds check is compiled in, and the Spectre guard is on. A renderer holds at most N sandboxes, so that two renderers stay within 25% of the address space; a build that would pass N fails and says so. G7f's bounds stay, judged against S." Two conflicts to settle with it. (1) The 32 MiB layout and its bounds are the owner's own decision (2026-10-15, `:328`), so going back to 4 GiB needs the owner's sign-off. (2) G7f today times the app against P1 in one process and adds P1's cost recorded on the Mac (`crates/gates/src/arena.rs:5-19`, `callback-S.txt`, `store-S.txt`). P1 goes, so G7f must time S in the same run, as the fresh gate on branch s7-sg does (`plan.md:42`) |
| **RENDER-25** (`:253`) | a kernel's box is its sandbox. The settings include channels per port, asset hashes and a saved state's hash. "Or that trapped" goes: a crashed box is replaced, not reset |
| **RENDER-10** (`:158`) | "or that trapped" goes |
| **RENDER-16** (`:198`) | a pool's kernels share one sandbox per package |
| **RENDER-28** (`:285`) | its *Today* line adds: a kernel memory grows only while its sandbox is being born. It also names the one exception, which is no longer followed by silence: a trap allocates its error (`src/core/runtime/src/engine.rs:573-574`) and takes the lock in §8's risks, on the thread that trapped |
| **ABI-1** (`:1166`) | the host places every port buffer, so ports can't overlap. The overlap check at build goes; the debug input check stays |
| **ABI-2, ABI-3** (`:1170-1180`) | v1 is §2.3; `m2_abi()` returns 1; v0 is refused |
| **ABI-5** (new) | the five kernel rules of §2.3 |
| **HOST-5** (`:1156`) | kernel boxes share its failure path |
| **Deferred** (`:1232`) | "Spectre guard back on, or another Store layout measured" is done |
| **design-7 §11** | superseded. Its reason still holds: never a Store shared across plans |
| **design-8a** | the precondition's words become "one Store per box" (`design-8a.md:100`). §4.3, live: a failed box is muted and its partition goes on. Offline is unchanged (`design-8a.md:363-372`) |

---

## 9. When to decide, and how it fits 8a

- **Decide before kernel ABI v1 freezes in step 9.**
  - ABI v1 is when the format freezes for other people's code.
  - This layout *is* the core of v1: state handles, settings and ports.
  - A layout change is cheap before the freeze and costly after it.
- **8a doesn't depend on it.**
  - 8a needs "one thread enters a Store at a time" (`src/docs/designs/step8/design-8a.md:98-101`).
  - That holds today, and it holds here, because a box is in one partition (`refresh.md:188`).
  - The precondition's words change, and so does §4.3 live: a failed box is muted and its partition goes on (§8's table).
- **What changes for 8a:**
  - A task enters fewer Stores: 3 per bass track, 1 per pluck track, 2 per pad track and 100 for the song (design-scale §6).
  - Retiring makes 200 unmaps instead of 6,058. That eases 8a's risk R15, unmaps racing real-time threads (`refresh.md:219`).
  - Workers still call `tls_eager_initialize` first (`W/engine.rs:782-799`).
- **Suggested order:**
  1. Cut 1 carries the Mac for now (`plan.md:9-11`). Cut 2 was dropped on 2026-10-16: on the small-cache half of the chip it saved nothing.
  2. Build 8a as designed.
  3. Then, in step 9 and before the sampler, run this layout's spike, then its slices.
- **Don't build cut 3** (one Store per pool) unless the Mac misses its bound before step 9. This design replaces it.

---

## 10. What a later prototype must measure first

This is a throwaway spike: the kit's v1 macro, and one pool sandbox for p50's voice kernel types. Nothing from it is merged.

| # | Measure | Why | Go if |
|---|---|---|---|
| 1 | p50's callback against S, HEAD, and HEAD with cut 1: the Linux lockstep, and one Mac run | it tests §4's estimate, and the gain over cut 1, which lands first | near "all cuts on P1" (S − 184) plus cut 4's 14 to 21 µs; G7f's bound met on the Mac |
| 2 | The build of p50, and how much of S's 181 ms is instantiation | §4 says only "plausibly under 199 ms" | within S's bound (a build at most half again longer than S's) |
| 3 | The footprint of one renderer, and what today's 32 KB per instance is made of | it tests §4's 45 to 69 MB estimate | under S + 64 MB (RENDER-32's bound) |
| 4 | The address space of one sandbox on the Mac | it sets the real ceiling (3,999 or 4,033) | — |
| 5 | The deepest stack `k_process` reaches | is touching the whole 64 KiB stack needed, and is 64 KiB the right size? | — |
| 6 | JIT instructions per call with a state handle and a port table, against v0 | the new call shape's own cost | no rise worth noting |
| 7 | The slowest activation block | it must stay under 5.3 ms. Today it is 3.0 to 3.8 ms on the Mac (`src/docs/architecture.md:330`) | under 5.3 ms |
| 8 | The drop of a renderer | today it is 84.6 ms (`results.md:174`) | not worse |
| 9 | The cost of one `asset_read` call, if chosen | open question 1 | small next to a 128-frame block |
| 10 | A crash's dropout on the Mac | the risk in §8 | — |
| 11 | The same bits: the 8 renders, the p16 hash, live equals export | the sound must not change | identical |
| 12 | That the kit's allocator links on Rust 1.99.0 for wasm32 | unverified | it builds |

---

## 11. The layers: what freezes, and what can change later

Added by the lead. The owner asked: "all those implementation details don't affect what the program does or whether it is right. Can we hide them, so we can optimize later?"

**In short:** yes. Two contracts must be right: kernel ABI v1 and the box. Everything behind them can change at any time. The same tests check each change.

```
 song code (song.ts)                          <- the owner writes this
   |
 the plan: order, live edits, voices, 8a      <- sees only boxes and their ports
   |   make a box . ports . process . reset . hand over . drop
 the sandbox layer (one module in core/runtime)   <- hidden: Stores, memories, addresses, reservations
   |   kernel ABI v1 (freezes in step 9)
 kernel code (std; later, other packages)     <- a plain Rust struct; the kit hides the rest
```

| What is fixed | Fixed when | What it promises |
|---|---|---|
| Kernel ABI v1 and its five rules (§2.3) | at the freeze, in step 9 | A kernel never assumes how many states share its sandbox. Every call names its state. No state lives in a static. Memory is taken only at birth. |
| The box, on the engine's side | already, since step 7 (`src/core/runtime/src/plan.rs:690-712`) | A box is what moves, what frees memory, what one thread enters, and what a crash mutes. The plan never reaches into a Store or a memory itself. Today it still does in a few places: `KernelNode::process`, `prewarm`, `kernel_memory` (`src/core/runtime/src/lib.rs`). Slice 2 (§5) closes them. |
| The tests | already | Live equals export bit for bit (RENDER-33, G7c), the 8 renders and the p16 render, nothing allocated on the audio thread (`no_alloc`), `churn`, untouched tails (G7b). None of them knows the layout. They are what checked cut 1. |

**What can change later, behind the contracts, without touching kernels or the plan:**
- the reservation and guard sizes;
- in-place wires and a runner inside the sandbox (§7);
- the pooling allocator and a compile cache (§7).

**What can't be hidden:**
- Speed and memory use. They are the reason to change a layout, and the gates measure them.
- The thread rule: a sandbox is entered by one thread at a time, so a sandbox never spans two 8a partitions.
- How many boxes share a sandbox. Grouping later (for example a track's small effects, §7) is possible, but a crash spoils everything in its sandbox, so one crash would then mute the whole group, and the user would hear it. A sandbox for a whole track would also need new live-edit machinery and a change to 8a, because a track can span two 8a partitions (`src/docs/designs/step8/design-8a.md:199-206`).

**Keep it simple:** one interface and one layout at a time. No framework for choosing layouts. To optimize later, rewrite the sandbox layer and run the same tests.

**`k_drop`: considered, and best left out** (the owner decides):
- The idea: reserve `k_drop(state)` in ABI v1, so that a later layout could free one state inside a sandbox that several boxes share.
- Why leave it out:
  - No grouped layout in the notes needs it. design-simple and design-scale's grouping (L1) both build a new sandbox and drop the old one whole (design-simple's summary, point 7; design-scale §12.2).
  - Freed room helps only if a new state is made in it later. That means `k_new` after the seal, in a sandbox the audio thread holds. v1 forbids that (§2.3), and design-safe §0 shows that each way to do it breaks a rule. So `k_drop` alone opens no door.
  - The kit's allocator would have to free. A grouped layout would then rely on it reusing freed blocks: the reliance §3 removes.
  - An optional export can still be added in a later v1 without breaking any v1 package. A host would group only the packages that export it. The others stay one sandbox per box, as here.
- A cheaper way to keep that door open: a rule that everything one `k_new` takes lies in one run of bytes, and no static points into it (rule 5). The kit's forward-only allocator gives that already, so it costs Rust kernels nothing. A later layout could then copy or reuse a state's bytes without calling the kernel. For C kernels it rules out `malloc` reusing freed holes between `k_new` calls. The owner decides.
- If the owner still wants `k_drop` marked, reserve only the name, and keep the kit's allocator as it is.

---

## 12. Sources

**Notes this report judges** (in this folder):
- `code-map.md`: how step 7 runs kernels. It has the p50 counts (§6.2), the address space (§2.3), and a refused memory grow followed through the code (§3.7).
- `external.md`: wasmtime facts (§A), prior art (§B), the toolchain (§C), and derived numbers (§D).
- `design-simple.md`, `design-scale.md` and `design-safe.md`, each with its attack at the end.

**Measurements and plans:**
- `results.md` (the speed work's profile):
  - lockstep campaigns (`:33-52`);
  - call counts (`:62-63`);
  - self time (`:84-93`);
  - instructions (`:106-111`);
  - G7f with all cuts (`:166-174`).
- `plan.md` (the speed plan, with its attack):
  - the cuts (`:7-15`);
  - O1's cost (`:40, 84`);
  - the build (`:54`).
- `refresh.md` (step 8a's plan checked against step 7): 8a against step 7 (`:52, 186-190, 219`).
- `cut1.diff`: cut 1's kept address (`:53-60`), as it was reviewed. It landed as `444fc66`.

**Code** (at `ed7fc8e`):
- `docs/architecture.md`:
  - RENDER-10 at `:158`, RENDER-16 at `:198`, RENDER-25 at `:253`, RENDER-25a at `:258`;
  - RENDER-28 at `:285`, RENDER-32 at `:328-330`;
  - HOST-5 at `:1156-1157`, ABI-1 to ABI-3 at `:1166-1181`, Deferred at `:1231-1232`.
- `docs/designs/step8/design-8a.md`: `:98-101`, `:199-206`, `:363-372`, `:699-716`.
- `std/kernels-src/kernel/src/lib.rs`: the trait (`:49-64`), reset (`:156-162`), `static mut HOST` (`:181`).
- `std/kernels-src/src/lib.rs`: `gain` (`:353-388`), `delay` (`:483-495`).
- `core/runtime/src/`:
  - `lib.rs` (`:353-355`, `:1364-1477`, `:1623-1627`);
  - `plan.rs` (`:551-712`);
  - `keep.rs` (`:166-189, 251-455`);
  - `engine.rs` (`:570-579`);
  - `plugins.rs` (`:104-113`);
  - `packages.rs` (`:120-121`).
- `core/play/src/lib.rs`: `:47-48`, `:693-711`.
- `crates/server/src/graph/actor.rs`: `:411-424`, `:1120-1142`.
- `crates/server/src/graph.rs`: `:382-387`.
- `crates/gates/src/arena.rs`: `:5-19` (G7f pairs the app with P1).

**wasmtime 49.0.2** (under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`):
- `wasmtime-49.0.2/src/runtime/store.rs:139-145, 2425-2457`: things are freed only when the Store drops.
- `wasmtime-49.0.2/src/runtime/vm/memory.rs:344-353, 651-659, 663-664, 674-678`: the limiter at creation and at growth; a memory never moves.
- `wasmtime-49.0.2/src/runtime/vm/table.rs:651-657`: table growth.
- `wasmtime-49.0.2/src/runtime/vm/memory/mmap.rs:62-69`: a memory's address space.
- `wasmtime-49.0.2/src/runtime/limits.rs:32-74`: the limiter.
- `wasmtime-49.0.2/src/runtime/func/typed.rs:100-105, 154-224`: a call needs `&mut Store`, and allocates nothing.
- `wasmtime-49.0.2/src/runtime/module/registry.rs:401-415, 425-445`: the lock a trap takes, and where its write side is taken.
- `wasmtime-49.0.2/src/runtime/vm/traphandlers.rs:994`: every trap looks up its code there.
- `wasmtime-49.0.2/src/config.rs:585-600, 744-762, 2273-2276, 3316-3335`: backtraces, epochs, copy-on-write on macOS, and shared memories.
- `wasmtime-49.0.2/src/engine.rs:782-799`: `tls_eager_initialize`.
- `wasmtime-environ-49.0.2/src/types.rs:2399-2407`: when bounds checks are left out.
- `cranelift-codegen-meta-0.136.2/src/shared/settings.rs:290-302`: the Spectre guard costs nothing where there are no bounds checks.

**Web:**
- WebAssembly memory limits: https://webassembly.github.io/spec/core/valid/types.html
- Faust's compiler (constants kept apart from clearing): https://faustdoc.grame.fr/manual/compiler/
- faustwasm, with all voices in one memory: https://github.com/grame-cncm/faustwasm/blob/master/src/FaustWasmInstantiator.ts
- CLAP, where a plugin may allocate: https://github.com/free-audio/clap/blob/main/include/clap/plugin.h

**Unverified, carried over from the notes:**
- the M1 Pro's copy speed;
- how today's per-instance footprint splits;
- how much of S's build is instantiation;
- whether wasmtime can map shared read-only asset pages;
- that the kit's allocator links on Rust 1.99.0;
- how long a trap takes on the Mac;
- the Mac's extra 35 MiB per memory.