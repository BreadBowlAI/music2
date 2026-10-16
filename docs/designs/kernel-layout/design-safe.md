# design-safe: one sandbox per box

Label: design-safe. Research only: nothing was built, run or measured for this note. Brief: the layout that best
serves live edits (tails intact, nothing allocated and no syscall on the audio thread), third-party packages after
ABI v1 (isolation between packages, asset data outside every memory cap) and determinism (the same bits live and in
export, on any thread count).

How to read the citations (the same as `code-map.md` and `external.md`):
- a bare path (`core/runtime/src/lib.rs:329`) is under `/home/jerry/m2-runs/idea3/src`, step 7 frozen at `ed7fc8e`;
- `cut1.diff:N` is a line of `/home/jerry/m2-runs/idea3/cut1.diff`;
- `W/` = `wasmtime-49.0.2/src`, `E/` = `wasmtime-environ-49.0.2/src`, `CL/` = `wasmtime-internal-cranelift-49.0.2/src`,
  `CG/` = `cranelift-codegen-meta-0.136.2/src`, all under `/home/jerry/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`;
- `results.md` = `/home/jerry/m2-runs/speed/prof/results.md`, `plan.md` = `/home/jerry/m2-runs/speed/plan.md`,
  `refresh.md` = `/home/jerry/m2-runs/s8a/refresh.md`; `code-map §n` and `external §n` are the notes in this folder;
- **estimate** = arithmetic shown, from measured numbers or counts from the code; **unverified** = not checked here;
  **proposal** = a value or wording this design suggests, for the owner to decide.

---

## The design in one table

| Question | Answer |
|---|---|
| **Idea 3** | Keep its ABI and its memory layout, change its unit. Adopted: many kernel states in one instance, placed by the host as slices of one memory; a 4 GiB reservation, so no bounds checks; sound passed between kernels in one memory without copies; never two packages in one memory. Changed: the sandbox is per **box** (a voice pool, or one top-level kernel node), not per track. Why: §0 |
| **The unit** | A *sandbox* is one wasmtime `Store`, holding one instance of one package's module and that instance's one memory. It holds the states and wires of exactly one *box*: the thing step 7 already keeps, moves and retires whole (`core/runtime/src/plan.rs:690-712`). A pool whose voices use two packages has two sandboxes |
| **p50** | 200 sandboxes: 50 voice pools + 150 top-level kernels. So 200 Stores, 200 instances, 200 memories (today 6,058 of each, code-map §6.2), holding 6,058 state blocks |
| **Memory** | wasmtime's 64-bit defaults: 4 GiB reservation, 32 MiB guard (and 32 MiB before), so bounds checks are elided; plus `memory_may_move(false)`. Only the host grows a memory, only while it builds the box (twice: once for the config blocks, once to the final size); after that it never grows and never moves. Spectre guard back on (a no-op on elided checks) |
| **Kernel ABI v1** | Per kernel type `k`: `k_ports() -> i32`, `k_size(cfg) -> i32`, `k_init(state, cfg)`, `k_reset(state)`, `k_process(state, io, from, to)`, `k_active(state) -> i32`; optional `k_save`, `k_load`, `k_latency`, `k_tail`. One import: `m2.asset_read`. No destructor: a kernel owns nothing but the bytes of its state block |
| **Who allocates** | Only the host, only while it builds a box, never on the audio thread. A kernel can't grow memory: the Store's limiter refuses any growth the host didn't start |
| **The limit** | A state budget per box and per renderer, checked from the declared sizes before any kernel runs (replaces the 32 MiB cap per kernel). Assets live outside every memory and outside every budget but their own |
| **A live edit** | Exactly step 7's arena. New boxes are built in new sandboxes off the audio thread; kept boxes move by `mem::swap`; ghosts run for the 10 ms window; retired boxes drop on `m2-play-drop`, which unmaps their memory whole. The actor never enters or writes a live sandbox |
| **Voice pools** | One sandbox per pool per package, laid out voice by voice; the voice graph's wires live in it, so voice kernels pass sound with no copies |
| **Plugins** | Unchanged |
| **Exports** | Their own renderer and sandboxes, as today: two p50 renderers hold 400 sandboxes, 1.24% of the address space |
| **Seeks, resets** | Unchanged in meaning. A reset is in place and can't allocate |
| **8a** | A box is in one partition, so its Store is entered only by the thread running that partition's task in that block |
| **Isolation** | A bad kernel can harm only its own box (one node, or one instrument's voices). A trap mutes that box alone, not the song |
| **Determinism** | A sandbox's layout is a pure function of its box's shape, the rate and the package bytes, and it starts as zeroed memory plus the module's data. So every state sits at the same address with the same bytes live, in export, on any thread count |
| **p50, estimates** | Linux callback about 480 to 540 µs under S (S ≈ 7,020; HEAD with P2 is S + 212). Footprint about 22 to 31 MB (195 MB today); Mac about 35 to 50 MB (575). Address space 1.24% for two renderers (25% allowed). Build at most about S's 181 ms |
| **Cost** | Four slices: kit v1 and std; sandboxes in the runtime; a trap mutes its box; ABI v1's reservations. Grouping, batching and zero-copy edges stay for later |
| **Weakest point** | Every top-level kernel spends a whole 4 GiB sandbox. The ceiling is about 4,000 sandboxes per renderer, so a mix of 8 effects a track hits it near 360 tracks. Grouping later rests on a relocation rule this design reserves in ABI v1 but its own host never exercises (§13) |

---

## 0. Idea 3: what is adopted, what is changed, and why

**Adopted from idea 3:**
1. **Many states per instance.** v0's state is one `static mut HOST` per kernel type per instance, and no export takes a
   handle (`std/kernels-src/kernel/src/lib.rs:179-188`). v1 passes the state's address to every export.
2. **The host places every state and buffer** in the sandbox's memory, from sizes the kernel declares. That is
   gen~'s `getstatesize` and faustwasm's `dspMeta.size * voices` (external §B).
3. **A 4 GiB reservation, so the hardware catches out-of-bounds accesses.** It removes O1's 9.6 M JIT instructions a
   callback (`results.md`: 87.8 M under P2 against 78.2 M under P1). It fits the 25% rule only with few memories: at
   4.0625 GiB a memory, two renderers may hold 32,768 / 2 / 4.0625 = 4,033 memories each (code-map §2.3).
4. **Wires in place.** Kernels in one sandbox read each other's outputs where they were written.
5. **One package per memory.** That is the isolation boundary between packages.

**Changed: the unit is the box, not the track.** A track's sandbox would hold nodes that live and die at different
times, and a live edit must then put new states into, and take old ones out of, a memory the audio thread is using.
wasmtime gives three ways to put a new state in, and each breaks one of this brief's properties:

| Way to start a new state in a live per-track sandbox | What it breaks |
|---|---|
| The actor writes it in, through the memory's raw base (cut 1 keeps one, `cut1.diff:53-60`), while the audio thread runs wasm in the same memory | `k_init` is kernel code, and calling it needs `&mut Store` (`W/runtime/func/typed.rs:100-105`), which the audio thread holds. Writing bytes from outside also races any buggy kernel in that sandbox. Nothing the actor does to a live sandbox is checked by the Store's ownership any more |
| The audio thread runs `k_init` at the activation | Init is unbounded: ABI-2's assets mean a third-party init may transform an impulse response, and any init zeroes its buffers. The rule today is that new instances are built and prewarmed off the audio thread (`core/runtime/src/lib.rs:1294-1310`) |
| Init in a scratch sandbox, then copy the bytes into a free slot at the activation | An audio-thread copy as big as the state: a 2-second stereo `delay` is 768,016 B at 48 kHz and 6,144,016 B at 384 kHz (code-map §1.4). It also needs every state to be relocatable |

Taking states out is no better:
- wasmtime frees nothing in a Store until it drops (`W/runtime/store.rs:139-145`), and a memory can't shrink
  (`W/runtime/memory.rs:269-713` has no such call; external §A1). A per-track memory keeps its high-water mark for the
  session, unless the host recycles slots.
- A recycled slot holds the old node's bytes. A third-party kernel that reads a field its init forgot would then sound
  different live (after edits) and in export (fresh). And the slot's address would depend on the edit history.

**What per track would win, measured against per box:**
- Fewer sandboxes: 51 against 200 for p50 (code-map §6.3). Both are far under the ceiling: 0.32% against 1.24% of the
  address space for two renderers.
- No copies on top-level edges. Those are small, though. 150 of the 2,245 `k_process` calls a block are top-level
  (code-map §6.2), 6.7%. If copy time follows calls, that is 0.067 × 393 µs ≈ 26 µs a callback (393 µs: port copies
  with all cuts, `results.md` profile table). **Estimate**, and low if top-level copies are larger per call.

So per box keeps nearly all of idea 3's gain (93% of the calls are inside pools), keeps step 7's live-edit mechanism
untouched, and changes nothing about who owns a Store when.

**Rejected outright:** one sandbox per package per renderer (it can't free a replaced node, `W/runtime/store.rs:139-145`);
one per kernel type per track (the same live-edit problem as per track, with more sandboxes, code-map §6.3); shared
memories across Stores (a tier-2 feature with no CVEs issued, `W/config.rs:3316-3335`, and a Rust std rebuilt with
atomics, external §C4).

---

## 1. The unit

**Definitions.**
- *Box* (step 7's word, kept): what an activation keeps, moves or retires whole: a top-level kernel node, a voice pool,
  a plugin instance, a feedback ring (`plan.rs:690-712`; RENDER-25's text, `docs/architecture.md:256`).
- *Sandbox* (new): one `Store`, one instance of one package's module, one memory. A kernel box owns one sandbox. A pool
  box owns one sandbox per package its voice graph uses. Plugins and rings own none.
- *State block*: the bytes of one kernel instance's state inside a sandbox: one per top-level kernel node, and one per
  kernel node per voice.
- *Wire*: one output port's buffer inside a sandbox, read in place by every consumer in that sandbox.

**p50's counts** (from code-map §6.2):

| | Today (step 7, P2) | This design |
|---|---|---|
| Stores | 6,058 (and a dummy default-caller instance in each, `W/runtime/store.rs:779-816`) | 200: 50 pools + 150 top-level kernels |
| Instances of std.wasm | 6,058 | 200 |
| Linear memories | 6,058 × 32.125 MiB | 200 × 4.0625 GiB |
| Kernel states | 6,058, one `HOST` slot used of 29 per instance (code-map §1.3) | 6,058 blocks placed by the host |
| Port buffers | 19,916, each 1 KiB (**count**: per voice bass 49, pluck 43, pad 38 ports, from `std/m2pkg.toml` and code-map §6.1; × 68, 204, 192 voices; plus 516 top-level) | about 6,836 voice wires (one per output port: bass 16, pluck 15, pad 14 a voice), 1,392 note-field buffers (464 voices × pitch, velocity, gate), the 516 top-level ports, and constants and cells once per pool |
| Kernel calls a callback | about 9,250 | about 9,250 (unchanged; §12 has the later cut) |

**What a sandbox's memory looks like:**

```
0            64 KiB        initial size                                         end (fixed at build)        4 GiB + 32 MiB
|  stack      |  data       |  io tables · cfg blocks · constants · cells ·      |  inaccessible: a stray
|  (module)   |  (module)   |  captures · note fields · wires · state blocks     |  access traps
|<-- the module's: its declared minimum -->|<-- the host's region, laid out at build -->|
```

- The module owns `[0, initial size)`: its shadow stack and data. For std.wasm that is 2 pages: a 64 KiB stack and
  9,808 B of data at 65,536 (code-map §1.3).
- The host owns everything above. In a pool it lays the region out voice by voice: voice 0's wires and states, then
  voice 1's, so each voice's working set is contiguous. Shared buffers (constants, cells, captures) come first.
  Blocks are 64-byte aligned.

---

## 2. The kernel ABI v1

### 2.1 The module

- **Exports `memory`, defined, not imported:** each sandbox owns exactly its memory, and copy-on-write images apply only
  to defined memories (`W/config.rs:2259`). It must be 32-bit and not shared: elision needs a 32-bit index
  (`E/types.rs:2399-2407`).
- **Exports `m2_abi() -> i32`**, which returns 1. The runtime refuses any other version (ABI-3,
  `docs/architecture.md:1180`). v0 modules are refused: no compatibility is kept.
- **Imports nothing but `m2.asset_read`** (§7.2). There is still no clock, no randomness and no WASI.
- **Never grows its memory.** `memory.grow` from wasm always returns −1 (§3.1).

### 2.2 Exports per kernel type `k`

`state`, `cfg`, `io`, `dst` and `src` are `i32` addresses in the sandbox's memory.

| Export | Signature | Meaning | Called by, on |
|---|---|---|---|
| `k_ports` | `() -> i32` | `inputs << 8 \| outputs`, checked against the manifest (as v0, `kernel/src/lib.rs:195-198`) | the package check, once |
| `k_size` | `(cfg) -> i32` | bytes of state for this config. Pure: the same `cfg` bytes always give the same size | the build, actor thread |
| `k_init` | `(state, cfg)` | build the state in the zeroed block `[state, state + size)`. May read assets. May take as long as it needs | the build, actor thread |
| `k_reset` | `(state)` | put the state back as `k_init` left it. Bounded work, no allocation (nothing to allocate with) | the build's prewarm; the audio thread (voice reuse, panic, a poisoned box); an offline seek |
| `k_process` | `(state, io, from, to)` | read inputs and write outputs for frames `[from, to)` of the block, `0 ≤ from < to ≤ max_block` | the thread running the box |
| `k_active` | `(state) -> i32` | 1 while sound already taken in can still come out (as v0, `kernel/src/lib.rs:55-63`) | voice ending, `can_idle` |
| `k_save` (optional) | `(state, dst, cap) -> i32` | write the kernel's own settings (never its DSP tail); return the bytes written, or −(bytes needed) when `cap` is too small | §7.3 |
| `k_load` (optional) | `(state, src, len) -> i32` | load them; 0 means accepted | the build, after `k_init` |
| `k_latency`, `k_tail` (optional) | `(state) -> i32` | frames of latency; frames of tail (−1: until `k_active` says 0) | the build |

Why these shapes:
- **Every `k_process` has the same signature** `(i32, i32, i32, i32) -> ()`. So a later in-sandbox runner can call any
  kernel through one `call_indirect` type (§12), with no ABI change.
- **`from` and `to` replace v0's "frames at index 0".** v0 copies a segment's frames to index 0 of each port and back
  (`lib.rs:343-373`). With wires in place, a voice segment at frame 64 must read and write frames 64 onwards where they
  are. v0's `Io::first` (the segment's first sample) becomes the input at `from`: the same value.
- **One `cfg` pointer, not a fixed argument list.** The block begins with its own length, so a v1.x field can be
  appended without changing a signature.

### 2.3 The config block (`cfg`) and the io table (`io`), written by the host

```
cfg   u32 size           bytes in this block; fields are only ever appended
      f32 sample_rate
      u32 max_block      128 live; larger for offline kernels (§7.5)
      u32 flags          bit 0: offline render
      u32 ports          inputs << 16 | outputs
      u8  channels[]     one per port, inputs then outputs (ABI-2's channel counts per port), padded to 4
      u32 assets         count, then { u32 handle, u64 len } each (§7.2)
      u32 blob, blob_len a saved state for k_load, or 0, 0 (§7.3)

io    u32 port[inputs + outputs]   the address of each port's buffer
```

- **An audio or control buffer** holds `channels × max_block` f32, channel-major: channel `c` starts at
  `4 × c × max_block` bytes. That is v0's format (`kernel/src/lib.rs:20-22`), with each port's own channel count
  instead of v0's 2 for every port.
- **An event buffer** is `{ u32 count, u32 cap, Event[cap] }`, sorted by frame. Each event is RENDER-14's fixed-size
  `Event`.
- **Both blocks are written once, at build,** and never change while the box lives. The host keeps a master copy of
  each (§8).
- **Inputs and outputs of one call never overlap.** The host lays them out, so v0's overlap refusal
  (`lib.rs:236-248`) becomes a host invariant. Two inputs of one call may be the same wire (`add(x, x)`): both only
  read it.
- **Inputs stay read-only (ABI-1).** This matters more now: a wire is also its producer's output, read by other
  consumers.

### 2.4 Lifecycle of a state

- **Create:** the host gives a zeroed, 64-byte-aligned block, then calls `k_init(state, cfg)`, then `k_load` if a blob
  is given. This happens only in a sandbox that isn't live yet.
- **Reset:** `k_reset(state)`, on any thread that holds the box.
- **Drop:** nothing. The whole sandbox drops with its box.
- **Relocation rule (reserved, §13):** a state's meaning must not depend on its address. It holds no absolute address
  of itself and is reached only through the `state` argument. Port addresses come from `io` at each call and must not
  be kept. v1's host never moves a state, but the package check moves every state between calls and compares the bits
  (§10), so the rule is enforced from day one.

### 2.5 The kit (`std/kernels-src/kernel`) becomes

```rust
pub trait Kernel: Sized {
    const INPUTS: usize;
    const OUTPUTS: usize;
    /// Fresh state from the config. Run twice per state: once to measure (`mem` only counts and
    /// `cfg.measuring()` is true, so heavy work can wait), then for real. It must lay out the same
    /// buffers both times and must not depend on history. `reset` runs it again in place.
    fn new(cfg: &Config, mem: &mut Mem) -> Self;
    fn process(&mut self, io: &mut Io);
    fn active(&self) -> bool { false }
    // Optional, with defaults: save, load, latency, tail.
}
```

- **`Mem::zeros::<f32>(n) -> Slab<f32>`** replaces `vec![0.0; n]`. A `Slab` is an offset from the state's base and a
  length, so a state holds no absolute address (the relocation rule, by construction).
- **`Io`** gives `input_ch(i, c) -> &[f32]` and `output_ch(i, c) -> &mut [f32]` over `[from, to)`, `first(i)`, and
  `slab(s) -> &mut [T]`. Inputs come out as shared slices, so two ports on one wire stay sound in Rust.
- **`export!(name, Type)`** generates the exports of §2.2:
  - `k_size` = the size of the kit's header and `K`, plus the measured slabs;
  - `k_init` writes the header (rate, channels, the `cfg` offset) and `K`;
  - `k_reset` zeroes the slabs and runs `new` again, which gives the same offsets because `new` is pure.
  - `abi!()` returns 1.
- **The kit has no allocating API, and there are no `static mut`s.** It stays a `std` crate, so std.wasm's math is the
  same code and the same bits (PLAT-4, `docs/architecture.md:1200`).
  - Whether std's allocator is still linked into std.wasm is unverified until it is built.
  - If it is, it is never called: a kernel's growth is refused (§3.1), so any allocation fails and traps.
- **std's 29 kernels:**
  - 25 hold no `Vec` (code-map §1.3). They change only in `new`'s signature;
  - `delay`, `reverb`, `chorus` and `widen` move their lines to slabs (`std/kernels-src/src/lib.rs:481-496`;
    `src/fx.rs:144-152, 208-245, 335-357, 667-685`).
- **Bit-identity.** The arithmetic and its order don't change. v0's reset (`K::new`) and v1's (`new` in place) give the
  same values. So the 8 recorded renders and the p16 render are expected to stay the same bits. They are the slice's
  gate. The module's SHA changes, so every kernel starts from init once (PLAT-5, `docs/architecture.md:1204-1206`).

---

## 3. Who allocates, where, and the limits

### 3.1 Only the host, only at build

The host builds a sandbox in this order, on the actor thread (or the CLI's main thread, or wherever `Renderer::build`
runs; code-map §7):
1. `Store::new`, then `Instance::new` with the one import.
2. A first host grow, for the cfg blocks (their sizes are the host's own), and write them.
3. `k_size` for each state. These calls can be cached per `(kernel, cfg bytes)`: most states of a type share one.
4. Lay out the host region (§1) and check the budgets (§3.2). Then a second and last `Memory::grow`, to the final
   size (`W/runtime/memory.rs:637`).
5. Write the io tables and constants.
6. `k_init` (and `k_load`) for every state, then one `k_reset` each: the prewarm, as `lib.rs:1294-1310` does today.
7. **Touch every page** of the memory, the module's pages included, by writing each page's own first byte back. After
   that no first touch, and no copy-on-write fault, can happen on the audio thread.
8. **Seal.** From then on the limiter refuses every growth.

**The limiter.** The Store's data holds `{ budget, host_growing: bool, refused_at: Option<usize> }`. Its
`memory_growing` (`W/runtime/limits.rs:69-74`) allows growth only while `host_growing` is set, which the host does
around its own `Memory::grow` call. Any other growth comes from wasm. It is refused and `refused_at` is recorded:
- the refusal comes before any syscall: the limiter is asked first (`W/runtime/vm/memory.rs:651-659`; code-map §3.7);
- wasm sees −1;
- a kernel that ignores −1 traps, and the error can now say "tried to allocate".

The Store keeps `instances(1)`, `memories(1)` and `tables(1)` (as `lib.rs:202`).

### 3.2 What replaces the 32 MiB cap per kernel

| Limit | Checked | Proposal | Why |
|---|---|---|---|
| A box's host region | at build, from the `k_size` sums, before any `k_init` | 256 MiB, the owner's to set | std's largest state is one `delay` at 384 kHz: 6,144,016 B (code-map §1.4). v1's reset is in place, so v0's 2× peak during a reset is gone |
| A renderer's total | at build, the sum over its boxes | a share of physical memory, the owner's to set | the machine, not the address space, is what runs out |
| Sandboxes per renderer | at build | 4,033 (the 25% rule at 4.0625 GiB each, two renderers) | RENDER-32's address-space rule (`docs/architecture.md:328`) |
| wasm32 itself | — | 4 GiB, less the module's part | can't be raised for a 32-bit memory |

**Assets count against none of these.** They are host objects (§7.2), with their own budget in step 9's loader.

### 3.3 What the user sees when a limit is hit

Every limit is met at build, off the audio thread. The version before keeps playing, through the path that exists
today (`Way::Unbuilt`, `crates/server/src/graph/actor.rs:411-424, 573-576`).

The error becomes typed (`StateBudget { node, package, kernel, asked, allowed }`, and so on). The server then stops
guessing from words (`out_of_memory` matches strings, `crates/server/src/graph.rs:382-387`). Proposed wording:
- **A box over its budget:** "Your last change is saved, but `t03 · delay` asks for 6.1 GB of sound memory, more than
  the 256 MB one sound part may use, so the version before it keeps playing."
- **The song over its total:** "This song needs 9.8 GB of sound memory, more than music2 can use on this computer
  (8 GB)."
- **Too many sandboxes:** "This song has more sound parts outside instruments than music2 can play at once (4,033)."
- **A kernel that tried to grow:** "`x` from package `y` tried to allocate memory while starting. Kernels take memory
  only from their own state (kernel ABI v1)." That is a package bug, so it names the package.

Live, the same refusal (in a buggy third-party kernel) traps, and the box is muted (§8).

Compare today: a refused grow becomes an `unreachable` trap whose message never says memory. On the audio thread it
silences the whole song with no message (code-map §3.7).

---

## 4. A live edit, step by step

| # | Thread | What happens | Allocates, locks or makes a syscall? |
|---|---|---|---|
| P1 | actor | `keep::wire` decides kept, new, ghosts and fades, unchanged (`core/runtime/src/keep.rs:166-189, 251-455`) | yes, off the audio thread |
| P2 | actor | each new box gets new sandboxes, built as in §3.1. Kept and ghost slots are `Make::Moved`, as today (`lib.rs:1623-1627`) | yes (mmap, mprotect, page touch), off the audio thread |
| P3 | actor | the `Entry` (moves, lanes, cells, releases) and the `Leftovers`, sized now (`plan.rs:257-270, 318-324`) | yes, off the audio thread |
| P4 | actor → device | `Msg::Activate(Box<Renderer>)` on the bounded queue (`core/play/src/lib.rs:47-48`) | the send. The receive is a non-blocking pop |
| A1 | device, at a block line, outside a window, with stash room (`engine.rs:411-422`) | `verify` (`plan.rs:551-578`); `take_from` swaps each kept box (`plan.rs:690-712`). A sandbox's memory doesn't move when its Store moves, so every cached base stays valid (`cut1.diff:53-60`) | no: swaps and compares |
| A2 | device | a poisoned box: the host copies back its io tables and cfg blocks from its master copies, then `k_reset` on each of its states (`plan.rs:595-604`) | no: bounded copies and a wasm call (`W/runtime/func/typed.rs:154-224`); the kit can't allocate, and memory can't grow (§3.1) |
| W | device and workers | the window: ghosts (old boxes, in their old sandboxes) and fades run in the new plan for `fade + wait` = 240 + 240 frames at 48 kHz (`lib.rs:1636-1639`, `plan.rs:541-544`) | no: kernel calls and host arithmetic |
| A3 | device, first block line after the window | `end_fade` moves the ghosts into the `Leftovers` (`plan.rs:642-669`), which leave as `Retired::Nodes` (`engine.rs:488-495`) | no: swaps into vectors sized at P3 |
| D | `m2-play-drop` | the retired boxes drop. Each Store drop unmaps its memory whole (`W/runtime/store.rs:2425-2457`) | yes, off the audio thread |

- **Untouched state stays where it is.** A kept box's sandbox isn't entered, copied or rewritten by the activation. It
  just changes plans.
- **Removed state is freed exactly when its box retires,** and wholly: the memory goes back to the OS, with no
  high-water mark left behind.

**The one exception is a fault, and it is bounded.** A trap allocates the error inside wasmtime
(`W/runtime/trap.rs:80-88`; the step-7 path says so, `engine.rs:570-579`). On the Mac it also waits for wasmtime's
Mach-port thread (`refresh.md:192`). It happens once per faulting box, which is then muted (§8). That is no worse
than today, where the same trap silences the whole song.

**Who touches which Store, when:**

| Period | Thread | Stores entered | Ownership from |
|---|---|---|---|
| package load | actor | a throwaway Store for `verify` (`core/runtime/src/packages.rs:470-511`) | made and dropped there |
| build of a box | actor (or the CLI's main thread) | only that box's new sandboxes | the plan being built |
| hand-off | actor → device | none (moved inside `Box<Renderer>`) | the queue's send and receive order it |
| activation, end of window | device, with 8a's pool idle (`refresh.md` §3.2) | poisoned boxes only (A2) | `take_over` holds the renderer by `&mut` |
| each block | device plus 8a's workers | a box's sandboxes, by the one thread running its partition's task | 8a's `own(i)` (`design-8a.md` §4.2) |
| retire | `m2-play-drop` | none: dropped, never called | the retire queue |
| export | built on the actor; rendered by `music2-export` and its offline pool | the export's own sandboxes only | moved with the `Render` (`crates/server/src/export.rs:146-152`) |

**So no Store is ever entered by two threads at once, and none by the actor once it is live. The actor never writes
into a live sandbox's memory.** The host writes into a sandbox during a block (cells, note fields, captures) only on
the thread that holds the box. That is cut 1's rule for its raw base (`cut1.diff:58-60, 111-113`).

---

## 5. Voice pools, plugins, exports, seeks and resets

### 5.1 Voice pools

- **Build.** One sandbox per package: `voices × kernel nodes` states, laid out voice by voice (§1). Today every voice
  kernel gets its own `KernelInst` and `Store` (`lib.rs:894-899`).
- **A block** keeps today's order (`lib.rs:1364-1477`), with these changes:
  - **Captures:** copied once a block into the pool's capture buffers, not once per voice and kernel.
  - **Cells inside the voice** (`CellAt::Voice`, `core/runtime/src/cells.rs:27-34`): one buffer per pool, copied once
    per value (RENDER-22).
  - **Note fields:** pitch and velocity are written when a voice takes a note; the gate for each segment.
  - **Each voice segment** calls `k_process(state, io, from, to)` for each kernel in plan order. No copies between them.
  - **Voice output:** the host reads each voice's output wire in place and adds it into the pool's output in voice
    order, as `lib.rs:1438-1444` does today. The same `+=` sequence gives the same bits (RENDER-2,
    `docs/architecture.md:101`).
- **Voice reuse:** `k_reset` on each of the voice's states, on the box's thread, as today (`lib.rs:1454-1456`).
- **Keep, steal, ghost:** unchanged. A pool is kept or replaced whole by its `Shape::Poly`, which already includes its
  kernels' wasm hashes (`plan.rs:48-54, 87-106`). A ghost pool reads no notes (`keep.rs:341-349`).
- **Plugins can't be inside a voice** (`core/ir/src/check.rs:563`), so a voice graph is kernels, note fields,
  captures, constants and cells only.

### 5.2 Plugins

Unchanged: CLAP, `Arc<PluginInstance>`, the plugin main thread, `kill()` on a live error (code-map §4.3). An edge
between a plugin and a kernel copies through host buffers, as today.

### 5.3 Exports (two renderers)

- **Unchanged in kind.** `Render::new` builds a fresh renderer for a pure song; `lending` borrows only plugin
  instances (`crates/editor/src/render.rs:132-155`).
- **Address space:** two p50 renderers hold 2 × 200 × 4.0625 = 1,625 GiB, 1.24% of 131,072 GiB. Even a third, partial
  set (a pure song's edits prepared during its export, code-map §5.6) leaves the 25% rule far away.
- **Determinism:** the export's layout is the live one's, made fresh (§9).

### 5.4 Seeks and resets

- **Unchanged in meaning** (RENDER-10, `docs/architecture.md:158`):
  - a live seek, loop wrap or stop resets nothing (`engine.rs:369-377`);
  - Panic resets every box on the device thread (`lib.rs:1210-1243`);
  - an offline seek resets and replays from 0 (`lib.rs:1987-2000`).
- **A reset zeroes slabs in place.** So a Panic's cost stays what it is today: the same delay lines are zeroed.

---

## 6. How 8a's partitions map onto it

- 8a's precondition holds as it is: "a `Store` is entered by one thread at a time, so kernels in different partitions
  can run at once" (`design-8a.md:98-101`), "still one per box, and a box lives in one partition" (`refresh.md:52`).
  Here a box owns its sandboxes, so a partition's task enters exactly the sandboxes of its own boxes.
- p50's 51 partitions (`design-8a.md:284`) enter these sandboxes a block:
  - a bass track: 3 (pool, drive, delay);
  - a pluck track: 1;
  - a pad track: 2;
  - the song: 100.
- **Workers** call `Engine::tls_eager_initialize()` once before their first kernel call (`refresh.md:66, 190`). With
  two engines (§8) that call applies to the process, so once is enough. That is unverified: the per-thread state is
  wasmtime's, not the Engine's (`W/engine.rs:782-799`).
- **Voices across threads, later** (`design-8a.md` §8): a pool splits into chunk sandboxes, each laid out the same way,
  summed in voice order by 8a's rule. That is a host choice; the ABI doesn't change.
- **A failure in a task** (§8): the box is muted and the partition goes on. Its consumers read silence, rather than
  being skipped as `design-8a.md` §4.3 has them. Offline, a failure still fails the render, reported at the lowest plan
  position, so the error is the same on 1 thread and on N.

---

## 7. ABI v1's reservations, fitted

### 7.1 Event ports

- An event input or output is a buffer in the sandbox (§2.3), sized by the manifest's capacity (plugins use 256 out,
  `lib.rs:487`).
- The checker's refusal of events into kernels (`core/ir/src/check.rs:929-931`) is lifted for v1 kernels.
- The host writes input events in frame order. A segment gets events with `from ≤ frame < to`, by the same split
  today's plugin calls use (`lib.rs:597-697`).

### 7.2 Asset blobs, outside every memory

- **A blob is a host object:** resident bytes (`Arc<[u8]>`), read into memory off the audio thread and never mapped
  lazily, so reading one is never I/O (RENDER-28, `docs/architecture.md:285`). Any number of sandboxes and both
  renderers share one blob. It counts against no box's budget.
- **The cfg block lists a state's assets:** handles into the sandbox's asset table (in the Store's data) and their
  lengths.
- **The import:** `m2.asset_read(handle: i32, offset: i64, dst: i32, len: i32) -> i32`. It copies up to `len` bytes
  into the caller's memory and returns the count, or −1 for a bad handle or a `dst` outside the memory.
  - It is allowed in `init`, `reset` and `process`.
  - The host side is a table lookup, a bounds check and a `memcpy`, with no allocation or lock. The host-call path
    shows no allocation (`W/runtime/func.rs:2361-2420`); its cost is unmeasured.
  - A per-call cap on `len` (proposal: 1 MiB) bounds the time a call takes.
- **Uses:** an impulse response is read at init and transformed into the state, and the state's size comes from the
  asset length in `cfg`. A sampler reads the frames it needs each block into a slab.
- **Step 9's streaming fits the same import.** A streamed blob is a resident window. A read outside it returns 0 bytes
  (the voice plays silence, counted as an underrun) live. Offline, the render waits for the window before the block.

### 7.3 State save and load

- **Load:** `k_load` runs at build only, after `k_init`. In v1 a changed blob is part of the box's shape, so the box is
  new and crossfades like any replaced node (RENDER-25a). That keeps the activation free of state loads. Loading into a
  kept instance (as RENDER-26b does for plugins) can come later.
- **Save:** asked for at a block line, `k_save` writes into a scratch area reserved in the sandbox at build (its size
  from the manifest). The host copies the bytes into a buffer the actor sent with the request, and sends it back on
  the retire queue. That is bounded and allocates nothing. Proposed for when a kernel needs it; nothing in std does.

### 7.4 Channel counts per port, latency and tail

- **Channels:** in `cfg` (§2.3). Wires are sized by them, so a mono control wire is 512 B, not v0's 1 KiB.
- **Latency and tail:** read at build (`k_latency`, `k_tail`). The engine owns latency (RENDER-17), so these feed it
  when compensation is built.

### 7.5 Offline kernels

- **A manifest flag.** The checker refuses an offline kernel in a live plan.
- **Its `cfg` gives a large `max_block`** and an extra time-map input, so the same exports serve ("whole buffers in and
  out, with a time map", ABI-2, `docs/architecture.md:1170-1178`).
- **Its box has an offline budget,** since whole buffers live in its sandbox.

---

## 8. Isolation: what a buggy kernel can harm

| | Its own box | Other boxes, same package | Other packages | The host, the process |
|---|---|---|---|---|
| Corrupt memory | any byte of its sandbox: its own state, its outputs, and in a pool its sibling voices and the other kernels of that package in the instrument; the io tables and cfg blocks too | no: another memory, behind its own 4 GiB and guard (`E/types.rs:2399-2407`) | no | no |
| Bad sound | its outputs. The non-finite check marks the box (`lib.rs:1181-1185`), and the output stage silences non-finite samples (`lib.rs:1909-1922`) | only through the song's own wiring | likewise | — |
| Take time | up to the deadline (guarded engine), then a trap. std has no deadline, as today | that block's deadline, shared by all | likewise | an xrun on that block |
| Trap | its box is muted until the next activation resets it | plays on | plays on | one allocated error; on the Mac, one wait for wasmtime's Mach-port thread |
| Memory | refused at build if over budget; it can never grow live | — | — | — |
| Read | the assets its `cfg` names | — | — | no files, no clock, no network (no imports but `asset_read`) |

**Three changes make this hold:**
1. **A trap mutes its box, not the renderer.** Today a trap sets `broken` and the transport stops, with no message
   (`engine.rs:570-579`; code-map §3.7). A plugin already fails alone (`kill()`, HOST-5,
   `docs/architecture.md:1156-1157`); kernels get the same:
   - the box's outputs play silence;
   - the session is told, and says "`t03 · reverb` stopped working and is muted; it starts again at your next edit";
   - the next activation resets it, after copying back the host's master io tables and cfg blocks;
   - HOST-5's once-in-10-seconds rule stops a kernel that fails at every call being restarted over and over.
   - Offline, a trap still fails the render, naming the node.
2. **Two engines.** std compiles on a *trusted* engine, as today with no epoch checks. Every other package compiles on
   a *guarded* engine with `epoch_interruption(true)`:
   - each guarded Store gets `set_epoch_deadline` at each block, which is a field write (`W/runtime/store.rs:2231-2239`);
   - a ticker thread calls `increment_epoch` (`W/engine.rs:875`);
   - epochs are "designed to handle malicious WebAssembly guests" (`W/config.rs:744-746`);
   - `memory.copy` checks only at its start, which is bounded by the box's budget (`W/config.rs:749-762`).
   - A sandbox never mixes packages, so two engines cost nothing structurally. Separate engines need separate Stores,
     which this design has anyway (design-7 §11.1, `docs/designs/step7/design-7.md:1311-1328`).
   - The epoch checks' own cost is unmeasured.
3. **The Spectre guard goes back on.** It is a no-op on accesses that need no bounds check (`CG/shared/settings.rs:290-302`).
   That settles RENDER-32's deferred item (`docs/architecture.md:1232`).

**What still isn't isolated:**
- **Kernels of one package inside one instrument share a sandbox.** A std bug in `svf` could corrupt the `adsr` beside
  it. That is the price of in-place wires. Per box limits it to one instrument; per track would have widened it to the
  whole track.
- **A kernel can make its own box's output wrong in finite ways.** No sandbox can stop that.

---

## 9. Determinism

1. **The code.**
   - No import but `asset_read`, which reads immutable bytes.
   - No threads, no shared memory (the check refuses one), relaxed SIMD off (`packages.rs:118`).
   - NaN canonicalisation stays off: it "costs about 4.6× in the M1 benchmark", and the renderer silences non-finite
     output instead (`core/runtime/src/packages.rs:105-108`).
   - Unverifiable for third-party code: a kernel that reads a NaN's bits could differ between CPUs, which PLAT-4
     forbids (`docs/architecture.md:1200`). The package check can't prove the absence; the renders record catches it
     for std.
2. **The layout.**
   - A sandbox's layout is a pure function of its box's shape, the sample rate and the package bytes. It is laid out
     in plan order, never by history.
   - So every state has the same wasm address live, in an export, after any number of edits, on any thread count and
     on any OS (wasm pages are 64 KiB everywhere, `W/runtime/memory.rs:865-867`).
   - A third-party kernel that depends on its address, against §2.4, still renders the same bits live and in export.
3. **The bytes.** Every new sandbox starts as zeroed pages plus the module's data. No slot is reused. So a kernel that
   reads a field its init forgot reads 0, the same every time.
4. **No growth.** Nothing can fail mid-render for lack of memory: every budget is met before the first block.
5. **Order.**
   - Voice allocation, the voice sum in voice order, and the partition rules are unchanged (RENDER-2,
     `docs/architecture.md:101`; `design-8a.md` §0, "Why the bits can't change").
   - One box is in one task. Boxes share nothing but host buffers, which are read after their producers finish.
6. **Faults are the only timing-dependent path.** A deadline trap is live only: the box is muted and the user is told.
   Offline the deadline is long (proposal: 10 s a call), and passing it fails the render, naming the node. So bits
   never differ silently.

---

## 10. What changes

**Code** (all paths under `/home/jerry/m2-runs/idea3/src`):

| File | Change |
|---|---|
| `std/kernels-src/kernel/src/lib.rs` | the kit v1 (§2.5), rewritten; about 270 lines today |
| `std/kernels-src/src/lib.rs`, `src/fx.rs` | 25 kernels: `new`'s signature. `delay`, `reverb`, `chorus`, `widen`: slabs |
| `std/m2pkg.toml`, `std/build.sh`, `std/kernels/std.wasm` | rebuilt; kernel hashes change (PLAT-5) |
| `core/runtime/src/packages.rs` | `engine_with`: wasmtime's defaults plus `memory_may_move(false)`, Spectre on, P2 removed or kept only for gates (`:103-133`); a guarded engine for other packages; `verify_module` v1 (`:470-511`): the exports and signatures of §2.2, a defined 32-bit unshared memory, only `m2.asset_read` imported; `KERNEL_ABI = 1` |
| `core/runtime/src/lib.rs` | `KernelInst` (`:148-276`) becomes `Sandbox` (Store, instance, memory, cut 1's `MemView`, per-type functions, the host region's master copies) plus a small `KernelRef { state, io, kind }`; `KernelNode::process` (`:329-375`) copies only at the top level; `PolyNode` build and process (`:882-907, 1364-1477`) run voices in their sandbox with wires in place; `Graph::prewarm` (`:1294-1310`) becomes the build's init, reset, touch and seal; `kernel_memory` (`:1312-1322`) sums host regions |
| `core/runtime/src/plan.rs` | the box types in `take_from` (`:690-712`) change; the mechanism doesn't. A kernel's settings in its shape gain per-port channels, asset hashes and the blob hash |
| `core/runtime/src/engine.rs` | a trap mutes its box and is counted for the session (`:570-579`) |
| `core/runtime/src/cells.rs` | a voice cell is copied once per pool |
| `core/ir/src/check.rs` | events into v1 kernels (`:929-931`); offline kernels refused live |
| `crates/server/src/graph.rs`, `graph/actor.rs` | typed budget errors and their wording (§3.3); the "muted" notice |
| `crates/gates/src/store.rs` (G7f) | measures this layout against S, P1 and P2 |
| tests | `core/play/tests/no_alloc.rs`, `churn.rs`, `bit_identity.rs`, `core/runtime/src/tests/*` adapted. New: a kernel's growth is refused live and at build, with the right message; a trapping kernel mutes only its box, and the rest is bit-identical; a box's layout is the same live and in export; std passes the relocation check |
| package check (`m2 pkg check`, or wherever packages are checked) | renders each kernel with every state moved between calls and compares the bits (§2.4); renders twice and compares (RENDER-2) |

**Rules** (`docs/architecture.md`):
- **RENDER-32** (`:328`), rewritten:
  - "One `Store` per sandbox: one package's instance and its one memory, holding one box's states, a top-level kernel
    node or a voice pool. A memory reserves 4 GiB plus a 32 MiB guard, so accesses aren't checked. Only the host grows it,
    while its box is built, never after, and it never moves. Cranelift's Spectre guard is on. Never a `Store` shared across
    plans or boxes."
  - The 25% rule, the footprint and build bounds, and G7f's judging stay.
- **ABI-1** (`:1166`): inputs are read-only, and now a wire is another kernel's output. The host never gives one call
  an input that overlaps an output. The debug check still compares inputs around every call.
- **ABI-2** (`:1170-1178`): each reservation gets its shape (§7). **ABI-3** (`:1180`): `m2_abi() = 1`; v0 is refused.
- **RENDER-25** (`:253`): a kernel's settings are per-port channels, asset hashes and the state blob's hash. A trapped
  box is muted alone and reset as it moves.
- **RENDER-28** (`:285`): add "a kernel memory grows only while its box is built; the limiter refuses growth from
  then on, so no `memory.grow` runs on the audio thread".
- **RENDER-16** (`:198-204`): a pool's voices share one sandbox per package.
- **RENDER-25a, RENDER-10:** unchanged.
- **design-7 §11.1** (`docs/designs/step7/design-7.md:1311-1328`): superseded, but its reasons hold. No Store is shared
  across plans. "The cost scales with memories, not stores" is exactly why memories become few.
- **Deferred table** (`:1231-1232`): the Spectre item is done. Kernel ABI v1 is this design.

---

## 11. Expected effect on p50

Linux numbers, information only; the Mac judges (PLAT-1).

**Time per 512-frame callback (estimate).**
- **Base:** with all four cuts and the P1 layout (4 GiB memories, no bounds checks), step 7 measured **−184 µs
  against S** (−219 to −170, `results.md`, campaign E). That base already has:
  - each memory's address kept (cut 1);
  - one Store per voice pool (cut 3);
  - the instance inline (cut 2);
  - the poison check at activations (cut 4);
  - no bounds checks.
  This design has all of those in kind.
- **Added: no copies inside pools.**
  - Port copies cost 393 µs with all cuts (profile build, `results.md`). Voice kernels make 2,095 of the 2,245
    `k_process` calls a block (code-map §6.2).
  - If copy time follows calls: 393 × 2,095 / 2,245 = 367 µs. If a top-level call copies 2× or 4× a voice call's
    bytes: 393 × 2,095 / 2,395 = 344, or 393 × 2,095 / 2,695 = 306.
  - Of a bass voice's ~34 copies a segment, about 1 remains: the gate. The 34 are ~18 inputs from kernels and note
    fields, counted from `std/src/instruments.ts:27-41`, and 16 outputs. Pitch and velocity go once a note. That
    removes about 97%.
  - Saving: 306 × 0.97 ≈ 297 up to 367 × 0.97 ≈ 356 µs.
- **Total:** S − 184 − (297 to 356) ≈ **S − 481 to S − 540 µs**. Widest, with the base's range: S − 467 to S − 575.
  - With S ≈ 7,020 µs, that is about **6,480 to 6,540 µs**.
  - HEAD with P2 is S + 212 (campaign E), so this is about 690 to 750 µs faster than HEAD.
- **Not counted** (unknown sign or size, unmeasured):
  - one instance and one memory per pool, against cut 3's one per kernel;
  - `memory_may_move(false)` letting Cranelift hoist the base (`CL/func_environ.rs:1606-1609`);
  - the runtime's per-copy bookkeeping that goes away;
  - the Spectre guard, which should cost nothing on elided accesses.
- **Caveats:** the 393 µs is from the sampling build and the −184 from lockstep runs. On the Mac, O1's bounds checks
  cost +141 µs against Linux's +85 to +105 (`plan.md`). So dropping them is worth more there, but the Mac must measure.

**Footprint of one p50 renderer (estimate).** This assumes today's remainder is overhead per instance that scales with
the instance count; how those pages split is unverified (external §D3).

*Today:*
- Port buffers: 19,916 × 1,024 B = 20.4 MB.
- `delay` lines: 17 × 96,002 floats × 4 B = 6.5 MB mono, or 13.1 MB stereo (code-map §1.4).
- `reverb` lines: 16 × about 27,700 floats × 4 B = 1.8 MB. The 27,700 is 2 × 12,587 plus 276 spread at 44.1 kHz,
  × 48/44.1 (`src/fx.rs:198-200, 264-269`).
- Small states: under 1 MB (assumed: most are a few floats, such as `Osc { sr, phase }`, `std/kernels-src/src/lib.rs:27-30`; not summed).
- So the remainder is 195 − 20.4 − (8.3 to 14.9) − 1 ≈ 159 to 165 MB, which is 26 to 27 KB per instance.

*This design:*
- Wires: 6,836 × ≤ 1 KiB, plus 1,392 × 0.5 KiB, plus 516 × 1 KiB, plus about 0.5 MB of constants and cells (assumed: about 20 distinct values a pool × 50 pools × 0.5 KiB; not counted from the code) ≈ ≤ 8.7 MB.
- States: 8.3 to 15.9 MB.
- Sandboxes: 200 × 26 to 32 KB = 5.2 to 6.4 MB.
- **Total about 22 to 31 MB, against 195 MB** (177 with all cuts).

*The Mac:*
- Today's remainder: 575 − 20.4 − (8.3 to 14.9) − 1 ≈ 539 to 545 MB, which is 89 to 90 KB per instance.
- This design: 200 × 89 to 95 KB ≈ 18 to 19 MB, plus wires and states (17 to 25 MB), plus 16 KiB page rounding
  (≤ 200 × 2 × 16 KiB = 6.6 MB).
- **Total about 35 to 50 MB, against 575 MB** (17.8 + 17.0 + 0 at the low end; 19.0 + 24.6 + 6.6 at the high end).

**Address space.**
- 200 × 4.0625 GiB = 812.5 GiB a renderer. Two renderers: 1,625 GiB = 1.24% of 131,072 GiB. Today with P2: 0.29%; P1
  on the Mac with 6,058 memories: 37.9% (`docs/architecture.md:330`).
- The ceiling is 4,033 sandboxes a renderer. A p50-like song spends 4 a track (200 / 50), so it fits about 1,000
  tracks. A song with 8 top-level effects a track, plus 2 mixer kernels and 1 pool, spends 11, so it fits about 366.

**Build.**
- S builds the same 6,058 instances in one Store in 181 ms; HEAD takes 216, all cuts 199 (`plan.md`, `results.md`).
- This design builds 200 instances. It makes the same 6,058 inits and prewarm resets, plus 6,058 `k_size` calls:
  ≤ 6,058 × 83 ns ≈ 0.5 ms of host cost, the measured upper bound per call (external §A7). Plus two grows and one page
  touch per sandbox.
- **So at most about S's 181 ms.** How far under is unknown, because S's 181 ms isn't split between instantiation and
  the rest. G7f must measure it.

**Drop** (today 84.6 ms, 76.3 with all cuts, `results.md`): 200 Stores instead of 6,058. Unmeasured; expected lower.

**Activation block:** the same moves as today. The slowest measured is 2.2 to 2.6 ms against the 5.3 ms bound
(`results.md`, G7f).

---

## 12. The cost to build it, and what it leaves for later

| Slice | What | Done when |
|---|---|---|
| 1 | Kit v1, std ported, `verify_module` v1, the package check's relocation and repeat renders. Built and tested natively first, as the kit's `port_mut` already allows (`kernel/src/lib.rs:151-154`) | std's tests pass; std.wasm rebuilds to the same bytes in two folders (PLAT-5) |
| 2 | Sandboxes in the runtime: the layout, the host's grows, touch and seal; top-level kernel boxes; pool boxes with wires in place; the engine layout; RENDER-32 rewritten; G7f against S, P1 and P2 | the 8 renders, the p16 render, G7b (tails), G7c (live = export), `no_alloc` and `churn` all pass; G7f's numbers are recorded |
| 3 | A trap mutes its box; the notice; restart rate-limited as HOST-5 | a trapping test kernel mutes its box while the rest stays bit-identical |
| 4 | ABI v1's reservations: event ports, `asset_read` and the asset table, channels per port, save, load, latency, tail, offline kernels; the guarded engine with epochs | each has a test kernel; the epoch cost is measured on a third-party test package |

Rough size: slice 2 is the largest. It touches about a third of `core/runtime/src/lib.rs` (2,292 lines) and the box
types in `plan.rs`. Slices 1, 3 and 4 are each smaller. **Estimate**, from reading the code.

**Left for later, all host-side (none changes the ABI):**
- **Grouping small top-level states** per module and package. It needs the relocation rule (§13). With it, top-level
  sandboxes fall from one per node to about one per module and package, plus one per large state.
- **An in-sandbox runner** that runs a voice graph in one call. A host-made module in the same Store imports the
  package's `k_process` functions and calls them through `call_indirect`. Calls across instances in one Store need no
  trampoline (`CL/func_environ.rs:1930-1947`).
  - It would cut host→wasm calls from about 2,245 to about 325 a block (150 to 175 voice segments plus 150 top-level,
    code-map §6.2).
  - Wasmtime's host code is 167 µs with all cuts. **Estimate:** 167 × (1 − 325/2,312) ≈ 144 µs saved.
- **No copies at the top level, or for plugins.** Host consumers would read a kernel's output where it is.
- **Voices across threads** (chunk sandboxes, §6).
- **The pooling allocator**, `Module::deserialize_file` (copy-on-write on the Mac, `W/config.rs:2273-2276`), and the
  `cache` feature.
- **Loading state into a kept kernel** (§7.3).

---

## 13. Its weakest point

**Every top-level kernel gets a whole sandbox, and the address space runs out by kernel count, not track count.**
- Each sandbox reserves 4.0625 GiB, and the 25% rule allows about 4,033 a renderer. p50 uses 200. A mix with eight
  effects a track hits the ceiling near 366 tracks, which a large professional session can pass.
- A plain `gain` (a `Smooth`: two floats and a flag, `std/kernels-src/src/lib.rs:353-357, 376`) costs the same 4 GiB reservation as an instrument.
- Edges between top-level kernels still copy.

**The way out is grouping small top-level states per module, and that needs the relocation rule.**
- A new small state is initialised in a scratch sandbox, off the audio thread, and copied into a free slot at the
  activation. That copy is bounded because only small states are grouped.
- So the rule (a state holds no address of itself) has to be part of ABI v1, before third-party code freezes it.
- This design's own host never relocates anything. The rule is exercised only by the package check, so a gap in it, or
  in the check, would surface only when the grouping is built.

Two smaller weaknesses:
- **Kernels of one package in one instrument can corrupt each other.** In-place wires require it.
- **Kernel authors give up `malloc`.** Code ported from C++ that allocates at init has to measure first and take its
  memory from its state block. The kit makes that easy in Rust (`Mem::zeros`); other languages will need a small
  header that does the same, which isn't designed here.

---

## Attack

Label: attack-safe. Research only: nothing was built, run or measured. I read this design, `code-map.md`, `external.md`,
step 7's frozen source and wasmtime 49.0.2's source. Citations as at the top of this note; `W/sync_std.rs` and the
other `W/` paths are under `wasmtime-49.0.2/src`. **Checked** marks a wasmtime fact I read in its source for this
attack. Every estimate shows its arithmetic.

### The verdict in one table

| Area | Verdict | In short |
|---|---|---|
| Live edits | **holds, with 2 fixes** | Step 7's arena really is untouched. But a trapped box can't be reset in place (fix A3), and the sandbox ceiling must count every set the process holds at once (A5) |
| Real-time safety | **holds, with 3 fixes** | No growth, no allocation, no syscall live. But the limiter as written refuses instantiation (A6), doesn't cover table growth, which allocates on the calling thread (A7), and the trap path takes a global lock the design doesn't list (A10) |
| Kernel ABI v1 | **breaks in 3 places, each fixable before the freeze** | A reset wipes what `k_load` loaded and reruns heavy init on the audio thread (A1); wires can't up-mix, and 10 std kernels touch channel 1 whatever their channel count (A2); v1 can't zero outputs at reset (A4) |
| Determinism | **holds** | Layout from shape, zeroed start, no growth, 8a's rules. One path to add to §9.6: a streamed asset's underrun |
| Address space | **holds** for p50 and p50 × 10 | 1.24% and 12.4% for two renderers. About 1,000 p50-like tracks or 366 with 8 effects, as §13 says |
| Footprint | **the estimate breaks; the design doesn't** | §3.1 step 7 makes every sandbox's 128 KiB module part resident: about 43 to 51 MB on Linux, not 22 to 31 (A11) |
| Isolation, third party | **holds, with 2 fixes** | Packages never share a memory. The host must trust no address or count read back from a sandbox (A8), and build-time calls need a deadline (A9) |
| Simplicity | **partly** | Not simpler than today by count. Simpler in what it removes, and the only rival that is simpler now (cut 3) leaves ABI v1 undesigned |
| Numbers | **arithmetic right except the footprint; inputs mixed** | The time saving is better stated as a band (§8 below) |
| **Overall** | **sound with fixes** | Fixes A1 to A11, §9 |

### 1. Live edits

| Claim | Verdict | How, and the fix |
|---|---|---|
| New boxes are built in new sandboxes off the audio thread; kept boxes move by `mem::swap`; cached bases stay valid | **holds** | `take_from` only swaps (`core/runtime/src/plan.rs:688-712`). A memory that grows within its capacity never moves, and wasmtime asserts it (**checked**: `W/runtime/vm/memory.rs:663-664, 711-712`). With `memory_may_move(false)` growth past capacity is refused (`memory.rs:674-678`) |
| Ghosts and fades run old and new boxes together for 10 ms | **holds** | Each ghost keeps its own sandbox; a replaced box costs one extra sandbox for the window. Nothing is shared between the two |
| "A kept box's sandbox isn't entered, copied or rewritten by the activation" | **holds in substance; reword** | The activation writes carried voice-cell values into a kept pool's cell buffers (`cells.carry`, `plan.rs:631`). That is a bounded host write on the device thread, which is fine, but it is a write |
| A2: a poisoned box is reset in place after the host copies back its io tables and cfg blocks | **breaks for a trap** | `k_reset` restores neither the shadow-stack pointer nor the module's own data. std.wasm's stack pointer is a mutable global that isn't exported (176 exports: `memory` and 175 functions, code-map §1.3), so the host can't put it back. LLVM's shadow stack moves that global in function prologues and epilogues (external §C5), and a trap ends the call without its epilogues. So each trap can leave the pointer lower (derived; unverified for which of std's functions keep stack frames). In a pool it is the pointer of every kernel of that package in the instrument. A third-party module may also keep statics that a bug overwrote. The master copies cover neither. **Fix A3**, below |
| "It starts again at your next edit" (§8's wording) | **breaks for a value-only edit** | A value-only edit sends no activation (`crates/server/src/graph/actor.rs:204, 270`; code-map §3.7), so the box stays muted. A3 makes this moot |
| Exports hold two renderers | **holds** | 400 sandboxes, 1.24% (§5) |
| Seeks, resets, loops | **holds in meaning** | A live seek or loop wrap resets nothing (`engine.rs:369-377`); Panic and an offline seek reset. But the reset itself breaks (A1) |
| A package rebuilt with a new hash | **holds in mechanism; breaks at the ceiling** | The module's SHA is in every shape (`plan.rs:35-39, 87-106`), so every box is new and the old ones ghost, then retire. Until `m2-play-drop` drops them (it polls every 5 ms, `core/play/src/lib.rs:693-711`) the process holds both sets, plus an export's. p50: 3 × 200 × 4.0625 = 2,437.5 GiB = 1.9%, fine. But §3.2's 4,033 "per renderer" assumes two sets in all. A song near it can't be rebuilt during an export. **Fix A5** |

**Fix A3: a trapped box is replaced, not reset.**
- It is never called again. Its outputs play silence until the next activation.
- The session puts it in its restarts, and the catch-up activation builds it new, off the audio thread. That is exactly
  HOST-5's path for a plugin that fails live: "the session puts it in `Open.restarts` and says so", with its 10-second
  rule (`docs/architecture.md:1156`). `keep::wire`'s `restarted` (`core/runtime/src/keep.rs:171, 180`) takes plugin
  handles today; it would take boxes too.
- A box that only went non-finite keeps today's in-place reset (`plan.rs:595-604`). Finite-math code can't corrupt
  the io tables.
- So the master copies of io tables and cfg blocks go: one concept fewer.
- The notice leaves the audio thread the way `crashes` does today (`core/runtime/src/engine.rs:576`): a counter or a
  slot sized at build, never an allocation.

**Fix A5: count sandboxes and reserved bytes in the process, not per renderer.**
- One process-wide count, as `LIVE_KERNELS` counts instances today (`core/runtime/src/lib.rs:172-184`). It covers the
  live plan, a plan being built, boxes waiting for `m2-play-drop`, and exports.
- A build that would pass the cap is refused with §3.3's wording; the version before keeps playing.
- Proposal: a cap of 8,066 sandboxes in all, 0.25 × 131,072 / 4.0625 (the 25% rule spent once, not per renderer).
- On the Mac the figure is a little lower. The Mac measured 49,637 GiB for 2 × 6,058 memories
  (`docs/architecture.md:330`): 49,637 / 12,116 = 4.097 GiB each, or 416 GiB more than 4.0625 GiB each predicts
  (code-map §2.3). Either way about 3,980 to 4,000 per renderer, not 4,033.

### 2. Real-time safety

| Claim | Verdict | How, and the fix |
|---|---|---|
| No `memory.grow` succeeds live; the refusal comes before any syscall | **holds** | The limiter is asked first, and a refusal returns `Ok(None)`, so wasm sees −1 (**checked**: `W/runtime/vm/memory.rs:651-657`). The `mprotect` comes only after (`memory.rs:685-707`) |
| The limiter "allows growth only while `host_growing` is set" | **breaks instantiation** | Creating a memory also asks `memory_growing(0, minimum, maximum)`, and a refusal fails with "memory minimum size of 2 pages exceeds memory limits" (**checked**: `memory.rs:344-353`). **Fix A6:** set `host_growing` around `Instance::new` too |
| Nothing else grows | **gap** | A wasm `table.grow` asks the limiter's `table_growing` (`W/runtime/vm/table.rs:651-657`), then resizes a heap vector on the calling thread (`table.rs:693-707`). The on-demand allocator makes every table dynamic (`W/runtime/vm/instance/allocator/on_demand.rs:168`). Step 7 caps the table count, not its elements (`StoreLimits::table_elements` is `None` unless set, `W/runtime/limits.rs:316, 327`). std's Rust code likely never grows a table (unverified); a third-party module can, in `k_process`, and that allocates on the audio thread. **Fix A7:** the sealed limiter refuses table growth too |
| No allocator on the audio thread | **holds** | Wasm can't grow; the kit has no allocating API (§2.5). Host functions don't count as instances, so `instances(1)` still fits with `asset_read` (**checked**: only module instantiation bumps the count, `W/runtime/instance.rs:302`, `W/runtime/store.rs:1523-1537`) |
| No first-touch fault live (§3.1 step 7) | **holds, with a note** | The touch must use volatile writes: a plain write of the byte just read may be compiled away. It doesn't stop reclaim (swap, or the Mac's memory compression); that is the same as today (unverified for the Mac) |
| No locks | **holds in steady play; the fault path takes one** | A hardware trap's handler (`W/runtime/vm/traphandlers.rs:994`), and the Mac's handler thread (`W/runtime/vm/sys/unix/machports.rs:426`), call `lookup_code`, which takes the read side of one global `std::sync::RwLock` (**checked**: `W/runtime/module/registry.rs:401-415`, `W/sync_std.rs:39-46`). The write side is taken whenever a module's code is registered or dropped (`W/runtime/code.rs:135, 267`), that is when the actor compiles a package. So a trap on the audio thread can wait for a compile's registration. **Fix A10:** add it to §4's fault costs |
| The fault is "no worse than today" | **partly** | Today the song stops at a trap anyway (`engine.rs:570-579`). Here it plays on, so the Mac's wait for wasmtime's Mach-port thread (`refresh.md:192`) becomes an audible dropout in a song that keeps playing. With A3 and HOST-5's rule it happens at most twice per bad box per song open (the trap, and one failed restart). Acceptable, but say so |
| Epoch checks are cheap to arm | **holds** | `increment_epoch` is one atomic add, "signal-safe" (**checked**: `W/engine.rs:865-877`). `set_epoch_deadline` loads the current epoch and writes one field (`W/runtime/store.rs:2230-2239`) |
| `tls_eager_initialize` once per worker covers both engines (§6, marked unverified) | **holds** | "Eagerly initialize thread-local functionality shared by all [`Engine`]s", and it takes no engine (**checked**: `W/engine.rs:782-799`) |
| Drop on `m2-play-drop` | **holds, and improves** | Each Store drop deallocates its instances (`W/runtime/store.rs:2425-2457`). 200 unmaps per full retire instead of 6,058 eases `refresh.md`'s risk R15 (unmaps racing real-time threads) |

### 3. Kernel ABI v1

**A1. A reset wipes what `k_load` loaded, and reruns heavy init on the audio thread (breaks).**
- §3.1 step 6 runs `k_init`, then `k_load`, then `k_reset` as the prewarm. §2.2 says `k_reset` puts the state "back as
  `k_init` left it", and §2.5's kit implements it as "zero the slabs and run `new` again". So the prewarm undoes the
  load, and so does every later reset: voice reuse, Panic, a poisoned box, an offline seek.
- `k_init` "may take as long as it needs" and may read assets: §7.2's own example transforms an impulse response into
  the state. A reset reruns `new`, so that work runs on the audio thread at each voice reuse and each Panic, the
  callers §2.2 lists.
- **Fix A1: two parts in every state**, as Faust splits `instanceConstants` from `instanceClear` (external §B).
  - *Constants*: set by `k_init` and `k_load`, from `cfg`, assets and the blob. A reset keeps them.
  - *Running state*: what a reset clears. `k_reset` touches only this, reads no asset, and is bounded.
  - The kit: `new(cfg, mem)` builds the constants and declares which slabs are running; a new `fn clear(&mut self)`
    sets the running fields; the generated `k_reset` zeroes the running slabs and calls `clear`.
  - std: v0's reset is `K::new` (`std/kernels-src/kernel/src/lib.rs:156-157`). For std's kernels `clear` sets the
    same fields to the same values, so the 8 renders and the p16 render stay the gate.

**A2. Wires in place can't up-mix, and 10 std kernels touch channel 1 whatever their channel count (breaks).**
- Today's copy is also an up-mix. A mono source feeding a stereo input is copied into both channels:
  `let sc = if c < *src_channels as usize { c } else { 0 }` (`core/runtime/src/lib.rs:353-355`; the voice sum does the
  same, `lib.rs:1438-1441`). §2.3 gives one address per port and a fixed channel stride, so a stereo consumer can't read
  a mono wire's channel 1 in place. p50's voices are all mono (code-map §6.1), so §11's estimate stands, but any stereo
  voice graph fed from a mono source needs either a copy or a new rule.
- Ten std kernels address channel 1 of an `audio<*>` input and output whatever `io.channels` is: `gain`
  (`std/kernels-src/src/lib.rs:388`), `duck` (`:575`), `limit` (`:604`), `reverb` (`src/fx.rs:287`), `chorus` (`:361`),
  `phaser` (`:422`), `tremolo` (`:471`), `compress` (`:512`), `eq3` (`:571`), `bitcrush` (`:633`); all ten have an
  `audio<*>` output (`std/m2pkg.toml`). v0 hides this: every port has two channels (`kernel/src/lib.rs:47, 143`) and the
  host copies out only the declared ones (`lib.rs:361-373`). If wires are sized by their channels (§7.4), these kernels
  index past a mono wire: the kit's assert traps (`kernel/src/lib.rs:112`), or a kit without it writes into the next
  wire.
- So "25 kernels change only in `new`'s signature" (§2.5) is wrong for these ten.
- **Fix A2:**
  - The io table gives one address per port *channel*. A mono source's one channel serves both channels of a stereo
    input: the up-mix with no copy.
  - The kit's rule: a kernel touches only the channels its port has.
  - The ten kernels loop over `io.channels`. `limit` already ignores `r` when mono (`lib.rs:602-611`), so its channel 0
    can't change; the renders gate the other nine.

**A3** is in §1. **A4. Outputs after a reset (gap).**
- v0's reset zeroes every output port (`kernel/src/lib.rs:158-161`). v1's `k_reset(state)` can't: the outputs are wires
  the state doesn't know.
- So the bits stay the same only if every kernel writes every frame of `[from, to)` of every output channel at every
  call.
- **Fix A4:** make that an ABI rule. The package check fills each output with a sentinel before each call and refuses a
  kernel that leaves one. For std, the renders check it.

**Event ports (gap, part of A8).** In place, nobody is said to clear an event wire, segments aren't said to append, and
frames aren't said to be block frames. Proposal: the host sets `count` to 0 at each block line; a kernel appends, with
block frames; the host reads `cap` from its own copy and clamps `count` to it.

**Offline kernels (should fix before the freeze).** "Whole buffers in and out" (ABI-2, `docs/architecture.md:1176`) in a
32-bit memory: a stereo f32 input and output take 2 × 2 × 4 = 16 bytes a frame, so a box holds at most
2^32 / 16 = 268 M frames: 93 minutes at 48 kHz, 23 at 192 kHz, before the time map and the module's part. Proposal:
offline kernels take chunks (a large `max_block`, the time map, state carried between calls), with the same exports.

**Smaller ABI points.**
- Latency and tail are read at build (§7.4). Say they depend on `cfg` only: a latency driven by a cell can't change
  without a new box.
- Allow `asset_read` in `k_size`: an impulse response trimmed of its silent end is sized by its content.
- `k_ports` packs `inputs << 8`, `cfg.ports` packs `inputs << 16`. Pick one.
- `cfg`'s variable-length arrays sit mid-block, so a v1.1 field lands after the blob. A fixed header with an offset to
  each array is simpler to extend.

**The relocation rule (§2.4, §13): holds as stated, but it is the owner's call.**
- Its only user is grouping. §5 below shows a way past the ceiling that needs no relocation and no ABI change.
- It burdens C and C++ ports, where a pointer to one's own buffer is normal.
- It is checked only by test renders with moved states, which can miss a path.
- Recommendation: drop it from v1, or make it advisory (a warning, not a refusal).

### 4. Determinism

| Claim | Verdict | How |
|---|---|---|
| Layout is a pure function of shape, rate and package bytes | **holds** | Cell values aren't in a pool's shape (`plan.rs:87-106`) and don't enter `k_size`, so a kept pool's layout is a fresh one's |
| Every sandbox starts as zeroed pages plus the module's data | **holds** | No slot is reused across boxes. Inside a box, voice reuse resets in place, the same in live and export |
| No growth, so no failure mid-render | **holds** | Budgets are met before the first block |
| The same bits on any thread count | **holds** | A box is in one task; voice order and 8a's sums are unchanged. 8a's wording should say the device thread also enters sandboxes (pre-pass cells, `can_idle`'s top-level `k_active`, Panic; code-map §7), ordered by the task's completion (`design-8a.md` §5.4): one thread at a time, with happens-before at each hand-off |
| 8a's failure rule | **changes** | `design-8a.md` §4.3 skips a failed task's consumers and breaks the plan. This design mutes the box and goes on. Amend §4.3 with this design |
| Faults are the only timing-dependent path (§9.6) | **holds, add one** | A streamed asset read outside its window returns 0 bytes live and waits offline (§7.2). That is a second timing-dependent path. List it and tell the user, as for a deadline |
| With A2, the up-mix | **holds** | It becomes an address the host chooses at build, from the shape |

### 5. Address space and footprint

**Address space (holds).**
- Two p50 renderers: 2 × 200 × 4.0625 = 1,625 GiB = 1.24% of 131,072 GiB. Checked against the source: a memory reserves
  pre-guard + reservation + guard (`W/runtime/vm/memory/mmap.rs:62-69, 102-105`), 32 MiB + 4 GiB + 32 MiB.
- **p50 × 10** (estimate, code-map §6.2 times ten): 2,000 sandboxes a renderer (500 pools; 340 bass `drive` and `delay`;
  160 pad `reverb`; the song's 500 `gain`, 499 `add` and 1 `limit`).
  - Two renderers: 4,000 × 4.0625 = 16,250 GiB = 12.4%. Holds.
  - A std rebuild during an export (three sets): 24,375 GiB = 18.6%. Holds.
- **p50 × 20:** 4,000 a renderer, at the ceiling. Two renderers take 24.8%; a rebuild during an export, 37%. Breaks.
  So the limit is about 1,000 p50-like tracks, or 366 with 8 effects a track (§13's own figure, which holds).

**The mixer spends most of the top-level sandboxes.** In p50 the song module takes 100 of the 150: 50 `gain`, 49 `add`
in a chain (`std/src/mix.ts:33-41`), 1 `limit` (code-map §6.1). Owner's call, no ABI change: an 8-input `sum` in std
turns the 49 `add`s into 8 kernels (7 for 50 inputs, then 1 for those 7).

**A way past the ceiling with no relocation (owner's call, host-only).**
- std's top-level boxes with small states could use a checked layout on a second std engine: O1's kind of memory with a
  reservation of the box budget, Spectre off since std is trusted. Then only pools and other packages' boxes spend
  4 GiB, and the ceiling counts instruments, not effects.
- Bounds checks change no bits: every layout gave the same hashes (`results.md`, "No sound changed").
- Its cost lies between O1's share of the top-level calls and all of O1:
  - if it follows calls: 150 / 2,245 = 6.7% (code-map §6.2), and 0.067 × (85 to 105) = 5.7 to 7.0 µs on Linux;
  - at most O1 itself: +85 to +105 µs on Linux, +141 on the Mac (`plan.md`);
  - plus a second compile of std (time unmeasured).
- It needs no ABI change, so it can wait for a song that needs it. That is why the relocation rule isn't needed for the
  ceiling (§3).
- Not for third-party boxes: the Spectre guard must be on for them, and on checked accesses it cost +473 µs a callback
  (RENDER-32's Linux measurement, `docs/architecture.md:328`).

**Footprint (A11: the estimate breaks; the design doesn't).**
- §3.1 step 7 touches every page of the memory, "the module's pages included". std.wasm's own part is 2 wasm pages,
  131,072 B (code-map §1.3), so each sandbox keeps at least that much resident.
- 200 × 131,072 B = 26.2 MB, where §11 counts 200 × 26 to 32 KB = 5.2 to 6.4 MB. That was today's per-instance
  remainder, in which most stack pages were never touched.
- Corrected, Linux: 8.7 (wires) + 8.3 to 15.9 (states) + 26.2 = **43.2 to 50.8 MB**, plus wasmtime's own structures for
  each Store and instance (unmeasured).
- Corrected, Mac: 26.2 + 17.0 to 24.6 (wires and states) + 0 to 6.6 (16 KiB rounding) = **43.2 to 57.4 MB**, plus the
  same unmeasured part.
- Still about 4× under Linux's 195 MB and 10× under the Mac's 575 MB.
- p50 × 10: 432 to 508 MB on Linux, 432 to 574 MB on the Mac (×10).
- A smaller std stack (64 KiB today, `std/kernels-src/.cargo/config.toml:2-3`, chosen because "each node gets its own
  instance") is a later saving.

### 6. Isolation and third-party packages

| Claim | Verdict | How, and the fix |
|---|---|---|
| Another package can't be reached | **holds** | One package a memory. Elision needs a 32-bit memory with reservation + guard ≥ 4 GiB (**checked**: `E/types.rs:2399-2407`), so every computable address lies in the box's own reservation. The Spectre guard "is a no-op for any heap that needs no bounds checks" (**checked**: `CG/shared/settings.rs:290-302`) |
| A bad kernel harms only its box | **holds, with A8** | Only if the host never reads an address, count or size back from sandbox memory. **Fix A8:** wire addresses and io tables come from the host's own copies; an event wire's `cap` from the host's copy, and `count` clamped to it; `k_size`'s answer checked against the budget (as §3.2 says) |
| Epochs bound a guarded kernel's time | **holds live; gap at build** | "Epochs are designed to handle malicious WebAssembly guests"; `memory.copy` checks once at its start (**checked**: `W/config.rs:744-762`). But deadlines are set "at each block" (§8). `k_size`, `k_init` and `k_load` of a guarded package run on the actor with none, so a hostile or buggy init hangs the actor and every edit stops. **Fix A9:** set a deadline before every guarded call, build included (proposal: the offline 10 s) |
| "Tried to allocate" names the cause | **may mislead** | It is shown when `refused_at` is set. A panic's own path may allocate too (unverified for Rust's wasm std), and the message would then blame allocation for a panic. Word it "trapped (it had also tried to allocate)" |
| Kernel authors give up `malloc` | **holds, and it costs** | §13 says so. Faust's output fits (`dspMeta.size`, external §B). C++ ported code mostly won't without the header §13 leaves undesigned |

### 7. Simplicity

**Concepts this design adds and removes** (counted from its own sections):

| Adds, because of the layout | Adds, needed by ABI v1 or third-party code in any layout | Removes |
|---|---|---|
| 1. the sandbox: one Store per box per package (replaces one per instance) | asset blobs, `asset_read` and the asset table | the Store per kernel instance |
| 2. the state block, `k_size` and the kit's measuring pass | event buffers | O1's tunables, its bounds checks, the 32 MiB cap per kernel |
| 3. the wire and `[from, to)` | channel counts per port | the kernel's own allocator, the prewarm that makes later resets reuse its memory, v0's 2× peak at a reset |
| 4. the io table and the cfg block | save and load | cut 1's "the memory moved" check |
| 5. the host region's layout | latency and tail | `k_port`, the port `Vec`s, the copies inside pools |
| 6. the build order: two grows, touch, seal | offline kernels | `static mut HOST` |
| 7. budgets per box and per renderer, the sandbox ceiling | two engines and epochs | the string match in `out_of_memory` (`crates/server/src/graph.rs:382-387`) |
| 8. the relocation rule and its check | a trap mutes its box; restarts | a trap silences the whole song |
| 9. master copies of io tables and cfg blocks | typed budget errors | |

- With A3 (master copies go) and the relocation rule dropped, the layout adds 7 and removes 8.
- A1 replaces "reset reruns `new`" with constants and running state: no more concepts, a clearer one. A2 folds into the
  io table.
- The ABI-v1 column must be designed in any layout.

**Against the rivals:**

| | Today (step 7) | This design | Cut 3: one Store per voice pool, ABI v0, O1 | One Store per track, ABI v0 |
|---|---|---|---|---|
| p50's Stores / memories | 6,058 / 6,058 | 200 / 200 | 200 / 6,058 (code-map §6.3) | 51 / 6,058 |
| Bounds checks | yes | none | yes: 6,058 memories can't each reserve 4 GiB under the 25% rule (4,033 at most, code-map §2.3) | yes |
| A replaced node's memory is freed when it retires | yes | yes | yes: a pool is built, moved and dropped whole (`results.md`, cost 2) | **no**: only when the track's Store drops (`W/runtime/store.rs:139-145`). Each edit leaves its replaced instances in the track's Store, unless the whole track is replaced, which cuts its tails (RENDER-25) |
| Copies inside pools | yes | none | yes | yes |
| Kernels may allocate | yes | no | yes | yes |
| ABI v1 | to design | designed here | still to design, in whatever shape | still to design |
| Linux callback against S | +212 (`results.md`, E) | estimate S − 470 to S − 540 (§8) | cuts 1 to 4 at P2, measured: −139 (`results.md`, E) | unmeasured |
| Layout concepts added | — | 7 to 9 | 1 | 1 |

**Verdict on simplicity.**
- Not simpler than today by count.
- Simpler where today is fragile: no allocator inside kernels, no growth, no moving memory, no per-kernel cap, one
  failing box instead of a silent song.
- Cut 3 is simpler now, but it leaves 6,058 memories with bounds checks and copies, and ABI v1 still to design. v1 is
  where the layout freezes (task facts).
- One Store per track fails RENDER-32's own reason, so it isn't a rival.

### 8. The numbers

| Estimate | Arithmetic | Verdict |
|---|---|---|
| Callback S − 481 to S − 540 µs (§11) | 184 + 297 = 481; 184 + 356 = 540. 393 × 2,095 / 2,245 = 367; × 2,095 / 2,395 = 344; × 2,095 / 2,695 = 306; × 0.97 = 297 to 356 | **arithmetic right; inputs mixed.** (1) The 393 µs is the profile column "all cuts P2 (HV1+HV3+HV4)", not all four cuts, and P2, not the P1 base (`results.md`). (2) The copy row moves with the layout and no code change: HEAD's is 475 µs at P1 and 408 at P2, and "the rows don't move one to one" (`results.md`). That 67 µs gap alone widens the band to 481 − 67 = 414 up to 540 + 67 = 607. (3) The −184 base includes cut 4 (−14, `results.md` C2), but §8 keeps the per-block non-finite check, and `plan.md` says the prototyped cut 4 isn't exact. Without it, S − 467 to S − 526. Say "about 400 to 600 µs under S on Linux; the Mac must measure" |
| Footprint 22 to 31 MB; Mac 35 to 50 | §11 | **breaks**: 43.2 to 50.8 MB and 43.2 to 57.4 MB, plus wasmtime's own (§5, A11) |
| Address space 1.24%; today's P2 0.29%; P1 on the Mac 37.9% | 1,625 / 131,072; 2 × 190.05 / 131,072 | **holds** |
| Ceiling 4,033 a renderer | 16,384 / 4.0625 | **holds on Linux**; about 3,980 to 4,000 on the Mac (§1, A5) |
| 366 tracks at 11 sandboxes a track | 4,033 / 11 = 366.6 | **holds** |
| Build at most S's 181 ms | 6,058 × 83 ns = 0.50 ms of `k_size` host cost | **not shown.** S's 181 ms isn't split. Cuts 1 to 4 (200 Stores) built in 199 ms against HEAD's 220 (`results.md`, G7f), so Stores aren't the cost; 30× fewer instances and memories should be. The `k_size` figure counts host cost only, not the measuring `new`'s JIT time, and the build now also touches 43 to 51 MB. Plausibly under 181 ms, unproven: G7f decides |
| Per track would save about 26 µs more (§0) | 0.067 × 393 = 26.3 | **holds**; with §11's own 4× weighting it is 393 − 306 = 87 µs |
| An in-sandbox runner saves about 144 µs (§12) | 167 × (1 − 325 / 2,312) = 143.5 | **holds**; if the 59.8 `k_active` and 7.2 `k_reset` calls a block stay host calls, 167 × (1 − 392 / 2,312) = 139 µs |
| 97% of the copies inside pools go | 1 of a bass voice's ~34 copies a segment remains | **holds for p50** (all-mono voices); lower where A2's up-mix would need copies, none with A2's fix |
| Drop "expected lower" | — | **plausible**: one memory a box instead of one a kernel |

### 9. The fixes, and the verdict

**Before slice 1 (they shape the ABI):**
- **A1.** States in two parts: constants, kept by a reset; running state, cleared by it. `k_reset` reads no asset.
- **A2.** One io address per port channel; a mono wire serves both channels of a stereo input. A kernel touches only
  its port's channels; fix std's ten kernels.
- **A4.** Every output frame of `[from, to)` is written at every call; the package check enforces it.
- Offline kernels in chunks; latency and tail from `cfg` only; `asset_read` allowed in `k_size`; one port encoding; a
  `cfg` header with offsets.

**Before slice 2 (the runtime):**
- **A3.** A trapped box is never called again and is built new through HOST-5's restart path. No master copies.
- **A5.** One process-wide count of sandboxes and reserved bytes, checked at build.
- **A6.** The limiter allows the memory's creation inside `Instance::new`.
- **A7.** The sealed limiter refuses table growth too.
- **A8.** The host trusts nothing it reads from a sandbox: addresses and caps from its own copies, `count` clamped.
- **A10.** §4's fault costs add the global code-registry lock, and the Mac's dropout in a song that now plays on.
- **A11.** Correct §11's footprint, and the summary table's.

**Before slice 4 (third-party code):**
- **A9.** A deadline before every guarded call, build included.

**The owner's calls:**
- Drop the relocation rule, or make it advisory (§3).
- Keep in reserve a checked layout for std's small top-level boxes, if a song nears the ceiling (§5).
- An 8-input `sum` in std, for the mixer (§5).

**Verdict: sound with fixes.**
- The core holds. One sandbox per box fits step 7's arena exactly. No Store is entered by two threads. Nothing grows,
  allocates or makes a syscall live. Packages never share a memory. The bounds checks go, and with them the Spectre
  guard's cost.
- What breaks is in the details of ABI v1 (A1, A2, A4) and in the runtime's edges (A3, A5 to A8). Each has a contained
  fix, and none changes the unit.
- The ABI fixes must land before v1 freezes, since after that they cost every kernel author.
