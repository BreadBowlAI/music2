# Track ab: kernel ABI v1 and the sampler

Step 9, the audio-and-sample turn 2 (architecture.md, Order of work, item 9). Branch `ab`, from `main` at `258275b`.

## In short

- Three slices, in this order. Each ends with a commit, and its tests are shown red before they pass.

  | # | Slice | What you get |
  |---|---|---|
  | 1 | The layout test (spike), then the decision | numbers for idea 3 against `main`, and a yes or no, written into the contract |
  | 2 | Kernel ABI v1, frozen | the contract with kernel code is final: version 1, written down, std built on it; kernels can read the song's audio files |
  | 3 | The sampler | a std instrument that plays a WAV, or one WAV per range of notes; drop a WAV on it in the app |

- **ABI v1 has the same shape either way.** Every call names its state, and memory is taken only at birth. So idea 3 can fit later even if the spike says no. Only the host's layout depends on the decision.
- **The rule for the decision:** go if the bits are the same and idea 3 is not slower than `main` by the mean. Otherwise keep today's layout (one state per sandbox, 32 MiB).
- **No existing render changes.** All 10 recorded renders keep their bits. The sampler adds a new example with 2 new lines.
- Linux timings are information only. The owner's Mac judges them later (PLAT-1).

## Words used here

| Word | Meaning |
|---|---|
| kernel | a small sound part written in WebAssembly (wasm), such as a filter |
| state | one kernel's own data, such as a filter's numbers or a delay line |
| sandbox | one wasmtime `Store` with one copy of a package's code and one memory: the only bytes that code can touch |
| box | what a live edit keeps or replaces whole: a kernel outside an instrument, or one instrument's voices |
| idea 3 | one sandbox per box (`docs/designs/kernel-layout/report.md`). Today it is one per state: 6,058 in p50; idea 3 needs 200 |
| ABI | the fixed contract between the host and kernel code |
| birth, seal | a sandbox is *born* off the audio thread, where its kernels may take memory; then it is *sealed* and its memory never grows |
| spike | a short test build that measures something before it is built for real |

## Slice 1: the layout spike and the decision

**Goal.** Build ABI v1's kit and std, run them in today's layout, then in idea 3's. Measure idea 3 against `main`. Decide, and write the decision and the numbers into the contract.

**Steps and files.**

| Step | What | Files |
|---|---|---|
| 1a | Kit v1, the frozen shape of report §2.3: `m2_abi()` returns 1; `m2_alloc(size, align)`; per kernel `k_new(cfg) -> state`, `k_reset(state)`, `k_process(state, io, from, to)`, `k_active(state)`, `k_ports()`. `trait Kernel`: `new(&Cfg)`, `reset` (by default `new` again from the kept settings), `process`, `active`. `static mut HOST` goes. On wasm32 a forward-only allocator works only inside `k_new` and `m2_alloc`; anywhere else it refuses, which traps | `std/kernels-src/kernel/src/lib.rs`, `std/kernels-src/src/testkit.rs` |
| 1b | std on the kit. Kernels that hold a `Vec` write their own `reset`, which zeroes in place. A mono port's channel 1 maps to channel 0 for reads and to a scratch buffer for writes, in the kit, so no kernel's arithmetic changes. `std.wasm` rebuilt by `std/build.sh` | `std/kernels-src/src/*.rs`, `std/kernels/std.wasm` |
| 1c | Commit A: the runtime speaks v1, still one state per sandbox, 32 MiB (P2). One new module holds the sandbox: Store, memory, kept base address, birth, the v1 calls. The host places port buffers and the settings block with `m2_alloc`, so ABI-1's overlap check goes; the debug input check stays | `core/runtime/src/sandbox.rs` (new), `core/runtime/src/lib.rs` (`KernelInst` becomes a state in a sandbox), `core/runtime/src/packages.rs` |
| 1d | Commit B: one sandbox per box. A kernel outside an instrument has its own sandbox. A `poly`'s voices share one sandbox per package: `VoicePool` holds it, and its voices' kernel nodes hold state handles. wasmtime's default 4 GiB reservation, `memory_may_move(false)`, the Spectre guard on. Seal by the limiter after birth; prewarm (one reset of each state) after the seal; every page touched once | `core/runtime/src/sandbox.rs`, `lib.rs` (`VoicePool`, `KernelNode`), `packages.rs` (`Layout` goes; G7f's P1 side then plays the same layout, printed, not judged) |

**How it is measured** (Linux; runs over 2 minutes go through `~/m2-runs/tw`):
- `main`'s copy: `git archive 258275b` into `/home/jerry/m2-runs/sab/main`, built in release there. Its `target/` is deleted after the spike.
- S's runner from `crates/gates/s/build.sh`, as G7f needs.
- Three pairs, in the order main, B, main, B, main, B. Each run is `m2-gates store --reference 50` (a block) and `m2-gates arena --reference 50 --at 180` (a 512-frame callback), held with `taskset -c 4,20` (the 96 MB-cache die), as `~/m2-runs/g7f-linux/one.sh` does. Each run measures against S fresh, so a pair's difference is B's figure minus main's.
- From the same runs: p50's footprint, its build time, the address space two renderers hold, the slowest activation block.
- Commit A against `main`: one pair, for information only (what the new call shape costs alone, report §10 #6).
- The same bits: the 10 renders against `e2e/renders.sha256`; `core/play/tests/bit_identity.rs` (live equals export on every pure fixture and p16, and 1, 2, 3, 4 and 7 threads); a 4-minute p16 render, byte for byte against `main`'s.

**The rule.** Go when every bit check passes, and the mean of the 3 pairs is 0 µs or less for a block and for a callback, and RENDER-32's bounds against S still hold (footprint +64 MB, build at most half again, two renderers within 25% of the address space, an activation block under 5.3 ms). If a mean lies between −20 and +20 µs, run 3 more pairs and judge all 6. Otherwise no-go.

**Tests (red first).**

| Test | Where | Red on |
|---|---|---|
| `two_states_in_one_instance_play_independently` | `std/kernels-src/src/tests.rs`, native and in wasm | the v0 kit (one state per kernel type in an instance) |
| `every_std_kernel_resets_in_a_sealed_sandbox` | `core/runtime/src/tests.rs` | 1b with one `Vec` kernel's own reset stubbed out (its reset asks for memory and traps) |
| `p50_builds_200_sandboxes` | `core/runtime/src/tests.rs` (the count p50 gives) | commit A (6,058) |
| `a_sealed_sandbox_never_grows` | `core/runtime/src/tests.rs`, a small test kernel in wat that grows its memory in `k_process` | the seal stubbed out |

The existing tests that guard live edits must pass unchanged: `keep.rs`, `no_alloc`, `churn`, G7b, G7c.

**Changed while building** (2026-10-18):
- `two_states_in_one_instance_play_independently` runs natively in `std/kernels-src/src/tests.rs` and in wasm in `core/runtime/src/tests.rs`: the kernels' crate has no wasm runner. Red with the kit's `k_new` stubbed to give one state per kernel type.
- `p50_builds_200_sandboxes` lives in `core/session/src/gate_tests.rs`: p50 is written by `m2-session`, which `m2-runtime` can't use.
- The settings block also holds `channels`, the count resolved for `audio<*>` ports: the kernels read it (`io.channels`), and the per-port list alone can't say it.
- The hand-written test kernels in wat (`keep.rs`, `cell.rs`, `tests.rs`) are rewritten for v1; their checks are unchanged. `a_kernel_with_overlapping_ports_is_refused` goes with the overlap check. `a_kernel_memory_never_moves` becomes `a_sealed_sandbox_never_grows`. `prewarming_keeps_kernel_memory_flat` now says a first reset grows nothing.
- In commit B a sandbox's birth cap stays 32 MiB (`KERNEL_MEMORY`, and its message); slice 2 makes it 256 MiB.
- `p50_builds_200_sandboxes` became `p50_builds_202_sandboxes`: since the report, std's master has two more kernels outside instruments (`stereo` and a gain), so p50 has 152 of them and 50 pools.
- The Deferred row "Spectre guard" is marked done with the decision (it was slice 2's): the guard is on since commit B.
- Commit A touches every page at its seal too (the sandbox code is shared). So A alone would not ship: its footprint was 839 MB against `main`'s 204. On a no-go, A's page touch would have had to go with B.

**The decision (2026-10-18): go.** The numbers are in RENDER-32. B against `main`, mean of 3 pairs: −211 µs a block, −1,080 µs a callback; footprint 75 MB against 204; build 62 ms against 222; the same bits everywhere. Commit B stays.

**Done when:**
- the numbers and the decision are in the contract: RENDER-32 rewritten (go, report §8's text) or its *Today* line extended with the numbers and the reason (no-go);
- on go, commit B stays; on no-go, a revert commit takes it out (merges only, no rebase), and commit A is what ships;
- `main`'s copy and its `target/` are deleted.

## Slice 2: kernel ABI v1, frozen

**Goal.** v1 is final, versioned and written down. std runs on it. v0 is refused. A kernel can read the song's audio files. On go, idea 3 is finished. Existing renders keep their bits.

**Files.**

| Part | Files |
|---|---|
| The spec for kernel authors: In short, the exports, the settings block with byte offsets, the port table, the one import, the five rules (ABI-5), the reserved shapes, the version rule | `docs/kernel-abi.md` (new); the kit's crate docs point to it |
| The check: `KERNEL_ABI = 1`; exports and signatures; a defined 32-bit memory with 64 KiB pages, not shared; the only import allowed is `m2.asset_read`; a v0 module is refused, naming its version (ABI-3) | `core/runtime/src/packages.rs` (`verify_module`) |
| Assets for kernels: `m2.asset_read(handle, channel, frame, dst, frames) -> frames read`. The host decodes each file once (the existing `audio::Cache`, by content hash), at its own rate, as 32-bit floats, and shares it. Nothing is copied into a sandbox. The sandbox's data keeps a table of its assets and its memory's base, so a read allocates nothing | `core/runtime/src/sandbox.rs`, `core/runtime/src/audio.rs` (one accessor only) |
| Kernel settings in the graph: a kernel node gains `assets` (asset names) and `values` (numbers). Both are part of its shape (RENDER-25), so a new sample makes the box new | `core/ir/src/lib.rs`, `core/ir/src/check.rs`, `core/runtime/src/keep.rs` |
| The SDK: `kernel(id, inputs, settings)`, `setting.asset()` and `setting.keymap()` (MIDI note to asset name). `@m2/core` 1.2.0 → 1.3.0 | `core/script/sdk/core.ts`, `core/script/src/lib.rs` |
| The revision records the hash of each asset a kernel reads, as it does for clips (RENDER-2). A missing one is `asset.missing`, naming the track, and that kernel reads silence | `core/song/src/data.rs`, `crates/server/src/graph/*` |
| Reserved, shapes fixed, not built: event ports `{count, cap, events}`; `k_save` and `k_load` (run off the audio thread, as a plugin's state load); `k_latency` and `k_tail` (fixed by the settings); the offline flag and the time map's place in the settings block | `docs/kernel-abi.md` only |
| On go: the birth cap (256 MiB a sandbox); the sandbox limit from the 25% rule (about 4,000 a renderer), checked at build, with report §2.7's plain words; G7f judges the app against S only; the Deferred row "Spectre guard" is done | `sandbox.rs`, `crates/gates/src/store.rs`, `arena.rs`, `crates/server/src/graph.rs` |
| The contract: ABI-2 (built), ABI-3 (v1; v0 refused), ABI-5 (new: the five rules), ABI-1 (the host places ports), RENDER-25 (settings: channels, asset hashes, values), RENDER-28's *Today*, Deferred ("Kernel ABI v1" goes) | `docs/architecture.md` |

**Tests (red first).**

| Test | Red on |
|---|---|
| `a_v0_module_is_refused_naming_its_version` | today's check, which takes v0 |
| `only_the_asset_import_is_allowed` (one wat module imports `m2.asset_read`, one imports `env.clock`) | today's check, which refuses every import |
| `a_kernel_reads_its_asset_through_the_import` (a wat test kernel copies its asset to its output) | before the import exists |
| `an_asset_read_allocates_nothing` (under the allocation trap, RENDER-28) | the read stubbed to look the memory up by name |
| `a_new_sample_makes_the_box_new` (the shape holds the asset's hash) | the shape without the assets |
| `a_missing_asset_is_named_and_silent` | before the build looks assets up |
| On go: `a_song_past_the_sandbox_limit_is_refused_in_plain_words`; `a_kernel_past_its_birth_cap_says_it_ran_out` (today's 32 MiB test, moved) | the limit stubbed out |

**Done when:** `m2_abi()` is 1 and v0 is refused; the 10 renders match; `std.wasm` rebuilds to its bytes in another folder (PLAT-5), and the commit says the kernel hashes changed; std's version stays 0.2.0 in this slice (no render changed, FMT-3); the mac lint passes; `docs/kernel-abi.md` and the contract say the same.

**Changed while building** (2026-10-18):
- `an_asset_read_allocates_nothing` is red with the read stubbed to copy through a `Vec` (198 heap operations in 99 blocks). Looking the memory up by name, the plan's stub, allocates nothing in wasmtime 49, so it can't show red.
- The settings block keeps slice 1's layout. Values are f64s; an asset entry is 16 bytes: handle, frames, channels, rate. A missing asset has 0 frames and 0 channels, and reads silence. `m2.asset_read` writes silence past the end and returns the frames it read.
- The revision records kernel assets' hashes in its data (`DataSet::kernel_assets`), not in the snapshot: the snapshot is read before the code says which files kernels read. A file a kernel reads that changes on disk is heard at the next build (gaps).
- `setting.keymap()` is empty unless given, so it is never required.
- The Rust kit gains no asset call in this slice (its docs only point to `docs/kernel-abi.md`, with the line count kept, so `std.wasm` keeps its bytes). The call comes with the sampler in slice 3, which changes `std.wasm` anyway.
- The sandbox limit is 3,999, the same on every OS (the Mac's 4.097 GiB a memory; Linux would allow 4,033).
- G7f keeps three sides. "P1" was already the app's layout since commit B; it is now named the A/A control and printed as what noise alone gives. Two sides would need a new turn order (`paired::cycle` needs three).
- `a_kernel_past_its_birth_cap_says_it_ran_out` is new in `core/runtime/src/tests.rs` (40 MiB at birth plays; past 256 MiB fails). The keep tests over memory stay, with 256 MB in their words: after the seal any grow fails, whatever the cap.

## Slice 3: the sampler

**Goal.** A std instrument plays one audio file, or one file per range of notes, through ABI v1's asset import. It shows in the app's browser. Dropping a WAV on its card sets its sample through a Tx. It renders the same bits on every thread count (RENDER-2).

**Files.**

| Part | Files |
|---|---|
| The kernel `sampler`: inputs `pitch`, `gate`, `velocity`, `tune`; settings: its assets, and a root and a note range for each. A note picks its zone when it starts. Playback speed is 2^((note − root + tune) / 12) × the file's rate / the song's rate. It reads with the cubic Hermite formula clips use. One-shot (plays to the file's end; `active` while it plays) or gated | `std/kernels-src/src/sampler.rs` (new), `std/m2pkg.toml` |
| The component `sampler`: settings `sample` (an asset), `root` (60), `zones` (a keymap), `mode` (`"oneshot"` or `"gate"`), `voices` (8); controls `tune`, `level`, `attack`, `release`. Category "Sampler", so it shows in the browser and in + Track. `zones` wins when both are written; each zone covers from its root up to the next zone's root | `std/src/sampler.ts` (new), `std/m2pkg.toml` (std 0.2.0 → 0.2.1: a new instrument, no render changed) |
| The builder: write or replace `sample: "<name>"` in the sampler's call, checked against the graph; one undo step. The WAV comes in first through the existing import (`import::audio`: create-only, never deleted) | `crates/editor/src/sampler.rs` (new) |
| The route: `POST /api/graph/sample` (`{session, at, call, file}`): import, then the Tx | `crates/server/src/graph/*` |
| The app: a sampler's card in Device is a drop target for one WAV. With no sample it says "Drop a WAV here" | `web/src/lib/drop.ts`, the Device card component |
| A new pure example with a small WAV, made by a checked-in script so its bytes are fixed: in the 3 fixture lists and in `e2e/renders.sha256` (2 new lines) | `examples/sampler/` (new), `e2e/baseline.sh`, `core/play/tests/bit_identity.rs`, `core/session/src/gate_tests.rs` |

**Tests (red first).**

| Test | Where | Red on |
|---|---|---|
| `at_its_root_a_sample_plays_its_own_samples`, `an_octave_up_plays_every_second_sample`, `a_note_picks_its_zone`, `one_shot_plays_past_the_note_off`, `a_missing_sample_is_silence` | `std/kernels-src/src/tests.rs` (native testkit) | the kernel stubbed to play silence |
| `the_sampler_example_gives_the_same_bits_on_1_to_7_threads` | `bit_identity.rs` (the example in its list) | before the example exists |
| `dropping_a_wav_sets_the_sample_and_undo_takes_it_back` (undo restores the code's bytes; the asset stays) | `crates/editor/src/sampler_tests.rs` | before the builder |
| the route: a WAV makes an asset and one Tx; a non-WAV is refused in words | `crates/server` | before the route |
| browser suite `ab-sampler`: the browser lists Sampler; + Track adds it; a WAV dropped on its card plays (the export isn't silent); undo | `e2e/ab-sampler.mjs` | before the drop target |

**Done when:** all of the above pass; 12 of 12 renders match (10 old, 2 new); a picture of the sampler's card from the app on Xvfb `:95` (`app-shot.sh`); `ci/check.sh --gates` once at the end of the track, through `tw`: PASS; short build notes `docs/designs/step9/build-notes-ab.md` and the HANDOFF lines.

**Changed while building** (2026-10-18):
- **The kernel takes more inputs**: `pitch, gate, velocity, tune, level, attack, release, mode`. A one-shot plays past the note's end, so an envelope outside the kernel (std:adsr on the gate) would cut it off: the fades and the level are the kernel's own. `mode` is an input read when a note starts, not a value, so the values are only the zones: a root, a lowest and a highest note for each file (left out: 60, 0, 127).
- **The kit's calls**: `Cfg::values`, `Cfg::assets` (`&'static` slices into the settings block, which lives as long as the sandbox) and `Asset::read`. Off wasm32, `m2_kernel::native` keeps test files on the test's thread.
- **The kernel's tests** are in `std/kernels-src/src/sampler/tests.rs`, as each instrument's are, with one more: `no_control_breaks_it`.
- **A Sampler with no file must build** from its notes alone (+ Track's rule), so `@m2/core` 1.3.0 (not yet released) gains `SettingDecl.optional()`: left out, a setting is `undefined`. The descriptor says `optional: true` only when it is, so no other descriptor changes.
- **Zones**: the lowest zone also plays the notes below its root, so every note plays something.
- **+ Track's order**: Drums, Instruments, then the other categories by name, so a new category never splits a group. Sampler comes before Sound design.
- **More lists than the 3 fixture lists**: `renders.json` (std 0.2.1, and the example's render), `every-component` (it calls every std component) and the planners' call forms, std's catalog tests, the server's list test and `s8-tracks`.
- **The example** has three files, made by `examples/make-sampler-wavs.mjs` (Node), outside the example's folder so "Open an Example" never copies it into a song. One file is at 44.1 kHz and one is stereo, so the rate and both channels are covered.
- **The route** takes a path (`{session, at, call, file}`, the app) or the bytes (`?session=&at=&call=&name=`, a browser), as the audio import does. Its undo step is "setting Sampler to kick.wav".
- **The builder refuses** a Sampler whose code gives `zones`, a file picked by code, and the file it plays already, in words.
- **The card**: a row under its knobs ("Drop a WAV here", the file, or "2 files: …"); the whole card takes the drop. The view's node says `sampler: {files}`.
- **The browser suite** uses the `export` kind, to render with the command line. first-song isn't silent, so it checks that the export changes with the drop and comes back with Undo.
- **The pictures** were taken with a copy of `app-shot.sh` whose scratch HOME is under `~/m2-runs/sab`.

## Decisions

Defaults chosen here, so no one waits. The owner can change any of them.

| # | Decision | Why |
|---|---|---|
| D1 | ABI v1 is report §2.3's shape in both outcomes; the spike decides only the host's layout | one state per sandbox frozen into the ABI would block idea 3 for good (report §1, §9) |
| D2 | The spike's kit and std are kept, not thrown away; only commit B goes on a no-go | the kit is needed either way; it saves a rebuild of the same work |
| D3 | Pairing: whole G7f runs of `main`'s build and the spike's, alternated, 3 pairs, held on one die, each against S fresh | uses the gates as they are; S in every run cancels slow drift of the machine |
| D4 | Assets: one import, `m2.asset_read`, of decoded floats, shared outside every sandbox (report §8, question 1) | one copy of a sample however many voices play it; ready for streaming later; works in both layouts |
| D5 | The kernel does the resampling by its pitch ratio; the host decodes at the file's own rate | one interpolation, not two; the math is the kernel's own, so the bits are the same on every OS |
| D6 | Birth cap 256 MiB; the sandbox limit per renderer, from the 25% rule (report §8, question 2) | a song either fits or doesn't, every time |
| D7 | Garbage-collected languages are out of scope for v1; `k_drop` is not reserved; one `k_new`'s memory lies in one run of bytes (report §11) | keeps v1 small; an optional export can be added later without breaking v1 |
| D8 | `k_save`/`k_load` run off the audio thread; latency and tail depend only on the settings; offline kernels work in chunks (report §8, question 4) | fixed now so they don't collide later; each is built with its first kernel |
| D9 | A trapped box keeps today's path: silent alone, reset at the next activation or a panic (RENDER-25) | no new failure machinery in this track; its one weak spot goes to gaps |
| D10 | The sampler's sample is named in the code (`sample: "kick.wav"`), as a setting; changing it makes the box new | the code is the preset (homes of values); one Tx, one undo |
| D11 | The sampler is one-shot by default, 8 voices, root 60 | drum hits and chops are its first use (gaps #22) |
| D12 | std 0.2.0 → 0.2.1 with the sampler; `@m2/core` 1.2.0 → 1.3.0 | a new instrument and new SDK words; no existing render changes, so no minor bump (FMT-3) |
| D13 | If idea 3 goes, it replaces the owner's 32 MiB layout (O1, 2026-10-15) on this measured rule; the Mac's G7f run then confirms or reverses it | the task's rule decides; PLAT-1 keeps the Mac as the judge |

## Risks

| Risk | How bad | What covers it |
|---|---|---|
| One run is good to about ±150 µs a callback, and other tracks build on this server at the same time | medium | runs held on one die; the load is noted with each run; 3 more pairs near 0 (the rule) |
| The pool sandbox touches `lib.rs` (2,900 lines), where other tracks may work too | medium | the sandbox in its own file; `lib.rs` changes kept to `KernelNode` and `VoicePool`; merge `main` before the final check |
| Track `io` (disk streaming) may change `audio::Cache` | low | the sampler reads assets through one accessor; streaming can sit behind the same import later |
| A `Vec` kernel without its own reset traps at prewarm | low | the build fails and names it; the red test of slice 1 |
| The kit's allocator may not link on the pinned Rust for wasm32 | low | found in step 1a, the first thing built |
| The ceiling of about 4,000 sandboxes a renderer (go only) | medium | a clear message at build; p50 uses 200 |
| A host call from kernel code costs more than expected | low | measured with the sampler on p16; a block reads each asset once a voice |

## To gaps.md instead (one line each)

- The sampler: loop points, start and end, reverse, velocity layers, round robin, choke groups.
- The sampler: dropping several WAVs to make zones (zones are code only); a waveform and a root picker on its card.
- Samples are held in memory; streaming them waits for RENDER-30's reader.
- Formats other than WAV for samples, as for import.
- An asset named in code has no hash pin (FMT-15 covers clips only).
- Reserved in ABI v1, not built: event ports, `k_save`/`k_load`, `k_latency`/`k_tail`, offline kernels.
- A box that trapped is reset, not rebuilt, so its stack loses the trapped frame each time.
- A song asking for std "0.2" in an older app that has 0.2.0 gets a missing import, not `pkg.unsatisfied`.
- On go: deadlines for other people's kernels (wasmtime epochs); the `sum` kernel and a checked engine if a song nears the ceiling.
- The Mac's numbers for the new layout (G7f, footprint, address space) are owed.

## How every run is kept safe

- Every run of music2: `HOME=/home/jerry/m2-runs/sab/home`, `M2_AUDIO=none`, `M2_MIDI=none`, `CARGO_HOME=/home/jerry/.cargo`, `RUSTUP_HOME=/home/jerry/.rustup`.
- Builds in this worktree's own `target/`, with `--locked`. `main`'s copy for the spike builds under `sab/main` and is deleted after.
- The app only on Xvfb `:95`. Long jobs through `~/m2-runs/tw`. No OS code is planned (PLAT-2); the mac lint still runs.
