# idea3, external facts: code apart from state, for music2's kernels

Research only. Nothing was built or run except a 60-line Python reader that printed the section headers of
`std/kernels/std.wasm` (§D2). Every fact cites a source. **Derived** marks arithmetic on measured numbers or on cited
facts. **Unverified** marks what could not be checked.

Short names for paths:
- `W` = `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wasmtime-49.0.2/src`
- `E` = `.../wasmtime-environ-49.0.2/src`
- `CL` = `.../wasmtime-internal-cranelift-49.0.2/src`
- `CG` = `.../cranelift-codegen-meta-0.136.2/src` (the Cranelift that wasmtime 49.0.2 uses: `wasmtime-internal-cranelift-49.0.2/Cargo.toml:50`)
- `S7` = `$SCRATCH/idea3/src` (step 7, frozen at ed7fc8e)

---

## A. wasmtime 49.0.2

### A1. Store and instance lifetimes, and what a Store holds

- **Nothing in a Store is freed before the Store drops.** The docs say: "once an instance is created within a Store it
  will not be deallocated until the Store itself is dropped. This makes Store unsuitable for creating an unbounded number
  of instances" (`W/runtime/store.rs:139-145`). Also `W/lib.rs:107-111`: a Store "does not GC objects such as unused
  instances internally, so it's intended to be short-lived".
- Code confirms it. Instances are freed only in `impl Drop for StoreOpaque`, which walks every instance and calls
  `deallocate_module` (`W/runtime/store.rs:2425-2457`). That is the only call site of `deallocate_module` in the crate
  (grep of `W`; defined at `W/runtime/vm/instance/allocator.rs:389`).
- **A memory can't be freed or shrunk on its own either.** The public `Memory` API has `new`, `read`, `write`, `data*`,
  `size`, `grow` and nothing that releases or shrinks (`W/runtime/memory.rs:269-713`). WebAssembly memory "can only grow
  and never shrink", and "there are no APIs for releasing memory to the host"
  (https://github.com/WebAssembly/memory-control/blob/main/proposals/memory-control/Overview.md). That proposal's
  `memory.discard` is at Phase 1 (https://github.com/WebAssembly/proposals/blob/main/README.md).
- A host-made `Memory::new` also lives until its Store drops. It is backed by a "frankenstein" dummy instance in that
  Store (`W/runtime/trampoline/memory.rs:22-60`; `StoreInstanceKind::Dummy`, `W/runtime/store.rs:689-695`).
- **What a Store holds per instance:**
  - one `StoreInstance { handle: InstanceHandle, kind }` in a `TryPrimaryMap` that only ever grows
    (`W/runtime/store.rs:471, 676-696`);
  - behind the handle, a `vm::Instance`: an `Arc` to the module's runtime info, its memories as
    `(MemoryAllocationIndex, Memory)`, its tables, its passive element segments, a pointer back to the Store, and a
    dynamically sized `VMContext` as the last field (`W/runtime/vm/instance.rs:91-151`);
  - for each memory, an `MmapMemory`: an `Arc<Mmap>`, `len`, `maximum`, the growth reserve and the guard sizes
    (`W/runtime/vm/memory/mmap.rs:14-46`).
- **The rest of the Store is per Store, not per instance:** an `Engine` clone, a `VMStoreContext`, a `ModuleRegistry`,
  `FuncRefs`, the instance, memory and table counters with their limits, the fuel and async state, two value
  scratch vectors, a protection key and the executor (`W/runtime/store.rs:439-552`).
- **Items can't cross Stores.** "This store is the only store that can be used to interact with wasm items after they're
  created"; passing the wrong Store panics (`W/runtime/store.rs:184-195`). Each call also asserts that the function
  belongs to the Store passed in (`self.store.assert_belongs_to(store.id())`, `W/runtime/func.rs:1210-1213`). So a
  plain (non-shared) `Memory` can be imported only by instances in its own Store.
- **wasmtime's own advice:** "have a Store correspond roughly to the lifetime of a 'main instance'"
  (`W/runtime/store.rs:144-145`).

### A2. Limits per Store

- By default a Store allows at most 10,000 instances, 10,000 memories and 10,000 tables (`W/runtime/limits.rs:4-8`,
  applied at `W/runtime/store.rs:742-744`). A `ResourceLimiter` can change them (`W/runtime/limits.rs:128-155`).
  Step 7 caps each kernel's Store at 1 instance, 1 memory, 1 table and a 32 MiB memory (`S7/core/runtime/src/lib.rs:202`).
- **Derived:** p50 has 6,058 instances, under the 10,000 default, so one Store per renderer fits in the defaults. More
  than 10,000 instances in one Store would need a limiter.

### A3. Sharing one memory among instances; multi-memory; shared memories

**One plain memory imported by several instances in the same Store**
- This is allowed in wasm and in wasmtime, from the same Store only (A1).
- Each access to an *imported* memory loads the `VMMemoryDefinition` pointer from the vmctx and then the base through
  it: two loads. A defined memory needs one load at a fixed vmctx offset (`CL/func_environ.rs:1598-1683`; defined at
  1637-1659, imported at 1661-1682). The base load is marked `readonly` when the memory can't move (1606-1609), so
  Cranelift may hoist it out of loops.
- **Copy-on-write images apply only to defined memories:** "Only memories defined in the module can be initialized this
  way" (`W/config.rs:2259`).
- **Active data segments are copied at every instantiation.** Instantiation runs `memory.init` and then `data.drop` for
  each active segment (https://webassembly.github.io/spec/core/exec/modules.html, Instantiation steps 6 and 28,
  `rundata`). So a second instance of a module that imports a memory rewrites the module's data region in it.

**Multi-memory**
- Enabled by default: "This is `true` by default" (`W/config.rs:1203-1215`). Standard since Wasm 3.0, announced
  2025-09-17: "a single module can now declare (define or import) multiple memories"
  (https://webassembly.org/news/2025-09-17-wasm-3.0/).
- Each memory gets its own heap base and bound loads (`CL/func_environ.rs:1587-1596`, one `HeapData` per memory index).
- Each memory is its own reservation under the same tunables (`W/runtime/vm/memory/mmap.rs:51-125`).
- The pooling allocator allows one memory per module by default (`max_memories_per_module: 1`, `W/config.rs:4021`).

**Shared memories (threads proposal)**
- The threads proposal is Phase 4 (https://github.com/WebAssembly/proposals/blob/main/README.md). In wasmtime the
  feature flag is on by default (`W/config.rs:988-1011`), but **creating** a shared memory is off by default:
  `Config::shared_memory`, "`false` by default", and "a tier 2 feature … will not receive security updates or fixes to
  historical releases. Additionally security CVEs will not be issued" (`W/config.rs:3316-3335`, default at
  `W/config.rs:318`).
- **A `SharedMemory` is made from an `Engine`, not a Store:** `SharedMemory::new(engine, ty)` (`W/runtime/memory.rs:864-875`).
  It is `Arc`-backed (`W/runtime/vm/memory/shared_memory.rs:23-30`), and the host reads it as `&[UnsafeCell<u8>]`,
  at the same time as wasm (`W/runtime/memory.rs:943`). Instances in any number of Stores can import it
  (`W/runtime/memory.rs:818-858`, doc and example). Each importing Store holds it through a dummy instance
  (`W/runtime/trampoline/memory.rs:27-31`, `preallocation: Option<&SharedMemory>`).
  - **Derived from these:** a shared memory outlives any one Store. It is freed when the last `Arc` holder drops.
    Which holders exist beyond the dummy instances is unverified.
- **Costs and limits of shared memories:**
  - A shared memory must declare a maximum: "shared memories must have a maximum size" (`W/runtime/types.rs:3305-3307`).
  - It never moves (`E/types.rs:2424-2430`).
  - Its `VMMemoryDefinition` is reached through a pointer even when the module defines it (`CL/func_environ.rs:1612-1636`).
  - `grow` takes a write `RwLock` (`W/runtime/vm/memory/shared_memory.rs:90-91`).
  - The pooling allocator refuses it: "memory is shared which is not supported in the pooling allocator"
    (`W/runtime/vm/instance/allocator/pooling/memory_pool.rs:337-343`).
  - Its mapping is a plain `MmapMemory` under the engine's tunables, with no CoW image
    (`W/runtime/vm/memory/shared_memory.rs:33-51`). The same reservation and guard rules therefore apply, so a 4 GiB
    shared memory elides bounds checks the same way (A4).
- Toolchain cost of a shared memory is in §C4.

### A4. Memory layout, bounds checks, moving, growth, page faults, CoW

**Defaults on a 64-bit host** (`E/tunables.rs:315-344`, plus the shared defaults at `E/tunables.rs:250-290`):
- `memory_reservation` = 4 GiB (326)
- `memory_guard_size` = 32 MiB (327)
- `memory_reservation_for_growth` = 2 GiB (333)
- `memory_may_move` = true (265)
- `guard_before_linear_memory` = true (266)
- `memory_init_cow` = true (273)
- `signals_based_traps` = true (341)
- On 32-bit hosts: 10 MiB reservation and 64 KiB guard (`E/tunables.rs:294-312`).

**A memory's address-space cost:** pre-guard (the guard size, when `guard_before_linear_memory`) + reservation + guard
(`W/runtime/vm/memory/mmap.rs:62-69, 102-107`). The growth reserve is added only when the module's own maximum
exceeds the reservation, or its minimum does (`mmap.rs:81-92`). The pre-guard "does not affect performance in any way"
and only costs address space (`W/config.rs:2151-2173`).

**When bounds checks are elided**
- `can_elide_bounds_check` holds iff virtual memory can be used (`signals_based_traps`, and a wasm page at least as big
  as a host page), the memory is 32-bit, and **reservation + guard ≥ 4 GiB** (`E/types.rs:2376-2407`).
- Even then, an access is unchecked only if `u32::MAX ≤ reservation + guard − (offset + size)`
  (`CL/bounds_checks.rs:261-311`). With 4 GiB plus the 32 MiB default guard, that covers static offsets up to about
  32 MiB (`W/config.rs:2006-2011`).
- **The P2 path:** if the memory can't move and its minimum fits the reservation, each access is compared with a
  constant bound and no length is loaded (`CL/bounds_checks.rs:328-371`). That is step 7's P2.
- Otherwise the bound is loaded and compared (`CL/bounds_checks.rs:373-470`).
- **Derived from `E/types.rs:2406`:** elision needs about 4 GiB of address space per memory, however that 4 GiB is split
  between reservation and guard.

**`memory_may_move` (false)** lets Cranelift treat the base as constant and hoist it. Memories then can't grow past
their first allocation (`W/config.rs:1934-1967`). Growth past capacity is refused with "Memory maximum size exceeded"
(`W/runtime/vm/memory.rs:674-678`).

**`memory_reservation_for_growth`** is the extra space added when a memory is first allocated past the reservation,
or relocated (`W/config.rs:2017-2080`).

**What `memory.grow` costs**
- Wasm calls the `memory_grow` libcall, which asks the Store's `ResourceLimiter` first (`W/runtime/vm/libcalls.rs:243-264`;
  `W/runtime/vm/memory.rs:606-660`).
- **Within capacity:** one `mprotect(READ|WRITE)` on the new pages (`W/runtime/vm/memory/mmap.rs:191-217` →
  `W/runtime/vm/sys/unix/mmap.rs:110-124`). With a CoW image the same `mprotect` happens in `set_heap_limit`
  (`W/runtime/vm/cow.rs:387-402`).
- **Past capacity, when the memory may move:** a new `mmap` reservation, an `mprotect`, and a copy of the whole
  memory (`W/runtime/vm/memory/mmap.rs:159-190`).
- **Pages are faulted in on first touch.** Reservations are `mmap_anonymous(PROT_NONE, MAP_PRIVATE|MAP_NORESERVE)` on
  Linux (`W/runtime/vm/sys/unix/mmap.rs:20-36, 60-82`). Anonymous mappings are "initialized to zero"
  (https://man7.org/linux/man-pages/man2/mmap.2.html). So the first write to each page faults. Prefaulting needs
  something like `MAP_POPULATE` ("Populate (prefault) page tables", same page), or touching the pages.
- Unverified: the kernel locks `mprotect` takes (Linux's mm sources could not be fetched).

**Copy-on-write images (`memory_init_cow`, default on)**
- They speed up instantiation, and pages that are only ever read stay shared between instances. A first write costs a
  private copy, and an IPI that "can be a significant bottleneck in high-parallelism situations" (`W/config.rs:2236-2288`).
- **Linux:** "supported for all instances of Module", via the module's mmap or `memfd_create` (`W/config.rs:2268-2272`).
- **Other Unix, so macOS:** "only supported when loading modules from a precompiled file via `Module::deserialize_file`"
  (`W/config.rs:2273-2276`). In code, `MemoryImageSource::from_data` returns `None` off Linux
  (`W/runtime/vm/sys/unix/vm.rs:120-123`).
- Step 7 loads kernels with `Module::new` (`S7/core/runtime/src/packages.rs:183`). **Derived:** on the Mac, every
  instantiation copies std.wasm's data segments into the new memory.
- A heuristic skips the image when the data is sparse. Images are always allowed up to
  `memory_guaranteed_dense_image_size`, 16 MiB by default (`W/config.rs:2339-2377`).

**Custom memories:** `Config::with_host_memory(MemoryCreator)` lets the embedder supply each memory. JIT code still
relies on the guard size it is given (`W/config.rs:1696-1704`; `W/runtime/memory.rs:764-815`). Such a memory is still
dropped with its Store (A1).

### A5. The pooling allocator

- **What it is:** one large slab mapped up front, carved into slots, each a memory plus guard. Instances take a slot and
  give it back when their Store drops. In steady state, freeing a memory is "a single `madvise` … followed by a single
  (optional) `mprotect`" (`W/config.rs:3779-3870`; layout at
  `W/runtime/vm/instance/allocator/pooling/memory_pool.rs:1-50, 87-104`).
- **Default counts** (`W/config.rs:3918-4029`):
  - `total_memories`, `total_core_instances` and `total_tables` are each 1,000 (100 on 32-bit);
  - `max_memories_per_module` is 1;
  - `max_memory_size` is 4 GiB;
  - `max_unused_warm_slots` is 100;
  - `decommit_batch_size` is 1;
  - `linear_memory_keep_resident` is 0;
  - `memory_protection_keys` is `No`.
- **Address space:** the slab reserves `total_memories × slot_bytes` up front, where slot_bytes =
  max(reservation, max_memory_size) + guard (`memory_pool.rs:832-939`, `faulting_region_bytes` at 856-859).
  - The docs: "each linear memory will *reserve* 6 GiB of space by default" (`W/config.rs:4269-4290`).
  - With 4 GiB per slot, a 47-bit space allows "at most 32k slots" (`W/config.rs:3836-3852`).
- **Freeing still waits for the Store.** Pooling only recycles memory faster after a Store drops. Slots go back only
  through `deallocate_module`, which only Store drop calls (A1).
  - Freeing takes locks: `Mutex<ImageSlot>` (`memory_pool.rs:118`), the decommit queue shard lock
    (`W/runtime/vm/instance/allocator/pooling.rs:476-492`) and the index allocator's `Mutex`
    (`.../pooling/index_allocator.rs:105`).
  - Freeing makes syscalls: on Linux `madvise(DONTNEED)` or `process_madvise`; elsewhere an `mmap(MAP_FIXED)` over the
    range (`W/runtime/vm/sys/unix/vm.rs:48-90`). On non-Linux the decommit behaviour is "Zero", not restore
    (`vm.rs:98-104`).
  - `linear_memory_keep_resident` and `table_keep_resident` are "only applicable on Linux" (`W/config.rs:4133-4155`).
- **MPK (memory protection keys)** shrinks the guard regions by colouring stripes. "MPK is only available on Linux
  (called `pku` there) and recent x86 systems"; it is off by default and "still experimental" (`W/config.rs:4444-4481`;
  `W/runtime/vm/mpk/mod.rs:1-30`: "On any other kind of machine, this module exposes noop implementations").
  - **So no MPK on Apple silicon.**
- **Fit for the Mac:** the pooling allocator runs on macOS, but there freeing is an `mmap` per range, keep-resident
  doesn't apply, CoW needs `deserialize_file`, and there is no MPK (all cited above). The docs: "while the pooling
  allocator works on Windows it has not been tuned for performance on Windows in the same way it has on Linux"
  (`W/config.rs:3830-3834`). Unverified: any tuning statement about macOS.
- **Elsewhere:** the pooling allocator "does not affect compiled WebAssembly code" (`W/config.rs:3785-3787`).
  Instantiation with it: "Creating a new instance doesn't require allocating new Wasm memories and tables on demand"
  (https://docs.wasmtime.dev/examples-fast-instantiation.html).

### A6. Send, Sync, and moving a Store between threads

- `Store<()>`, `StoreContextMut<'_, ()>`, `Instance`, `Memory`, `Func` and `TypedFunc` are asserted `Send + Sync`
  (`W/runtime.rs:173-190`). `Store<T>` is a `Box<StoreInner<T>>` (`W/runtime/store.rs:196-199`). **Derived:** its
  auto-trait `Send`/`Sync` therefore follow `T`. Step 7's `T` is `StoreLimits` (`S7/core/runtime/src/lib.rs:202-203`).
- **Calls need `&mut Store`** (`TypedFunc::call(&self, mut store: impl AsContextMut, …)`, `W/runtime/func/typed.rs:100-105`).
  So one Store runs on one thread at a time, and two instances in one Store can't run at the same time.
- **A Store can move between threads between calls.** On each outermost entry, `enter_wasm` reads the *current* stack
  pointer and sets the stack limit to SP − `max_wasm_stack` (`W/runtime/func.rs:1517-1580`). Per-call trap state is a
  stack-allocated `CallThreadState` linked from TLS (`W/runtime/vm/traphandlers.rs:457-509`).
- **First wasm call on a new thread:** it pays a lazy per-thread set-up, "on the order of a few hundred microseconds".
  `Engine::tls_eager_initialize()` moves that cost earlier (`W/engine.rs:782-799`).
- **Wasm runs on the calling thread's own stack** when sync. `max_wasm_stack` is 512 KiB by default
  (`W/config.rs:295, 832-856`).
- **macOS traps** go through Mach exception ports by default (`macos_use_mach_ports`, `W/config.rs:3106-3135`, default
  at 312).

### A7. Call costs

**Host → wasm**
- `TypedFunc::call` → `call_raw`: params and results go in a stack-allocated union; no heap allocation on the success path
  (`W/runtime/func/typed.rs:154-224`).
- Then `invoke_wasm_and_catch_traps`: save the store context, run the call hooks (none unless set), `catch_traps`, and
  the array-call trampoline (`W/runtime/func.rs:1457-1483`).
- wasmtime's docs: "the overhead of calling WebAssembly is otherwise on the order of a few nanoseconds"
  (`W/engine.rs:786-788`).
- **Derived from music2's profile:** wasmtime host code was 766 µs over about 9,250 calls, so 766 / 9,250 ≈ 83 ns a call.
  That counts all of wasmtime's host self time, the cold-Store lookups that Cut 1 removes included, so it is an upper
  bound. Trampolines: 37 / 9,250 ≈ 4.0 ns a call.

**Wasm → wasm across instances (an imported function) in one Store**
- It is an indirect call. Load the callee's vmctx and its `wasm_call` pointer from the caller's vmctx, then
  `call_indirect`. There is no trampoline, no host code and no store access (`CL/func_environ.rs:1930-1947, 2040-2055`).
- A call to a function in the same module is a direct call that passes the same vmctx (`CL/func_environ.rs:1909-1928`).
- Unverified: a measured number for either.

**Wasm → host:** goes through an exit trampoline (not studied further: kernels import nothing, ABI v0,
`S7/std/kernels-src/kernel/src/lib.rs:4-6`).

### A8. Spectre mitigation on heap accesses

- Cranelift's `enable_heap_access_spectre_mitigation` defaults to true. "This is a no-op for any heap that needs no bounds
  checks; e.g., if the limit is static and the guard region is large enough that the index cannot reach past it"
  (`CG/shared/settings.rs:290-302`).
- Step 7 turns it off under P2 (`S7/core/runtime/src/packages.rs:125-130`). RENDER-32 says it goes back on before a
  non-std kernel runs (`S7/docs/architecture.md:328`).
- **Derived:** with 4 GiB-class memories the guard costs nothing on the accesses that are elided, so turning it back on
  would cost nothing there.

### A9. Platform facts the layout depends on

- **macOS arm64 page size is 16 KiB** ("On arm64, the system page size is currently 16kB"; Chromium,
  https://crrev.com/808610). Wasm pages are 64 KiB (`W/runtime/memory.rs:865-867`, "64K-sized pages").
- **macOS arm64 user address ceiling** is `0x00007FFFFE000000` (XNU `osfmk/mach/arm/vm_param.h`,
  https://git.saurik.com/apple/xnu.git/blob/HEAD:/osfmk/mach/arm/vm_param.h). **Derived:** that is 2^47 − 2^25 bytes,
  about 128 TiB.
- macOS maps out the first 4 GiB (Apple DTS, https://developer.apple.com/forums/thread/781966).
- Linux x86-64 user space is 128 TiB (`W/config.rs:4284-4285`).

---

## B. Prior art: code apart from state

| System | Code | State | Many instances share | Source |
|---|---|---|---|---|
| Faust, C++ backend | one class `mydsp` | fields of each object; `classInit` sets "static shared state" once, `instanceInit` per instance (`instanceConstants`, `instanceClear` for delay lines) | code and static tables | https://faustdoc.grame.fr/manual/compiler/ |
| Faust, wasm backend | one module | each DSP is a struct in linear memory; every export takes `$dsp` first: `compute($dsp, count, $inputs, $output)`, `init($dsp, sampleRate)`, `instanceClear($dsp)`, … | **one memory**: a polyphonic node's voices, mixer and effect all get the same `WebAssembly.Memory`, sized from `dspMeta.size * voices` + effect + I/O buffers | https://github.com/grame-cncm/faustwasm/blob/master/src/FaustDspInstance.ts ; https://github.com/grame-cncm/faustwasm/blob/master/src/FaustWasmInstantiator.ts |
| Faust, memory options | — | `-mem`: allocations through a custom memory manager; `-mem1`..`-mem3`: the "iControl/fControl, iZone/fZone model", state in host-given zones (`-mem3`: "access as function parameters") | — | https://faustdoc.grame.fr/manual/options |
| Max gen~ export | free functions in a namespace | an opaque `CommonState*` from `create(sr, vs)`; `perform(state, ins, nin, outs, nout, n)`, `reset(state)`, `destroy(state)`, `getstatesize/getstate/setstate(state, …)` | code | https://github.com/Cycling74/gen-plugin-export/blob/master/misc/Source-Plugin/PluginProcessor.cpp |
| CLAP | one binary; a factory's `create_plugin(host, plugin_id)` ("[thread-safe]") | each `clap_plugin` carries `void *plugin_data` ("reserved pointer for the plugin"); `activate(sample_rate, min, max)` is where "the plugin may allocate memory"; `process` is "[audio-thread & active & processing]" | code | https://github.com/free-audio/clap/blob/main/include/clap/plugin.h ; https://github.com/free-audio/clap/blob/main/include/clap/factory/plugin-factory.h |
| VST3 | one module exports `GetPluginFactory`; "the factory can create any number of classes" | one object per component instance | code | https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/VST+Module+Architecture/Index.html ; "the binary will only be loaded into memory ONCE" (JUCE forum, secondary: https://forum.juce.com/t/is-there-any-possible-to-load-vst3-in-multi-thread/64449) |
| Cmajor | the `Engine` compiles and links once | "An Engine can create many performer instances for a program, and they can all be run independently"; the low-level performer calls "are designed to be called synchronously on a real-time thread" | code | https://cmajor.dev/docs/Tools/C++API |
| SOUL | — | — | — | Unverified. A forum user says "SOUL is dead" and its developers started Cmajor (https://forum.juce.com/t/is-soul-still-being-actively-developed/64035); the README says nothing about status (https://github.com/soul-lang/SOUL) |
| Elementary Audio | one C++ `elem::Runtime<FloatType>` | each node a `GraphNode<FloatType>` object (`std::make_shared<MyCustomNode>(…)`); `setProperty` vs `process` must be thread-safe (an `std::atomic` in the example) | one runtime and its memory; on the web "Elementary will construct an AudioWorkletNode in which the Elementary runtime operates" | https://www.elementary.audio/docs/guides/Custom_Native_Nodes ; https://www.elementary.audio/docs/packages/web-renderer . Unverified: that the web runtime is wasm (the page doesn't say) |
| VCV Rack | a plugin library registers `Model`s; `createModule()` returns a new `engine::Module` | each `Module` object: params, inputs, outputs, lights; `process()` "Advances the module by one audio sample"; `dataToJson/dataFromJson` for extra state | code | https://vcvrack.com/docs-v2/structrack_1_1engine_1_1Module ; https://vcvrack.com/manual/PluginGuide . Threading: a pool of workers (community sources, secondary: https://mifki.com/blog/details-and-comparison-of-mirack-and-vcv-rack-multithreading-implementation-and-performance/) |
| Reaper JSFX (EEL2) | the script | memory "about 8 million slots" by default, up to 128 million (`maxmem=`); `gmem=` "allocate[s] their own global shared buffer" in a named space; "All memory and variables are zero on load, and are re-zeroed before calling @init" | `gmem` by name | https://www.reaper.fm/sdk/js/js.php . jsusfx makes one `NSEEL_VM_alloc()` VM per object (https://sources.debian.org/src/jsusfx/0.3.1-4/jsusfx.cpp/). Unverified: whether Reaper compiles code once per instance |
| Web Audio Modules 2.0 | `WamNode` extends `AudioNode`; `WamProcessor` extends `AudioWorkletProcessor`, made by a `WamNode`; `WamEnv` is "a global singleton" on the audio thread | `getState`/`setState`: "any serializable type" | the `WamEnv` | https://github.com/webaudiomodules/api . Unverified: wasm memory per instance (the README doesn't say) |
| AudioWorklet with wasm | "Each BaseAudioContext has a single AudioWorklet … in a single AudioWorkletGlobalScope", shared across the context | the "audio data needs to be cloned back and forth between the WASM heap and the audio data arrays" | one global scope, so one module instance can serve every processor | https://developer.mozilla.org/en-US/docs/Web/API/AudioWorkletGlobalScope ; https://developer.chrome.com/blog/audio-worklet-design-pattern |
| Emscripten Wasm Audio Worklets | C function-pointer callbacks with `void *userData`; processors "instantiated one or more times … as AudioWorkletNodes" | per node, via `userData` | built on Wasm Workers (shared memory) | https://emscripten.org/docs/api_reference/wasm_audio_worklets.html |
| wasm-bindgen (Rust → JS) | one module | `Box::into_raw(Box::new(WasmRefCell::new(ret))) as u32` is the handle; each method shim takes `me: u32`; `__wbindgen_foo_free(me)` drops it | one memory | https://docs.rs/crate/wasm-bindgen/0.2.69/source/guide/src/contributing/design/exporting-rust-struct.md |
| Bitwig Grid | — | — | — | Unverified: no public description of its engine (searches found only reviews) |

**Many-sandbox servers**
- **Fastly Compute runs on Wasmtime.** Fastly "switched to Wasmtime from another WebAssembly engine in March 2022" and
  "has since served trillions of requests using Wasmtime"
  (https://bytecodealliance.org/articles/wasmtime-1-0-fast-safe-and-production-ready).
  - Wasmtime 1.0: SpiderMonkey.wasm instantiation "went from about 2 milliseconds … to 5 microseconds"
    (https://bytecodealliance.org/articles/wasmtime-10-performance).
  - Lucet, Fastly's earlier engine, made "a WebAssembly instance for each of the tens of thousands of requests per second
    in a single process", "in under 50 microseconds, with just a few kilobytes of memory overhead"
    (https://www.fastly.com/blog/announcing-lucet-fastly-native-webassembly-compiler-runtime).
  - Lucet used "a memory region with reusable slots"; "acquiring the memory slot and populating the heap takes an
    average of 30µs"; zeroing via `madvise` "takes around 35-40µs/MiB" (https://www.fastly.com/blog/lucet-performance-and-lifecycle).
  - So: one instance (one memory) per request, recycled from a pool. The state is short-lived.
- **Cloudflare Workers:** "A single instance of the runtime can run hundreds or thousands of isolates"; "Each isolate's
  memory is completely isolated" (https://developers.cloudflare.com/workers/reference/how-workers-works/).
  - Memory is "128 MB" per isolate, and "This limit is per-isolate, not per-invocation. A single isolate can handle many
    concurrent requests" (https://developers.cloudflare.com/workers/platform/limits/).
  - So: one sandbox per *program*, shared by many requests, not one per request.

---

## C. Toolchain facts: many kernel states in one module's memory

### C1. The usual pattern

- **A handle into the module's own memory.** The module exports a constructor that returns a pointer into its linear
  memory, methods that take that pointer first, and a destructor. Examples: wasm-bindgen's shims, Faust's `$dsp`,
  gen~'s `CommonState*` (all in §B).
- **Or the host does the layout.** The module states its state size and the host places the instances. faustwasm sizes
  one memory from `dspMeta.size * voices` (§B).
- **Rust's defaults for `wasm32-unknown-unknown`:**
  - The default global allocator is `dlmalloc` (https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html;
    https://docs.rs/crate/dlmalloc/latest).
  - Its wasm back end gets pages with `memory_grow` and never gives them back: `free`, `free_part` and `can_release_part`
    do nothing or return false (https://github.com/alexcrichton/dlmalloc-rs/blob/main/src/wasm.rs).
  - By default rustc links wasm with a 1 MiB stack (`-z stack-size=1048576`) placed first (`--stack-first`)
    (https://github.com/rust-lang/rust/blob/master/compiler/rustc_target/src/spec/base/wasm.rs). std.wasm overrides it
    to 64 KiB (§D2).

### C2. What an allocator must avoid on a real-time thread

- **Unbounded time.** TLSF is O(1), "less than 200 processor instructions on a x86 processor"
  (https://www.cs.york.ac.uk/rts/publications/Masmano2008.html). The Rust port `rlsf` says its allocate and free are
  constant time and `no_std`. It does not support concurrent access, and its `GlobalTlsf` never returns pages
  (https://docs.rs/crate/rlsf/latest).
- **`memory.grow`.** In wasmtime this is a libcall, a limiter call and at least one `mprotect` syscall (A4).
- **First-touch page faults** on fresh pages (A4). `mlock(2)`: "Real-time processes that are using mlockall() to prevent
  delays on page faults should reserve enough locked stack pages before entering the time-critical section"
  (https://man7.org/linux/man-pages/man2/mlock.2.html).
- **Locks shared with other threads** (C5).
- **Derived:** a design that creates or frees kernel states in a live module memory would have to do it either off the
  audio thread or from memory already grown and touched.

### C3. Dynamic linking and position-independent code (PIC)

- **The spec is unstable.** `DynamicLinking.md`: "This ABI is still a work in progress. There is no stable ABI yet"
  (https://github.com/WebAssembly/tool-conventions/blob/main/DynamicLinking.md). From the same page:
  - `env.memory` is "shared between all wasm modules that make up the program";
  - `env.__memory_base` and `env.__table_base` "are `i32` imports that contain offsets into the linked memory and table";
  - `env.__stack_pointer` is "A mutable `i32` global representing the explicit stack pointer as an offset into the above
    memory";
  - the `dylink.0` section gives `memorysize`, the area "the loader should reserve for the module, which will begin at
    `env.__memory_base`";
  - "All code that gets linked into a WebAssembly dynamic library must be compiled as position independent";
  - addresses are imported from `GOT.mem` and `GOT.func`.
- **lld:** "Dynamic linking support is still experimental. The spec for this is not yet finalized"
  (https://lld.llvm.org/WebAssembly.html).
- **Rust:** wasm targets build with `relocation_model: RelocModel::Static` ("defaulting to `static` for now"), and
  allow `dynamic_linking: true` with `only_cdylib: true` ("we don't allow intermediate `dylib` crate types")
  (https://github.com/rust-lang/rust/blob/master/compiler/rustc_target/src/spec/base/wasm.rs). PIC needs a std
  rebuilt with `-C relocation-model=pic`. Secondary sources:
  - Pyodide: building std with PIC fails at link time because the prebuilt std isn't PIC
    (https://blog.pyodide.org/posts/rust-pyo3-support-in-pyodide/);
  - rust-lang/llvm-project#148 describes the manual workaround (https://github.com/rust-lang/llvm-project/pull/148).
  - Unverified: the current state on `wasm32-unknown-unknown`.
- **Derived:** two different non-PIC modules can share one memory only if they are linked to disjoint addresses
  (`--global-base` "Address at which to place global data", https://lld.llvm.org/WebAssembly.html) and use separate
  stacks. Two instances of the *same* non-PIC module in one memory collide on their static data and their stack (C5).

### C4. Static linking, merging, and shared-memory builds

- **Static linking is the normal path.** The std kernels are already one crate and one `cdylib`
  (`S7/std/kernels-src/Cargo.toml`, `crate-type = ["cdylib", "rlib"]`), so all std kernels are one module (§D2).
- **wasm-merge** (Binaryen) "Merge[s] wasm files into one": "if the second imports from 'foo', we will see that as an
  import from the first module after the merge" (https://manpages.debian.org/unstable/binaryen/wasm-merge.1.en.html).
  It has `--enable-multimemory`; the source "renames memory items" and checks that imported memories match their
  exports (https://chromium.googlesource.com/external/github.com/WebAssembly/binaryen/+/b4e7159f0ea2ef11f4cdaf48eb8ce50fdddf88cd/src/tools/wasm-merge.cpp).
  Unverified: that each input keeps its own memory, so the result needs multi-memory.
- **Importing memory instead of defining it:** `--import-memory`, "Import memory from the environment"
  (https://lld.llvm.org/WebAssembly.html).
- **A shared memory needs the atomics feature** and the `--shared-memory` linker flag. On `wasm32-unknown-unknown` that
  means a nightly std rebuilt with `-Zbuild-std`. Prebuilt std lacks atomics, so linking with `--shared-memory` fails
  (https://github.com/rust-lang/rust/issues/102157; https://github.com/rust-lang/rust/pull/147225). These are
  secondary; check them against the pinned toolchain (1.99.0, `S7/rust-toolchain.toml`).
- **Under `--shared-memory`, lld makes data segments passive.** It initialises them once in a synthesised
  `__wasm_init_memory` start function, guarded by `__wasm_init_memory_flag`
  (https://llvm.googlesource.com/lld/+/5542545ce7d176e066e0d7fe5b574295b4fa0372%5E%21/).

### C5. One memory, many instances of one module: stacks and TLS

- "LLVM does generate a 'shadow stack', a piece of linear memory managed by the Wasm code … using a Global acting as
  'stack pointer' … both this space and global need to be initialized for every thread"
  (https://blog.stackblitz.com/posts/thread-destroyer).
- In LLVM's TLS design, `__tls_base` is a per-instance global set by `__wasm_init_tls` (search summary of LLVM commits;
  unverified wording).
- **Derived:** if several instances (in one Store or several) share one memory, each needs its own stack region, and the
  host must set each one's stack-pointer global before its first call. With a single instance hosting every state
  there is one stack, and none of this applies.

### C6. Real-time rules for audio threads

- CLAP's `thread-check.h`: the audio thread's "execution should be as deterministic as possible to meet the audio
  interface's deadline (can be <1ms). There are a known set of operations that should be avoided: malloc() and free(),
  contended locks and mutexes, I/O, waiting, and so forth"
  (https://github.com/free-audio/clap/blob/main/include/clap/ext/thread-check.h).
- Timur Doumler: "you cannot do anything on the audio thread that might block the thread or otherwise take an unknown
  amount of time … such as allocating memory, performing any system call, or doing any I/O"; waiting on a mutex "also
  leads to priority inversion" (https://timur.audio/using-locks-in-real-time-audio-processing-safely). It cites "Ross
  Bencina's seminal article"
  (http://www.rossbencina.com/code/real-time-audio-programming-101-time-waits-for-nothing; the site refused the
  connection, so its text is unverified).
- **Page faults:** see `mlock(2)` in C2.
- **macOS audio workgroups:** real-time threads call `os_workgroup_join`, and `os_workgroup_leave` before exiting
  (https://developer.apple.com/documentation/audiotoolbox/adding-parallel-real-time-threads-to-audio-workgroups).
  WWDC20 session 10224 says joining is once per thread (https://developer.apple.com/videos/play/wwdc2020/10224/,
  per the search summary; unverified wording).
- **wasmtime on a real-time thread, derived from A6 and A7:**
  - A steady-state `TypedFunc::call` makes no heap allocation and no syscall.
  - The first call on a thread pays the TLS set-up unless `tls_eager_initialize` ran first.
  - A trap allocates an error and, on macOS, goes through Mach exception handling. Unverified: how long that takes.
  - `memory.grow` is a syscall.
  - Dropping a Store frees its memories, which is `munmap` or, with pooling, `madvise` plus locks.

---

## D. Derived numbers (arithmetic only, from the measured facts and the citations above)

### D1. Address space

- **P2, per memory:** 64 KiB pre-guard (`guard_before_linear_memory` defaults to true, `E/tunables.rs:266`) + 32 MiB +
  64 KiB guard = 32.125 MiB.
  - × 6,058 = 194,613 MiB = 190.05 GiB. That matches the measured 190 GiB.
- **P1, per memory:** 32 MiB + 4 GiB + 32 MiB = 4.0625 GiB. No growth reserve is added, because std.wasm declares no
  maximum, its 4 GiB implicit maximum is ≤ the reservation, and so `mmap.rs:84-88` sets the extra to zero.
  - Two p50 renderers: 2 × 6,058 × 4.0625 = 49,221 GiB = 48.07 TiB. That is 37.6% of the 128 TiB ceiling (A9),
    against the measured 37.9%.
- **The 25% rule with memories that elide bounds checks:**
  - Budget: 0.25 × 131,072 GiB = 32,768 GiB for two renderers, so 16,384 GiB each.
  - At 4.0625 GiB a memory: 16,384 / 4.0625 = **4,033 memories per renderer**.
  - At the smallest eliding layout, 4 GiB reservation with a 64 KiB guard on each side: 16,384 / 4.000122 ≈ 4,095.
  - So p50's 6,058 instances can't each have an eliding memory.
  - If memories were per track: 50 × 4.0625 = 203 GiB a renderer, 0.16% of 128 TiB.

### D2. std.wasm today

Section headers read from `S7/std/kernels/std.wasm` (143,431 bytes):
- one memory, minimum 2 pages (128 KiB), no maximum;
- one global, the stack pointer, initialised to 65,536;
- 176 exports;
- a 130,498-byte code section;
- two active data segments at 65,536 (8,200 B) and 73,736 (1,608 B), 9,808 B in all.

The 64 KiB stack comes from `S7/std/kernels-src/.cargo/config.toml` (`-zstack-size=65536`, "Each node gets its own
instance, so keep the per-instance stack small").

**Derived:**
- every one of the 6,058 instances repeats the module's stack region and its 9,808 bytes of data;
- the data spans bytes 65,536–75,544: three 4 KiB pages on Linux, or one 16 KiB page on the Mac.
- On the Mac the data is copied at every instantiation (A4).

### D3. Footprint per instance

- Linux: 195 MB / 6,058 ≈ 32.2 kB, about 7.9 pages of 4 KiB.
- Mac: 575 MB / 6,058 ≈ 94.9 kB, about 5.8 pages of 16 KiB.
- These take MB as 10^6 bytes. Read as MiB, they are 33.8 KiB and 99.5 KiB.
- Unverified: how those pages split between stack, data, heap and wasmtime's own structures.

### D4. Per call (9,250 kernel calls per 512-frame callback)

| Part | µs per callback | ns per call |
|---|---|---|
| Kernels' JIT code | 5,661 | 612 |
| wasmtime host code | 766 | 83 |
| m2-runtime's own code | 416 | 45 |
| Port copies (memcpy) | 408 | 44 |
| Trampolines | 37 | 4.0 |

- O1's explicit bounds checks: 9.6 M / 9,250 ≈ 1,038 extra instructions per call. +85 to +105 µs is 1.16% to 1.44% of
  HEAD P2's 7,312 µs.

---

## E. Not verified

- Linux `mprotect` lock behaviour (the kernel sources couldn't be fetched).
- Whether Reaper compiles JSFX code once per instance.
- WAM 2.0's wasm memory per instance.
- Bitwig Grid's internals.
- SOUL's official status.
- Whether Elementary's web runtime is wasm.
- How wasm-merge treats several memories.
- The current state of Rust PIC on `wasm32-unknown-unknown`.
- Whether shared-memory linking works with the pinned Rust 1.99.0.
- The exact text of WWDC20 10224 and of Ross Bencina's article.
- Which holders keep a `SharedMemory` alive beyond the dummy instances.
- What a trap costs on macOS.
- How p50's touched pages split per instance.
