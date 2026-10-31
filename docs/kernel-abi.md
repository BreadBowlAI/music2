# Kernel ABI v1

The contract between music2 and kernel code. Frozen in step 9 (track ab). The contract's rules are [ABI-1, ABI-2, ABI-3, ABI-5, ABI-8 and ABI-9](architecture.md#kernel-abi-reservations); this page is the detail behind them.

## In short

- A **kernel** is a small sound part in WebAssembly (wasm), such as a filter. A package ships one wasm module with all its kernels.
- The module runs in **sandboxes**. A sandbox is one copy of the module and its one memory. It can hold many **states**, one per use of a kernel. So every call names its state.
- Memory is taken only while a sandbox is **born**. Then it is **sealed**: its memory never grows again.
- The host writes each state's **settings block** before the state is made. The kernel reads its rate, its ports, its numbers and its audio files there.
- The only thing a kernel may import is **`m2.asset_read`**: it copies part of an audio file into the kernel's memory.
- An **offline kernel** is run by music2 off the audio thread, to make something from a file: `std:stretch` makes a warped clip's stretch. Song code can't call one.
- A kernel may take **notes**: an `events<note>` input gets its lane's notes and controllers each block, stamped to the sample.
- A module says its version in a small part of its file named **`m2_abi`**: **1**. music2 reads it first, before it looks at anything else. It refuses a module built for any other version, and says which.
- v1 only ever grows at the end: new optional exports, and new fields at the end of the settings block. Anything else is v2.

| Part | Where below |
|---|---|
| What the module exports | [The module](#the-module), [Each kernel](#each-kernel) |
| The settings block, byte by byte | [The settings block](#the-settings-block) |
| Where the sound goes in and out | [The port table](#the-port-table) |
| Reading an audio file | [The asset import](#the-asset-import) |
| Notes in, each block | [Event inputs](#event-inputs) |
| A kernel music2 runs to make a render | [Offline kernels](#offline-kernels) |
| What every kernel must keep | [The five rules](#the-five-rules-abi-5) |
| What is fixed now, built later | [Reserved shapes](#reserved-shapes) |

The Rust kit (`std/kernels-src/kernel`) writes all of this for you. Write a `struct`, implement `Kernel`, and call `export!`. In `Kernel::new`, `cfg.values` holds your numbers and `cfg.assets` your audio files; `asset.read(channel, frame, &mut buf)` reads one. A kernel that takes notes names those inputs in `Kernel::NOTES` and reads them with `io.events(i)`. `std:sampler` (`std/kernels-src/src/sampler.rs`) is a whole example.

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
| custom section `m2_abi` | exactly 4 bytes: the version as a little-endian u32, so 1 | read from the module's bytes before it is compiled, so before its imports, memory and exports. Any other version is refused, naming it: "built for kernel ABI v2, but this music2 speaks v1 only". No section: "built for kernel ABI v0, or isn't a music2 kernel" |
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
| 12 | `flags` | u32 | bit 0: an offline kernel ([below](#offline-kernels)). The other bits are 0 in v1 |
| 16 | `channels` | u32 | the channel count resolved for the kernel's `audio<*>` ports: 1 or 2 |
| 20 | `ports` | address | the channel count of each port, `n_ports` u32s, inputs first |
| 24 | `n_ports` | u32 | inputs + outputs |
| 28 | `values` | address | the kernel's numbers, `n_values` f64s, 8-byte aligned |
| 32 | `n_values` | u32 | |
| 36 | `assets` | address | the kernel's audio files, `n_assets` entries of 16 bytes (below) |
| 40 | `n_assets` | u32 | |
| 44 | `saved` | address | reserved: a saved state for `k_load`. 0 in v1 |
| 48 | `saved_len` | u32 | reserved. 0 |
| 52 | `time_map` | address | an offline kernel's time map: pairs of f64, 8-byte aligned. 0 for a live kernel |
| 56 | `time_map_len` | u32 | how many pairs. 0 for a live kernel |

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
- An `events<note>` input has a channel count of 1 in the settings block's list, and one entry in the table: the address of its header ([below](#event-inputs)).

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
- The samples are the file's, at its own rate, as 32-bit floats, decoded the same way wherever they come from. Resampling is the kernel's own work (by the pitch ratio it wants).

Where the host reads from is its choice, per sandbox. The kernel can't tell, and reads the same bits:

| Held as | Who | A read |
|---|---|---|
| **whole**: decoded once, shared by every sandbox that reads it | live kernels (on the audio thread), such as `std:sampler` | copies. Never allocates, never waits |
| **disk**: the file, read in pieces at each call | offline kernels (on their own thread) | reads the file, so it may wait. A file that can't be read traps |

**Streaming live, later, still v1.** A live kernel may one day read a file the host streams. Then a frame the host has not loaded yet is given as silence (0.0), and counted as a **miss**, as a lane's chunk is. The count returned is the same as if the frame were there, so the kernel goes on the same. This never happens in an export or an offline render: those wait for every frame. A kernel must not try to tell a miss from silence.

## Event inputs

A kernel's input of kind `events<note>` in its manifest (ABI-8). Built in track events; outputs of kind `events` are still [reserved](#reserved-shapes).

| Part | Rule |
|---|---|
| Wiring | connected straight to a notes lane at a module's top level, never inside a `poly` voice (a voice's kernels get their note from its note fields). It must be connected |
| Table | one entry: the address of the header |
| Header (16 bytes) | `count` u32 at 0: events this block; `cap` u32 at 4: 1024; `events` address at 8: where they start (just after the header); 4 bytes kept at 12 |
| Each event (24 bytes) | `kind` u32 at 0, `frame` u32 at 4, `id` u32 at 8, then three 4-byte values at 12, 16 and 20 (below) |
| `frame` | the frame in the block, 0 to `max_block` − 1: never counted from `from`. A call reads the events whose frame is in its `from..to` |
| Order | by frame; at one frame the controllers (MIDI) first, then Offs, then Ons, so a key can be struck again where it was let go |
| What comes | the lane's notes, live ones (a keyboard, the window's keys) included, and its controllers from live input: pitch bend, CC 1 (the mod wheel), CC 64 (the sustain pedal), CC 121. Late with the lane when the kernel is late |
| When | written by the host before each `k_process`, in the state's own block, placed at birth. Read-only: a kernel never writes it (ABI-1), and debug builds check the whole header and its 1024 events after each call |
| Past `cap` | every event that isn't an On is kept, and the latest Ons are left out. Never silently: an offline render fails, naming the sound; live the sound plays on and music2 says so once a plan (`notes.dropped`). A lane's buffer holds at most 812 a block unless its notes need more than 256 sounding at once or 128 starting in one block |

| `kind` | Means | Values at 12, 16, 20 |
|---|---|---|
| 0 | a note starts (On) | pitch (f32, semitones, 60 is C4), velocity (f32, 0 to 1), `from_pitch` (f32, NaN for none) |
| 1 | a note ends (Off) | release velocity (f32), 0, 0 |
| 2 | a note expression (reserved: not sent yet) | which (u32: 0 pitch, 1 pressure, 2 timbre), value (f32), 0 |
| 3 | a parameter (reserved: not sent yet) | value (f32), 0, 0 |
| 4 | a MIDI message | its 3 bytes in the low 3 bytes of the first (status at 12, data at 13 and 14, 0 at 15), 0, 0 |

`id` is the note's: an On and its Off share it, and no two notes sounding at once have the same. A MIDI message's `id` is 0.

The Rust kit reads all this for you (ABI-9): name the inputs in `Kernel::NOTES`, and `io.events(i)` gives this call's events, each as an `Event`, its frame counted from the call's first frame, so `frame` indexes the call's other inputs. A kind the kit doesn't know is skipped.

**Why it keeps v1:** this shape was fixed among v1's reservations, and a module without an events input sees nothing new. So it is an addition, as an optional export is.

## Offline kernels

A kernel music2 runs off the audio thread, to make something from a file, never in a song's graph. Built in track sx; its first is `std:stretch` (`std/kernels-src/src/stretch.rs`), which makes every warped clip's stretch. The host side is `core/runtime/src/offline.rs`; the kit has `Cfg::offline()` and `Cfg::time_map()`.

| Part | Rule |
|---|---|
| Manifest | `offline = true` on its `[[kernel]]`. No inputs, one `audio` output. `audio<*>` gets the source's channels |
| Song code | can't call it: the error `kernel.offline`, at the call. No song's graph has it |
| `flags` | bit 0 set |
| Source | its asset 0, read from the file on disk ([held as **disk**](#the-asset-import)), never whole |
| `values` | its own numbers. `std:stretch`: `values[0]` is the pitch change in semitones |
| Time map | `time_map_len` pairs of f64 at `time_map`: (source seconds, output seconds from the output's start), in order. Between two pairs, time runs in a straight line. Before the first and after the last, the nearest line goes on. One pair: speed 1 through it. None: the source as it is |
| A run | one sandbox, born, sealed and prewarmed as any. Then `k_process(state, io, 0, n)` again and again, `n` at most `max_block`: each call writes the next `n` frames of the output. The host stops when it has the frames it wants |
| Output | goes to its sink a block at a time. The host never holds the source or the output whole |
| When | live, on the session's stretch thread, one run at a time in the order asked, while what it has made already plays; a run no longer wanted is stopped between calls. Offline (an export, a range render), before the render plays |

The five rules hold for offline kernels too: memory only at birth, a reset that takes none, and the rest.

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

Fixed now, so later work can't collide with v1. Each is built with the first kernel that needs it, as an optional export or a new settings field, so v1 modules keep working. Offline kernels were reserved here too; they are built now ([above](#offline-kernels)), and so are event inputs ([above](#event-inputs)).

| Shape | Fixed now | Built when |
|---|---|---|
| Event outputs | as [event inputs](#event-inputs): one port-table entry, the address of `{count: u32, cap: u32, events: address}`, and the same 24-byte events. A kernel writes up to `cap` events to an output and sets its `count` | the first arpeggiator or kernel that makes notes |
| `k_save`, `k_load` | `k_save(state, dst, cap) -> i32`: writes the state's bytes at `dst`, returns how many (more than `cap`: nothing written, that many needed). `k_load(state, src, len) -> i32`: 1 loaded, 0 refused. Both run off the audio thread, as a plugin's state load does. A loaded state is the one `k_reset` returns to; the settings block's `saved`/`saved_len` give it at birth | the first kernel with state worth saving |
| `k_latency`, `k_tail` | `k_latency(state) -> i32` and `k_tail(state) -> i32`, in frames. Each depends only on the settings, so the host may ask once, after `k_new` | latency compensation; a kernel whose tail outlasts `k_active` |

Not reserved: `k_drop`. One state is never freed alone; a sandbox drops whole. Languages with one garbage-collected heap per instance are out of scope for v1.

## The version rule

- The `m2_abi` section says the version. This music2 speaks v1 only. A module with no section is taken as v0 and refused, and the message says to rebuild it with the v1 kit.
- Every later ABI keeps the `m2_abi` section, as it is. So this music2 can always refuse a later module by its version, whatever else changed.
- Adding an optional export, or a field at the end of the settings block, keeps v1.
- Changing any shape above, or a rule, makes v2.
