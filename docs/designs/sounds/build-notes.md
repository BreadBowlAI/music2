# The sounds merge: build notes

What each slice of the `sounds` merge did, and what its checks said. The plan is
`$SCRATCH/sounds/plan.md` (with its Attack and the lead decisions L1 to L7); slice 4 copies it here as
`design.md`. Each slice adds its own section. All runs are on Linux (the server), in the worktree
`$SCRATCH/wt/sm`, branch `sounds-merge`, made from `main` at `cbac57c`.

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
| `ci/check.sh --gates` at `e8bba0e` (`$SCRATCH/check-sm1c`, 31 min) | **PASS**, every row: 915 Rust tests, 0 failed; clippy; std kernels; std.wasm; mac lint; 29/29 browser suites; 10 of 10 renders; old engine (S); the gates, all 6 runs; repo unchanged. Nothing passed only on a retry. G11 p95 9.0 to 9.4 ms; G7a heard at p95 113 ms. |
| `ci/check.sh` at `ec0acad`, the slice's tip but these notes' last lines (`$SCRATCH/check-sm1d`, 10 min) | **PASS**, every row, the same counts: 915 Rust tests, 0 failed; 29/29 browser suites; 10 of 10 renders. Nothing passed only on a retry. No gates: the commits after `e8bba0e` change a test and these notes only. |
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

This worktree's `target/` was step 7's, moved from `$SCRATCH/wt/s7` (since deleted). Cargo reuses what
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
- Scratch: `$SCRATCH/sm-scratch` (renders, the std load timing). The check logs are in
  `$SCRATCH/check-sm1`, `-sm1b` and `-sm1c`.

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
the effect songs and the tremolo song use it, so slice 3 changes std's version in one place there.

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
| `ci/check.sh --gates` at `09b2dfd` (`$SCRATCH/check-sm2b`, 27 min) | **PASS**, every row: 921 Rust tests, 0 failed (915 at slice 1, and 6 new); clippy; std kernels (110 tests, 107 at slice 1); std.wasm; mac lint; 29/29 browser suites (`e2e/dsp.mjs` among them: a chorus added after the Keys from the browser, its settings as knobs); 10 of 10 renders; old engine (S); the gates, all 6 runs; repo unchanged. Nothing passed only on a retry. G11 p95 8.3 to 9.1 ms; G7a heard at p95 113 ms. |
| `ci/check.sh` at `ddf63da`, the slice's tip but these lines (`$SCRATCH/check-sm2c`, 10 min) | **PASS**, every row, the same counts: 921 Rust tests, 0 failed; 29/29 browser suites; 10 of 10 renders. Nothing passed only on a retry. No gates: the commit after `09b2dfd` changes these notes only. |
| The run before the gates' (`$SCRATCH/check-sm2`, at `09b2dfd`) | FAIL on "repo unchanged" only: I edited these notes in the worktree while it ran, and that row saw it. Every other row passed, the gates too. Not counted. Nothing is written in the worktree during a check now. |
| The recorded tests, `cargo test --locked -p m2-play --release --test bit_identity --test live --test churn -- --ignored --nocapture` (`$SCRATCH/logs/sm-recorded-1017-032326-3122353.log`, 11 min) | **PASS**, 5 of 5: `live_is_the_export_on_all_of_p16`, `live_is_the_export_on_p50` and `a_no_op_activation_on_p50` (422 s together); `p16_churns_200_edits_within_20_mb` (183.0 MB after 70 edits, then 177.5 MB); `p16_loops_ten_minutes_without_drift` (79 laps, 504 Ons each, 116 s). |
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

## Slice 3: the bump, std 0.1 to 0.2

**In short**
- std is now 0.2.0. Every project the repo ships, every manifest written in code, and the doc
  examples ask for std "0.2". `git grep -nE 'std = \\?"0\.1'` finds nothing.
- No render changed. `renders.json` differs only in its `"std"` line. The 10 e2e records match.
- A new test reads every `music2.toml` the repo ships and checks the bundled std meets it. So the
  next bump needs no grep.
- A song that asks for an older std now gets one more sentence in its error: "This app has std
  0.2. To use it, change this line to std = "0.2". The song may sound a little different." Only for
  std, and only when the app's std is newer (L7).
- Found: G7f would have failed at the final check. S's runner builds the projects m2-gates writes,
  and S's own std is 0.1.0, so S refused them. The runner now builds a copy that asks for S's
  std. Both G7f parts pass by hand, with the same music as before.
- `ci/check.sh` passes at `b790a63`: every row, nothing passed only on a retry.

### What was built

| Commit | What |
|---|---|
| `a3214f6` | S's runner (`crates/gates/s/main.rs`) compiles a copy of each project, in its own folder, with the manifest's std line set to S's std. Its README says so. Below, "Found". |
| `6591ecd` | The bump. `std/m2pkg.toml` 0.2.0. The 9 project files. The manifests in code: the desktop app's test, `core/play`'s `common/mod.rs` (one place since slice 2), `core/session` (`MANIFEST`, p16 and p50), `core/song` (data tests, `GOOD` and its `>=` case), the editor's tests, the gates' songs, the server (export, files, graph tests, lib, project ×2), `e2e/fx-fixes.mjs` ×2. `renders.json`: `"std": "0.2.0"`. Doc examples: README, `core/song`'s doc comments, `architecture.md` (RENDER-2's *Why*, the layout, the manifest example), `song-as-program.md` (`m2pkg.toml`). The tests below. |
| `b790a63` | The `pkg.unsatisfied` hint (L7). `core/song/src/manifest.rs`: only for `std`, only when the installed std is past what the song admits. It names the shortest requirement that admits the app's std (`Requirement::admitting`: `"0.2"` for 0.2.0). |

Left as they are, on purpose (plan 4.2): `@m2/core = "1"`, `format = 1`, `IR_VERSION`, the Rust
crates' own versions, and the caret-rule cases in `manifest.rs` (they test the rule, not std). One
line of slice 2's notes named the old requirement; it now says "std's version".

### Tests

| Test | Where | Proves |
|---|---|---|
| `every_shipped_manifest_is_met` (new) | `core/session/src/lib.rs` | every `music2.toml` under `examples`, `templates`, `core/session/tests/fixtures` and `e2e/fixtures` is met by the bundled std and the SDK, through the same check a build runs. Each folder must hold at least one, or it fails: a test built where the repo no longer is would check nothing. |
| `unmet_dependencies` (moved, L4) | `core/song/src/manifest.rs` | with `GOOD` on "0.2": std 0.2.4 is met, 0.3.0 is not, and the message names "0.2" and 0.3.0. Then the hint: there for std 0.3.0; not for the SDK, not for a std older than asked (0.1.9), not for another package. |
| `caret_requirements` (one more loop) | same | `Requirement::admitting` gives "0.2" for 0.2.0 and 0.2.7, "0.0" for 0.0.3, "1" for 1.4.2, "12" for 12.0.0, and each admits its version |
| `unmet_dependencies_block` | `core/session/src/lib.rs` | a song asking for std "9" (newer): no hint. Asking for "0.0" (older): the hint names the bundled std's own numbers, read from `std/m2pkg.toml`. The test writes the std line itself, so the next bump can't leave it testing nothing. |
| `fixture_renders_change_only_with_stds_version` | `core/session/src/gate_tests.rs` | as before. Now it first says which fixture doesn't build, and why. Before, a fixture left behind at a bump rendered nothing and panicked deep in the engine (`index out of bounds`). |

### Red first

| Test | Run without the change | Result |
|---|---|---|
| `every_shipped_manifest_is_met` | written on `30739f3` (green), then only `std/m2pkg.toml` moved to 0.2.0 | failed, naming all 9 projects: `examples/with-plugins/music2.toml: music2.toml asks for std "0.1" (at least 0.1.0, below 0.2.0), but 0.2.0 is installed`, and the same for the other 8 |
| the renders test, at the same point | before its change | panicked in `core/runtime/src/lib.rs` (`index out of bounds: the len is 0`); after it: `examples/first-song doesn't build with std 0.2.0: [… pkg.unsatisfied …]` |
| `unmet_dependencies` (attack, claim 3) | after the sweep, before moving it | failed: `m.check_dependencies(installed("1.0.0", Some("0.1.4"))).is_empty()` |
| the hint, both tests | the hint's arm switched off | `manifest.rs`: the message ended at "but 0.3.0 is installed". Session: `asks for std "0.0" (at least 0.0.0, below 0.1.0), but 0.2.0 is installed`, with no hint |
| G7f with the bump | `m2-gates store --reference 50`, S's runner as it was | `FAIL store: couldn't run: S's runner: /tmp/m2-gates-…/p50-0 doesn't compile: [… pkg.unsatisfied … asks for std "0.2" (at least 0.2.0, below 0.3.0), but 0.1.0 is installed …]` |

### Found: G7f's S can't build the app's projects after a bump

- G7f (RENDER-32) times S, HEAD `8aa301e`, beside the app. m2-gates writes p50 and hands the folder
  to S's runner. The runner builds it with S's own code and S's own std, 0.1.0.
- S checks dependencies too (FMT-3, since step 4). After the bump, p50 asks for std "0.2", so S
  refused it, and G7f couldn't run. Neither the plan nor the attack saw this: the attack only said the
  final `--gates` run would cover the gates' manifests. This slice's check has no gates, so it would
  have shown up only at the final check.
- The fix keeps what G7f already did: S plays its own std. The runner copies the project into its own
  folder and sets the std line to S's version. p50 uses none of std's new kernels, so all three
  sides still play the same music, and G7f checks that.
- Run by hand on the bumped tree (`$SCRATCH/logs/sm3-g7f-1017-035254-3363279.log`), S's runner
  built in a scratch cache:

| | Slice 2, `09b2dfd` (in the check) | Slice 3, by hand |
|---|---|---|
| Verdict, both parts | PASS | PASS |
| A block, the app's layout against S (bound 80) | +2 µs (−9 to +8) | −15 µs (−162 to +21) |
| A callback, the same (bound 320) | +52 µs (+29 to +87) | −76 µs (−407 to −66) |
| P1 against S: block, callback | −2, +26 | −26, +4 |
| The full build (S's) | 217 ms (185) | 223 ms (189) |
| The slowest activation block (bound 5,333) | 3,060 µs | 2,783 µs |
| Footprint of one renderer of p50 | 203 MB | 203 MB |
| The same music | blocks `22cd1b3810530499`, callbacks `cdb85ca0bbb29381` | the same hashes |

Information only (PLAT-1). The server was not quiet: another agent's job ran beside parts of both
runs. The point is that G7f runs again and hears the same music. The final `--gates` run builds the
runner again in its usual cache (`~/.cache/m2-check/s-8aa301e`): its code changed, so `build.sh`
takes its copy of S again.

### Found: the renders test needs its copy until the render ends

My first version of the renders test's change opened each fixture with `let s = fixture(n).open();`.
That drops the copied folder at the end of the line. `examples/resample` plays an audio file from
that folder, so its render came out different (`8658f49d…` instead of `b3daee50…`), and the test
failed: "a pure fixture renders other bits while std is still 0.2.0". The fixture now lives until the
render is done, and the record is back to `b3daee50…`. Never committed. It shows the test catches a
changed render.

### The records

| Record | Result |
|---|---|
| `core/session/tests/renders.json` | only `"std": "0.1.0"` → `"0.2.0"`; the 5 hashes the same |
| `e2e/renders.sha256` | unchanged; 10 of 10 match in the check |

### The check

| Run | Result |
|---|---|
| `ci/check.sh` at `b790a63`, the slice's code (`$SCRATCH/check-sm3`, 10 min) | **PASS**, every row: 922 Rust tests, 0 failed (921 at slice 2, and the new manifest test); clippy; std kernels; std.wasm; mac lint; 29/29 browser suites; 10 of 10 renders; repo unchanged. Nothing passed only on a retry (`failed` and `flaky` empty). No gates: no kernel changed, and G7f was run by hand above. |
| `ci/check.sh` at `df017d7`, the slice's tip but this line (`$SCRATCH/check-sm3b`, 10 min) | **PASS**, every row, the same counts: 922 Rust tests, 0 failed; 29/29 browser suites; 10 of 10 renders. Nothing passed only on a retry. The commit after `b790a63` changes these notes only. |
| Before each commit | `6591ecd`: `cargo test --locked --workspace`, every test passed but the renders test, which my own mistake above broke; after its fix it passed; clippy clean. `b790a63`: `m2-song`'s 76 tests, the session's dependency tests, clippy clean. |

### Left, and why

- Docs (slice 4): FMT-3's bullet and *Today* (the hint; the trigger's wording, L6), the
  `pkg.unsatisfied` row of the diagnostics table ("for an older std, says how to move"), the
  Deferred row, HANDOFF's note for the owner. Write the owner's line without the literal old
  requirement, or the done grep finds it, for example "change the std line to std = "0.2"".
- The final `ci/check.sh --gates` judges G7f and runs the other gates on the moved manifests
  (`crates/gates/src/songs.rs`, `core/session/src/reference.rs`).
- For the lead: `a3214f6` is a choice the plan didn't name. It keeps S on its own std, as before
  (risk 17, L5). The other way, S playing the app's std, can't work: the app's std/fx reads
  `time.playing`, which S's IR doesn't have.
- Scratch: `$SCRATCH/sm-scratch/s-cache` (S's runner, about 1 GB) is deleted. The checks' logs are
  in `$SCRATCH/check-sm3` and `-sm3b`.

## Slice 4: docs

**In short**
- The docs now say what the merge built. Nothing in the code changed but one test's module doc.
- The contract: RENDER-6, RENDER-6a, RENDER-7 and RENDER-33's *Today*; FMT-3 in L6's words; the
  `pkg.unsatisfied` row; WRITE-8's *Today*; the Deferred row of the lockfile.
- HANDOFF: the merge in "In short" and the branch table; the Mac run's new row and its by-ear steps
  (L2's Space, Enter, Space among them); L2 as an open owner decision; the FMT-3 row moved to the
  decisions; Left for later; a note for the owner about old songs; merge notes for `taste`.
- `design.md` beside these notes: the plan, trimmed to what was built and why, with the attack and
  L1 to L7 folded in.
- Every code name the changed docs cite exists: a script checked the 308 names they cite.

### What changed

| File | What |
|---|---|
| `docs/architecture.md` | RENDER-6: the transport clock drives `time.playing`; *Today* says what is built (the LFOs, the bitcrusher's clock, the drums' hit count), what still differs (Space, Enter, Space: L2's default; free-running sources). RENDER-6a: `time.playing` is 0 at Stop. RENDER-7: the rule names `time.playing`; *Today* says how it is filled, as plugins are told (HOST-6's twin for kernels, in one line). RENDER-33's *Today*: five pure fixtures, the after-idle test, and what "fresh" still matters to. FMT-3: the hint, L6's trigger, and a *Today* for std 0.2. The diagnostics table's `pkg.unsatisfied`. WRITE-8's *Today*: the `Op::Time` arm and its test. The Deferred row: L6's words. |
| `README.md` | the SDK table's `time` row gains `time.playing`, and says a component's body imports `time`; one plain paragraph under std's effects |
| `docs/song-as-program.md` | three time inputs; the IR's `time` op reads `playing` too; std is 36 kernels; `std/synth`, `std/fm` and `std/drums` in the table and the module list; "`time` is always created at module level" was no longer true since slice 2: now it says where a call's `time` node is made |
| `docs/gaps.md` | "What works today" gains one line: a song you open sounds like its export, and what Space, Enter, Space does |
| `e2e/README.md` | ten renders, `examples/sounds` among them; the skip of a fixture an older commit lacks |
| `core/play/tests/bit_identity.rs` | the module doc: std's effects and drums no longer need a fresh engine; top-level oscillators, `noise` and smoothers still do |
| `HANDOFF.md` | as in "In short". Also: the check table's counts (922 Rust tests, 10 renders, about 10 and 30 minutes); the out-of-memory gap's numbers (`delay` at 384 kHz, from slice 1's test); the logs and scratch rows |
| `docs/designs/sounds/design.md` | new |

### Checks

| Check | Result |
|---|---|
| Every backticked name on the added lines, against the repo outside the changed files (a script: `git grep -F` for each name, or each part of a qualified one, or the path) | 308 names; 43 not found that way, and each checked by hand: 14 commit hashes (`git cat-file`) and the branches; check folders (`check-sm4` is this slice's); paths written from `docs/designs/` or short (`sounds/design.md`, `testkit.rs`, `sdk/check/std.ts`); commands; TOML (`[package] version`, `@m2/core = "1"`); `music2.lock`, a deferred file; `Run.moving`, a field of `Run` (`core/runtime/src/transport.rs`); and `std_plays_the_export_after_the_engine_sat_stopped`, which lives in `bit_identity.rs`, one of the changed files. One was the doc's own notation, not the SDK's (`Control<Beats>`): `song-as-program.md` now writes the SDK's types (`Control<"unit01">`). |
| The test and code names the docs cite, by hand | each found: `time_reads_in_its_own_unit`, `time_playing_follows_the_transport`, `time_is_one_node_per_call`, the delta test, the three kernel tests, `std_plays_the_export_after_the_engine_sat_stopped`, `tremolo_holds_while_stopped_and_never_jumps`, `effects_that_follow_the_song_show_no_time_node`, `the_starters_add_reverb_keeps_the_hooks_reverb`, `every_shipped_manifest_is_met`, `unmet_dependencies`, `unmet_dependencies_block`, `fixture_renders_change_only_with_stds_version`, `every_std_kernel_fits_its_memory_at_the_highest_rate`, `Requirement::admitting`, `Transport::starting`, `PURE`, `g7c()` |
| `git grep -nE 'std = \\?"0\.1'` | nothing (the owner's note says "the std line", never the old line itself) |
| The `taste` merge notes | `git merge-tree --write-tree sounds-merge origin/taste`: conflicts in `docs/gaps.md` and `std/src/instruments.ts` that `main` doesn't have, as the plan found; the rest (`delta.rs`, `delta_tests.rs`, `e2e/README.md`, `e2e/fx-fixes.mjs`, the server's graph, `GraphCanvas.tsx`, `architecture.md`) conflict with `main` already |

### Found

- The plan said `taste`'s "+ Track" list would leave out the 22 new instruments because they have
  no category. Read in `taste`'s code: a missing category falls back to "Instruments". They are left
  out because the list takes a component only through an export with no dot in its name, and the
  new ones are exported only as members (`synth.lead`). The merge note says both.
- `song-as-program.md` still said `time` nodes are always made at module level, so a voice never
  holds one. Untrue since slice 2's fix A. Fixed.
- HANDOFF said `delay` needs about 1.6 MiB. Slice 1's test measured 11.88 MiB at 384 kHz.

### The check

| Run | Result |
|---|---|
| `ci/check.sh` at `f1fa181`, the slice's tip but these lines (`$SCRATCH/check-sm4`, 10 min) | **PASS**, every row: 922 Rust tests, 0 failed (as at slice 3); clippy; std kernels; std.wasm; mac lint; 29/29 browser suites; 10 of 10 renders; repo unchanged. Nothing passed only on a retry (`failed` and `flaky` empty). No gates: no code changed but one test's module doc. The commit after `f1fa181` changes these notes and HANDOFF's "In short" only. |
| Besides the run | `cargo test --locked -p m2-play --test bit_identity --no-run` built the test whose module doc `056223c` changed (the run's "rust tests" row builds and runs it too); the names script and the std grep (Checks, above). |

### Left, and why

- Plan section 5, after the slices: a review with several lenses, the skeptic, fixes, and a final
  `ci/check.sh --gates`. That run times G7f with S on its own std (slice 3's `a3214f6`; information
  on Linux, PLAT-1) and runs the other gates on the moved manifests. Then HANDOFF says "Linux green at …; macOS owed".
- The recorded tests (`--ignored`) last ran at slice 2 (`09b2dfd`). Slice 3 moved the std line in
  their shared `core/play/tests/common/mod.rs`, and `ci/check.sh` doesn't run them. Run them once
  more before the merge lands.
- For the owner: L2 (Space, Enter, Space). It is in HANDOFF's open owner decisions, with its default:
  the wobble carries on.
- The Mac run, with the by-ear steps 18 to 21: owed, by hand, after the merge lands.

## After the slices: the review, the fixes, and the final checks

**In short**
- A review with several lenses (the merge's tests, the engine's contract, the docs) gave 12 findings that the
  skeptic confirmed. Some were the same problem seen by two or three lenses: 8 problems in all.
- One was a test that could not see what it claimed to test (F1). One was a hint the app hid (D3). The rest were
  docs that said more, or less, than the code does.
- All are fixed, in 7 commits: one new test, one code change with its tests, five doc fixes (D2 and D4 share one).
- The full `ci/check.sh --gates` at `e108e46`, the last fix, passes: every row, nothing passed only on a retry. So
  do the recorded tests, 5 of 5. The commit after it changes these notes, HANDOFF and the contract's *Today*
  lines only.
- A screenshot of **Pop sounds** in the app, on an invisible screen:
  `$SCRATCH/app-shots/sounds-e108e46/pop-sounds.png`.

### The review's findings, and what was done

| Finding | Severity | What was wrong | Fix | Test or check |
|---|---|---|---|---|
| F1 | medium | `stopped_the_effects_wait_and_play_on_where_they_were` could not see the bitcrusher's two stop rules. Its stops landed where the clock is exactly 1, its start value, and it fed silence while stopped. | `8b8a907`: one more stop, after 30,001 samples; a new test, `stopped_the_bitcrusher_still_crushes` (stopped, it crushes noise bit for bit as while playing, and never holds one value) | red first, below |
| D3 | low | The `pkg.unsatisfied` hint came at the end of a 189-character message. The app shows it on one line, cut at the window's edge: at 1440 px about 143 characters show, so the hint was cut off. | `bb2f981`: the hint comes first. HANDOFF, FMT-3 and `design.md` quote the new message; HANDOFF also says to hold the mouse over it to read all of it. | `unmet_dependencies`, `unmet_dependencies_block`: they now check the message starts with the hint. Red first, below |
| F2, ENG-2, D5 | low | `song-as-program.md`: "`time` read in a track's own code" is made at module level. Not inside a `key()` frame: there it is made in that frame, and can sit in a voice. | `9c6c54f` | doc; read against `timeNode` (`core/script/sdk/core.ts`) |
| F3, ENG-1, D1 | low (D1 was medium; the skeptic lowered it) | RENDER-6's *Today* said "nothing of std's that sounds follows the engine clock". README, `gaps.md` and `design.md` said a song you open sounds like its export however long it sat. Not so for an oscillator or `noise` at a track's top level: it runs on while stopped. | `14fd0d8`: each now says std's effects and drums, and what still runs on. Also the after-idle test's comment and `fx.rs`' module doc. | doc; comments only in code: `std/build.sh --check` gives the same bytes |
| ENG-3 | low | RENDER-16, the Deferred list and order of work item 9 said mono, legato and glide are built later. std's `mono()` does them since the merge. | `4deb78c`: RENDER-16's *Today* says how `mono()` is built; its *Why*, the Deferred row and item 9 keep only what is deferred, the `poly` op's own options. The row's early trigger (808 slides) is marked as met in std. | doc |
| D7 | low | RENDER-33's *Today* and README said `examples/sounds` plays every synth, FM sound and drum. It never calls `fm.custom`. | `b801724`: both name what it plays. Adding an `fm.custom` call would move its render records. | doc |
| D2 | low (was medium; the skeptic lowered it) | The Mac's by-ear steps 18 to 20: step 18 started the app while step 16's still ran; step 19 could fall after the song's end (about 27 s); step 20 said Enter, not the Mac's Return. | `e108e46`: step 18 quits first; step 19 stops in the first 20 seconds; step 20, and the open question, say Return | doc |
| D4 | low | HANDOFF said this worktree's `target/` is about 70 GB, step 7's. Slice 1 cleaned and rebuilt it: 40 GB. | `e108e46` | doc; `du -sh` |

### Red first

The bitcrusher's code is right today, so F1's test is shown red on two broken copies of it. Each ran on a
copy of `std/` in the scratchpad, never in the worktree: `cargo test --locked --release --workspace`.

| Test | Run against | Result |
|---|---|---|
| the old tests | (a) `self.kept = self.clock` taken out, so Play always puts the clock back to 1 | 110 of 110 pass: blind |
| the old tests | (b) the clock steps only while playing, so stopped it holds one sample | 110 of 110 pass: blind |
| the stop test with 30,001 | (a) | fails: "bitcrush (1 ch), after 30001 samples, stopped with playing 0", left `Some(0)` |
| `stopped_the_bitcrusher_still_crushes` | (b) | fails: "bitcrush (1 ch), stopped after 0 samples: not crushed as while playing", left `Some(4)` |
| the new tests | the real code | 111 of 111 pass |
| `unmet_dependencies` | the old message, hint last | fails at `manifest.rs:384`, and prints the message: "music2.toml asks for std "0.2" (…), but 0.3.0 is installed. This app has std 0.3. …" |
| `unmet_dependencies_block` | the old message | fails at `lib.rs:348`, the same way |
| both | the new message | pass |

### The final checks

| Run | Result |
|---|---|
| `ci/check.sh --gates` at `e108e46` (`$SCRATCH/check-sm-fg`, 29 min) | **PASS**, every row: 922 Rust tests, 0 failed (as at slice 4); clippy; std kernels (111 tests, 110 before F1's); std.wasm; mac lint; 29/29 browser suites; 10 of 10 renders; old engine (S); the gates, all 6 runs; repo unchanged. Nothing passed only on a retry: `failed` and `flaky` are empty. The run's log: `$SCRATCH/logs/check-sm-fg-1017-051929-4107515.log`. |
| The recorded tests at `e108e46`, `cargo test --locked -p m2-play --release --test bit_identity --test live --test churn -- --ignored --nocapture` (`$SCRATCH/logs/sm-recorded-fg-1017-054825-118021.log`, 9 min) | **PASS**, 5 of 5. They last ran at slice 2; slice 3 moved the std line in their `common/mod.rs`. |
| Before each fix's commit | F1: the kernel crate's tests (111 of 111), clippy, `cargo fmt --check`, `std/build.sh --check` (the same bytes). D3: `m2-song`'s 75 tests, the session's dependency tests, clippy. F3: `std/build.sh --check` (a comment changed in `fx.rs`), `bit_identity` built. |

The summary, verbatim:

```
music2 check: PASS
commit   e108e46ff7c5 on sounds-merge (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 29 min
plugins  ~/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)

toolchain        PASS  rustc 1.99.0
web build        PASS  
build            PASS  
app              PASS  the UI connected: first-song in 1 s, with-plugins in 1 s
rust tests       PASS  922 passed, 0 failed
clippy           PASS  
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
mac lint         PASS  the macOS code type-checks and lints
browser suites   PASS  29/29 suites ok
renders          PASS  10 of 10 match e2e/renders.sha256
old engine (S)   PASS  built for G7f from a clean copy of 8aa301e, in ~/.cache/m2-check/s-8aa301e
gates            PASS  the 6 runs ci/gates.txt names
repo unchanged   PASS  
```

Its gate lines, verbatim:

```
PASS  G11 tx-data (PLANE-5, PLANE-1, WRITE-15) on server on p16
PASS  G11 watcher (WRITE-19) on server on p16 with 5,000 files in assets/
PASS  G5 and G7a add effect, through the server (RENDER-8, RENDER-25, RENDER-25a) on server on p50 from 180 s
PASS  G5 add effect (RENDER-25, 25a, 8) on engine (step 7) on p50
PASS  G7b tails of untouched tracks (RENDER-25, 25a) on engine (step 7) on p50
PASS  G7e export (RENDER-1, RENDER-6a, HOME-4, HOME-6) on server on examples/with-plugins, real time
PASS  G7f store (RENDER-32) on the runtime on p50
PASS  G7f the arena's callback (RENDER-32, RENDER-25) on the player on p50 from 180 s
ok    the same music: every renderer's timed blocks the same bits: the same 2880000 samples (hash 22cd1b3810530499) from the app's layout, P1 and S in every round, peak -1.0 dB
skip  block within S + 80 µs, by the mean: would pass: -25 µs (5 pairs, -51 to +9) over S, measured here (bound 80). p99, not judged: -226 µs (5 pairs, -407 to +1204); not judged off the reference Mac (PLAT-1)
ok    the same music: every player's timed callbacks the same bits: the same 2856960 samples (hash cdb85ca0bbb29381) from the app's layout, P1 and S in every round, peak -1.0 dB
skip  callback within S's + 320 µs, by the mean: would pass: -166 µs (5 pairs, -775 to +125) over S, measured here (bound 320). p99, not judged: -1709 µs (5 pairs, -9226 to +6201); not judged off the reference M
ok    the slowest activation block under half the device buffer's period: 3427 µs, p50 1892; the block before it p50 2700, max 4179 (bound 5333 µs; the run's p99 callback 20885)
```

The recorded tests, verbatim:

```
test a_no_op_activation_on_p50 ... ok
test live_is_the_export_on_all_of_p16 ... ok
test live_is_the_export_on_p50 ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 384.54s
p16, 283 activations: footprint 183.4 MB after 70 edits, then 177.4 MB (-5.9 MB)
test p16_churns_200_edits_within_20_mb ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 16.26s
p16, 10 minutes of a loop: 79 laps, 504 Ons each, rendered in 117.4s (release)
test p16_loops_ten_minutes_without_drift ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 16 filtered out; finished in 117.64s
```

G7f beside slice 2's (information on Linux, PLAT-1). This is the first `--gates` run since slice 3, where S's runner
began to set each project's std line to S's own std (`a3214f6`). Other agents' jobs ran on the server before the
check (load average about 5.5), so the callbacks spread wider than at slice 2; I ran nothing else during the gates.

| | Slice 2, `09b2dfd` | Final, `e108e46` |
|---|---|---|
| A block, the app's layout against S (bound 80) | +2 µs (−9 to +8) | −25 µs (−51 to +9) |
| A callback, the same (bound 320) | +52 µs (+29 to +87) | −166 µs (−775 to +125) |
| P1 against S: block, callback | −2, +26 | −29, −273 |
| The app's layout against P1: block, callback | +3, +25 | +23, −11 |
| The full build (S's) | 217 ms (185) | 221 ms (185) |
| The slowest activation block (bound 5,333) | 3,060 µs | 3,427 µs |
| Footprint of one renderer of p50 | 203 MB | 203 MB |
| The same music | blocks `22cd1b3810530499`, callbacks `cdb85ca0bbb29381` | the same hashes |

### For the owner: a screenshot

`$SCRATCH/app-shots/sounds-e108e46/pop-sounds.png`: the debug app at `e108e46` with **Pop sounds**
(`examples/sounds`) open, on an invisible Xvfb screen, as the check's app row opens it (a copy of the song, a
scratch `HOME`, no sound). It shows the timeline with the song's tracks, the track tabs, and the Arp track's
graph: `synth.pluck` with its knobs. The check's own two screenshots are in `$SCRATCH/check-sm-fg/`
(`app-first-song.png`, `app-with-plugins.png`).

### Left, and why

- The lead lands it: push `sounds-merge`, and fast-forward `main` (no agent pushed). Then delete the worktree
  `$SCRATCH/wt/sm` with its `target/` (about 40 GB) and the detached `$SCRATCH/wt/sounds`.
- The Mac run, with the by-ear steps 18 to 21: owed, by hand, after it lands. One run then covers step 7 too.
- For the owner: L2 (Space, Return, Space). It is in HANDOFF's open owner decisions, with its default: the
  wobble carries on.
- The compile bar still cuts other long messages at the window's edge (D3 moved only the std hint first). Wrapping
  them belongs with `taste`'s redraw of the UI.
- Scratch: the scratch `HOME`s under `$SCRATCH/sm-scratch` are deleted. What is left there is small evidence the
  slices' notes cite: the renders of slices 1, 3 and 4 (30 MB each), copies of the old and S's `std.wasm`, and a
  few logs.
