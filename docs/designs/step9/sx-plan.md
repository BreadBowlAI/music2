# Track sx: stretching as an offline kernel

Branch `sx`, from `main` at `97b4cdf`, 2026-10-18. Step 9, audio and sample turn 3 ([architecture.md, Order of work](../../architecture.md#order-of-work), item 9).

## In short

- **First, kernel ABI v1 is settled.** music2 reads a module's ABI version from its bytes, before anything else. So a module built for a later ABI is always refused by its version (gaps: "A kernel module built for a later ABI").
- **A kernel can read its audio file from the disk.** Offline kernels do so now. Live kernels (the Sampler) still hold their files in memory, but the ABI already allows streaming them later.
- **Then stretching moves into std.** A new std kernel, `std:stretch`, stretches a clip to the song's tempo and keeps its pitch. It is the first **offline kernel**: it runs off the audio thread, in pieces, never on the whole file.
- **Then "Follow tempo".** An audio clip's menu gets **Follow tempo**. Turn it on, and the clip keeps in time when you change the tempo. The new stretch is made in the background and plays as it is made.
- One example changes sound: `examples/resample` has a warped clip, so its 2 renders change once (why: [D12](#decisions)). The other 10 keep their bits.

| Slice | You get | Size |
|---|---|---|
| 1. ABI v1 settled | the version is read first; kernels may read files from the disk | small |
| 2. `std:stretch` | warped clips stretched by a std kernel, in pieces, never whole in memory | medium |
| 3. Follow tempo | the menu item; a tempo change re-stretches in the background and streams | medium |

## What is there today

| Part | Where | Today |
|---|---|---|
| ABI version | `verify_module`, `core/runtime/src/packages.rs` | asked by calling `m2_abi()`, so only after the module is compiled and made. Imports and memory are checked first |
| `m2.asset_read` | `core/runtime/src/sandbox.rs` | copies from a file decoded whole (`Cache::kernel_asset`, `audio.rs`) |
| Warp | `Cache::warp`, `render_clip`, `stretch` in `core/runtime/src/audio.rs` | WSOLA in the runtime. Source and render held whole in memory, once, during the build. Then written to `<cache>/renders/<hash>.f32` and streamed (RENDER-30) |
| Cache name | `stream::render_path` | `WARP_VERSION` (1), bumped by hand |
| Offline kernels | `docs/kernel-abi.md`, Reserved shapes | shape fixed: `flags` bit 0, `time_map` pairs, chunks. Not built |
| The app | `web/src/components/ClipMenu.tsx`, `crates/editor/src/clips.rs` (`Gesture::Audio`) | volume, fades, Reverse. Warp only by hand in JSON |

## Slice 1: kernel ABI v1 settled

**Goal.** A module's version is read from its bytes before it is compiled. So imports, memory and exports are only checked for a v1 module. `m2.asset_read` can be served from the disk (track io's reader), for offline kernels now and live kernels later.

| Crate | Files | Change |
|---|---|---|
| `m2-runtime` | `core/runtime/src/packages.rs` | `abi_version(bytes)`: a small hand parser walks the module's sections and reads the custom section `m2_abi` (4 bytes, a u32). Called before `Module::new`. `verify_module` no longer needs `m2_abi()` |
| `m2-runtime` | `core/runtime/src/sandbox.rs`, `stream.rs` | the host's asset list holds `Whole(Arc<Asset>)` or `Disk(Arc<Wav>)`. `Disk` reads the frames asked for with `read_at`, through `stream.rs`'s WAV reader, into a buffer the sandbox keeps. Only for sandboxes off the audio thread |
| std kit | `std/kernels-src/kernel/src/lib.rs` | `abi!` (once per module) writes the `m2_abi` section (`#[link_section]`, wasm32 only), and no longer exports `m2_abi()`. Not `export!`: it runs once per kernel, and the linker would join the copies into one longer section |
| std | `std/kernels/std.wasm` | rebuilt. Every std kernel's hash changes; no sound changes |
| docs | `docs/kernel-abi.md`, `architecture.md` (ABI-2, ABI-3), `gaps.md` | the version rule and the asset rule below ([D1](#decisions) to [D4](#decisions)) |

**Tests, each shown red first.**

| Test | Where | Red on |
|---|---|---|
| `a_later_abi_module_is_refused_by_its_version_first`: a module whose section says 2, with an import `m2.other` and a 64-bit memory, is refused naming v2 | `core/runtime/src/tests.rs`, next to the ABI tests it joins | today's code (refused as it compiles) |
| `a_module_with_no_abi_section_is_refused_as_v0` (replaces `a_v0_module_is_refused_naming_its_version`); one of its modules exports an `m2_abi()` that returns 1 | `core/runtime/src/tests.rs` | today's code (it calls `m2_abi()`) |
| `std_wasm_says_v1_in_its_section` | `core/runtime/src/tests.rs` | the old std.wasm |
| `an_asset_read_from_disk_is_the_decoded_asset`: a kernel reads a 16-bit and a float WAV, mono, stereo and 3 channels, in odd pieces (one longer than the host's piece), past the end; same counts and bits as `Whole`, same settings entry. Nothing is decoded whole by its type: `Disk` holds a `Wav`, no samples (no `Cache` takes part until slice 2) | `core/runtime/src/tests.rs` | `Disk` stubbed to silence |

**Done when:** the tests are green. `std/build.sh --check` gives the same bytes. Renders: 12 of 12 match. std's kernel tests pass in release. gaps.md loses "A kernel module built for a later ABI".

## Slice 2: `std:stretch`, the first offline kernel

**Goal.** The offline-kernel shape is built. std 0.2.2 has `stretch`. The runtime runs it to make every warped clip's render, in pieces, straight into the cache file. The source is read from the disk (slice 1). The runtime's own WSOLA is deleted. In this slice the render still happens during the build, as today.

| Crate | Files | Change |
|---|---|---|
| std | `std/kernels-src/src/stretch.rs` (new), `lib.rs`, `std/m2pkg.toml` | the kernel: WSOLA ported from `audio.rs` ([D6](#decisions)), streaming: it reads the source through `asset_read` and writes `max_block` frames per call. Pitch as today: stretch, then cubic Hermite. `[[kernel]] name = "stretch"`, `offline = true`, `outputs = ["out: audio<*>"]` |
| std kit | `kernel/src/lib.rs` | `Cfg::offline()` and `Cfg::time_map()` |
| `m2-runtime` | `core/runtime/src/offline.rs` (new) | run one offline kernel: a sandbox with `flags` bit 0 and the time map, born, sealed, prewarmed as any sandbox; then `k_process` block by block into a sink |
| `m2-runtime` | `audio.rs`, `stream.rs` | `Cache::warp` builds the time map from the warp markers and the tempo map ([D5](#decisions)) and writes the render to the part file block by block (gain, fades and reverse after, as today). `stretch` and `WARP_VERSION` go; the cache name holds the std module's hash ([D8](#decisions)) |
| `m2-runtime` | `packages.rs`, the graph build | the manifest's `offline`. Song code that calls an offline kernel gets the error `kernel.offline` |
| fixtures | `e2e/renders.sha256` | `examples-resample.wav` and `examples-resample-from4-for6.wav` change once ([D12](#decisions)) |

**Tests, each shown red first.**

| Test | Where | Red on |
|---|---|---|
| `a_stretch_keeps_its_pitch`: a 440 Hz sine at 0.5, 0.8, 1.25 and 2 times its length is 440 Hz within 0.5% | `stretch.rs` | a stub kernel that writes silence |
| `a_stretch_is_as_long_as_its_map_says`, `the_identity_map_gives_the_source_back` (over 40 dB clean, away from the ends), `a_stretch_has_no_clicks` (a sine's step between frames stays under a bound) | `stretch.rs` | the stub |
| `pitch_with_a_map_transposes_and_keeps_time` | `stretch.rs` | the stub |
| `a_warped_clip_is_stretched_by_std_stretch`: the runtime calls the kernel; a std without it plays the clip silent, not the old WSOLA | `core/runtime/src/tests.rs` | today's code |
| `a_warped_render_holds_neither_file_whole`: nothing decoded whole, the sink takes blocks | `core/runtime/src/tests.rs` | today's code |
| `song_code_cannot_call_an_offline_kernel` | `core/runtime/src/tests.rs` | before the check |
| `the_resample_example_gives_the_same_bits_on_1_to_7_threads`, and an export with an empty cache folder equals one with a full one | `core/runtime/src/tests.rs` | the stub |

The kept tests stay green: `pitch_without_warp_is_varispeed_and_with_warp_keeps_timing`, `a_streamed_lane_is_the_rendered_lane` (its reference for a warped clip becomes the kernel's whole run).

**Done when:** all green; renders 10 of 12 unchanged, the 2 resample lines updated with the why in the commit. A by-ear step on the resample example's warped clip. Linux render speed of the stretch written down (information only, PLAT-1).

## Slice 3: Follow tempo, stretched in the background

**Goal.** A live session never waits for a stretch. A changed tempo or clip asks for a new render. A background thread makes it. The lane plays what is made, as it is made. An export or a range render makes its renders first, so its bits never depend on timing. The clip menu gets **Follow tempo**.

| Crate | Files | Change |
|---|---|---|
| `m2-runtime` | `core/runtime/src/renders.rs` (new), `audio.rs` | a live `Cache` hands each new render to its one stretch thread (in the order asked). An offline `Cache` renders at once, as in slice 2. A render the next prune drops stops, and its part file is removed |
| `m2-runtime` | `stream.rs` | a `Render` source knows how many frames are done (an atomic). A chunk is filled only when all its frames are done; till then it stays empty, and the sound thread plays silence and counts a miss (RENDER-30 as today). When done, the part file is renamed to its cache name |
| `m2-play` | `core/play/src/disk.rs` | fill again a chunk left empty for a render; a render's progress wakes the disk thread. Activation's 500 ms wait and seek's 30 ms wait stay as they are |
| `m2-server` | `crates/server/src/graph/actor.rs` | the session's cache is the live kind |
| `m2-editor` | `crates/editor/src/clips.rs` | `Gesture::Audio { follow: Option<bool> }` ([D11](#decisions)). Through `POST /api/graph/clips`, as the other clip settings. One undo step |
| web | `web/src/components/ClipMenu.tsx`, `web/src/lib/` (the clip's call) | a check row **Follow tempo**, its hint, on audio clips only |
| docs | `architecture.md` (RENDER-30, ABI-2, Deferred, Order of work), `kernel-abi.md`, `gaps.md`, `HANDOFF.md`, `build-notes-sx.md` | |

**Tests, each shown red first.**

| Test | Where | Red on |
|---|---|---|
| `a_tempo_change_never_waits_for_a_stretch`: with a stretch held back by a latch, the build returns, and the lane plays silence there, counted as misses, never wrong samples | `core/runtime/src/tests/stream.rs` | slice 2 (it renders in the build) |
| `a_lane_plays_its_render_as_it_is_made`: chunks fill as frames are done; the end result equals the offline render bit for bit | `core/runtime/src/tests/stream.rs` | chunk fill not waiting for `done` |
| `an_export_never_plays_a_render_half_made`: export right after a tempo change equals a cold export | `core/runtime/src/tests.rs` | the live cache used offline |
| `a_dropped_render_stops_and_leaves_no_part_file` | `core/runtime/src/tests.rs` | no cancel |
| `follow_tempo_keeps_the_clip_where_it_plays_and_undo_takes_it_back`: on, the clip sounds the same at the current tempo; then at a new tempo it keeps its beats | `crates/editor/src/clips_tests.rs` | the gesture field ignored |
| `follow_tempo_through_the_clips_route` | `crates/server/src/routes_8b_tests.rs` | before the field |
| browser suite `sx-follow`: the row, its check, a tempo change, Undo | `e2e/sx-follow.mjs` | before the row |

**Done when:** all green. `ci/check.sh --gates` passes, run with `~/m2-runs/tw`. Pictures from the app on `:98` (the menu, a followed clip after a tempo change). By-ear steps written. Every new test was seen red once.

## Decisions

| # | Question | Pick |
|---|---|---|
| D1 | Where is the version? | A custom section named `m2_abi`, exactly 4 bytes: the version as a little-endian u32. music2 reads it from the bytes before compiling. Every later ABI must keep this section, so this music2 can always refuse it by name |
| D2 | Keep `m2_abi()`? | No. One place for the version. v1 hasn't shipped, so this is still v1. A module with no section is told: "built for kernel ABI v0, or not a music2 kernel. Rebuild it with the v1 kit" |
| D3 | Where does `asset_read` read from? | The host's choice, per sandbox: a file decoded whole, or the disk. Offline kernels read from the disk now (they run on their own thread, so a read may wait). Live kernels keep whole files ([gaps](#what-goes-to-gapsmd)) |
| D4 | Can live kernels stream later without v2? | Yes, written now: live, a frame the host has not loaded yet is given as silence and counted as a miss, like a lane's chunk. The count `asset_read` returns is the same as if it were there, so the kernel goes on the same. Never in an export or offline render |
| D5 | The time map | Pairs of f64 (source seconds, output seconds from the clip's start); `time_map_len` counts pairs. Before the first pair and after the last, the nearest segment goes on; one pair means speed 1. The host builds it from the warp markers and the tempo map: one pair per marker and per tempo change inside the clip. `values[0]` is the pitch in semitones. The source is asset 0. Output only, `max_block` 128 frames per call |
| D6 | Which method? | WSOLA, ported from today's runtime: channels share one alignment, so stereo stays put. Changed: the match is normalized by loudness. WSOLA keeps drum hits sharp; a phase vocoder smears them. Loops are what people stretch most |
| D7 | Kernel or host? | The kernel does time, pitch and the change of sample rate. The host does gain, fades and reverse, as now |
| D8 | The cache name | Holds the stretch module's hash (std.wasm's), not a hand-bumped number. A new std makes new renders |
| D9 | Which kernel stretches? | Always `std:stretch`, from the song's std. A choice per clip goes to gaps |
| D10 | Background order | One stretch thread per open song. Renders run in the order asked, each from the clip's start. Offline renders (export, range render) make their renders first, on their own thread |
| D11 | What does Follow tempo set? | On: warp markers at the clip's start, its end and each tempo change inside it, from the tempo now. So nothing changes when you turn it on; later tempo changes keep its beats. Off: no markers; it plays at its own speed again. Words: **Follow tempo**, hint "Stretch this clip with the song's tempo. Its pitch stays the same." Undo: "Undo Follow tempo" |
| D12 | Which renders change, and why? (FMT-3) | std goes to 0.2.2 (as ab went to 0.2.1). Only `examples/resample` changes: it has one warped clip. Its stretch now runs in std, with the map given as pairs and the normalized match, so its samples differ slightly. No one has songs yet, so one bundled std stays enough (Deferred table) |
| D13 | New crates? | None. The section parser is hand-written (about 30 lines). The runtime's tests take `wat` as a dev-dependency, the version wasmtime's `wat` feature already builds: test packages are now wasm bytes, since the version is read from bytes |

## Risks

| Risk | What we do |
|---|---|
| `strip = true` in std's release build may drop the custom section | `std_wasm_says_v1_in_its_section` catches it. Then use `strip = "debuginfo"` and check the bytes rebuild the same |
| The stretch is slow in wasm | Today's WSOLA does about 60 multiply-adds per output frame; expect well over 50 times faster than real time. Measure in slice 2 (Linux, information only) |
| A short lane is filled whole when it is built; a render not done then leaves holes | Slice 3's disk thread fills empty chunks again; `a_lane_plays_its_render_as_it_is_made` covers a short lane |
| Bits differ between threads or machines | The kernel uses `libm` only and finite maths. The 1 to 7 threads test and the cold/warm export test |
| Renaming an open part file on Windows | Design note only: open it with delete sharing in `m2-os` when Windows comes (PLAT-2). Not built |
| A long clip, tempo changed while playing near its end | Silent there until the render gets there. Logged to gaps (render from the playhead later) |

## What goes to gaps.md

One line each, not fixed in this track:

- A Sampler's files are still held whole in memory. The ABI allows streaming them (D4); the host needs to load each file's start ahead and read ahead per voice.
- A stretch renders from the clip's start. After a tempo change deep inside a long clip, that clip is silent until the render reaches the playhead.
- Warp markers can't be placed by hand in the app. Follow tempo is the only way.
- A clip's own tempo isn't found from its file. Follow tempo assumes it fits the song when you turn it on.
- One stretch method for all clips. No choice per clip (drums, voice, pads).
- No `m2` command for Follow tempo. Agents use `POST /api/graph/clips`.
- A stretch that fails plays silence, with a line in the log only.

Updated, not new: "A warped audio clip is worked out whole in memory" goes (slice 2). "Kernel ABI v1's reserved parts aren't built" loses offline kernels. #18 says Follow tempo works.
