# The sounds merge: design

Written 2026-10-17, at the end of the merge's fourth slice, on the branch `sounds-merge` (from `main` at
`cbac57c`). It is the plan (`$SCRATCH/sounds/plan.md`, 2026-10-16), trimmed to what was built and why, with
its attack and the lead's decisions L1 to L7 folded in. What each slice did, its checks, its red-first runs and
its numbers are in `build-notes.md` beside this file.

**In short**
- `sounds` is merged onto step 7. It brings 22 new std sounds: 8 on one polysynth, 4 FM sounds, and a drum
  machine of 10. They run on 7 new kernels, which fit the 32 MiB each kernel may hold (O1) many times over.
- O3 is fixed. std's chorus, phaser and tremolo wobble only while the song plays. Stopped, they wait. Press
  Play and they go on. The bitcrusher's clock is put back at Play to where it was at Stop.
- So std's effects and drums sound as in the export when you press Play, however long the song sat first. Each
  of the four effects failed that test before the fix, and passes it now.
- How: a new song-time signal, `time.playing`: 1 while the song plays, 0 while it is stopped. std's effects
  read it. No kernel ABI change.
- No existing render changed. Offline, the song plays on every frame, so the effects do the same sums.
- std went from 0.1 to 0.2, with every project the repo ships. A song that asks for the old std is told how to
  move.
- What still differs from the export, on purpose: Stop, back to bar 1, Play. That is a jump, and the wobble
  carries on. The owner may still change this (HANDOFF's open owner decisions). And an oscillator or noise at a
  track's top level runs on while stopped (3.6).

| | Before | After |
|---|---|---|
| std's sounds | 5 simple instruments | the same 5, and 22 fuller ones (`std/synth`, `std/fm`, `std/drums`) |
| std's kernels | 29 | 36 |
| `std.wasm` | 143,431 bytes | 217,359 bytes |
| Chorus, phaser, tremolo after a stop | from another phase than the export | as the export |
| std's version | 0.1.0 | 0.2.0 |
| Pure fixtures and render records | 4 fixtures; 8 e2e renders | 5 (`examples/sounds` added); 10 |

## 1. What `sounds` brought

`origin/sounds` at `916127d`, based on `8aa301e`: 68 files, +7,673 / −148.

| Area | What |
|---|---|
| Kernels (Rust, in `std.wasm`) | `synth`, `fm`, `drum` (one per voice); `notecv`, `mono`, `drumhit`, `hitcount` (notes as signals). `fx.rs`' helpers became `pub(crate)`, with no change in behaviour. |
| Kernel tests | a test file per kernel, and `testkit.rs` |
| Components (TypeScript) | `std/src/synth.ts` (8 sounds and `mono()`), `fm.ts` (4), `drums.ts` (10). The commit says 20; there are 22. `instruments.ts` gets editor descriptions. |
| Tests | `bit_identity.rs`: the drums after idle; `core/script/tests/std.rs` checks every knob reaches a kernel; `sdk/check/std.ts`; the `every-component` fixture; the editor's `forms.ts` |
| Example | `examples/sounds`, **Pop sounds** on the start screen: 12 tracks, 16 bars at 140 bpm, no plugins |
| Docs | README's std section, `docs/gaps.md`, `docs/music-basics.md` |

How `sounds` keeps its drums equal to the export after a stop: a drum hit takes its noise seed from how many
hits its lane has played (`hitcount`, at the top level), never from samples processed. A one-note voice
(`mono()`) skips all its work while its envelope is idle, so it doesn't drift while stopped.

## 2. The merge

Onto step 7 it conflicted in two places only, as the plan found:

| File | Resolved |
|---|---|
| `core/play/tests/bit_identity.rs` | only the `use` lines: both sides' imports |
| `std/kernels/std.wasm` | neither side taken: rebuilt with `std/build.sh`, then `std/build.sh --check` gave the same bytes |

Places that merged cleanly but could have broken, and what happened:

| Place | What happened |
|---|---|
| `sounds`' after-idle drum test, written against the player before step 7 | built as it was: `Player::driven`, `open`, `transport` and `applied` keep their shapes |
| The new kernels' tests, run on Linux for the first time (host maths differs from the Mac's) | 107 of 107 passed; no tolerance changed |
| Clippy under the pinned Rust 1.99.0 | clean; one `allow` got its written reason |
| O1: each kernel memory is capped at 32 MiB | a new test, `every_std_kernel_fits_its_memory_at_the_highest_rate`: every std kernel at 384 kHz in stereo, played one block and reset. The most is `std:delay`, 11.88 MiB; each new kernel holds 0.19 MiB |
| `examples/sounds` was in no render record | added to `PURE` (`core/session/src/gate_tests.rs`), `e2e/baseline.sh`, `e2e/renders.sha256` (two cases) and G7c's `g7c()`; recorded on Linux. `baseline.sh` skips a fixture an older commit doesn't have, and says so (risk 22) |
| G7c on graphs step 7 never kept (polys feeding top-level kernels, several polys on one lane) | passed the first time; no keep or swap bug |
| G7f with a larger `std.wasm` | its numbers moved less than one run's noise on Linux. The Mac judges |

## 3. O3: std's effects follow the song

### 3.1 The defect

`chorus`, `phaser` and `tremolo` (`std/kernels-src/src/fx.rs`) each step an LFO phase on every sample they
process. That is engine time. Stopped, the engine keeps processing for a while (RENDER-6a), so an engine that
sat stopped has moved these LFOs on. Played from bar 1, they start from another phase than the export.
`bitcrush` had the same defect, found while planning: its sample clock ran on engine time too.

A kernel knows nothing of time but its port buffers. Song time reaches it only through a port, as `time.beats`
reaches std's `lfo`. The engine already knew, per frame, whether the song moves (`Run.moving`), and told plugins
(HOST-6). Kernels had no such signal.

### 3.2 The options

| | 1. Wait while stopped (built) | 2. Phase from the song's position | 3. Start again at each Play |
|---|---|---|---|
| How | the LFO steps only on frames where the song plays: a new signal, `time.playing` | `phase = frac(rate × time.seconds)` | phase to 0 at Play, a seek and a wrap |
| Live after a stop equals the export | yes, from a fresh open after any wait, and Stop then Play in place carries on | yes, everywhere | only from bar 1 |
| At a seek or a loop wrap | carries on smoothly, as a delay's echoes do (RENDER-9) | jumps: a click on tremolo | jumps to 0 at every wrap |
| Turning the rate knob | smooth | broken: at 3:00, 0.1 Hz more sweeps 18 cycles during the turn | smooth |
| Offline renders | none change | every song with these effects changes | none change |

Also dropped: stepping by the change in `time.seconds`. It is an `f32`; after 4:16 two neighbouring samples can
share one value, so the LFO would move in lumps.

L1: option 1, with the wiring written in std/fx, not hidden as a manifest default (the attack's alternative).

### 3.3 What was built

**The rule, in plain words:** std's chorus, phaser and tremolo wobble only while the song plays. Stopped, they
wait where they are. Press Play and they carry on. Through a loop or a jump they keep going, with no jump of
their own.

| Part | Change |
|---|---|
| IR | `TimeOf::Playing` (`core/ir/src/lib.rs`), serialised `"playing"`, unit `unit01` (`TimeOf::unit`). No `IR_VERSION` bump: it only adds. |
| Runtime | `Exec::Time(TimeOf::Playing)` fills its output from the block's runs: 1 where `moving`, else 0 (`core/runtime/src/lib.rs`). No new buffer in `Block`, as the attack suggested: nothing is filled unless a node reads it. No allocation, no maths. |
| SDK | `@m2/core` exports `time` (`beats`, `seconds`, `playing`), the same object as `ctx.time`, so a component's body can read song time. `@m2/core` stays at "1": an addition is not a break. |
| One node per call (attack A, L3) | Read outside any call, `time` is the module's one node, as before. Read inside a component call or a `key()` frame, it is one node per call, cached by frame and scope (a voice has its own). So the node sits inside the effect's box in the graph view, and in the delta's names. |
| The delta (WRITE-8) | `delta.rs` names a `time` node made in a call under that call's frame chain. Before, a call added before another of its kind took the other's number, and Add Chorus on the starter's chords was refused. |
| Kernels | `chorus`, `phaser`, `tremolo` and `bitcrush` get a last input, `playing: control<unit01> = 1` (`std/m2pkg.toml`; `Phase` and `Bitcrush` in `fx.rs`). `Phase` steps only when `playing > 0.5`: 0 or NaN waits. While playing, the sum is exactly the old one. Left out, `playing` is 1, so a direct `kernel("std:chorus", …)` call runs as before; the manifest's descriptions say so. |
| std/fx | the four bodies pass `playing: time.playing`. No new knob: the editor shows the same knobs. |

What it costs on the audio thread, per effect call: one 128-float fill of its `time` node a block, one more
input copy into the kernel, and one compare a sample. Nothing allocates.

### 3.4 The bitcrusher's rule

It can't wait while stopped: its held sample would freeze into a DC level. So **at Stop it remembers its clock,
and at Play it puts it back.** In between it keeps crushing the tail.
- A fresh one has nothing remembered: Play puts back 1.0, its start, so its first sample takes a new value at
  once, as frame 0 does offline. The remembered clock is 1.0 at start and after a reset, so a range render and a
  reused voice are right too.
- Stop, then Play in place: it carries on as if never stopped.

### 3.5 Why no render changed (attack, claim 1)

- Every offline transport plays from its first frame: renders and `Renderer::seek` start with
  `Transport::new(0, true)`, range renders with `Transport::starting(t0)`. Commands land at block lines; only a
  loop wrap splits a block, and both halves move. So `playing` is 1 on every offline frame.
- While playing, `Phase::next` does the same sum as before. Rust never reorders float maths.
- A new node changes node ids and buffer places, not sums. No seed comes from a node's place: `noise(seed)` and
  the synth's `seed` are settings, drum seeds come from `hitcount`, and the scheduler sorts its events.

The proof: through slices 2 and 3 the 10 e2e records and the 5 hashes in `renders.json` stayed the same.

### 3.6 What still differs from the export

| Case | What you hear | Why |
|---|---|---|
| **Space, Enter, Space** (stop, back to bar 1, play) | the wobble and the drums' noise carry on from where they stopped, so this play may differ a little from the export | it is a seek. RENDER-6 allows live to differ after a seek ("seek and loop never reset DSP", the owner's decision of 2026-10-12). L2: the default, written in HANDOFF's open owner decisions for the owner to confirm |
| Stop, then Play in place | the wobble carries on as if never stopped | the chorus's delay lines and the phaser's filters hold the tail, and the bitcrusher a sample of it; only the LFO and the clock wait |
| A seek or a loop wrap | the LFO keeps its phase and speed | after it, live differs from the export, as RENDER-6 allows. Lap 1 from a fresh start matches |
| Stopped | the effects are still: a tail goes through a chorus that no longer moves, a tremolo held at its level | nothing clicks. Live input (8b.10) would hear a still chorus: decide then, by ear |
| Free-running sources at the top level (std's oscillators, `noise`) | they run on while stopped | one that waited would hold a constant value, a DC offset. RENDER-33 still needs a fresh engine for them |
| A smoother that sets itself from its first input (`Gain`, `Duck`, `Glide`) | it does that while stopped, from the input it has then | it matches the export only when that input at frame 0 is the same. Every fixture meets this |

### 3.7 Tests

| Test | Where | Proves |
|---|---|---|
| `time_reads_in_its_own_unit` | `core/ir/src/check.rs` | `playing` is `unit01`, at the top level and in a voice |
| `time_playing_follows_the_transport` | `core/runtime/src/tests/live.rs` | 0 stopped; 1 through a seek and a mid-block wrap; 0 from the stop |
| `time_is_one_node_per_call` | `core/script/src/tests.rs` | one node per call, a voice its own; at the top, `time` and `ctx.time` are one node |
| `a_call_reading_time_added_or_removed_beside_another_renames_nothing` | `core/session/src/tx/delta_tests.rs` | attack A's fix |
| `playing_the_lfo_steps_as_it_always_did`, `playing_the_bitcrusher_crushes_as_it_always_did` | `std/kernels-src/src/fx/tests.rs` | bit for bit the old arithmetic, against oracle copies |
| `stopped_the_effects_wait_and_play_on_where_they_were` | same | each of the four, stopped with `playing` 0 or NaN, then played: the run with no stop |
| `std_plays_the_export_after_the_engine_sat_stopped` | `core/play/tests/bit_identity.rs` | after 2 s and after 5 blocks stopped, live is the export: the drum beat, a held chord through each effect alone (risk 20), and every pure fixture. Red first on each effect |
| `tremolo_holds_while_stopped_and_never_jumps` | `core/play/tests/live.rs` | stopped, the level holds; Play goes on as the export; through loop wraps and a seek, no step bigger than twice the wobble's own |
| `effects_that_follow_the_song_show_no_time_node` | `crates/editor/src/view_tests.rs` | the graph view shows one box per effect call, and no cables from a shared time node |

## 4. std 0.2

The owner chose (O3) that the fix comes with std's version bumped. No render changed, but the new components
and the effects' `playing` input are new API: a song written for them must not build against std 0.1.

| Place | Now |
|---|---|
| `std/m2pkg.toml` `[package] version` | `"0.2.0"` |
| every `music2.toml` the repo ships (9), and every manifest written in code | `std = "0.2"` |
| `core/session/tests/renders.json` `"std"` | `"0.2.0"`; the hashes the same |
| doc examples | README, `core/song`'s doc comments, `architecture.md`, `song-as-program.md` |
| left alone, on purpose | `@m2/core = "1"`, `format = 1`, `IR_VERSION`, the Rust crates' own versions, and the caret-rule cases in `manifest.rs` (they test the rule, not std) |

Added, from the attack and L4:
- `every_shipped_manifest_is_met` (`core/session/src/lib.rs`): the bundled std meets every `music2.toml` under
  `examples`, `templates`, `core/session/tests/fixtures` and `e2e/fixtures`. The next bump needs no grep for
  them. Manifests written in code are still found by `git grep -nE 'std = \\?"0\.1'`.
- `unmet_dependencies` moved to 0.2.4 (met) and 0.3.0 (not met); without that, slice 3 was red (attack B).

The `pkg.unsatisfied` hint (L7, risk 21): only for std, only when the bundled std is newer than the song admits.
It starts with: "This app has std 0.2. To use it, change the std line in music2.toml to std = "0.2". The song
may sound a little different." The hint comes first, as the app shows a message on one line, cut at the
window's edge (the review's D3). It stays true when a later std does change renders. `Requirement::admitting`
names the shortest requirement that admits the app's std.

FMT-3's trigger (L6), the same words everywhere: the lockfile and several bundled std versions wait for "the
first std release, after people have songs, that changes existing renders". std 0.2 changed none, and no one
had songs.

Found while building: **G7f's S could no longer build the app's projects.** S, HEAD `8aa301e`, checks
dependencies with its own std, 0.1.0. After the bump p50 asks for "0.2", so S refused it. S's runner
(`crates/gates/s/main.rs`, `a3214f6`) now builds a copy of each project with the std line set to S's version.
S keeps playing its own std (risk 17); it can't play the app's, whose std/fx reads `time.playing`. p50 uses
none of std's new kernels, so all three sides still play the same music, and G7f checks that.

## 5. The slices, as built

| # | Slice | Commits | Checks |
|---|---|---|---|
| 1 | The merge | `3d4a552` merge, `71195b9`, `2796b13`, `e8bba0e`, `52801b6` | `ci/check.sh --gates` at `e8bba0e` (`check-sm1c`) |
| 2 | O3 | `8b7b7e9` IR and runtime, `02aab27` SDK and delta (fix A), `ed2e9ec` the port, `09b2dfd` std/fx | `--gates` at `09b2dfd` (`check-sm2b`); the recorded tests, 5 of 5 |
| 3 | std 0.2 | `a3214f6` S's runner, `6591ecd` the bump, `b790a63` the hint | `ci/check.sh` at `b790a63` (`check-sm3`); G7f by hand |
| 4 | Docs | this file, the contract, README, HANDOFF and the rest | `ci/check.sh` (`check-sm4`): the "repo unchanged" row; every code name the docs cite exists |
| after | Review, skeptic, fixes, a final `ci/check.sh --gates` | | |

Each `ci/check.sh` passed every row, with nothing passed only on a retry. Each commit that touched
`std/kernels-src` carries its rebuilt `std.wasm`.

## 6. Risks, and how they came out

| # | Risk | Outcome |
|---|---|---|
| 1 | The handoff expected O3 to change `templates/starter`'s render | option 1 changes none; no record moved |
| 2 | Stop, Home, Play doesn't start song-time state again | accepted as a seek (L2); in HANDOFF's open owner decisions and Left for later |
| 3 | Stopped, the effects are still | accepted; decide at 8b.10, by ear |
| 4 | `bitcrush` has the same defect | fixed in slice 2, with its own rule |
| 5 | The speed work changes the engine before the merge | it landed first (`main` at `cbac57c`), and the merge was built on it |
| 6, 7 | The kernels' tests on Linux; clippy | both clean the first time |
| 8 | The larger `std.wasm` costs a page per kernel instance | p50's footprint +8 MB on Linux (195 → 203 MB); the Mac judges |
| 9 | After idle, `examples/sounds` meets parts never tested so | it passed whole: the limiter, compressor, `duck` and `delay` too |
| 10 | The owner's own songs ask for std 0.1 | the hint, and a note in HANDOFF |
| 11 | A direct `kernel("std:chorus", …)` call has no `playing` | its default, 1, keeps it running as before |
| 12, 13 | `time` is new SDK surface; 20 or 22 components | additive; the docs say 22 |
| 14, 18 | Test time; std loads compile a larger `std.wasm` | +70 ms a load; "rust tests" +59 s; the whole check with the gates 26 → 31 min |
| 15 | Gates beside other agents' jobs | the lead scheduled the `--gates` runs; Linux timings are information only (PLAT-1) |
| 16 | G7c on new kinds of graph | passed the first time |
| 17 | G7f's S plays its old std | numbers within one run's noise; S keeps its own std (`a3214f6`) |
| 19 | Disk | this worktree took over step 7's `target/`; a moved `target/` needs `cargo clean` first (slice 1's notes) |
| 20 | One song shows only its first difference | one song per effect; the test lists every song that differs |
| 21 | The hint's wording | as L7 says |
| 22 | `baseline.sh` on a commit without `examples/sounds` | skips it, and says so |

## 7. Left for later

- If the owner wants it: played from bar 1, std's song-time state (LFO phases, hit counts) starts again. It
  changes no export.
- At live input (8b.10): stopped, should the effects run free, and Play put the LFO back where the song left it?
  Decide by ear. It changes no offline render, so it needs no std bump.
- A compiled-module cache for `std.wasm` would win back the slower std loads.
- The Mac run: the 10 records and the rebuilt `std.wasm` on macOS (PLAT-4), G7f with the larger std, and the by-ear
  check in HANDOFF.

## 8. Merge notes for `taste`

From `git merge-tree` of `sounds-merge` and `origin/taste`, besides the conflicts `taste` already has with step 7:
- `docs/gaps.md` and `std/src/instruments.ts` conflict. In `instruments.ts` both sides add an `editor` to the
  five first instruments: keep `taste`'s `category` and `sounds`' `description`.
- `taste`'s "+ Track" list (`instruments` in its `crates/editor/src/tracks.rs`) takes a component only through an
  export whose name has no dot, and groups by `editor.category`. The 22 new instruments are exported only as
  members (`synth.lead`, `drums.kick`) and have no category, so none would be listed. Let the list take a member
  export, and give each a category (the drums `Drums`).
- `taste` adds 2 manifests in code with the old std line (`crates/editor/src/song_tests.rs`): make them
  `std = "0.2"`.
- `core/session/src/tx/delta.rs` conflicts with step 7 already. Keep this merge's `Op::Time` arm.
