# The sounds merge: build notes

What each slice of the `sounds` merge did, and what its checks said. The plan is
`~/m2-runs/sounds/plan.md` (with its Attack and the lead decisions L1 to L7); slice 4 copies it here as
`design.md`. Each slice adds its own section. All runs are on Linux (jerry-ax102), in the worktree
`~/m2-runs/wt/sm`, branch `sounds-merge`, made from `main` at `cbac57c`.

## Slice 1: the merge

**In short**
- `sounds` is merged onto step 7. It conflicted only where the plan said: the `use` lines of
  `bit_identity.rs`, and `std.wasm`. `std.wasm` was rebuilt, not taken from either side.
- `sounds`' own kernel tests pass on Linux the first time: 107 of 107. Clippy finds nothing. One `allow`
  had no written reason; it has one now.
- `examples/sounds` is now in every render record and in G7c. The 8 old records and the 4 old
  `renders.json` hashes did not move.
- A new test checks that every std kernel fits its 32 MiB memory at 384 kHz. The most is `std:delay`,
  11.88 MiB. Each of the 7 new kernels holds 0.19 MiB.
- `ci/check.sh --gates` passes at `e8bba0e`, and `ci/check.sh` at `ec0acad`: every row, nothing passed
  only on a retry.
- G7f plays the same music as before the merge. Its numbers moved less than one run's noise. No lead
  decision (L5) was needed.
- Found on the way: this worktree's `target/` had moved from step 7's worktree, and some of what was in
  it still pointed there. Two checks failed because of it. A clean build fixed it. One test that then
  passed having checked nothing now fails instead.

### Slice 0: the re-check

| What | Result |
|---|---|
| `git merge-tree --write-tree cbac57c origin/sounds` | Conflicts in `core/play/tests/bit_identity.rs` and `std/kernels/std.wasm` only, as the plan found against `step7` |
| `origin/sounds` | still `916127d` |

### What was built

| Commit | What |
|---|---|
| `3d4a552` | The merge (`--no-ff`). `bit_identity.rs`: the union of both sides' `use` lines, as the plan wrote it. `std.wasm` rebuilt with `std/build.sh`: 215,070 bytes (step 7's 143,431; `sounds`' Mac build 215,110). `std/build.sh --check` gives the same bytes. |
| `71195b9` | `drums.rs`: a written reason for `#[allow(clippy::large_enum_variant)]` (boxing the largest drum sound would allocate at a hit, on the audio thread). `std.wasm` rebuilt: 3 bytes move, the line numbers in its panic messages. |
| `2796b13` | `examples/sounds` joins `PURE` (`core/session/src/gate_tests.rs`: `renders.json` and the permutation test), `e2e/baseline.sh` and `e2e/renders.sha256` (two cases each), and `g7c()` in `bit_identity.rs`. `baseline.sh` skips a fixture the commit doesn't have, and says so (risk 22). |
| `e8bba0e` | The O1 memory test, `every_std_kernel_fits_its_memory_at_the_highest_rate` (`core/runtime/src/tests.rs`). |
| `52801b6` | `fixture_data_files_are_canonical` fails when it finds no data files (below, "Found"). |

The five-fixture `FIXTURES` lists (`gate5_tests.rs`, `delta_tests.rs`) are left alone, as the plan says.
README's merged text was read once: it reads right.

### `sounds`' own checks, first

Run before anything else in the slice, so `sounds`' failures would show apart from the merge's.

| Check | Result |
|---|---|
| `cargo test --locked --release --workspace` in `std/kernels-src` | 107 passed, 0 failed. These tests had only run on a Mac before. No tolerance needed changing (risk 6). |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` there, Rust 1.99.0 | Clean (risk 7). One `allow` had no written reason: fixed in `71195b9`. |
| `cargo test -p m2-script --test std` | 7 passed |

### The records

Recorded on Linux. Debug and release `m2` render the same bytes.

| Record | `examples/sounds` | The old ones |
|---|---|---|
| `e2e/renders.sha256` | `examples-sounds.wav` `15fa77b6…`, `examples-sounds-from4-for6.wav` `38b33cd6…` | 8 of 8 the same |
| `core/session/tests/renders.json` | `76178757…`; `"std"` stays `"0.1.0"` | 4 of 4 the same |

Risk 22: a checkout without `examples/sounds` (a scratch copy of the tree without it) rendered its 8
cases and printed `skipped examples/sounds: … doesn't have it`, exit 0. Before the change, `cp -R` of
the missing folder would stop the script (`set -e`).

Risk 16, G7c on new kinds of graph: `a_no_op_activation_changes_no_bit` and
`an_activation_with_rebuilt_lane_data_changes_no_bit` pass on `examples/sounds`, first time. No keep or
swap bug showed. `examples/sounds` also passes the permutation test, `live_is_the_export_on_every_pure_fixture`
and `sounds`' own after-idle drum test.

### The O1 memory test

Every std kernel is built at 384 kHz (the highest rate a project may have) in stereo, with its inputs
at their defaults. It plays one block and is reset. Its memory must stay under the 32 MiB cap. The test
prints the one that holds the most.

| Kernel | Memory at 384 kHz, stereo |
|---|---|
| `std:delay` | 11.88 MiB (the plan guessed about 6; its reset builds new lines before it drops the old) |
| `std:reverb` | 1.88 MiB |
| `std:chorus` | 0.25 MiB |
| the other 33, the 7 new ones among them | 0.19 MiB each |

Red first: with the bound set to 4 MiB for a trial, the test fails and names `std:delay` at 11.88 MiB.

### The check

| Run | Result |
|---|---|
| `ci/check.sh --gates` at `e8bba0e` (`~/m2-runs/check-sm1c`, 31 min) | **PASS**, every row: 915 Rust tests, 0 failed; clippy; std kernels; std.wasm; mac lint; 29/29 browser suites; 10 of 10 renders; old engine (S); the gates, all 6 runs; repo unchanged. Nothing passed only on a retry. G11 p95 9.0 to 9.4 ms; G7a heard at p95 113 ms. |
| `ci/check.sh` at `ec0acad`, the slice's tip but these notes' last lines (`~/m2-runs/check-sm1d`, 10 min) | **PASS**, every row, the same counts: 915 Rust tests, 0 failed; 29/29 browser suites; 10 of 10 renders. Nothing passed only on a retry. No gates: the commits after `e8bba0e` change a test and these notes only. |
| The two runs before them (`check-sm1`, `check-sm1b`) | FAIL, both from the moved `target/` (below). Not counted. |

### G7f beside step 7's (information on Linux, PLAT-1)

The app's layout against S, by the mean, each from `ci/check.sh --gates`, not held on one die:

| | Step 7 at `ffc36aa` | After the merge, `e8bba0e` |
|---|---|---|
| A block (bound 80) | +20 µs (+17 to +27) | +10 µs (−8 to +15) |
| A callback (bound 320) | +86 µs (+46 to +147) | −18 µs (−89 to +67) |
| P1 against S: block, callback | +8, +63 | −17, −48 |
| The app's layout against P1: block, callback | +12, +48 | +27, +67 |
| The starting line: blocks (the app's layout, P1) | +1, +17 | +17, +19 |
| The starting line: callbacks | +63, +26 | −27, −49 |
| The full build (S's) | 219 ms (183) | 221 ms (182) |
| The slowest activation block (bound 5,333) | 3,142 µs | 2,797 µs |
| Footprint of one renderer of p50 | 195 MB | 203 MB |
| The same music | blocks `22cd1b3810530499`, callbacks `cdb85ca0bbb29381` | the same hashes |

- One run can be off by about ±150 µs a callback and ±30 µs a block (step 7's A/A control). Every change
  above is inside that. The app did not get slower.
- The footprint grew by 8 MB. The plan expected up to 24 MB on Linux (one more 4 KiB page per kernel
  instance, for std's larger data). Not judged here: S's footprint record is the Mac's.
- **Risk 17.** G7f's S plays its own std, from `8aa301e`: 176 exports, 9,848 bytes of data. The app's
  side now plays the merged std: 218 exports, 12,940 bytes of data, which each kernel instance copies.
  p50 uses none of the new kernels, so all three sides still play the same music. If a Mac bound fails,
  check this first. L5 said: if G7f's numbers move, try letting S's runner play the app's `std.wasm`.
  They didn't move on Linux, so nothing was changed.

### How long things take now (risk 18)

Every load of std compiles `std.wasm`; it is half as big again.

| What | Step 7 | After the merge |
|---|---|---|
| Loading std: `m2 render` of `templates/empty` for 0.1 s, 6 runs each | 0.18 s every run (std from `cbac57c`) | 0.25 s every run (+70 ms) |
| The "rust tests" row | 224 s (two runs) | 283 s (+59 s) |
| … of which `m2-session`'s unit tests (the kill tests start many processes, each loading std) | 141 s | 189 s |
| The "app" row | first-song in 1 s | first-song in 1 s (it counts whole seconds) |
| The "browser suites" row | 219 s | 221 s |
| The "renders" row | 4 s | 10 s (2 more renders, 27 s of music each) |
| The "gates" row | 17.6 min | 18.2 min |
| The whole check, with the gates | 26 min | 31 min (with a build from nothing: 97 s) |

Part of the +59 s is `examples/sounds`' own new tests (risk 14 guessed 1 to 2 minutes). Information only.

### Found: a `target/` folder that had moved

This worktree's `target/` was step 7's, moved from `~/m2-runs/wt/s7` (since deleted). Cargo reuses what
it finds there. Some of it still named the old folder:
- Build-script outputs (tauri, its plugins, wasmtime, cranelift, rquickjs) pointed at files under
  `wt/s7/target`. Run 1 (`check-sm1`) failed to build the desktop app, so the tests, clippy, the mac
  lint and the browser suites failed. The "app" row passed with an old binary.
- After `cargo clean -p` of those 8 packages, run 2 (`check-sm1b`) built. But some test binaries
  were not rebuilt, and they look for the repo's files from the folder they were built in. Three failed
  (`os_code_lives_in_m2_os`, `the_audio_side_never_blocks_or_allocates`, `the_fast_read_is_the_old_read`);
  the last passed on its retry, which built it again. It was stopped during the gates.
- `fixture_data_files_are_canonical` would pass in that state having checked nothing. Shown in a scratch
  copy of the tree: built there, then its `examples/` and `templates/` moved away, the old test passes;
  with `52801b6`, it fails and names the folder.
- The fix: `cargo clean` (77 GB) and a build from nothing (97 s). Run 3 (`check-sm1c`) passed.

For the lead: a `target/` should not move between worktrees. If one does, `cargo clean` it first.

### Left, and why

- The Mac run: only it judges G7f (PLAT-1), with the larger std. Owed, as planned.
- `e2e/README.md` and HANDOFF still say 8 renders: slice 4 (docs).
- Other tests that find the repo's files from where they were built fail loudly when it moves (an
  `unwrap`); only the fixtures test passed quietly. They were not all read.
- Scratch: `~/m2-runs/sm-scratch` (renders, the std load timing). The check logs are in
  `~/m2-runs/check-sm1`, `-sm1b` and `-sm1c`.

## Slice 2: O3, std's effects follow the song

**In short**
- std's chorus, phaser and tremolo now wobble only while the song plays. Stopped, they wait.
  Press Play and they carry on. The bitcrusher's sample clock is put back at Play to where it was
  at Stop.
- So a song played after the engine sat stopped sounds exactly like its export. A test plays a
  held chord through each effect, alone, after 2 s and after 5 blocks stopped. Each one failed
  before the last commit and passes after it.
- How: a new signal, `time.playing` (1 while the song plays, else 0), in the IR, the engine and
  the SDK. Each of the four kernels has a last input, `playing`. std/fx connects it.
- The attack's fix A landed first: a component call that reads song time gets its own `time`
  node, named under its call. Without it, Add Chorus on the starter's chords was refused.
- **No render record changed**: 10 of 10 e2e renders, and the 5 hashes in `renders.json`.
  Offline the song plays on every frame, so the effects do the same sums as before.
- `ci/check.sh --gates` passes at `09b2dfd`: every row, nothing passed only on a retry. So do the
  recorded tests (`--ignored`, 5 of 5). G7f moved less than one run's noise.
- Found on the way: the first full check failed one row, "repo unchanged", because I wrote these
  notes while it ran. The second, on a tree left alone, passed.

### What was built

| Commit | What |
|---|---|
| `8b7b7e9` | `TimeOf::Playing` in the IR (serialised `"playing"`, unit `unit01`; `TimeOf::unit`). The engine fills it from the block's runs: 1 where `moving`, else 0, as plugins are told (HOST-6). No new `Block` buffer: nothing is filled unless a node reads it (the attack's "smaller change"). Nothing reads it yet. |
| `02aab27` | Fix A (L3). `@m2/core` exports `time` (`beats`, `seconds`, `playing`), the same object as `ctx.time`. Read outside any call: the module's one node, as before. Read inside a component call or a `key()` frame: one node per call, cached by frame and scope (a voice has its own). `delta.rs` names such a node under its frame chain. WRITE-8's bullet says so. |
| `ed2e9ec` | The `playing: control<unit01> = 1` input, last, on `chorus`, `phaser`, `tremolo` and `bitcrush` (`fx.rs`, `m2pkg.toml` with descriptions that say "left out, always"). `std.wasm` rebuilt: 215,070 → 217,359 bytes; `std/build.sh --check` gives the same bytes. Nothing connects it yet. |
| `09b2dfd` | std/fx passes `playing: time.playing` in the four bodies. The after-idle test is renamed and widened; the tremolo stop, seek and wrap test; the graph view test. |

The rules, in the kernels:

| Effect | Stopped (`playing` 0, or NaN) | At Play |
|---|---|---|
| chorus, phaser, tremolo | the LFO phase stays where it is | it steps again from there |
| bitcrush | its clock runs on, so a tail is still crushed (a held sample would freeze into DC) | the clock is put back to its value at Stop; never played: 1.0, its start |

While `playing` is 1 the arithmetic is exactly the old one. On the audio thread, per effect call:
one 128-float fill of its `time` node a block, one more input copy, one compare a sample. Nothing
allocates.

### Tests

| Test | Where | Proves |
|---|---|---|
| `time_reads_in_its_own_unit` | `core/ir/src/check.rs` | `playing` is `unit01`, at the top level and in a voice; beats and seconds don't fit a `unit01` port |
| `time_playing_follows_the_transport` | `core/runtime/src/tests/live.rs` | 0 stopped; 1 from the block line Play lands on, through a seek and a loop wrap 96 frames into a block; 0 from the stop |
| `time_is_one_node_per_call` | `core/script/src/tests.rs` | a call reading twice has one node in its frame; two calls, two nodes; a voice in the call its own; at the top `time` and `ctx.time` are one node |
| `a_call_reading_time_added_or_removed_beside_another_renames_nothing` | `core/session/src/tx/delta_tests.rs` | adding a call before another of its kind (built first) and removing it are both confined to the call |
| SDK types | `core/script/sdk/check/calls.ts` | `time.playing` is `Control<"unit01">`, not beats; `time` can't be written |
| `playing_the_lfo_steps_as_it_always_did` | `std/kernels-src/src/fx/tests.rs` | against an oracle copy of the old `Phase`, bit for bit: 400,000 samples, 10 rates (absurd ones too) changing every 9,000, at 4 sample rates and limits |
| `playing_the_bitcrusher_crushes_as_it_always_did` | same | against an oracle copy of the old bitcrusher, bit for bit, over every torture setting |
| `stopped_the_effects_wait_and_play_on_where_they_were` | same | each of the four, stopped 20,003 samples with `playing` 0 or NaN, then played: bit for bit the run with no stop; and a fresh one that sat stopped plays as one made at Play |
| `std_plays_the_export_after_the_engine_sat_stopped` (was `drums_play_…`) | `core/play/tests/bit_identity.rs` | after 2 s and after 5 blocks stopped, live is the export: the drum beat; a held chord through chorus, phaser, tremolo and bitcrush, one song each; first-song, resample, sounds (whole), starter, empty. It lists every song that differs, not only the first |
| `tremolo_holds_while_stopped_and_never_jumps` | `core/play/tests/live.rs` | on a steady level: stopped, the level holds at the export's level there; Play goes on bit for bit as the export from that frame; through 3 or more mid-block loop wraps and a seek, no step bigger than twice the wobble's own |
| `effects_that_follow_the_song_show_no_time_node` | `crates/editor/src/view_tests.rs` | a chain of all four (two choruses) shows one node per call, 6 edges, only the declared knobs |

`the_starters_add_reverb_keeps_the_hooks_reverb` (Add Chorus on the starter's chords) passes
unchanged. In `ed2e9ec` its labels showed the folded default (`std:chorus[0.6,0.4,0.4,1]`); from
`09b2dfd` `playing` is a cable, and the labels are as before.

`common/mod.rs` gains `project(what, files)`: a temp project with std's manifest. The drum beat,
the effect songs and the tremolo song use it, so slice 3 changes `std = "0.1"` in one place there.

### Red first

| Test | Run without the change | Result |
|---|---|---|
| after idle | on `ed2e9ec` (the port, not yet connected) | failed, every difference listed below |
| `tremolo_holds_…` | std/fx of `ed2e9ec` | "stopped, the level holds at [0.72498727, 0.72498727]": the level moved on 1 frame after the stop landed |
| the delta test | `delta.rs` of `8b7b7e9` | "the edit was refused: song/time:beats/#1 is added outside the ranges it may change" |
| the kernel test | `Phase` stepping and the clock never put back | failed on chorus first; with only the bitcrusher's put-back removed, on bitcrush from sample 0 |
| the view test, and the starter's Add Chorus | the SDK making one `time` node per module | 9 edges: a cable from one shared time node into every effect; Add Chorus refused ("chords/std:chorus@…/time:playing/#0 is removed outside the ranges it may change") |

The after-idle failures on `ed2e9ec` (frame: the first that differs from the export):

| Song | After 96,000 frames (2 s) | After 640 frames (5 blocks) |
|---|---|---|
| a held chord through chorus | 480 | 492 |
| … through phaser | 1 | 1 |
| … through tremolo | 1 | 1 |
| … through bitcrush | 5 | 5 |
| examples/sounds (chorus on keys and strings) | 435 | 436 |
| templates/starter (chorus on chords) | 386 | 387 |
| the drum beat, first-song, resample, empty | the same | the same |

On `09b2dfd` all pass. Risk 9: the whole of `examples/sounds` (its master limiter, compressor,
`duck` and `delay`) plays the export after idle. No other defect was found, so nothing was
narrowed.

### The records

| Record | Result |
|---|---|
| `e2e/renders.sha256` | 10 of 10 the same, at `ed2e9ec` and at `09b2dfd` |
| `core/session/tests/renders.json` | unchanged (its test passes; `"std"` stays `"0.1.0"`) |
| G7c on every pure fixture and p16 | no bit changes |

### The check

| Run | Result |
|---|---|
| `ci/check.sh --gates` at `09b2dfd` (`~/m2-runs/check-sm2b`, 27 min) | **PASS**, every row: 921 Rust tests, 0 failed (915 at slice 1, and 6 new); clippy; std kernels (110 tests, 107 at slice 1); std.wasm; mac lint; 29/29 browser suites (`e2e/dsp.mjs` among them: a chorus added after the Keys from the browser, its settings as knobs); 10 of 10 renders; old engine (S); the gates, all 6 runs; repo unchanged. Nothing passed only on a retry. G11 p95 8.3 to 9.1 ms; G7a heard at p95 113 ms. |
| `ci/check.sh` at `ddf63da`, the slice's tip but these lines (`~/m2-runs/check-sm2c`, 10 min) | **PASS**, every row, the same counts: 921 Rust tests, 0 failed; 29/29 browser suites; 10 of 10 renders. Nothing passed only on a retry. No gates: the commit after `09b2dfd` changes these notes only. |
| The run before the gates' (`~/m2-runs/check-sm2`, at `09b2dfd`) | FAIL on "repo unchanged" only: I edited these notes in the worktree while it ran, and that row saw it. Every other row passed, the gates too. Not counted. Nothing is written in the worktree during a check now. |
| The recorded tests, `cargo test --locked -p m2-play --release --test bit_identity --test live --test churn -- --ignored --nocapture` (`~/m2-runs/logs/sm-recorded-1017-032326-3122353.log`, 11 min) | **PASS**, 5 of 5: `live_is_the_export_on_all_of_p16`, `live_is_the_export_on_p50` and `a_no_op_activation_on_p50` (422 s together); `p16_churns_200_edits_within_20_mb` (183.0 MB after 70 edits, then 177.5 MB); `p16_loops_ten_minutes_without_drift` (79 laps, 504 Ons each, 116 s). |
| Each commit, before it was made | `8b7b7e9`: the `m2-ir` and `m2-runtime` tests, clippy. `02aab27`: `cargo test --locked --workspace`, 919 passed, 0 failed; the SDK's `tsc` check. `ed2e9ec`: the workspace run failed one test, the starter's Add Chorus labels (see Tests); after its fix the editor's 150 passed; `std/kernels-src` tests, clippy and `cargo fmt --check` clean; the 10 renders made and matched. `09b2dfd`: the workspace, 921 passed, 0 failed; the 10 renders matched. Clippy clean at each. |

G7f beside slice 1's, for information (PLAT-1; p50 uses none of the four effects, so all three
sides play the same graph as before):

| | Slice 1, `e8bba0e` | Slice 2, `09b2dfd` |
|---|---|---|
| A block, the app's layout against S (bound 80) | +10 µs (−8 to +15) | +2 µs (−9 to +8) |
| A callback (bound 320) | −18 µs (−89 to +67) | +52 µs (+29 to +87) |
| P1 against S: block, callback | −17, −48 | −2, +26 |
| The app's layout against P1: block, callback | +27, +67 | +3, +25 |
| The full build (S's) | 221 ms (182) | 217 ms (185) |
| The slowest activation block (bound 5,333) | 2,797 µs | 3,060 µs |
| Footprint of one renderer of p50 | 203 MB | 203 MB |
| The same music | blocks `22cd1b3810530499`, callbacks `cdb85ca0bbb29381` | the same hashes |

Every change is inside one run's noise (about ±30 µs a block, ±150 µs a callback).

### Left, and why

- Docs (slice 4): RENDER-6, RENDER-7 and RENDER-33's *Today*, WRITE-8's *Today*, README's
  `time` row and std's effects line, `song-as-program.md`'s three time inputs, the module doc of
  `bit_identity.rs` (std's effects and drums no longer need a fresh engine; top-level oscillators
  and noise still do), HANDOFF (L2: Space, Enter, Space carries the wobble on; the Mac by-ear
  check).
- What still differs from the export, on purpose (L2): Stop, back to bar 1, Play is a seek. The
  wobble and the drums' hit counts carry on. "Stop, then Play in place" carries on the wobble
  only: the chorus's delay lines and the phaser's filters hold the tail, the bitcrusher a sample of
  it.
- The std bump to 0.2 (slice 3).
