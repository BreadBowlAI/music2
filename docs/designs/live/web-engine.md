# The web engine (Part 2 of the live note)

**In short**
- Part 2 of [design.md](design.md): music2's engine, the same Rust, built to WebAssembly and run in a browser, playing the same kernels with the same bits as the app. This note says what stands in the way, measured, and how to build it in small tracks.
- **The kernels give the same bits.** All 56 live kernels music2 ships (std's and its 7 packs'), run on the same inputs for 750 blocks, give the same output bytes, bit for bit, under wasmtime, Node's V8, and headless Chrome 133's page thread and Workers.
- **But not on the browser's audio thread.** Chrome's AudioWorklet thread flushes subnormals (numbers too small for the normal float range) to zero. There 25 of the 56 differ when their inputs reach subnormals, and `std:svf`, the filter in almost every synth, differs even on ordinary inputs, in its tail at about −690 dB. So the engine should run in a **Worker**, and the AudioWorklet should only copy its blocks to the speakers.
- **Speed is the same.** V8 runs the kernels about as fast as wasmtime: the median kernel takes 0.97 to 1.06 times as long. The `sounds` example takes 22% of one core to render here, so a song of that size should fit in one browser core on a fast computer. A 50-track song won't, and needs threads, which come later.
- **The engine doesn't build for the web yet.** Two walls: `m2-os` has no web folder, so every crate but `m2-ir` stops there; and `wasmtime` can't build for `wasm32` (and couldn't compile machine code in a page if it did). Past them, the build would succeed, but the clock, threads and files would fail when the engine runs.
- **The fix is one small trait**, a kernel host. wasmtime is one backend and the browser's `WebAssembly` the other. The runtime already copies each kernel's inputs in and its outputs out, so the trait needs only "make a sandbox, call an export, copy bytes in and out, seal".
- **Song code is a separate problem.** V8's `Math` isn't the `libm` crate that song code's `Math` is in the app: up to 410 of 4,096 inputs differ, by 1 or 2 ulps (units in the last place). So song code must run in QuickJS built to WebAssembly, or the page must use music2's own math. Part 2 starts from the song's IR (its compiled graph) and leaves song code to Part 3.
- Status: **accepted by the owner, 2026-11-28**, with all six decisions as proposed ("I like draft 150, and go ahead"). Its tracks are built in order, each its own pull request. Nothing in this note changes the app. The probe is in [probe/](probe/); its one Rust file is an example of `m2-runtime` (`cargo run --example web-probe`), which nothing in the app runs.

## Words used here

| Word | Meaning |
|---|---|
| kernel | a small sound part in WebAssembly, such as a filter ([kernel-abi.md](../../kernel-abi.md)) |
| sandbox | one instance of a kernel module and its one memory |
| AudioWorklet | the browser's audio thread: a script there fills each 128-frame block for the speakers |
| Worker | a background thread of a web page, at normal priority |
| SharedArrayBuffer | memory two of a page's threads can both see; a page gets it only when it is *cross-origin isolated*, which takes two HTTP headers (COOP and COEP) |
| subnormal | a float smaller than the normal range (under about 1.2 × 10⁻³⁸ for `f32`). WebAssembly keeps them, and so does music2 ([RENDER-2a](../../architecture.md#offline)) |
| flush to zero (FTZ) | a CPU mode that turns subnormals into 0. It is faster, but it gives different bits |
| V8 | the JavaScript and WebAssembly engine of Chrome and Node |
| ulp | one step of the last bit of a float |
| IR | the song's compiled graph, as `m2 check --json` prints it |

## 1. What stops the engine building for the web

Each crate was built with `cargo build --locked --target wasm32-unknown-unknown -p <crate>`, using the pinned Rust 1.99 and its `wasm32-unknown-unknown` target, which was already installed. `--keep-going` showed every crate that fails.

### The walls the build hits

| Crate | Result | Why |
|---|---|---|
| `m2-ir` | **builds** | |
| `m2-os` | 3 errors | `compile_error!` "music2 builds on macOS and Linux" ([PLAT-1](../../architecture.md#platforms)); `compile_error!` in `fp.rs`: "RENDER-2a: music2 sets the floating-point mode of x86-64 and arm64 CPUs only"; no `os` module (there's no `mac/` or `linux/` folder for this target) |
| `m2-song`, `m2-pool`, `m2-plugins`, `m2-script`, `m2-session` | stop at `m2-os` | each depends on it |
| `m2-runtime`, `m2-play` | stop at `m2-os` and `wasmtime` | |
| `wasmtime` 50.0.2 (as music2 builds it: `cranelift`, `runtime`, `std`) | 3 errors | its file mapping (`open_file_for_mmap`, `Mmap`, `MmapVec::new_mmap`) has no form here. Even if it built, it compiles kernels to machine code, which a page can't run. Its interpreter, Pulley, is in `Cargo.lock`, but it would run WebAssembly inside WebAssembly |

These build alone for `wasm32`: `hound`, `claxon`, `libm`, `m2-kernel`, `bytemuck`, `toml` 1.2, `serde_json`, `sha2` 0.11, `icu_normalizer`, `anyhow`, `crossbeam-channel`, `libc`, `oxc`, `rquickjs-sys`, and `cpal`. `cpal` brings its web backend, through `wasm-bindgen`, `js-sys` and `web-sys`, which are already in `Cargo.lock`. The web player won't use it: in a browser, the AudioWorklet is the audio device.

### What `m2-os` would need on the web

The engine's crates call only a few of `m2-os`'s parts. Its portable modules reach 65 private functions of each OS's folder, but most of those are for the app: windows, the plugin host's IPC, the main loop, file watching.

| Crate | Calls into `m2-os` |
|---|---|
| `m2-song` | `dirs::cache`, `dirs::packs`, `dirs::packs_under`, `fs::Stamp` |
| `m2-runtime` | `dirs::cache`, `dirs::packs`; `fp` (the float mode); `fs::folder_of`, `make_dirs`, `push`, `read_at`, `read_full_at`, `sync_dir`, `watch_reads`; `rt::engine_thread`, `end_engine_thread`, `Group`; `thread::background` |
| `m2-pool` | `rt::Wake` (the futex a caller sleeps on) |
| `m2-play` | `host::microphone_refused`, `process::cores`, `rt::*` (not needed on the web: below) |
| `m2-plugins` | the IPC, the main loop, windows, processes: CLAP only, never on the web |

### What would build but fail when it runs

`wasm32-unknown-unknown` has no OS, so Rust's standard library gives these a form that fails when called. A tiny module built with the pinned Rust and run under Node shows how each one fails:

| Call | On the web | Lines that name it, outside tests |
|---|---|---|
| `Instant::now()` | traps ("unreachable"): it panics, and a panic aborts | runtime 17, play 24 |
| `std::thread::spawn` | traps; `Builder::spawn` returns an error | runtime 3 (the stretch thread, the limit's ticker, a fault test), play 9 |
| `std::fs` | every call returns an error | runtime 38, song 68 |
| `std::env::var` | always "not set" | runtime 4, play 4, song 2 |
| `catch_unwind` | can't catch: the target's panic strategy is `abort` | play 1 (the device callback) |
| the `m2-pool` threads | none: no threads ([below](#threads)) | runtime 9 |

### `m2-plugins` in the runtime

The runtime names `m2_plugins` in 67 lines, for its types (`Handle`, `Owner`, `Event`, `PluginHost`, `Blob`, `Found`, `RenderMode`), not for CLAP itself. On the web, the types should stay and the CLAP host should go: put the host behind a default Cargo feature, so the web build leaves it out, and a song with a plugin says in words that the web can't play it.

### `core/play` stays the app's

`m2-play` is the device, its threads, the input rings, the disk thread and the resampler. On the web, those are a Worker, a ring, and the AudioWorklet ([decision 1](#decisions-proposed)). The engine the web needs is `m2-runtime`'s `Renderer` and `Engine`. At 48 kHz the AudioContext can run at the song's rate (`new AudioContext({ sampleRate })`), so the device-rate converter ([RENDER-31](../../architecture.md#sample-rate)) isn't needed there either.

## 2. The kernel host interface

### What the runtime uses of wasmtime today

wasmtime is named in 7 of the runtime's files: `sandbox.rs` (17 lines), `packages.rs` (5), `threads.rs` (4), `limit.rs` (2), `lib.rs` (2), `offline.rs` (1), `tests.rs` (1).

| Use | Where | What for |
|---|---|---|
| `Config`: NaN canonicalisation off, relaxed SIMD off, `memory_may_move(false)`, epoch interruption for packs | `packages.rs` `engine()` | two engines: std's with no checks, packs' with the time checks (ABI-6) |
| `Module::new`; `module.imports()` and `exports()` with their types; `Engine::same` | `packages.rs` (`verify_module`: ABI-3; `checks`) | compile once; refuse a module whose imports, memory or exports are wrong |
| `Store<Host>` with a `ResourceLimiter` | `sandbox.rs` | memory up to 256 MiB while a sandbox is born, none once sealed; one instance, memory and table |
| `Func::wrap(asset_read)`, using `Caller` and `data_and_store_mut` | `sandbox.rs` | the one import: copy part of an asset into the kernel's memory |
| `Instance::new`, `get_memory`, `get_typed_func` | `sandbox.rs` | `m2_alloc`, and each kernel's `_ports`, `_new`, `_process`, `_reset`, `_active` |
| `data_ptr`, `data_size`, `data_mut`, and the address kept at the seal | `sandbox.rs`, `lib.rs` (`Ports`) | write the settings block and port table; copy inputs in and outputs out |
| `set_epoch_deadline` before each call; `Trap::Interrupt` | `sandbox.rs`, `limit.rs` | the time limit, told as `TooSlow` |
| `increment_epoch` from a ticker thread, every 10 ms | `limit.rs` | moves the epoch |
| `Engine::tls_eager_initialize` | `threads.rs` | each render thread's wasmtime state, made at once |

Every call into a kernel has the same shape: up to four `i32`s in, and nothing or one `i32` out. The engine never shares memory with a kernel. It already copies each input into the kernel's memory and each output out of it (`KernelNode::process` in `lib.rs`).

### The proposed trait

```rust
/// What runs kernel modules: wasmtime in the app, the browser's WebAssembly on the web.
pub trait KernelHost: Send + Sync + 'static {
    type Module: Clone + Send + Sync;
    type Sandbox: Sandbox;
    /// Compile a module that `check_abi` and `verify_module` passed. `checked`: with the time
    /// checks (ABI-6), for a pack that isn't music2's own.
    fn compile(&self, bytes: &[u8], checked: bool) -> Result<Self::Module>;
    /// One instance and its one memory; its start function runs within `limit`. Its
    /// `m2.asset_read` reads `assets`.
    fn instantiate(&self, module: &Self::Module, assets: Assets, limit: Duration) -> Result<Self::Sandbox>;
    /// What a thread that calls kernels does first (wasmtime: its per-thread state).
    fn thread_start(&self) {}
}

pub trait Sandbox: Send {
    /// An export by name, checked against its shape: `m2_alloc`, or `<kernel>_<call>`.
    fn export(&mut self, name: &str, shape: Shape) -> Result<Export>;
    /// Call it with up to four i32s, within `limit` (None: no limit, as std's).
    /// Its i32 result, or 0 for none.
    fn call(&mut self, f: Export, args: &[u32], limit: Option<Duration>) -> Result<u32, Fault>;
    /// From now on its memory never grows. Touches every page once.
    fn seal(&mut self) -> Result<()>;
    fn len(&self) -> usize;
    /// Copies in and out of its memory. Never allocates.
    fn write(&mut self, at: usize, bytes: &[u8]);
    fn read(&self, at: usize, out: &mut [u8]);
}

/// How a call failed: past its limit (ABI-6), memory it may not take (RENDER-32), or a trap.
pub enum Fault { TooSlow, OutOfMemory, Trap(String) }
```

| Item | wasmtime backend | browser backend |
|---|---|---|
| `compile` | `Module::new` on one of the two engines, as now | the page compiles the bytes once (`WebAssembly.compile`) and keeps the module by number |
| `instantiate` | `Store`, `Instance`, the limiter, as now | `new WebAssembly.Instance(module, { m2: { asset_read } })` in the JS glue |
| `export`, `call` | `TypedFunc`; `set_epoch_deadline` | the glue calls `instance.exports[name](a, b, c, d)` |
| `write`, `read` | `copy_from_slice` on the address kept at the seal | the glue copies between the engine's memory and the kernel's (`Uint8Array.prototype.set`) |
| `seal` | the limiter refuses growth from then on | the glue notes the memory's size; a call that grew it is an `OutOfMemory` fault, found after the call |
| `asset_read` | `Func::wrap`, as now | served by the glue alone, from each asset's place in the engine's memory, registered at birth: a kernel's call never re-enters the engine |
| `thread_start` | `Engine::tls_eager_initialize` | nothing |
| the time limit | epochs, as now | none in Part 2 ([below](#the-time-limit)) |

Two checks move out of wasmtime and into the runtime's own code, so both backends refuse a module in the same words:
- **ABI-3's signatures.** The browser's `WebAssembly.Module.exports()` lists names and kinds, not signatures. The runtime already reads a module's custom sections from its bytes (`check_abi`). It would read the type, import, function and export sections the same way: about a hundred lines, or `wasmparser`, which is in `Cargo.lock` already as wasmtime's, but whose use by a core crate would be a dependency change.
- **The memory cap while a sandbox is born.** It is checked after each birth call, by size.

The wasmtime backend stays the default (a Cargo feature), so the app is unchanged, and the runtime chooses its backend by feature, not by target: it names no platform ([PLAT-2](../../architecture.md#platforms)). The browser backend needs `extern "C"` imports from the page, which PLAT-2 allows only in `m2-os`, so it lives in a new crate, `core/web`: the web's shell, an exception as the desktop shell is ([decision 2](#decisions-proposed)).

## 3. Bits: the probe

### What it does

[probe/wasmtime.rs](probe/wasmtime.rs) reads every `[[kernel]]` of std's and the 7 packs' `m2pkg.toml`, leaving out the 3 offline kernels: 56 kernels. For each one, it makes a state as `sandbox.rs` does: the same block layout, settings block, port table and prewarm, with no values and no assets. It feeds the state fixed inputs for 750 blocks (2 s at 48 kHz) and hashes everything it gives, block by block: each output, an events output's notes, and `k_active`. [probe/host.mjs](probe/host.mjs) is the same host in JavaScript. It has no Node or DOM calls, so Node, a page, a Worker and an AudioWorklet all run it. Both hosts also hash their inputs, and those hashes match everywhere, so every host fed the kernels the same bytes.

| Input | What it gets |
|---|---|
| audio | its own noise stream per channel (an LCG, the same in Rust and JS): blocks 0–299 noise at half scale, 300–549 silence (a tail rings out), 550–749 subnormal noise. With `--normal`, silence instead of the subnormals |
| a control | its default, or one by unit (`hz` with no default: a sweep from 50 Hz; `beats`: the beat at 120 bpm) |
| `gate`, `trig…` | on for blocks 20–199 and 320–339 |
| `hits` | a hit every 50 blocks |
| notes | 3 notes On and Off (one Off at frame 127, two notes in a block, a `from_pitch` of NaN) |

48 kernels give sound. 5 give silence on these inputs: the Sampler and `regions` need files and values; `regionmark`, `hitcount` and the Drum need `drumhit`'s marks. Their bits match too, but they prove less. `@m2/notes`' chord, echo and lock give notes, and those bytes match.

### The result

On this server (AMD Ryzen 9 7950X3D, Linux, x86-64), with the kernels' committed `.wasm` files:

| Host | Same output bits as wasmtime: inputs with subnormals | inputs without | Flushes subnormals |
|---|---|---|---|
| wasmtime 50 (the runtime's own engine) | 56 of 56 | 56 of 56 | no |
| wasmtime 50 with flush-to-zero on | **31 of 56** | **55 of 56** (`std:svf` differs) | yes (the probe sets it) |
| Node 24.22 (V8 13.6) | 56 of 56 | 56 of 56 | no |
| Chrome 133 headless, the page's thread | 56 of 56 | 56 of 56 | no |
| Chrome 133 headless, a dedicated Worker | 56 of 56 | 56 of 56 | no |
| Chrome 133 headless, an AudioWorklet (OfflineAudioContext) | **31 of 56** | **55 of 56** | **yes** |
| Chrome 133 headless, an AudioWorklet (AudioContext, real time) | **31 of 56** | **55 of 56** | **yes** |

- In both AudioWorklets, every one of the 56 output hashes is exactly wasmtime's **with flush-to-zero on**, in both runs. So the difference is the flush, and only the flush. A check inside the worklet agrees: `2⁻¹⁰²² / 2` is 0 there, a subnormal on the page's thread.
- With subnormal inputs, 25 differ: `std:` scale, add, blend, gain, svf, drive, duck, limit, chorus, compress, eq3; `@m2/effects`' phaser, tremolo, widen, clip, flanger, grains; all 5 of `@m2/mixing`'s; `@m2/vocals`' pitch, shift, vocoder.
- With ordinary inputs, only `std:svf` differs. It makes subnormals itself as its tail dies away: 340,555 samples differ, in blocks 306 to 749 (after its input fell silent), none larger than 3.3 × 10⁻³⁵ (about −690 dB). That can't be heard, but it breaks RENDER-33, and the svf is in almost every synth.
- No kernel gave a NaN or an infinity on these inputs.
- **No SIMD.** None of the 8 modules uses SIMD at all: each compiles under a wasmtime engine with SIMD turned off. The runtime's engine already refuses relaxed SIMD. So the two places where WebAssembly may differ between machines don't arise, except NaN's bits, and the host silences non-finite output anyway.

This is the decisive finding. The browser's audio thread can't give music2's bits, and no WebAssembly or JavaScript code can change its float mode. So the engine runs where the mode is WebAssembly's own: a Worker.

## 4. The real-time budget

One 128-frame block at 48 kHz lasts **2,667 µs**. The probe also timed each kernel with block 100's inputs held, for 750 calls, keeping the best of three runs. The numbers come from one run (the `--normal` run) on a shared machine, so they are rough. The AudioWorklet's numbers use `Date.now()` (a worklet has no `performance`), so they are rougher still.

| Kernel | wasmtime µs | Node µs | Chrome page µs | Chrome Worker µs | Chrome worklet µs |
|---|---:|---:|---:|---:|---:|
| `std:sine` | 1.45 | 0.64 | 1.56 | 1.51 | ~0 |
| `std:svf` (stereo) | 4.32 | 4.77 | 4.08 | 5.14 | ~5 |
| `std:chorus` | 12.6 | 8.8 | 14.2 | 12.2 | ~13 |
| `@m2/effects:reverb` | 14.6 | 15.0 | 14.1 | 10.7 | ~13 |
| `@m2/synths:synth` (one voice) | 15.0 | 9.6 | 12.9 | 12.0 | ~12 |
| `@m2/mixing:multiband` | 39.7 | 42.7 | 34.3 | 38.6 | ~40 |
| `@m2/vocals:vocoder` | 95.8 | 94.2 | 88.0 | 96.7 | ~92 |
| all 56, one block each | 531 | 546 | 504 | 488 | 507 |

For the 23 kernels that take 5 µs or more, a kernel's time in V8 against wasmtime's has a median of 1.06 (Node), 1.04 (Chrome page) and 0.97 (Chrome Worker), and ranges from 0.5 to 1.8. So V8 is about as fast as wasmtime, and the kernels' part of a song costs the same in a browser.

What real songs cost, rendered on one thread by `m2 render --threads 1` (release, the same server, a scratch `HOME`), with their IR counted from `m2 check --json`:

| Song | Kernels outside instruments | Instruments, voices | Kernels in all voices, at most | Render, 1 thread | Share of one core |
|---|---:|---:|---:|---|---:|
| `examples/first-song` | 20 | 6, 44 | 240 | 16.2 s in 0.72 s | 4.4% |
| `templates/starter` | 29 | 8, 58 | 396 | 20.1 s in 1.54 s | 7.7% |
| `examples/sounds` (every std synth and drum) | 96 | 43, 379 | 379 | 32.2 s in 7.19 s | 22% |
| `core/session/tests/fixtures/vocoder` | 9 | 2, 17 | 75 | 8.6 s in 0.46 s | 5.3% |

The estimate: the kernels cost the same in a browser. The engine's own code (mixing, scheduling, copies) is native Rust today, and in WebAssembly it may take up to about twice as long. That part wasn't measured, because the engine doesn't build yet. So `sounds` would take something like 25–45% of one core of this machine in a Chrome Worker. A laptop core is perhaps 1.5–3 times slower, which brings it to 40–100% or more. Songs the size of the examples fit one core on a desktop, and are tight on a laptop. The gates' 50-track song, p50, already takes 84% of a 10.7 ms period on one native thread ([recorded in the contract](../../architecture.md#engine-shape-step-7), step 7), so it needs threads.

## 5. The rest of the sketch

### Song code

The app runs song code in QuickJS. It strips TypeScript with `oxc`, rewrites calls to record their sites, puts `Math` on the `libm` crate and turns `**` into libm's `pow` (PLAT-4), deletes `Date`, `performance`, `eval` and the `Function` constructors, and takes each source place from QuickJS's stack (`core/script/src/lib.rs`). Could the page's own V8 do the same? The probe checked the math that matters (`wasmtime.rs --libm`, [probe/libm.mjs](probe/libm.mjs)): 4,096 inputs per function, held against `libm`, bit for bit.

| Function | Node 24 (V8 13.6): differ | Chrome 133: differ | Most ulps apart |
|---|---:|---:|---:|
| `sin` | 40 | 106 | 1 |
| `cos` | 32 | 120 | 1 |
| `log` | 4 | 4 | 1 |
| `log10` | 68 | 68 | 1 |
| `tanh` | 322 | 322 | 2 |
| `cbrt` | 326 | 326 | 1 |
| `pow` (10^(dB/20) and 2^(st/12)) | 410 | 410 | 1 |
| `tan`, `exp`, `log2`, `atan`, `asin`, `acos`, `sinh`, `expm1`, `log1p`, `atan2` | 0 | 0 | 0 |

So V8's `Math` isn't `libm`, and two V8 versions don't even agree with each other on `sin` and `cos`. A knob written `db(-6)` would give other bits in the IR. Song code in a page needs one of these:
- **QuickJS built to WebAssembly, running the same `m2-script`.** Then the IR is the same by construction: QuickJS's own number printing, `libm`, `oxc`'s rewrites. `oxc` builds for `wasm32-unknown-unknown`, and so does `rquickjs-sys` 0.14, which ships a WebAssembly shim and a libc. It leaves 74 symbols to the page: 45 WASI calls to stub, 23 compiler helpers for 128-bit numbers and complex products, the linker's 4, and a clock hook. It isn't linked or run yet.
- Or V8 with `Math` replaced by `libm` from the engine's module, plus a new way to find source places. That is a second implementation of the build, and the IR would have to be compared against the app's for every song.

Part 2 needs neither: it renders from the IR the app makes. The choice is Part 3's ([decision 4](#decisions-proposed)).

### Samples and packs over HTTP

Every byte the engine reads comes from a file today: `std::fs` in 68 lines of `m2-song` and 38 of `m2-runtime`, decoded by `hound` and `claxon` (which take any reader, so bytes work). On the web the page fetches the files and gives the engine bytes. A pack's listing already names each file's sha256 (`m2_song::pack::Listing`), so the engine can check that what it fetched is what the app plays: same bytes, same bits. For Part 2, every file is fetched whole before Play and held whole. Streaming a lane from the disk (`stream.rs`, RENDER-30) becomes "held whole" on the web, and web sample sets are Part 3's.

### The time limit

On the web there are no epochs, and nothing can stop a call on the AudioWorklet's thread. Part 2 plays only std and music2's own packs, which have no limit in the app either ([ABI-11](../../architecture.md#kernel-abi-reservations)), so it needs nothing. For anyone's pack (Part 3), the Worker gives a way: the page can stop a Worker stuck in a kernel. The probe tried it ([probe/stuck.mjs](probe/stuck.mjs)). In Chrome 133, a Worker spinning in an endless WebAssembly loop was stopped with `terminate()`, a new Worker answered 10 ms later, and the page's own thread was never late. The engine would mark "in a call into node X" in a SharedArrayBuffer, so the watchdog can name the sound (ABI-7). The price is that the whole song restarts without that sound, where the app silences only that sound. Compiling the checks into a pack's module, as wasmtime's epochs do, would match ABI-6 exactly. It needs a module rewriter (`wasmparser` and `wasm-encoder` are in `Cargo.lock` as wasmtime's), so it is a later choice.

### Threads

- In the probe's AudioWorklets, `SharedArrayBuffer` exists when the page is cross-origin isolated (COOP and COEP headers). The probe ran only isolated, so the case without the headers wasn't measured.
- One core first. The renders are the same bits on any number of threads ([RENDER-2](../../architecture.md#offline)), so one core is enough for the proof.
- Several cores later. Rust's standard library for `wasm32-unknown-unknown` is built without atomics (the target's features list none), so a shared-memory engine on several Workers needs a nightly `-Z build-std`, or another target such as `wasm32-wasip1-threads`. That is a decision for when a song needs it.

## Decisions (the owner said yes, 2026-11-28)

1. **The engine runs in a dedicated Worker, not in the AudioWorklet.** The worklet only copies the Worker's blocks to its output with `TypedArray.prototype.set`, never sample by sample: on a thread that flushes, a JavaScript read of a subnormal `f32` is 0. They talk through a ring in a SharedArrayBuffer, or through a `MessagePort` when the page isn't isolated (unmeasured).
   - *Why:* measured. A Worker gives wasmtime's bits for all 56 kernels; the audio thread doesn't, and no code can change its float mode. Keeping heavy WebAssembly off the audio thread, behind a ring, is also a known pattern for web audio.
   - *The cost:* a ring of a few blocks between the two (some milliseconds of latency), and a thread without real-time priority, which may run late under load. Part 2's track measures the underruns.
   - *The other choice:* run in the worklet, with a new rule that every kernel and the engine's own math give the same bits with flush-to-zero on (55 of 56 already do on ordinary inputs; `std:svf` would set its dying state to 0). That gives lower latency and real-time priority, but music2 couldn't hold anyone else's pack to the rule. Keep it open for later.
2. **One `KernelHost` trait in `m2-runtime`, with wasmtime behind a default feature, and the browser's backend in a new crate, `core/web`.** `core/web` holds the hand-written JS glue, the engine's exports and the page's imports, and is the web's shell under PLAT-2, as the desktop shell is.
   - *Why:* the runtime keeps naming no platform. `extern "C"` imports stay out of the core. The app's build doesn't change.
   - *Not `wasm-bindgen`:* the glue is small, a few hundred lines; `wasm-bindgen` needs its command-line tool at exactly the crate's version; and hand-written glue keeps the module's bytes under music2's control. `wasm-bindgen` 0.2.129 is in `Cargo.lock` already (through `cpal`), so the choice can be made again later.
3. **The web is a third platform:** `wasm32-unknown-unknown`, Chrome first. `m2-os` gets a `web/` folder. The float mode is a no-op (WebAssembly has one). There are no folders: the caches live in memory, as PLAT-7 already allows. Files are `Unsupported` (PLAT-3): the engine takes bytes the page gave. There is one thread. Edit PLAT-1, PLAT-2 and RENDER-2a first, as the contract says.
4. **Part 2 renders from the IR; song code in a page is Part 3, in QuickJS built to WebAssembly.**
   - *Why:* V8's `Math` gives other bits (measured), and the rest of the build (sites, source places) is QuickJS's. The same `m2-script` gives the same IR by construction.
5. **One core in Part 2.** Threads wait for a song that needs them, and for the choice of nightly `build-std` or another target.
6. **No time limit in Part 2**, since only std and music2's own packs play. Anyone's pack on the web (Part 3) gets the Worker watchdog first.

## Risks

| Risk | What would show it | Measured? |
|---|---|---|
| Firefox and Safari: their Workers' float mode, their audio threads, their WebAssembly floats | the probe, run in each | no: only Chrome 133 and Node 24, on Linux x86-64 |
| ARM CPUs (Apple silicon, phones): a NaN's default bits differ from x86's; flush-to-zero works differently on ARM | the probe on a Mac | no |
| The engine's own code in WebAssembly, and a copy through JS for every kernel call (hundreds a block) | the first engine build, timed | no |
| A Worker isn't real-time: blocks late under load | underruns counted in the Part 2 suite | no |
| Cross-origin isolation is a burden for pages that embed music2 (it limits what they can load from other sites) | the `MessagePort` path's latency | no |
| Memory: one WebAssembly memory per sandbox. Node held 4,000 instances of `std.wasm` (the limit is 3,999), 500 MiB of memories and a 277 MB process, each made in 0.2–0.35 ms. A tab's limits, and whether V8 drops its guard pages past some count, weren't measured | the 26 renders in Chrome; a test at the sandbox limit | in Node only |
| The probe proves the kernels, not the engine: the mix, the scheduler, the WAV writer and the fades are host code | the 26 renders in Chrome, Part 2's proof | no |
| A future Chrome changes how a Worker computes | the Part 2 suite, on every pull request | — |

## The build, in small tracks

Each track is one pull request with its own test, written first. Each track's contract edit comes first, in its own commit.

| Track | What | Its test | Needs |
|---|---|---|---|
| `web-os` | PLAT-1, PLAT-2 and RENDER-2a for the web; `m2-os`'s `web/` folder: the float mode, `rt::Wake`, no dirs, files `Unsupported`; `m2-song` and `m2-pool` build for `wasm32` | CI builds `m2-ir`, `m2-os`, `m2-pool` and `m2-song` for `wasm32-unknown-unknown` | the owner's yes |
| `kernel-host` | the `KernelHost` trait; wasmtime behind it, as a default feature; ABI-3's checks read from the module's bytes | every test and the 26 renders unchanged: no bit moves | `web-os` |
| `plugins-off` | `m2-plugins`' CLAP host behind a default feature; without it, a song with a plugin is told in words | a runtime build without the feature refuses a plugin song in those words | — |
| `runtime-web` | `m2-runtime` builds for `wasm32` with neither wasmtime nor CLAP: the clock from the caller, no threads (warped clips stretched before Play, on the one thread), files as bytes | CI builds it; a native test runs the same paths with the "bytes" sources | the three above |
| `web-node` | `core/web`: the browser backend in JS glue, the engine's exports (load IR and files, render), and a Node script that renders the 26 renders | the 26 renders in Node match `e2e/renders.sha256` (Node's V8 gave Chrome's bits for every kernel) | `runtime-web` |
| `web-chrome` | the engine in a Chrome Worker, a SharedArrayBuffer ring, the AudioWorklet that copies | a browser suite: the 26 renders' hashes from a Chrome Worker; `templates/starter` played for 30 s through the worklet, underruns counted | `web-node` |
| later | threads; the module rewriter for packs' time limit; the "same bits with flush-to-zero" rule, if the worklet is wanted | their own notes | a song or a pack that needs them |

## The probe

All in [probe/](probe/). Run it with `docs/designs/live/probe/run.sh <out dir>`. It writes its results under `<out dir>`, never in the repo. Chrome needs `M2_CHROME` (a `chrome-headless-shell`) and `M2_PUPPETEER` (the folder of `puppeteer-core`, as `e2e/node_modules/puppeteer-core`). Without `M2_CHROME`, Chrome is left out. It added no dependency, and nothing in the app runs it.

| File | What it is |
|---|---|
| `wasmtime.rs` | the reference host: an example of `m2-runtime` (`[[example]] web-probe` in `core/runtime/Cargo.toml`) on the runtime's own engine. It writes `cases.json` and `wasmtime.json`; `--flush` sets flush-to-zero; `--normal` uses no subnormal inputs; `--libm` writes the `Math` table; `M2_PROBE_DUMP=<id>` writes one kernel's output bytes |
| `host.mjs` | the same host in JavaScript, for any WebAssembly engine |
| `node.mjs` | Node's run |
| `chrome.mjs`, `page.mjs`, `worker.mjs`, `worklet.mjs` | headless Chrome on a local server with COOP and COEP: the page's thread, a Worker, an OfflineAudioContext's worklet and an AudioContext's worklet |
| `compare.mjs` | the tables above, from the runs' files |
| `math.mjs`, `libm.mjs` | `Math` against `libm` (Chrome's comes from `page.mjs`) |
| `stuck.mjs` | a Worker stuck in a kernel's loop, stopped and replaced |
| `run.sh` | all of it, both inputs |

The run that gave the numbers in this note: Rust 1.99.0, wasmtime 50.0.2, Node 24.22.2, Chrome headless shell 133.0.6943.141, on 2026-11-28.
