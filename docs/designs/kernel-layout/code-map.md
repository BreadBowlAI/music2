# Code map: how music2 runs kernels today (step 7, `ed7fc8e`, plus cut 1)

Label: code-map. Read-only. Nothing was built, run or measured for these notes.

How to read the citations:
- A bare path such as `core/runtime/src/lib.rs:200` is under `$SCRATCH/idea3/src` (the frozen copy of step 7).
- `cut1.diff:N` is a line of `$SCRATCH/idea3/cut1.diff`.
- `wasmtime-49.0.2/…`, `wasmtime-environ-49.0.2/…` are under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.
- `results.md` is `$SCRATCH/speed/prof/results.md`, `plan.md` is `$SCRATCH/speed/plan.md`, `refresh.md` is `$SCRATCH/s8a/refresh.md`.
- A fact about `std/kernels/std.wasm`'s binary layout comes from a small read-only section parse I ran on the file (sections, memory limits, globals, exports, data segments). No wasm tool is installed. These facts are marked "(parsed)".
- "unverified" means I could not check it against source on this machine.
- Every estimate shows its arithmetic, and starts only from measured numbers or counts taken from the code.

---

## 0. Summary

- **One module holds every std kernel.** `std/kernels/std.wasm` has 29 kernel types (`std/m2pkg.toml`, 29 `[[kernel]]` entries). Each instance instantiates the whole module and uses one kernel's six exports.
- **A kernel's state is a `static mut` in the module.** It is one `Option<Host<K>>` per kernel type. So one wasm instance can hold only one state of each kernel type (`std/kernels-src/kernel/src/lib.rs:181`). ABI v0 has no handle argument. One instance is one node, or one node in one voice.
- **Ports live in the kernel's own memory.** Each is a `Vec<f32>` of 2×128 floats on the kernel's heap. The host copies inputs in and outputs out at every call (`core/runtime/src/lib.rs:332-375`).
- **Each kernel instance has a `Store` of its own (RENDER-32).** So p50 has 6,058 Stores, 6,058 memories and 6,058 instances of std.wasm. Each Store also holds a dummy "default caller" instance (`wasmtime-49.0.2/src/runtime/store.rs:779-816`).
- **The 6,058 is exact from the code.** 5,908 of them are voice kernels inside 50 voice pools. 150 are top-level: 50 in the song module, 100 in the tracks (§6).
- **Memory.** A kernel allocates only in `init` and `reset`, through Rust's allocator, which grows memory with `memory.grow`. `reset` builds the new state before it drops the old. So the build's `prewarm` runs every kernel's first reset off the audio thread.
- **A grow refused at 32 MiB** returns -1, Rust aborts, and wasmtime traps. If that happens while the plan is built, the user sees "couldn't get it ready to play", with a detail that doesn't say "memory". If it happens on the audio thread, the whole renderer goes silent and the transport stops, with no message (§3.7).
- **8a.** Partitions are per module. A Store is entered by whichever engine thread runs its partition in that block. It is built on the actor's thread, moved to the device thread inside a `Box<Renderer>`, swapped between plans on the device thread, and dropped on `m2-play-drop` (§7).

---

## 1. Kernel ABI v0

### 1.1 Exports and imports

| Export (kernel `k`) | Signature | What the Rust kit does | Runtime caller |
|---|---|---|---|
| `m2_abi` | `() -> i32` | returns `ABI = 0` (`kernel/src/lib.rs:44, 224-232`) | `verify_module`, once per package load, in a throwaway `Store` (`core/runtime/src/packages.rs:495-500`) |
| `memory` | memory export | the Rust linker's default memory | `get_memory(.., "memory")` (`lib.rs:213`); checked present (`packages.rs:492-494`) |
| `k_init` | `(f32 sample_rate, i32 max_block, i32 channels)` | `HOST = Some(Host::new(..))` (`kernel/src/lib.rs:190-193`) | `KernelInst::new`, always with `max_block = BLOCK = 128` (`lib.rs:214`) |
| `k_ports` | `() -> i32` | `INPUTS << 8 \| OUTPUTS` (`kernel/src/lib.rs:195-198`) | checked against the manifest's counts (`lib.rs:216-225`) |
| `k_port` | `(i32 index) -> i32` | the address of port buffer `i`: inputs first, then outputs; 0 when out of range (`kernel/src/lib.rs:147-149, 200-203`) | read once per port at build (`lib.rs:226-233`) |
| `k_reset` | `()` | `kernel = K::new(..)`, output ports zeroed, inputs untouched (`kernel/src/lib.rs:156-162`) | §3.4 |
| `k_process` | `(i32 frames)` | runs `K::process` on an `Io` over the ports; `frames` is clamped to `max_block` (`kernel/src/lib.rs:168-172`) | `KernelInst::process` (`lib.rs:279-286`) |
| `k_active` | `() -> i32` | `K::active()`; defaults to false (`kernel/src/lib.rs:61-63, 215-218`) | voice-ending decisions and `can_idle` (§3.5) |

- **Imports: none.** `verify_module` refuses a module with any import (`packages.rs:470-474`). Every instance is made with `Instance::new(&mut store, module, &[])` (`lib.rs:205`). A kernel has no clock, no host calls and no WASI (`kernel/src/lib.rs:3-6`).
- **Signatures** are checked per declared kernel when the package loads (`packages.rs:501-509`).
- **Version:** `KERNEL_ABI = 0` (`packages.rs:467-468`). A module that says another version is refused (`packages.rs:497-500`; ABI-3, `docs/architecture.md:1180`).
- **Panics:** the kernels build with `panic = "abort"` (`std/kernels-src/Cargo.toml`, `[profile.release]`). On `wasm32-unknown-unknown` an abort is the `unreachable` instruction (unverified here: the Rust std source isn't on this machine). That traps as `wasm trap: wasm \`unreachable\` instruction executed` (`wasmtime-environ-49.0.2/src/trap_encoding.rs:95, 138`).

### 1.2 Ports and port buffers

- **Size.** Every port buffer holds `PORT_CHANNELS × max_block` floats, `PORT_CHANNELS = 2`, so 2×128×4 = 1,024 bytes (`kernel/src/lib.rs:20-22, 47, 143`). The runtime's `PORT_BYTES` is the same number (`lib.rs:80-81`).
- **Layout.** Channel-major: channel `c` starts at float `c × max_block` (`kernel/src/lib.rs:20-21, 83-86`). Control ports use channel 0 only. `channels` (1 or 2, clamped, `kernel/src/lib.rs:136`) is the count resolved for the kernel's `audio<*>` ports. It is the IR's `FOp::Kernel { channels }`, which is passed to `init` (`core/ir/src/check.rs:68-69`).
- **Where.** Each port is its own `Vec<f32>` on the kernel's heap (`Host.ports: Vec<Vec<f32>>`, `kernel/src/lib.rs:130, 143`). `k_port` returns its pointer as a wasm address. Inputs and outputs are disjoint `Vec`s, so the kit can't overlap them (`kernel/src/lib.rs:28-31`).
- **Host checks, in every build** (`lib.rs:226-248`):
  - each address is nonzero, 4-aligned, and `p + 1024 ≤ memory.data_size` at build;
  - no two ports overlap (ABI-1), with an error that names both ports.
- **Inputs belong to the host (ABI-1, `docs/architecture.md:1166-1168`).**
  - A constant or unconnected input (`Input::Const`) is written once at build: both channels, the whole block (`lib.rs:250-256`).
  - A connected input is copied in before each call (`lib.rs:343-359`). From a steady constant (a cell) it is copied once per value, over the whole block, and skipped while `seen == version` (RENDER-22). From anything else it is copied for the segment's frames only.
  - A kernel always sees its frames starting at index 0 of each channel. The host writes `port[0..n]` and reads `out[0..n]`, then places them at `seg.off` in its own buffers (`lib.rs:352-355, 363-366`).
- **Outputs** are copied out for each channel the manifest declares (`out_channels`, `lib.rs:268, 361-373`).
- **So every port's samples exist twice:** in the host's `Buf` (`Graph.bufs[node][port]`, `channels × 128` floats, `lib.rs:790-791, 1065-1067`) and in kernel memory. The measured cost of those copies is 408 µs a 512-frame callback on Linux (task facts).

### 1.3 Where a kernel keeps its state

| Layer | What | Where |
|---|---|---|
| per kernel type, per instance | `static mut HOST: Option<Host<K>>` | in the module's data or bss, one per `export!` (`kernel/src/lib.rs:179-188`). std.wasm has 29 of them, so each instance carries 29 slots and initialises one |
| `Host<K>` | `kernel: K`, `sample_rate`, `channels`, `max_block`, `ports: Vec<Vec<f32>>` | `kernel/src/lib.rs:125-131` |
| `K` | the DSP state struct | plain fields for most kernels, e.g. `Osc { sr, phase }` (`std/kernels-src/src/lib.rs:27-30`) and `Svf { sr, ic1, ic2 }` (`lib.rs:436-440`) |
| heap | `Vec`s inside `K`, and the port `Vec`s | only `delay` (`lines: Vec<Vec<f32>>`, `std/kernels-src/src/lib.rs:481-496`), `reverb` (comb and allpass buffers, `src/fx.rs:243-245, 264-283`), `chorus` and `widen` (a `Line` each, `src/fx.rs:144-152, 335-357, 667-685`). Every other std kernel's `K` holds no `Vec` (a grep of `Vec\|vec!` in `src/lib.rs` and `src/fx.rs`) |
| allocator | Rust std's global allocator for `wasm32-unknown-unknown` (dlmalloc), which takes memory with `memory.grow` | unverified here: the toolchain has only the compiled `libdlmalloc-*.rlib` (`~/.rustup/toolchains/1.99.0-x86_64-unknown-linux-gnu/lib/rustlib/wasm32-unknown-unknown/lib/`) |

std.wasm itself (parsed):
- 143,431 bytes.
- 304 functions; 176 exports, of which 175 are functions: 1 `m2_abi` plus 29×6.
- One memory: minimum 2 pages (128 KiB), no maximum. Only the host caps it.
- One mutable i32 global, initialised to 65,536. That is the shadow stack pointer: `-zstack-size=65536` (`std/kernels-src/.cargo/config.toml:2-3`), so the stack is `[0, 64 KiB)`.
- Two active data segments at 65,536 and 73,736, 9,808 bytes in all.

### 1.4 When it allocates or grows memory

| When | What allocates | Thread |
|---|---|---|
| `k_init` | `Host::new`: one 1 KiB `Vec` per port, plus `K::new` (`kernel/src/lib.rs:135-145`) | the thread building the renderer (§7) |
| `k_reset` | `K::new` again (`kernel/src/lib.rs:157`). The new state is built while the old one is still alive, then the old one drops. So an allocating kernel briefly needs 2× its heap (`lib.rs:1294-1296`; cut 1's comment, `cut1.diff:94-97`) | build (prewarm), or the audio thread (§3.4) |
| `k_process` | never, for std (`std/kernels-src/src/lib.rs:3`). The kit's `Io::ports` asserts, so misuse panics, which aborts and traps (`kernel/src/lib.rs:110-113`) | — |

- **Prewarm.** `Graph::prewarm` resets every kernel once, voice kernels included, inside `Renderer::build` (`lib.rs:1294-1310`, called at `lib.rs:1629`). Every later reset then reuses the freed block and doesn't grow the memory (`lib.rs:29-31`).
- That reuse is the allocator's behaviour, not a guarantee of the ABI (unverified as a property of dlmalloc). It is checked by tests: `core/play/tests/no_alloc.rs:312` ("no kernel grew its memory on the audio thread, nor after the panics") and `core/runtime/src/tests/live.rs:485-487` ("eight steals later").
- **Worst case in std.** `delay` at the highest allowed rate. Rates go up to 384 kHz (`core/song/src/manifest.rs:32`).
  - One line is `2.0 × 384,000 + 2 = 768,002` floats (`std/kernels-src/src/lib.rs:494`).
  - Stereo is 2 × 768,002 × 4 = 6,144,016 bytes.
  - During a reset two copies exist: 12,288,032 bytes, about 11.7 MiB.
  - That is under the 32 MiB cap, so no std kernel reaches the cap at any allowed rate. Kernels from other packages could.

### 1.5 What reset does

- **Kernel side.** It rebuilds `K` from `(sample_rate, channels)` and zeroes the output ports. Inputs keep the host's values, so a cell written once stays written (`kernel/src/lib.rs:156-162`; ABI-1).
- **Host side, debug builds.** Input memory is compared before and after every `k_reset` and `k_process` (`lib.rs:288-296, 303-326`). A mismatch fails, naming the kernel and the port (`lib.rs:316-322`). That costs a 1 KiB copy per input per call, in debug builds only. Test: `every_std_kernel_keeps_its_inputs` (`core/runtime/src/tests/cell.rs:495`).
- **Which ABI-2 reservations are missing in v0** (`docs/architecture.md:1170-1178`):
  - event ports: the checker refuses events into a kernel (`core/ir/src/check.rs:930`; the doc's `:731-732` is stale);
  - asset blobs;
  - channel counts per port: one `channels` for all `audio<*>` ports;
  - state save and load;
  - latency and tail queries: a manifest `latency` field exists (`packages.rs:44-45`), but no export;
  - offline kernels.

---

## 2. Packaging and loading

### 2.1 One module for every std kernel

- `std/m2pkg.toml` sets `wasm = "kernels/std.wasm"` and declares 29 kernels with their ports.
- The Rust source is one cdylib crate: `std/kernels-src/src/lib.rs` has 21 `export!` lines and `src/fx.rs` has 8.
- It is built for `wasm32-unknown-unknown` with `opt-level = 3`, `lto`, `codegen-units = 1`, `panic = "abort"`, `strip` (`std/kernels-src/Cargo.toml`). `std/build.sh` rebuilds it reproducibly (PLAT-5).
- The 29 types:
  - oscillators: sine, saw, square, tri, noise;
  - modulation: adsr, follow, lfo;
  - math: scale, add, blend, mtof, range, xrange;
  - mixing: gain, pan;
  - filter: svf;
  - effects: delay, drive, duck, limit, reverb, chorus, phaser, tremolo, compress, eq3, bitcrush, widen.

### 2.2 From kernel id to code

| Step | Code |
|---|---|
| A session opens. `packages()` makes a new `PackageSet::default()` (a new `Engine`) and loads `std` | `crates/server/src/graph/actor.rs:384-388`, called at `actor.rs:1082-1084` |
| `PackageSet::load` reads the manifest, then `Module::new(&engine, &bytes)`: one compile of the whole wasm per load. Then `verify_module` | `packages.rs:162-205` (compile `:183`) |
| Each `[[kernel]]` becomes `kernels["std:<name>"] = (KernelSig, "std")` | `packages.rs:190-194` |
| `module_for("std:svf")` returns the package's single `Module` and the export prefix `svf` | `packages.rs:324-329` |
| `KernelInst::new` makes a new `Store`, instantiates that module, and looks up `{prefix}_init` … `{prefix}_active` | `lib.rs:200-213` |
| `module_sha(id)` is the SHA-256 of the whole package wasm. It is part of every kernel's `Shape` | `packages.rs:318-322`; `plan.rs:35-39, 64-66` |

Consequences:
- Rebuilding std.wasm changes every std kernel's shape. The next activation then starts every std instance from init (RENDER-25's purpose, `docs/architecture.md:253`).
- Each instance links the whole module: its table, its 29 `HOST` slots, and its data segments.
- The compiled code is shared by every instance through the one `Module`.
- `cache` and `parallel-compilation` aren't among the wasmtime features (`Cargo.toml:39`: `default-features = false, features = ["anyhow", "cranelift", "runtime", "std"]`). So each session open compiles std.wasm again, on one thread. How long that takes is unmeasured.

### 2.3 The engine (`engine_with`, `packages.rs:103-133`)

| Setting | P2 (the app, `engine()` → `engine_with(Layout::P2)`, `packages.rs:109-111`) | P1 (gate-only, `packages.rs:97-100`) |
|---|---|---|
| `cranelift_nan_canonicalization` | false (`:117`) | false |
| `wasm_relaxed_simd` | false (`:118`) | false |
| `memory_reservation` | `KERNEL_MEMORY` = 32 MiB (`:120`; `lib.rs:87-88`) | wasmtime's default, 4 GiB (`wasmtime-environ-49.0.2/src/tunables.rs:326`) |
| `memory_guard_size` | 64 KiB (`:121`) | default 32 MiB (`tunables.rs:327`) |
| guard before the memory | default true, which on the unix mmap path makes it equal to the guard: 64 KiB (`tunables.rs:266`; `wasmtime-49.0.2/src/runtime/vm/memory/mmap.rs:64-68`) | 32 MiB |
| `memory_reservation_for_growth` | 0 (`:122`) | default 2 GiB (`tunables.rs:333`) |
| `memory_may_move` | false (`:123`) | default true (`tunables.rs:265`) |
| Spectre heap guard | off (`:124-130`) | on |
| bounds checks | explicit, against a fixed bound | elided: the hardware traps on the guard |
| pooling allocator | not enabled (the `pooling-allocator` feature isn't in `Cargo.toml:39`); on-demand allocation | same |

Address space per memory:
- P2: 64 KiB + 32 MiB + 64 KiB = 32.125 MiB. That times 6,058 memories is 194,613 MiB = 190.05 GiB, which matches the measured "reserves 190 GiB".
- P1: 32 MiB + 4 GiB + 32 MiB = 4.0625 GiB.
  - That times 2 × 6,058 is 49,221 GiB. The Mac measured 49,637 GiB (RENDER-32's text).
  - The 416 GiB difference isn't accounted for here: unverified.
- The 25% rule allows 0.25 × 131,072 GiB = 32,768 GiB for two renderers.
  - Under P1 that is 32,768 / 4.0625 / 2 = **4,033 memories per renderer at most**.
  - Under P2 it is 32,768 / (32.125/1024) / 2 = 522,240.
  - 131,072 GiB is the Linux figure in RENDER-32. The Mac's 49,637 GiB at 37.9% implies about the same: 49,637 / 0.379 = 130,968 GiB.

### 2.4 Copy-on-write images

- `memory_init_cow` defaults to true (`tunables.rs:273`).
- **Linux:** a module compiled from bytes has no backing file, so wasmtime puts its data image in a sealed memfd (`wasmtime-49.0.2/src/runtime/vm/sys/unix/vm.rs:125-172`). Each instance maps it copy-on-write (`src/runtime/vm/cow.rs:105-140`; `memory.rs:555-580`).
- **macOS:** `from_data` returns `None` (`vm.rs:120-123`), and there is no file (`cow.rs:105-130`), so there is no image. Each instance's data segments are written into fresh pages when it is instantiated.
- I did not check that std.wasm's segments qualify for a paged image (they start page-aligned at 65,536). Unverified.
- Measured per instance:
  - Linux: 195 MB / 6,058 = 32.2 KB.
  - Mac: 575 MB / 6,058 = 94.9 KB.
  - Mac at S (one Store per renderer, but the same 6,058 instances and memories; RENDER-32's text says its two renderers held the same address space "as S did", `docs/architecture.md:330`): 554 MB / 6,058 = 91.4 KB.
  - So the Stores themselves add about (575 − 554) MB / 6,058 = 3.5 KB each on the Mac. Most of the footprint is per instance and per memory, not per Store.
  - Some of the Mac's larger number is plausibly its 16 KiB pages and the missing CoW. That is inference, not measured.

### 2.5 StoreLimits

- `StoreLimitsBuilder::new().memory_size(KERNEL_MEMORY).instances(1).memories(1).tables(1)`, installed with `store.limiter(|l| l)` (`lib.rs:202-204`).
- `memory_growing` refuses when `desired > 32 MiB`. With `trap_on_grow_failure` at its default false, it returns `Ok(false)` rather than trapping (`wasmtime-49.0.2/src/runtime/limits.rs:336-356`).
- Exactly 32 MiB is allowed. Cut 1's test grows a memory to exactly `KERNEL_MEMORY` (`cut1.diff:241, 258`).
- `instances(1)` counts the kernel's instance only. The Store's dummy default-caller instance is allocated without a limiter (`store.rs:787-807`).

---

## 3. The runtime's life of an instance

### 3.1 Types

- **`KernelInst`** (`lib.rs:148-170`) holds:
  - `store: Store<StoreLimits>`;
  - `shape: u64`. Top-level kernels get the hash of `Shape::Kernel`. Voice kernels get 0 (`lib.rs:897`, `Make::New(0)`); their pool carries the shape;
  - `name`, `memory`, and typed `reset`, `process`, `active`;
  - `in_ptrs`, `out_ptrs`, `out_channels`;
  - debug-only: `before`, `kernel`, `in_names`.
- **Cut 1** adds `view: MemView { base: NonNull<u8>, len }` with `unsafe impl Send` (`cut1.diff:53-60`).
- **`KernelNode`** (`lib.rs:186-195`) holds `inputs: Vec<Input>`, `seen: Vec<u64>` and `inst: Option<Box<KernelInst>>`. `inst` is `None` only while its box moves, and the node plays silence then (`lib.rs:334-341`).
- **Cut 2** would keep the instance inline, not boxed. The speed plan says to do it second (`plan.md:10`). It is not in `cut1.diff`.

### 3.2 `KernelInst::new` (`lib.rs:197-276`), in order

1. `module_for(id)`, or the error "kernel `id` is not loaded" (`:201`).
2. A new `Store` with the limits above (`:202-204`).
3. `Instance::new`, with the context "instantiating `id`" (`:205`).
4. Six typed exports and `memory` (`:206-213`).
5. `k_init(sample_rate, 128, channels)` (`:214`). It has no node-naming context, so a trap here doesn't name the node.
6. Check `k_ports` (`:216-225`).
7. `k_port(i)` for each port, and the address checks (`:226-233`).
8. The ABI-1 overlap check, against `m2_ir::Kernels::kernel` (`:236-248`).
9. Write the constant inputs (`:250-256`).
10. Cut 1: keep `data_ptr` and `data_size` (`cut1.diff`, after `:278`).
11. `LIVE_KERNELS += 1` (`:257`). It counts only an instance that passed every check.

### 3.3 Prewarm

- `Graph::prewarm` calls `k.reset()` on every top-level kernel and on every voice kernel in every pool (`lib.rs:1294-1310`). It runs once per `Renderer::build`, before ghosts and fades are added (`lib.rs:1628-1629`).
- Cut 1 replaces the call with `KernelInst::prewarm`: a reset, then `bail!` if `data_ptr` moved, "its kernel memory moved as it grew (RENDER-32 …)" (`cut1.diff:94-104, 162-166`).
- Debug builds also check the address at every call (`cut1.diff:106-114`).

### 3.4 Reset: every caller

| Caller | Thread | What is reset |
|---|---|---|
| `prewarm`, in the build | the builder's thread (§7) | every kernel |
| a voice given a new note when it ran since its last reset (`dirty`) | audio | that voice's whole graph (`lib.rs:1454-1456`) |
| `Renderer::panic` → `reset` → `Graph::reset` → `reset_node` | audio, at a block line (`engine.rs:369-381, 400-407`) | every top-level kernel, plugin and ring; only the dirty voices of each pool (`lib.rs:1210-1243, 2151-2155, 1975-1985`) |
| `take_over`: a moved box marked poisoned | audio, at a block line | that node (`plan.rs:595-604`); a failure there sets `broken` |
| `Renderer::seek`, offline only: reset, then replay from 0 | the render's thread | everything (`lib.rs:1987-2000`) |

- A live seek, a loop wrap or a stop resets nothing (`lib.rs:22-25`; `engine.rs:369-377`).
- In p50's voices, a reset allocates nothing: none of the voice kernel types holds a `Vec` (§1.3).

### 3.5 process, active, and the checks after them

- **`KernelNode::process`** (`lib.rs:329-375`): copy inputs in (§1.2), `k.process(n)`, copy outputs out.
  - A trap gets the context "`<node>` crashed" (`lib.rs:282`).
  - **Strict** (offline by default, `Renderer::new`): each output sample is checked. The first non-finite one fails as "`<node>` produced inf at 1.234s" (`lib.rs:367-371`; test `strict_mode_names_the_kernel_that_broke`, `core/runtime/src/tests.rs:437-450`).
- **RENDER-25's non-finite check** (`lib.rs:1181-1185`). It runs live only (`!b.strict`), at the top level only (`outer.is_none()`), for kernel, poly, plugin and feedback nodes.
  - Every block, it scans every output sample and sets `poisoned[i]` when one is non-finite or when the node returned an error. A finite block clears the mark.
  - `take_over` resets a poisoned box as it moves it (`plan.rs:595-604`).
  - Measured: about 14 µs a callback and 316,000 instructions (`results.md:70`).
  - The song output's own check silences non-finite samples live and counts them (`lib.rs:1909-1922`).
- **`k_active`** calls (measured 59.8 a block, `results.md:62-63`) come from these places:
  - `PolyNode::process`: a voice whose note is off and whose output is quiet asks its graph `active()`, which stops at the first active kernel (`lib.rs:1386-1394, 1466-1470, 1349-1361`);
  - `Renderer::can_idle`, while stopped (`lib.rs:2174-2176`).
- **`LIVE_KERNELS`** (`lib.rs:172-184`): a process-wide `AtomicUsize`, incremented at the end of `new` and decremented in `Drop`. `live_kernels()` is read only by the churn test (`core/play/tests/churn.rs:58, 98`).
- **`kernel_memory`** (`lib.rs:1312-1322`, `Renderer::kernel_memory_bytes` at `:2239-2241`): the sum of `memory.data_size` over every kernel instance, voices included. Only tests read it (`core/play/tests/no_alloc.rs:159-163, 306-312`; `core/runtime/src/tests/live.rs:485-487`).

### 3.6 Drop

- Dropping a `KernelInst` drops its `Store`, which frees its instance and its memory mapping. wasmtime frees an instance only when its `Store` drops (`wasmtime-49.0.2/src/runtime/store.rs:139-144`). That is RENDER-32's reason (`docs/architecture.md:328-329`; design-7 §11.1, `docs/designs/step7/design-7.md:1311-1328`).
- Live, the drop happens on the `m2-play-drop` thread (§5.7).

### 3.7 A `memory.grow` refused at 32 MiB, followed through the code

1. **The kernel's allocator calls `memory.grow`.** wasmtime's `LocalMemory::grow` does the following (`wasmtime-49.0.2/src/runtime/vm/memory.rs:607-730`):
   - it asks the limiter first (`:651-659`). `StoreLimits::memory_growing` returns `Ok(false)` because desired > 32 MiB (`limits.rs:336-356`);
   - so `grow` returns `Ok(None)`, and `memory.grow` gives the wasm code -1;
   - without the limiter, P2 would refuse too: `!memory_may_move && new > capacity` bails, and `memory_grow_failed` logs at debug level and returns `Ok(())` (`memory.rs:676-678, 718-726`; `limits.rs:357-365`).
   - A grow that does succeed (under the cap) calls `mprotect`: `set_heap_limit` with a CoW image (`cow.rs:387-406`), or `make_accessible` (`mmap.rs:197-216`). That is a system call on whatever thread made the call.
2. **Rust's allocator returns null.** `Vec` allocation calls `handle_alloc_error`. The default prints "memory allocation of N bytes failed", which goes nowhere on `wasm32-unknown-unknown`, then aborts (https://doc.rust-lang.org/std/alloc/fn.handle_alloc_error.html; that the print goes nowhere and that the abort is `unreachable` are unverified here).
   - The wasm call fails with `wasm trap: wasm \`unreachable\` instruction executed` (`trap_encoding.rs:95, 138`).
   - The message never says "memory". It may carry a wasm backtrace, which shows no function names because the module is stripped (unverified: the exact text depends on wasmtime's backtrace setting).
3. **Where the user sees it depends on the call:**

| Call that grew | Path | What the user sees |
|---|---|---|
| `k_init`, or the first reset in `prewarm` | `KernelInst::new` or `Graph::prewarm` returns `Err` with no node context (`lib.rs:214, 292`; a voice adds "<poly>: voice", `lib.rs:897`). `Renderer::build` fails, and so does `prepare` | **An edit:** `Way::Unbuilt` (`actor.rs:573-576`) shows "Your last change is saved, but music2 couldn't get it ready to play, so the version before it keeps playing. Undo it, or fix what the detail names; music2 keeps trying. (In detail: …wasm trap…)" (`actor.rs:411-424`). `out_of_memory` matches only "mmap failed", "Cannot allocate memory" and "out of memory" (`crates/server/src/graph.rs:382-387`), so this case is not called running out of memory. **An open or an export:** the same error, through `plainly` (`graph.rs:392-397`; the export's renderer is built on the actor, `actor.rs:1200-1205`) or the export's `error` event |
| `k_process` (no std kernel does this), or a reset on the audio thread that grew | `KernelInst::process` returns "<node> crashed: wasm trap…" (`lib.rs:282`). `Graph::process` sets `poisoned` and returns the error (`lib.rs:1181-1190`). `render_block` sets `broken = !strict` (`lib.rs:1905-1908`). `Engine::render` drops the error, adds 1 to `crashes` and sends `Stop` to the transport (`engine.rs:570-579`). Building that error allocated on the audio thread; this is the only path where that happens (`engine.rs:573-574`) | **Live:** the whole song goes silent and playback stops, with no message. `Engine::crashes()` is read only by tests (`crates/server/src/graph/tests.rs:3049, 3156, 3185`). Pressing Play plays silence, since `broken` silences every block (`lib.rs:1887-1890`). The next activation resets the poisoned box and plays again (`plan.rs:595-604`; test `a_trapped_kernel_is_reset_at_the_next_activation`, `core/runtime/src/tests/live/keep.rs:423-440`). If that reset traps again, the new plan starts broken (`plan.rs:600-602`). A value-only edit sends no activation (`actor.rs:204, 270`), so it doesn't bring the sound back. **Export** (`set_strict(false)`, `crates/editor/src/render.rs:135`): `render_block` returns the error, so the export fails with "<node> crashed: …" |

- In std today the cap is unreachable (§1.4).
- The P1 layout has the same `StoreLimits`, so it fails the same way.

---

## 4. Voice pools, plugins, constants, captures, `Exec`

### 4.1 The `Exec` enum (`lib.rs:750-784`)

| Variant | Holds a box that moves? | Notes |
|---|---|---|
| `Control { slot, default }` | no | a control lane, read per sample (`lib.rs:1095-1106`) |
| `Audio { slot }` | no (its `AudioPlayer` is a lane, which moves with lanes) | copies the lane's block (`lib.rs:1107-1117`) |
| `Notes` | no | the scheduler runs before the graph (`lib.rs:1894-1900`) |
| `Time(TimeOf)` | no | beats or seconds (`lib.rs:1119-1126`) |
| `Const(f32)` | no | its buffer is filled once at `push` (`lib.rs:1068-1071`). Cells rewrite it with `write_const` (`lib.rs:1327-1346`) |
| `Kernel(KernelNode)` | **yes**: `Box<KernelInst>` | §3 |
| `Plugin(Box<PluginNode>)` | **yes**: `Arc<PluginInstance>` | §4.3 |
| `Missing(Option<Input>)` | no | a plugin that isn't installed: passes its input through, or plays silence (`lib.rs:1130-1141`) |
| `Poly(Box<PolyNode>)` | **yes**: `Box<VoicePool>` | §4.2 |
| `Feedback { input, ring }` | **yes**: the ring | a delay of exactly one block (`lib.rs:1142-1150, 1192-1205`) |
| `NoteField(NoteField)` | no | voices only: pitch, velocity, gate (`lib.rs:1151-1166`) |
| `Capture(usize)` | no | voices only: copies an outer node's output (`lib.rs:1167-1175`) |
| `Fade { old, new, wait }` | no | RENDER-25a's crossfade (`lib.rs:1176-1179, 1020-1061`) |

- `Graph` (`lib.rs:786-802`) holds `exec`, `order`, `bufs[node][port]`, `versions` (`COPY = u64::MAX` for non-constants, `lib.rs:83-85`), `resets` and `poisoned`.
- Nodes off the processing order are built as `Const(0.0)` (`lib.rs:851-854`).

### 4.2 Voice pools (RENDER-16, `docs/architecture.md:198-204`)

- **Types.** `VoicePool { voices: Vec<VoiceSlot>, born, shape }` (`lib.rs:410-416`). Each `VoiceSlot` has its own whole `Graph`, a note, `born`, `dirty`, `last_peak` and `taken` (`lib.rs:388-400`). `PolyNode` holds the lane slot, the voice output, `captures`, `plan` (this block's note-ons, sized `2 × ons`) and `pool: Option<Box<VoicePool>>` (`lib.rs:418-432, 905`).
- **Build** (`lib.rs:882-907`). For each of `voices`, the voice graph is built with every node `Make::New(0)`, so every voice kernel is its own `KernelInst` with its own `Store`. A poly inside a poly is refused (`:883-885`).
- **Process** (`lib.rs:1364-1477`), in order:
  1. Note-offs.
  2. Notes whose off is on this block line end here (`:1386-1394`).
  3. Allocation: the first free voice, else the oldest (`:1395-1412`).
  4. Each busy voice's graph runs per segment. The block splits where its note changes, and the voice graph is reset if dirty (`:1425-1465`).
  5. Outputs sum in voice order into `out` (`:1438-1444`).
  6. A voice ends when its note is off, its output is quiet (`QUIET = 1e-4`, `lib.rs:90-91`) and nothing is `active`.
- **Shape.** `Shape::Poly { voices, channels, voice: blank(voice) }`: the voice graph's structure without cell values, origins and names, plus the wasm SHAs of its kernels (`plan.rs:48-54, 72-74, 87-106`). A kept pool moves whole, by one swap (`plan.rs:693`).

### 4.3 Hosted plugins: what they are, and how they differ from kernels

What they are:
- CLAP plugins, native code, reached only through `PluginHost` (`core/plugins/src/lib.rs:1-20`; HOST-1).
- An instance is created, its state loaded and activated on the plugin main thread while the old plan plays (`lib.rs:914-985`; `core/runtime/src/plugins.rs:1-6`).
- `process` and `reset` run on whichever thread renders (`core/plugins/src/lib.rs:13-15`).
- Its last reference queues its destruction on the main thread (`plugins.rs:1-6, 15-17`).

| | Kernel | Plugin |
|---|---|---|
| Code | wasm in a sandbox, no imports | native, trusted like any DAW's (`core/plugins/src/lib.rs:8-9`) |
| Box | `Box<KernelInst>`, its own `Store` | `Arc<PluginInstance>` (a handle into the host) |
| Lifecycle | `init`, `reset`, `process`, `active`; made on the builder's thread | create, load state, activate and destroy on the plugin main thread; `process` and `reset` on the render thread |
| Inputs | port buffers in its memory | audio slices, plus an event list of notes, parameter changes every 16 frames (`PARAM_STEP`, `lib.rs:484`) and transport info. One call per transport run, so a call never spans a jump (`lib.rs:597-697`) |
| Outputs | port buffers, copied out | audio into the host's `Buf`; output events, up to 256 (`lib.rs:487`) |
| State | init only | a saved blob loaded at creation; a new committed blob loads into a kept instance (RENDER-26b, `lib.rs:1671-1721`) |
| Shape | kernel id, module SHA, channels | plugin id, version, channels (`plan.rs:40-47`) |
| On error, live | traps, the whole renderer goes silent | `inst.kill()`: only that plugin goes silent, and the session replaces it (`lib.rs:675-685`) |
| Missing | refused at check | `Exec::Missing`: passes its input through, or plays silence (FMT-18a, `lib.rs:877-880`) |
| Export | new instances | the live instances are borrowed while the engine is held (RENDER-1, `lib.rs:1796-1819`) |

### 4.4 Constants and cells

- A constant with no origin is folded into kernel inputs as `Input::Const` (`core/ir/src/check.rs:52-62, 115-116`). It is written once into kernel memory at build (`lib.rs:250-256`).
- A constant with an origin is a cell: an `Exec::Const` node (`core/runtime/src/cells.rs:1-13`).
- A cell inside a voice is one cell for that node in every voice (`CellAt::Voice`, `cells.rs:27-34`). It is written into every voice's graph (`cells.rs:376-386`).
- A step bumps the node's version, so each kernel it feeds copies it once. A ramp sets `COPY` for a block (`lib.rs:1339-1345`).

### 4.5 Captures

- A voice reads an outer node through `Exec::Capture(k)`, which indexes `PolyNode::captures` (`lib.rs:423-425, 904, 1167-1175`).
- The captures are kept outside the pool, so a pool moved into another plan reads that plan's nodes (`lib.rs:422-425`).
- Fades rewire them with `Edge::Capture` (`lib.rs:1279`; `keep.rs:130`).

---

## 5. Live edits

### 5.1 Who prepares, and against what

- The actor thread prepares. `Actor::way` (`actor.rs:520`) chooses the basis:
  - `Basis::Keep(Keep { old flat, old data, PlanInfo, Continues, restarts })` for an activation in the same engine;
  - `Basis::Fresh` otherwise (`crates/server/src/graph/actor.rs:552-570`).
- While an export borrows live instances, nothing is prepared (`actor.rs:547-551`). `exporting` is set only for an export that lends (`actor.rs:1199-1217`).
- `build()` calls `Renderer::prepare` (`actor.rs:426-438`).
- `Continues` is the session's mapping from new top-level nodes to old ones, by name (`plan.rs:108-134`; `m2_session::tx::continues`).

### 5.2 Wiring: `core/runtime/src/keep.rs`

- **What is kept** (`keep.rs:166-189`). A stateful node is kept when it continues an old node with the same `Shape`, unless it is a restarted plugin or one that lost its blob. A pure node continues when it has the same op (`continues_pure`, `keep.rs:153-164`).
- **The window** (`keep.rs:251-455`):
  - *changed edges* into kept nodes and into the song output (`:270-285`);
  - *fade-ins* for a new node's audio input from a node that plays on (`:286-299`);
  - *ghosts*: the old sources of changed edges that don't continue, plus what they read (`:300-354`). A poly ghost reads no notes (`:341-349`);
  - *fades* (`:355-375`);
  - the window's order, by Kahn's algorithm (`:376-444`). A cycle demotes the kept nodes on it and wires again (`:191-202, 445-455`).
- **Moves** (`keep.rs:211-225`): kept boxes first, then the ghosts' boxes, each `Move { from, to, shape }`. Ghost polys and plugins are released at entry (`:222-224`).

### 5.3 Building the plan (`Renderer::build`, `lib.rs:1596-1791`)

- A kept node is built `Make::Moved`, an empty slot. Everything else is built `Make::New(shape hash)` (`:1623-1627`).
- `prewarm` runs on the new graph (`:1629`). Then ghosts are appended as `Make::Moved` nodes (`:1632-1635`), then fades (`:1636-1640`), then the rewires (`:1641-1643`).
- Fade length: `round(0.005 × rate)`, which is 240 frames at 48 kHz. `wait` equals it when anything fades in (`:1636-1639, 1767`). The window is `fade + wait` (`plan.rs:541-544`).
- The `Entry` holds `after` (the old plan's id), `moves`, `lanes`, cell pairs, voice cells and `release` (`plan.rs:257-270; lib.rs:1725-1744`).
- `Leftovers` are sized now, so `end_fade` only swaps (`plan.rs:318-324, 528-533`).

### 5.4 Activate (audio thread, at a block line)

1. `Msg::Activate { renderer: Box<Renderer> }` travels over a bounded crossbeam channel of 1,024 (`core/play/src/lib.rs:35, 47-48`; `engine.rs:41-58`).
2. It applies only outside a fade window, with at least 3 free stash slots (`STASH = 16`) and no hold (`engine.rs:108-109, 411-422`).
3. `take_over` (`plan.rs:580-640`):
   - `verify`: the plan's id, each old box filled and of the stored shape hash, each new slot empty, the lanes' kinds and room (`plan.rs:551-578`). Otherwise the whole activation is refused (`engine.rs:442-448`);
   - `end_ramps`;
   - `take_from` for each move: `mem::swap` of the `Option<Box<KernelInst>>`, the `Option<Box<VoicePool>>`, the ring or the `Option<Arc<PluginInstance>>`; the poison mark moves too (`plan.rs:688-712`);
   - poisoned boxes are reset (`:595-604`);
   - the engine clock, the transport and the note ids are taken over (`:605-610`), and the lanes are swapped (`:611-630`);
   - cells are carried (`:631`), the releases applied (`:632-634`), and the window started (`:635-637`).
4. The old renderer, with every box nothing moved, goes to `retire_renderer` (`engine.rs:436-440`).

### 5.5 Fades and ghosts (`fade.rs`, `Exec::Fade`)

- `xfade` is linear. It is the new side exactly from the window's last frame on, and bit-exact where the two sides are equal (`core/runtime/src/fade.rs:4-17`).
- A fade node gives `old` for `wait` frames, then crossfades. After the window it copies `new` only when its version changes (`lib.rs:1020-1061`).
- Ghosts run in the new plan with their old instances for the window.
- At the first block line after the window, `end_fade` moves the ghosts (replaced by `Const(0.0)`), the ghost lanes and the replaced renders into `Leftovers`, then swaps in `order_after` (`plan.rs:642-669`). They leave as `Retired::Nodes` (`engine.rs:488-495`).

### 5.6 Exports, which hold two renderers

- **Where it is built and run.** The actor builds the export's `Render` (`actor.rs:1183-1205`). It is then moved to the `music2-export` thread, which renders it (`crates/server/src/export.rs:146-152`).
- **A pure song.** `Render::new` calls `Renderer::new`, so every kernel instance is new. Then `set_strict(false)` and `seek(first)`, which replays from 0 (`crates/editor/src/render.rs:132-136`; `actor.rs:1200-1203`).
  - So while it runs, the process holds the live renderer's 6,058 Stores and the export's 6,058.
  - That is the "two renderers" of RENDER-32's 25% rule (`docs/architecture.md:328`).
- **A song with plugins.** `Renderer::lending` borrows the live plugin instances while the engine is held (`Msg::Hold`), and makes new kernel instances (`lib.rs:1793-1805`; `render.rs:145-155`; `engine.rs:23-28`).
- Edits made during an export: the actor prepares nothing while instances are lent (`actor.rs:547-551`). A pure song's edits are still prepared, so a third, partial set of instances (the new plan's fresh ones) can exist during one. That follows from the code; no measurement backs it.

### 5.7 Retiring off the audio thread

- `Retired::{Renderer, Lane, Nodes}` (`engine.rs:66-72`) go over a bounded channel of 64 (`core/play/src/lib.rs:47-48`). When it is full they wait in the engine's 16-slot stash (`engine.rs:331-353`) or the callback's 8-slot `gone` (`core/play/src/lib.rs:52, 347-358`).
- The `m2-play-drop` thread polls every 5 ms and drops them (`core/play/src/lib.rs:693-711`). That is where `Store`s drop and their memories are unmapped.
- An object that can't be retired is leaked and counted, never dropped on the audio thread (`engine.rs:345-353`).

### 5.8 Seeks and resets

- **Live:** a seek is `Msg::Transport(Cmd::Seek)`, applied at once with no reset (`engine.rs:369-377`; RENDER-9 and RENDER-10). Panic resets everything (§3.4).
- **Offline:** `Renderer::seek` resets and replays from 0 (`lib.rs:1987-2000`). A range render is fresh, with the transport at `t0` (`lib.rs:1821-1828`).

---

## 6. The size of p50, from the code

The generator is `core/session/src/reference.rs` (`write`, `:58-150`), with `Reference::P50 = { tracks: 50, bpm: 127, repeats: 16, stems: 0 }` (`:39-40`).
- Track `i` is `delay(bass(..))` when `i % 3 == 0`, `pluck(..)` when it is 1, and `reverb(pad(..))` when it is 2 (`:88-92`).
- The song is `master(mixer({ tNN: { in, db: db(-12) }, … }))` (`:140-148`).
- Over i = 0..49: 17 bass tracks (0, 3, …, 48), 17 pluck tracks (1, 4, …, 49) and 16 pad tracks (2, 5, …, 47).

### 6.1 Kernels per voice, from `std/src/*.ts`

- **The building blocks:**
  - `osc.saw(pitch)` and `osc.square(pitch)` add `mtof` when given semitones (`std/src/dsp.ts:7-9, 21, 34`);
  - `add` keeps its unit (`std/src/math.ts:22`);
  - `vca` is `std:scale` (`dsp.ts:143`), and `svf.lowpass` is `std:svf` (`dsp.ts:104`);
  - written numbers are cells, not kernels (PKG-3).
- **bass** (`std/src/instruments.ts:27-41`), 4 voices (`:23`). Per voice:
  - `mtof`, `saw`, `add(p,-12)`, `mtof`, `square`, `scale(·,0.5)`, `add` (the body);
  - `adsr` env, `adsr` fenv, `xrange`, `scale(cutoff·)`, `svf`, `scale(env·vel)`, `scale` (the vca);
  - **14 kernels, 8 types**: mtof, saw, add, square, scale, adsr, xrange, svf.
  - Outside the poly: `drive` (`:40`). Then `delay` from `reference.rs:90`.
- **pluck** (`instruments.ts:105-116`), 12 voices (`:102`). Per voice:
  - `adsr`;
  - `mtof`, `saw`, `add(p,12)`, `mtof`, `square`, `scale(·,0.3)`, `add`;
  - `xrange`, `svf`, `scale(vel·0.6)`, `scale(env·)`, `scale` (the vca);
  - **13 kernels, the same 8 types.**
- **pad** (`instruments.ts:130-140`), 12 voices (`:127`). Per voice:
  - `adsr`;
  - `add(p,-0.07)`, `mtof`, `saw`, `add(p,0.07)`, `mtof`, `saw`, `add`;
  - `svf`, `scale(vel·0.35)`, `scale(env·)`, `scale` (the vca);
  - **12 kernels, 6 types**: adsr, add, mtof, saw, svf, scale.
  - Then `reverb` from `reference.rs:91`.
- **The song module:**
  - `mixer` gives each channel a `gain` (`db` given, no `pan`), so 50 `gain`s (`std/src/mix.ts:33-38`);
  - `sum` is 49 `add`s in a chain (`mix.ts:41`; `math.ts:65-68`);
  - `master` is one `limit` (`mix.ts:55`).
  - design-8a puts these 100 in the `song` module (`docs/designs/step8/design-8a.md:284`).

### 6.2 Instances, Stores and memories today (one of each per kernel instance)

| Module | How many | Pool: voices × kernels | Top-level kernels | Instances per module | Kernel types per module | All modules of this kind |
|---|---|---|---|---|---|---|
| bass track | 17 | 4 × 14 = 56 | drive, delay = 2 | 58 | 10 | 986 |
| pluck track | 17 | 12 × 13 = 156 | 0 | 156 | 8 | 2,652 |
| pad track | 16 | 12 × 12 = 144 | reverb = 1 | 145 | 7 | 2,320 |
| song | 1 | — | 50 gain + 49 add + 1 limit = 100 | 100 | 3 | 100 |
| **total** | 51 | **5,908 pooled** in 50 pools (464 voice slots: 17×4 + 17×12 + 16×12) | **150 top-level** | **6,058** | 13 distinct in the song | 6,058 |

- The total is 986 + 2,652 + 2,320 + 100 = 6,058, the measured instance count exactly.
- The song uses 13 distinct types of std's 29: mtof, saw, square, add, scale, adsr, xrange, svf, drive, delay, gain, reverb and limit.
- Today: 6,058 Stores, 6,058 linear memories, and 6,058 instances of std.wasm. Each Store also has one dummy default-caller instance (`store.rs:779-816`).
- Per call, measured per block (`results.md:62-63`): 2,245 `k_process`, 59.8 `k_active` and 7.2 `k_reset`.
  - Voice-kernel `k_process` calls a block: 2,245 − 150 = 2,095.
  - Divided by 12 to 14 kernels a voice, that is roughly 150 to 175 voice-segments a block. That is an upper bound on sounding voices, since a split voice runs twice.

### 6.3 Under the alternative groupings (counts from §6.2)

| Grouping | Units (Stores, and instances or memories) | Arithmetic |
|---|---|---|
| today: one per kernel instance | 6,058 | — |
| cut 3: one `Store` per voice pool, still one instance and one memory per kernel (`hv3.diff:198-209`) | 200 Stores; 6,058 memories | 50 pools + 150 top-level |
| one per kernel type per module | 421 | 17×10 + 17×8 + 16×7 + 3 |
| the same, with the voice pool's types and the module's top-level types counted apart | 421 | no type is in both, per module (bass: 8 + 2; pluck: 8 + 0; pad: 6 + 1; song: 3) |
| one per package per module | 51 | 50 tracks + song, all std |
| one per kernel type per renderer | 13 | — |
| one per package per renderer | 1 | — |

Address space under P1's 4 GiB layout, at 4.0625 GiB a memory, for two renderers against the 25% limit of 32,768 GiB:
- 421 memories: 2 × 421 × 4.0625 = 3,421 GiB, 2.6%.
- 51 memories: 2 × 51 × 4.0625 = 414 GiB, 0.32%.
- Cut 3 keeps 6,058 memories, so it doesn't change the address space.

**What the ABI must change before any grouping of more than one instance a memory works:**
- v0's state is one `static mut HOST` per kernel type per instance (`kernel/src/lib.rs:181`), and no export takes a handle.
- So "one instance per type per module" needs an ABI that creates many states in one instance and passes a state handle to `port`, `reset`, `process` and `active`.
- "One instance per package per module" needs the same, across types.
- A memory would then hold many kernels' states. The 32 MiB cap (`lib.rs:87-88`) would then bound a whole group, not one kernel. Today the largest std state is one `delay` at 384 kHz, 11.7 MiB during a reset (§1.4).

---

## 7. Where 8a would put Stores

Sources: `docs/designs/step8/design-8a.md` (revision 2) and `refresh.md`.
- design-8a's precondition is one `Store` per kernel instance, so "a `Store` is entered by one thread at a time, so kernels in different partitions can run at once" (`design-8a.md:98-101`).
- With cut 3 it would be one Store per voice pool: "still one per box, and a box lives in one partition" (`refresh.md:52, 188`).

| Stage | Thread | What happens to the Stores | Source |
|---|---|---|---|
| Build | the session actor: every live plan, and the export's renderer too (`actor.rs:1200-1205`); a CLI's main thread | `KernelInst::new` makes each Store, instance and memory, and runs `init` and the prewarm reset. 8a builds the partition schedule here too | `actor.rs:426-438`; `refresh.md:135, 317`; `design-8a.md:191-224` |
| Hand-off | actor → device thread | `Msg::Activate(Box<Renderer>)` over the bounded queue. `Store<StoreLimits>` is `Send`, so `KernelInst` is `Send` (cut 1 adds `unsafe impl Send for MemView`, `cut1.diff:58-60`). Never `Sync` | `refresh.md:189` |
| Activation | device thread, at a block line, the pool quiescent | `take_over` swaps boxes (`Box<KernelInst>`, `Box<VoicePool>`) from the old plan to the new | `design-8a.md:485-498`; `refresh.md:123-133` |
| Each block | device thread plus N−1 workers | one task per partition (a track, or `song`); a `poly`'s voices stay in its task. A partition's kernel Stores and its pool's voice Stores are entered by the thread that pops the partition, for that task only. The next block may run it on another thread: "nothing is pinned to a thread" | `design-8a.md:27-35, 326-351`; `refresh.md:129-131` |
| Pre-pass and post-pass | device thread | cells, schedulers, players and timing before; feedback rings, output, `can_idle` (which calls top-level `k_active`) after | `refresh.md:110-121` |
| Resets | device thread (panic, poisoned at `take_over`); a worker (a voice reuse inside its poly's task) | — | `refresh.md:132-133`; `lib.rs:1454-1456` |
| A trap | the thread that trapped | the partition fails, its dependents are skipped, and the plan goes `broken` and silent as today. On the Mac a trap waits for wasmtime's Mach-port helper thread, which runs at normal priority | `design-8a.md:363-372`; `refresh.md:192` |
| Thread set-up | every worker and the device thread | `Engine::tls_eager_initialize()` before the first kernel call: on Linux a 256 KiB signal stack, on the Mac exception ports. Nothing calls it today, so the device thread does that work inside its first callback | `refresh.md:66, 190`; `design-8a.md:425-429` |
| Retire | `m2-play-drop` | the old renderer, its leftovers and their Stores drop there. Its `munmap`s now race N real-time threads (risk R15) | `refresh.md:219, 391` |
| Export | built on the actor, moved to `music2-export`, rendered on its own offline pool, made per render | the export's renderer and its Stores are entered by that pool's threads: the E-cores beside playback, or the P-cores while the engine is held. refresh.md:165 says the export renders "on the export's thread", which is true of rendering; the build happens on the actor | `design-8a.md:676-697`; `refresh.md:74, 165-168`; `actor.rs:1200-1205` |
| Voices across threads | not built | if built after cut 3, a pool's shared Store would need one Store per chunk of voices | `design-8a.md:699-716`; `refresh.md:75` |

Sizes design-8a quotes, as its own estimates:
- p50 has 51 partitions: 50 track roots and `song`, which waits for all 50 (`design-8a.md:284`).
- Per module, the Stores entered by one task: bass 58, pluck 156, pad 145, song 100 (§6.2).

---

## 8. Facts a redesign must keep or replace

- **Isolation and movability come from the Store, not the instance.** Memory is freed only when a Store drops (`store.rs:139-144`). Boxes move between plans by swap (`plan.rs:688-712`). Any grouping must keep "a box frees its memory when it drops" and "a box moves whole".
- **The voice pool is already one box** (`plan.rs:693`). Its kernels have shape 0 and are never moved alone (`lib.rs:897`).
- **The shape hash includes the whole package's wasm SHA** (`plan.rs:35-39`). Per-kernel modules would make shapes per kernel.
- **Ports are copied.** Every input that isn't a steady constant is copied into kernel memory, and every output is copied out of it, at every call (`lib.rs:343-373`). With shared memories, ports could be read in place. That is a design option, not code today.
- **The cost of an instance is mostly its memory, not its Store.** On the Mac: 3.5 KB for each Store against 91.4 KB for each instance (§2.4). Measured build time per instance is 216 ms / 6,058 = 35.7 µs, an upper bound covering the whole build; S's 181 ms gives 29.9 µs.
- **The hardware-guard layout (4 GiB per memory) fits the 25% rule up to 4,033 memories per renderer** (§2.3). p50 has 6,058 today. A grouping into 421 or 51 memories fits by a wide margin, and would also drop O1's explicit bounds checks (+9.6 M JIT instructions, +85 to +105 µs a callback on Linux; task facts).
- **ABI v1 is the moment to change these** (ABI-2 and ABI-3, `docs/architecture.md:1170-1181`):
  - v0's kit already isolates state behind `Host<K>` and `export!`. Kernels written with the kit never touch `HOST` directly (`kernel/src/lib.rs:123-221`);
  - so changing the macro to handle-based exports would leave each kernel's `impl Kernel` unchanged. That is an inference from the code: no such change was tried.
