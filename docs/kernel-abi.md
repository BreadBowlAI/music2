# Kernel ABI v1

The contract between music2 and kernel code. Frozen in step 9 (track ab). The contract's rules are [ABI-1, ABI-2, ABI-3 and ABI-5](architecture.md#kernel-abi-reservations); this page is the detail behind them.

## In short

- A **kernel** is a small sound part in WebAssembly (wasm), such as a filter. A package ships one wasm module with all its kernels.
- The module runs in **sandboxes**. A sandbox is one copy of the module and its one memory. It can hold many **states**, one per use of a kernel. So every call names its state.
- Memory is taken only while a sandbox is **born**. Then it is **sealed**: its memory never grows again.
- The host writes each state's **settings block** before the state is made. The kernel reads its rate, its ports, its numbers and its audio files there.
- The only thing a kernel may import is **`m2.asset_read`**: it copies part of an audio file into the kernel's memory.
- `m2_abi()` returns **1**. music2 refuses a module built for any other version, and says which.
- v1 only ever grows at the end: new optional exports, and new fields at the end of the settings block. Anything else is v2.

| Part | Where below |
|---|---|
| What the module exports | [The module](#the-module), [Each kernel](#each-kernel) |
| The settings block, byte by byte | [The settings block](#the-settings-block) |
| Where the sound goes in and out | [The port table](#the-port-table) |
| Reading an audio file | [The asset import](#the-asset-import) |
| What every kernel must keep | [The five rules](#the-five-rules-abi-5) |
| What is fixed now, built later | [Reserved shapes](#reserved-shapes) |

The Rust kit (`std/kernels-src/kernel`) writes all of this for you. Write a `struct`, implement `Kernel`, and call `export!`. In `Kernel::new`, `cfg.values` holds your numbers and `cfg.assets` your audio files; `asset.read(channel, frame, &mut buf)` reads one. `std:sampler` (`std/kernels-src/src/sampler.rs`) is a whole example.

## Words used here

| Word | Meaning |
|---|---|
| host | music2's engine, which loads the module and calls it |
| sandbox | one wasmtime `Store`: one instance of the module and its memory. One per box: a kernel outside an instrument, or one package's kernels in one instrument's voices (RENDER-32) |
| state | the data of one use of a kernel: one per graph node, one per voice in a `poly` |
| birth | the time before the seal, off the audio thread, when states are made and memory may grow |
| frame | one sample time; a block holds up to `max_block` frames (128) |
| address | a byte offset into the module's memory, as an `i32` |

All numbers are little-endian. An address is 4 bytes. Wasm types: `i32` (also used for `u32`), `f32`, `f64`.

## The module

| Item | Shape | Rule |
|---|---|---|
| `memory` | exported, defined by the module (not imported), 32-bit, 64 KiB pages, not shared | checked at load. The hardware checks need it |
| `m2_abi() -> i32` | returns 1 | checked first, before any other export. Any other value is refused: "built for kernel ABI v0, but this music2 speaks v1 only" |
| `m2_alloc(size: i32, align: i32) -> i32` | an address of `size` bytes aligned to `align`, or 0 for "no room" | works only during birth. The host takes room for each state's block here |
| imports | none, or `m2.asset_read` only | anything else is refused, naming it |

## Each kernel

For a kernel named `k` in the package's `m2pkg.toml`:

| Export | Signature | Meaning |
|---|---|---|
| `k_ports` | `() -> i32` | `inputs << 8 \| outputs`. Checked against the manifest |
| `k_new` | `(cfg: i32) -> i32` | build one state from the settings block at `cfg`. Runs only during birth, so it may take memory. Returns the state's address; 0 means it failed |
| `k_reset` | `(state: i32)` | put the state back as `k_new` left it. Takes no memory. Keeps the settings and assets |
| `k_process` | `(state: i32, io: i32, from: i32, to: i32)` | read inputs and write outputs for frames `from..to` of the block, through the port table at `io` |
| `k_active` | `(state: i32) -> i32` | 1 while sound already taken in can still come out (a delay line, a release); else 0 |

The host calls `k_reset` once on every state right after the seal (the prewarm). A reset that tries to take memory fails there, at the build, and names its node. It never fails later on the audio thread.

## The settings block

The host writes it into the state's block before `k_new`, and keeps it unchanged for as long as the sandbox lives. A kernel may keep its address.

| Byte | Field | Type | Meaning |
|---|---|---|---|
| 0 | `len` | u32 | bytes in the block: 60 in v1. Read a field only if `len` covers it |
| 4 | `sample_rate` | f32 | the song's rate, in Hz |
| 8 | `max_block` | u32 | the most frames one `k_process` call is given (128) |
| 12 | `flags` | u32 | 0 in v1. Bit 0 is reserved: "offline" |
| 16 | `channels` | u32 | the channel count resolved for the kernel's `audio<*>` ports: 1 or 2 |
| 20 | `ports` | address | the channel count of each port, `n_ports` u32s, inputs first |
| 24 | `n_ports` | u32 | inputs + outputs |
| 28 | `values` | address | the kernel's numbers, `n_values` f64s, 8-byte aligned |
| 32 | `n_values` | u32 | |
| 36 | `assets` | address | the kernel's audio files, `n_assets` entries of 16 bytes (below) |
| 40 | `n_assets` | u32 | |
| 44 | `saved` | address | reserved: a saved state for `k_load`. 0 in v1 |
| 48 | `saved_len` | u32 | reserved. 0 |
| 52 | `time_map` | address | reserved: an offline kernel's time map. 0 |
| 56 | `time_map_len` | u32 | reserved. 0 |

- Every list is an address and a count, never written inline. So a field never moves when another is added.
- An empty list has address 0 and count 0.
- New fields are only ever added at the end, and `len` grows to cover them.

**An asset entry** (16 bytes):

| Byte | Field | Type | Meaning |
|---|---|---|---|
| 0 | `handle` | u32 | what `m2.asset_read` takes |
| 4 | `frames` | u32 | its length, in frames at its own rate |
| 8 | `channels` | u32 | 1 or 2; 0 for a file that is missing |
| 12 | `sample_rate` | f32 | its own rate, in Hz; 0 for a missing file |

The song names a kernel's assets and values in its code: `kernel(id, inputs, { assets: ["kick.wav"], values: [60] })` (`@m2/core` 1.3.0). A missing file is told as `asset.missing`, naming the file and the track. Its entry has 0 frames and 0 channels, and it reads silence. Both lists are part of the kernel's shape (RENDER-25): a new file, or a changed file under the same name, makes a new state.

## The port table

`io` holds one address per port *channel*: the inputs first, then the outputs, each port's channels in order, each port with the count `ports` gives it.

- Each address is of a buffer of `max_block` f32s, indexed by the frame in the block. Frame `i` is at `address + 4 × i`.
- The host places every buffer. No two overlap.
- Read the addresses from `io` at each call. Never keep them.
- A mono input may be read as stereo by reading channel 0 twice; the Rust kit does this for you, and gives a mono output's channel 1 a scratch buffer of its own.

## The asset import

```
(import "m2" "asset_read" (func (param i32 i32 i32 i32 i32) (result i32)))
m2.asset_read(handle, channel, frame, dst, frames) -> frames read
```

- It writes `frames` f32s at `dst`: channel `channel` of the asset `handle` names, from frame `frame` on, at the asset's own rate.
- A mono asset's channel 1 is its channel 0.
- Past the asset's end, and for a handle or channel it doesn't have, it writes silence (0.0).
- It returns how many frames came from the asset: from 0 to `frames`.
- `dst` to `dst + 4 × frames` must lie inside the memory, or the call traps.
- The host decodes each file once, at its own rate, as 32-bit floats, and shares it with every sandbox that reads it. Nothing is copied into a sandbox ahead of time.
- It never allocates and never waits, so a kernel may call it in `k_process` on the audio thread. Resampling is the kernel's own work (by the pitch ratio it wants).

## The five rules (ABI-5)

| # | Rule | Why |
|---|---|---|
| 1 | Take memory only in `k_new` (and the host's `m2_alloc`). Once the sandbox is sealed, its memory can't grow: a try traps | the audio thread never waits for memory (RENDER-28) |
| 2 | `k_reset` takes no memory and leaves the state as `k_new` (and later `k_load`) left it | voices are reset on the audio thread at every reuse and Panic |
| 3 | Never write an input (ABI-1). Read the port addresses from `io` at each call, and never keep them | the host writes a steady input once and leaves it there |
| 4 | Write every frame of `from..to` on every output channel, and touch only the channels a port has | the host doesn't clear outputs |
| 5 | Keep no per-state data in statics: every state in the sandbox shares them. Let no static point at memory a state took. All one `k_new` takes lies in one run of bytes | many states share one sandbox, and a later layout may move a state's bytes |

The host checks rules 1 and 2 by construction (the seal and the prewarm). Debug builds check rule 3 at every call.

## Reserved shapes

Fixed now, so later work can't collide with v1. None is built yet. Each is built with the first kernel that needs it, as an optional export or a new settings field, so v1 modules keep working.

| Shape | Fixed now | Built when |
|---|---|---|
| Event ports | a port of kind `events` has one port-table entry: the address of `{count: u32, cap: u32, events: address}`. Each event is 24 bytes: `kind` u32 (0 on, 1 off, 2 note expression, 3 param, 4 MIDI), `frame` u32, `id` u32, then three f32: on: pitch, velocity, `from_pitch` (NaN for none); off: release velocity; note expression: kind (0 pitch, 1 pressure, 2 timbre, as a u32), value; param: value; MIDI: the 3 bytes in the low bytes of the first. The host sets an input's `count`; a kernel writes up to `cap` events to an output and sets its `count`. RENDER-14's `Event`, in bytes | the first arpeggiator or event kernel |
| `k_save`, `k_load` | `k_save(state, dst, cap) -> i32`: writes the state's bytes at `dst`, returns how many (more than `cap`: nothing written, that many needed). `k_load(state, src, len) -> i32`: 1 loaded, 0 refused. Both run off the audio thread, as a plugin's state load does. A loaded state is the one `k_reset` returns to; the settings block's `saved`/`saved_len` give it at birth | the first kernel with state worth saving |
| `k_latency`, `k_tail` | `k_latency(state) -> i32` and `k_tail(state) -> i32`, in frames. Each depends only on the settings, so the host may ask once, after `k_new` | latency compensation; a kernel whose tail outlasts `k_active` |
| Offline kernels | `flags` bit 0 says "offline". The time map is f64 pairs (source time, output time) at `time_map`. Input and output come in chunks through the same ports, never whole files: a 32-bit memory holds only about 93 minutes of stereo floats at 48 kHz | clip stretching as a package kernel |

Not reserved: `k_drop`. One state is never freed alone; a sandbox drops whole. Languages with one garbage-collected heap per instance are out of scope for v1.

## The version rule

- `m2_abi()` says the version. This music2 speaks v1 only. A v0 module is refused, and the message says to rebuild it with the v1 kit.
- Adding an optional export, or a field at the end of the settings block, keeps v1.
- Changing any shape above, or a rule, makes v2.
