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
- `ci/check.sh --gates` passes at `e8bba0e`: every row, nothing passed only on a retry.
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
| The two runs before it (`check-sm1`, `check-sm1b`) | FAIL, both from the moved `target/` (below). Not counted. |

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
